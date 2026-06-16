use anyhow::Context;
use std::sync::Arc;

use aes::Aes256;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::NoPadding};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use tokio::time::{Duration, sleep};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    a2a::{self, Task},
    app_state::{
        AgentExperienceListFilter, AgentExperienceRecord, AppState, ChatAutomationMode,
        ChatAutomationModeRecord, ChatChannelIdentityRecord, ChatChannelIdentityStatus,
        ChatIngressEventRecord, ChatIngressStatus, NodeCommandStatus, unix_timestamp_ms,
    },
    connectors::{self, ChatDispatchRequest, OpenAIResponseRequest},
    control_plane, identity, qgis, skill_registry,
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
    feishu_event_signature_configured: bool,
    dingtalk_callback_token_configured: bool,
    dingtalk_callback_encryption_configured: bool,
    wecom_callback_token_configured: bool,
    wecom_callback_encryption_configured: bool,
    wechat_official_account_token_configured: bool,
    wechat_official_account_encryption_configured: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalActionIntent {
    BrowserOpen {
        target: String,
    },
    DesktopNotification {
        message: String,
    },
    DesktopSnapshot {
        include_screenshot: bool,
    },
    DesktopMousePosition,
    DesktopMouseMove {
        x: i32,
        y: i32,
    },
    DesktopMouseClick {
        x: Option<i32>,
        y: Option<i32>,
        button: String,
        double_click: bool,
    },
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
    #[serde(alias = "msg_signature")]
    msg_signature: Option<String>,
    timestamp: Option<String>,
    nonce: Option<String>,
    echostr: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeChatOfficialAccountVerifyQuery {
    signature: Option<String>,
    #[serde(alias = "msg_signature")]
    msg_signature: Option<String>,
    timestamp: Option<String>,
    nonce: Option<String>,
    echostr: Option<String>,
    #[serde(alias = "encrypt_type")]
    encrypt_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DingTalkCallbackQuery {
    signature: Option<String>,
    timestamp: Option<String>,
    nonce: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    let management = Router::new()
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
        .route_layer(middleware::from_fn(
            crate::security::require_local_or_admin_token,
        ));

    let webhooks = Router::new()
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
        .route("/qq/events", post(qq_events));

    management.merge(webhooks)
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
        feishu_event_signature_configured: configured_optional_multi_secret(&[
            "FEISHU_EVENT_ENCRYPT_KEY",
            "DAWN_FEISHU_EVENT_ENCRYPT_KEY",
        ])
        .is_some(),
        dingtalk_callback_token_configured: configured_optional_multi_secret(&[
            "DAWN_DINGTALK_CALLBACK_TOKEN",
            "DINGTALK_CALLBACK_TOKEN",
        ])
        .is_some(),
        dingtalk_callback_encryption_configured: configured_optional_multi_secret(&[
            "DAWN_DINGTALK_ENCODING_AES_KEY",
            "DINGTALK_ENCODING_AES_KEY",
        ])
        .is_some(),
        wecom_callback_token_configured: configured_optional_multi_secret(&[
            "DAWN_WECOM_CALLBACK_TOKEN",
            "WECOM_CALLBACK_TOKEN",
        ])
        .is_some(),
        wecom_callback_encryption_configured: configured_optional_multi_secret(&[
            "DAWN_WECOM_ENCODING_AES_KEY",
            "WECOM_ENCODING_AES_KEY",
        ])
        .is_some(),
        wechat_official_account_token_configured: std::env::var(
            "DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN",
        )
        .is_ok(),
        wechat_official_account_encryption_configured: configured_optional_multi_secret(&[
            "WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY",
            "DAWN_WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY",
        ])
        .is_some(),
        qq_bot_callback_secret_configured: configured_optional_multi_secret(&[
            "DAWN_QQ_BOT_CALLBACK_SECRET",
            "QQ_BOT_CLIENT_SECRET",
        ])
        .is_some(),
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
    headers: HeaderMap,
    Json(update): Json<TelegramUpdate>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_telegram_secret(&secret, &headers).map_err(bad_request)?;
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
    headers: HeaderMap,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let payload = verify_and_decode_feishu_event(&headers, &body).map_err(bad_request)?;
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
    Query(query): Query<DingTalkCallbackQuery>,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (payload, encrypted_response) =
        verify_and_decode_dingtalk_event(&query, &body).map_err(bad_request)?;

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

    if encrypted_response {
        return Ok(Json(
            encrypt_dingtalk_success_response().map_err(service_error)?,
        ));
    }

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
    verify_and_decode_wecom_echostr(&query).map_err(bad_request)
}

async fn wecom_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WeComVerifyQuery>,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let payload = verify_and_decode_wecom_event(&query, &body).map_err(bad_request)?;

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
    verify_and_decode_wechat_official_account_echostr(&query).map_err(plain_bad_request)
}

async fn wechat_official_account_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WeChatOfficialAccountVerifyQuery>,
    body: String,
) -> Result<String, (StatusCode, String)> {
    let decoded_body =
        verify_and_decode_wechat_official_account_body(&query, &body).map_err(plain_bad_request)?;
    let payload = parse_wechat_official_account_xml(&decoded_body)
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
            "rawXml": decoded_body
        }),
        true,
    )
    .await
    .map_err(plain_service_error)?;

    Ok("success".to_string())
}

