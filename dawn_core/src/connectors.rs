use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::info;

use crate::app_state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorStatusReport {
    configured: ConfiguredConnectors,
    supported_model_providers: Vec<ModelProviderSupport>,
    supported_chat_platforms: Vec<ChatPlatformSupport>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfiguredConnectors {
    openai: bool,
    deepseek: bool,
    qwen: bool,
    zhipu: bool,
    moonshot: bool,
    doubao: bool,
    telegram: bool,
    feishu: bool,
    dingtalk: bool,
    wecom_bot: bool,
    wechat_official_account: bool,
    qq: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelProviderSupport {
    provider: &'static str,
    region: &'static str,
    integration_mode: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatPlatformSupport {
    platform: &'static str,
    region: &'static str,
    integration_mode: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIResponseRequest {
    pub input: String,
    pub model: Option<String>,
    pub instructions: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponseResult {
    pub mode: &'static str,
    pub provider: &'static str,
    pub model: String,
    pub output_text: String,
    pub raw_response: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatDispatchRequest {
    pub platform: String,
    pub text: String,
    pub chat_id: Option<String>,
    pub parse_mode: Option<String>,
    pub disable_notification: Option<bool>,
    pub target_type: Option<String>,
    pub event_id: Option<String>,
    pub msg_id: Option<String>,
    pub msg_seq: Option<i64>,
    pub is_wakeup: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramSendRequest {
    pub chat_id: String,
    pub text: String,
    pub parse_mode: Option<String>,
    pub disable_notification: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeChatOfficialAccountSendRequest {
    pub open_id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QQSendRequest {
    pub recipient_id: String,
    pub text: String,
    pub target_type: Option<String>,
    pub event_id: Option<String>,
    pub msg_id: Option<String>,
    pub msg_seq: Option<i64>,
    pub is_wakeup: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookTextRequest {
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendResult {
    pub mode: &'static str,
    pub platform: &'static str,
    pub delivered: bool,
    pub raw_response: Option<Value>,
}

pub async fn execute_model_connector(
    provider: &str,
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    match provider {
        "openai" => execute_openai_response(request).await,
        "deepseek" => execute_deepseek_response(request).await,
        "qwen" => execute_qwen_response(request).await,
        "zhipu" => execute_zhipu_response(request).await,
        "moonshot" => execute_moonshot_response(request).await,
        "doubao" => execute_doubao_response(request).await,
        other => Err(anyhow::anyhow!("unsupported model provider: {other}")),
    }
}

pub async fn execute_chat_connector(
    request: ChatDispatchRequest,
) -> anyhow::Result<ChatSendResult> {
    match request.platform.as_str() {
        "telegram" => {
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("telegram connector requires chatId"))?;
            send_telegram_connector(TelegramSendRequest {
                chat_id,
                text: request.text,
                parse_mode: request.parse_mode,
                disable_notification: request.disable_notification,
            })
            .await
        }
        "feishu" => send_webhook_connector("feishu", "FEISHU_BOT_WEBHOOK_URL", request.text).await,
        "dingtalk" => {
            send_webhook_connector("dingtalk", "DINGTALK_BOT_WEBHOOK_URL", request.text).await
        }
        "wecom_bot" | "wecom" => {
            send_webhook_connector("wecom_bot", "WECOM_BOT_WEBHOOK_URL", request.text).await
        }
        "wechat_official_account" => {
            let open_id = request.chat_id.ok_or_else(|| {
                anyhow::anyhow!("wechat_official_account connector requires chatId as openId")
            })?;
            send_wechat_official_account_connector(WeChatOfficialAccountSendRequest {
                open_id,
                text: request.text,
            })
            .await
        }
        "qq" => {
            let recipient_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("qq connector requires chatId as recipient_id"))?;
            send_qq_connector(QQSendRequest {
                recipient_id,
                text: request.text,
                target_type: request.target_type,
                event_id: request.event_id,
                msg_id: request.msg_id,
                msg_seq: request.msg_seq,
                is_wakeup: request.is_wakeup,
            })
            .await
        }
        other => Err(anyhow::anyhow!("unsupported chat platform: {other}")),
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/model/openai/respond", post(openai_respond))
        .route("/model/deepseek/respond", post(deepseek_respond))
        .route("/model/qwen/respond", post(qwen_respond))
        .route("/model/zhipu/respond", post(zhipu_respond))
        .route("/model/moonshot/respond", post(moonshot_respond))
        .route("/model/doubao/respond", post(doubao_respond))
        .route("/chat/telegram/send", post(send_telegram_message))
        .route("/chat/feishu/send", post(send_feishu_message))
        .route("/chat/dingtalk/send", post(send_dingtalk_message))
        .route("/chat/wecom/send", post(send_wecom_message))
        .route(
            "/chat/wechat-official-account/send",
            post(send_wechat_official_account_message),
        )
        .route("/chat/qq/send", post(send_qq_message))
}

async fn status() -> Json<ConnectorStatusReport> {
    Json(ConnectorStatusReport {
        configured: ConfiguredConnectors {
            openai: std::env::var("OPENAI_API_KEY").is_ok(),
            deepseek: std::env::var("DEEPSEEK_API_KEY").is_ok(),
            qwen: resolve_first_present_env(&["QWEN_API_KEY", "DASHSCOPE_API_KEY"]).is_some(),
            zhipu: std::env::var("ZHIPU_API_KEY").is_ok(),
            moonshot: std::env::var("MOONSHOT_API_KEY").is_ok(),
            doubao: resolve_first_present_env(&["DOUBAO_API_KEY", "ARK_API_KEY"]).is_some(),
            telegram: std::env::var("TELEGRAM_BOT_TOKEN").is_ok(),
            feishu: std::env::var("FEISHU_BOT_WEBHOOK_URL").is_ok(),
            dingtalk: std::env::var("DINGTALK_BOT_WEBHOOK_URL").is_ok(),
            wecom_bot: std::env::var("WECOM_BOT_WEBHOOK_URL").is_ok(),
            wechat_official_account: has_wechat_official_account_credentials(),
            qq: has_qq_bot_credentials(),
        },
        supported_model_providers: vec![
            ModelProviderSupport {
                provider: "openai",
                region: "global",
                integration_mode: "live",
            },
            ModelProviderSupport {
                provider: "deepseek",
                region: "china",
                integration_mode: "live",
            },
            ModelProviderSupport {
                provider: "qwen",
                region: "china",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "zhipu",
                region: "china",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "moonshot",
                region: "china",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "doubao",
                region: "china",
                integration_mode: "live_ark_chat_compatible",
            },
        ],
        supported_chat_platforms: vec![
            ChatPlatformSupport {
                platform: "telegram",
                region: "global",
                integration_mode: "live",
            },
            ChatPlatformSupport {
                platform: "feishu",
                region: "china",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "dingtalk",
                region: "china",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "wecom_bot",
                region: "china",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "wechat_official_account",
                region: "china",
                integration_mode: "live_official_account_text",
            },
            ChatPlatformSupport {
                platform: "qq",
                region: "china",
                integration_mode: "live_openapi_c2c_group",
            },
        ],
    })
}

async fn openai_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_openai_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn deepseek_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_deepseek_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn qwen_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_qwen_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn zhipu_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_zhipu_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn moonshot_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_moonshot_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn doubao_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_doubao_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_telegram_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<TelegramSendRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_telegram_connector(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_feishu_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("feishu", "FEISHU_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_dingtalk_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("dingtalk", "DINGTALK_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_wecom_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("wecom_bot", "WECOM_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_wechat_official_account_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WeChatOfficialAccountSendRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_wechat_official_account_connector(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_qq_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<QQSendRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_qq_connector(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn execute_openai_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let model = request.model.unwrap_or_else(|| "gpt-4.1-mini".to_string());
    let Some(api_key) = std::env::var("OPENAI_API_KEY").ok() else {
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider: "openai",
            model,
            output_text: format!(
                "OPENAI_API_KEY is not configured. Dry-run request would send input: {}",
                request.input
            ),
            raw_response: None,
        });
    };

    info!("Dispatching live OpenAI response request through gateway connector");
    let body = json!({
        "model": model,
        "input": request.input,
        "instructions": request.instructions,
    });
    let response = Client::new()
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("OpenAI connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "openai",
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_openai_text(&raw_response),
        raw_response: Some(raw_response),
    })
}

async fn execute_deepseek_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "deepseek",
        request,
        "deepseek-chat",
        &["DEEPSEEK_API_KEY"],
        Some("DEEPSEEK_CHAT_COMPLETIONS_URL"),
        "https://api.deepseek.com/chat/completions",
    )
    .await
}

async fn execute_qwen_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "qwen",
        request,
        "qwen-plus",
        &["QWEN_API_KEY", "DASHSCOPE_API_KEY"],
        Some("QWEN_CHAT_COMPLETIONS_URL"),
        "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
    )
    .await
}

async fn execute_zhipu_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "zhipu",
        request,
        "glm-4.5-air",
        &["ZHIPU_API_KEY"],
        Some("ZHIPU_CHAT_COMPLETIONS_URL"),
        "https://open.bigmodel.cn/api/paas/v4/chat/completions",
    )
    .await
}

async fn execute_moonshot_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "moonshot",
        request,
        "moonshot-v1-8k",
        &["MOONSHOT_API_KEY"],
        Some("MOONSHOT_CHAT_COMPLETIONS_URL"),
        "https://api.moonshot.cn/v1/chat/completions",
    )
    .await
}

async fn execute_doubao_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let default_model = resolve_first_present_env(&["DOUBAO_ENDPOINT_ID", "ARK_MODEL_ENDPOINT_ID"])
        .unwrap_or_else(|| "ep-your-doubao-endpoint-id".to_string());
    execute_openai_compatible_chat_response(
        "doubao",
        request,
        &default_model,
        &["DOUBAO_API_KEY", "ARK_API_KEY"],
        Some("DOUBAO_CHAT_COMPLETIONS_URL"),
        "https://ark.cn-beijing.volces.com/api/v3/chat/completions",
    )
    .await
}

