use anyhow::Context;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::warn;
use uuid::Uuid;
use wasmtime::Engine;

use crate::{
    a2a::{self, Task},
    app_state::{AppState, ChatIngressEventRecord, ChatIngressStatus, unix_timestamp_ms},
    sandbox,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatIngressStatusReport {
    supported_platforms: Vec<&'static str>,
    telegram_webhook_secret_configured: bool,
    total_events: usize,
    task_created_events: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEventsQuery {
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: Option<i64>,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: Option<i64>,
    text: Option<String>,
    chat: TelegramChat,
    from: Option<TelegramUser>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    first_name: Option<String>,
    last_name: Option<String>,
    username: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/events", get(list_events))
        .route("/telegram/webhook/:secret", post(telegram_webhook))
        .route("/feishu/events", post(feishu_events))
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ChatIngressStatusReport>, (StatusCode, Json<Value>)> {
    let events = state.list_chat_ingress_events(None).await.map_err(internal_error)?;
    let task_created_events = events
        .iter()
        .filter(|event| event.status == ChatIngressStatus::TaskCreated)
        .count();
    Ok(Json(ChatIngressStatusReport {
        supported_platforms: vec!["telegram", "feishu"],
        telegram_webhook_secret_configured: std::env::var("DAWN_TELEGRAM_WEBHOOK_SECRET").is_ok(),
        total_events: events.len(),
        task_created_events,
    }))
}

async fn list_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListEventsQuery>,
) -> Result<Json<Vec<ChatIngressEventRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_chat_ingress_events(query.limit)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn telegram_webhook(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(update): Json<TelegramUpdate>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_telegram_secret(&secret).map_err(bad_request)?;
    let Some(message) = update.message else {
        return Ok(Json(json!({
            "ok": true,
            "ignored": true,
            "reason": "telegram update did not contain a message payload"
        })));
    };

    let text = message.text.unwrap_or_default();
    let sender_display = message
        .from
        .as_ref()
        .and_then(telegram_display_name)
        .or(message.chat.title.clone());
    let record = ingest_message(
        state,
        "telegram",
        format!(
            "telegram.message.{}",
            message.message_id.unwrap_or(update.update_id.unwrap_or_default())
        ),
        Some(message.chat.id.to_string()),
        message.from.as_ref().map(|user| user.id.to_string()),
        sender_display,
        text,
        json!({
            "updateId": update.update_id,
            "messageId": message.message_id,
            "chatId": message.chat.id
        }),
    )
    .await
    .map_err(service_error)?;

    Ok(Json(json!({
        "ok": true,
        "ingressId": record.ingress_id,
        "status": record.status,
        "taskId": record.linked_task_id
    })))
}

async fn feishu_events(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if let Some(challenge) = payload.get("challenge").and_then(Value::as_str) {
        return Ok(Json(json!({ "challenge": challenge })));
    }

    let text = extract_feishu_text(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported feishu event; expected a text message payload"
        ))
    })?;
    let chat_id = payload
        .pointer("/event/message/chat_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let sender_id = payload
        .pointer("/event/sender/sender_id/open_id")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/event/sender/sender_id/user_id")
                .and_then(Value::as_str)
        })
        .map(ToString::to_string);
    let sender_display = payload
        .pointer("/event/sender/sender_id/user_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let event_type = payload
        .pointer("/header/event_type")
        .and_then(Value::as_str)
        .unwrap_or("feishu.event")
        .to_string();

    let record = ingest_message(
        state,
        "feishu",
        event_type,
        chat_id,
        sender_id,
        sender_display,
        text,
        payload,
    )
    .await
    .map_err(service_error)?;

    Ok(Json(json!({
        "ok": true,
        "ingressId": record.ingress_id,
        "status": record.status,
        "taskId": record.linked_task_id
    })))
}

