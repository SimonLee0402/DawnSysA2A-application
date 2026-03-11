use std::sync::Arc;

use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use crate::app_state::{AppState, unix_timestamp_ms};

const DEFAULT_BOOTSTRAP_TOKEN: &str = "dawn-dev-bootstrap";
const DEFAULT_WORKSPACE_ID: &str = "default";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProfileRecord {
    pub workspace_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub display_name: String,
    pub region: String,
    pub default_model_providers: Vec<String>,
    pub default_chat_platforms: Vec<String>,
    pub onboarding_status: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OperatorSessionRecord {
    pub session_id: Uuid,
    pub operator_name: String,
    pub revoked: bool,
    pub created_at_unix_ms: u128,
    pub last_seen_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeClaimStatus {
    Pending,
    Consumed,
    Expired,
    Revoked,
}

impl NodeClaimStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "consumed" => Ok(Self::Consumed),
            "expired" => Ok(Self::Expired),
            "revoked" => Ok(Self::Revoked),
            _ => Err(anyhow!("unknown node claim status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeClaimRecord {
    pub claim_id: Uuid,
    pub node_id: String,
    pub display_name: String,
    pub transport: String,
    pub requested_capabilities: Vec<String>,
    pub issued_by_session_id: Option<Uuid>,
    pub issued_by_operator: String,
    pub status: NodeClaimStatus,
    pub expires_at_unix_ms: u128,
    pub consumed_at_unix_ms: Option<u128>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityStatus {
    bootstrap_mode: String,
    workspace: WorkspaceProfileRecord,
    active_sessions: usize,
    pending_node_claims: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSessionRequest {
    bootstrap_token: String,
    operator_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSessionResponse {
    session: OperatorSessionRecord,
    session_token: String,
    bootstrap_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProfileUpdateRequest {
    session_token: String,
    tenant_id: String,
    project_id: String,
    display_name: String,
    region: String,
    default_model_providers: Vec<String>,
    default_chat_platforms: Vec<String>,
    onboarding_status: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProfileUpdateResponse {
    workspace: WorkspaceProfileRecord,
    actor: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeClaimCreateRequest {
    session_token: String,
    node_id: String,
    display_name: Option<String>,
    transport: Option<String>,
    requested_capabilities: Option<Vec<String>>,
    expires_in_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeClaimCreateResponse {
    claim: NodeClaimRecord,
    claim_token: String,
    session_url: String,
    issued_by: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeNodeClaimRequest {
    session_token: String,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeClaimRevokeResponse {
    claim: NodeClaimRecord,
    reason: String,
}

#[derive(Debug, FromRow)]
struct WorkspaceProfileRow {
    workspace_id: String,
    tenant_id: String,
    project_id: String,
    display_name: String,
    region: String,
    default_model_providers: String,
    default_chat_platforms: String,
    onboarding_status: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, FromRow)]
struct OperatorSessionRow {
    session_id: String,
    operator_name: String,
    revoked: i64,
    created_at_unix_ms: i64,
    last_seen_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, FromRow)]
struct NodeClaimRow {
    claim_id: String,
    node_id: String,
    display_name: String,
    transport: String,
    requested_capabilities: String,
    #[sqlx(rename = "claim_token_hash")]
    _claim_token_hash: String,
    issued_by_session_id: Option<String>,
    issued_by_operator: String,
    status: String,
    expires_at_unix_ms: i64,
    consumed_at_unix_ms: Option<i64>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/bootstrap/session", post(create_bootstrap_session))
        .route("/sessions", get(list_sessions))
        .route("/workspace", get(get_workspace).put(update_workspace))
        .route("/node-claims", get(list_node_claims).post(create_node_claim))
        .route("/node-claims/:claim_id/revoke", post(revoke_node_claim))
}

pub async fn authorize_node_session_open(
    state: &Arc<AppState>,
    node_id: &str,
    claim_token: Option<&str>,
) -> anyhow::Result<Option<NodeClaimRecord>> {
    expire_stale_claims(state).await?;
    if state.get_node(node_id).await?.is_some() {
        return Ok(None);
    }
    let token = claim_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("node onboarding claim token is required for first-time node registration"))?;
    preview_pending_claim(state, node_id, token).await
}

pub async fn consume_node_session_claim(
    state: &Arc<AppState>,
    node_id: &str,
    claim_token: Option<&str>,
) -> anyhow::Result<Option<NodeClaimRecord>> {
    let token = claim_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("node onboarding claim token is required"))?;
    let Some(claim) = preview_pending_claim(state, node_id, token).await? else {
        return Ok(None);
    };
    let consumed = update_node_claim_status(
        state,
        claim.claim_id,
        NodeClaimStatus::Consumed,
        Some(unix_timestamp_ms()),
    )
    .await?;
    state.emit_console_event(
        "node_onboarding",
        Some(claim.node_id.clone()),
        Some("consumed".to_string()),
        format!("node claim '{}' consumed", claim.claim_id),
    );
    Ok(Some(consumed))
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<IdentityStatus>, (StatusCode, Json<Value>)> {
    expire_stale_claims(&state).await.map_err(internal_error)?;
    let workspace = ensure_workspace_profile(&state)
        .await
        .map_err(internal_error)?;
    let sessions = list_operator_session_records(&state)
        .await
        .map_err(internal_error)?;
    let claims = list_node_claim_records_inner(&state)
        .await
        .map_err(internal_error)?;
    Ok(Json(IdentityStatus {
        bootstrap_mode: bootstrap_mode(),
        workspace,
        active_sessions: sessions.iter().filter(|session| !session.revoked).count(),
        pending_node_claims: claims
            .iter()
            .filter(|claim| claim.status == NodeClaimStatus::Pending)
            .count(),
    }))
}

async fn create_bootstrap_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BootstrapSessionRequest>,
) -> Result<Json<BootstrapSessionResponse>, (StatusCode, Json<Value>)> {
    if request.bootstrap_token.trim() != bootstrap_token() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid bootstrap token" })),
        ));
    }
    let operator_name = request.operator_name.trim();
    if operator_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "operatorName cannot be empty" })),
        ));
    }
    let session_token = format!("dawn-session-{}-{}", Uuid::new_v4(), Uuid::new_v4());
    let session = insert_operator_session(&state, operator_name, &session_token)
        .await
        .map_err(internal_error)?;
    state.emit_console_event(
        "identity",
        Some(session.session_id.to_string()),
        Some("session_created".to_string()),
        format!("operator '{}' bootstrapped a session", session.operator_name),
    );
    Ok(Json(BootstrapSessionResponse {
        session,
        session_token,
        bootstrap_mode: bootstrap_mode(),
    }))
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<OperatorSessionRecord>>, (StatusCode, Json<Value>)> {
    list_operator_session_records(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_workspace(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WorkspaceProfileRecord>, (StatusCode, Json<Value>)> {
    ensure_workspace_profile(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn update_workspace(
    State(state): State<Arc<AppState>>,
    Json(request): Json<WorkspaceProfileUpdateRequest>,
) -> Result<Json<WorkspaceProfileUpdateResponse>, (StatusCode, Json<Value>)> {
    let session = resolve_session_by_token(&state, &request.session_token)
        .await
        .map_err(auth_error)?;
    let previous = ensure_workspace_profile(&state)
        .await
        .map_err(internal_error)?;
    let workspace = WorkspaceProfileRecord {
        workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        tenant_id: request.tenant_id.trim().to_string(),
        project_id: request.project_id.trim().to_string(),
        display_name: request.display_name.trim().to_string(),
        region: request.region.trim().to_string(),
        default_model_providers: normalize_metadata(request.default_model_providers),
        default_chat_platforms: normalize_metadata(request.default_chat_platforms),
        onboarding_status: request
            .onboarding_status
            .unwrap_or_else(|| "configured".to_string()),
        created_at_unix_ms: previous.created_at_unix_ms,
        updated_at_unix_ms: unix_timestamp_ms(),
    };
    if workspace.tenant_id.is_empty()
        || workspace.project_id.is_empty()
        || workspace.display_name.is_empty()
        || workspace.region.is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "tenantId, projectId, displayName, and region are required" })),
        ));
    }
    let workspace = save_workspace_profile(&state, &workspace)
        .await
        .map_err(internal_error)?;
    state.emit_console_event(
        "identity",
        Some(workspace.workspace_id.clone()),
        Some("workspace_updated".to_string()),
        format!("workspace updated by {}", session.operator_name),
    );
    Ok(Json(WorkspaceProfileUpdateResponse {
        workspace,
        actor: session.operator_name,
    }))
}

