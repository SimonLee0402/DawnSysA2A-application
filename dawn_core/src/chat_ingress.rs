use anyhow::Context;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use tokio::time::{Duration, sleep};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    a2a::{self, Task},
    app_state::{
        AppState, ChatAutomationMode, ChatAutomationModeRecord, ChatChannelIdentityRecord,
        ChatChannelIdentityStatus, ChatIngressEventRecord, ChatIngressStatus, NodeCommandStatus,
        unix_timestamp_ms,
    },
    connectors::{self, ChatDispatchRequest, OpenAIResponseRequest},
    control_plane,
    identity,
    skill_registry,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatIngressStatusReport {
    supported_platforms: Vec<&'static str>,
    telegram_webhook_secret_configured: bool,
    telegram_polling_enabled: bool,
    telegram_ingress_mode: &'static str,
    signal_callback_secret_configured: bool,
    signal_dm_policy: &'static str,
    signal_allowlist_count: usize,
    signal_pending_pairings: usize,
    bluebubbles_callback_secret_configured: bool,
    bluebubbles_dm_policy: &'static str,
    bluebubbles_allowlist_count: usize,
    bluebubbles_pending_pairings: usize,
    dingtalk_callback_token_configured: bool,
    wecom_callback_token_configured: bool,
    wechat_official_account_token_configured: bool,
    qq_bot_callback_secret_configured: bool,
    total_events: usize,
    task_created_events: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEventsQuery {
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListPairingsQuery {
    platform: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairingDecisionRequest {
    actor: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
struct IngressMessageSummary {
    text: String,
    route_to_task: bool,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: Option<i64>,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramGetUpdatesResponse {
    ok: bool,
    #[serde(default)]
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, Serialize)]
struct TelegramBotCommand {
    command: &'static str,
    description: &'static str,
}

enum IngressCommandResult {
    Reply(String),
    Task {
        instruction: String,
        task_name: String,
    },
}

enum IngressCommand {
    Help,
    New,
    Skills { query: Option<String> },
    Skill { selector: String },
    Model,
    ModeStatus,
    ModeSet { mode: ChatAutomationMode },
    Status,
    Task(String),
    Orchestrate(String),
    Wasm(String),
    Unknown(String),
}

enum LocalActionIntent {
    BrowserOpen { target: String },
    DesktopNotification { message: String },
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeComVerifyQuery {
    echostr: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeChatOfficialAccountVerifyQuery {
    signature: Option<String>,
    timestamp: Option<String>,
    nonce: Option<String>,
    echostr: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/events", get(list_events))
        .route("/pairings", get(list_pairings))
        .route(
            "/pairings/:platform/:identity_key/approve",
            post(approve_pairing),
        )
        .route(
            "/pairings/:platform/:identity_key/reject",
            post(reject_pairing),
        )
        .route("/telegram/webhook/:secret", post(telegram_webhook))
        .route("/signal/events/:secret", post(signal_events))
        .route("/bluebubbles/events/:secret", post(bluebubbles_events))
        .route("/feishu/events", post(feishu_events))
        .route("/dingtalk/events", post(dingtalk_events))
        .route("/wecom/events", get(wecom_verify).post(wecom_events))
        .route(
            "/wechat-official-account/events",
            get(wechat_official_account_verify).post(wechat_official_account_events),
        )
        .route("/qq/events", post(qq_events))
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ChatIngressStatusReport>, (StatusCode, Json<Value>)> {
    let events = state
        .list_chat_ingress_events(None)
        .await
        .map_err(internal_error)?;
    let signal_policy = chat_dm_policy_for_platform("signal");
    let bluebubbles_policy = chat_dm_policy_for_platform("bluebubbles");
    let signal_allowlist = allowlist_values_for_platform("signal");
    let bluebubbles_allowlist = allowlist_values_for_platform("bluebubbles");
    let signal_pending_pairings = state
        .list_chat_channel_identities(Some("signal"), Some(ChatChannelIdentityStatus::Pending))
        .await
        .map_err(internal_error)?
        .len();
    let bluebubbles_pending_pairings = state
        .list_chat_channel_identities(
            Some("bluebubbles"),
            Some(ChatChannelIdentityStatus::Pending),
        )
        .await
        .map_err(internal_error)?
        .len();
    let task_created_events = events
        .iter()
        .filter(|event| event.status == ChatIngressStatus::TaskCreated)
        .count();
    Ok(Json(ChatIngressStatusReport {
        supported_platforms: vec![
            "telegram",
            "signal",
            "bluebubbles",
            "feishu",
            "dingtalk",
            "wecom",
            "wechat_official_account",
            "qq",
        ],
        telegram_webhook_secret_configured: std::env::var("DAWN_TELEGRAM_WEBHOOK_SECRET").is_ok(),
        telegram_polling_enabled: telegram_polling_enabled(),
        telegram_ingress_mode: telegram_ingress_mode(),
        signal_callback_secret_configured: std::env::var("DAWN_SIGNAL_CALLBACK_SECRET").is_ok(),
        signal_dm_policy: chat_dm_policy_label(signal_policy),
        signal_allowlist_count: signal_allowlist.len(),
        signal_pending_pairings,
        bluebubbles_callback_secret_configured: std::env::var("DAWN_BLUEBUBBLES_CALLBACK_SECRET")
            .is_ok(),
        bluebubbles_dm_policy: chat_dm_policy_label(bluebubbles_policy),
        bluebubbles_allowlist_count: bluebubbles_allowlist.len(),
        bluebubbles_pending_pairings,
        dingtalk_callback_token_configured: std::env::var("DAWN_DINGTALK_CALLBACK_TOKEN").is_ok(),
        wecom_callback_token_configured: std::env::var("DAWN_WECOM_CALLBACK_TOKEN").is_ok(),
        wechat_official_account_token_configured: std::env::var(
            "DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN",
        )
        .is_ok(),
        qq_bot_callback_secret_configured: std::env::var("DAWN_QQ_BOT_CALLBACK_SECRET").is_ok(),
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

async fn list_pairings(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListPairingsQuery>,
) -> Result<Json<Vec<ChatChannelIdentityRecord>>, (StatusCode, Json<Value>)> {
    let platform = query
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let status = parse_pairing_status(query.status.as_deref())?;
    state
        .list_chat_channel_identities(platform, status)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn approve_pairing(
    State(state): State<Arc<AppState>>,
    Path((platform, identity_key)): Path<(String, String)>,
    Json(request): Json<PairingDecisionRequest>,
) -> Result<Json<ChatChannelIdentityRecord>, (StatusCode, Json<Value>)> {
    resolve_pairing_decision(state, &platform, &identity_key, true, request)
        .await
        .map(Json)
}

async fn reject_pairing(
    State(state): State<Arc<AppState>>,
    Path((platform, identity_key)): Path<(String, String)>,
    Json(request): Json<PairingDecisionRequest>,
) -> Result<Json<ChatChannelIdentityRecord>, (StatusCode, Json<Value>)> {
    resolve_pairing_decision(state, &platform, &identity_key, false, request)
        .await
        .map(Json)
}

async fn telegram_webhook(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(update): Json<TelegramUpdate>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_telegram_secret(&secret).map_err(bad_request)?;
    let Some(record) = process_telegram_update(state, update)
        .await
        .map_err(service_error)?
    else {
        return Ok(Json(json!({
            "ok": true,
            "ignored": true,
            "reason": "telegram update did not contain a message payload"
        })));
    };

    Ok(Json(json!({
        "ok": true,
        "ingressId": record.ingress_id,
        "status": record.status,
        "taskId": record.linked_task_id
    })))
}

async fn process_telegram_update(
    state: Arc<AppState>,
    update: TelegramUpdate,
) -> anyhow::Result<Option<ChatIngressEventRecord>> {
    let Some(message) = update.message else {
        return Ok(None);
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
            message
                .message_id
                .unwrap_or(update.update_id.unwrap_or_default())
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
        true,
    )
    .await?;

    Ok(Some(record))
}

async fn signal_events(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_callback_secret("DAWN_SIGNAL_CALLBACK_SECRET", "signal", &secret)
        .map_err(bad_request)?;

    let summary = summarize_signal_event(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported signal event; expected a text, attachment, reaction, receipt, or typing payload"
        ))
    })?;
    let chat_id = payload
        .pointer("/envelope/source")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/source").and_then(Value::as_str))
        .or_else(|| payload.pointer("/data/source").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_id = chat_id.clone();
    let sender_display = payload
        .pointer("/envelope/sourceName")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(sender_id.clone());
    let event_type = payload
        .pointer("/envelope/type")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/type").and_then(Value::as_str))
        .unwrap_or("signal.event")
        .to_string();

    let record = ingest_message(
        state,
        "signal",
        event_type,
        chat_id,
        sender_id,
        sender_display,
        summary.text,
        payload,
        summary.route_to_task,
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

async fn bluebubbles_events(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_callback_secret("DAWN_BLUEBUBBLES_CALLBACK_SECRET", "bluebubbles", &secret)
        .map_err(bad_request)?;

    let summary = summarize_bluebubbles_event(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported bluebubbles event; expected a text, attachment, reaction, receipt, or typing payload"
        ))
    })?;
    let chat_id = payload
        .pointer("/chatGuid")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/message/chatGuid").and_then(Value::as_str))
        .or_else(|| payload.pointer("/data/chatGuid").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_id = payload
        .pointer("/handle/address")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/message/handle/address")
                .and_then(Value::as_str)
        })
        .or_else(|| payload.pointer("/sender/address").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_display = payload
        .pointer("/handle/displayName")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/message/handle/displayName")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            payload
                .pointer("/sender/displayName")
                .and_then(Value::as_str)
        })
        .map(ToString::to_string)
        .or(sender_id.clone());
    let event_type = payload
        .pointer("/event")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/type").and_then(Value::as_str))
        .unwrap_or("bluebubbles.event")
        .to_string();

    let record = ingest_message(
        state,
        "bluebubbles",
        event_type,
        chat_id,
        sender_id,
        sender_display,
        summary.text,
        payload,
        summary.route_to_task,
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
        true,
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

async fn dingtalk_events(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_dingtalk_callback_token(&payload).map_err(bad_request)?;

    if let Some(challenge) = payload.get("challenge").and_then(Value::as_str) {
        return Ok(Json(json!({ "challenge": challenge })));
    }

    let text = extract_dingtalk_text(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported dingtalk event; expected a text message payload"
        ))
    })?;
    let chat_id = payload
        .pointer("/conversationId")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/conversation_id").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_id = payload
        .pointer("/senderStaffId")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/senderId").and_then(Value::as_str))
        .or_else(|| payload.pointer("/staffId").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_display = payload
        .pointer("/senderNick")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/senderNickname").and_then(Value::as_str))
        .map(ToString::to_string)
        .or(sender_id.clone());
    let event_type = payload
        .pointer("/EventType")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/eventType").and_then(Value::as_str))
        .or_else(|| payload.pointer("/msgtype").and_then(Value::as_str))
        .unwrap_or("dingtalk.event")
        .to_string();

    let record = ingest_message(
        state,
        "dingtalk",
        event_type,
        chat_id,
        sender_id,
        sender_display,
        text,
        payload,
        true,
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

async fn wecom_verify(
    Query(query): Query<WeComVerifyQuery>,
) -> Result<String, (StatusCode, Json<Value>)> {
    query
        .echostr
        .ok_or_else(|| bad_request(anyhow::anyhow!("missing echostr query parameter")))
}

async fn wecom_events(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_wecom_callback_token(&payload).map_err(bad_request)?;

    let text = extract_wecom_text(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported wecom event; expected a text message payload"
        ))
    })?;
    let chat_id = payload
        .pointer("/chatid")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/conversationId").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_id = payload
        .pointer("/from")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/userid").and_then(Value::as_str))
        .or_else(|| payload.pointer("/sender").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_display = payload
        .pointer("/sender_name")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(sender_id.clone());
    let event_type = payload
        .pointer("/Event")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/event").and_then(Value::as_str))
        .or_else(|| payload.pointer("/msgtype").and_then(Value::as_str))
        .unwrap_or("wecom.event")
        .to_string();

    let record = ingest_message(
        state,
        "wecom",
        event_type,
        chat_id,
        sender_id,
        sender_display,
        text,
        payload,
        true,
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

async fn wechat_official_account_verify(
    Query(query): Query<WeChatOfficialAccountVerifyQuery>,
) -> Result<String, (StatusCode, String)> {
    verify_wechat_official_account_query(&query).map_err(plain_bad_request)?;
    query
        .echostr
        .ok_or_else(|| plain_bad_request(anyhow::anyhow!("missing echostr query parameter")))
}

async fn wechat_official_account_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WeChatOfficialAccountVerifyQuery>,
    body: String,
) -> Result<String, (StatusCode, String)> {
    verify_wechat_official_account_query(&query).map_err(plain_bad_request)?;
    let payload = parse_wechat_official_account_xml(&body)
        .ok_or_else(|| plain_bad_request(anyhow::anyhow!("unsupported wechat xml payload")))?;
    let event_type = payload
        .event_type
        .clone()
        .or_else(|| payload.msg_type.clone())
        .unwrap_or_else(|| "wechat.event".to_string());
    let text = payload.text.clone().ok_or_else(|| {
        plain_bad_request(anyhow::anyhow!(
            "unsupported wechat event; expected a text message payload"
        ))
    })?;

    ingest_message(
        state,
        "wechat_official_account",
        event_type,
        payload.chat_id.clone().or(payload.sender_id.clone()),
        payload.sender_id.clone(),
        payload.sender_display.clone(),
        text,
        json!({
            "toUserName": payload.to_user_name,
            "fromUserName": payload.from_user_name,
            "msgType": payload.msg_type,
            "msgId": payload.msg_id,
            "createTime": payload.create_time,
            "event": payload.event_type,
            "rawXml": body
        }),
        true,
    )
    .await
    .map_err(plain_service_error)?;

    Ok("success".to_string())
}

async fn qq_events(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(text) = extract_qq_text(&payload) else {
        if let Some(plain_token) = payload.pointer("/d/plain_token").and_then(Value::as_str) {
            return Ok(Json(json!({
                "plain_token": plain_token,
                "note": "qq callback challenge echoed; signature flow is not yet enforced by the gateway"
            })));
        }

        return Err(bad_request(anyhow::anyhow!(
            "unsupported qq event; expected a text message payload"
        )));
    };

    let chat_id = payload
        .pointer("/d/group_openid")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/d/group_id").and_then(Value::as_str))
        .or_else(|| payload.pointer("/d/channel_id").and_then(Value::as_str))
        .or_else(|| payload.pointer("/d/author/id").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_id = payload
        .pointer("/d/author/id")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/d/member_openid").and_then(Value::as_str))
        .map(ToString::to_string);
    let sender_display = payload
        .pointer("/d/author/username")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/d/author/nick").and_then(Value::as_str))
        .map(ToString::to_string)
        .or(sender_id.clone());
    let event_type = payload
        .pointer("/t")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/eventType").and_then(Value::as_str))
        .unwrap_or("qq.event")
        .to_string();

    let record = ingest_message(
        state,
        "qq",
        event_type,
        chat_id,
        sender_id,
        sender_display,
        text,
        payload,
        true,
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

pub(crate) async fn simulate_ingress_message(
    state: Arc<AppState>,
    platform: &str,
    event_type: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    text: String,
    raw_payload: Value,
    route_to_task: bool,
) -> anyhow::Result<ChatIngressEventRecord> {
    ingest_message(
        state,
        platform,
        event_type,
        chat_id,
        sender_id,
        sender_display,
        text,
        raw_payload,
        route_to_task,
    )
    .await
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
    route_to_task: bool,
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

    if !route_to_task {
        record.status = ChatIngressStatus::Ignored;
        record.error = Some("event recorded without task routing".to_string());
        record.updated_at_unix_ms = unix_timestamp_ms();
        state.upsert_chat_ingress_event(record.clone()).await?;
        return Ok(record);
    }

    match evaluate_ingress_access(
        state.clone(),
        platform,
        record.chat_id.as_deref(),
        record.sender_id.as_deref(),
        record.sender_display.as_deref(),
        record.ingress_id,
    )
    .await?
    {
        IngressAccessDecision::Allow => {}
        IngressAccessDecision::PendingPairing(identity) => {
            let pairing_code = identity
                .pairing_code
                .clone()
                .unwrap_or_else(|| "pending".to_string());
            let actor = identity
                .sender_display
                .clone()
                .or(identity.sender_id.clone())
                .unwrap_or_else(|| identity.identity_key.clone());
            let reply = format!(
                "Pairing required for {platform}. Ask the operator to approve code {pairing_code} for {actor}."
            );
            if let Err(error) =
                dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                    .await
            {
                warn!(?error, platform, "failed to deliver pending-pairing ingress reply");
                record.error = Some(format!(
                    "{platform} sender is waiting for pairing approval ({pairing_code}); reply dispatch failed: {error}"
                ));
            }
            record.reply_text = Some(reply);
            record.status = ChatIngressStatus::PendingApproval;
            if record.error.is_none() {
                record.error = Some(format!(
                    "{platform} sender is waiting for pairing approval ({pairing_code})"
                ));
            }
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            return Ok(record);
        }
        IngressAccessDecision::Rejected(message) => {
            if let Err(error) =
                dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &message)
                    .await
            {
                warn!(?error, platform, "failed to deliver rejected ingress reply");
                record.error = Some(format!("{message}; reply dispatch failed: {error}"));
            }
            record.reply_text = Some(message.clone());
            record.status = ChatIngressStatus::Ignored;
            if record.error.is_none() {
                record.error = Some(message);
            }
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            return Ok(record);
        }
    }

    let current_mode = current_chat_automation_mode(
        state.clone(),
        platform,
        record.chat_id.as_deref(),
        record.sender_id.as_deref(),
    )
    .await?;

    let normalized_command_text = normalize_ingress_command_text(platform, &record.text);
    let command_task = if let Some(command) = parse_ingress_command(&normalized_command_text) {
        match execute_ingress_command(state.clone(), platform, &record, command, current_mode)
            .await?
        {
            IngressCommandResult::Reply(reply) => {
                if let Err(error) =
                    dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                        .await
                {
                    warn!(?error, platform, "failed to deliver ingress command reply");
                    record.reply_text = Some(reply);
                    record.status = ChatIngressStatus::Failed;
                    record.error = Some(format!("failed to dispatch command reply: {error}"));
                    record.updated_at_unix_ms = unix_timestamp_ms();
                    state.upsert_chat_ingress_event(record.clone()).await?;
                    return Ok(record);
                }
                record.reply_text = Some(reply);
                record.status = ChatIngressStatus::Replied;
                record.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_chat_ingress_event(record.clone()).await?;
                return Ok(record);
            }
            IngressCommandResult::Task {
                instruction,
                task_name,
            } => Some((instruction, task_name)),
        }
    } else {
        None
    };

    if command_task.is_none() {
        if let Some(reply) =
            try_mode_aware_reply(state.clone(), platform, &record, current_mode).await?
        {
            if let Err(error) =
                dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                    .await
            {
                warn!(?error, platform, "failed to deliver mode-aware ingress reply");
                record.reply_text = Some(reply);
                record.status = ChatIngressStatus::Failed;
                record.error = Some(format!("failed to dispatch mode-aware reply: {error}"));
                record.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_chat_ingress_event(record.clone()).await?;
                return Ok(record);
            }
            record.reply_text = Some(reply);
            record.status = ChatIngressStatus::Replied;
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            return Ok(record);
        }
    }

    if command_task.is_none() && should_attempt_default_model_reply(&record.text) {
        match try_default_model_reply(state.clone(), platform, &record.text).await {
            Ok(Some(reply)) => {
                if let Err(error) =
                    dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                        .await
                {
                    warn!(?error, platform, "failed to deliver default model reply");
                    record.reply_text = Some(reply);
                    record.status = ChatIngressStatus::Failed;
                    record.error =
                        Some(format!("default model reply generated but dispatch failed: {error}"));
                    record.updated_at_unix_ms = unix_timestamp_ms();
                    state.upsert_chat_ingress_event(record.clone()).await?;
                    return Ok(record);
                }
                record.reply_text = Some(reply);
                record.status = ChatIngressStatus::Replied;
                record.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_chat_ingress_event(record.clone()).await?;
                return Ok(record);
            }
            Ok(None) => {}
            Err(error) => {
                warn!(?error, platform, "default model reply failed for chat ingress");
                let reply = format!("Model reply failed: {error}");
                if let Err(dispatch_error) =
                    dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                        .await
                {
                    warn!(
                        ?dispatch_error,
                        platform,
                        "failed to deliver default model failure reply"
                    );
                }
                record.reply_text = Some(reply);
                record.status = ChatIngressStatus::Failed;
                record.error = Some(error.to_string());
                record.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_chat_ingress_event(record.clone()).await?;
                return Ok(record);
            }
        }
    }

    let (instruction, task_name) = command_task.unwrap_or_else(|| {
        (
            normalize_ingress_instruction(&record.text),
            format!(
                "{} inbound {}",
                platform,
                record
                    .sender_display
                    .clone()
                    .or(record.sender_id.clone())
                    .unwrap_or_else(|| "message".to_string())
            ),
        )
    });

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
            let reply = record.reply_text.clone().unwrap_or_default();
            if let Err(error) =
                dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                    .await
            {
                warn!(?error, platform, "failed to deliver task-created ingress reply");
                record.error = Some(format!(
                    "task {} created, but reply dispatch failed: {error}",
                    task_response.task.task_id
                ));
            }
            record.status = ChatIngressStatus::TaskCreated;
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            Ok(record)
        }
        Err(error) => {
            warn!(?error, platform, "failed to route chat ingress into A2A");
            let reply = format!("Failed to route your message: {error}");
            if let Err(dispatch_error) =
                dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                    .await
            {
                warn!(
                    ?dispatch_error,
                    platform,
                    "failed to deliver ingress routing failure reply"
                );
            }
            record.reply_text = Some(reply);
            record.status = ChatIngressStatus::Failed;
            record.error = Some(error.to_string());
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            Ok(record)
        }
    }
}

enum IngressAccessDecision {
    Allow,
    PendingPairing(ChatChannelIdentityRecord),
    Rejected(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChatDmPolicy {
    Open,
    Allowlist,
    Pairing,
    Disabled,
}

async fn evaluate_ingress_access(
    state: Arc<AppState>,
    platform: &str,
    chat_id: Option<&str>,
    sender_id: Option<&str>,
    sender_display: Option<&str>,
    ingress_id: Uuid,
) -> anyhow::Result<IngressAccessDecision> {
    if !matches!(platform, "signal" | "bluebubbles") {
        return Ok(IngressAccessDecision::Allow);
    }

    let policy = chat_dm_policy_for_platform(platform);
    let identity_key = build_chat_identity_key(platform, chat_id, sender_id)?;
    if allowlist_contains(platform, sender_id, chat_id) {
        return Ok(IngressAccessDecision::Allow);
    }
    if let Some(identity) = state
        .get_chat_channel_identity(platform, &identity_key)
        .await?
    {
        return match identity.status {
            ChatChannelIdentityStatus::Paired => Ok(IngressAccessDecision::Allow),
            ChatChannelIdentityStatus::Pending if policy == ChatDmPolicy::Pairing => {
                Ok(IngressAccessDecision::PendingPairing(identity))
            }
            ChatChannelIdentityStatus::Rejected | ChatChannelIdentityStatus::Blocked => {
                Ok(IngressAccessDecision::Rejected(format!(
                    "{platform} sender {} is not approved for inbound automation.",
                    identity
                        .sender_display
                        .clone()
                        .or(identity.sender_id.clone())
                        .unwrap_or_else(|| identity.identity_key.clone())
                )))
            }
            _ => Ok(IngressAccessDecision::Allow),
        };
    }

    match policy {
        ChatDmPolicy::Open => Ok(IngressAccessDecision::Allow),
        ChatDmPolicy::Allowlist => Ok(IngressAccessDecision::Rejected(format!(
            "{platform} sender is not allowlisted for inbound automation."
        ))),
        ChatDmPolicy::Disabled => Ok(IngressAccessDecision::Rejected(format!(
            "{platform} inbound automation is disabled."
        ))),
        ChatDmPolicy::Pairing => {
            let now = unix_timestamp_ms();
            let identity = state
                .upsert_chat_channel_identity(ChatChannelIdentityRecord {
                    platform: platform.to_string(),
                    identity_key: identity_key.clone(),
                    chat_id: chat_id.map(ToString::to_string),
                    sender_id: sender_id.map(ToString::to_string),
                    sender_display: sender_display.map(ToString::to_string),
                    pairing_code: Some(generate_pairing_code()),
                    dm_policy: chat_dm_policy_label(policy).to_string(),
                    decision_reason: None,
                    last_ingress_id: Some(ingress_id),
                    status: ChatChannelIdentityStatus::Pending,
                    created_at_unix_ms: now,
                    updated_at_unix_ms: now,
                })
                .await?;
            Ok(IngressAccessDecision::PendingPairing(identity))
        }
    }
}

async fn dispatch_ingress_reply_if_possible(
    platform: &str,
    chat_id: Option<&str>,
    text: &str,
) -> anyhow::Result<()> {
    if matches!(platform, "app" | "control_ui" | "local") {
        return Ok(());
    }
    let Some(chat_id) = chat_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let request = ChatDispatchRequest {
        platform: platform.to_string(),
        text: Some(text.to_string()),
        chat_id: Some(chat_id.to_string()),
        account_key: None,
        attachment_name: None,
        attachment_base64: None,
        attachment_content_type: None,
        reaction: None,
        target_message_id: None,
        target_author: None,
        remove_reaction: None,
        receipt_type: None,
        typing: None,
        mark_read: None,
        mark_unread: None,
        part_index: None,
        effect_id: None,
        edit_message_id: None,
        edited_text: None,
        unsend_message_id: None,
        participant_action: None,
        participant_address: None,
        group_action: None,
        group_id: None,
        group_name: None,
        group_description: None,
        group_link_mode: None,
        group_members: None,
        group_admins: None,
        parse_mode: None,
        disable_notification: Some(false),
        target_type: None,
        event_id: None,
        msg_id: None,
        msg_seq: None,
        is_wakeup: None,
    };
    connectors::execute_chat_connector(request)
        .await
        .map(|_| ())
        .map_err(|error| {
            warn!(?error, platform, "failed to dispatch ingress reply");
            error
        })
}

fn build_chat_identity_key(
    platform: &str,
    chat_id: Option<&str>,
    sender_id: Option<&str>,
) -> anyhow::Result<String> {
    let value = sender_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| chat_id.map(str::trim).filter(|value| !value.is_empty()))
        .ok_or_else(|| anyhow::anyhow!("missing sender identity for {platform} ingress"))?;
    Ok(value.to_string())
}

fn generate_pairing_code() -> String {
    let hex = Uuid::new_v4().simple().to_string();
    hex.chars().take(6).collect::<String>().to_ascii_uppercase()
}

fn chat_dm_policy_for_platform(platform: &str) -> ChatDmPolicy {
    let env_var = match platform {
        "signal" => "DAWN_SIGNAL_DM_POLICY",
        "bluebubbles" => "DAWN_BLUEBUBBLES_DM_POLICY",
        _ => return ChatDmPolicy::Open,
    };
    match std::env::var(env_var)
        .unwrap_or_else(|_| "open".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "allowlist" | "allow_list" => ChatDmPolicy::Allowlist,
        "pairing" | "pair" => ChatDmPolicy::Pairing,
        "disabled" | "off" => ChatDmPolicy::Disabled,
        _ => ChatDmPolicy::Open,
    }
}

fn chat_dm_policy_label(policy: ChatDmPolicy) -> &'static str {
    match policy {
        ChatDmPolicy::Open => "open",
        ChatDmPolicy::Allowlist => "allowlist",
        ChatDmPolicy::Pairing => "pairing",
        ChatDmPolicy::Disabled => "disabled",
    }
}

fn allowlist_contains(platform: &str, sender_id: Option<&str>, chat_id: Option<&str>) -> bool {
    let values = allowlist_values_for_platform(platform);
    if values.is_empty() {
        return false;
    }
    sender_id
        .map(str::trim)
        .filter(|value| {
            values
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(value))
        })
        .is_some()
        || chat_id
            .map(str::trim)
            .filter(|value| {
                values
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(value))
            })
            .is_some()
}

fn allowlist_values_for_platform(platform: &str) -> Vec<String> {
    let env_keys: &[&str] = match platform {
        "signal" => &["DAWN_SIGNAL_ALLOW_FROM", "DAWN_SIGNAL_ALLOWLIST"],
        "bluebubbles" => &["DAWN_BLUEBUBBLES_ALLOW_FROM", "DAWN_BLUEBUBBLES_ALLOWLIST"],
        _ => &[],
    };
    env_keys
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
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

fn normalize_ingress_command_text(platform: &str, text: &str) -> String {
    let mut normalized = normalize_fullwidth_command_prefix(text.trim());
    let mut stripped_prefix = false;
    for _ in 0..4 {
        let trimmed = normalized.trim_start();
        if let Some(rest) = strip_leading_chat_command_prefix(platform, trimmed) {
            stripped_prefix = true;
            normalized = normalize_fullwidth_command_prefix(rest.trim_start());
            continue;
        }
        break;
    }
    if stripped_prefix && normalized.trim().is_empty() {
        return "/help".to_string();
    }
    let trimmed = normalized.trim();
    if !trimmed.starts_with('/') && !trimmed.starts_with('#') {
        if let Some(alias) = normalize_platform_command_alias(platform, trimmed) {
            return alias;
        }
    }
    normalized.trim().to_string()
}

fn normalize_fullwidth_command_prefix(text: &str) -> String {
    if let Some(rest) = text.strip_prefix('／') {
        return format!("/{rest}");
    }
    if let Some(rest) = text.strip_prefix('＃') {
        return format!("#{rest}");
    }
    text.to_string()
}

fn strip_leading_chat_command_prefix<'a>(platform: &str, text: &'a str) -> Option<&'a str> {
    let trimmed = text.trim_start();
    if let Some(rest) = strip_leading_qq_mention(trimmed) {
        return Some(rest);
    }
    if let Some(rest) = strip_leading_at_mention(trimmed) {
        return Some(rest);
    }
    if matches!(platform, "feishu" | "dingtalk" | "wechat_official_account" | "qq") {
        if let Some(rest) = strip_leading_tag_mention(trimmed) {
            return Some(rest);
        }
    }
    None
}

fn strip_leading_at_mention(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    let first = trimmed.chars().next()?;
    if first != '@' && first != '＠' {
        return None;
    }
    if let Some(boundary) = trimmed.find(char::is_whitespace) {
        return Some(&trimmed[boundary..]);
    }
    Some("")
}

fn strip_leading_tag_mention(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let close_tag = "</at>";
    if !lower.starts_with("<at") {
        return None;
    }
    let close_index = lower.find(close_tag)?;
    Some(&trimmed[close_index + close_tag.len()..])
}

fn normalize_platform_command_alias(platform: &str, text: &str) -> Option<String> {
    if !matches!(
        platform,
        "feishu" | "dingtalk" | "wechat_official_account" | "qq" | "wecom"
    ) {
        return None;
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let exact = match lowered.as_str() {
        "help" | "commands" | "start" => Some("/help".to_string()),
        "status" => Some("/status".to_string()),
        "model" | "models" => Some("/model".to_string()),
        "skills" => Some("/skills".to_string()),
        "new" => Some("/new".to_string()),
        "mode" => Some("#mode".to_string()),
        _ => None,
    };
    if exact.is_some() {
        return exact;
    }
    match trimmed {
        "帮助" | "命令" | "菜单" | "开始" => Some("/help".to_string()),
        "状态" => Some("/status".to_string()),
        "模型" => Some("/model".to_string()),
        "技能" | "技能列表" => Some("/skills".to_string()),
        "新建" | "新对话" => Some("/new".to_string()),
        "模式" | "功能等级" => Some("#mode".to_string()),
        "聊天模式" => Some("#chat".to_string()),
        "观察模式" | "观察" => Some("#observe".to_string()),
        "辅助模式" | "辅助" => Some("#assist".to_string()),
        "自动驾驶" | "自动模式" => Some("#autopilot".to_string()),
        _ => {
            if let Some(rest) = trimmed
                .strip_prefix("技能搜索 ")
                .or_else(|| trimmed.strip_prefix("技能 搜索 "))
                .or_else(|| trimmed.strip_prefix("搜索技能 "))
            {
                let query = rest.trim();
                if !query.is_empty() {
                    return Some(format!("/skills search {query}"));
                }
            }
            if let Some(rest) = trimmed
                .strip_prefix("使用技能 ")
                .or_else(|| trimmed.strip_prefix("调用技能 "))
            {
                let selector = rest.trim();
                if !selector.is_empty() {
                    return Some(format!("/skill {selector}"));
                }
            }
            None
        }
    }
}

fn parse_ingress_command(text: &str) -> Option<IngressCommand> {
    let canonical = normalize_fullwidth_command_prefix(text.trim());
    let trimmed = canonical.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('#') {
        return parse_mode_command(trimmed);
    }
    if !trimmed.starts_with('/') {
        return None;
    }
    if trimmed == "/" {
        return Some(IngressCommand::Help);
    }
    let body = trimmed.trim_start_matches('/').trim();
    if body.is_empty() {
        return Some(IngressCommand::Help);
    }
    let (command, remainder) = match body.split_once(char::is_whitespace) {
        Some((command, remainder)) => (command, remainder.trim()),
        None => (body, ""),
    };
    let remainder = remainder.trim();
    let command = command
        .split_once('@')
        .map(|(base, _)| base)
        .unwrap_or(command);
    match command.to_ascii_lowercase().as_str() {
        "help" | "start" | "commands" => Some(IngressCommand::Help),
        "new" => Some(IngressCommand::New),
        "skills" => Some(IngressCommand::Skills {
            query: parse_skills_query(remainder),
        }),
        "skill" | "use" => Some(IngressCommand::Skill {
            selector: remainder.to_string(),
        }),
        "mode" => Some(IngressCommand::ModeStatus),
        "model" | "models" => Some(IngressCommand::Model),
        "status" => Some(IngressCommand::Status),
        "task" => Some(IngressCommand::Task(remainder.to_string())),
        "orchestrate" => Some(IngressCommand::Orchestrate(remainder.to_string())),
        "wasm" => Some(IngressCommand::Wasm(remainder.to_string())),
        other => Some(IngressCommand::Unknown(other.to_string())),
    }
}

fn parse_mode_command(text: &str) -> Option<IngressCommand> {
    let canonical = normalize_fullwidth_command_prefix(text.trim());
    let trimmed = canonical.trim();
    if trimmed.is_empty() || !trimmed.starts_with('#') {
        return None;
    }
    let body = trimmed.trim_start_matches('#').trim();
    if body.is_empty() {
        return Some(IngressCommand::Help);
    }
    let (command, _remainder) = match body.split_once(char::is_whitespace) {
        Some((command, remainder)) => (command, remainder.trim()),
        None => (body, ""),
    };
    match command.to_ascii_lowercase().as_str() {
        "help" | "commands" => Some(IngressCommand::Help),
        "mode" | "status" => Some(IngressCommand::ModeStatus),
        "chat" => Some(IngressCommand::ModeSet {
            mode: ChatAutomationMode::Chat,
        }),
        "observe" => Some(IngressCommand::ModeSet {
            mode: ChatAutomationMode::Observe,
        }),
        "assist" => Some(IngressCommand::ModeSet {
            mode: ChatAutomationMode::Assist,
        }),
        "autopilot" | "auto" => Some(IngressCommand::ModeSet {
            mode: ChatAutomationMode::Autopilot,
        }),
        other => Some(IngressCommand::Unknown(format!("#{other}"))),
    }
}

async fn execute_ingress_command(
    state: Arc<AppState>,
    platform: &str,
    record: &ChatIngressEventRecord,
    command: IngressCommand,
    current_mode: ChatAutomationMode,
) -> anyhow::Result<IngressCommandResult> {
    let help_text = help_command_text_for_platform(platform);
    match command {
        IngressCommand::Help => Ok(IngressCommandResult::Reply(help_text.clone())),
        IngressCommand::New => Ok(IngressCommandResult::Reply(
            "新的对话已准备好。直接发问题即可，或输入 /skills 查看已安装技能。".to_string(),
        )),
        IngressCommand::Skills { query } => {
            let reply = render_skills_command(&state, query.as_deref()).await?;
            Ok(IngressCommandResult::Reply(reply))
        }
        IngressCommand::Skill { selector } => {
            let parsed = parse_skill_selector(&selector)?;
            let Some(skill) =
                skill_registry::find_skill(&state, &parsed.skill_id, parsed.version.as_deref())
                    .await?
            else {
                return Ok(IngressCommandResult::Reply(format!(
                    "没有找到技能 `{}`。先输入 /skills 查看可用技能。",
                    parsed.skill_id
                )));
            };
            if skill_registry::is_native_builtin_skill(&skill) {
                let reply = skill_registry::native_builtin_skill_usage(&skill.skill_id)
                    .unwrap_or_else(|| {
                        format!(
                            "技能 `{}` 是 Dawn 的原生内置技能，当前本机已经可用。",
                            skill.display_name
                        )
                    });
                return Ok(IngressCommandResult::Reply(reply));
            }
            let selector = build_skill_selector_for_task(&skill, parsed.function_name.as_deref());
            let mut task_name = format!("{platform} skill {}", skill.display_name);
            if let Some(chat_id) = record.chat_id.as_deref() {
                task_name.push_str(&format!(" ({chat_id})"));
            }
            Ok(IngressCommandResult::Task {
                instruction: format!("wasm:{selector}"),
                task_name,
            })
        }
        IngressCommand::Model => {
            let workspace = identity::ensure_workspace_profile(&state).await?;
            let live = workspace
                .default_model_providers
                .iter()
                .filter(|provider| is_model_provider_live_configured(provider))
                .cloned()
                .collect::<Vec<_>>();
            Ok(IngressCommandResult::Reply(format!(
                "当前默认模型: {}。\n已就绪模型: {}。",
                if workspace.default_model_providers.is_empty() {
                    "<none>".to_string()
                } else {
                    workspace.default_model_providers.join(", ")
                },
                if live.is_empty() {
                    "<none>".to_string()
                } else {
                    live.join(", ")
                }
            )))
        }
        IngressCommand::ModeStatus => Ok(IngressCommandResult::Reply(format!(
            "当前功能等级: {}。\n{}",
            chat_mode_label(current_mode),
            chat_mode_description(current_mode)
        ))),
        IngressCommand::ModeSet { mode } => {
            let Some(chat_key) = chat_mode_key(record.chat_id.as_deref(), record.sender_id.as_deref()) else {
                return Ok(IngressCommandResult::Reply(
                    "当前会话没有可持久化的 chat 标识，暂时无法切换功能等级。".to_string(),
                ));
            };
            let now = unix_timestamp_ms();
            state
                .upsert_chat_automation_mode(ChatAutomationModeRecord {
                    platform: platform.to_string(),
                    chat_key,
                    chat_id: record.chat_id.clone(),
                    sender_id: record.sender_id.clone(),
                    mode,
                    updated_by: record.sender_display.clone().or(record.sender_id.clone()),
                    reason: Some("changed from chat ingress".to_string()),
                    last_ingress_id: Some(record.ingress_id),
                    created_at_unix_ms: now,
                    updated_at_unix_ms: now,
                })
                .await?;
            Ok(IngressCommandResult::Reply(format!(
                "已切换到 {}。\n{}",
                chat_mode_label(mode),
                chat_mode_description(mode)
            )))
        }
        IngressCommand::Status => {
            let workspace = identity::ensure_workspace_profile(&state).await?;
            let nodes = state.list_nodes().await?;
            let connected = nodes.iter().filter(|node| node.connected).count();
            let trusted = nodes
                .iter()
                .filter(|node| node.connected && node.attestation_verified)
                .count();
            Ok(IngressCommandResult::Reply(format!(
                "工作区: {} [{}]\n当前功能等级: {}\n默认模型: {}\n默认聊天: {}\n在线节点: {}，可信节点: {}。",
                workspace.display_name,
                workspace.region,
                chat_mode_label(current_mode),
                if workspace.default_model_providers.is_empty() {
                    "<none>".to_string()
                } else {
                    workspace.default_model_providers.join(", ")
                },
                if workspace.default_chat_platforms.is_empty() {
                    "<none>".to_string()
                } else {
                    workspace.default_chat_platforms.join(", ")
                },
                connected,
                trusted
            )))
        }
        IngressCommand::Task(text) => {
            let text = text.trim();
            if text.is_empty() {
                return Ok(IngressCommandResult::Reply(
                    "用法: /task <要提交的任务内容>".to_string(),
                ));
            }
            Ok(IngressCommandResult::Task {
                instruction: text.to_string(),
                task_name: format!("{platform} task request"),
            })
        }
        IngressCommand::Orchestrate(text) => {
            let text = text.trim();
            if text.is_empty() {
                return Ok(IngressCommandResult::Reply(
                    "用法: /orchestrate <JSON 编排步骤>".to_string(),
                ));
            }
            Ok(IngressCommandResult::Task {
                instruction: format!("orchestrate:{text}"),
                task_name: format!("{platform} orchestration request"),
            })
        }
        IngressCommand::Wasm(text) => {
            let text = text.trim();
            if text.is_empty() {
                return Ok(IngressCommandResult::Reply(
                    "用法: /wasm <skill[@version][#function]>".to_string(),
                ));
            }
            Ok(IngressCommandResult::Task {
                instruction: format!("wasm:{text}"),
                task_name: format!("{platform} wasm request"),
            })
        }
        IngressCommand::Unknown(command) => Ok(IngressCommandResult::Reply(format!(
            "未知命令 `{command}`。\n{}",
            help_text
        ))),
    }
}

fn help_command_text() -> String {
    [
        "可用命令:",
        "#chat - 纯聊天模式，不读取电脑状态",
        "#observe - 只读观察模式，可分析当前电脑状态",
        "#assist - 辅助模式，会预览可执行动作但不直接执行",
        "#autopilot - 自动驾驶模式，支持在审批链内下发受控动作",
        "#mode - 查看当前功能等级",
        "/help - 查看命令帮助",
        "/commands - 查看命令帮助",
        "/new - 开始新的对话",
        "/skills [关键字] - 查看已安装技能",
        "/skills search <关键字> - 搜索已安装技能",
        "/skill <skill[@version][#function]> - 调用一个已安装技能",
        "/model - 查看当前默认模型",
        "/status - 查看工作区与节点状态",
        "/task <内容> - 提交普通任务",
        "/orchestrate <JSON> - 提交编排任务",
        "/wasm <skill[@version][#function]> - 直接提交 Wasm 技能任务",
    ]
    .join("\n")
}

fn help_command_text_for_platform(platform: &str) -> String {
    let mut help = help_command_text();
    let platform_hint = match platform {
        "telegram" => Some("平台提示：直接发送 /help、/skills、/status 即可。"),
        "signal" | "bluebubbles" => Some("平台提示：直接发送 /help、/status 或 #observe。"),
        "feishu" | "dingtalk" | "qq" | "wecom" => Some(
            "平台提示：可以直接发 `帮助`、`状态`、`技能`，也支持 `@机器人 /help`、`／skills`、`＃observe`。",
        ),
        "wechat_official_account" => Some(
            "平台提示：可以直接发 `帮助`、`状态`、`技能`，也支持 `／skills`、`＃observe`。",
        ),
        _ => None,
    };
    if let Some(platform_hint) = platform_hint {
        help.push_str("\n\n");
        help.push_str(platform_hint);
    }
    help
}

fn chat_mode_label(mode: ChatAutomationMode) -> &'static str {
    match mode {
        ChatAutomationMode::Chat => "#chat",
        ChatAutomationMode::Observe => "#observe",
        ChatAutomationMode::Assist => "#assist",
        ChatAutomationMode::Autopilot => "#autopilot",
    }
}

fn chat_mode_description(mode: ChatAutomationMode) -> &'static str {
    match mode {
        ChatAutomationMode::Chat => {
            "仅使用默认模型回复，不主动读取电脑状态，也不执行本机动作。"
        }
        ChatAutomationMode::Observe => {
            "允许只读观察当前电脑状态，会在需要时采样进程快照并让模型总结。"
        }
        ChatAutomationMode::Assist => {
            "会先给出本机动作预览和安全提示；危险动作不会直接执行。"
        }
        ChatAutomationMode::Autopilot => {
            "允许在审批链内自动下发受控电脑动作；浏览器和桌面动作仍然需要审批。"
        }
    }
}

fn chat_mode_key(chat_id: Option<&str>, sender_id: Option<&str>) -> Option<String> {
    chat_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            sender_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
}

async fn current_chat_automation_mode(
    state: Arc<AppState>,
    platform: &str,
    chat_id: Option<&str>,
    sender_id: Option<&str>,
) -> anyhow::Result<ChatAutomationMode> {
    let Some(chat_key) = chat_mode_key(chat_id, sender_id) else {
        return Ok(ChatAutomationMode::Chat);
    };
    Ok(state
        .get_chat_automation_mode(platform, &chat_key)
        .await?
        .map(|record| record.mode)
        .unwrap_or(ChatAutomationMode::Chat))
}

async fn try_mode_aware_reply(
    state: Arc<AppState>,
    platform: &str,
    record: &ChatIngressEventRecord,
    mode: ChatAutomationMode,
) -> anyhow::Result<Option<String>> {
    let text = record.text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let action = parse_local_action_intent(text);
    match mode {
        ChatAutomationMode::Chat => Ok(None),
        ChatAutomationMode::Observe => {
            if should_attempt_observation(text) {
                return Ok(Some(
                    execute_observation_mode_reply(state, platform, record, text).await?,
                ));
            }
            Ok(None)
        }
        ChatAutomationMode::Assist => {
            if should_attempt_observation(text) {
                return Ok(Some(
                    execute_observation_mode_reply(state, platform, record, text).await?,
                ));
            }
            Ok(action.map(render_assist_action_preview))
        }
        ChatAutomationMode::Autopilot => {
            if should_attempt_observation(text) {
                return Ok(Some(
                    execute_observation_mode_reply(state, platform, record, text).await?,
                ));
            }
            if let Some(action) = action {
                return Ok(Some(
                    execute_autopilot_action(state, platform, record, action).await?,
                ));
            }
            Ok(None)
        }
    }
}

fn should_attempt_observation(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    let keywords = [
        "电脑",
        "计算机",
        "当前在干什么",
        "现在在干什么",
        "进程",
        "cpu",
        "内存",
        "活动窗口",
        "what is my computer doing",
        "what is the computer doing",
        "current process",
        "processes",
        "memory",
        "cpu usage",
        "system status",
    ];
    keywords.iter().any(|keyword| normalized.contains(keyword))
}

fn parse_local_action_intent(text: &str) -> Option<LocalActionIntent> {
    let trimmed = text.trim();
    for prefix in ["打开 ", "open "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let target = rest.trim();
            if !target.is_empty() {
                return Some(LocalActionIntent::BrowserOpen {
                    target: target.to_string(),
                });
            }
        }
    }
    for prefix in ["通知 ", "提醒 ", "notify "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let message = rest.trim();
            if !message.is_empty() {
                return Some(LocalActionIntent::DesktopNotification {
                    message: message.to_string(),
                });
            }
        }
    }
    None
}

fn render_assist_action_preview(action: LocalActionIntent) -> String {
    match action {
        LocalActionIntent::BrowserOpen { target } => format!(
            "辅助模式已识别出浏览器动作预览：将打开 `{}`。\n出于安全原则，辅助模式只预览不执行。发送 `#autopilot` 后重试，或显式使用 /task /orchestrate。",
            normalize_browser_target(&target)
        ),
        LocalActionIntent::DesktopNotification { message } => format!(
            "辅助模式已识别出桌面通知预览：将发送通知 `{}`。\n出于安全原则，辅助模式只预览不执行。发送 `#autopilot` 后重试，或显式使用 /task /orchestrate。",
            message
        ),
    }
}

async fn execute_autopilot_action(
    state: Arc<AppState>,
    _platform: &str,
    _record: &ChatIngressEventRecord,
    action: LocalActionIntent,
) -> anyhow::Result<String> {
    match action {
        LocalActionIntent::BrowserOpen { target } => {
            let node = select_node_for_capability(&state, "browser_open").await?;
            let (command, delivery) = control_plane::dispatch_gateway_command(
                &state,
                &node.node_id,
                "browser_open",
                json!({
                    "url": normalize_browser_target(&target),
                    "approvalRequired": true
                }),
            )
            .await?;
            Ok(match delivery {
                "awaiting_approval" => format!(
                    "已创建浏览器打开请求，等待审批。\nnode={} commandId={} target={}",
                    node.node_id, command.command_id, normalize_browser_target(&target)
                ),
                other => format!(
                    "已下发浏览器打开请求。\nnode={} commandId={} delivery={} target={}",
                    node.node_id, command.command_id, other, normalize_browser_target(&target)
                ),
            })
        }
        LocalActionIntent::DesktopNotification { message } => {
            let node = select_node_for_capability(&state, "desktop_notification").await?;
            let (command, delivery) = control_plane::dispatch_gateway_command(
                &state,
                &node.node_id,
                "desktop_notification",
                json!({
                    "message": message,
                    "approvalRequired": true
                }),
            )
            .await?;
            Ok(match delivery {
                "awaiting_approval" => format!(
                    "已创建桌面通知请求，等待审批。\nnode={} commandId={}",
                    node.node_id, command.command_id
                ),
                other => format!(
                    "已下发桌面通知请求。\nnode={} commandId={} delivery={}",
                    node.node_id, command.command_id, other
                ),
            })
        }
    }
}

async fn execute_observation_mode_reply(
    state: Arc<AppState>,
    platform: &str,
    record: &ChatIngressEventRecord,
    question: &str,
) -> anyhow::Result<String> {
    let node = select_node_for_capability(&state, "process_snapshot").await?;
    let system_info = dispatch_and_wait_node_command(&state, &node.node_id, "system_info", json!({})).await.ok();
    let process_snapshot = dispatch_and_wait_node_command(
        &state,
        &node.node_id,
        "process_snapshot",
        json!({ "limit": 12 }),
    )
    .await?;
    let observation = json!({
        "node": {
            "nodeId": node.node_id,
            "displayName": node.display_name,
        },
        "systemInfo": system_info.as_ref().map(extract_command_result_payload),
        "processSnapshot": extract_command_result_payload(&process_snapshot),
        "chatPlatform": platform,
        "chatId": record.chat_id,
    });
    if let Some(provider) = pick_live_default_model_provider(&state).await? {
        let response = connectors::execute_model_connector(
            &provider,
            OpenAIResponseRequest {
                input: format!(
                    "用户问题：{question}\n\n以下是该电脑的只读观测数据(JSON)：\n{}\n\n请用中文回答：现在这台电脑大概率正在做什么，哪些进程最值得注意；不要假装看到了屏幕内容，只根据这些观测数据回答。",
                    serde_json::to_string_pretty(&observation)?
                ),
                model: None,
                instructions: Some(
                    "You are Dawn. Respect the read-only security boundary: summarize the computer state from the provided telemetry, state uncertainty explicitly, and do not claim you saw content not present in the telemetry. Keep the reply concise but useful."
                        .to_string(),
                ),
            },
        )
        .await?;
        if !response.output_text.trim().is_empty() {
            return Ok(format!(
                "{}\n\n功能等级：{}（只读观察）",
                response.output_text.trim(),
                chat_mode_label(ChatAutomationMode::Observe)
            ));
        }
    }
    Ok(format!(
        "{}\n\n功能等级：{}（只读观察）",
        render_observation_fallback(&observation),
        chat_mode_label(ChatAutomationMode::Observe)
    ))
}

async fn select_node_for_capability(
    state: &Arc<AppState>,
    capability: &str,
) -> anyhow::Result<crate::app_state::NodeRecord> {
    let nodes = state.list_nodes().await?;
    nodes.into_iter()
        .find(|node| {
            node.connected
                && node.attestation_verified
                && node.capabilities.iter().any(|value| value == capability)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "当前没有可信在线节点支持 `{capability}`。先运行 `dawn-node start` 和 `dawn-node node trust-self`。"
            )
        })
}

async fn dispatch_and_wait_node_command(
    state: &Arc<AppState>,
    node_id: &str,
    command_type: &str,
    payload: Value,
) -> anyhow::Result<Value> {
    let (command, delivery) =
        control_plane::dispatch_gateway_command(state, node_id, command_type, payload).await?;
    if delivery == "awaiting_approval" {
        anyhow::bail!("命令 `{command_type}` 进入了审批队列，当前观察模式不能自动继续");
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let command_record = state
            .get_node_command(command.command_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("node command disappeared: {}", command.command_id))?;
        match command_record.status {
            NodeCommandStatus::Succeeded => {
                return Ok(command_record
                    .result
                    .unwrap_or_else(|| json!({ "status": "succeeded" })));
            }
            NodeCommandStatus::Failed => {
                anyhow::bail!(
                    "命令 `{command_type}` 执行失败：{}",
                    command_record
                        .error
                        .unwrap_or_else(|| "unknown node error".to_string())
                );
            }
            NodeCommandStatus::PendingApproval
            | NodeCommandStatus::Queued
            | NodeCommandStatus::Dispatched => {
                if tokio::time::Instant::now() >= deadline {
                    anyhow::bail!("等待 `{command_type}` 执行超时");
                }
                sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

fn extract_command_result_payload(value: &Value) -> Value {
    value.get("result").cloned().unwrap_or_else(|| value.clone())
}

fn render_observation_fallback(observation: &Value) -> String {
    let processes = observation
        .pointer("/processSnapshot/processes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if processes.is_empty() {
        return "我已经进入只读观察模式，但这次没有采到可用的进程快照。".to_string();
    }
    let top = processes
        .into_iter()
        .take(5)
        .filter_map(|item| {
            let name = item
                .get("name")
                .or_else(|| item.get("imageName"))
                .and_then(Value::as_str)?;
            let pid = item.get("pid").and_then(Value::as_i64).unwrap_or_default();
            Some(format!("{name}(pid={pid})"))
        })
        .collect::<Vec<_>>();
    format!(
        "我已经采样了当前电脑的只读状态。当前最显眼的进程有：{}。如果你需要更细的动作执行，请先切到 #assist 或 #autopilot。",
        top.join("、")
    )
}

async fn pick_live_default_model_provider(state: &Arc<AppState>) -> anyhow::Result<Option<String>> {
    let workspace = identity::ensure_workspace_profile(state).await?;
    Ok(workspace
        .default_model_providers
        .iter()
        .find(|value| is_model_provider_live_configured(value))
        .cloned())
}

fn normalize_browser_target(target: &str) -> String {
    let trimmed = target.trim();
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

async fn render_skills_command(
    state: &Arc<AppState>,
    query: Option<&str>,
) -> anyhow::Result<String> {
    let distribution = skill_registry::current_distribution(state).await?;
    let normalized_query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let mut skills = distribution
        .skills
        .into_iter()
        .filter(|skill| {
            normalized_query.as_ref().is_none_or(|query| {
                skill.skill_id.to_ascii_lowercase().contains(query)
                    || skill.display_name.to_ascii_lowercase().contains(query)
                    || skill
                        .description
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(query)
                    || skill
                        .capabilities
                        .iter()
                        .any(|capability| capability.to_ascii_lowercase().contains(query))
            })
        })
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.skill_id.cmp(&right.skill_id))
            .then_with(|| right.version.cmp(&left.version))
    });
    if skills.is_empty() {
        return Ok(match normalized_query {
            Some(query) => format!(
                "没有找到和 `{query}` 匹配的已安装技能。输入 /skills 查看全部技能。"
            ),
            None => {
                "当前没有已安装技能。先在 CLI 中运行 `dawn-node setup` 或 `dawn-node skills install`。"
                    .to_string()
            }
        });
    }
    let mut lines = vec!["已安装技能:".to_string()];
    for skill in skills.into_iter().take(8) {
        let source_suffix = match skill.source_kind.as_str() {
            "native_builtin" => " [native]",
            "signed_publisher" => " [signed]",
            _ => "",
        };
        lines.push(format!(
            "- {}@{}{}{}: {}",
            skill.skill_id,
            skill.version,
            if skill.active { " [active]" } else { "" },
            source_suffix,
            skill
                .description
                .as_deref()
                .unwrap_or(skill.display_name.as_str())
        ));
    }
    lines.push("使用方式: /skill <skill_id>".to_string());
    Ok(lines.join("\n"))
}

fn parse_skills_query(remainder: &str) -> Option<String> {
    let trimmed = remainder.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (command, query) = match trimmed.split_once(char::is_whitespace) {
        Some((command, query)) => (command, query.trim()),
        None => (trimmed, ""),
    };
    if matches!(command, "search" | "find") {
        return (!query.is_empty()).then(|| query.to_string());
    }
    Some(trimmed.to_string())
}

struct ParsedSkillSelector {
    skill_id: String,
    version: Option<String>,
    function_name: Option<String>,
}

fn parse_skill_selector(raw: &str) -> anyhow::Result<ParsedSkillSelector> {
    let selector = raw.trim();
    if selector.is_empty() {
        anyhow::bail!("用法: /skill <skill[@version][#function]>");
    }
    let selector = selector
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("用法: /skill <skill[@version][#function]>"))?;
    let (skill_selector, function_name) = match selector.split_once('#') {
        Some((selector, function_name)) if !function_name.trim().is_empty() => {
            (selector.trim(), Some(function_name.trim().to_string()))
        }
        Some((_selector, _)) => anyhow::bail!("技能函数名不能为空"),
        None => (selector.trim(), None),
    };
    let (skill_id, version) = match skill_selector.split_once('@') {
        Some((skill_id, version)) if !skill_id.trim().is_empty() && !version.trim().is_empty() => {
            (skill_id.trim().to_string(), Some(version.trim().to_string()))
        }
        Some((_skill_id, _version)) => anyhow::bail!("技能版本选择器格式无效"),
        None => (skill_selector.to_string(), None),
    };
    Ok(ParsedSkillSelector {
        skill_id,
        version,
        function_name,
    })
}

fn build_skill_selector_for_task(skill: &skill_registry::SkillRecord, function: Option<&str>) -> String {
    match function.filter(|value| !value.trim().is_empty()) {
        Some(function) => format!("{}@{}#{}", skill.skill_id, skill.version, function.trim()),
        None => format!("{}@{}", skill.skill_id, skill.version),
    }
}

fn should_attempt_default_model_reply(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && !trimmed.starts_with('/') && !trimmed.starts_with('#')
}

async fn try_default_model_reply(
    state: Arc<AppState>,
    platform: &str,
    text: &str,
) -> anyhow::Result<Option<String>> {
    let Some(provider) = pick_live_default_model_provider(&state).await? else {
        return Ok(None);
    };

    let response = connectors::execute_model_connector(
        &provider,
        OpenAIResponseRequest {
            input: text.trim().to_string(),
            model: None,
            instructions: Some(format!(
                "You are Dawn, a concise desktop AI assistant replying inside a {platform} chat. Respond directly in the user's language. Keep replies short unless the user asks for detail."
            )),
        },
    )
    .await?;

    let output = response.output_text.trim().to_string();
    if output.is_empty() {
        return Ok(None);
    }
    Ok(Some(output))
}

fn is_model_provider_live_configured(provider: &str) -> bool {
    match provider {
        "openai" => std::env::var("OPENAI_API_KEY").is_ok(),
        "openai_codex" => connectors::openai_codex_login_ready(),
        "anthropic" => std::env::var("ANTHROPIC_API_KEY").is_ok(),
        "google" => std::env::var("GEMINI_API_KEY").is_ok() || std::env::var("GOOGLE_API_KEY").is_ok(),
        "bedrock" => std::env::var("BEDROCK_API_KEY").is_ok()
            && (std::env::var("BEDROCK_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("BEDROCK_BASE_URL").is_ok()
                || std::env::var("BEDROCK_RUNTIME_ENDPOINT").is_ok()),
        "cloudflare_ai_gateway" => (std::env::var("CLOUDFLARE_AI_GATEWAY_API_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok())
            && (std::env::var("CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("CLOUDFLARE_AI_GATEWAY_BASE_URL").is_ok()
                || (std::env::var("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID").is_ok()
                    && std::env::var("CLOUDFLARE_AI_GATEWAY_ID").is_ok())),
        "github_models" => {
            std::env::var("GITHUB_MODELS_API_KEY").is_ok() || std::env::var("GITHUB_TOKEN").is_ok()
        }
        "huggingface" => {
            std::env::var("HUGGINGFACE_API_KEY").is_ok() || std::env::var("HF_TOKEN").is_ok()
        }
        "openrouter" => std::env::var("OPENROUTER_API_KEY").is_ok(),
        "groq" => std::env::var("GROQ_API_KEY").is_ok(),
        "together" => std::env::var("TOGETHER_API_KEY").is_ok(),
        "vercel_ai_gateway" => std::env::var("VERCEL_AI_GATEWAY_API_KEY").is_ok()
            || std::env::var("AI_GATEWAY_API_KEY").is_ok()
            || std::env::var("VERCEL_AI_GATEWAY_BASE_URL").is_ok()
            || std::env::var("VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL").is_ok(),
        "vllm" => {
            std::env::var("VLLM_CHAT_COMPLETIONS_URL").is_ok() || std::env::var("VLLM_BASE_URL").is_ok()
        }
        "mistral" => std::env::var("MISTRAL_API_KEY").is_ok(),
        "nvidia" => std::env::var("NVIDIA_API_KEY").is_ok() || std::env::var("NVIDIA_NIM_API_KEY").is_ok(),
        "litellm" => {
            std::env::var("LITELLM_CHAT_COMPLETIONS_URL").is_ok() || std::env::var("LITELLM_BASE_URL").is_ok()
        }
        "deepseek" => std::env::var("DEEPSEEK_API_KEY").is_ok(),
        "qwen" => std::env::var("QWEN_API_KEY").is_ok() || std::env::var("DASHSCOPE_API_KEY").is_ok(),
        "zhipu" => std::env::var("ZHIPU_API_KEY").is_ok(),
        "moonshot" => std::env::var("MOONSHOT_API_KEY").is_ok(),
        "doubao" => std::env::var("DOUBAO_API_KEY").is_ok() || std::env::var("ARK_API_KEY").is_ok(),
        "ollama" => {
            std::env::var("OLLAMA_CHAT_URL").is_ok() || std::env::var("OLLAMA_BASE_URL").is_ok()
        }
        _ => false,
    }
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

fn summarize_signal_event(payload: &Value) -> Option<IngressMessageSummary> {
    if let Some(text) = extract_signal_text(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: true,
        });
    }
    if let Some(text) = extract_signal_attachment_summary(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: true,
        });
    }
    if let Some(text) = extract_signal_reaction_summary(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: true,
        });
    }
    if let Some(text) = extract_signal_receipt_summary(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: false,
        });
    }
    extract_signal_typing_summary(payload).map(|text| IngressMessageSummary {
        text,
        route_to_task: false,
    })
}

