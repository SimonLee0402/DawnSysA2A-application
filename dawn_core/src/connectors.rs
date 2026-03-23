use std::{io::Write as _, path::Path, process::Command as StdCommand, process::Stdio, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::app_state::AppState;

const DEFAULT_OPENAI_RESPONSES_MODEL: &str = "gpt-4o-mini";
const DEFAULT_OPENAI_COMPATIBLE_MODEL: &str = "openai/gpt-4o-mini";

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
    openai_codex: bool,
    anthropic: bool,
    google: bool,
    bedrock: bool,
    cloudflare_ai_gateway: bool,
    github_models: bool,
    huggingface: bool,
    openrouter: bool,
    groq: bool,
    together: bool,
    vercel_ai_gateway: bool,
    vllm: bool,
    mistral: bool,
    nvidia: bool,
    litellm: bool,
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
    matrix: bool,
    google_chat: bool,
    signal: bool,
    bluebubbles: bool,
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

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignalAccountConfig {
    account: Option<String>,
    base_url: Option<String>,
    send_api_url: Option<String>,
    reaction_api_url: Option<String>,
    receipt_api_url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BlueBubblesAccountConfig {
    password: Option<String>,
    base_url: Option<String>,
    send_message_url: Option<String>,
    send_attachment_url: Option<String>,
    send_reaction_url: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedSignalAccount {
    account: String,
    base_url: String,
    send_api_url: Option<String>,
    reaction_api_url: Option<String>,
    receipt_api_url: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedBlueBubblesAccount {
    password: String,
    base_url: String,
    send_message_url: Option<String>,
    send_attachment_url: Option<String>,
    send_reaction_url: Option<String>,
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
    pub text: Option<String>,
    pub chat_id: Option<String>,
    pub account_key: Option<String>,
    pub attachment_name: Option<String>,
    pub attachment_base64: Option<String>,
    pub attachment_content_type: Option<String>,
    pub reaction: Option<String>,
    pub target_message_id: Option<String>,
    pub target_author: Option<String>,
    pub remove_reaction: Option<bool>,
    pub receipt_type: Option<String>,
    pub typing: Option<String>,
    pub mark_read: Option<bool>,
    pub mark_unread: Option<bool>,
    pub part_index: Option<i64>,
    pub effect_id: Option<String>,
    pub edit_message_id: Option<String>,
    pub edited_text: Option<String>,
    pub unsend_message_id: Option<String>,
    pub participant_action: Option<String>,
    pub participant_address: Option<String>,
    pub group_action: Option<String>,
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub group_description: Option<String>,
    pub group_link_mode: Option<String>,
    pub group_members: Option<Vec<String>>,
    pub group_admins: Option<Vec<String>>,
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
    #[serde(default)]
    pub chat_id: String,
    pub text: Option<String>,
    pub account_key: Option<String>,
    pub attachment_name: Option<String>,
    pub attachment_base64: Option<String>,
    pub attachment_content_type: Option<String>,
    pub reaction: Option<String>,
    pub target_message_id: Option<String>,
    pub target_author: Option<String>,
    pub remove_reaction: Option<bool>,
    pub receipt_type: Option<String>,
    pub typing: Option<String>,
    pub mark_read: Option<bool>,
    pub mark_unread: Option<bool>,
    pub part_index: Option<i64>,
    pub effect_id: Option<String>,
    pub edit_message_id: Option<String>,
    pub edited_text: Option<String>,
    pub unsend_message_id: Option<String>,
    pub participant_action: Option<String>,
    pub participant_address: Option<String>,
    pub group_action: Option<String>,
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub group_description: Option<String>,
    pub group_link_mode: Option<String>,
    pub group_members: Option<Vec<String>>,
    pub group_admins: Option<Vec<String>>,
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
        "openai_codex" => execute_openai_codex_response(request).await,
        "anthropic" => execute_anthropic_response(request).await,
        "google" => execute_google_response(request).await,
        "bedrock" => execute_bedrock_response(request).await,
        "cloudflare_ai_gateway" => execute_cloudflare_ai_gateway_response(request).await,
        "github_models" => execute_github_models_response(request).await,
        "huggingface" => execute_huggingface_response(request).await,
        "openrouter" => execute_openrouter_response(request).await,
        "groq" => execute_groq_response(request).await,
        "together" => execute_together_response(request).await,
        "vercel_ai_gateway" => execute_vercel_ai_gateway_response(request).await,
        "vllm" => execute_vllm_response(request).await,
        "mistral" => execute_mistral_response(request).await,
        "nvidia" => execute_nvidia_response(request).await,
        "litellm" => execute_litellm_response(request).await,
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
            let text = require_chat_text(request.text.as_deref(), "telegram")?;
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("telegram connector requires chatId"))?;
            send_telegram_connector(TelegramSendRequest {
                chat_id,
                text,
                parse_mode: request.parse_mode,
                disable_notification: request.disable_notification,
            })
            .await
        }
        "slack" => {
            send_webhook_connector(
                "slack",
                "SLACK_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "slack")?,
            )
            .await
        }
        "discord" => {
            send_webhook_connector(
                "discord",
                "DISCORD_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "discord")?,
            )
            .await
        }
        "mattermost" => {
            send_webhook_connector(
                "mattermost",
                "MATTERMOST_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "mattermost")?,
            )
            .await
        }
        "msteams" => {
            send_webhook_connector(
                "msteams",
                "MSTEAMS_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "msteams")?,
            )
            .await
        }
        "whatsapp" => {
            let text = require_chat_text(request.text.as_deref(), "whatsapp")?;
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("whatsapp connector requires chatId"))?;
            send_whatsapp_connector(ChatTargetTextRequest {
                chat_id,
                text: Some(text),
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
            })
            .await
        }
        "line" => {
            let text = require_chat_text(request.text.as_deref(), "line")?;
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("line connector requires chatId"))?;
            send_line_connector(ChatTargetTextRequest {
                chat_id,
                text: Some(text),
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
            })
            .await
        }
        "matrix" => {
            let text = require_chat_text(request.text.as_deref(), "matrix")?;
            let chat_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("matrix connector requires chatId"))?;
            send_matrix_connector(ChatTargetTextRequest {
                chat_id,
                text: Some(text),
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
            })
            .await
        }
        "google_chat" => {
            send_webhook_connector(
                "google_chat",
                "GOOGLE_CHAT_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "google_chat")?,
            )
            .await
        }
        "signal" => {
            let chat_id = request
                .chat_id
                .or_else(|| request.group_id.clone())
                .unwrap_or_default();
            send_signal_connector(ChatTargetTextRequest {
                chat_id,
                text: request.text,
                account_key: request.account_key,
                attachment_name: request.attachment_name,
                attachment_base64: request.attachment_base64,
                attachment_content_type: request.attachment_content_type,
                reaction: request.reaction,
                target_message_id: request.target_message_id,
                target_author: request.target_author,
                remove_reaction: request.remove_reaction,
                receipt_type: request.receipt_type,
                typing: request.typing,
                mark_read: request.mark_read,
                mark_unread: request.mark_unread,
                part_index: request.part_index,
                effect_id: request.effect_id,
                edit_message_id: request.edit_message_id,
                edited_text: request.edited_text,
                unsend_message_id: request.unsend_message_id,
                participant_action: request.participant_action,
                participant_address: request.participant_address,
                group_action: request.group_action,
                group_id: request.group_id,
                group_name: request.group_name,
                group_description: request.group_description,
                group_link_mode: request.group_link_mode,
                group_members: request.group_members,
                group_admins: request.group_admins,
            })
            .await
        }
        "bluebubbles" => {
            let chat_id = request.chat_id.unwrap_or_default();
            send_bluebubbles_connector(ChatTargetTextRequest {
                chat_id,
                text: request.text,
                account_key: request.account_key,
                attachment_name: request.attachment_name,
                attachment_base64: request.attachment_base64,
                attachment_content_type: request.attachment_content_type,
                reaction: request.reaction,
                target_message_id: request.target_message_id,
                target_author: request.target_author,
                remove_reaction: request.remove_reaction,
                receipt_type: request.receipt_type,
                typing: request.typing,
                mark_read: request.mark_read,
                mark_unread: request.mark_unread,
                part_index: request.part_index,
                effect_id: request.effect_id,
                edit_message_id: request.edit_message_id,
                edited_text: request.edited_text,
                unsend_message_id: request.unsend_message_id,
                participant_action: request.participant_action,
                participant_address: request.participant_address,
                group_action: request.group_action,
                group_id: request.group_id,
                group_name: request.group_name,
                group_description: request.group_description,
                group_link_mode: request.group_link_mode,
                group_members: request.group_members,
                group_admins: request.group_admins,
            })
            .await
        }
        "feishu" => {
            send_webhook_connector(
                "feishu",
                "FEISHU_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "feishu")?,
            )
            .await
        }
        "dingtalk" => {
            send_webhook_connector(
                "dingtalk",
                "DINGTALK_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "dingtalk")?,
            )
            .await
        }
        "wecom_bot" | "wecom" => {
            send_webhook_connector(
                "wecom_bot",
                "WECOM_BOT_WEBHOOK_URL",
                require_chat_text(request.text.as_deref(), "wecom_bot")?,
            )
            .await
        }
        "wechat_official_account" => {
            let text = require_chat_text(request.text.as_deref(), "wechat_official_account")?;
            let open_id = request.chat_id.ok_or_else(|| {
                anyhow::anyhow!("wechat_official_account connector requires chatId as openId")
            })?;
            send_wechat_official_account_connector(WeChatOfficialAccountSendRequest {
                open_id,
                text,
            })
            .await
        }
        "qq" => {
            let text = require_chat_text(request.text.as_deref(), "qq")?;
            let recipient_id = request
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("qq connector requires chatId as recipient_id"))?;
            send_qq_connector(QQSendRequest {
                recipient_id,
                text,
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
        .route("/model/openai-codex/respond", post(openai_codex_respond))
        .route("/model/anthropic/respond", post(anthropic_respond))
        .route("/model/google/respond", post(google_respond))
        .route("/model/bedrock/respond", post(bedrock_respond))
        .route(
            "/model/cloudflare-ai-gateway/respond",
            post(cloudflare_ai_gateway_respond),
        )
        .route("/model/github-models/respond", post(github_models_respond))
        .route("/model/huggingface/respond", post(huggingface_respond))
        .route("/model/openrouter/respond", post(openrouter_respond))
        .route("/model/groq/respond", post(groq_respond))
        .route("/model/together/respond", post(together_respond))
        .route(
            "/model/vercel-ai-gateway/respond",
            post(vercel_ai_gateway_respond),
        )
        .route("/model/vllm/respond", post(vllm_respond))
        .route("/model/mistral/respond", post(mistral_respond))
        .route("/model/nvidia/respond", post(nvidia_respond))
        .route("/model/litellm/respond", post(litellm_respond))
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
        .route("/chat/matrix/send", post(send_matrix_message))
        .route("/chat/google-chat/send", post(send_google_chat_message))
        .route("/chat/signal/send", post(send_signal_message))
        .route("/chat/bluebubbles/send", post(send_bluebubbles_message))
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
    let openai_codex = openai_codex_login_ready();
    Json(ConnectorStatusReport {
        configured: ConfiguredConnectors {
            openai: std::env::var("OPENAI_API_KEY").is_ok(),
            openai_codex,
            anthropic: std::env::var("ANTHROPIC_API_KEY").is_ok(),
            google: resolve_first_present_env(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]).is_some(),
            bedrock: resolve_first_present_env(&["BEDROCK_API_KEY"]).is_some()
                && (std::env::var("BEDROCK_CHAT_COMPLETIONS_URL").is_ok()
                    || std::env::var("BEDROCK_BASE_URL").is_ok()
                    || std::env::var("BEDROCK_RUNTIME_ENDPOINT").is_ok()),
            cloudflare_ai_gateway: resolve_first_present_env(&[
                "CLOUDFLARE_AI_GATEWAY_API_KEY",
                "OPENAI_API_KEY",
            ])
            .is_some()
                && (std::env::var("CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL").is_ok()
                    || std::env::var("CLOUDFLARE_AI_GATEWAY_BASE_URL").is_ok()
                    || (std::env::var("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID").is_ok()
                        && std::env::var("CLOUDFLARE_AI_GATEWAY_ID").is_ok())),
            github_models: resolve_first_present_env(&["GITHUB_MODELS_API_KEY", "GITHUB_TOKEN"])
                .is_some(),
            huggingface: resolve_first_present_env(&["HUGGINGFACE_API_KEY", "HF_TOKEN"]).is_some(),
            openrouter: std::env::var("OPENROUTER_API_KEY").is_ok(),
            groq: std::env::var("GROQ_API_KEY").is_ok(),
            together: std::env::var("TOGETHER_API_KEY").is_ok(),
            vercel_ai_gateway: resolve_first_present_env(&[
                "VERCEL_AI_GATEWAY_API_KEY",
                "AI_GATEWAY_API_KEY",
            ])
            .is_some()
                || std::env::var("VERCEL_AI_GATEWAY_BASE_URL").is_ok()
                || std::env::var("VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL").is_ok(),
            vllm: std::env::var("VLLM_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("VLLM_BASE_URL").is_ok(),
            mistral: std::env::var("MISTRAL_API_KEY").is_ok(),
            nvidia: resolve_first_present_env(&["NVIDIA_API_KEY", "NVIDIA_NIM_API_KEY"]).is_some(),
            litellm: std::env::var("LITELLM_CHAT_COMPLETIONS_URL").is_ok()
                || std::env::var("LITELLM_BASE_URL").is_ok(),
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
            matrix: std::env::var("MATRIX_ACCESS_TOKEN").is_ok()
                && std::env::var("MATRIX_HOMESERVER_URL").is_ok(),
            google_chat: std::env::var("GOOGLE_CHAT_BOT_WEBHOOK_URL").is_ok(),
            signal: resolve_signal_account_config(None).is_some()
                || std::env::var("DAWN_SIGNAL_ACCOUNTS_JSON").is_ok(),
            bluebubbles: resolve_bluebubbles_account_config(None).is_some()
                || std::env::var("DAWN_BLUEBUBBLES_ACCOUNTS_JSON").is_ok(),
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
                provider: "openai_codex",
                region: "global",
                integration_mode: "live_chatgpt_codex_cli",
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
                provider: "bedrock",
                region: "global",
                integration_mode: "live_openai_compatible_bedrock",
            },
            ModelProviderSupport {
                provider: "cloudflare_ai_gateway",
                region: "global",
                integration_mode: "live_openai_compatible_gateway",
            },
            ModelProviderSupport {
                provider: "github_models",
                region: "global",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "huggingface",
                region: "global",
                integration_mode: "live_openai_compatible_router",
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
                provider: "vercel_ai_gateway",
                region: "global",
                integration_mode: "live_openai_compatible_gateway",
            },
            ModelProviderSupport {
                provider: "vllm",
                region: "local",
                integration_mode: "live_local_openai_compatible",
            },
            ModelProviderSupport {
                provider: "mistral",
                region: "global",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "nvidia",
                region: "global",
                integration_mode: "live_openai_compatible",
            },
            ModelProviderSupport {
                provider: "litellm",
                region: "global",
                integration_mode: "live_openai_compatible_gateway",
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
                platform: "matrix",
                region: "global",
                integration_mode: "live_client_api",
            },
            ChatPlatformSupport {
                platform: "google_chat",
                region: "global",
                integration_mode: "live_webhook",
            },
            ChatPlatformSupport {
                platform: "signal",
                region: "global",
                integration_mode: "live_signal_rest",
            },
            ChatPlatformSupport {
                platform: "bluebubbles",
                region: "global",
                integration_mode: "live_private_api",
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

async fn openai_codex_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_openai_codex_response(request)
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

async fn bedrock_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_bedrock_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn cloudflare_ai_gateway_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_cloudflare_ai_gateway_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn github_models_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_github_models_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn huggingface_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_huggingface_response(request)
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

async fn vercel_ai_gateway_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_vercel_ai_gateway_response(request)
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

async fn mistral_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_mistral_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn nvidia_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_nvidia_response(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn litellm_respond(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<OpenAIResponseRequest>,
) -> Result<Json<ModelResponseResult>, (axum::http::StatusCode, Json<Value>)> {
    execute_litellm_response(request)
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

async fn send_matrix_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ChatTargetTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_matrix_connector(request)
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

async fn send_signal_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ChatTargetTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_signal_connector(request)
        .await
        .map(Json)
        .map_err(connector_anyhow_error)
}

async fn send_bluebubbles_message(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ChatTargetTextRequest>,
) -> Result<Json<ChatSendResult>, (axum::http::StatusCode, Json<Value>)> {
    send_bluebubbles_connector(request)
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
    let model = request
        .model
        .or_else(|| resolve_first_present_env(&["OPENAI_MODEL"]))
        .unwrap_or_else(|| DEFAULT_OPENAI_RESPONSES_MODEL.to_string());
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

async fn execute_openai_codex_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model
        .or_else(|| resolve_first_present_env(&["OPENAI_CODEX_MODEL"]))
        .unwrap_or_else(|| "gpt-5.3-codex".to_string());
    if !openai_codex_login_ready() {
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider: "openai_codex",
            model,
            output_text: format!(
                "OpenAI Codex is not logged in locally. Run `codex login` or `dawn-node models auth-login openai-codex`. Dry-run request would send input: {input}"
            ),
            raw_response: None,
        });
    }

    let prompt = build_openai_codex_prompt(&input, instructions.as_deref());
    let model_for_result = model.clone();
    let execution = tokio::task::spawn_blocking(move || run_openai_codex_exec(&model, &prompt))
        .await
        .map_err(|error| anyhow::anyhow!("OpenAI Codex execution task join failure: {error}"))??;

    if !execution.success {
        anyhow::bail!(
            "OpenAI Codex connector request failed with status {}: {}",
            execution
                .status_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            serde_json::json!({
                "stdout": execution.stdout,
                "stderr": execution.stderr,
            })
        );
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "openai_codex",
        model: model_for_result,
        output_text: execution.output_text,
        raw_response: Some(json!({
            "stdout": execution.stdout,
            "stderr": execution.stderr,
            "statusCode": execution.status_code,
        })),
    })
}

struct OpenAICodexExecResult {
    success: bool,
    status_code: Option<i32>,
    output_text: String,
    stdout: String,
    stderr: String,
}

fn run_openai_codex_exec(model: &str, prompt: &str) -> anyhow::Result<OpenAICodexExecResult> {
    let output_path = std::env::temp_dir().join(format!("dawn-codex-output-{}.txt", Uuid::new_v4()));
    let mut command = new_codex_command(&[
            "exec",
            "-m",
            model,
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--color",
            "never",
            "--output-last-message",
        ]);
    let mut child = command
        .arg(&output_path)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| anyhow::anyhow!("failed to run `codex exec`: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|error| anyhow::anyhow!("failed to write prompt to `codex exec`: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| anyhow::anyhow!("failed while waiting on `codex exec`: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let mut output_text = std::fs::read_to_string(&output_path)
        .unwrap_or_default()
        .trim()
        .to_string();
    let _ = std::fs::remove_file(&output_path);
    if output_text.is_empty() {
        output_text = stdout
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
    }
    Ok(OpenAICodexExecResult {
        success: output.status.success(),
        status_code: output.status.code(),
        output_text,
        stdout,
        stderr,
    })
}

fn build_openai_codex_prompt(input: &str, instructions: Option<&str>) -> String {
    match instructions.map(str::trim).filter(|value| !value.is_empty()) {
        Some(instructions) => format!(
            "System instructions:\n{instructions}\n\nUser input:\n{input}\n\nRespond to the user request directly."
        ),
        None => input.to_string(),
    }
}

pub(crate) fn openai_codex_login_ready() -> bool {
    if codex_auth_file_present() {
        return true;
    }
    let output = new_codex_command(&["login", "status"]).output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("Logged in using")
                || stdout.to_ascii_lowercase().contains("logged in")
        }
        _ => false,
    }
}

fn codex_auth_file_present() -> bool {
    let base = std::env::var("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(std::path::PathBuf::from)
                .map(|home| home.join(".codex"))
        });
    base.map(|dir| dir.join("auth.json").exists()).unwrap_or(false)
}

fn resolve_codex_cli_path() -> std::path::PathBuf {
    if let Ok(explicit) = std::env::var("CODEX_CLI_PATH") {
        let path = std::path::PathBuf::from(explicit);
        if path.exists() {
            return path;
        }
    }
    if let Some(raw_path) = std::env::var_os("PATH") {
        let path_dirs = std::env::split_paths(&raw_path).collect::<Vec<_>>();
        for candidate_name in ["codex.cmd", "codex.exe", "codex"] {
            if let Some(path) = path_dirs
                .iter()
                .map(|dir| dir.join(candidate_name))
                .find(|candidate| candidate.exists())
            {
                return path;
            }
        }
    }
    if let Ok(output) = StdCommand::new("where").arg("codex").output() {
        if output.status.success() {
            let mut candidates = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>();
            candidates.sort_by_key(|path| {
                if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"))
                {
                    0
                } else if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                {
                    1
                } else {
                    2
                }
            });
            if let Some(first) = candidates.into_iter().find(|path| path.exists())
            {
                return first;
            }
        }
    }
    std::path::PathBuf::from("codex")
}

fn new_codex_command(args: &[&str]) -> StdCommand {
    let path = resolve_codex_cli_path();
    let is_cmd_wrapper = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"));
    let mut command = if is_cmd_wrapper {
        let mut command = StdCommand::new("cmd");
        command.arg("/C").arg(&path);
        command
    } else {
        StdCommand::new(&path)
    };
    command.args(args);
    command
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

async fn execute_bedrock_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let endpoint = resolve_openai_style_endpoint(
        &[
            "BEDROCK_CHAT_COMPLETIONS_URL",
            "BEDROCK_BASE_URL",
            "BEDROCK_RUNTIME_ENDPOINT",
        ],
        "https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1/chat/completions",
        "/openai/v1/chat/completions",
    );
    execute_openai_compatible_chat_response_with_custom_endpoint(
        "bedrock",
        request,
        resolve_first_present_env(&["BEDROCK_MODEL"]).as_deref(),
        &["BEDROCK_API_KEY"],
        &endpoint,
    )
    .await
}

async fn execute_openrouter_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model
        .or_else(|| resolve_first_present_env(&["OPENROUTER_MODEL", "OPENAI_MODEL"]))
        .unwrap_or_else(|| DEFAULT_OPENAI_COMPATIBLE_MODEL.to_string());
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

async fn execute_mistral_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "mistral",
        request,
        "mistral-medium-latest",
        &["MISTRAL_API_KEY"],
        Some("MISTRAL_CHAT_COMPLETIONS_URL"),
        "https://api.mistral.ai/v1/chat/completions",
    )
    .await
}