async fn send_telegram_connector(request: TelegramSendRequest) -> anyhow::Result<ChatSendResult> {
    let Some(bot_token) = std::env::var("TELEGRAM_BOT_TOKEN").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "telegram",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": request.text,
                "reason": "TELEGRAM_BOT_TOKEN is not configured"
            })),
        });
    };

    info!("Dispatching live Telegram sendMessage request through gateway connector");
    let response = Client::new()
        .post(format!(
            "https://api.telegram.org/bot{}/sendMessage",
            bot_token
        ))
        .json(&json!({
            "chat_id": request.chat_id,
            "text": request.text,
            "parse_mode": request.parse_mode,
            "disable_notification": request.disable_notification.unwrap_or(false)
        }))
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("Telegram connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "telegram",
        delivered: raw_response
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        raw_response: Some(raw_response),
    })
}

async fn send_webhook_connector(
    platform: &'static str,
    env_var: &'static str,
    text: String,
) -> anyhow::Result<ChatSendResult> {
    let payload = match platform {
        "feishu" => json!({
            "msg_type": "text",
            "content": {
                "text": text
            }
        }),
        "dingtalk" => json!({
            "msgtype": "text",
            "text": {
                "content": text
            }
        }),
        "wecom_bot" => json!({
            "msgtype": "text",
            "text": {
                "content": text
            }
        }),
        _ => anyhow::bail!("unsupported webhook chat platform: {platform}"),
    };

    let Some(webhook_url) = std::env::var(env_var).ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform,
            delivered: false,
            raw_response: Some(json!({
                "reason": format!("{env_var} is not configured"),
                "payload": payload
            })),
        });
    };

    info!("Dispatching live webhook chat message for platform {platform}");
    let response = Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let raw_response = match response.json::<Value>().await {
        Ok(value) => value,
        Err(_) => json!({
            "status": status.as_u16(),
            "payloadAccepted": status.is_success()
        }),
    };

    if !status.is_success() {
        anyhow::bail!("{platform} webhook request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform,
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_wechat_official_account_connector(
    request: WeChatOfficialAccountSendRequest,
) -> anyhow::Result<ChatSendResult> {
    let Some(access_token) = resolve_wechat_official_account_access_token().await? else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "wechat_official_account",
            delivered: false,
            raw_response: Some(json!({
                "openId": request.open_id,
                "text": request.text,
                "reason": "WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN or WECHAT_OFFICIAL_ACCOUNT_APP_ID/WECHAT_OFFICIAL_ACCOUNT_APP_SECRET is not configured"
            })),
        });
    };

    info!("Dispatching live WeChat Official Account custom message through gateway connector");
    let response = Client::new()
        .post("https://api.weixin.qq.com/cgi-bin/message/custom/send")
        .query(&[("access_token", access_token.as_str())])
        .json(&build_wechat_official_account_payload(
            &request.open_id,
            &request.text,
        ))
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!(
            "wechat_official_account connector request failed with status {status}: {raw_response}"
        );
    }

    let delivered = raw_response
        .get("errcode")
        .and_then(Value::as_i64)
        .map(|code| code == 0)
        .unwrap_or(true);

    if !delivered {
        anyhow::bail!("wechat_official_account send failed: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "wechat_official_account",
        delivered,
        raw_response: Some(raw_response),
    })
}

