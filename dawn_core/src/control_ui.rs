use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::{get, post},
};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use crate::{app_state::AppState, chat_ingress, identity};

const CONTROL_UI_HTML: &str = include_str!("../../templates/frontend/control_ui.html");

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(page))
        .route("/command", post(submit_command))
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchCommandRequest {
    session_token: String,
    platform: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    text: String,
    route_to_task: Option<bool>,
}

async fn submit_command(
    State(state): State<Arc<AppState>>,
    Json(request): Json<WorkbenchCommandRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = identity::resolve_session_by_token(&state, &request.session_token)
        .await
        .map_err(auth_error)?;
    let platform = request.platform.trim().to_ascii_lowercase();
    let text = request.text.trim().to_string();
    if platform.is_empty() {
        return Err(bad_request("platform is required"));
    }
    if text.is_empty() {
        return Err(bad_request("text is required"));
    }
    let record = chat_ingress::simulate_ingress_message(
        state,
        &platform,
        "control_ui.command".to_string(),
        request.chat_id,
        request.sender_id,
        request
            .sender_display
            .or_else(|| Some(session.operator_name.clone())),
        text.clone(),
        json!({
            "source": "control_ui",
            "platform": platform,
            "text": text,
            "actor": session.operator_name,
        }),
        request.route_to_task.unwrap_or(true),
    )
    .await
    .map_err(service_error)?;
    Ok(Json(json!({
        "record": record,
        "actor": session.operator_name,
    })))
}

fn bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": message,
        })),
    )
}

fn auth_error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": error.to_string(),
        })),
    )
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": error.to_string(),
        })),
    )
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
        assert!(CONTROL_UI_HTML.contains("id=\"command-form\""));
        assert!(CONTROL_UI_HTML.contains("/api/gateway/identity/status"));
        assert!(CONTROL_UI_HTML.contains("/api/a2a/task"));
        assert!(CONTROL_UI_HTML.contains("/app/command"));
        assert!(CONTROL_UI_HTML.contains("/app/ws"));
    }
}