async fn list_node_claims(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<NodeClaimRecord>>, (StatusCode, Json<Value>)> {
    expire_stale_claims(&state).await.map_err(internal_error)?;
    list_node_claim_records_inner(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn create_node_claim(
    State(state): State<Arc<AppState>>,
    Json(request): Json<NodeClaimCreateRequest>,
) -> Result<Json<NodeClaimCreateResponse>, (StatusCode, Json<Value>)> {
    let session = resolve_session_by_token(&state, &request.session_token)
        .await
        .map_err(auth_error)?;
    expire_stale_claims(&state).await.map_err(internal_error)?;
    let node_id = request.node_id.trim();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "nodeId cannot be empty" })),
        ));
    }
    let token = format!("dawn-claim-{}-{}", Uuid::new_v4(), Uuid::new_v4());
    let now = unix_timestamp_ms();
    let claim = NodeClaimRecord {
        claim_id: Uuid::new_v4(),
        node_id: node_id.to_string(),
        display_name: request
            .display_name
            .unwrap_or_else(|| format!("Dawn Node {node_id}"))
            .trim()
            .to_string(),
        transport: request
            .transport
            .unwrap_or_else(|| "websocket".to_string())
            .trim()
            .to_string(),
        requested_capabilities: normalize_metadata(request.requested_capabilities.unwrap_or_default()),
        issued_by_session_id: Some(session.session_id),
        issued_by_operator: session.operator_name.clone(),
        status: NodeClaimStatus::Pending,
        expires_at_unix_ms: now
            + u128::from(request.expires_in_seconds.unwrap_or(1800).max(60)).saturating_mul(1000),
        consumed_at_unix_ms: None,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    save_node_claim(&state, &claim, &token)
        .await
        .map_err(internal_error)?;
    state.emit_console_event(
        "node_onboarding",
        Some(claim.node_id.clone()),
        Some("claim_issued".to_string()),
        format!("claim issued by {}", claim.issued_by_operator),
    );
    Ok(Json(NodeClaimCreateResponse {
        session_url: build_node_session_url(&claim),
        claim,
        claim_token: token,
        issued_by: session.operator_name,
    }))
}