async fn execute_nvidia_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "nvidia",
        request,
        "meta/llama-3.3-70b-instruct",
        &["NVIDIA_API_KEY", "NVIDIA_NIM_API_KEY"],
        Some("NVIDIA_CHAT_COMPLETIONS_URL"),
        "https://integrate.api.nvidia.com/v1/chat/completions",
    )
    .await
}

async fn execute_litellm_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = model
        .or_else(|| resolve_first_present_env(&["LITELLM_MODEL", "OPENAI_MODEL"]))
        .unwrap_or_else(|| DEFAULT_OPENAI_COMPATIBLE_MODEL.to_string());
    let Some(endpoint) = std::env::var("LITELLM_CHAT_COMPLETIONS_URL")
        .ok()
        .or_else(|| {
            std::env::var("LITELLM_BASE_URL")
                .ok()
                .map(|base| format!("{}/chat/completions", base.trim_end_matches('/')))
        })
    else {
        return Ok(ModelResponseResult {
            mode: "dry_run",
            provider: "litellm",
            model,
            output_text: format!(
                "LITELLM_CHAT_COMPLETIONS_URL or LITELLM_BASE_URL is not configured. Dry-run request would send input: {input}"
            ),
            raw_response: None,
        });
    };
    let body = json!({
        "model": model,
        "messages": build_chat_completion_messages(&input, instructions.as_deref()),
        "stream": false
    });

    info!("Dispatching live LiteLLM chat completion request through gateway connector");
    let mut request_builder = Client::new().post(&endpoint);
    if let Ok(api_key) = std::env::var("LITELLM_API_KEY") {
        request_builder = request_builder.bearer_auth(api_key);
    }
    let response = request_builder.json(&body).send().await?;
    let status = response.status();
    let raw_response = response.json::<Value>().await?;

    if !status.is_success() {
        anyhow::bail!("litellm connector request failed with status {status}: {raw_response}");
    }

    Ok(ModelResponseResult {
        mode: "live",
        provider: "litellm",
        model: body["model"].as_str().unwrap_or("unknown").to_string(),
        output_text: extract_chat_completion_text(&raw_response),
        raw_response: Some(raw_response),
    })
}