async fn qq_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let payload = verify_and_decode_qq_event(&headers, &body).map_err(bad_request)?;
    if payload.get("op").and_then(Value::as_i64) == Some(13) {
        let validation = qq_validation_response(&payload).map_err(bad_request)?;
        return Ok(Json(validation));
    }
    let Some(text) = extract_qq_text(&payload) else {
        if let Some(plain_token) = payload.pointer("/d/plain_token").and_then(Value::as_str) {
            let validation = qq_validation_response_for_values(
                plain_token,
                payload
                    .pointer("/d/event_ts")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .map_err(bad_request)?;
            return Ok(Json(validation));
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
                warn!(
                    ?error,
                    platform, "failed to deliver pending-pairing ingress reply"
                );
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
                warn!(
                    ?error,
                    platform, "failed to deliver mode-aware ingress reply"
                );
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
                    record.error = Some(format!(
                        "default model reply generated but dispatch failed: {error}"
                    ));
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
            Ok(None) => {
                let reply = no_live_model_reply(&state).await?;
                if let Err(error) =
                    dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                        .await
                {
                    warn!(?error, platform, "failed to deliver no-model reply");
                    record.reply_text = Some(reply);
                    record.status = ChatIngressStatus::Failed;
                    record.error = Some(format!(
                        "no-model reply generated but dispatch failed: {error}"
                    ));
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
            Err(error) => {
                warn!(
                    ?error,
                    platform, "default model reply failed for chat ingress"
                );
                let reply = render_model_failure_reply(&error);
                if let Err(dispatch_error) =
                    dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply)
                        .await
                {
                    warn!(
                        ?dispatch_error,
                        platform, "failed to deliver default model failure reply"
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
                warn!(
                    ?error,
                    platform, "failed to deliver task-created ingress reply"
                );
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
                    platform, "failed to deliver ingress routing failure reply"
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
    if matches!(
        platform,
        "feishu" | "dingtalk" | "wechat_official_account" | "qq"
    ) {
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
                if qgis::is_qgis_native_skill(&skill.skill_id) {
                    let Some(arguments) = parsed.arguments.as_deref() else {
                        let reply = skill_registry::native_builtin_skill_usage(&skill.skill_id)
                            .unwrap_or_else(|| {
                                format!(
                                    "技能 `{}` 是 Dawn 的原生内置技能，当前本机已经可用。",
                                    skill.display_name
                                )
                            });
                        return Ok(IngressCommandResult::Reply(reply));
                    };
                    let instruction = build_qgis_native_instruction(
                        platform,
                        record,
                        &skill.skill_id,
                        arguments,
                    )?;
                    let mut task_name = format!("{platform} native {}", skill.display_name);
                    if let Some(chat_id) = record.chat_id.as_deref() {
                        task_name.push_str(&format!(" ({chat_id})"));
                    }
                    return Ok(IngressCommandResult::Task {
                        instruction,
                        task_name,
                    });
                }
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
            let live = live_model_provider_candidates(&workspace.default_model_providers);
            Ok(IngressCommandResult::Reply(format!(
                "当前默认模型: {}。\n可用对话模型: {}。",
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
            let Some(chat_key) =
                chat_mode_key(record.chat_id.as_deref(), record.sender_id.as_deref())
            else {
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
            let live_model = live_model_provider_candidates(&workspace.default_model_providers)
                .into_iter()
                .next()
                .unwrap_or_else(|| "<none>".to_string());
            let nodes = state.list_nodes().await?;
            let connected = nodes.iter().filter(|node| node.connected).count();
            let trusted = nodes
                .iter()
                .filter(|node| node.connected && node.attestation_verified)
                .count();
            Ok(IngressCommandResult::Reply(format!(
                "工作区: {} [{}]\n当前功能等级: {}\n默认模型: {}\n可用对话模型: {}\n默认聊天: {}\n在线节点: {}，可信节点: {}。",
                workspace.display_name,
                workspace.region,
                chat_mode_label(current_mode),
                if workspace.default_model_providers.is_empty() {
                    "<none>".to_string()
                } else {
                    workspace.default_model_providers.join(", ")
                },
                live_model,
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
        "/skill <skill[@version][#function]> [参数] - 调用一个已安装技能",
        "/model - 查看当前默认模型",
        "/status - 查看工作区与节点状态",
        "桌面控制: #assist 预览，#autopilot 后可发 `看一下屏幕`、`鼠标位置`、`移动鼠标到 400,300`、`点击 400,300`",
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
        "wechat_official_account" => {
            Some("平台提示：可以直接发 `帮助`、`状态`、`技能`，也支持 `／skills`、`＃observe`。")
        }
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
        ChatAutomationMode::Chat => "仅使用默认模型回复，不主动读取电脑状态，也不执行本机动作。",
        ChatAutomationMode::Observe => {
            "允许只读观察当前电脑状态，会在需要时采样进程快照并让模型总结。"
        }
        ChatAutomationMode::Assist => "会先给出本机动作预览和安全提示；危险动作不会直接执行。",
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
    let normalized = trimmed.to_ascii_lowercase();
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
    if is_desktop_snapshot_intent(trimmed, &normalized) {
        return Some(LocalActionIntent::DesktopSnapshot {
            include_screenshot: should_include_desktop_screenshot(trimmed, &normalized),
        });
    }
    if is_mouse_position_intent(trimmed, &normalized) {
        return Some(LocalActionIntent::DesktopMousePosition);
    }
    if is_mouse_move_intent(trimmed, &normalized) {
        if let Some((x, y)) = parse_coordinate_pair(trimmed) {
            return Some(LocalActionIntent::DesktopMouseMove { x, y });
        }
    }
    if is_mouse_click_intent(trimmed, &normalized) {
        let coordinates = parse_coordinate_pair(trimmed);
        if coordinates.is_some() || mentions_current_pointer(trimmed, &normalized) {
            let (x, y) = coordinates
                .map(|(x, y)| (Some(x), Some(y)))
                .unwrap_or((None, None));
            return Some(LocalActionIntent::DesktopMouseClick {
                x,
                y,
                button: parse_desktop_mouse_button(trimmed, &normalized).to_string(),
                double_click: is_double_click_intent(trimmed, &normalized),
            });
        }
    }
    None
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}

fn is_desktop_snapshot_intent(text: &str, normalized: &str) -> bool {
    contains_any(
        text,
        &[
            "看一下屏幕",
            "看看屏幕",
            "观察屏幕",
            "屏幕快照",
            "屏幕截图",
            "截屏",
            "截图",
            "当前屏幕",
            "桌面状态",
            "屏幕状态",
        ],
    ) || contains_any(
        normalized,
        &[
            "screenshot",
            "screen shot",
            "screen snapshot",
            "desktop snapshot",
            "show screen",
            "look at screen",
        ],
    )
}

fn should_include_desktop_screenshot(text: &str, normalized: &str) -> bool {
    contains_any(
        text,
        &["看一下屏幕", "看看屏幕", "屏幕截图", "截屏", "截图"],
    ) || contains_any(
        normalized,
        &["screenshot", "screen shot", "show screen", "look at screen"],
    )
}

fn is_mouse_position_intent(text: &str, normalized: &str) -> bool {
    (contains_any(text, &["鼠标", "光标"])
        && contains_any(text, &["位置", "坐标", "在哪", "在哪里"]))
        || contains_any(
            normalized,
            &["mouse position", "cursor position", "where is the mouse"],
        )
}

fn is_mouse_move_intent(text: &str, normalized: &str) -> bool {
    (contains_any(text, &["鼠标", "光标"])
        && contains_any(text, &["移动", "移到", "移动到", "挪到"]))
        || contains_any(
            normalized,
            &["move mouse", "move cursor", "mouse move", "cursor move"],
        )
}

fn is_mouse_click_intent(text: &str, normalized: &str) -> bool {
    contains_any(
        text,
        &["点击", "点一下", "单击", "双击", "左键", "右键", "中键"],
    ) || text.starts_with("点 ")
        || contains_any(
            normalized,
            &[
                "click",
                "left click",
                "right click",
                "double click",
                "middle click",
            ],
        )
}

fn mentions_current_pointer(text: &str, normalized: &str) -> bool {
    contains_any(
        text,
        &["当前位置", "当前鼠标", "鼠标当前位置", "光标当前位置"],
    ) || contains_any(
        normalized,
        &["current position", "current mouse", "current cursor"],
    )
}

fn parse_desktop_mouse_button(text: &str, normalized: &str) -> &'static str {
    if contains_any(text, &["右键"]) || contains_any(normalized, &["right click", "secondary"]) {
        "right"
    } else if contains_any(text, &["中键", "滚轮"])
        || contains_any(normalized, &["middle click", "wheel"])
    {
        "middle"
    } else {
        "left"
    }
}

fn is_double_click_intent(text: &str, normalized: &str) -> bool {
    contains_any(text, &["双击"]) || contains_any(normalized, &["double click"])
}

fn parse_coordinate_pair(text: &str) -> Option<(i32, i32)> {
    let mut values = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || (ch == '-' && current.is_empty()) {
            current.push(ch);
        } else if !current.is_empty() {
            if current != "-" {
                if let Ok(value) = current.parse::<i32>() {
                    values.push(value);
                    if values.len() >= 2 {
                        return Some((values[0], values[1]));
                    }
                }
            }
            current.clear();
        }
    }
    if !current.is_empty() && current != "-" {
        if let Ok(value) = current.parse::<i32>() {
            values.push(value);
        }
    }
    if values.len() >= 2 {
        Some((values[0], values[1]))
    } else {
        None
    }
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
        LocalActionIntent::DesktopSnapshot { include_screenshot } => format!(
            "辅助模式已识别出桌面观察预览：将读取桌面快照{}。\n出于安全原则，辅助模式只预览不执行。发送 `#autopilot` 后重试；执行时仍会进入审批链。",
            if include_screenshot { "并保存截图" } else { "" }
        ),
        LocalActionIntent::DesktopMousePosition => {
            "辅助模式已识别出鼠标位置读取预览：将读取当前鼠标坐标。\n出于安全原则，辅助模式只预览不执行。发送 `#autopilot` 后重试；执行时仍会进入审批链。".to_string()
        }
        LocalActionIntent::DesktopMouseMove { x, y } => format!(
            "辅助模式已识别出鼠标移动预览：将鼠标移动到 ({x}, {y})。\n出于安全原则，辅助模式只预览不执行。发送 `#autopilot` 后重试；执行时仍会进入审批链。"
        ),
        LocalActionIntent::DesktopMouseClick {
            x,
            y,
            button,
            double_click,
        } => format!(
            "辅助模式已识别出鼠标点击预览：将在{}执行{}{}。\n出于安全原则，辅助模式只预览不执行。发送 `#autopilot` 后重试；执行时仍会进入审批链。",
            match (x, y) {
                (Some(x), Some(y)) => format!("坐标 ({x}, {y}) "),
                _ => "当前鼠标位置 ".to_string(),
            },
            if double_click { "双击" } else { "单击" },
            match button.as_str() {
                "right" => "右键",
                "middle" => "中键",
                _ => "左键",
            }
        ),
    }
}

async fn dispatch_guarded_desktop_command(
    state: &Arc<AppState>,
    capability: &str,
    command_type: &str,
    payload: Value,
    summary: String,
) -> anyhow::Result<String> {
    let node = select_node_for_capability(state, capability).await?;
    let (command, delivery) =
        control_plane::dispatch_gateway_command(state, &node.node_id, command_type, payload)
            .await?;
    Ok(match delivery {
        "awaiting_approval" => format!(
            "已创建桌面控制请求，等待审批：{summary}。\nnode={} commandId={}",
            node.node_id, command.command_id
        ),
        other => format!(
            "已下发桌面控制请求：{summary}。\nnode={} commandId={} delivery={}",
            node.node_id, command.command_id, other
        ),
    })
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
                    node.node_id,
                    command.command_id,
                    normalize_browser_target(&target)
                ),
                other => format!(
                    "已下发浏览器打开请求。\nnode={} commandId={} delivery={} target={}",
                    node.node_id,
                    command.command_id,
                    other,
                    normalize_browser_target(&target)
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
        LocalActionIntent::DesktopSnapshot { include_screenshot } => {
            dispatch_guarded_desktop_command(
                &state,
                "desktop_snapshot",
                "desktop_snapshot",
                json!({
                    "windowLimit": 10,
                    "includeScreenshot": include_screenshot,
                    "approvalRequired": true
                }),
                if include_screenshot {
                    "读取桌面快照并保存截图".to_string()
                } else {
                    "读取桌面快照".to_string()
                },
            )
            .await
        }
        LocalActionIntent::DesktopMousePosition => {
            dispatch_guarded_desktop_command(
                &state,
                "desktop_mouse_position",
                "desktop_mouse_position",
                json!({ "approvalRequired": true }),
                "读取当前鼠标坐标".to_string(),
            )
            .await
        }
        LocalActionIntent::DesktopMouseMove { x, y } => {
            dispatch_guarded_desktop_command(
                &state,
                "desktop_mouse_move",
                "desktop_mouse_move",
                json!({
                    "x": x,
                    "y": y,
                    "approvalRequired": true
                }),
                format!("移动鼠标到 ({x}, {y})"),
            )
            .await
        }
        LocalActionIntent::DesktopMouseClick {
            x,
            y,
            button,
            double_click,
        } => {
            let mut payload = json!({
                "button": button,
                "doubleClick": double_click,
                "approvalRequired": true
            });
            if let Value::Object(map) = &mut payload {
                if let (Some(x), Some(y)) = (x, y) {
                    map.insert("x".to_string(), json!(x));
                    map.insert("y".to_string(), json!(y));
                }
            }
            let target = match (x, y) {
                (Some(x), Some(y)) => format!("坐标 ({x}, {y})"),
                _ => "当前鼠标位置".to_string(),
            };
            let button_label = match button.as_str() {
                "right" => "右键",
                "middle" => "中键",
                _ => "左键",
            };
            dispatch_guarded_desktop_command(
                &state,
                "desktop_mouse_click",
                "desktop_mouse_click",
                payload,
                format!(
                    "在{target}执行{}{}",
                    if double_click { "双击" } else { "单击" },
                    button_label
                ),
            )
            .await
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
    let system_info =
        dispatch_and_wait_node_command(&state, &node.node_id, "system_info", json!({}))
            .await
            .ok();
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
    value
        .get("result")
        .cloned()
        .unwrap_or_else(|| value.clone())
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
    Ok(
        live_model_provider_candidates(&workspace.default_model_providers)
            .into_iter()
            .next(),
    )
}

fn live_model_provider_candidates(defaults: &[String]) -> Vec<String> {
    model_provider_candidates(defaults)
        .into_iter()
        .filter(|value| is_model_provider_live_configured(value))
        .map(ToString::to_string)
        .collect()
}

fn model_provider_candidates(defaults: &[String]) -> Vec<&str> {
    let mut candidates = Vec::new();
    for provider in defaults {
        push_unique_provider(&mut candidates, provider);
    }
    for fallback in [
        "openai_codex",
        "ollama",
        "openai",
        "anthropic",
        "google",
        "deepseek",
        "qwen",
        "zhipu",
        "moonshot",
        "doubao",
        "openrouter",
        "groq",
        "together",
        "github_models",
        "huggingface",
        "vllm",
        "mistral",
        "nvidia",
        "litellm",
        "bedrock",
        "cloudflare_ai_gateway",
        "vercel_ai_gateway",
    ] {
        push_unique_provider(&mut candidates, fallback);
    }
    candidates
}

fn push_unique_provider<'a>(providers: &mut Vec<&'a str>, provider: &'a str) {
    let normalized = provider.trim();
    if !normalized.is_empty() && !providers.iter().any(|value| *value == normalized) {
        providers.push(normalized);
    }
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
    arguments: Option<String>,
}

fn parse_skill_selector(raw: &str) -> anyhow::Result<ParsedSkillSelector> {
    let selector = raw.trim();
    if selector.is_empty() {
        anyhow::bail!("用法: /skill <skill[@version][#function]>");
    }
    let (selector, arguments) = match selector.find(char::is_whitespace) {
        Some(index) => (
            &selector[..index],
            Some(selector[index..].trim().to_string()),
        ),
        None => (selector, None),
    };
    let selector = selector
        .trim()
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
        Some((skill_id, version)) if !skill_id.trim().is_empty() && !version.trim().is_empty() => (
            skill_id.trim().to_string(),
            Some(version.trim().to_string()),
        ),
        Some((_skill_id, _version)) => anyhow::bail!("技能版本选择器格式无效"),
        None => (skill_selector.to_string(), None),
    };
    Ok(ParsedSkillSelector {
        skill_id,
        version,
        function_name,
        arguments: arguments.filter(|value| !value.is_empty()),
    })
}

fn build_skill_selector_for_task(
    skill: &skill_registry::SkillRecord,
    function: Option<&str>,
) -> String {
    match function.filter(|value| !value.trim().is_empty()) {
        Some(function) => format!("{}@{}#{}", skill.skill_id, skill.version, function.trim()),
        None => format!("{}@{}", skill.skill_id, skill.version),
    }
}

fn build_qgis_native_instruction(
    platform: &str,
    record: &ChatIngressEventRecord,
    skill_id: &str,
    raw_arguments: &str,
) -> anyhow::Result<String> {
    let payload: Value = serde_json::from_str(raw_arguments).map_err(|error| {
        anyhow::anyhow!(
            "QGIS 原生技能参数必须是 JSON 对象，例如 /skill {} {{\"projectId\":\"demo-map\"}}。解析失败: {}",
            skill_id,
            error
        )
    })?;
    let mut envelope = match payload {
        Value::Object(map) => Value::Object(map),
        _ => anyhow::bail!("QGIS 原生技能参数必须是 JSON 对象"),
    };
    let object = envelope
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("QGIS 原生技能参数必须是 JSON 对象"))?;
    object.insert("skillId".to_string(), Value::String(skill_id.to_string()));
    object.insert(
        "auth".to_string(),
        json!({
            "actor": ingress_actor_identity(platform, record),
            "role": "user",
            "scopes": qgis::default_scopes_for_skill(skill_id).unwrap_or_default(),
        }),
    );
    if !object.contains_key("requestId") {
        object.insert(
            "requestId".to_string(),
            Value::String(format!("ingress:{}", record.ingress_id)),
        );
    }
    if !object.contains_key("idempotencyKey") {
        object.insert(
            "idempotencyKey".to_string(),
            Value::String(format!("ingress:{}:{}", record.ingress_id, skill_id)),
        );
    }
    let _parsed: qgis::QgisSkillInvocationEnvelope = serde_json::from_value(envelope.clone())
        .map_err(|error| anyhow::anyhow!("QGIS 原生技能参数无效: {error}"))?;
    Ok(format!(
        "native:{}",
        serde_json::to_string(&envelope).context("failed to serialize qgis native envelope")?
    ))
}

fn ingress_actor_identity(platform: &str, record: &ChatIngressEventRecord) -> String {
    record
        .sender_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(|value| format!("{platform}:{value}"))
        .or_else(|| {
            record
                .sender_display
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(|value| format!("{platform}:{value}"))
        })
        .or_else(|| {
            record
                .chat_id
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(|value| format!("{platform}:chat:{value}"))
        })
        .unwrap_or_else(|| format!("{platform}:ingress"))
}

fn should_attempt_default_model_reply(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('#') {
        return false;
    }
    let normalized = trimmed.to_ascii_lowercase();
    is_conversational_message(trimmed, &normalized)
        && !looks_like_task_request(trimmed, &normalized)
}

fn is_conversational_message(text: &str, normalized: &str) -> bool {
    contains_any(
        text,
        &[
            "你是谁",
            "你在吗",
            "在吗",
            "你好",
            "您好",
            "嗨",
            "哈喽",
            "谢谢",
            "多谢",
            "早上好",
            "晚上好",
            "你叫什么",
            "你能做什么",
            "介绍一下你自己",
            "可以聊天",
            "陪我聊",
        ],
    ) || [
        "hi",
        "hello",
        "hey",
        "thanks",
        "thank you",
        "who are you",
        "what are you",
        "what can you do",
        "how are you",
        "are you there",
        "tell me about yourself",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix))
        || text.ends_with('?')
        || text.ends_with('？')
}

fn looks_like_task_request(text: &str, normalized: &str) -> bool {
    contains_any(
        text,
        &[
            "打开",
            "启动",
            "进入",
            "搜索",
            "点击",
            "控制",
            "读取",
            "执行",
            "创建",
            "帮我",
            "测试",
            "订",
            "预订",
            "总结",
            "发送",
            "提醒",
            "通知",
            "分析",
            "生成",
            "上传",
            "下载",
            "删除",
            "清理",
            "运行",
            "调用",
            "安装",
            "查找",
            "整理",
            "修改",
            "移动",
            "拖动",
            "截图",
            "截屏",
            "鼠标",
            "键盘",
            "文件",
            "浏览器",
            "电脑",
            "程序",
            "任务",
            "审批",
            "支付",
        ],
    ) || contains_any(
        normalized,
        &[
            "attachment received",
            "reaction received",
            "book ",
            "create ",
            "draft ",
            "summarize ",
            "send ",
            "open ",
            "launch ",
            "search ",
            "click ",
            "control ",
            "read ",
            "run ",
            "execute ",
            "call ",
            "install ",
            "delete ",
            "clean ",
            "download ",
            "upload ",
            "move ",
            "type ",
            "notify ",
            "remind ",
            "analyze ",
            "generate ",
            "start ",
            "stop ",
            "browser",
            "mouse",
            "keyboard",
            "file",
            "task ",
        ],
    )
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
            instructions: Some(build_default_chat_instructions(&state, platform, text).await),
        },
    )
    .await?;

    let output = response.output_text.trim().to_string();
    if output.is_empty() {
        return Ok(None);
    }
    Ok(Some(output))
}

fn render_model_failure_reply(error: &anyhow::Error) -> String {
    let mut summary = error.to_string();
    if let Some((head, _tail)) = summary.split_once("{\"stderr\"") {
        summary = head.trim().trim_end_matches(':').to_string();
    }
    if summary
        .to_ascii_lowercase()
        .contains("model is not supported")
    {
        summary = "当前 Codex 模型不支持这个账号，请移除 OPENAI_CODEX_MODEL 覆盖或改成 Codex CLI 可用模型。".to_string();
    }
    let summary: String = summary.chars().take(600).collect();
    format!("模型回复失败：{summary}\n普通聊天没有被转成任务；请检查 /model 或模型连接器配置。")
}

async fn build_default_chat_instructions(
    state: &Arc<AppState>,
    platform: &str,
    text: &str,
) -> String {
    let mut instructions = format!(
        "You are Dawn, a concise desktop AI assistant replying inside a {platform} chat. Respond directly in the user's language. Keep replies short unless the user asks for detail."
    );
    match relevant_experience_context(state, text).await {
        Ok(Some(context)) => {
            instructions
                .push_str("\n\nRelevant learned experiences from this local Dawn workspace:\n");
            instructions.push_str(&context);
            instructions.push_str(
                "\nUse these records only as operational hints. Do not quote internal experience ids, do not claim certainty from them, and do not perform actions outside the current chat mode.",
            );
        }
        Ok(None) => {}
        Err(error) => {
            warn!(?error, "failed to load learned experiences for chat reply");
        }
    }
    instructions
}

async fn relevant_experience_context(
    state: &Arc<AppState>,
    text: &str,
) -> anyhow::Result<Option<String>> {
    let Some(query) = experience_query_for_text(text) else {
        return Ok(None);
    };
    let experiences = state
        .list_agent_experiences(AgentExperienceListFilter {
            limit: Some(3),
            query: Some(query),
            ..Default::default()
        })
        .await?;
    Ok(render_experience_context(&experiences))
}

fn experience_query_for_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.len() < 2 {
        return None;
    }
    Some(trimmed.chars().take(80).collect())
}

fn render_experience_context(experiences: &[AgentExperienceRecord]) -> Option<String> {
    if experiences.is_empty() {
        return None;
    }
    let lines = experiences
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, experience)| {
            let hint = experience
                .reusable_hint
                .as_deref()
                .map(truncate_experience_fragment)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "none".to_string());
            let tags = if experience.tags.is_empty() {
                "none".to_string()
            } else {
                experience.tags.join(",")
            };
            format!(
                "{}. kind={}, outcome={}, risk={}, lesson={}, hint={}, tags={}",
                index + 1,
                truncate_experience_fragment(&experience.task_kind),
                truncate_experience_fragment(&experience.outcome),
                truncate_experience_fragment(&experience.risk_level),
                truncate_experience_fragment(&experience.lesson),
                hint,
                truncate_experience_fragment(&tags)
            )
        })
        .collect::<Vec<_>>();
    Some(lines.join("\n"))
}

fn truncate_experience_fragment(value: &str) -> String {
    value.trim().chars().take(240).collect()
}

async fn no_live_model_reply(state: &Arc<AppState>) -> anyhow::Result<String> {
    let workspace = identity::ensure_workspace_profile(state).await?;
    let defaults = if workspace.default_model_providers.is_empty() {
        "<none>".to_string()
    } else {
        workspace.default_model_providers.join(", ")
    };
    Ok(format!(
        "我现在能收到你的消息，但还没有可用的默认对话模型。\n当前默认模型: {defaults}。\n请配置该模型凭据，或把默认模型切到已登录的 `openai_codex` / 本地 `ollama`。普通聊天不会再被自动转成任务。"
    ))
}

fn is_model_provider_live_configured(provider: &str) -> bool {
    match provider {
        "openai" => std::env::var("OPENAI_API_KEY").is_ok(),
        "openai_codex" => connectors::openai_codex_login_ready(),
        "anthropic" => std::env::var("ANTHROPIC_API_KEY").is_ok(),
        "google" => {
            std::env::var("GEMINI_API_KEY").is_ok() || std::env::var("GOOGLE_API_KEY").is_ok()
        }
        "bedrock" => {
            std::env::var("BEDROCK_API_KEY").is_ok()
                && (std::env::var("BEDROCK_CHAT_COMPLETIONS_URL").is_ok()
                    || std::env::var("BEDROCK_BASE_URL").is_ok()
                    || std::env::var("BEDROCK_RUNTIME_ENDPOINT").is_ok())
        }
        "cloudflare_ai_gateway" => {
            (std::env::var("CLOUDFLARE_AI_GATEWAY_API_KEY").is_ok()
                || std::env::var("OPENAI_API_KEY").is_ok())
                && (std::env::var("CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL").is_ok()
                    || std::env::var("CLOUDFLARE_AI_GATEWAY_BASE_URL").is_ok()
                    || (std::env::var("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID").is_ok()
                        && std::env::var("CLOUDFLARE_AI_GATEWAY_ID").is_ok()))
        }
        "github_models" => {
            std::env::var("GITHUB_MODELS_API_KEY").is_ok() || std::env::var("GITHUB_TOKEN").is_ok()
        }
        "huggingface" => {
            std::env::var("HUGGINGFACE_API_KEY").is_ok() || std::env::var("HF_TOKEN").is_ok()
        }
        "openrouter" => std::env::var("OPENROUTER_API_KEY").is_ok(),
        "groq" => std::env::var("GROQ_API_KEY").is_ok(),
        "together" => std::env::var("TOGETHER_API_KEY").is_ok(),
        "vercel_ai_gateway" => {
            std::env::var("VERCEL_AI_GATEWAY_API_KEY").is_ok()
                || std::env::var("AI_GATEWAY_API_KEY").is_ok()
                || std::env::var("VERCEL_AI_GATEWAY_BASE_URL").is_ok()
                || std::env::var("VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL").is_ok()
        }
        "vllm" => {
            std::env::var("VLLM_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("VLLM_BASE_URL").is_ok()
        }
        "mistral" => std::env::var("MISTRAL_API_KEY").is_ok(),
        "nvidia" => {
            std::env::var("NVIDIA_API_KEY").is_ok() || std::env::var("NVIDIA_NIM_API_KEY").is_ok()
        }
        "litellm" => {
            std::env::var("LITELLM_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("LITELLM_BASE_URL").is_ok()
        }
        "deepseek" => std::env::var("DEEPSEEK_API_KEY").is_ok(),
        "qwen" => {
            std::env::var("QWEN_API_KEY").is_ok() || std::env::var("DASHSCOPE_API_KEY").is_ok()
        }
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

type Aes256CbcDecryptor = cbc::Decryptor<Aes256>;
type Aes256CbcEncryptor = cbc::Encryptor<Aes256>;

fn verify_telegram_secret(secret: &str, headers: &HeaderMap) -> anyhow::Result<()> {
    let Some(expected) = configured_ingress_secret("DAWN_TELEGRAM_WEBHOOK_SECRET", "telegram")?
    else {
        return Ok(());
    };
    let header_secret = header_str(headers, "x-telegram-bot-api-secret-token");
    if !telegram_secret_matches(&expected, secret, header_secret) {
        anyhow::bail!("telegram webhook secret mismatch");
    }
    Ok(())
}

fn telegram_secret_matches(expected: &str, path_secret: &str, header_secret: Option<&str>) -> bool {
    constant_time_str_eq(expected, path_secret)
        || header_secret
            .map(|value| constant_time_str_eq(expected, value))
            .unwrap_or(false)
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
                .get(format!(
                    "https://api.telegram.org/bot{bot_token}/getUpdates"
                ))
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
                        warn!(
                            ?error,
                            "telegram polling worker failed to decode getUpdates response"
                        );
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
    let Some(expected) = configured_ingress_secret(env_var, platform)? else {
        return Ok(());
    };
    if !constant_time_str_eq(&expected, secret) {
        anyhow::bail!("{platform} callback secret mismatch");
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
    let Some(token) = configured_ingress_secret(
        "DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN",
        "wechat official account",
    )?
    else {
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
    if !constant_time_str_eq(&expected, signature) {
        anyhow::bail!("wechat signature mismatch");
    }
    Ok(())
}

fn compute_wechat_signature(token: &str, timestamp: &str, nonce: &str) -> String {
    compute_sorted_sha1_signature(&[token, timestamp, nonce])
}

fn verify_and_decode_feishu_event(headers: &HeaderMap, body: &str) -> anyhow::Result<Value> {
    let Some(encrypt_key) = configured_multi_ingress_secret(
        &["FEISHU_EVENT_ENCRYPT_KEY", "DAWN_FEISHU_EVENT_ENCRYPT_KEY"],
        "feishu",
    )?
    else {
        return serde_json::from_str(body).context("failed to parse unsigned feishu payload");
    };

    verify_feishu_signature(headers, &encrypt_key, body)?;
    let raw_payload: Value =
        serde_json::from_str(body).context("failed to parse feishu payload")?;
    let payload = if let Some(encrypt) = raw_payload.get("encrypt").and_then(Value::as_str) {
        let decrypted = decrypt_feishu_event(encrypt, &encrypt_key)?;
        serde_json::from_str(&decrypted).context("failed to parse decrypted feishu payload")?
    } else {
        raw_payload
    };
    verify_feishu_verification_token(&payload)?;
    Ok(payload)
}

fn verify_feishu_signature(
    headers: &HeaderMap,
    encrypt_key: &str,
    body: &str,
) -> anyhow::Result<()> {
    let timestamp = header_str(headers, "x-lark-request-timestamp")
        .ok_or_else(|| anyhow::anyhow!("missing feishu X-Lark-Request-Timestamp header"))?;
    let nonce = header_str(headers, "x-lark-request-nonce")
        .ok_or_else(|| anyhow::anyhow!("missing feishu X-Lark-Request-Nonce header"))?;
    let actual = header_str(headers, "x-lark-signature")
        .ok_or_else(|| anyhow::anyhow!("missing feishu X-Lark-Signature header"))?;
    let expected = compute_feishu_signature(timestamp, nonce, encrypt_key, body);
    if !constant_time_str_eq(&expected, actual) {
        anyhow::bail!("feishu signature mismatch");
    }
    Ok(())
}

fn compute_feishu_signature(timestamp: &str, nonce: &str, encrypt_key: &str, body: &str) -> String {
    let mut sha = Sha256::new();
    sha.update(timestamp.as_bytes());
    sha.update(nonce.as_bytes());
    sha.update(encrypt_key.as_bytes());
    sha.update(body.as_bytes());
    hex::encode(sha.finalize())
}

fn decrypt_feishu_event(encrypt: &str, encrypt_key: &str) -> anyhow::Result<String> {
    let ciphertext = BASE64_STANDARD
        .decode(encrypt)
        .context("feishu encrypt field is not valid base64")?;
    if ciphertext.len() < 32 || ciphertext.len() % 16 != 0 {
        anyhow::bail!("feishu ciphertext length is invalid");
    }
    let key = Sha256::digest(encrypt_key.as_bytes());
    let iv: [u8; 16] = ciphertext[..16]
        .try_into()
        .map_err(|_| anyhow::anyhow!("feishu ciphertext IV is invalid"))?;
    let key: [u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("feishu derived key is invalid"))?;
    let decrypted = aes256_cbc_decrypt_no_padding(&key, &iv, &ciphertext[16..])?;
    extract_json_object(&decrypted).context("decrypted feishu payload did not contain JSON")
}

fn verify_feishu_verification_token(payload: &Value) -> anyhow::Result<()> {
    let Some(expected) = configured_optional_multi_secret(&[
        "FEISHU_VERIFICATION_TOKEN",
        "DAWN_FEISHU_VERIFICATION_TOKEN",
    ]) else {
        return Ok(());
    };
    let actual = payload
        .pointer("/header/token")
        .and_then(Value::as_str)
        .or_else(|| payload.get("token").and_then(Value::as_str))
        .ok_or_else(|| anyhow::anyhow!("missing feishu verification token"))?;
    if !constant_time_str_eq(&expected, actual) {
        anyhow::bail!("feishu verification token mismatch");
    }
    Ok(())
}

fn verify_and_decode_dingtalk_event(
    query: &DingTalkCallbackQuery,
    body: &str,
) -> anyhow::Result<(Value, bool)> {
    let raw_payload: Value =
        serde_json::from_str(body).context("failed to parse dingtalk payload")?;
    if let Some(encrypt) = raw_payload.get("encrypt").and_then(Value::as_str) {
        verify_dingtalk_encrypted_signature(query, encrypt)?;
        let message = decrypt_wechat_style_message(
            &required_multi_secret(
                &[
                    "DAWN_DINGTALK_ENCODING_AES_KEY",
                    "DINGTALK_ENCODING_AES_KEY",
                ],
                "dingtalk EncodingAESKey",
            )?,
            encrypt,
        )?;
        let payload =
            serde_json::from_str(&message).context("failed to parse decrypted dingtalk payload")?;
        return Ok((payload, true));
    }

    verify_dingtalk_callback_token(&raw_payload).map(|()| (raw_payload, false))
}

fn verify_dingtalk_encrypted_signature(
    query: &DingTalkCallbackQuery,
    encrypt: &str,
) -> anyhow::Result<()> {
    let token = required_multi_secret(
        &["DAWN_DINGTALK_CALLBACK_TOKEN", "DINGTALK_CALLBACK_TOKEN"],
        "dingtalk callback token",
    )?;
    let timestamp = query
        .timestamp
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing dingtalk timestamp"))?;
    let nonce = query
        .nonce
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing dingtalk nonce"))?;
    let actual = query
        .signature
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing dingtalk signature"))?;
    let expected = compute_sorted_sha1_signature(&[&token, timestamp, nonce, encrypt]);
    if !constant_time_str_eq(&expected, actual) {
        anyhow::bail!("dingtalk signature mismatch");
    }
    Ok(())
}

fn encrypt_dingtalk_success_response() -> anyhow::Result<Value> {
    let token = required_multi_secret(
        &["DAWN_DINGTALK_CALLBACK_TOKEN", "DINGTALK_CALLBACK_TOKEN"],
        "dingtalk callback token",
    )?;
    let aes_key = required_multi_secret(
        &[
            "DAWN_DINGTALK_ENCODING_AES_KEY",
            "DINGTALK_ENCODING_AES_KEY",
        ],
        "dingtalk EncodingAESKey",
    )?;
    let timestamp = (unix_timestamp_ms() / 1000).to_string();
    let nonce = Uuid::new_v4().simple().to_string();
    let encrypt = encrypt_wechat_style_message(&aes_key, "success", "")?;
    let signature = compute_sorted_sha1_signature(&[&token, &timestamp, &nonce, &encrypt]);
    Ok(json!({
        "msg_signature": signature,
        "timeStamp": timestamp,
        "nonce": nonce,
        "encrypt": encrypt
    }))
}

fn verify_and_decode_wecom_echostr(query: &WeComVerifyQuery) -> anyhow::Result<String> {
    let echostr = query
        .echostr
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing echostr query parameter"))?;
    if query.msg_signature.is_some() {
        verify_wechat_style_signature_query(
            "wecom",
            &["DAWN_WECOM_CALLBACK_TOKEN", "WECOM_CALLBACK_TOKEN"],
            query.msg_signature.as_deref(),
            query.timestamp.as_deref(),
            query.nonce.as_deref(),
            echostr,
        )?;
        return decrypt_wechat_style_message(
            &required_multi_secret(
                &["DAWN_WECOM_ENCODING_AES_KEY", "WECOM_ENCODING_AES_KEY"],
                "wecom EncodingAESKey",
            )?,
            echostr,
        );
    }
    if allow_unauthenticated_ingress_for_development() {
        return Ok(echostr.to_string());
    }
    anyhow::bail!("missing wecom msg_signature for callback URL verification");
}

fn verify_and_decode_wecom_event(query: &WeComVerifyQuery, body: &str) -> anyhow::Result<Value> {
    if let Some(encrypt) = extract_xml_tag(body, "Encrypt") {
        verify_wechat_style_signature_query(
            "wecom",
            &["DAWN_WECOM_CALLBACK_TOKEN", "WECOM_CALLBACK_TOKEN"],
            query.msg_signature.as_deref(),
            query.timestamp.as_deref(),
            query.nonce.as_deref(),
            &encrypt,
        )?;
        let message = decrypt_wechat_style_message(
            &required_multi_secret(
                &["DAWN_WECOM_ENCODING_AES_KEY", "WECOM_ENCODING_AES_KEY"],
                "wecom EncodingAESKey",
            )?,
            &encrypt,
        )?;
        return Ok(wecom_xml_to_payload(&message));
    }

    let payload: Value = serde_json::from_str(body).context("failed to parse wecom payload")?;
    verify_wecom_callback_token(&payload)?;
    Ok(payload)
}

fn verify_and_decode_wechat_official_account_echostr(
    query: &WeChatOfficialAccountVerifyQuery,
) -> anyhow::Result<String> {
    let echostr = query
        .echostr
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing echostr query parameter"))?;
    if query.msg_signature.is_some() || query.encrypt_type.as_deref() == Some("aes") {
        verify_wechat_style_signature_query(
            "wechat official account",
            &["DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN"],
            query.msg_signature.as_deref(),
            query.timestamp.as_deref(),
            query.nonce.as_deref(),
            echostr,
        )?;
        return decrypt_wechat_style_message(
            &required_multi_secret(
                &[
                    "WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY",
                    "DAWN_WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY",
                ],
                "wechat official account EncodingAESKey",
            )?,
            echostr,
        );
    }
    verify_wechat_official_account_query(query)?;
    Ok(echostr.to_string())
}

fn verify_and_decode_wechat_official_account_body(
    query: &WeChatOfficialAccountVerifyQuery,
    body: &str,
) -> anyhow::Result<String> {
    if let Some(encrypt) = extract_xml_tag(body, "Encrypt") {
        verify_wechat_style_signature_query(
            "wechat official account",
            &["DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN"],
            query.msg_signature.as_deref(),
            query.timestamp.as_deref(),
            query.nonce.as_deref(),
            &encrypt,
        )?;
        return decrypt_wechat_style_message(
            &required_multi_secret(
                &[
                    "WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY",
                    "DAWN_WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY",
                ],
                "wechat official account EncodingAESKey",
            )?,
            &encrypt,
        );
    }
    verify_wechat_official_account_query(query)?;
    Ok(body.to_string())
}

fn verify_and_decode_qq_event(headers: &HeaderMap, body: &str) -> anyhow::Result<Value> {
    if let Some(secret) = configured_qq_callback_secret()? {
        verify_qq_callback_signature(headers, body.as_bytes(), &secret)?;
    }
    serde_json::from_str(body).context("failed to parse qq payload")
}

fn configured_qq_callback_secret() -> anyhow::Result<Option<String>> {
    configured_multi_ingress_secret(
        &["DAWN_QQ_BOT_CALLBACK_SECRET", "QQ_BOT_CLIENT_SECRET"],
        "qq",
    )
}

fn verify_qq_callback_signature(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
) -> anyhow::Result<()> {
    let signature_hex = header_str(headers, "x-signature-ed25519")
        .ok_or_else(|| anyhow::anyhow!("missing qq X-Signature-Ed25519 header"))?;
    let timestamp = header_str(headers, "x-signature-timestamp")
        .ok_or_else(|| anyhow::anyhow!("missing qq X-Signature-Timestamp header"))?;
    let signature_bytes = hex::decode(signature_hex).context("qq signature is not valid hex")?;
    let signature_bytes: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("qq signature must be 64 bytes"))?;
    let signature = Signature::from_bytes(&signature_bytes);
    let public_key = qq_verifying_key_from_secret(secret)?;
    let mut message = timestamp.as_bytes().to_vec();
    message.extend_from_slice(body);
    public_key
        .verify(&message, &signature)
        .context("qq signature verification failed")
}

fn qq_validation_response(payload: &Value) -> anyhow::Result<Value> {
    let plain_token = payload
        .pointer("/d/plain_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing qq validation plain_token"))?;
    let event_ts = payload
        .pointer("/d/event_ts")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing qq validation event_ts"))?;
    qq_validation_response_for_values(plain_token, event_ts)
}

fn qq_validation_response_for_values(plain_token: &str, event_ts: &str) -> anyhow::Result<Value> {
    let secret = configured_qq_callback_secret()?
        .ok_or_else(|| anyhow::anyhow!("qq callback secret is required for URL validation"))?;
    let signing_key = qq_signing_key_from_secret(&secret)?;
    let mut message = event_ts.as_bytes().to_vec();
    message.extend_from_slice(plain_token.as_bytes());
    let signature = signing_key.sign(&message);
    Ok(json!({
        "plain_token": plain_token,
        "signature": hex::encode(signature.to_bytes())
    }))
}

fn qq_signing_key_from_secret(secret: &str) -> anyhow::Result<SigningKey> {
    Ok(SigningKey::from_bytes(&qq_seed_from_secret(secret)?))
}

fn qq_verifying_key_from_secret(secret: &str) -> anyhow::Result<VerifyingKey> {
    Ok(VerifyingKey::from(&qq_signing_key_from_secret(secret)?))
}

fn qq_seed_from_secret(secret: &str) -> anyhow::Result<[u8; 32]> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        anyhow::bail!("qq callback secret cannot be empty");
    }
    let mut seed = trimmed.to_string();
    while seed.len() < 32 {
        seed.push_str(trimmed);
    }
    seed.as_bytes()[..32]
        .try_into()
        .map_err(|_| anyhow::anyhow!("failed to derive qq seed"))
}

fn verify_wechat_style_signature_query(
    platform: &str,
    token_env_names: &[&str],
    actual_signature: Option<&str>,
    timestamp: Option<&str>,
    nonce: Option<&str>,
    encrypt: &str,
) -> anyhow::Result<()> {
    let token = required_multi_secret(token_env_names, platform)?;
    let timestamp = timestamp.ok_or_else(|| anyhow::anyhow!("missing {platform} timestamp"))?;
    let nonce = nonce.ok_or_else(|| anyhow::anyhow!("missing {platform} nonce"))?;
    let actual_signature =
        actual_signature.ok_or_else(|| anyhow::anyhow!("missing {platform} msg_signature"))?;
    let expected = compute_sorted_sha1_signature(&[&token, timestamp, nonce, encrypt]);
    if !constant_time_str_eq(&expected, actual_signature) {
        anyhow::bail!("{platform} msg_signature mismatch");
    }
    Ok(())
}

fn decrypt_wechat_style_message(aes_key: &str, encrypt: &str) -> anyhow::Result<String> {
    let key = decode_platform_encoding_aes_key(aes_key)?;
    let iv: [u8; 16] = key[..16]
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid EncodingAESKey IV"))?;
    let ciphertext = BASE64_STANDARD
        .decode(encrypt)
        .context("encrypted payload is not valid base64")?;
    let decrypted = aes256_cbc_decrypt_no_padding(&key, &iv, &ciphertext)?;
    let unpadded = remove_wechat_pkcs7_padding(&decrypted)?;
    if unpadded.len() < 20 {
        anyhow::bail!("decrypted payload is too short");
    }
    let msg_len = u32::from_be_bytes(
        unpadded[16..20]
            .try_into()
            .map_err(|_| anyhow::anyhow!("decrypted payload length prefix is invalid"))?,
    ) as usize;
    let msg_start = 20;
    let msg_end = msg_start + msg_len;
    if msg_end > unpadded.len() {
        anyhow::bail!("decrypted payload message length is invalid");
    }
    String::from_utf8(unpadded[msg_start..msg_end].to_vec())
        .context("decrypted payload message is not valid utf-8")
}

fn encrypt_wechat_style_message(
    aes_key: &str,
    message: &str,
    receive_id: &str,
) -> anyhow::Result<String> {
    let key = decode_platform_encoding_aes_key(aes_key)?;
    let iv: [u8; 16] = key[..16]
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid EncodingAESKey IV"))?;
    let mut plain = vec![0_u8; 16];
    rand::thread_rng().fill_bytes(&mut plain);
    plain.extend_from_slice(&(message.len() as u32).to_be_bytes());
    plain.extend_from_slice(message.as_bytes());
    plain.extend_from_slice(receive_id.as_bytes());
    add_wechat_pkcs7_padding(&mut plain);
    let encrypted = aes256_cbc_encrypt_no_padding(&key, &iv, &plain)?;
    Ok(BASE64_STANDARD.encode(encrypted))
}

fn decode_platform_encoding_aes_key(raw: &str) -> anyhow::Result<[u8; 32]> {
    let value = raw.trim();
    let padded = if value.len() % 4 == 0 {
        value.to_string()
    } else {
        format!("{value}{}", "=".repeat(4 - value.len() % 4))
    };
    let bytes = BASE64_STANDARD
        .decode(padded)
        .context("EncodingAESKey is not valid base64")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("EncodingAESKey must decode to 32 bytes"))
}

fn aes256_cbc_decrypt_no_padding(
    key: &[u8; 32],
    iv: &[u8; 16],
    ciphertext: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        anyhow::bail!("ciphertext length must be a non-empty AES block multiple");
    }
    let mut buffer = ciphertext.to_vec();
    let decrypted = Aes256CbcDecryptor::new(key.into(), iv.into())
        .decrypt_padded_mut::<NoPadding>(&mut buffer)
        .map_err(|_| anyhow::anyhow!("AES-CBC decryption failed"))?;
    Ok(decrypted.to_vec())
}

fn aes256_cbc_encrypt_no_padding(
    key: &[u8; 32],
    iv: &[u8; 16],
    plaintext: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if plaintext.is_empty() || plaintext.len() % 16 != 0 {
        anyhow::bail!("plaintext length must be a non-empty AES block multiple");
    }
    let mut buffer = plaintext.to_vec();
    let len = buffer.len();
    let encrypted = Aes256CbcEncryptor::new(key.into(), iv.into())
        .encrypt_padded_mut::<NoPadding>(&mut buffer, len)
        .map_err(|_| anyhow::anyhow!("AES-CBC encryption failed"))?;
    Ok(encrypted.to_vec())
}

fn remove_wechat_pkcs7_padding(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let Some(&padding) = bytes.last() else {
        anyhow::bail!("empty padded payload");
    };
    let padding = padding as usize;
    if padding == 0 || padding > 32 || padding > bytes.len() {
        anyhow::bail!("invalid PKCS7 padding");
    }
    if !bytes[bytes.len() - padding..]
        .iter()
        .all(|value| *value as usize == padding)
    {
        anyhow::bail!("invalid PKCS7 padding bytes");
    }
    Ok(bytes[..bytes.len() - padding].to_vec())
}

fn add_wechat_pkcs7_padding(bytes: &mut Vec<u8>) {
    let mut padding = 32 - (bytes.len() % 32);
    if padding == 0 {
        padding = 32;
    }
    bytes.extend(std::iter::repeat(padding as u8).take(padding));
}

fn compute_sorted_sha1_signature(parts: &[&str]) -> String {
    let mut parts = parts.to_vec();
    parts.sort_unstable();
    let mut sha = Sha1::new();
    sha.update(parts.concat().as_bytes());
    hex::encode(sha.finalize())
}

fn extract_json_object(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end >= start).then(|| text[start..=end].to_string())
}

fn wecom_xml_to_payload(xml: &str) -> Value {
    let msg_type = extract_xml_tag(xml, "MsgType").unwrap_or_else(|| "event".to_string());
    json!({
        "msgtype": msg_type,
        "text": {
            "content": extract_xml_tag(xml, "Content").unwrap_or_default()
        },
        "content": extract_xml_tag(xml, "Content"),
        "chatid": extract_xml_tag(xml, "ChatId"),
        "from": extract_xml_tag(xml, "FromUserName"),
        "sender_name": extract_xml_tag(xml, "FromUserName"),
        "ToUserName": extract_xml_tag(xml, "ToUserName"),
        "CreateTime": extract_xml_tag(xml, "CreateTime"),
        "MsgId": extract_xml_tag(xml, "MsgId"),
        "Event": extract_xml_tag(xml, "Event")
    })
}

fn verify_dingtalk_callback_token(payload: &Value) -> anyhow::Result<()> {
    let Some(expected) = configured_ingress_secret("DAWN_DINGTALK_CALLBACK_TOKEN", "dingtalk")?
    else {
        return Ok(());
    };
    let actual = payload
        .pointer("/token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing dingtalk callback token"))?;
    if !constant_time_str_eq(&expected, actual) {
        anyhow::bail!("dingtalk callback token mismatch");
    }
    Ok(())
}