fn extract_signal_text(payload: &Value) -> Option<String> {
    payload
        .pointer("/envelope/dataMessage/message")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/dataMessage/message")
                .and_then(Value::as_str)
        })
        .or_else(|| payload.pointer("/message").and_then(Value::as_str))
        .or_else(|| payload.pointer("/text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_signal_attachment_summary(payload: &Value) -> Option<String> {
    summarize_attachment_event(
        "Signal attachment received",
        payload,
        &[
            "/envelope/dataMessage/attachments",
            "/dataMessage/attachments",
            "/attachments",
        ],
    )
}

fn extract_signal_reaction_summary(payload: &Value) -> Option<String> {
    let reaction = payload
        .pointer("/envelope/dataMessage/reaction")
        .or_else(|| payload.pointer("/dataMessage/reaction"))
        .or_else(|| payload.pointer("/reaction"))?;
    let emoji = reaction
        .get("emoji")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("reaction");
    let target_author = reaction
        .get("targetAuthor")
        .and_then(Value::as_str)
        .or_else(|| reaction.get("author").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty());
    let target_timestamp = reaction
        .get("targetSentTimestamp")
        .or_else(|| reaction.get("targetTimestamp"))
        .and_then(value_as_i64);
    let removed = reaction
        .get("remove")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut summary = format!(
        "Signal reaction {}: {}",
        if removed { "removed" } else { "received" },
        emoji
    );
    if let Some(author) = target_author {
        summary.push_str(&format!(" for {author}"));
    }
    if let Some(timestamp) = target_timestamp {
        summary.push_str(&format!(" @ {timestamp}"));
    }
    Some(summary)
}

fn extract_signal_receipt_summary(payload: &Value) -> Option<String> {
    let receipt = payload
        .pointer("/envelope/receiptMessage")
        .or_else(|| payload.pointer("/receiptMessage"))?;
    let receipt_type = receipt
        .get("type")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("receipt");
    let count = receipt
        .get("timestamps")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Some(if count > 0 {
        format!("Signal {receipt_type} receipt for {count} message(s)")
    } else {
        format!("Signal {receipt_type} receipt")
    })
}

fn extract_signal_typing_summary(payload: &Value) -> Option<String> {
    let typing = payload
        .pointer("/envelope/typingMessage")
        .or_else(|| payload.pointer("/typingMessage"))?;
    let action = typing
        .get("action")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("updated");
    Some(format!("Signal typing indicator: {action}"))
}

fn summarize_bluebubbles_event(payload: &Value) -> Option<IngressMessageSummary> {
    if let Some(text) = extract_bluebubbles_text(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: true,
        });
    }
    if let Some(text) = extract_bluebubbles_attachment_summary(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: true,
        });
    }
    if let Some(text) = extract_bluebubbles_reaction_summary(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: true,
        });
    }
    if let Some(text) = extract_bluebubbles_receipt_summary(payload) {
        return Some(IngressMessageSummary {
            text,
            route_to_task: false,
        });
    }
    extract_bluebubbles_typing_summary(payload).map(|text| IngressMessageSummary {
        text,
        route_to_task: false,
    })
}