async fn execute_cloudflare_ai_gateway_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    let endpoint = resolve_cloudflare_ai_gateway_endpoint()?;
    execute_openai_compatible_chat_response_with_custom_endpoint(
        "cloudflare_ai_gateway",
        request,
        resolve_first_present_env(&["CLOUDFLARE_AI_GATEWAY_MODEL", "OPENAI_MODEL"])
            .as_deref()
            .or(Some(DEFAULT_OPENAI_RESPONSES_MODEL)),
        &["CLOUDFLARE_AI_GATEWAY_API_KEY", "OPENAI_API_KEY"],
        endpoint.as_str(),
    )
    .await
}

async fn execute_github_models_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "github_models",
        request,
        DEFAULT_OPENAI_COMPATIBLE_MODEL,
        &["GITHUB_MODELS_API_KEY", "GITHUB_TOKEN"],
        Some("GITHUB_MODELS_CHAT_COMPLETIONS_URL"),
        "https://models.github.ai/inference/chat/completions",
    )
    .await
}

async fn execute_huggingface_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "huggingface",
        request,
        "meta-llama/Llama-3.1-8B-Instruct",
        &["HUGGINGFACE_API_KEY", "HF_TOKEN"],
        Some("HUGGINGFACE_CHAT_COMPLETIONS_URL"),
        "https://router.huggingface.co/v1/chat/completions",
    )
    .await
}