async fn send_qq_connector(request: QQSendRequest) -> anyhow::Result<ChatSendResult> {
    let Some(access_token) = resolve_qq_bot_access_token().await? else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "qq",
            delivered: false,
            raw_response: Some(json!({
                "recipientId": request.recipient_id,
                "targetType": normalize_qq_target_type(request.target_type.as_deref()),
                "text": request.text,
                "reason": "QQ_BOT_APP_ID and QQ_BOT_CLIENT_SECRET are not configured"
            })),
        });
    };

    let target_type = normalize_qq_target_type(request.target_type.as_deref());
    let endpoint = match target_type {
        "group" => format!(
            "https://api.sgroup.qq.com/v2/groups/{}/messages",
            request.recipient_id
        ),
        _ => format!(
            "https://api.sgroup.qq.com/v2/users/{}/messages",
            request.recipient_id
        ),
    };
    let payload = build_qq_message_payload(&request.text, &request);

    info!("Dispatching live QQ bot message through gateway connector");
    let response = Client::new()
        .post(endpoint)
        .header("Authorization", format!("QQBot {access_token}"))
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("qq connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "qq",
        delivered: raw_response.get("id").is_some(),
        raw_response: Some(raw_response),
    })
}

async fn execute_openai_compatible_chat_response(
    provider: &'static str,
    request: OpenAIResponseRequest,
    default_model: &str,
    api_key_env_vars: &[&str],
    endpoint_env_var: Option<&str>,
    default_endpoint: &str,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model.unwrap_or_else(|| default_model.to_string());
    let Some(api_key) = resolve_first_present_env(api_key_env_vars) else {
        let env_names = api_key_env_vars.join(" or ");
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider,
            model,
            output_text: format!(
                "{env_names} is not configured. Dry-run request would send input: {input}"
            ),
            raw_response: None,
        });
    };
    let endpoint = resolve_endpoint(endpoint_env_var, default_endpoint);

    info!("Dispatching live {provider} chat completion request through gateway connector");
    let body = json!({
        "model": model,
        "messages": build_chat_completion_messages(&input, instructions.as_deref()),
        "stream": false
    });
    let response = Client::new()
        .post(&endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("{provider} connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider,
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_chat_completion_text(&raw_response),
        raw_response: Some(raw_response),
    })
}