async fn revoke_node_claim(
    State(state): State<Arc<AppState>>,
    Path(claim_id): Path<Uuid>,
    Json(request): Json<RevokeNodeClaimRequest>,
) -> Result<Json<NodeClaimRevokeResponse>, (StatusCode, Json<Value>)> {
    let session = resolve_session_by_token(&state, &request.session_token)
        .await
        .map_err(auth_error)?;
    let mut claim = get_node_claim(&state, claim_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "node claim not found" })),
            )
        })?;
    claim.status = NodeClaimStatus::Revoked;
    claim.updated_at_unix_ms = unix_timestamp_ms();
    save_node_claim_record(&state, &claim)
        .await
        .map_err(internal_error)?;
    let reason = request
        .reason
        .unwrap_or_else(|| format!("revoked by {}", session.operator_name));
    state.emit_console_event(
        "node_onboarding",
        Some(claim.node_id.clone()),
        Some("claim_revoked".to_string()),
        reason.clone(),
    );
    Ok(Json(NodeClaimRevokeResponse { claim, reason }))
}

fn bootstrap_token() -> String {
    std::env::var("DAWN_OPERATOR_BOOTSTRAP_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BOOTSTRAP_TOKEN.to_string())
}

fn bootstrap_mode() -> String {
    if std::env::var("DAWN_OPERATOR_BOOTSTRAP_TOKEN").is_ok() {
        "env_override".to_string()
    } else {
        "development_default".to_string()
    }
}

fn public_ws_base_url() -> String {
    if let Ok(value) = std::env::var("DAWN_PUBLIC_WS_BASE_URL") {
        if !value.trim().is_empty() {
            return value.trim_end_matches('/').to_string();
        }
    }
    if let Ok(value) = std::env::var("DAWN_PUBLIC_BASE_URL") {
        if !value.trim().is_empty() {
            let trimmed = value.trim_end_matches('/');
            if let Some(rest) = trimmed.strip_prefix("http://") {
                return format!("ws://{rest}");
            }
            if let Some(rest) = trimmed.strip_prefix("https://") {
                return format!("wss://{rest}");
            }
        }
    }
    "ws://127.0.0.1:8000".to_string()
}

fn url_encode(input: &str) -> String {
    input.replace(' ', "%20")
}

fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn normalize_metadata(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn build_node_session_url(claim: &NodeClaimRecord) -> String {
    format!(
        "{}/api/gateway/control-plane/nodes/{}/session?displayName={}&transport={}&claimToken={{claimToken}}",
        public_ws_base_url(),
        claim.node_id,
        url_encode(&claim.display_name),
        url_encode(&claim.transport)
    )
}

async fn ensure_workspace_profile(state: &Arc<AppState>) -> anyhow::Result<WorkspaceProfileRecord> {
    if let Some(record) = get_workspace_profile(state).await? {
        return Ok(record);
    }
    let now = unix_timestamp_ms();
    let profile = WorkspaceProfileRecord {
        workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        tenant_id: "dawn-labs".to_string(),
        project_id: "agent-commerce".to_string(),
        display_name: "Dawn Agent Commerce".to_string(),
        region: "global".to_string(),
        default_model_providers: vec!["deepseek".to_string(), "qwen".to_string()],
        default_chat_platforms: vec!["feishu".to_string(), "wechat_official_account".to_string()],
        onboarding_status: "bootstrap_pending".to_string(),
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    save_workspace_profile(state, &profile).await
}

async fn get_workspace_profile(state: &Arc<AppState>) -> anyhow::Result<Option<WorkspaceProfileRecord>> {
    let row = sqlx::query_as::<_, WorkspaceProfileRow>(
        r#"
        SELECT
            workspace_id,
            tenant_id,
            project_id,
            display_name,
            region,
            default_model_providers,
            default_chat_platforms,
            onboarding_status,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM workspace_profiles
        WHERE workspace_id = ?1
        "#,
    )
    .bind(DEFAULT_WORKSPACE_ID)
    .fetch_optional(state.pool())
    .await
    .context("failed to fetch workspace profile")?;

    row.map(WorkspaceProfileRecord::try_from).transpose()
}

async fn save_workspace_profile(
    state: &Arc<AppState>,
    profile: &WorkspaceProfileRecord,
) -> anyhow::Result<WorkspaceProfileRecord> {
    sqlx::query(
        r#"
        INSERT INTO workspace_profiles (
            workspace_id,
            tenant_id,
            project_id,
            display_name,
            region,
            default_model_providers,
            default_chat_platforms,
            onboarding_status,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(workspace_id) DO UPDATE SET
            tenant_id = excluded.tenant_id,
            project_id = excluded.project_id,
            display_name = excluded.display_name,
            region = excluded.region,
            default_model_providers = excluded.default_model_providers,
            default_chat_platforms = excluded.default_chat_platforms,
            onboarding_status = excluded.onboarding_status,
            created_at_unix_ms = workspace_profiles.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&profile.workspace_id)
    .bind(&profile.tenant_id)
    .bind(&profile.project_id)
    .bind(&profile.display_name)
    .bind(&profile.region)
    .bind(serde_json::to_string(&profile.default_model_providers)?)
    .bind(serde_json::to_string(&profile.default_chat_platforms)?)
    .bind(&profile.onboarding_status)
    .bind(profile.created_at_unix_ms as i64)
    .bind(profile.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .context("failed to save workspace profile")?;

    get_workspace_profile(state)
        .await?
        .ok_or_else(|| anyhow!("workspace profile disappeared after save"))
}

async fn insert_operator_session(
    state: &Arc<AppState>,
    operator_name: &str,
    session_token: &str,
) -> anyhow::Result<OperatorSessionRecord> {
    let session_id = Uuid::new_v4();
    let now = unix_timestamp_ms();
    sqlx::query(
        r#"
        INSERT INTO operator_sessions (
            session_id,
            operator_name,
            session_token_hash,
            revoked,
            created_at_unix_ms,
            last_seen_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
    )
    .bind(session_id.to_string())
    .bind(operator_name)
    .bind(hash_token(session_token))
    .bind(0_i64)
    .bind(now as i64)
    .bind(now as i64)
    .bind(now as i64)
    .execute(state.pool())
    .await
    .context("failed to insert operator session")?;

    get_operator_session(state, session_id)
        .await?
        .ok_or_else(|| anyhow!("operator session disappeared after insert"))
}

async fn resolve_session_by_token(
    state: &Arc<AppState>,
    session_token: &str,
) -> anyhow::Result<OperatorSessionRecord> {
    let token_hash = hash_token(session_token.trim());
    let row = sqlx::query_as::<_, OperatorSessionRow>(
        r#"
        SELECT
            session_id,
            operator_name,
            revoked,
            created_at_unix_ms,
            last_seen_at_unix_ms,
            updated_at_unix_ms
        FROM operator_sessions
        WHERE session_token_hash = ?1
        "#,
    )
    .bind(token_hash)
    .fetch_optional(state.pool())
    .await
    .context("failed to resolve operator session by token")?;

    let Some(mut session) = row.map(OperatorSessionRecord::try_from).transpose()? else {
        anyhow::bail!("invalid or unknown session token");
    };
    if session.revoked {
        anyhow::bail!("session token has been revoked");
    }
    session.last_seen_at_unix_ms = unix_timestamp_ms();
    session.updated_at_unix_ms = session.last_seen_at_unix_ms;
    sqlx::query(
        r#"
        UPDATE operator_sessions
        SET last_seen_at_unix_ms = ?2, updated_at_unix_ms = ?3
        WHERE session_id = ?1
        "#,
    )
    .bind(session.session_id.to_string())
    .bind(session.last_seen_at_unix_ms as i64)
    .bind(session.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .context("failed to update operator session last_seen")?;
    Ok(session)
}

async fn list_operator_session_records(state: &Arc<AppState>) -> anyhow::Result<Vec<OperatorSessionRecord>> {
    let rows = sqlx::query_as::<_, OperatorSessionRow>(
        r#"
        SELECT
            session_id,
            operator_name,
            revoked,
            created_at_unix_ms,
            last_seen_at_unix_ms,
            updated_at_unix_ms
        FROM operator_sessions
        ORDER BY created_at_unix_ms DESC
        "#,
    )
    .fetch_all(state.pool())
    .await
    .context("failed to list operator sessions")?;
    rows.into_iter().map(OperatorSessionRecord::try_from).collect()
}

async fn get_operator_session(
    state: &Arc<AppState>,
    session_id: Uuid,
) -> anyhow::Result<Option<OperatorSessionRecord>> {
    let row = sqlx::query_as::<_, OperatorSessionRow>(
        r#"
        SELECT
            session_id,
            operator_name,
            revoked,
            created_at_unix_ms,
            last_seen_at_unix_ms,
            updated_at_unix_ms
        FROM operator_sessions
        WHERE session_id = ?1
        "#,
    )
    .bind(session_id.to_string())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch operator session {session_id}"))?;
    row.map(OperatorSessionRecord::try_from).transpose()
}

async fn save_node_claim(
    state: &Arc<AppState>,
    claim: &NodeClaimRecord,
    raw_token: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_claims (
            claim_id,
            node_id,
            display_name,
            transport,
            requested_capabilities,
            claim_token_hash,
            issued_by_session_id,
            issued_by_operator,
            status,
            expires_at_unix_ms,
            consumed_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
    )
    .bind(claim.claim_id.to_string())
    .bind(&claim.node_id)
    .bind(&claim.display_name)
    .bind(&claim.transport)
    .bind(serde_json::to_string(&claim.requested_capabilities)?)
    .bind(hash_token(raw_token))
    .bind(claim.issued_by_session_id.map(|value| value.to_string()))
    .bind(&claim.issued_by_operator)
    .bind(claim.status.as_db())
    .bind(claim.expires_at_unix_ms as i64)
    .bind(claim.consumed_at_unix_ms.map(|value| value as i64))
    .bind(claim.created_at_unix_ms as i64)
    .bind(claim.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .context("failed to save node claim")?;
    Ok(())
}

async fn save_node_claim_record(state: &Arc<AppState>, claim: &NodeClaimRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE node_claims
        SET
            display_name = ?2,
            transport = ?3,
            requested_capabilities = ?4,
            issued_by_session_id = ?5,
            issued_by_operator = ?6,
            status = ?7,
            expires_at_unix_ms = ?8,
            consumed_at_unix_ms = ?9,
            updated_at_unix_ms = ?10
        WHERE claim_id = ?1
        "#,
    )
    .bind(claim.claim_id.to_string())
    .bind(&claim.display_name)
    .bind(&claim.transport)
    .bind(serde_json::to_string(&claim.requested_capabilities)?)
    .bind(claim.issued_by_session_id.map(|value| value.to_string()))
    .bind(&claim.issued_by_operator)
    .bind(claim.status.as_db())
    .bind(claim.expires_at_unix_ms as i64)
    .bind(claim.consumed_at_unix_ms.map(|value| value as i64))
    .bind(claim.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .context("failed to update node claim")?;
    Ok(())
}

async fn list_node_claim_records_inner(state: &Arc<AppState>) -> anyhow::Result<Vec<NodeClaimRecord>> {
    let rows = sqlx::query_as::<_, NodeClaimRow>(
        r#"
        SELECT
            claim_id,
            node_id,
            display_name,
            transport,
            requested_capabilities,
            claim_token_hash,
            issued_by_session_id,
            issued_by_operator,
            status,
            expires_at_unix_ms,
            consumed_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM node_claims
        ORDER BY created_at_unix_ms DESC
        "#,
    )
    .fetch_all(state.pool())
    .await
    .context("failed to list node claims")?;
    rows.into_iter().map(NodeClaimRecord::try_from).collect()
}

async fn get_node_claim(
    state: &Arc<AppState>,
    claim_id: Uuid,
) -> anyhow::Result<Option<NodeClaimRecord>> {
    let row = sqlx::query_as::<_, NodeClaimRow>(
        r#"
        SELECT
            claim_id,
            node_id,
            display_name,
            transport,
            requested_capabilities,
            claim_token_hash,
            issued_by_session_id,
            issued_by_operator,
            status,
            expires_at_unix_ms,
            consumed_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM node_claims
        WHERE claim_id = ?1
        "#,
    )
    .bind(claim_id.to_string())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch node claim {claim_id}"))?;
    row.map(NodeClaimRecord::try_from).transpose()
}

async fn preview_pending_claim(
    state: &Arc<AppState>,
    node_id: &str,
    raw_token: &str,
) -> anyhow::Result<Option<NodeClaimRecord>> {
    let row = sqlx::query_as::<_, NodeClaimRow>(
        r#"
        SELECT
            claim_id,
            node_id,
            display_name,
            transport,
            requested_capabilities,
            claim_token_hash,
            issued_by_session_id,
            issued_by_operator,
            status,
            expires_at_unix_ms,
            consumed_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM node_claims
        WHERE node_id = ?1
          AND claim_token_hash = ?2
          AND status = ?3
        ORDER BY created_at_unix_ms DESC
        LIMIT 1
        "#,
    )
    .bind(node_id)
    .bind(hash_token(raw_token))
    .bind(NodeClaimStatus::Pending.as_db())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to preview node claim for {node_id}"))?;

    let Some(record) = row.map(NodeClaimRecord::try_from).transpose()? else {
        anyhow::bail!("invalid node claim token for '{}'", node_id);
    };
    if record.expires_at_unix_ms <= unix_timestamp_ms() {
        anyhow::bail!("node claim for '{}' has expired", node_id);
    }
    Ok(Some(record))
}

async fn update_node_claim_status(
    state: &Arc<AppState>,
    claim_id: Uuid,
    status: NodeClaimStatus,
    consumed_at_unix_ms: Option<u128>,
) -> anyhow::Result<NodeClaimRecord> {
    let mut claim = get_node_claim(state, claim_id)
        .await?
        .ok_or_else(|| anyhow!("node claim '{}' was not found", claim_id))?;
    claim.status = status;
    claim.consumed_at_unix_ms = consumed_at_unix_ms.or(claim.consumed_at_unix_ms);
    claim.updated_at_unix_ms = unix_timestamp_ms();
    save_node_claim_record(state, &claim).await?;
    Ok(claim)
}

async fn expire_stale_claims(state: &Arc<AppState>) -> anyhow::Result<()> {
    let now = unix_timestamp_ms() as i64;
    sqlx::query(
        r#"
        UPDATE node_claims
        SET status = ?1, updated_at_unix_ms = ?2
        WHERE status = ?3 AND expires_at_unix_ms <= ?4
        "#,
    )
    .bind(NodeClaimStatus::Expired.as_db())
    .bind(now)
    .bind(NodeClaimStatus::Pending.as_db())
    .bind(now)
    .execute(state.pool())
    .await
    .context("failed to expire stale node claims")?;
    Ok(())
}

fn auth_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

impl TryFrom<WorkspaceProfileRow> for WorkspaceProfileRecord {
    type Error = anyhow::Error;

    fn try_from(row: WorkspaceProfileRow) -> Result<Self, Self::Error> {
        Ok(Self {
            workspace_id: row.workspace_id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            display_name: row.display_name,
            region: row.region,
            default_model_providers: serde_json::from_str(&row.default_model_providers)
                .context("failed to parse workspace default model providers")?,
            default_chat_platforms: serde_json::from_str(&row.default_chat_platforms)
                .context("failed to parse workspace default chat platforms")?,
            onboarding_status: row.onboarding_status,
            created_at_unix_ms: row.created_at_unix_ms as u128,
            updated_at_unix_ms: row.updated_at_unix_ms as u128,
        })
    }
}

impl TryFrom<OperatorSessionRow> for OperatorSessionRecord {
    type Error = anyhow::Error;

    fn try_from(row: OperatorSessionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            session_id: Uuid::parse_str(&row.session_id)
                .with_context(|| format!("invalid operator session id '{}'", row.session_id))?,
            operator_name: row.operator_name,
            revoked: row.revoked != 0,
            created_at_unix_ms: row.created_at_unix_ms as u128,
            last_seen_at_unix_ms: row.last_seen_at_unix_ms as u128,
            updated_at_unix_ms: row.updated_at_unix_ms as u128,
        })
    }
}

impl TryFrom<NodeClaimRow> for NodeClaimRecord {
    type Error = anyhow::Error;

    fn try_from(row: NodeClaimRow) -> Result<Self, Self::Error> {
        Ok(Self {
            claim_id: Uuid::parse_str(&row.claim_id)
                .with_context(|| format!("invalid node claim id '{}'", row.claim_id))?,
            node_id: row.node_id,
            display_name: row.display_name,
            transport: row.transport,
            requested_capabilities: serde_json::from_str(&row.requested_capabilities)
                .context("failed to parse node claim capabilities")?,
            issued_by_session_id: row
                .issued_by_session_id
                .map(|value| {
                    Uuid::parse_str(&value)
                        .with_context(|| format!("invalid claim session id '{value}'"))
                })
                .transpose()?,
            issued_by_operator: row.issued_by_operator,
            status: NodeClaimStatus::from_db(&row.status)?,
            expires_at_unix_ms: row.expires_at_unix_ms as u128,
            consumed_at_unix_ms: row.consumed_at_unix_ms.map(|value| value as u128),
            created_at_unix_ms: row.created_at_unix_ms as u128,
            updated_at_unix_ms: row.updated_at_unix_ms as u128,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::util::ServiceExt;
    use uuid::Uuid;
    use wasmtime::Engine;

    use super::{
        AppState, BootstrapSessionRequest, NodeClaimCreateRequest, WorkspaceProfileUpdateRequest,
        authorize_node_session_open, consume_node_session_claim, ensure_workspace_profile, router,
    };
    use crate::sandbox;

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-identity-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    async fn test_app() -> anyhow::Result<(Router, PathBuf)> {
        let (state, path) = test_state().await?;
        let app = Router::new().nest("/identity", router()).with_state(state);
        Ok((app, path))
    }

    async fn json_request(
        app: Router,
        method: &str,
        uri: &str,
        body: Value,
    ) -> anyhow::Result<axum::http::Response<Body>> {
        Ok(app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))?,
            )
            .await?)
    }

    async fn response_json(response: axum::http::Response<Body>) -> anyhow::Result<Value> {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    #[tokio::test]
    async fn workspace_profile_bootstraps_defaults() -> anyhow::Result<()> {
        let (state, db_path) = test_state().await?;
        let profile = ensure_workspace_profile(&state).await?;
        assert_eq!(profile.workspace_id, "default");
        assert_eq!(profile.onboarding_status, "bootstrap_pending");
        drop(state);
        let _ = fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn bootstrap_session_issue_and_claim_lifecycle_work() -> anyhow::Result<()> {
        let (app, db_path) = test_app().await?;
        let response = json_request(
            app.clone(),
            "POST",
            "/identity/bootstrap/session",
            serde_json::to_value(BootstrapSessionRequest {
                bootstrap_token: "dawn-dev-bootstrap".to_string(),
                operator_name: "alice".to_string(),
            })?,
        )
        .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await?;
        let session_token = payload["sessionToken"].as_str().unwrap().to_string();

        let update_response = json_request(
            app.clone(),
            "PUT",
            "/identity/workspace",
            serde_json::to_value(WorkspaceProfileUpdateRequest {
                session_token: session_token.clone(),
                tenant_id: "tenant-cn".to_string(),
                project_id: "ap2-network".to_string(),
                display_name: "Dawn China".to_string(),
                region: "china".to_string(),
                default_model_providers: vec!["qwen".to_string(), "deepseek".to_string()],
                default_chat_platforms: vec!["wechat_official_account".to_string()],
                onboarding_status: Some("identity_ready".to_string()),
            })?,
        )
        .await?;
        assert_eq!(update_response.status(), StatusCode::OK);

        let claim_response = json_request(
            app.clone(),
            "POST",
            "/identity/node-claims",
            serde_json::to_value(NodeClaimCreateRequest {
                session_token,
                node_id: "node-cn-01".to_string(),
                display_name: Some("Shanghai Node".to_string()),
                transport: Some("websocket".to_string()),
                requested_capabilities: Some(vec![
                    "system_info".to_string(),
                    "process_snapshot".to_string(),
                ]),
                expires_in_seconds: Some(600),
            })?,
        )
        .await?;
        assert_eq!(claim_response.status(), StatusCode::OK);
        let claim_payload = response_json(claim_response).await?;
        assert_eq!(claim_payload["claim"]["status"], "pending");
        assert!(claim_payload["sessionUrl"]
            .as_str()
            .unwrap()
            .contains("claimToken={claimToken}"));

        drop(app);
        let _ = fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn first_time_node_requires_and_consumes_claim() -> anyhow::Result<()> {
        let (state, db_path) = test_state().await?;
        let session = super::insert_operator_session(&state, "alice", "session-token").await?;
        let claim = super::NodeClaimRecord {
            claim_id: Uuid::new_v4(),
            node_id: "node-first".to_string(),
            display_name: "First Node".to_string(),
            transport: "websocket".to_string(),
            requested_capabilities: vec!["agent_ping".to_string()],
            issued_by_session_id: Some(session.session_id),
            issued_by_operator: "alice".to_string(),
            status: super::NodeClaimStatus::Pending,
            expires_at_unix_ms: crate::app_state::unix_timestamp_ms() + 60_000,
            consumed_at_unix_ms: None,
            created_at_unix_ms: crate::app_state::unix_timestamp_ms(),
            updated_at_unix_ms: crate::app_state::unix_timestamp_ms(),
        };
        super::save_node_claim(&state, &claim, "claim-token").await?;

        let preview = authorize_node_session_open(&state, "node-first", Some("claim-token")).await?;
        assert!(preview.is_some());

        let consumed = consume_node_session_claim(&state, "node-first", Some("claim-token")).await?;
        assert_eq!(
            consumed.as_ref().map(|record| record.status),
            Some(super::NodeClaimStatus::Consumed)
        );

        let second_attempt = authorize_node_session_open(&state, "node-first", Some("claim-token")).await;
        assert!(second_attempt.is_err());

        drop(state);
        let _ = fs::remove_file(db_path);
        Ok(())
    }
}