async fn execute_vercel_ai_gateway_response(
    request: OpenAIResponseRequest,
) -> anyhow::Result<ModelResponseResult> {
    execute_openai_compatible_chat_response(
        "vercel_ai_gateway",
        request,
        DEFAULT_OPENAI_COMPATIBLE_MODEL,
        &["VERCEL_AI_GATEWAY_API_KEY", "AI_GATEWAY_API_KEY"],
        Some("VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL"),
        "https://ai-gateway.vercel.sh/v1/chat/completions",
    )
    .await
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
        .json(&build_telegram_send_payload(&request))
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

fn build_telegram_send_payload(request: &TelegramSendRequest) -> Value {
    let mut payload = json!({
        "chat_id": request.chat_id,
        "text": request.text,
        "disable_notification": request.disable_notification.unwrap_or(false)
    });
    if let Some(parse_mode) = request
        .parse_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        payload["parse_mode"] = Value::String(parse_mode.to_string());
    }
    payload
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
    let text = require_chat_text(request.text.as_deref(), "whatsapp")?;
    let Some(access_token) = std::env::var("WHATSAPP_ACCESS_TOKEN").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "whatsapp",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": text,
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
                "text": text,
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
    let payload = build_whatsapp_text_payload(&request.chat_id, &text);

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
    let text = require_chat_text(request.text.as_deref(), "line")?;
    let Some(access_token) = std::env::var("LINE_CHANNEL_ACCESS_TOKEN").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "line",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": text,
                "reason": "LINE_CHANNEL_ACCESS_TOKEN is not configured"
            })),
        });
    };
    let endpoint = std::env::var("LINE_PUSH_API_URL")
        .unwrap_or_else(|_| "https://api.line.me/v2/bot/message/push".to_string());
    let payload = build_line_push_payload(&request.chat_id, &text);

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

async fn send_matrix_connector(request: ChatTargetTextRequest) -> anyhow::Result<ChatSendResult> {
    let text = require_chat_text(request.text.as_deref(), "matrix")?;
    let Some(access_token) = std::env::var("MATRIX_ACCESS_TOKEN").ok() else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "matrix",
            delivered: false,
            raw_response: Some(json!({
                "chatId": request.chat_id,
                "text": text,
                "reason": "MATRIX_ACCESS_TOKEN is not configured"
            })),
        });
    };
    let homeserver = std::env::var("MATRIX_HOMESERVER_URL")
        .unwrap_or_else(|_| "https://matrix-client.matrix.org".to_string());
    let txn_id = Uuid::new_v4().to_string();
    let endpoint = build_matrix_send_endpoint(&homeserver, &request.chat_id, &txn_id)?;
    let payload = build_matrix_text_payload(&text);

    info!("Dispatching live Matrix room message through gateway connector");
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
        anyhow::bail!("matrix connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "matrix",
        delivered: raw_response.get("event_id").is_some() || status.is_success(),
        raw_response: Some(raw_response),
    })
}

async fn send_signal_connector(request: ChatTargetTextRequest) -> anyhow::Result<ChatSendResult> {
    let Some(account) = resolve_signal_account_config(request.account_key.as_deref()) else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "signal",
            delivered: false,
            raw_response: Some(json!({
                "accountKey": request.account_key,
                "chatId": request.chat_id,
                "text": request.text,
                "attachmentName": request.attachment_name,
                "reaction": request.reaction,
                "targetMessageId": request.target_message_id,
                "receiptType": request.receipt_type,
                "groupAction": request.group_action,
                "groupId": request.group_id,
                "reason": "SIGNAL account is not configured"
            })),
        });
    };

    if request.group_action.is_some() {
        return send_signal_group_action_connector(&account, request).await;
    }

    if request.typing.is_some()
        || request.mark_read == Some(true)
        || request.mark_unread == Some(true)
        || request.effect_id.is_some()
        || request.edit_message_id.is_some()
        || request.edited_text.is_some()
        || request.unsend_message_id.is_some()
        || request.participant_action.is_some()
        || request.participant_address.is_some()
    {
        anyhow::bail!(
            "signal connector does not support typing, mark-read, edit/unsend, effect, or bluebubbles-style participant actions"
        );
    }

    if request.receipt_type.is_some() {
        return send_signal_receipt_connector(&account, request).await;
    }

    if request.reaction.is_some() || request.remove_reaction == Some(true) {
        return send_signal_reaction_connector(&account, request).await;
    }

    send_signal_message_connector(&account, request).await
}

async fn send_bluebubbles_connector(
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let Some(account) = resolve_bluebubbles_account_config(request.account_key.as_deref()) else {
        return Ok(ChatSendResult {
            mode: "dry_run",
            platform: "bluebubbles",
            delivered: false,
            raw_response: Some(json!({
                "accountKey": request.account_key,
                "chatId": request.chat_id,
                "text": request.text,
                "attachmentName": request.attachment_name,
                "reaction": request.reaction,
                "targetMessageId": request.target_message_id,
                "typing": request.typing,
                "markRead": request.mark_read,
                "markUnread": request.mark_unread,
                "editMessageId": request.edit_message_id,
                "unsendMessageId": request.unsend_message_id,
                "participantAction": request.participant_action,
                "participantAddress": request.participant_address,
                "groupName": request.group_name,
                "reason": "BlueBubbles account is not configured"
            })),
        });
    };

    if request.group_name.is_some() {
        return send_bluebubbles_rename_group_connector(&account, request).await;
    }

    if request.participant_action.is_some() || request.participant_address.is_some() {
        return send_bluebubbles_participant_connector(&account, request).await;
    }

    if request.typing.is_some() {
        return send_bluebubbles_typing_connector(&account, request).await;
    }

    if request.mark_read == Some(true) {
        return send_bluebubbles_mark_read_connector(&account, request).await;
    }

    if request.mark_unread == Some(true) {
        return send_bluebubbles_mark_unread_connector(&account, request).await;
    }

    if request.edit_message_id.is_some() || request.edited_text.is_some() {
        return send_bluebubbles_edit_connector(&account, request).await;
    }

    if request.unsend_message_id.is_some() {
        return send_bluebubbles_unsend_connector(&account, request).await;
    }

    if request.reaction.is_some() {
        return send_bluebubbles_reaction_connector(&account, request).await;
    }

    if request.receipt_type.is_some() || request.remove_reaction == Some(true) {
        anyhow::bail!(
            "bluebubbles connector does not support receipt_type or remove_reaction outside native reaction values"
        );
    }

    if request.attachment_base64.is_some() {
        return send_bluebubbles_attachment_connector(&account, request).await;
    }

    send_bluebubbles_text_connector(&account, request).await
}

struct DecodedAttachment {
    name: String,
    bytes: Vec<u8>,
    content_type: Option<String>,
}

async fn send_signal_message_connector(
    account: &ResolvedSignalAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let attachment = decode_attachment(&request)?;
    let text = request
        .text
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    if text.is_none() && attachment.is_none() {
        anyhow::bail!("signal connector requires text or attachment content");
    }

    let endpoint = resolve_signal_send_endpoint(account)?;
    let payload = build_signal_send_payload(
        &account.account,
        &request.chat_id,
        text,
        attachment.as_ref(),
    );

    info!("Dispatching live Signal message through gateway connector");
    let response = Client::new().post(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("signal connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "signal",
        delivered: raw_response.get("timestamp").is_some() || status.is_success(),
        raw_response: Some(raw_response),
    })
}