fn build_wechat_official_account_payload(open_id: &str, text: &str) -> Value {
    json!({
        "touser": open_id,
        "msgtype": "text",
        "text": {
            "content": text
        }
    })
}

fn build_qq_message_payload(text: &str, request: &QQSendRequest) -> Value {
    let mut payload = json!({
        "content": text,
        "msg_type": 0
    });
    if let Some(event_id) = &request.event_id {
        payload["event_id"] = json!(event_id);
    }
    if let Some(msg_id) = &request.msg_id {
        payload["msg_id"] = json!(msg_id);
    }
    if let Some(msg_seq) = request.msg_seq {
        payload["msg_seq"] = json!(msg_seq);
    }
    if let Some(is_wakeup) = request.is_wakeup {
        payload["is_wakeup"] = json!(is_wakeup);
    }
    payload
}

fn build_chat_completion_messages(input: &str, instructions: Option<&str>) -> Value {
    match instructions {
        Some(instructions) => json!([
            { "role": "system", "content": instructions },
            { "role": "user", "content": input }
        ]),
        None => json!([
            { "role": "user", "content": input }
        ]),
    }
}

fn resolve_first_present_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| std::env::var(name).ok())
}

fn has_wechat_official_account_credentials() -> bool {
    std::env::var("WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN").is_ok()
        || (std::env::var("WECHAT_OFFICIAL_ACCOUNT_APP_ID").is_ok()
            && std::env::var("WECHAT_OFFICIAL_ACCOUNT_APP_SECRET").is_ok())
}

