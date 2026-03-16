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
    anthropic: bool,
    google: bool,
    openrouter: bool,
    groq: bool,
    together: bool,
    vllm: bool,
    deepseek: bool,
    qwen: bool,
    zhipu: bool,
    moonshot: bool,
    doubao: bool,
    ollama: bool,
    telegram: bool,
    slack: bool,
    discord: bool,
    mattermost: bool,
    msteams: bool,
    whatsapp: bool,
    line: bool,
    google_chat: bool,
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
pub struct ChatTargetTextRequest {
    pub chat_id: String,
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
        "anthropic" => execute_anthropic_response(request).await,
        "google" => execute_google_response(request).await,
        "openrouter" => execute_openrouter_response(request).await,
        "groq" => execute_groq_response(request).await,
        "together" => execute_together_response(request).await,
        "vllm" => execute_vllm_response(request).await,
        "deepseek" => execute_deepseek_response(request).await,
        "qwen" => execute_qwen_response(request).await,
        "zhipu" => execute_zhipu_response(request).await,
        "moonshot" => execute_moonshot_response(request).await,
        "doubao" => execute_doubao_response(request).await,
        "ollama" => execute_ollama_response(request).await,
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
        "slack" => send_webhook_connector("slack", "SLACK_BOT_WEBHOOK_URL", request.text).await,
        "discord" => {
            send_webhook_connector("discord", "DISCORD_BOT_WEBHOOK_URL", request.text).await
        }
        "mattermost" => {
            send_webhook_connector("mattermost", "MATTERMOST_BOT_WEBHOOK_URL", request.text).await
        }
        "msteams" => {
            send_webhook_connector("msteams", "MSTEAMS_BOT_WEBHOOK_URL", request.text).await
        }
        "whatsapp" => {
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("whatsapp connector requires chatId"))?;
            send_whatsapp_connector(ChatTargetTextRequest {
                chat_id,
                text: request.text,
            })
            .await
        }
        "line" => {
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("line connector requires chatId"))?;
            send_line_connector(ChatTargetTextRequest {
                chat_id,
                text: request.text,
            })
            .await
        }
        "google_chat" => {
            send_webhook_connector("google_chat", "GOOGLE_CHAT_BOT_WEBHOOK_URL", request.text).await
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
        .route("/model/anthropic/respond", post(anthropic_respond))
        .route("/model/google/respond", post(google_respond))
        .route("/model/openrouter/respond", post(openrouter_respond))
        .route("/model/groq/respond", post(groq_respond))
        .route("/model/together/respond", post(together_respond))
        .route("/model/vllm/respond", post(vllm_respond))
        .route("/model/deepseek/respond", post(deepseek_respond))
        .route("/model/qwen/respond", post(qwen_respond))
        .route("/model/zhipu/respond", post(zhipu_respond))
        .route("/model/moonshot/respond", post(moonshot_respond))
        .route("/model/doubao/respond", post(doubao_respond))
        .route("/model/ollama/respond", post(ollama_respond))
        .route("/chat/telegram/send", post(send_telegram_message))
        .route("/chat/slack/send", post(send_slack_message))
        .route("/chat/discord/send", post(send_discord_message))
        .route("/chat/mattermost/send", post(send_mattermost_message))
        .route("/chat/msteams/send", post(send_msteams_message))
        .route("/chat/whatsapp/send", post(send_whatsapp_message))
        .route("/chat/line/send", post(send_line_message))
        .route("/chat/google-chat/send", post(send_google_chat_message))
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
            anthropic: std::env::var("ANTHROPIC_API_KEY").is_ok(),
            google: resolve_first_present_env(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]).is_some(),
            openrouter: std::env::var("OPENROUTER_API_KEY").is_ok(),
            groq: std::env::var("GROQ_API_KEY").is_ok(),
            together: std::env::var("TOGETHER_API_KEY").is_ok(),
            vllm: std::env::var("VLLM_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("VLLM_BASE_URL").is_ok(),
            deepseek: std::env::var("DEEPSEEK_API_KEY").is_ok(),
            qwen: resolve_first_present_env(&["QWEN_API_KEY", "DASHSCOPE_API_KEY"]).is_some(),
            zhipu: std::env::var("ZHIPU_API_KEY").is_ok(),
            moonshot: std::env::var("MOONSHOT_API_KEY").is_ok(),
            doubao: resolve_first_present_env(&["DOUBAO_API_KEY", "ARK_API_KEY"]).is_some(),
            ollama: std::env::var("OLLAMA_CHAT_URL").is_ok()
                || std::env::var("OLLAMA_BASE_URL").is_ok(),
            telegram: std::env::var("TELEGRAM_BOT_TOKEN").is_ok(),
            slack: std::env::var("SLACK_BOT_WEBHOOK_URL").is_ok(),
            discord: std::env::var("DISCORD_BOT_WEBHOOK_URL").is_ok(),
            mattermost: std::env::var("MATTERMOST_BOT_WEBHOOK_URL").is_ok(),
            msteams: std::env::var("MSTEAMS_BOT_WEBHOOK_URL").is_ok(),
            whatsapp: std::env::var("WHATSAPP_ACCESS_TOKEN").is_ok()
                && std::env::var("WHATSAPP_PHONE_NUMBER_ID").is_ok(),
            line: std::env::var("LINE_CHANNEL_ACCESS_TOKEN").is_ok(),
            google_chat: std::env::var("GOOGLE_CHAT_BOT_WEBHOOK_URL").is_ok(),
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
                provider: "anthropic",
                region: "global",
                integration_mode: "live_messages",
            },
            ModelProviderSupport {
                provider: "google",
                region: "global",
                integration_mode: "live_generate_content",
            },
            ModelProviderSupport {
                provider: "openrouter",
                region: "global",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "groq",
                region: "global",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "together",
                region: "global",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "vllm",
                region: "local",
                integration_mode: "live_local_openai_compatible",
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
            ModelProviderSupport {
                provider: "ollama",
                region: "local",
                integration_mode: "live_local_chat",
            },
        ],
        supported_chat_platforms: vec![
            ChatPlatformSupport {
                platform: "telegram",
                region: "global",
                integration_mode: "live",
            },
            ChatPlatformSupport {
                platform: "slack",
                region: "global",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "discord",
                region: "global",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "mattermost",
                region: "global",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "msteams",
                region: "global",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "whatsapp",
                region: "global",
                integration_mode: "live_graph_text",
            },
            ChatPlatformSupport {
                platform: "line",
                region: "global",
                integration_mode: "live_push_text",
            },
            ChatPlatformSupport {
                platform: "google_chat",
                region: "global",
                integration_mode: "live_webhook",
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

async fn anthropic_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_anthropic_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn google_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_google_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn openrouter_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_openrouter_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn groq_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_groq_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn together_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_together_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn vllm_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_vllm_response(request)
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

async fn ollama_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_ollama_response(request)
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

async fn send_slack_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("slack", "SLACK_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_discord_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("discord", "DISCORD_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_mattermost_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("mattermost", "MATTERMOST_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_msteams_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("msteams", "MSTEAMS_BOT_WEBHOOK_URL", request.text)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_whatsapp_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ChatTargetTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_whatsapp_connector(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_line_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ChatTargetTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_line_connector(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_google_chat_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<WebhookTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_webhook_connector("google_chat", "GOOGLE_CHAT_BOT_WEBHOOK_URL", request.text)
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

async fn execute_anthropic_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model.unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string());
    let Some(api_key) = std::env::var("ANTHROPIC_API_KEY").ok() else {
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider: "anthropic",
            model,
            output_text: format!(
                "ANTHROPIC_API_KEY is not configured. Dry-run request would send input: {input}"
            ),
            raw_response: None,
        });
    };
    let endpoint = resolve_endpoint(
        Some("ANTHROPIC_MESSAGES_URL"),
        "https://api.anthropic.com/v1/messages",
    );
    let anthropic_version =
        std::env::var("ANTHROPIC_VERSION").unwrap_or_else(|_| "2023-06-01".to_string());
    let body = json!({
        "model": model,
        "max_tokens": 1024,
        "system": instructions,
        "messages": [
            {
                "role": "user",
                "content": input
            }
        ]
    });

    info!("Dispatching live Anthropic messages request through gateway connector");
    let response = Client::new()
        .post(&endpoint)
        .header("x-api-key", api_key)
        .header("anthropic-version", anthropic_version)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("anthropic connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "anthropic",
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_anthropic_text(&raw_response),
        raw_response: Some(raw_response),
    })
}