fn extract_bluebubbles_text(payload: &Value) -> Option<String> {
    payload
        .pointer("/text")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/message/text").and_then(Value::as_str))
        .or_else(|| payload.pointer("/message").and_then(Value::as_str))
        .or_else(|| payload.pointer("/data/text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_bluebubbles_attachment_summary(payload: &Value) -> Option<String> {
    summarize_attachment_event(
        "BlueBubbles attachment received",
        payload,
        &[
            "/attachments",
            "/message/attachments",
            "/data/attachments",
            "/message/attachmentMetadata",
        ],
    )
}

fn extract_bluebubbles_reaction_summary(payload: &Value) -> Option<String> {
    let associated = payload
        .pointer("/associatedMessage")
        .or_else(|| payload.pointer("/message/associatedMessage"))
        .or_else(|| payload.pointer("/data/associatedMessage"))?;
    let emoji = associated
        .get("emoji")
        .and_then(Value::as_str)
        .or_else(|| associated.get("body").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("reaction");
    let target_guid = associated
        .get("guid")
        .and_then(Value::as_str)
        .or_else(|| associated.get("messageGuid").and_then(Value::as_str))
        .or_else(|| associated.get("targetGuid").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty());
    let removed = associated
        .get("remove")
        .and_then(Value::as_bool)
        .or_else(|| associated.get("isRemoved").and_then(Value::as_bool))
        .unwrap_or(false);
    let mut summary = format!(
        "BlueBubbles reaction {}: {}",
        if removed { "removed" } else { "received" },
        emoji
    );
    if let Some(guid) = target_guid {
        summary.push_str(&format!(" for {guid}"));
    }
    Some(summary)
}

fn extract_bluebubbles_receipt_summary(payload: &Value) -> Option<String> {
    let event = payload
        .pointer("/event")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/type").and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if event.contains("read")
        || payload
            .pointer("/message/dateRead")
            .and_then(value_as_i64)
            .is_some()
    {
        return Some("BlueBubbles read receipt".to_string());
    }
    if event.contains("delivered")
        || payload
            .pointer("/message/dateDelivered")
            .and_then(value_as_i64)
            .is_some()
    {
        return Some("BlueBubbles delivery receipt".to_string());
    }
    None
}

fn extract_bluebubbles_typing_summary(payload: &Value) -> Option<String> {
    let event = payload
        .pointer("/event")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/type").and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if event.contains("typing") {
        return Some(format!("BlueBubbles typing indicator: {event}"));
    }
    payload
        .pointer("/typing/status")
        .and_then(Value::as_str)
        .map(|status| format!("BlueBubbles typing indicator: {status}"))
}

fn summarize_attachment_event(prefix: &str, payload: &Value, paths: &[&str]) -> Option<String> {
    let mut labels = Vec::new();
    for path in paths {
        let Some(items) = payload.pointer(path).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if let Some(label) = attachment_label(item) {
                labels.push(label);
            }
        }
    }
    labels.sort();
    labels.dedup();
    if labels.is_empty() {
        None
    } else {
        Some(format!("{prefix}: {}", labels.join(", ")))
    }
}

fn attachment_label(value: &Value) -> Option<String> {
    let name = [
        "/filename",
        "/fileName",
        "/name",
        "/storedFilename",
        "/transferName",
        "/originalName",
    ]
    .iter()
    .find_map(|path| value.pointer(path).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty());
    let mime = ["/contentType", "/mimeType", "/mime_type", "/type"]
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let identifier = ["/id", "/guid", "/attachmentGuid"]
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (name, mime, identifier) {
        (Some(name), Some(mime), _) => Some(format!("{name} ({mime})")),
        (Some(name), None, _) => Some(name.to_string()),
        (None, Some(mime), Some(identifier)) => Some(format!("{identifier} ({mime})")),
        (None, Some(mime), None) => Some(mime.to_string()),
        (None, None, Some(identifier)) => Some(identifier.to_string()),
        _ => None,
    }
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
        .or_else(|| value.as_str().and_then(|raw| raw.parse::<i64>().ok()))
}

fn extract_dingtalk_text(payload: &Value) -> Option<String> {
    if let Some(message_type) = payload.pointer("/msgtype").and_then(Value::as_str) {
        if message_type != "text" {
            return None;
        }
    }

    payload
        .pointer("/text/content")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/content/text").and_then(Value::as_str))
        .or_else(|| payload.pointer("/msg/text/content").and_then(Value::as_str))
        .or_else(|| payload.pointer("/text").and_then(Value::as_str))
        .map(ToString::to_string)
}

fn extract_wecom_text(payload: &Value) -> Option<String> {
    if let Some(message_type) = payload.pointer("/msgtype").and_then(Value::as_str) {
        if message_type != "text" {
            return None;
        }
    }

    payload
        .pointer("/text/content")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/content").and_then(Value::as_str))
        .or_else(|| payload.pointer("/text").and_then(Value::as_str))
        .map(ToString::to_string)
}

fn extract_qq_text(payload: &Value) -> Option<String> {
    let raw_text = payload
        .pointer("/d/content")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/content").and_then(Value::as_str))?;
    let normalized = normalize_qq_message_text(raw_text);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_qq_message_text(text: &str) -> String {
    let mut cleaned = text.trim().to_string();
    while let Some(rest) = strip_leading_qq_mention(&cleaned) {
        cleaned = rest.trim_start().to_string();
    }
    cleaned
}

fn strip_leading_qq_mention(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("<@!")?;
    let close_idx = rest.find('>')?;
    Some(&rest[close_idx + 1..])
}

#[derive(Debug)]
struct WeChatOfficialAccountMessage {
    to_user_name: Option<String>,
    from_user_name: Option<String>,
    msg_type: Option<String>,
    text: Option<String>,
    msg_id: Option<String>,
    create_time: Option<String>,
    event_type: Option<String>,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
}

fn parse_wechat_official_account_xml(xml: &str) -> Option<WeChatOfficialAccountMessage> {
    let msg_type = extract_xml_tag(xml, "MsgType");
    let text = match msg_type.as_deref() {
        Some("text") => extract_xml_tag(xml, "Content"),
        _ => None,
    };
    Some(WeChatOfficialAccountMessage {
        to_user_name: extract_xml_tag(xml, "ToUserName"),
        from_user_name: extract_xml_tag(xml, "FromUserName"),
        msg_type: msg_type.clone(),
        text,
        msg_id: extract_xml_tag(xml, "MsgId"),
        create_time: extract_xml_tag(xml, "CreateTime"),
        event_type: extract_xml_tag(xml, "Event"),
        chat_id: extract_xml_tag(xml, "FromUserName"),
        sender_id: extract_xml_tag(xml, "FromUserName"),
        sender_display: extract_xml_tag(xml, "FromUserName"),
    })
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let cdata_open = format!("<{tag}><![CDATA[");
    let cdata_close = "]]>";
    if let Some(start) = xml.find(&cdata_open) {
        let value_start = start + cdata_open.len();
        let remainder = &xml[value_start..];
        let end = remainder.find(cdata_close)?;
        return Some(remainder[..end].to_string());
    }

    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let value_start = start + open.len();
    let remainder = &xml[value_start..];
    let end = remainder.find(&close)?;
    Some(remainder[..end].trim().to_string())
}

fn verify_telegram_secret(secret: &str) -> anyhow::Result<()> {
    if let Ok(expected) = std::env::var("DAWN_TELEGRAM_WEBHOOK_SECRET") {
        if expected != secret {
            anyhow::bail!("telegram webhook secret mismatch");
        }
    }
    Ok(())
}

fn telegram_ingress_mode() -> &'static str {
    if telegram_polling_enabled() {
        "polling"
    } else if std::env::var("DAWN_TELEGRAM_WEBHOOK_SECRET").is_ok() {
        "webhook"
    } else {
        "disabled"
    }
}

fn telegram_polling_enabled() -> bool {
    let explicit = std::env::var("DAWN_TELEGRAM_POLLING")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    if explicit {
        return std::env::var("TELEGRAM_BOT_TOKEN").is_ok();
    }
    if std::env::var("TELEGRAM_BOT_TOKEN").is_err() {
        return false;
    }
    if std::env::var("DAWN_TELEGRAM_WEBHOOK_SECRET").is_err() {
        return true;
    }
    matches!(
        std::env::var("DAWN_PUBLIC_BASE_URL"),
        Ok(value) if public_base_url_is_local_only(&value)
    )
}

fn public_base_url_is_local_only(raw: &str) -> bool {
    let value = raw.trim().to_ascii_lowercase();
    value.contains("127.0.0.1")
        || value.contains("localhost")
        || value.contains("0.0.0.0")
        || value.contains("[::1]")
}

fn telegram_bot_commands() -> Vec<TelegramBotCommand> {
    vec![
        TelegramBotCommand {
            command: "help",
            description: "Show the command list",
        },
        TelegramBotCommand {
            command: "commands",
            description: "Show the command list",
        },
        TelegramBotCommand {
            command: "new",
            description: "Start a fresh chat turn",
        },
        TelegramBotCommand {
            command: "skills",
            description: "List installed Dawn skills",
        },
        TelegramBotCommand {
            command: "skill",
            description: "Run an installed skill by id",
        },
        TelegramBotCommand {
            command: "model",
            description: "Show current default model",
        },
        TelegramBotCommand {
            command: "status",
            description: "Show workspace and node status",
        },
    ]
}

async fn register_telegram_bot_commands(bot_token: String) -> anyhow::Result<()> {
    let client = Client::new();
    let response = client
        .post(format!(
            "https://api.telegram.org/bot{bot_token}/setMyCommands"
        ))
        .json(&json!({
            "commands": telegram_bot_commands()
        }))
        .send()
        .await
        .context("failed to call Telegram setMyCommands")?;
    let payload: Value = response
        .json()
        .await
        .context("failed to decode Telegram setMyCommands response")?;
    if payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        info!("Registered Telegram bot commands for Dawn ingress");
        Ok(())
    } else {
        anyhow::bail!(
            "Telegram setMyCommands failed: {}",
            payload
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        )
    }
}