fn has_qq_bot_credentials() -> bool {
    std::env::var("QQ_BOT_APP_ID").is_ok() && std::env::var("QQ_BOT_CLIENT_SECRET").is_ok()
}

fn resolve_endpoint(endpoint_env_var: Option<&str>, default_endpoint: &str) -> String {
    endpoint_env_var
        .and_then(|name| std::env::var(name).ok())
        .unwrap_or_else(|| default_endpoint.to_string())
}

async fn resolve_wechat_official_account_access_token() -> anyhow::Result<Option<String>> {
    if let Ok(access_token) = std::env::var("WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN") {
        return Ok(Some(access_token));
    }

    let Ok(app_id) = std::env::var("WECHAT_OFFICIAL_ACCOUNT_APP_ID") else {
        return Ok(None);
    };
    let Ok(app_secret) = std::env::var("WECHAT_OFFICIAL_ACCOUNT_APP_SECRET") else {
        return Ok(None);
    };

    let response = Client::new()
        .get("https://api.weixin.qq.com/cgi-bin/token")
        .query(&[
            ("grant_type", "client_credential"),
            ("appid", app_id.as_str()),
            ("secret", app_secret.as_str()),
        ])
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!(
            "wechat_official_account access_token request failed with status {status}: {raw_response}"
        );
    }

    raw_response
        .get("access_token")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            anyhow::anyhow!("wechat_official_account access_token missing: {raw_response}")
        })
        .map(Some)
}

async fn resolve_qq_bot_access_token() -> anyhow::Result<Option<String>> {
    let Ok(app_id) = std::env::var("QQ_BOT_APP_ID") else {
        return Ok(None);
    };
    let Ok(client_secret) = std::env::var("QQ_BOT_CLIENT_SECRET") else {
        return Ok(None);
    };

    let response = Client::new()
        .post("https://bots.qq.com/app/getAppAccessToken")
        .json(&json!({
            "appId": app_id,
            "clientSecret": client_secret
        }))
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("qq bot access_token request failed with status {status}: {raw_response}");
    }

    raw_response
        .get("access_token")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("qq bot access_token missing: {raw_response}"))
        .map(Some)
}

fn normalize_qq_target_type(target_type: Option<&str>) -> &'static str {
    match target_type.unwrap_or("user") {
        "group" => "group",
        _ => "user",
    }
}