async fn send_signal_reaction_connector(
    account: &ResolvedSignalAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let target_message_id = request
        .target_message_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("signal reactions require targetMessageId"))?;
    let endpoint = resolve_signal_reaction_endpoint(account)?;
    let payload = build_signal_reaction_payload(
        &request.chat_id,
        target_message_id,
        request.reaction.as_deref(),
        request.target_author.as_deref(),
    );
    let remove = request.remove_reaction.unwrap_or(false);
    let method = if remove {
        reqwest::Method::DELETE
    } else {
        reqwest::Method::POST
    };

    info!("Dispatching live Signal reaction through gateway connector");
    let response = Client::new()
        .request(method, endpoint)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("signal reaction request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "signal",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_signal_receipt_connector(
    account: &ResolvedSignalAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let target_message_id = request
        .target_message_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("signal receipts require targetMessageId"))?;
    let receipt_type = request
        .receipt_type
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("read");
    let endpoint = resolve_signal_receipt_endpoint(account)?;
    let payload = build_signal_receipt_payload(&request.chat_id, target_message_id, receipt_type);

    info!("Dispatching live Signal receipt through gateway connector");
    let response = Client::new().post(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("signal receipt request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "signal",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_signal_group_action_connector(
    account: &ResolvedSignalAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let action = request
        .group_action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("signal group actions require groupAction"))?
        .to_ascii_lowercase();
    let endpoint = resolve_signal_group_endpoint(
        account,
        request.group_id.as_deref(),
        signal_group_action_suffix(&action)?,
    )?;
    let method = signal_group_action_method(&action)?;
    let payload = build_signal_group_action_payload(&action, &request)?;

    info!("Dispatching live Signal group action through gateway connector");
    let mut request_builder = Client::new().request(method, endpoint);
    if let Some(payload) = payload {
        request_builder = request_builder.json(&payload);
    }
    let response = request_builder.send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("signal group action `{action}` failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "signal",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_text_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let text = request
        .text
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("bluebubbles connector requires text"))?;
    let endpoint = resolve_bluebubbles_send_endpoint(account)?;
    let payload = build_bluebubbles_text_payload(
        &request.chat_id,
        text,
        &format!("dawn-{}", Uuid::new_v4()),
        request.target_message_id.as_deref(),
        request.effect_id.as_deref(),
    );

    info!("Dispatching live BlueBubbles text message through gateway connector");
    let response = Client::new().post(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles connector request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: status.is_success(),
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_attachment_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    if request
        .text
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        anyhow::bail!("bluebubbles attachment send currently supports attachment-only messages");
    }

    let attachment = decode_attachment(&request)?.ok_or_else(|| {
        anyhow::anyhow!("bluebubbles attachment send requires attachment content")
    })?;
    let endpoint = resolve_bluebubbles_attachment_endpoint(account)?;
    let mut form = Form::new()
        .text("chatGuid", request.chat_id)
        .text("name", attachment.name.clone())
        .text("method", "private-api");
    if let Some(target_message_id) = request.target_message_id.as_deref() {
        form = form.text("selectedMessageGuid", target_message_id.to_string());
    }
    if let Some(effect_id) = request
        .effect_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        form = form.text("effectId", effect_id.to_string());
    }
    if let Some(part_index) = request.part_index {
        form = form.text("partIndex", part_index.to_string());
    }
    let mut part = Part::bytes(attachment.bytes).file_name(attachment.name);
    if let Some(content_type) = attachment.content_type.as_deref() {
        part = part
            .mime_str(content_type)
            .map_err(|error| anyhow::anyhow!("invalid attachment content type: {error}"))?;
    }
    form = form.part("attachment", part);

    info!("Dispatching live BlueBubbles attachment through gateway connector");
    let response = Client::new().post(endpoint).multipart(form).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles attachment request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_reaction_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let target_message_id = request
        .target_message_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bluebubbles reactions require targetMessageId"))?;
    let reaction = normalize_bluebubbles_reaction(
        request
            .reaction
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("bluebubbles reactions require reaction"))?,
        request.remove_reaction.unwrap_or(false),
    )?;
    let endpoint = resolve_bluebubbles_reaction_endpoint(account)?;
    let payload = build_bluebubbles_reaction_payload(
        &request.chat_id,
        target_message_id,
        &reaction,
        request.part_index,
    );

    info!("Dispatching live BlueBubbles reaction through gateway connector");
    let response = Client::new().post(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles reaction request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_typing_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let typing = request
        .typing
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bluebubbles typing action requires typing=start|stop"))?;
    let action = match typing.trim().to_ascii_lowercase().as_str() {
        "start" | "started" => ("start", reqwest::Method::POST),
        "stop" | "stopped" => ("stop", reqwest::Method::DELETE),
        other => anyhow::bail!("unsupported bluebubbles typing action: {other}"),
    };
    let endpoint = resolve_bluebubbles_chat_action_endpoint(account, &request.chat_id, "typing")?;

    info!("Dispatching live BlueBubbles typing action through gateway connector");
    let response = Client::new().request(action.1, endpoint).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!(
            "bluebubbles {} typing request failed with status {status}: {raw_response}",
            action.0
        );
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_mark_read_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let endpoint = resolve_bluebubbles_chat_action_endpoint(account, &request.chat_id, "read")?;

    info!("Dispatching live BlueBubbles mark-read action through gateway connector");
    let response = Client::new().post(endpoint).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles mark-read request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_mark_unread_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let endpoint = resolve_bluebubbles_chat_action_endpoint(account, &request.chat_id, "unread")?;

    info!("Dispatching live BlueBubbles mark-unread action through gateway connector");
    let response = Client::new().post(endpoint).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!(
            "bluebubbles mark-unread request failed with status {status}: {raw_response}"
        );
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_edit_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let message_guid = request
        .edit_message_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bluebubbles edit requires editMessageId"))?;
    let edited_text = request
        .edited_text
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("bluebubbles edit requires editedText"))?;
    let endpoint = resolve_bluebubbles_message_action_endpoint(account, message_guid, "edit")?;
    let payload = json!({
        "editedMessage": edited_text,
        "backwardsCompatibilityMessage": request.text.as_deref().unwrap_or(edited_text),
        "partIndex": request.part_index,
    });

    info!("Dispatching live BlueBubbles edit through gateway connector");
    let response = Client::new().post(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles edit request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_unsend_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let message_guid = request
        .unsend_message_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bluebubbles unsend requires unsendMessageId"))?;
    let endpoint = resolve_bluebubbles_message_action_endpoint(account, message_guid, "unsend")?;
    let mut payload = json!({});
    if let Some(part_index) = request.part_index {
        payload["partIndex"] = json!(part_index);
    }

    info!("Dispatching live BlueBubbles unsend through gateway connector");
    let response = Client::new().post(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles unsend request failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_participant_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let action = request
        .participant_action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("add")
        .to_ascii_lowercase();
    let address = request
        .participant_address
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("bluebubbles participant changes require participantAddress")
        })?;
    let method = match action.as_str() {
        "add" => reqwest::Method::POST,
        "remove" => reqwest::Method::DELETE,
        other => anyhow::bail!("unsupported bluebubbles participantAction: {other}"),
    };
    let endpoint =
        resolve_bluebubbles_chat_action_endpoint(account, &request.chat_id, "participant")?;
    let payload = json!({ "address": address });

    info!("Dispatching live BlueBubbles participant action through gateway connector");
    let response = Client::new()
        .request(method, endpoint)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!(
            "bluebubbles participant action `{action}` failed with status {status}: {raw_response}"
        );
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
        delivered: true,
        raw_response: Some(raw_response),
    })
}