async fn ingest_message(
    state: Arc<AppState>,
    platform: &str,
    event_type: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    text: String,
    raw_payload: Value,
) -> anyhow::Result<ChatIngressEventRecord> {
    let now = unix_timestamp_ms();
    let mut record = ChatIngressEventRecord {
        ingress_id: Uuid::new_v4(),
        platform: platform.to_string(),
        event_type,
        chat_id,
        sender_id,
        sender_display,
        text: text.trim().to_string(),
        raw_payload,
        linked_task_id: None,
        reply_text: None,
        status: ChatIngressStatus::Received,
        error: None,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    state
        .upsert_chat_ingress_event(record.clone())
        .await
        .context("failed to persist received chat ingress event")?;

    if record.text.is_empty() {
        record.status = ChatIngressStatus::Ignored;
        record.error = Some("text message was empty".to_string());
        record.updated_at_unix_ms = unix_timestamp_ms();
        state.upsert_chat_ingress_event(record.clone()).await?;
        return Ok(record);
    }

    let task_name = format!(
        "{} inbound {}",
        platform,
        record
            .sender_display
            .clone()
            .or(record.sender_id.clone())
            .unwrap_or_else(|| "message".to_string())
    );
    let instruction = normalize_ingress_instruction(&record.text);

    match a2a::submit_task(
        state.clone(),
        Task {
            name: task_name,
            task_id: None,
            parent_task_id: None,
            instruction,
        },
    )
    .await
    {
        Ok(task_response) => {
            record.linked_task_id = Some(task_response.task.task_id);
            record.reply_text = Some(format!(
                "Task {} accepted with status {:?}",
                task_response.task.task_id, task_response.task.status
            ));
            record.status = ChatIngressStatus::TaskCreated;
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            Ok(record)
        }
        Err(error) => {
            warn!(?error, platform, "failed to route chat ingress into A2A");
            record.status = ChatIngressStatus::Failed;
            record.error = Some(error.to_string());
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            Ok(record)
        }
    }
}

fn normalize_ingress_instruction(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(value) = trimmed.strip_prefix("/orchestrate ") {
        return format!("orchestrate:{}", value.trim());
    }
    if let Some(value) = trimmed.strip_prefix("/wasm ") {
        return format!("wasm:{}", value.trim());
    }
    if let Some(value) = trimmed.strip_prefix("/task ") {
        return value.trim().to_string();
    }
    trimmed.to_string()
}

fn telegram_display_name(user: &TelegramUser) -> Option<String> {
    if let Some(username) = user.username.as_deref() {
        return Some(username.to_string());
    }
    let joined = [user.first_name.as_deref(), user.last_name.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn extract_feishu_text(payload: &Value) -> Option<String> {
    let message_type = payload
        .pointer("/event/message/message_type")
        .and_then(Value::as_str)?;
    if message_type != "text" {
        return None;
    }
    let raw_content = payload
        .pointer("/event/message/content")
        .and_then(Value::as_str)?;
    let content = serde_json::from_str::<Value>(raw_content).ok()?;
    content
        .get("text")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn verify_telegram_secret(secret: &str) -> anyhow::Result<()> {
    if let Ok(expected) = std::env::var("DAWN_TELEGRAM_WEBHOOK_SECRET") {
        if expected != secret {
            anyhow::bail!("telegram webhook secret mismatch");
        }
    }
    Ok(())
}

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    if message.contains("unsupported") || message.contains("mismatch") || message.contains("empty")
    {
        return bad_request(error);
    }
    internal_error(error)
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use axum::Router;
    use reqwest::Client;

    use super::*;

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-chat-ingress-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn spawn_test_server() -> anyhow::Result<(String, tokio::task::JoinHandle<()>, Arc<AppState>, PathBuf)> {
        let (database_url, db_path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        let app = Router::new()
            .nest("/api/gateway/ingress", router())
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((format!("http://{addr}"), handle, state, db_path))
    }

    #[tokio::test]
    async fn telegram_webhook_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/telegram/webhook/test-secret"
            ))
            .json(&json!({
                "update_id": 1,
                "message": {
                    "message_id": 99,
                    "text": "Book train to Shanghai",
                    "chat": { "id": 12345, "title": "Travel Ops" },
                    "from": { "id": 777, "first_name": "Lin", "last_name": "Wei" }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "telegram");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "Book train to Shanghai");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn feishu_challenge_round_trip_is_supported() -> anyhow::Result<()> {
        let (base_url, handle, _state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/feishu/events"))
            .json(&json!({
                "challenge": "abc123"
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["challenge"], "abc123");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }
}