fn extract_openai_text(raw_response: &Value) -> String {
    if let Some(text) = raw_response.get("output_text").and_then(Value::as_str) {
        return text.to_string();
    }

    raw_response
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("content")
                    .and_then(Value::as_array)
                    .and_then(|content| {
                        content.iter().find_map(|part| {
                            part.get("text")
                                .and_then(|text| text.as_str().map(ToString::to_string))
                                .or_else(|| {
                                    part.get("output_text")
                                        .and_then(Value::as_str)
                                        .map(ToString::to_string)
                                })
                        })
                    })
            })
        })
        .unwrap_or_else(|| raw_response.to_string())
}

fn extract_chat_completion_text(raw_response: &Value) -> String {
    raw_response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| raw_response.to_string())
}

fn connector_anyhow_error(error: anyhow::Error) -> (axum::http::StatusCode, Json<Value>) {
    (
        axum::http::StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        QQSendRequest, build_chat_completion_messages, build_qq_message_payload,
        build_wechat_official_account_payload, extract_chat_completion_text, extract_openai_text,
        normalize_qq_target_type,
    };

    #[test]
    fn extracts_output_text_field() {
        let raw = json!({
            "output_text": "gateway reply"
        });

        assert_eq!(extract_openai_text(&raw), "gateway reply");
    }

    #[test]
    fn extracts_nested_output_content_text() {
        let raw = json!({
            "output": [
                {
                    "content": [
                        {
                            "text": "nested reply"
                        }
                    ]
                }
            ]
        });

        assert_eq!(extract_openai_text(&raw), "nested reply");
    }

    #[test]
    fn extracts_chat_completion_message_content() {
        let raw = json!({
            "choices": [
                {
                    "message": {
                        "content": "deepseek reply"
                    }
                }
            ]
        });

        assert_eq!(extract_chat_completion_text(&raw), "deepseek reply");
    }

    #[test]
    fn extracts_openai_compatible_chat_content() {
        let raw = json!({
            "choices": [
                {
                    "message": {
                        "content": "china provider reply"
                    }
                }
            ]
        });

        assert_eq!(extract_chat_completion_text(&raw), "china provider reply");
    }

    #[test]
    fn builds_messages_with_system_instructions() {
        let messages = build_chat_completion_messages("hello", Some("be concise"));

        assert_eq!(
            messages,
            json!([
                {
                    "role": "system",
                    "content": "be concise"
                },
                {
                    "role": "user",
                    "content": "hello"
                }
            ])
        );
    }

    #[test]
    fn builds_wechat_official_account_text_payload() {
        let payload = build_wechat_official_account_payload("openid-123", "hello china");

        assert_eq!(
            payload,
            json!({
                "touser": "openid-123",
                "msgtype": "text",
                "text": {
                    "content": "hello china"
                }
            })
        );
    }

    #[test]
    fn builds_qq_text_payload_with_reply_metadata() {
        let payload = build_qq_message_payload(
            "hello qq",
            &QQSendRequest {
                recipient_id: "user-openid".to_string(),
                text: "hello qq".to_string(),
                target_type: Some("group".to_string()),
                event_id: Some("evt-1".to_string()),
                msg_id: Some("msg-1".to_string()),
                msg_seq: Some(2),
                is_wakeup: Some(true),
            },
        );

        assert_eq!(
            payload,
            json!({
                "content": "hello qq",
                "msg_type": 0,
                "event_id": "evt-1",
                "msg_id": "msg-1",
                "msg_seq": 2,
                "is_wakeup": true
            })
        );
    }

    #[test]
    fn normalizes_qq_target_type_to_supported_values() {
        assert_eq!(normalize_qq_target_type(Some("group")), "group");
        assert_eq!(normalize_qq_target_type(Some("unknown")), "user");
        assert_eq!(normalize_qq_target_type(None), "user");
    }
}