pub fn spawn_telegram_ingress_worker(state: Arc<AppState>) {
    let Ok(bot_token) = std::env::var("TELEGRAM_BOT_TOKEN") else {
        return;
    };
    let command_token = bot_token.clone();
    tokio::spawn(async move {
        if let Err(error) = register_telegram_bot_commands(command_token).await {
            warn!(?error, "failed to register Telegram bot commands");
        }
    });
    if !telegram_polling_enabled() {
        return;
    }
    info!("Starting Telegram long-poll ingress worker");
    tokio::spawn(async move {
        let client = Client::new();
        let mut next_offset: Option<i64> = None;
        loop {
            let mut request = client
                .get(format!("https://api.telegram.org/bot{bot_token}/getUpdates"))
                .query(&[("timeout", "30"), ("allowed_updates", "[\"message\"]")]);
            if let Some(offset) = next_offset {
                request = request.query(&[("offset", offset)]);
            }
            match request.send().await {
                Ok(response) => match response.json::<TelegramGetUpdatesResponse>().await {
                    Ok(payload) if payload.ok => {
                        for update in payload.result {
                            if let Some(update_id) = update.update_id {
                                next_offset = Some(update_id + 1);
                            }
                            if let Err(error) = process_telegram_update(state.clone(), update).await
                            {
                                warn!(?error, "telegram polling worker failed to process update");
                            }
                        }
                    }
                    Ok(_) => {
                        warn!("telegram polling worker received non-ok getUpdates response");
                        sleep(Duration::from_secs(2)).await;
                    }
                    Err(error) => {
                        warn!(?error, "telegram polling worker failed to decode getUpdates response");
                        sleep(Duration::from_secs(2)).await;
                    }
                },
                Err(error) => {
                    warn!(?error, "telegram polling worker failed to fetch updates");
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}

fn verify_callback_secret(env_var: &str, platform: &str, secret: &str) -> anyhow::Result<()> {
    if let Ok(expected) = std::env::var(env_var) {
        if expected != secret {
            anyhow::bail!("{platform} callback secret mismatch");
        }
    }
    Ok(())
}

async fn resolve_pairing_decision(
    state: Arc<AppState>,
    platform: &str,
    identity_key: &str,
    approved: bool,
    request: PairingDecisionRequest,
) -> Result<ChatChannelIdentityRecord, (StatusCode, Json<Value>)> {
    let normalized_platform = platform.trim().to_ascii_lowercase();
    let Some(mut identity) = state
        .get_chat_channel_identity(&normalized_platform, identity_key)
        .await
        .map_err(internal_error)?
    else {
        return Err(not_found("chat pairing identity not found"));
    };
    identity.status = if approved {
        ChatChannelIdentityStatus::Paired
    } else {
        ChatChannelIdentityStatus::Rejected
    };
    identity.decision_reason = request.reason.clone();
    identity.updated_at_unix_ms = unix_timestamp_ms();
    let identity = state
        .upsert_chat_channel_identity(identity)
        .await
        .map_err(internal_error)?;

    let actor = request
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("operator");
    let reason_suffix = request
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(" Reason: {value}"))
        .unwrap_or_default();
    let message = if approved {
        format!(
            "Pairing approved for {normalized_platform}. {actor} allowed this chat to create tasks.{reason_suffix}"
        )
    } else {
        format!(
            "Pairing rejected for {normalized_platform}. {actor} denied inbound automation for this chat.{reason_suffix}"
        )
    };
    let _ = dispatch_ingress_reply_if_possible(
        &normalized_platform,
        identity.chat_id.as_deref(),
        &message,
    )
    .await;

    Ok(identity)
}

fn parse_pairing_status(
    raw: Option<&str>,
) -> Result<Option<ChatChannelIdentityStatus>, (StatusCode, Json<Value>)> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some("pending") => Ok(Some(ChatChannelIdentityStatus::Pending)),
        Some("paired") => Ok(Some(ChatChannelIdentityStatus::Paired)),
        Some("rejected") => Ok(Some(ChatChannelIdentityStatus::Rejected)),
        Some("blocked") => Ok(Some(ChatChannelIdentityStatus::Blocked)),
        Some(_) => Err(bad_request(anyhow::anyhow!(
            "pairing status must be pending, paired, rejected, or blocked"
        ))),
    }
}

fn verify_wechat_official_account_query(
    query: &WeChatOfficialAccountVerifyQuery,
) -> anyhow::Result<()> {
    let Ok(token) = std::env::var("DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN") else {
        return Ok(());
    };

    let signature = query
        .signature
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing wechat signature"))?;
    let timestamp = query
        .timestamp
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing wechat timestamp"))?;
    let nonce = query
        .nonce
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing wechat nonce"))?;
    let expected = compute_wechat_signature(&token, timestamp, nonce);
    if expected != signature {
        anyhow::bail!("wechat signature mismatch");
    }
    Ok(())
}

fn compute_wechat_signature(token: &str, timestamp: &str, nonce: &str) -> String {
    let mut parts = [token, timestamp, nonce];
    parts.sort_unstable();
    let mut sha = Sha1::new();
    sha.update(parts.concat().as_bytes());
    hex::encode(sha.finalize())
}

fn verify_dingtalk_callback_token(payload: &Value) -> anyhow::Result<()> {
    if let Ok(expected) = std::env::var("DAWN_DINGTALK_CALLBACK_TOKEN") {
        let actual = payload
            .pointer("/token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing dingtalk callback token"))?;
        if expected != actual {
            anyhow::bail!("dingtalk callback token mismatch");
        }
    }
    Ok(())
}

fn verify_wecom_callback_token(payload: &Value) -> anyhow::Result<()> {
    if let Ok(expected) = std::env::var("DAWN_WECOM_CALLBACK_TOKEN") {
        let actual = payload
            .pointer("/token")
            .and_then(Value::as_str)
            .or_else(|| payload.pointer("/ToUserName").and_then(Value::as_str))
            .ok_or_else(|| anyhow::anyhow!("missing wecom callback token"))?;
        if expected != actual {
            anyhow::bail!("wecom callback token mismatch");
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

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": message
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

fn plain_bad_request(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn plain_service_error(error: anyhow::Error) -> (StatusCode, String) {
    let message = error.to_string();
    if message.contains("unsupported") || message.contains("mismatch") || message.contains("empty")
    {
        return plain_bad_request(error);
    }
    plain_internal_error(error)
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

fn plain_internal_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };

    use axum::Router;
    use reqwest::Client;
    use wasmtime::Engine;

    use super::*;
    use crate::sandbox;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct ScopedEnvRestore {
        previous: Vec<(String, Option<String>)>,
    }

    impl ScopedEnvRestore {
        fn apply(entries: &[(&str, Option<&str>)]) -> Self {
            let previous = entries
                .iter()
                .map(|(key, value)| {
                    let prior = std::env::var(key).ok();
                    match value {
                        Some(next) => unsafe {
                            std::env::set_var(key, next);
                        },
                        None => unsafe {
                            std::env::remove_var(key);
                        },
                    }
                    ((*key).to_string(), prior)
                })
                .collect();
            Self { previous }
        }
    }

    impl Drop for ScopedEnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.previous.drain(..).rev() {
                match value {
                    Some(previous) => unsafe {
                        std::env::set_var(&key, previous);
                    },
                    None => unsafe {
                        std::env::remove_var(&key);
                    },
                }
            }
        }
    }

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-chat-ingress-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn spawn_test_server()
    -> anyhow::Result<(String, tokio::task::JoinHandle<()>, Arc<AppState>, PathBuf)> {
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

    #[tokio::test]
    async fn signal_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_SIGNAL_DM_POLICY", None)]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/signal/events/test-secret"
            ))
            .json(&json!({
                "envelope": {
                    "type": "receipt",
                    "source": "+15550002222",
                    "sourceName": "Signal Friend",
                    "dataMessage": {
                        "message": "/task Summarize Signal backlog"
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "signal");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "Summarize Signal backlog");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn signal_attachment_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_SIGNAL_DM_POLICY", None)]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/signal/events/test-secret"
            ))
            .json(&json!({
                "envelope": {
                    "type": "receipt",
                    "source": "+15550009999",
                    "sourceName": "Signal Attachment User",
                    "dataMessage": {
                        "attachments": [
                            {
                                "filename": "receipt.png",
                                "contentType": "image/png"
                            }
                        ]
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["status"], "task_created");

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        assert!(events[0].text.contains("Signal attachment received"));
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert!(task.instruction.contains("receipt.png"));

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn signal_typing_event_is_recorded_without_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_SIGNAL_DM_POLICY", None)]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/signal/events/test-secret"
            ))
            .json(&json!({
                "envelope": {
                    "type": "typing",
                    "source": "+15550006666",
                    "typingMessage": {
                        "action": "started"
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["status"], "ignored");

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].status, ChatIngressStatus::Ignored);
        assert!(events[0].linked_task_id.is_none());
        assert!(events[0].text.contains("typing indicator"));

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn dingtalk_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/dingtalk/events"))
            .json(&json!({
                "msgtype": "text",
                "text": { "content": "/task Create reimbursement summary" },
                "conversationId": "cid-dingtalk-001",
                "senderStaffId": "staff-001",
                "senderNick": "Chen Li"
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "dingtalk");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "Create reimbursement summary");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn bluebubbles_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_BLUEBUBBLES_DM_POLICY", None)]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/bluebubbles/events/test-secret"
            ))
            .json(&json!({
                "event": "message.created",
                "chatGuid": "iMessage;+15550002222",
                "message": {
                    "text": "/task Draft iMessage follow-up",
                    "handle": {
                        "address": "+15550002222",
                        "displayName": "Blue Contact"
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "bluebubbles");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "Draft iMessage follow-up");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn bluebubbles_reaction_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_BLUEBUBBLES_DM_POLICY", None)]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/bluebubbles/events/test-secret"
            ))
            .json(&json!({
                "event": "message.tapback",
                "chatGuid": "iMessage;+15550002222",
                "handle": {
                    "address": "+15550002222",
                    "displayName": "Blue Contact"
                },
                "associatedMessage": {
                    "emoji": "❤️",
                    "guid": "message-guid-123"
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["status"], "task_created");

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        assert!(events[0].text.contains("BlueBubbles reaction received"));
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert!(task.instruction.contains("message-guid-123"));

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn signal_event_can_require_pairing_and_then_create_task_after_approval()
    -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_SIGNAL_DM_POLICY", Some("pairing"))]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();

        let pending_response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/signal/events/test-secret"
            ))
            .json(&json!({
                "envelope": {
                    "type": "receipt",
                    "source": "+15550003333",
                    "sourceName": "Signal Pairing User",
                    "dataMessage": {
                        "message": "/task Pair me"
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let pending_body: Value = pending_response.json().await?;
        assert_eq!(pending_body["status"], "pending_approval");

        let identities = state
            .list_chat_channel_identities(Some("signal"), Some(ChatChannelIdentityStatus::Pending))
            .await?;
        assert_eq!(identities.len(), 1);
        assert!(identities[0].pairing_code.is_some());

        client
            .post(format!(
                "{base_url}/api/gateway/ingress/pairings/signal/{}/approve",
                identities[0].identity_key
            ))
            .json(&json!({
                "actor": "test-operator"
            }))
            .send()
            .await?
            .error_for_status()?;

        let approved_response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/signal/events/test-secret"
            ))
            .json(&json!({
                "envelope": {
                    "type": "receipt",
                    "source": "+15550003333",
                    "sourceName": "Signal Pairing User",
                    "dataMessage": {
                        "message": "/task Paired now"
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let approved_body: Value = approved_response.json().await?;
        assert_eq!(approved_body["status"], "task_created");

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn ingress_status_reports_pairing_policy_and_allowlist_counts() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_SIGNAL_DM_POLICY", Some("pairing")),
            ("DAWN_SIGNAL_ALLOWLIST", Some("+15550001111,+15550002222")),
            ("DAWN_BLUEBUBBLES_DM_POLICY", Some("allowlist")),
            ("DAWN_BLUEBUBBLES_ALLOWLIST", Some("iMessage;+15550003333")),
        ]);
        let (base_url, handle, _state, db_path) = spawn_test_server().await?;
        let client = Client::new();

        let response = client
            .get(format!("{base_url}/api/gateway/ingress/status"))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["signalDmPolicy"], "pairing");
        assert_eq!(body["signalAllowlistCount"], 2);
        assert_eq!(body["bluebubblesDmPolicy"], "allowlist");
        assert_eq!(body["bluebubblesAllowlistCount"], 1);

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn wecom_verify_round_trip_is_supported() -> anyhow::Result<()> {
        let (base_url, handle, _state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .get(format!(
                "{base_url}/api/gateway/ingress/wecom/events?echostr=hello-wecom"
            ))
            .send()
            .await?
            .error_for_status()?;
        let body = response.text().await?;
        assert_eq!(body, "hello-wecom");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn wecom_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/wecom/events"))
            .json(&json!({
                "msgtype": "text",
                "text": { "content": "/wasm echo-skill@1.0.0#run" },
                "chatid": "wecom-chat-123",
                "from": "zhangsan",
                "sender_name": "Zhang San",
                "event": "message"
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "wecom");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "wasm:echo-skill@1.0.0#run");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn wechat_official_account_verify_round_trip_is_supported() -> anyhow::Result<()> {
        let (base_url, handle, _state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .get(format!(
                "{base_url}/api/gateway/ingress/wechat-official-account/events?echostr=wechat-ok"
            ))
            .send()
            .await?
            .error_for_status()?;
        let body = response.text().await?;
        assert_eq!(body, "wechat-ok");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn wechat_official_account_xml_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/wechat-official-account/events"
            ))
            .header("content-type", "application/xml")
            .body(
                "<xml>\
                    <ToUserName><![CDATA[gh_001]]></ToUserName>\
                    <FromUserName><![CDATA[user-openid-123]]></FromUserName>\
                    <CreateTime>1710000000</CreateTime>\
                    <MsgType><![CDATA[text]]></MsgType>\
                    <Content><![CDATA[/task Schedule Shenzhen trip]]></Content>\
                    <MsgId>987654321</MsgId>\
                </xml>",
            )
            .send()
            .await?
            .error_for_status()?;
        let body = response.text().await?;
        assert_eq!(body, "success");

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "wechat_official_account");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "Schedule Shenzhen trip");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn qq_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/qq/events"))
            .json(&json!({
                "t": "AT_MESSAGE_CREATE",
                "d": {
                    "content": "<@!botid> /task Draft AP2 settlement summary",
                    "author": {
                        "id": "qq-user-001",
                        "username": "qq-operator"
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);

        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].platform, "qq");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        let task_id = events[0]
            .linked_task_id
            .ok_or_else(|| anyhow::anyhow!("missing linked task id"))?;
        let task = state
            .get_task(task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task not found"))?;
        assert_eq!(task.instruction, "Draft AP2 settlement summary");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[test]
    fn parses_help_and_skills_commands() {
        assert!(matches!(parse_ingress_command("/"), Some(IngressCommand::Help)));
        assert!(matches!(
            parse_ingress_command("/help@Helios042agentbot"),
            Some(IngressCommand::Help)
        ));
        assert!(matches!(
            parse_ingress_command("/commands"),
            Some(IngressCommand::Help)
        ));
        assert!(matches!(
            parse_ingress_command("/skills search echo"),
            Some(IngressCommand::Skills { query: Some(query) }) if query == "echo"
        ));
        assert!(matches!(
            parse_ingress_command("/skills find travel"),
            Some(IngressCommand::Skills { query: Some(query) }) if query == "travel"
        ));
        assert!(matches!(
            parse_ingress_command("／help"),
            Some(IngressCommand::Help)
        ));
        assert!(matches!(
            parse_ingress_command("＃observe"),
            Some(IngressCommand::ModeSet { mode: ChatAutomationMode::Observe })
        ));
    }

    #[test]
    fn normalizes_platform_specific_command_prefixes() {
        assert_eq!(
            normalize_ingress_command_text("feishu", "@Helios ／help"),
            "/help"
        );
        assert_eq!(
            normalize_ingress_command_text("dingtalk", "＠机器人 ＃assist"),
            "#assist"
        );
        assert_eq!(
            normalize_ingress_command_text("qq", "<@!botid> ／skills search echo"),
            "/skills search echo"
        );
        assert_eq!(
            normalize_ingress_command_text("wechat_official_account", "<at user_id=\"ou_x\">机器人</at> /status"),
            "/status"
        );
        assert_eq!(normalize_ingress_command_text("feishu", "帮助"), "/help");
        assert_eq!(normalize_ingress_command_text("dingtalk", "状态"), "/status");
        assert_eq!(
            normalize_ingress_command_text("qq", "技能搜索 echo"),
            "/skills search echo"
        );
        assert_eq!(
            normalize_ingress_command_text("wechat_official_account", "观察模式"),
            "#observe"
        );
        assert_eq!(
            normalize_ingress_command_text("wecom", "使用技能 echo-skill"),
            "/skill echo-skill"
        );
        assert_eq!(normalize_ingress_command_text("feishu", "@Helios"), "/help");
        assert_eq!(
            normalize_ingress_command_text(
                "wechat_official_account",
                "<at user_id=\"ou_x\">机器人</at>"
            ),
            "/help"
        );
        assert_eq!(
            normalize_ingress_command_text("qq", "<@!botid>"),
            "/help"
        );
    }

    #[test]
    fn normalizes_skills_search_query_prefixes() {
        assert_eq!(parse_skills_query(""), None);
        assert_eq!(parse_skills_query("search   "), None);
        assert_eq!(parse_skills_query("search echo skill"), Some("echo skill".to_string()));
        assert_eq!(parse_skills_query("find travel"), Some("travel".to_string()));
        assert_eq!(parse_skills_query("echo"), Some("echo".to_string()));
    }

    #[test]
    fn computes_wechat_signature_with_sorted_parts() {
        let signature = compute_wechat_signature("token123", "1710000000", "xyz");
        assert_eq!(signature.len(), 40);
        assert_eq!(
            signature,
            compute_wechat_signature("token123", "1710000000", "xyz")
        );
    }
}