async fn send_bluebubbles_rename_group_connector(
    account: &ResolvedBlueBubblesAccount,
    request: ChatTargetTextRequest,
) -> anyhow::Result<ChatSendResult> {
    let display_name = request
        .group_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("bluebubbles group rename requires groupName"))?;
    let endpoint = resolve_bluebubbles_chat_update_endpoint(account, &request.chat_id)?;
    let payload = json!({ "displayName": display_name });

    info!("Dispatching live BlueBubbles group rename through gateway connector");
    let response = Client::new().put(endpoint).json(&payload).send().await?;
    let status = response.status();
    let raw_response = parse_connector_response(response).await?;

    if !status.is_success() {
        anyhow::bail!("bluebubbles group rename failed with status {status}: {raw_response}");
    }

    Ok(ChatSendResult {
        mode: "live",
        platform: "bluebubbles",
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
    execute_openai_compatible_chat_response_with_custom_endpoint(
        provider,
        request,
        Some(default_model),
        api_key_env_vars,
        &resolve_endpoint(endpoint_env_var, default_endpoint),
    )
    .await
}

async fn execute_openai_compatible_chat_response_with_custom_endpoint(
    provider: &'static str,
    request: OpenAIResponseRequest,
    default_model: Option<&str>,
    api_key_env_vars: &[&str],
    endpoint: &str,
) -> anyhow::Result<ModelResponseResult> {
    let OpenAIResponseRequest {
        input,
        model,
        instructions,
    } = request;
    let model = match model {
        Some(model) => model,
        None => default_model.map(str::to_string).ok_or_else(|| {
            anyhow::anyhow!("{provider} requires an explicit model or configured default model")
        })?,
    };
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

    info!("Dispatching live {provider} chat completion request through gateway connector");
    let body = json!({
        "model": model,
        "messages": build_chat_completion_messages(&input, instructions.as_deref()),
        "stream": false
    });
    let response = Client::new()
        .post(endpoint)
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

fn resolve_openai_style_endpoint(
    env_vars: &[&str],
    default_endpoint: &str,
    suffix: &str,
) -> String {
    resolve_first_present_env(env_vars)
        .map(|value| {
            let trimmed = value.trim().trim_end_matches('/');
            if trimmed.ends_with("/chat/completions") {
                trimmed.to_string()
            } else {
                format!("{trimmed}{suffix}")
            }
        })
        .unwrap_or_else(|| default_endpoint.to_string())
}

fn resolve_cloudflare_ai_gateway_endpoint() -> anyhow::Result<String> {
    if let Ok(endpoint) = std::env::var("CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL") {
        let trimmed = endpoint.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    if let Ok(base_url) = std::env::var("CLOUDFLARE_AI_GATEWAY_BASE_URL") {
        let trimmed = base_url.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            return Ok(format!("{trimmed}/chat/completions"));
        }
    }
    let account_id = std::env::var("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID")
        .map(|value| value.trim().to_string())
        .ok();
    let gateway_id = std::env::var("CLOUDFLARE_AI_GATEWAY_ID")
        .map(|value| value.trim().to_string())
        .ok();
    match (account_id, gateway_id) {
        (Some(account_id), Some(gateway_id))
            if !account_id.is_empty() && !gateway_id.is_empty() =>
        {
            Ok(format!(
                "https://gateway.ai.cloudflare.com/v1/{account_id}/{gateway_id}/openai/chat/completions"
            ))
        }
        _ => anyhow::bail!(
            "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL or CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID + CLOUDFLARE_AI_GATEWAY_ID is required"
        ),
    }
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

fn build_matrix_text_payload(text: &str) -> Value {
    json!({
        "msgtype": "m.text",
        "body": text
    })
}

fn build_signal_send_payload(
    account: &str,
    recipient: &str,
    text: Option<&str>,
    attachment: Option<&DecodedAttachment>,
) -> Value {
    let mut payload = json!({
        "number": account,
        "recipients": [recipient]
    });
    if let Some(text) = text {
        payload["message"] = json!(text);
    }
    if let Some(attachment) = attachment {
        let content_type = attachment
            .content_type
            .as_deref()
            .unwrap_or("application/octet-stream");
        let encoded = BASE64_STANDARD.encode(&attachment.bytes);
        payload["base64_attachments"] = json!([format!(
            "data:{content_type};filename={};base64,{encoded}",
            attachment.name
        )]);
    }
    payload
}

fn build_signal_reaction_payload(
    recipient: &str,
    target_message_id: &str,
    reaction: Option<&str>,
    target_author: Option<&str>,
) -> Value {
    let timestamp = parse_numeric_or_string(target_message_id);
    let mut payload = json!({
        "recipient": recipient,
        "timestamp": timestamp,
        "target_author": target_author.unwrap_or(recipient),
    });
    if let Some(reaction) = reaction.filter(|value| !value.trim().is_empty()) {
        payload["reaction"] = json!(reaction);
    }
    payload
}

fn build_signal_receipt_payload(
    recipient: &str,
    target_message_id: &str,
    receipt_type: &str,
) -> Value {
    json!({
        "recipient": recipient,
        "timestamp": parse_numeric_or_string(target_message_id),
        "receipt_type": receipt_type,
    })
}

fn build_matrix_send_endpoint(
    homeserver: &str,
    room_id: &str,
    transaction_id: &str,
) -> anyhow::Result<Url> {
    let mut url = Url::parse(homeserver)?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid MATRIX_HOMESERVER_URL path"))?;
        segments.pop_if_empty();
        segments.extend(["_matrix", "client", "v3", "rooms"]);
        segments.push(room_id);
        segments.extend(["send", "m.room.message"]);
        segments.push(transaction_id);
    }
    Ok(url)
}

fn build_bluebubbles_text_payload(
    chat_guid: &str,
    text: &str,
    temp_guid: &str,
    selected_message_guid: Option<&str>,
    effect_id: Option<&str>,
) -> Value {
    let mut payload = json!({
        "chatGuid": chat_guid,
        "message": text,
        "tempGuid": temp_guid
    });
    if let Some(selected_message_guid) =
        selected_message_guid.filter(|value| !value.trim().is_empty())
    {
        payload["selectedMessageGuid"] = json!(selected_message_guid);
    }
    if let Some(effect_id) = effect_id.filter(|value| !value.trim().is_empty()) {
        payload["effectId"] = json!(effect_id);
    }
    payload
}

fn build_bluebubbles_reaction_payload(
    chat_guid: &str,
    selected_message_guid: &str,
    reaction: &str,
    part_index: Option<i64>,
) -> Value {
    let mut payload = json!({
        "chatGuid": chat_guid,
        "selectedMessageGuid": selected_message_guid,
        "reaction": reaction,
    });
    if let Some(part_index) = part_index {
        payload["partIndex"] = json!(part_index);
    }
    payload
}

fn resolve_signal_send_endpoint(account: &ResolvedSignalAccount) -> anyhow::Result<Url> {
    let endpoint = account.send_api_url.clone().unwrap_or_else(|| {
        let trimmed = account.base_url.trim_end_matches('/');
        if trimmed.ends_with("/v2/send") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/v2/send")
        }
    });
    Url::parse(&endpoint).map_err(Into::into)
}

fn resolve_signal_reaction_endpoint(account: &ResolvedSignalAccount) -> anyhow::Result<Url> {
    resolve_signal_account_endpoint(
        account,
        account.reaction_api_url.as_deref(),
        &["v1", "reactions"],
    )
}

fn resolve_signal_receipt_endpoint(account: &ResolvedSignalAccount) -> anyhow::Result<Url> {
    resolve_signal_account_endpoint(
        account,
        account.receipt_api_url.as_deref(),
        &["v1", "receipts"],
    )
}

fn resolve_signal_account_endpoint(
    account: &ResolvedSignalAccount,
    explicit_url: Option<&str>,
    segments: &[&str],
) -> anyhow::Result<Url> {
    if let Some(explicit_url) = explicit_url {
        return Url::parse(explicit_url).map_err(Into::into);
    }
    let mut url = Url::parse(&account.base_url)?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid Signal REST API base url"))?;
        path.pop_if_empty();
        path.extend(segments);
        path.push(&account.account);
    }
    Ok(url)
}

fn resolve_signal_group_endpoint(
    account: &ResolvedSignalAccount,
    group_id: Option<&str>,
    suffix: Vec<String>,
) -> anyhow::Result<Url> {
    let mut url = Url::parse(&account.base_url)?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid Signal REST API base url"))?;
        path.pop_if_empty();
        path.extend(["v1", "groups"]);
        path.push(&account.account);
        for segment in suffix {
            if segment == "{groupId}" {
                let group_id = group_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("signal group action requires groupId"))?;
                path.push(group_id);
            } else {
                path.push(&segment);
            }
        }
    }
    Ok(url)
}

fn resolve_bluebubbles_send_endpoint(account: &ResolvedBlueBubblesAccount) -> anyhow::Result<Url> {
    resolve_bluebubbles_authed_endpoint(
        account,
        account.send_message_url.as_deref(),
        &["api", "v1", "message", "text"],
    )
}

fn resolve_bluebubbles_attachment_endpoint(
    account: &ResolvedBlueBubblesAccount,
) -> anyhow::Result<Url> {
    resolve_bluebubbles_authed_endpoint(
        account,
        account.send_attachment_url.as_deref(),
        &["api", "v1", "message", "attachment"],
    )
}

fn resolve_bluebubbles_reaction_endpoint(
    account: &ResolvedBlueBubblesAccount,
) -> anyhow::Result<Url> {
    resolve_bluebubbles_authed_endpoint(
        account,
        account.send_reaction_url.as_deref(),
        &["api", "v1", "message", "react"],
    )
}

fn resolve_bluebubbles_chat_action_endpoint(
    account: &ResolvedBlueBubblesAccount,
    chat_guid: &str,
    action: &str,
) -> anyhow::Result<Url> {
    resolve_bluebubbles_authed_endpoint(account, None, &["api", "v1", "chat", chat_guid, action])
}

fn resolve_bluebubbles_chat_update_endpoint(
    account: &ResolvedBlueBubblesAccount,
    chat_guid: &str,
) -> anyhow::Result<Url> {
    resolve_bluebubbles_authed_endpoint(account, None, &["api", "v1", "chat", chat_guid])
}