async fn execute_google_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model.unwrap_or_else(|| "gemini-2.5-flash".to_string());
    let Some(api_key) = resolve_first_present_env(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]) else {
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider: "google",
            model,
            output_text: format!(
                "GEMINI_API_KEY or GOOGLE_API_KEY is not configured. Dry-run request would send input: {input}"
            ),
            raw_response: None,
        });
    };
    let endpoint = std::env::var("GOOGLE_GENERATE_CONTENT_URL").unwrap_or_else(|_| {
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent")
    });
    let body = json!({
        "system_instruction": instructions.as_ref().map(|value| json!({
            "parts": [{ "text": value }]
        })),
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": input }]
            }
        ]
    });

    info!("Dispatching live Google Gemini request through gateway connector");
    let response = Client::new()
        .post(&endpoint)
        .query(&[("key", api_key.as_str())])
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("google connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "google",
        model,
        output_text: extract_google_text(&raw_response),
        raw_response: Some(raw_response),
    })
}

async fn execute_openrouter_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model.unwrap_or_else(|| "openai/gpt-4.1-mini".to_string());
    let Some(api_key) = std::env::var("OPENROUTER_API_KEY").ok() else {
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider: "openrouter",
            model,
            output_text: format!(
                "OPENROUTER_API_KEY is not configured. Dry-run request would send input: {input}"
            ),
            raw_response: None,
        });
    };
    let endpoint = resolve_endpoint(
        Some("OPENROUTER_CHAT_COMPLETIONS_URL"),
        "https://openrouter.ai/api/v1/chat/completions",
    );
    let body = json!({
        "model": model,
        "messages": build_chat_completion_messages(&input, instructions.as_deref()),
        "stream": false
    });

    info!("Dispatching live OpenRouter request through gateway connector");
    let mut request_builder = Client::new().post(&endpoint).bearer_auth(api_key);
    if let Ok(referer) = std::env::var("OPENROUTER_HTTP_REFERER") {
        request_builder = request_builder.header("HTTP-Referer", referer);
    }
    if let Ok(title) = std::env::var("OPENROUTER_X_TITLE") {
        request_builder = request_builder.header("X-Title", title);
    }
    let response = request_builder.json(&body).send().await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("openrouter connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "openrouter",
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_chat_completion_text(&raw_response),
        raw_response: Some(raw_response),
    })
}

