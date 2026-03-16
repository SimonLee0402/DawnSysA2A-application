use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::Serialize;
use serde_json::{Value, json};
use tracing::error;

use crate::{
    agent_cards, app_state::AppState, approval_center, chat_ingress, connectors, control_plane,
    end_user_approvals, identity, marketplace, policy, skill_registry,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayStatus {
    mode: &'static str,
    networked_control_plane: bool,
    chat_ingress: bool,
    model_connectors: bool,
    node_execution: bool,
    payment_authorization: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayCapabilities {
    supported_chat_platforms: Vec<&'static str>,
    supported_model_providers: Vec<&'static str>,
    core_protocols: Vec<&'static str>,
    differentiators: Vec<&'static str>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/capabilities", get(capabilities))
        .route("/policy", get(get_policy).put(update_policy))
        .route("/policy/distribution", get(get_policy_distribution))
        .route("/policy/signed", axum::routing::put(activate_signed_policy))
        .route("/policy/audit", get(list_policy_audit))
        .route(
            "/policy/trust-roots",
            get(list_policy_trust_roots).post(upsert_policy_trust_root),
        )
        .nest("/approvals", approval_center::router())
        .nest("/control-plane", control_plane::router())
        .nest("/connectors", connectors::router())
        .nest("/end-user", end_user_approvals::api_router())
        .nest("/identity", identity::router())
        .nest("/ingress", chat_ingress::router())
        .nest("/marketplace", marketplace::router())
        .nest("/agent-cards", agent_cards::router())
        .nest("/skills", skill_registry::router())
}

async fn status() -> Json<GatewayStatus> {
    Json(GatewayStatus {
        mode: "networked_gateway",
        networked_control_plane: true,
        chat_ingress: true,
        model_connectors: true,
        node_execution: true,
        payment_authorization: true,
    })
}

async fn capabilities() -> Json<GatewayCapabilities> {
    Json(GatewayCapabilities {
        supported_chat_platforms: vec![
            "telegram",
            "discord",
            "slack",
            "mattermost",
            "msteams",
            "google_chat",
            "feishu",
            "dingtalk",
            "wecom_bot",
            "wechat_official_account",
            "qq",
        ],
        supported_model_providers: vec![
            "openai",
            "anthropic",
            "google",
            "openrouter",
            "groq",
            "together",
            "vllm",
            "deepseek",
            "qwen",
            "zhipu",
            "moonshot",
            "doubao",
            "ollama",
        ],
        core_protocols: vec!["a2a", "ap2", "wasm_skill_runtime"],
        differentiators: vec![
            "protocol-native agent-to-agent orchestration",
            "agent card registry, publishing, and discovery",
            "hardware-mediated payment authorization",
            "wasm sandbox for third-party skills",
            "signed wasm skill distribution with trusted publishers",
            "event-auditable task, node, and payment transitions",
            "connector-ready gateway for model and chat integrations",
            "china-market model and chat connector expansion path",
        ],
    })
}

async fn get_policy(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::app_state::PolicyProfileRecord>, (StatusCode, Json<Value>)> {
    policy::current_profile(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn update_policy(
    State(state): State<Arc<AppState>>,
    Json(request): Json<policy::PolicyUpdateRequest>,
) -> Result<Json<policy::PolicyUpdateResponse>, (StatusCode, Json<Value>)> {
    policy::update_profile(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_policy_distribution(
    State(state): State<Arc<AppState>>,
) -> Result<Json<policy::PolicyDistributionResponse>, (StatusCode, Json<Value>)> {
    policy::current_distribution(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn activate_signed_policy(
    State(state): State<Arc<AppState>>,
    Json(request): Json<policy::SignedPolicyActivationRequest>,
) -> Result<Json<policy::SignedPolicyActivationResponse>, (StatusCode, Json<Value>)> {
    policy::activate_signed_profile(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn list_policy_audit(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::app_state::PolicyAuditEventRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_policy_audit_events()
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn list_policy_trust_roots(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::app_state::PolicyTrustRootRecord>>, (StatusCode, Json<Value>)> {
    policy::list_trust_roots(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn upsert_policy_trust_root(
    State(state): State<Arc<AppState>>,
    Json(request): Json<policy::PolicyTrustRootUpsertRequest>,
) -> Result<Json<policy::PolicyTrustRootUpsertResponse>, (StatusCode, Json<Value>)> {
    policy::upsert_trust_root(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "Gateway policy failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}