fn resolve_bluebubbles_message_action_endpoint(
    account: &ResolvedBlueBubblesAccount,
    message_guid: &str,
    action: &str,
) -> anyhow::Result<Url> {
    resolve_bluebubbles_authed_endpoint(
        account,
        None,
        &["api", "v1", "message", message_guid, action],
    )
}

fn resolve_bluebubbles_authed_endpoint(
    account: &ResolvedBlueBubblesAccount,
    explicit_url: Option<&str>,
    segments: &[&str],
) -> anyhow::Result<Url> {
    let mut url = if let Some(explicit_url) = explicit_url {
        Url::parse(explicit_url)?
    } else {
        bluebubbles_base_url(account)?
    };

    if url.path() == "/" || explicit_url.is_none() || url.path().ends_with('/') {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid BLUEBUBBLES_SERVER_URL path"))?;
        path.pop_if_empty();
        path.extend(segments);
    }

    let has_auth = url
        .query_pairs()
        .any(|(key, _)| key == "guid" || key == "password");
    if !has_auth {
        url.query_pairs_mut().append_pair("guid", &account.password);
    }
    Ok(url)
}

fn bluebubbles_base_url(account: &ResolvedBlueBubblesAccount) -> anyhow::Result<Url> {
    Url::parse(&account.base_url).map_err(Into::into)
}

fn decode_attachment(request: &ChatTargetTextRequest) -> anyhow::Result<Option<DecodedAttachment>> {
    let Some(encoded) = request
        .attachment_base64
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|error| anyhow::anyhow!("invalid attachmentBase64 payload: {error}"))?;
    let name = request
        .attachment_name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "attachment.bin".to_string());
    Ok(Some(DecodedAttachment {
        name,
        bytes,
        content_type: request
            .attachment_content_type
            .clone()
            .or_else(|| infer_content_type_from_name(request.attachment_name.as_deref())),
    }))
}

fn infer_content_type_from_name(name: Option<&str>) -> Option<String> {
    let extension = Path::new(name?).extension()?.to_str()?.to_ascii_lowercase();
    let content_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => return None,
    };
    Some(content_type.to_string())
}

fn parse_numeric_or_string(value: &str) -> Value {
    value
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::from(value.to_string()))
}

fn require_chat_text(text: Option<&str>, platform: &str) -> anyhow::Result<String> {
    text.filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("{platform} connector requires text"))
}

async fn parse_connector_response(response: reqwest::Response) -> anyhow::Result<Value> {
    let status = response.status();
    Ok(match response.json::<Value>().await {
        Ok(value) => value,
        Err(_) => json!({
            "status": status.as_u16(),
            "payloadAccepted": status.is_success()
        }),
    })
}

fn resolve_signal_account_config(account_key: Option<&str>) -> Option<ResolvedSignalAccount> {
    let normalized_key = account_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if let Some(config) = normalized_key
        .as_deref()
        .and_then(parse_signal_account_registry)
    {
        return Some(config);
    }

    resolve_first_present_env(&["SIGNAL_ACCOUNT", "SIGNAL_NUMBER"]).map(|account| {
        ResolvedSignalAccount {
            account,
            base_url: resolve_first_present_env(&["SIGNAL_HTTP_URL", "SIGNAL_CLI_REST_API_URL"])
                .unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
            send_api_url: std::env::var("SIGNAL_SEND_API_URL").ok(),
            reaction_api_url: std::env::var("SIGNAL_REACTION_API_URL").ok(),
            receipt_api_url: std::env::var("SIGNAL_RECEIPT_API_URL").ok(),
        }
    })
}

fn parse_signal_account_registry(account_key: &str) -> Option<ResolvedSignalAccount> {
    let raw = std::env::var("DAWN_SIGNAL_ACCOUNTS_JSON").ok()?;
    let registry = serde_json::from_str::<serde_json::Map<String, Value>>(&raw).ok()?;
    let entry = registry.get(account_key)?;
    let config = serde_json::from_value::<SignalAccountConfig>(entry.clone()).ok()?;
    let account = config.account?.trim().to_string();
    if account.is_empty() {
        return None;
    }
    let base_url = config
        .base_url
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string())
        .trim()
        .to_string();
    if base_url.is_empty() {
        return None;
    }
    Some(ResolvedSignalAccount {
        account,
        base_url,
        send_api_url: config.send_api_url.filter(|value| !value.trim().is_empty()),
        reaction_api_url: config
            .reaction_api_url
            .filter(|value| !value.trim().is_empty()),
        receipt_api_url: config
            .receipt_api_url
            .filter(|value| !value.trim().is_empty()),
    })
}

fn resolve_bluebubbles_account_config(
    account_key: Option<&str>,
) -> Option<ResolvedBlueBubblesAccount> {
    let normalized_key = account_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if let Some(config) = normalized_key
        .as_deref()
        .and_then(parse_bluebubbles_account_registry)
    {
        return Some(config);
    }

    let password = std::env::var("BLUEBUBBLES_PASSWORD").ok()?;
    let base_url = resolve_first_present_env(&["BLUEBUBBLES_SERVER_URL"])
        .or_else(|| std::env::var("BLUEBUBBLES_SEND_MESSAGE_URL").ok())
        .unwrap_or_default();
    if base_url.trim().is_empty() {
        return None;
    }
    Some(ResolvedBlueBubblesAccount {
        password,
        base_url,
        send_message_url: std::env::var("BLUEBUBBLES_SEND_MESSAGE_URL").ok(),
        send_attachment_url: std::env::var("BLUEBUBBLES_SEND_ATTACHMENT_URL").ok(),
        send_reaction_url: std::env::var("BLUEBUBBLES_SEND_REACTION_URL").ok(),
    })
}

fn parse_bluebubbles_account_registry(account_key: &str) -> Option<ResolvedBlueBubblesAccount> {
    let raw = std::env::var("DAWN_BLUEBUBBLES_ACCOUNTS_JSON").ok()?;
    let registry = serde_json::from_str::<serde_json::Map<String, Value>>(&raw).ok()?;
    let entry = registry.get(account_key)?;
    let config = serde_json::from_value::<BlueBubblesAccountConfig>(entry.clone()).ok()?;
    let password = config.password?.trim().to_string();
    if password.is_empty() {
        return None;
    }
    let base_url = config.base_url?.trim().to_string();
    if base_url.is_empty() {
        return None;
    }
    Some(ResolvedBlueBubblesAccount {
        password,
        base_url,
        send_message_url: config
            .send_message_url
            .filter(|value| !value.trim().is_empty()),
        send_attachment_url: config
            .send_attachment_url
            .filter(|value| !value.trim().is_empty()),
        send_reaction_url: config
            .send_reaction_url
            .filter(|value| !value.trim().is_empty()),
    })
}

fn signal_group_action_method(action: &str) -> anyhow::Result<reqwest::Method> {
    match action {
        "list_groups" | "get_group" => Ok(reqwest::Method::GET),
        "create_group" | "join_group" | "leave_group" | "block_group" | "add_members"
        | "add_admins" => Ok(reqwest::Method::POST),
        "update_group" => Ok(reqwest::Method::PUT),
        "delete_group" | "remove_members" | "remove_admins" => Ok(reqwest::Method::DELETE),
        other => anyhow::bail!("unsupported signal groupAction: {other}"),
    }
}

fn signal_group_action_suffix(action: &str) -> anyhow::Result<Vec<String>> {
    let suffix = match action {
        "list_groups" => vec![],
        "get_group" | "update_group" | "delete_group" => vec!["{groupId}".to_string()],
        "create_group" => vec![],
        "join_group" => vec!["{groupId}".to_string(), "join".to_string()],
        "leave_group" => vec!["{groupId}".to_string(), "quit".to_string()],
        "block_group" => vec!["{groupId}".to_string(), "block".to_string()],
        "add_members" | "remove_members" => vec!["{groupId}".to_string(), "members".to_string()],
        "add_admins" | "remove_admins" => vec!["{groupId}".to_string(), "admins".to_string()],
        other => anyhow::bail!("unsupported signal groupAction: {other}"),
    };
    Ok(suffix)
}