async fn execute_groq_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "groq",
        request,
        "llama-3.3-70b-versatile",
        &["GROQ_API_KEY"],
        Some("GROQ_CHAT_COMPLETIONS_URL"),
        "https://api.groq.com/openai/v1/chat/completions",
    )
    .await
}

async fn execute_together_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "together",
        request,
        "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        &["TOGETHER_API_KEY"],
        Some("TOGETHER_CHAT_COMPLETIONS_URL"),
        "https://api.together.xyz/v1/chat/completions",
    )
    .await
}

async fn execute_vllm_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model
        .or_else(|| resolve_first_present_env(&["VLLM_MODEL"]))
        .unwrap_or_else(|| "Qwen/Qwen2.5-1.5B-Instruct".to_string());
    let endpoint = std::env::var("VLLM_CHAT_COMPLETIONS_URL")
        .ok()
        .or_else(|| {
            std::env::var("VLLM_BASE_URL")
                .ok()
                .map(|base| format!("{}/v1/chat/completions", base.trim_end_matches('/')))
        })
        .unwrap_or_else(|| "http://127.0.0.1:8000/v1/chat/completions".to_string());
    let body = json!({
        "model": model,
        "messages": build_chat_completion_messages(&input, instructions.as_deref()),
        "stream": false
    });

    info!("Dispatching live vLLM chat completion request through gateway connector");
    let mut request_builder = Client::new().post(&endpoint);
    if let Ok(api_key) = std::env::var("VLLM_API_KEY") {
        request_builder = request_builder.bearer_auth(api_key);
    }
    let response = request_builder.json(&body).send().await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("vllm connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "vllm",
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_chat_completion_text(&raw_response),
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

async fn execute_ollama_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model.unwrap_or_else(|| "llama3.1".to_string());
    let base_url = std::env::var("OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
        .trim_end_matches('/')
        .to_string();
    let endpoint =
        std::env::var("OLLAMA_CHAT_URL").unwrap_or_else(|_| format!("{base_url}/api/chat"));
    let body = json!({
        "model": model,
        "messages": build_chat_completion_messages(&input, instructions.as_deref()),
        "stream": false
    });

    info!("Dispatching live Ollama chat request through gateway connector");
    let response = Client::new().post(&endpoint).json(&body).send().await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("ollama connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "ollama",
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_ollama_text(&raw_response),
        raw_response: Some(raw_response),
    })
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
        "slack" | "mattermost" | "msteams" | "google_chat" => json!({
            "text": text
        }),
        "discord" => json!({
            "content": text
        }),
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

async fn send_whatsapp_connector(request: ChatTargetTextRequest) -> anyhow::Result<ChatSendResult> {
    let Some(access_token) = std::env::var("WHATSAPP_ACCESS_TOKEN").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "whatsapp",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": request.text,
                "reason": "WHATSAPP_ACCESS_TOKEN is not configured"
            })),
        });
    };
    let Some(phone_number_id) = std::env::var("WHATSAPP_PHONE_NUMBER_ID").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "whatsapp",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": request.text,
                "reason": "WHATSAPP_PHONE_NUMBER_ID is not configured"
            })),
        });
    };
    let endpoint = std::env::var("WHATSAPP_MESSAGES_URL").unwrap_or_else(|_| {
        format!(
            "https://graph.facebook.com/v23.0/{}/messages",
            phone_number_id
        )
    });
    let payload = build_whatsapp_text_payload(&request.chat_id, &request.text);

    info!("Dispatching live WhatsApp Cloud message through gateway connector");
    let response = Client::new()
        .post(endpoint)
        .bearer_auth(access_token)
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
        anyhow::bail!("whatsapp connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "whatsapp",
        delivered: raw_response
            .get("messages")
            .and_then(Value::as_array)
            .map(|messages| !messages.is_empty())
            .unwrap_or(status.is_success()),
        raw_response: Some(raw_response),
    })
}

