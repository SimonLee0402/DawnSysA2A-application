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
use sha1::{Digest, Sha1};
use tracing::warn;
use uuid::Uuid;

use crate::{
    a2a::{self, Task},
    app_state::{
        AppState, ChatChannelIdentityRecord, ChatChannelIdentityStatus, ChatIngressEventRecord,
        ChatIngressStatus, unix_timestamp_ms,
    },
    connectors::{self, ChatDispatchRequest},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatIngressStatusReport {
    supported_platforms: Vec<&'static str>,
    telegram_webhook_secret_configured: bool,
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
        .route("/pairings/:platform/:identity_key/approve", post(approve_pairing))
        .route("/pairings/:platform/:identity_key/reject", post(reject_pairing))
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
    let platform = query.platform.as_deref().map(str::trim).filter(|value| !value.is_empty());
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

async fn signal_events(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_callback_secret("DAWN_SIGNAL_CALLBACK_SECRET", "signal", &secret)
        .map_err(bad_request)?;

    let text = extract_signal_text(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported signal event; expected a text message payload"
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

async fn bluebubbles_events(
    State(state): State<Arc<AppState>>,
    Path(secret): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    verify_callback_secret("DAWN_BLUEBUBBLES_CALLBACK_SECRET", "bluebubbles", &secret)
        .map_err(bad_request)?;

    let text = extract_bluebubbles_text(&payload).ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "unsupported bluebubbles event; expected a text message payload"
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
            let _ = dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &reply).await;
            record.reply_text = Some(reply);
            record.status = ChatIngressStatus::PendingApproval;
            record.error = Some(format!(
                "{platform} sender is waiting for pairing approval ({pairing_code})"
            ));
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            return Ok(record);
        }
        IngressAccessDecision::Rejected(message) => {
            let _ =
                dispatch_ingress_reply_if_possible(platform, record.chat_id.as_deref(), &message)
                    .await;
            record.reply_text = Some(message.clone());
            record.status = ChatIngressStatus::Ignored;
            record.error = Some(message);
            record.updated_at_unix_ms = unix_timestamp_ms();
            state.upsert_chat_ingress_event(record.clone()).await?;
            return Ok(record);
        }
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
    if let Some(identity) = state.get_chat_channel_identity(platform, &identity_key).await? {
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
    let Some(chat_id) = chat_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let request = ChatDispatchRequest {
        platform: platform.to_string(),
        text: text.to_string(),
        chat_id: Some(chat_id.to_string()),
        parse_mode: None,
        disable_notification: Some(false),
        target_type: None,
        event_id: None,
        msg_id: None,
        msg_seq: None,
        is_wakeup: None,
    };
    match connectors::execute_chat_connector(request).await {
        Ok(_) => Ok(()),
        Err(error) => {
            warn!(?error, platform, "failed to dispatch ingress reply");
            Ok(())
        }
    }
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
        .filter(|value| values.iter().any(|allowed| allowed.eq_ignore_ascii_case(value)))
        .is_some()
        || chat_id
            .map(str::trim)
            .filter(|value| values.iter().any(|allowed| allowed.eq_ignore_ascii_case(value)))
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
    async fn signal_event_can_require_pairing_and_then_create_task_after_approval(
    ) -> anyhow::Result<()> {
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
    fn computes_wechat_signature_with_sorted_parts() {
        let signature = compute_wechat_signature("token123", "1710000000", "xyz");
        assert_eq!(signature.len(), 40);
        assert_eq!(
            signature,
            compute_wechat_signature("token123", "1710000000", "xyz")
        );
    }
}
