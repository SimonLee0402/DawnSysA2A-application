use std::sync::Arc;

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::get,
};
use serde_json::json;

use crate::app_state::AppState;

const CONTROL_UI_HTML: &str = include_str!("../../templates/frontend/control_ui.html");

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(page))
        .route("/ws", get(workbench_ws))
}

async fn page() -> Html<&'static str> {
    Html(CONTROL_UI_HTML)
}

async fn workbench_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_workbench_ws(socket, state))
}

async fn handle_workbench_ws(mut socket: WebSocket, state: Arc<AppState>) {
    let _ = socket
        .send(Message::Text(
            json!({
                "kind": "ready",
                "transport": "websocket",
                "detail": "gateway workbench websocket connected"
            })
            .to_string()
            .into(),
        ))
        .await;

    let mut receiver = state.subscribe_console_events();

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Text(text))) => {
                        if text.trim().eq_ignore_ascii_case("refresh") {
                            let _ = socket.send(Message::Text(
                                json!({
                                    "kind": "refresh_requested",
                                    "detail": "client requested dashboard refresh"
                                }).to_string().into()
                            )).await;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            event = receiver.recv() => {
                let payload = match event {
                    Ok(event) => json!({
                        "kind": "console_update",
                        "event": event,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => json!({
                        "kind": "lagged",
                        "skipped": skipped,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if socket.send(Message::Text(payload.to_string().into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CONTROL_UI_HTML;

    #[test]
    fn control_ui_markup_contains_expected_sections() {
        assert!(CONTROL_UI_HTML.contains("Dawn Personal Workbench"));
        assert!(CONTROL_UI_HTML.contains("id=\"bootstrap-form\""));
        assert!(CONTROL_UI_HTML.contains("id=\"task-form\""));
        assert!(CONTROL_UI_HTML.contains("/api/gateway/identity/status"));
        assert!(CONTROL_UI_HTML.contains("/api/a2a/task"));
        assert!(CONTROL_UI_HTML.contains("/app/ws"));
    }
}