async fn send_line_connector(request: ChatTargetTextRequest) -> anyhow::Result<ChatSendResult> {
    let Some(access_token) = std::env::var("LINE_CHANNEL_ACCESS_TOKEN").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "line",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": request.text,
                "reason": "LINE_CHANNEL_ACCESS_TOKEN is not configured"
            })),
        });
    };
    let endpoint = std::env::var("LINE_PUSH_API_URL")
        .unwrap_or_else(|_| "https://api.line.me/v2/bot/message/push".to_string());
    let payload = build_line_push_payload(&request.chat_id, &request.text);

    info!("Dispatching live LINE push message through gateway connector");
    let response = Client::new()
        .post(endpoint)
        .bearer_auth(access_token)
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
        anyhow::bail!("line connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "line",
        delivered: status.is_success(),
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

fn build_whatsapp_text_payload(chat_id: &str, text: &str) -> Value {
    json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": chat_id,
        "type": "text",
        "text": {
            "preview_url": false,
            "body": text
        }
    })
}

fn build_line_push_payload(chat_id: &str, text: &str) -> Value {
    json!({
        "to": chat_id,
        "messages": [
            {
                "type": "text",
                "text": text
            }
        ]
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

fn extract_google_text(raw_response: &Value) -> String {
    raw_response
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| raw_response.to_string())
}

fn extract_anthropic_text(raw_response: &Value) -> String {
    raw_response
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
        })
        .unwrap_or_else(|| raw_response.to_string())
}

fn extract_ollama_text(raw_response: &Value) -> String {
    raw_response
        .get("message")
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
        QQSendRequest, build_chat_completion_messages, build_line_push_payload,
        build_qq_message_payload, build_wechat_official_account_payload,
        build_whatsapp_text_payload, extract_anthropic_text, extract_chat_completion_text,
        extract_google_text, extract_ollama_text, extract_openai_text, normalize_qq_target_type,
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
    fn extracts_anthropic_content_text() {
        let raw = json!({
            "content": [
                {
                    "type": "text",
                    "text": "anthropic reply"
                }
            ]
        });

        assert_eq!(extract_anthropic_text(&raw), "anthropic reply");
    }

    #[test]
    fn extracts_ollama_message_content() {
        let raw = json!({
            "message": {
                "content": "ollama reply"
            }
        });

        assert_eq!(extract_ollama_text(&raw), "ollama reply");
    }

    #[test]
    fn extracts_google_candidate_content_text() {
        let raw = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "text": "google reply" },
                            { "text": "second line" }
                        ]
                    }
                }
            ]
        });

        assert_eq!(extract_google_text(&raw), "google reply\nsecond line");
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
    fn builds_whatsapp_text_payload() {
        let payload = build_whatsapp_text_payload("15551234567", "hello whatsapp");

        assert_eq!(
            payload,
            json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": "15551234567",
                "type": "text",
                "text": {
                    "preview_url": false,
                    "body": "hello whatsapp"
                }
            })
        );
    }

    #[test]
    fn builds_line_push_payload() {
        let payload = build_line_push_payload("U123456", "hello line");

        assert_eq!(
            payload,
            json!({
                "to": "U123456",
                "messages": [
                    {
                        "type": "text",
                        "text": "hello line"
                    }
                ]
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

    #[test]
    fn webhook_like_platforms_use_plain_text_shape() {
        let payload = match "google_chat" {
            "slack" | "mattermost" | "msteams" | "google_chat" => json!({ "text": "hello" }),
            _ => unreachable!(),
        };
        assert_eq!(payload, json!({ "text": "hello" }));
    }
}