fn build_signal_group_action_payload(
    action: &str,
    request: &ChatTargetTextRequest,
) -> anyhow::Result<Option<Value>> {
    let payload = match action {
        "list_groups" | "get_group" | "delete_group" | "join_group" | "leave_group"
        | "block_group" => None,
        "create_group" => {
            let name = request
                .group_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("signal create_group requires groupName"))?;
            let members = request
                .group_members
                .clone()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("signal create_group requires at least one groupMember")
                })?;
            Some(json!({
                "name": name,
                "members": members,
                "description": request.group_description,
                "group_link": request.group_link_mode.clone().unwrap_or_else(|| "disabled".to_string()),
                "permissions": {
                    "add_members": "only-admins",
                    "edit_group": "only-admins",
                }
            }))
        }
        "update_group" => Some(json!({
            "name": request.group_name,
            "description": request.group_description,
        })),
        "add_members" | "remove_members" => {
            let members = request
                .group_members
                .clone()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("signal {action} requires groupMembers"))?;
            Some(json!({ "members": members }))
        }
        "add_admins" | "remove_admins" => {
            let admins = request
                .group_admins
                .clone()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("signal {action} requires groupAdmins"))?;
            Some(json!({ "admins": admins }))
        }
        other => anyhow::bail!("unsupported signal groupAction: {other}"),
    };
    Ok(payload)
}

fn normalize_bluebubbles_reaction(reaction: &str, remove: bool) -> anyhow::Result<String> {
    let normalized = match reaction.trim().to_ascii_lowercase().as_str() {
        "❤️" | "❤" | "love" => "love",
        "👍" | "like" => "like",
        "👎" | "dislike" => "dislike",
        "😂" | "😆" | "haha" | "laugh" => "laugh",
        "‼️" | "!!" | "emphasize" => "emphasize",
        "❓" | "?" | "question" => "question",
        "-love" | "-like" | "-dislike" | "-laugh" | "-emphasize" | "-question" => {
            return Ok(reaction.trim().to_ascii_lowercase());
        }
        other => anyhow::bail!("unsupported BlueBubbles reaction: {other}"),
    };
    if remove {
        Ok(format!("-{normalized}"))
    } else {
        Ok(normalized.to_string())
    }
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
        DecodedAttachment, QQSendRequest, TelegramSendRequest, build_bluebubbles_reaction_payload,
        build_bluebubbles_text_payload, build_chat_completion_messages, build_line_push_payload,
        build_matrix_send_endpoint, build_matrix_text_payload, build_qq_message_payload,
        build_signal_reaction_payload, build_signal_receipt_payload, build_signal_send_payload,
        build_telegram_send_payload, build_wechat_official_account_payload,
        build_whatsapp_text_payload, extract_anthropic_text, extract_chat_completion_text,
        extract_google_text, extract_ollama_text, extract_openai_text,
        normalize_bluebubbles_reaction, normalize_qq_target_type,
        resolve_cloudflare_ai_gateway_endpoint, resolve_openai_style_endpoint,
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
    fn telegram_payload_omits_parse_mode_when_not_requested() {
        let payload = build_telegram_send_payload(&TelegramSendRequest {
            chat_id: "123".to_string(),
            text: "hello".to_string(),
            parse_mode: None,
            disable_notification: Some(false),
        });

        assert_eq!(payload["chat_id"], "123");
        assert!(payload.get("parse_mode").is_none());
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
    fn chat_target_request_defaults_missing_chat_id() {
        let request: super::ChatTargetTextRequest = serde_json::from_value(json!({
            "text": "hello",
            "groupAction": "list_groups"
        }))
        .expect("request should deserialize without chatId");

        assert_eq!(request.chat_id, "");
        assert_eq!(request.text.as_deref(), Some("hello"));
        assert_eq!(request.group_action.as_deref(), Some("list_groups"));
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
    fn builds_matrix_text_payload() {
        let payload = build_matrix_text_payload("hello matrix");

        assert_eq!(
            payload,
            json!({
                "msgtype": "m.text",
                "body": "hello matrix"
            })
        );
    }

    #[test]
    fn builds_signal_send_payload() {
        let payload =
            build_signal_send_payload("+15550001111", "+15550002222", Some("hello signal"), None);

        assert_eq!(
            payload,
            json!({
                "message": "hello signal",
                "number": "+15550001111",
                "recipients": ["+15550002222"]
            })
        );
    }

    #[test]
    fn builds_signal_send_payload_with_attachment() {
        let payload = build_signal_send_payload(
            "+15550001111",
            "+15550002222",
            None,
            Some(&DecodedAttachment {
                name: "proof.txt".to_string(),
                bytes: b"hello".to_vec(),
                content_type: Some("text/plain".to_string()),
            }),
        );

        assert_eq!(
            payload,
            json!({
                "number": "+15550001111",
                "recipients": ["+15550002222"],
                "base64_attachments": ["data:text/plain;filename=proof.txt;base64,aGVsbG8="]
            })
        );
    }

    #[test]
    fn builds_signal_reaction_payload() {
        let payload = build_signal_reaction_payload(
            "+15550002222",
            "1712345678901",
            Some("❤️"),
            Some("+15550003333"),
        );

        assert_eq!(
            payload,
            json!({
                "recipient": "+15550002222",
                "timestamp": 1712345678901i64,
                "target_author": "+15550003333",
                "reaction": "❤️"
            })
        );
    }

    #[test]
    fn builds_signal_receipt_payload() {
        let payload = build_signal_receipt_payload("+15550002222", "1712345678901", "viewed");

        assert_eq!(
            payload,
            json!({
                "recipient": "+15550002222",
                "timestamp": 1712345678901i64,
                "receipt_type": "viewed"
            })
        );
    }

    #[test]
    fn builds_bluebubbles_text_payload() {
        let payload = build_bluebubbles_text_payload(
            "iMessage;+15550002222",
            "hello blue",
            "temp-123",
            None,
            None,
        );

        assert_eq!(
            payload,
            json!({
                "chatGuid": "iMessage;+15550002222",
                "message": "hello blue",
                "tempGuid": "temp-123"
            })
        );
    }

    #[test]
    fn builds_bluebubbles_reaction_payload() {
        let payload = build_bluebubbles_reaction_payload(
            "iMessage;+15550002222",
            "message-guid-1",
            "love",
            Some(2),
        );

        assert_eq!(
            payload,
            json!({
                "chatGuid": "iMessage;+15550002222",
                "selectedMessageGuid": "message-guid-1",
                "reaction": "love",
                "partIndex": 2
            })
        );
    }

    #[test]
    fn normalizes_bluebubbles_reaction_aliases() {
        assert_eq!(
            normalize_bluebubbles_reaction("❤️", false).expect("love reaction"),
            "love"
        );
        assert_eq!(
            normalize_bluebubbles_reaction("love", true).expect("remove love reaction"),
            "-love"
        );
        assert_eq!(
            normalize_bluebubbles_reaction("-question", false)
                .expect("pre-normalized remove reaction"),
            "-question"
        );
    }

    #[test]
    fn builds_matrix_send_endpoint() {
        let endpoint = build_matrix_send_endpoint(
            "https://matrix-client.matrix.org",
            "!roomid:matrix.org",
            "txn-123",
        )
        .expect("matrix endpoint");

        assert_eq!(
            endpoint.as_str(),
            "https://matrix-client.matrix.org/_matrix/client/v3/rooms/!roomid:matrix.org/send/m.room.message/txn-123"
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

    #[test]
    fn resolves_openai_style_endpoint_from_runtime_base() {
        let endpoint = resolve_openai_style_endpoint(
            &[],
            "https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1/chat/completions",
            "/openai/v1/chat/completions",
        );
        assert_eq!(
            endpoint,
            "https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1/chat/completions"
        );

        let computed = {
            unsafe {
                std::env::set_var(
                    "TEST_OPENAI_STYLE_BASE",
                    "https://bedrock-runtime.us-west-2.amazonaws.com",
                );
            }
            let value = resolve_openai_style_endpoint(
                &["TEST_OPENAI_STYLE_BASE"],
                "https://example.com/chat/completions",
                "/openai/v1/chat/completions",
            );
            unsafe {
                std::env::remove_var("TEST_OPENAI_STYLE_BASE");
            }
            value
        };
        assert_eq!(
            computed,
            "https://bedrock-runtime.us-west-2.amazonaws.com/openai/v1/chat/completions"
        );
    }

    #[test]
    fn resolves_cloudflare_ai_gateway_endpoint_from_ids() {
        unsafe {
            std::env::set_var("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID", "cf-account");
            std::env::set_var("CLOUDFLARE_AI_GATEWAY_ID", "gateway-main");
        }
        let endpoint = resolve_cloudflare_ai_gateway_endpoint().expect("cloudflare endpoint");
        unsafe {
            std::env::remove_var("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID");
            std::env::remove_var("CLOUDFLARE_AI_GATEWAY_ID");
        }
        assert_eq!(
            endpoint,
            "https://gateway.ai.cloudflare.com/v1/cf-account/gateway-main/openai/chat/completions"
        );
    }
}