fn verify_wecom_callback_token(payload: &Value) -> anyhow::Result<()> {
    let Some(expected) = configured_ingress_secret("DAWN_WECOM_CALLBACK_TOKEN", "wecom")? else {
        return Ok(());
    };
    let actual = payload
        .pointer("/token")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/ToUserName").and_then(Value::as_str))
        .ok_or_else(|| anyhow::anyhow!("missing wecom callback token"))?;
    if !constant_time_str_eq(&expected, actual) {
        anyhow::bail!("wecom callback token mismatch");
    }
    Ok(())
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn configured_optional_multi_secret(env_vars: &[&str]) -> Option<String> {
    env_vars.iter().find_map(|env_var| {
        std::env::var(env_var)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn configured_multi_ingress_secret(
    env_vars: &[&str],
    platform: &str,
) -> anyhow::Result<Option<String>> {
    if let Some(value) = configured_optional_multi_secret(env_vars) {
        return Ok(Some(value));
    }
    if allow_unauthenticated_ingress_for_development() {
        return Ok(None);
    }
    anyhow::bail!(
        "{platform} ingress secret is not configured; set one of {} or explicitly enable DAWN_ALLOW_UNAUTHENTICATED_INGRESS for local development",
        env_vars.join(", ")
    )
}

fn required_multi_secret(env_vars: &[&str], label: &str) -> anyhow::Result<String> {
    configured_optional_multi_secret(env_vars).ok_or_else(|| {
        anyhow::anyhow!(
            "{label} is not configured; set one of {}",
            env_vars.join(", ")
        )
    })
}

fn configured_ingress_secret(env_var: &str, platform: &str) -> anyhow::Result<Option<String>> {
    match std::env::var(env_var) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_string())),
        _ if allow_unauthenticated_ingress_for_development() => Ok(None),
        _ => anyhow::bail!(
            "{platform} ingress secret is not configured; set {env_var} or explicitly enable DAWN_ALLOW_UNAUTHENTICATED_INGRESS for local development"
        ),
    }
}

fn allow_unauthenticated_ingress_for_development() -> bool {
    cfg!(test)
        || std::env::var("DAWN_ALLOW_UNAUTHENTICATED_INGRESS")
            .ok()
            .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
}

fn constant_time_str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
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

    fn test_encoding_aes_key() -> String {
        BASE64_STANDARD
            .encode([7_u8; 32])
            .trim_end_matches('=')
            .to_string()
    }

    fn signed_wechat_style_query(
        token: &str,
        timestamp: &str,
        nonce: &str,
        encrypt: &str,
    ) -> String {
        let signature = compute_sorted_sha1_signature(&[token, timestamp, nonce, encrypt]);
        format!("msg_signature={signature}&timestamp={timestamp}&nonce={nonce}")
    }

    fn qq_callback_signature(secret: &str, timestamp: &str, body: &str) -> anyhow::Result<String> {
        let signing_key = qq_signing_key_from_secret(secret)?;
        let mut message = timestamp.as_bytes().to_vec();
        message.extend_from_slice(body.as_bytes());
        Ok(hex::encode(signing_key.sign(&message).to_bytes()))
    }

    #[tokio::test]
    async fn telegram_webhook_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_TELEGRAM_WEBHOOK_SECRET", None)]);
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

    #[test]
    fn telegram_secret_matching_accepts_official_header() {
        assert!(telegram_secret_matches(
            "telegram-header-secret",
            "legacy-path-secret",
            Some("telegram-header-secret")
        ));
        assert!(telegram_secret_matches(
            "telegram-header-secret",
            "telegram-header-secret",
            None
        ));
        assert!(!telegram_secret_matches(
            "telegram-header-secret",
            "wrong-path-secret",
            Some("wrong-header-secret")
        ));
    }

    #[tokio::test]
    async fn feishu_challenge_round_trip_is_supported() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("FEISHU_EVENT_ENCRYPT_KEY", None),
            ("DAWN_FEISHU_EVENT_ENCRYPT_KEY", None),
            ("FEISHU_VERIFICATION_TOKEN", None),
            ("DAWN_FEISHU_VERIFICATION_TOKEN", None),
        ]);
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
    async fn feishu_signed_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("FEISHU_EVENT_ENCRYPT_KEY", Some("feishu-encrypt-key")),
            ("FEISHU_VERIFICATION_TOKEN", Some("feishu-verify-token")),
        ]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let body = serde_json::to_string(&json!({
            "schema": "2.0",
            "header": {
                "event_type": "im.message.receive_v1",
                "token": "feishu-verify-token"
            },
            "event": {
                "message": {
                    "chat_id": "oc_feishu_chat",
                    "message_type": "text",
                    "content": "{\"text\":\"/task Signed Feishu event\"}"
                },
                "sender": {
                    "sender_id": {
                        "open_id": "ou_feishu_sender"
                    }
                }
            }
        }))?;
        let timestamp = "1725442341";
        let nonce = "nonce-feishu";
        let signature = compute_feishu_signature(timestamp, nonce, "feishu-encrypt-key", &body);
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/feishu/events"))
            .header("content-type", "application/json")
            .header("X-Lark-Request-Timestamp", timestamp)
            .header("X-Lark-Request-Nonce", nonce)
            .header("X-Lark-Signature", signature)
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let response_body: Value = response.json().await?;
        assert_eq!(response_body["ok"], true);
        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].platform, "feishu");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
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
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_DINGTALK_CALLBACK_TOKEN", None),
            ("DINGTALK_CALLBACK_TOKEN", None),
            ("DAWN_DINGTALK_ENCODING_AES_KEY", None),
            ("DINGTALK_ENCODING_AES_KEY", None),
        ]);
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
    async fn dingtalk_encrypted_event_validates_signature_and_creates_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let aes_key = test_encoding_aes_key();
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_DINGTALK_CALLBACK_TOKEN", Some("dingtalk-token")),
            ("DAWN_DINGTALK_ENCODING_AES_KEY", Some(&aes_key)),
        ]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let encrypted = encrypt_wechat_style_message(
            &aes_key,
            r#"{"msgtype":"text","text":{"content":"/task Signed DingTalk event"},"conversationId":"dt-cid","senderStaffId":"dt-user"}"#,
            "",
        )?;
        let timestamp = "1725442342";
        let nonce = "nonce-dingtalk";
        let signature =
            compute_sorted_sha1_signature(&["dingtalk-token", timestamp, nonce, &encrypted]);
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/dingtalk/events?signature={signature}&timestamp={timestamp}&nonce={nonce}"
            ))
            .json(&json!({ "encrypt": encrypted }))
            .send()
            .await?
            .error_for_status()?;
        let response_body: Value = response.json().await?;
        let encrypted_ack = response_body["encrypt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing encrypted dingtalk ack"))?;
        assert_eq!(
            decrypt_wechat_style_message(&aes_key, encrypted_ack)?,
            "success"
        );
        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].platform, "dingtalk");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
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
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_WECOM_CALLBACK_TOKEN", None),
            ("WECOM_CALLBACK_TOKEN", None),
            ("DAWN_WECOM_ENCODING_AES_KEY", None),
            ("WECOM_ENCODING_AES_KEY", None),
        ]);
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
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_WECOM_CALLBACK_TOKEN", None),
            ("WECOM_CALLBACK_TOKEN", None),
            ("DAWN_WECOM_ENCODING_AES_KEY", None),
            ("WECOM_ENCODING_AES_KEY", None),
        ]);
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
    async fn wecom_encrypted_event_validates_signature_and_creates_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let aes_key = test_encoding_aes_key();
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_WECOM_CALLBACK_TOKEN", Some("wecom-token")),
            ("DAWN_WECOM_ENCODING_AES_KEY", Some(&aes_key)),
        ]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let encrypted = encrypt_wechat_style_message(
            &aes_key,
            "<xml>\
                <ToUserName><![CDATA[wwcorp]]></ToUserName>\
                <FromUserName><![CDATA[zhangsan]]></FromUserName>\
                <CreateTime>1710000000</CreateTime>\
                <MsgType><![CDATA[text]]></MsgType>\
                <Content><![CDATA[/task Signed WeCom event]]></Content>\
                <MsgId>12345</MsgId>\
            </xml>",
            "wwcorp",
        )?;
        let query =
            signed_wechat_style_query("wecom-token", "1725442343", "nonce-wecom", &encrypted);
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/wecom/events?{query}"
            ))
            .header("content-type", "application/xml")
            .body(format!(
                "<xml><Encrypt><![CDATA[{encrypted}]]></Encrypt></xml>"
            ))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);
        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].platform, "wecom");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn wechat_official_account_verify_round_trip_is_supported() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN", None),
            ("WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY", None),
            ("DAWN_WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY", None),
        ]);
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
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN", None),
            ("WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY", None),
            ("DAWN_WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY", None),
        ]);
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
    async fn wechat_official_account_encrypted_event_validates_signature_and_creates_task()
    -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let aes_key = test_encoding_aes_key();
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN", Some("wechat-token")),
            ("WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY", Some(&aes_key)),
        ]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let encrypted = encrypt_wechat_style_message(
            &aes_key,
            "<xml>\
                <ToUserName><![CDATA[gh_001]]></ToUserName>\
                <FromUserName><![CDATA[user-openid-456]]></FromUserName>\
                <CreateTime>1710000000</CreateTime>\
                <MsgType><![CDATA[text]]></MsgType>\
                <Content><![CDATA[/task Signed WeChat event]]></Content>\
                <MsgId>987654322</MsgId>\
            </xml>",
            "wxappid",
        )?;
        let query =
            signed_wechat_style_query("wechat-token", "1725442344", "nonce-wechat", &encrypted);
        let response = client
            .post(format!(
                "{base_url}/api/gateway/ingress/wechat-official-account/events?encrypt_type=aes&{query}"
            ))
            .header("content-type", "application/xml")
            .body(format!(
                "<xml><Encrypt><![CDATA[{encrypted}]]></Encrypt></xml>"
            ))
            .send()
            .await?
            .error_for_status()?;
        let body = response.text().await?;
        assert_eq!(body, "success");
        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].platform, "wechat_official_account");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn qq_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[
            ("DAWN_QQ_BOT_CALLBACK_SECRET", None),
            ("QQ_BOT_CLIENT_SECRET", None),
        ]);
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

    #[tokio::test]
    async fn qq_signed_event_creates_ingress_event_and_task() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_QQ_BOT_CALLBACK_SECRET", Some("qq-secret"))]);
        let (base_url, handle, state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let body = serde_json::to_string(&json!({
            "t": "AT_MESSAGE_CREATE",
            "d": {
                "content": "<@!botid> /task Signed QQ event",
                "author": {
                    "id": "qq-user-002",
                    "username": "qq-signed-operator"
                }
            }
        }))?;
        let timestamp = "1725442345";
        let signature = qq_callback_signature("qq-secret", timestamp, &body)?;
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/qq/events"))
            .header("content-type", "application/json")
            .header("X-Signature-Timestamp", timestamp)
            .header("X-Signature-Ed25519", signature)
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        assert_eq!(body["ok"], true);
        let events = state.list_chat_ingress_events(Some(10)).await?;
        assert_eq!(events[0].platform, "qq");
        assert_eq!(events[0].status, ChatIngressStatus::TaskCreated);
        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn qq_validation_response_is_signed() -> anyhow::Result<()> {
        let _guard = env_lock().lock().expect("env mutex");
        let _env = ScopedEnvRestore::apply(&[("DAWN_QQ_BOT_CALLBACK_SECRET", Some("qq-secret"))]);
        let (base_url, handle, _state, db_path) = spawn_test_server().await?;
        let client = Client::new();
        let body = serde_json::to_string(&json!({
            "op": 13,
            "d": {
                "plain_token": "plain-token-123",
                "event_ts": "1725442346"
            }
        }))?;
        let timestamp = "1725442346";
        let callback_signature = qq_callback_signature("qq-secret", timestamp, &body)?;
        let response = client
            .post(format!("{base_url}/api/gateway/ingress/qq/events"))
            .header("content-type", "application/json")
            .header("X-Signature-Timestamp", timestamp)
            .header("X-Signature-Ed25519", callback_signature)
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let response_body: Value = response.json().await?;
        assert_eq!(response_body["plain_token"], "plain-token-123");
        let signature_hex = response_body["signature"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing qq validation signature"))?;
        let signature_bytes = hex::decode(signature_hex)?;
        let signature_bytes: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid signature length"))?;
        let signature = Signature::from_bytes(&signature_bytes);
        let verifying_key = qq_verifying_key_from_secret("qq-secret")?;
        verifying_key.verify(b"1725442346plain-token-123", &signature)?;
        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[test]
    fn parses_help_and_skills_commands() {
        assert!(matches!(
            parse_ingress_command("/"),
            Some(IngressCommand::Help)
        ));
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
            Some(IngressCommand::ModeSet {
                mode: ChatAutomationMode::Observe
            })
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
            normalize_ingress_command_text(
                "wechat_official_account",
                "<at user_id=\"ou_x\">机器人</at> /status"
            ),
            "/status"
        );
        assert_eq!(normalize_ingress_command_text("feishu", "帮助"), "/help");
        assert_eq!(
            normalize_ingress_command_text("dingtalk", "状态"),
            "/status"
        );
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
        assert_eq!(normalize_ingress_command_text("qq", "<@!botid>"), "/help");
    }

    #[test]
    fn default_model_reply_only_handles_clear_conversation() {
        assert!(should_attempt_default_model_reply("你是谁"));
        assert!(should_attempt_default_model_reply("hello"));
        assert!(should_attempt_default_model_reply("What can you do?"));

        assert!(!should_attempt_default_model_reply(
            "Book train to Shanghai"
        ));
        assert!(!should_attempt_default_model_reply("打开浏览器，搜索抖音"));
        assert!(!should_attempt_default_model_reply(
            "BlueBubbles reaction received for message-guid-123"
        ));
    }

    #[test]
    fn normalizes_skills_search_query_prefixes() {
        assert_eq!(parse_skills_query(""), None);
        assert_eq!(parse_skills_query("search   "), None);
        assert_eq!(
            parse_skills_query("search echo skill"),
            Some("echo skill".to_string())
        );
        assert_eq!(
            parse_skills_query("find travel"),
            Some("travel".to_string())
        );
        assert_eq!(parse_skills_query("echo"), Some("echo".to_string()));
    }

    #[test]
    fn model_provider_candidates_keep_live_fallbacks_after_defaults() {
        let defaults = vec!["openai".to_string(), "openai_codex".to_string()];
        let candidates = model_provider_candidates(&defaults);

        assert_eq!(candidates.first(), Some(&"openai"));
        assert_eq!(candidates.get(1), Some(&"openai_codex"));
        assert_eq!(
            candidates
                .iter()
                .filter(|provider| **provider == "openai_codex")
                .count(),
            1
        );
        assert!(candidates.contains(&"ollama"));
    }

    #[test]
    fn renders_experience_context_without_evidence_payload() {
        let now = unix_timestamp_ms();
        let context = render_experience_context(&[AgentExperienceRecord {
            experience_id: Uuid::new_v4(),
            source: "chat_ingress:telegram".to_string(),
            scope: "chat".to_string(),
            task_kind: "conversation".to_string(),
            input_summary: "用户原始消息不应进入提示".to_string(),
            action_summary: "模型回复正文不应进入提示".to_string(),
            outcome: "success".to_string(),
            lesson: "普通聊天应优先回复，不应误创建任务".to_string(),
            reusable_hint: Some("检查可用对话模型和 linkedTaskId".to_string()),
            evidence: json!({"rawPayload": "secret-value"}),
            tags: vec!["telegram".to_string(), "model-fallback".to_string()],
            risk_level: "low".to_string(),
            related_task_id: None,
            related_ingress_id: None,
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        }])
        .expect("experience context should render");

        assert!(context.contains("普通聊天应优先回复"));
        assert!(context.contains("检查可用对话模型"));
        assert!(!context.contains("secret-value"));
        assert!(!context.contains("用户原始消息"));
        assert!(!context.contains("模型回复正文"));
    }

    #[test]
    fn model_failure_reply_is_short_and_user_safe() {
        let error = anyhow::anyhow!(
            "OpenAI Codex connector request failed with status 1: {{\"stderr\":\"{}\"}}",
            "x".repeat(5000)
        );
        let reply = render_model_failure_reply(&error);

        assert!(reply.contains("模型回复失败"));
        assert!(reply.contains("普通聊天没有被转成任务"));
        assert!(reply.chars().count() < 800);
        assert!(!reply.contains(&"x".repeat(1000)));
    }

    #[test]
    fn parses_desktop_control_action_intents() {
        assert_eq!(
            parse_local_action_intent("看一下屏幕"),
            Some(LocalActionIntent::DesktopSnapshot {
                include_screenshot: true
            })
        );
        assert_eq!(
            parse_local_action_intent("鼠标位置"),
            Some(LocalActionIntent::DesktopMousePosition)
        );
        assert_eq!(
            parse_local_action_intent("移动鼠标到 400,300"),
            Some(LocalActionIntent::DesktopMouseMove { x: 400, y: 300 })
        );
        assert_eq!(
            parse_local_action_intent("右键点击 -20 300"),
            Some(LocalActionIntent::DesktopMouseClick {
                x: Some(-20),
                y: Some(300),
                button: "right".to_string(),
                double_click: false,
            })
        );
        assert_eq!(
            parse_local_action_intent("双击当前位置"),
            Some(LocalActionIntent::DesktopMouseClick {
                x: None,
                y: None,
                button: "left".to_string(),
                double_click: true,
            })
        );
        assert_eq!(parse_local_action_intent("点击确定"), None);
    }

    #[test]
    fn parses_skill_selector_with_json_arguments() {
        let parsed = parse_skill_selector(
            r#"qgis.render.exportMap {"projectId":"demo-map","draftId":"draft-1"}"#,
        )
        .expect("skill selector should parse");

        assert_eq!(parsed.skill_id, "qgis.render.exportMap");
        assert_eq!(parsed.version, None);
        assert_eq!(
            parsed.arguments.as_deref(),
            Some(r#"{"projectId":"demo-map","draftId":"draft-1"}"#)
        );
    }

    #[test]
    fn builds_qgis_native_instruction_from_ingress_record() {
        let record = ChatIngressEventRecord {
            ingress_id: Uuid::new_v4(),
            platform: "telegram".to_string(),
            event_type: "telegram.message".to_string(),
            chat_id: Some("123".to_string()),
            sender_id: Some("456".to_string()),
            sender_display: Some("alice".to_string()),
            text: "/skill qgis.render.exportMap".to_string(),
            raw_payload: json!({}),
            linked_task_id: None,
            reply_text: None,
            status: ChatIngressStatus::Received,
            error: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };

        let instruction = build_qgis_native_instruction(
            "telegram",
            &record,
            "qgis.render.exportMap",
            r#"{"projectId":"demo-map","draftId":"draft-1","outputKinds":["image"]}"#,
        )
        .expect("native instruction should be built");
        assert!(instruction.starts_with("native:"));
        assert!(instruction.contains("\"skillId\":\"qgis.render.exportMap\""));
        assert!(instruction.contains("\"projectId\":\"demo-map\""));
        assert!(instruction.contains("\"actor\":\"telegram:456\""));
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
