use std::{process::Command as StdCommand, sync::Arc};

use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use crate::app_state::{
    AppState, ApprovalRequestKind, ApprovalRequestStatus, NodeRecord, unix_timestamp_ms,
};

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
    readiness: IdentityReadinessSummary,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct IdentityReadinessSummary {
    overall_status: String,
    completion_percent: u8,
    next_step: Option<String>,
    ready_steps: usize,
    total_steps: usize,
    metrics: IdentityReadinessMetrics,
    checklist: Vec<IdentityReadinessItem>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct IdentityReadinessMetrics {
    active_sessions: usize,
    total_nodes: usize,
    connected_nodes: usize,
    trusted_nodes: usize,
    issued_node_claims: usize,
    pending_node_claims: usize,
    default_model_providers_ready: usize,
    total_default_model_providers: usize,
    default_chat_platforms_ready: usize,
    total_default_chat_platforms: usize,
    ingress_platforms_ready: usize,
    total_ingress_platforms: usize,
    pending_payment_approvals: usize,
    pending_end_user_sessions: usize,
    public_base_url_configured: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct IdentityReadinessItem {
    key: String,
    label: String,
    status: String,
    detail: String,
    action: Option<String>,
    surface: Option<String>,
    target: Option<String>,
}

#[derive(Debug, Clone)]
struct IdentityEnvironmentReadiness {
    public_base_url: Option<String>,
    configured_model_providers: Vec<String>,
    configured_chat_platforms: Vec<String>,
    configured_ingress_platforms: Vec<String>,
    present_env_keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SetupVerificationReceiptRecord {
    receipt_id: Uuid,
    surface: String,
    target: String,
    label: String,
    region: String,
    integration_mode: String,
    status: String,
    summary: String,
    detail: String,
    action: Option<String>,
    endpoint: String,
    env_keys: Vec<String>,
    missing_env_keys: Vec<String>,
    is_default_path: bool,
    verified_by: String,
    created_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSetupVerificationReceiptRequest {
    session_token: String,
    surface: String,
    target: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupVerificationReceiptResponse {
    receipt: SetupVerificationReceiptRecord,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListSetupVerificationReceiptsQuery {
    surface: Option<String>,
    target: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Clone)]
struct SetupTargetProfile {
    surface: &'static str,
    target: &'static str,
    label: &'static str,
    region: &'static str,
    integration_mode: &'static str,
    endpoint: &'static str,
    note: &'static str,
    env_hints: Vec<&'static str>,
    env_requirement_groups: Vec<Vec<&'static str>>,
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
    pub(crate) session_token: String,
    pub(crate) tenant_id: String,
    pub(crate) project_id: String,
    pub(crate) display_name: String,
    pub(crate) region: String,
    pub(crate) default_model_providers: Vec<String>,
    pub(crate) default_chat_platforms: Vec<String>,
    pub(crate) onboarding_status: Option<String>,
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
    launch_url: String,
    token_hint: String,
    reissued_from_claim_id: Option<Uuid>,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReissueNodeClaimRequest {
    session_token: String,
    expires_in_seconds: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct NodeClaimAuditEventRecord {
    event_id: i64,
    claim_id: Uuid,
    node_id: String,
    event_type: String,
    actor: String,
    detail: String,
    token_hint: Option<String>,
    session_url: Option<String>,
    created_at_unix_ms: u128,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListNodeClaimAuditEventsQuery {
    claim_id: Option<String>,
    node_id: Option<String>,
    limit: Option<u32>,
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

#[derive(Debug, FromRow)]
struct SetupVerificationReceiptRow {
    receipt_id: String,
    surface: String,
    target: String,
    label: String,
    region: String,
    integration_mode: String,
    status: String,
    summary: String,
    detail: String,
    action: Option<String>,
    endpoint: String,
    env_keys: String,
    missing_env_keys: String,
    is_default_path: i64,
    verified_by: String,
    created_at_unix_ms: i64,
}

#[derive(Debug, FromRow)]
struct NodeClaimAuditEventRow {
    event_id: i64,
    claim_id: String,
    node_id: String,
    event_type: String,
    actor: String,
    detail: String,
    token_hint: Option<String>,
    session_url: Option<String>,
    created_at_unix_ms: i64,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/bootstrap/session", post(create_bootstrap_session))
        .route("/sessions", get(list_sessions))
        .route("/workspace", get(get_workspace).put(update_workspace))
        .route(
            "/setup-verifications",
            get(list_setup_verification_receipts).post(create_setup_verification_receipt),
        )
        .route("/node-claim-events", get(list_node_claim_audit_events))
        .route(
            "/node-claims",
            get(list_node_claims).post(create_node_claim),
        )
        .route("/node-claims/:claim_id/revoke", post(revoke_node_claim))
        .route("/node-claims/:claim_id/reissue", post(reissue_node_claim))
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
        .ok_or_else(|| {
            anyhow!("node onboarding claim token is required for first-time node registration")
        })?;
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
    record_node_claim_audit_event(
        state,
        &NodeClaimAuditEventRecord {
            event_id: 0,
            claim_id: consumed.claim_id,
            node_id: consumed.node_id.clone(),
            event_type: "consumed".to_string(),
            actor: consumed.node_id.clone(),
            detail: "first-time node session consumed onboarding claim".to_string(),
            token_hint: None,
            session_url: Some(build_node_session_url(&consumed)),
            created_at_unix_ms: unix_timestamp_ms(),
        },
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
    let (workspace, sessions, claims, nodes, approvals, pending_end_user_sessions) =
        tokio::try_join!(
            ensure_workspace_profile(&state),
            list_operator_session_records(&state),
            list_node_claim_records_inner(&state),
            state.list_nodes(),
            state.list_approval_requests(Some(ApprovalRequestStatus::Pending)),
            count_pending_end_user_sessions(&state),
        )
        .map_err(internal_error)?;
    let active_sessions = sessions.iter().filter(|session| !session.revoked).count();
    let pending_node_claims = claims
        .iter()
        .filter(|claim| claim.status == NodeClaimStatus::Pending)
        .count();
    let pending_payment_approvals = approvals
        .iter()
        .filter(|approval| approval.kind == ApprovalRequestKind::Payment)
        .count();
    let readiness = build_identity_readiness(
        &workspace,
        active_sessions,
        &claims,
        &nodes,
        pending_payment_approvals,
        pending_end_user_sessions,
        &capture_identity_environment_readiness(),
    );
    Ok(Json(IdentityStatus {
        bootstrap_mode: bootstrap_mode(),
        workspace,
        active_sessions,
        pending_node_claims,
        readiness,
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
        format!(
            "operator '{}' bootstrapped a session",
            session.operator_name
        ),
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
    apply_workspace_update(&state, request)
        .await
        .map(Json)
        .map_err(|error| {
            if error
                .to_string()
                .contains("tenantId, projectId, displayName, and region are required")
            {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": error.to_string() })),
                )
            } else if error
                .to_string()
                .contains("invalid or unknown session token")
                || error.to_string().contains("session token has been revoked")
            {
                auth_error(error)
            } else {
                internal_error(error)
            }
        })
}

pub(crate) async fn apply_workspace_update(
    state: &Arc<AppState>,
    request: WorkspaceProfileUpdateRequest,
) -> anyhow::Result<WorkspaceProfileUpdateResponse> {
    let session = resolve_session_by_token(state, &request.session_token).await?;
    let previous = ensure_workspace_profile(state).await?;
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
        anyhow::bail!("tenantId, projectId, displayName, and region are required");
    }
    let workspace = save_workspace_profile(state, &workspace).await?;
    state.emit_console_event(
        "identity",
        Some(workspace.workspace_id.clone()),
        Some("workspace_updated".to_string()),
        format!("workspace updated by {}", session.operator_name),
    );
    Ok(WorkspaceProfileUpdateResponse {
        workspace,
        actor: session.operator_name,
    })
}

async fn list_setup_verification_receipts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListSetupVerificationReceiptsQuery>,
) -> Result<Json<Vec<SetupVerificationReceiptRecord>>, (StatusCode, Json<Value>)> {
    list_setup_verification_receipts_inner(&state, query)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn create_setup_verification_receipt(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSetupVerificationReceiptRequest>,
) -> Result<Json<SetupVerificationReceiptResponse>, (StatusCode, Json<Value>)> {
    let session = resolve_session_by_token(&state, &request.session_token)
        .await
        .map_err(auth_error)?;
    let workspace = ensure_workspace_profile(&state)
        .await
        .map_err(internal_error)?;
    let receipt = build_setup_verification_receipt(
        &workspace,
        &session.operator_name,
        &request.surface,
        &request.target,
        &capture_identity_environment_readiness(),
    )
    .map_err(bad_request)?;
    save_setup_verification_receipt(&state, &receipt)
        .await
        .map_err(internal_error)?;
    state.emit_console_event(
        "setup_verification",
        Some(receipt.receipt_id.to_string()),
        Some(receipt.status.clone()),
        format!(
            "{} {} verified by {}",
            receipt.surface, receipt.target, receipt.verified_by
        ),
    );
    Ok(Json(SetupVerificationReceiptResponse { receipt }))
}

async fn list_node_claim_audit_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListNodeClaimAuditEventsQuery>,
) -> Result<Json<Vec<NodeClaimAuditEventRecord>>, (StatusCode, Json<Value>)> {
    list_node_claim_audit_events_inner(&state, query)
        .await
        .map(Json)
        .map_err(internal_error)
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
    issue_node_claim_inner(
        &state,
        &session,
        request.node_id,
        request.display_name,
        request.transport,
        request.requested_capabilities,
        request.expires_in_seconds,
        None,
    )
    .await
    .map(Json)
    .map_err(internal_error)
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
    record_node_claim_audit_event(
        &state,
        &NodeClaimAuditEventRecord {
            event_id: 0,
            claim_id: claim.claim_id,
            node_id: claim.node_id.clone(),
            event_type: "revoked".to_string(),
            actor: session.operator_name.clone(),
            detail: reason.clone(),
            token_hint: None,
            session_url: Some(build_node_session_url(&claim)),
            created_at_unix_ms: unix_timestamp_ms(),
        },
    )
    .await
    .map_err(internal_error)?;
    state.emit_console_event(
        "node_onboarding",
        Some(claim.node_id.clone()),
        Some("claim_revoked".to_string()),
        reason.clone(),
    );
    Ok(Json(NodeClaimRevokeResponse { claim, reason }))
}

async fn reissue_node_claim(
    State(state): State<Arc<AppState>>,
    Path(claim_id): Path<Uuid>,
    Json(request): Json<ReissueNodeClaimRequest>,
) -> Result<Json<NodeClaimCreateResponse>, (StatusCode, Json<Value>)> {
    let session = resolve_session_by_token(&state, &request.session_token)
        .await
        .map_err(auth_error)?;
    expire_stale_claims(&state).await.map_err(internal_error)?;
    let mut claim = get_node_claim(&state, claim_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "node claim not found" })),
            )
        })?;
    if claim.status == NodeClaimStatus::Consumed {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "consumed node claims cannot be reissued" })),
        ));
    }
    if state
        .get_node(&claim.node_id)
        .await
        .map_err(internal_error)?
        .is_some()
    {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({ "error": "node already exists in gateway; reissuing a first-connect claim is not allowed" }),
            ),
        ));
    }
    if claim.status == NodeClaimStatus::Pending {
        claim.status = NodeClaimStatus::Revoked;
        claim.updated_at_unix_ms = unix_timestamp_ms();
        save_node_claim_record(&state, &claim)
            .await
            .map_err(internal_error)?;
        let reason = format!("reissued by {}", session.operator_name);
        record_node_claim_audit_event(
            &state,
            &NodeClaimAuditEventRecord {
                event_id: 0,
                claim_id: claim.claim_id,
                node_id: claim.node_id.clone(),
                event_type: "reissued_old_revoked".to_string(),
                actor: session.operator_name.clone(),
                detail: reason.clone(),
                token_hint: None,
                session_url: Some(build_node_session_url(&claim)),
                created_at_unix_ms: unix_timestamp_ms(),
            },
        )
        .await
        .map_err(internal_error)?;
    }
    issue_node_claim_inner(
        &state,
        &session,
        claim.node_id.clone(),
        Some(claim.display_name.clone()),
        Some(claim.transport.clone()),
        Some(claim.requested_capabilities.clone()),
        request.expires_in_seconds.or_else(|| {
            let now = unix_timestamp_ms();
            if claim.expires_at_unix_ms > now {
                Some(((claim.expires_at_unix_ms - now) / 1000) as u64)
            } else {
                None
            }
        }),
        Some(claim.claim_id),
    )
    .await
    .map(Json)
    .map_err(internal_error)
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

fn public_base_url() -> Option<String> {
    std::env::var("DAWN_PUBLIC_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn public_ws_base_url() -> String {
    if let Ok(value) = std::env::var("DAWN_PUBLIC_WS_BASE_URL") {
        if !value.trim().is_empty() {
            return value.trim_end_matches('/').to_string();
        }
    }
    if let Some(value) = public_base_url() {
        if let Some(rest) = value.strip_prefix("http://") {
            return format!("ws://{rest}");
        }
        if let Some(rest) = value.strip_prefix("https://") {
            return format!("wss://{rest}");
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

fn generate_node_claim_token() -> String {
    format!("dawn-claim-{}-{}", Uuid::new_v4(), Uuid::new_v4())
}

fn node_claim_token_hint(token: &str) -> String {
    token
        .chars()
        .rev()
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
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

fn build_node_launch_url(claim: &NodeClaimRecord, claim_token: &str) -> String {
    build_node_session_url(claim).replace("{claimToken}", &url_encode(claim_token))
}

async fn issue_node_claim_inner(
    state: &Arc<AppState>,
    session: &OperatorSessionRecord,
    node_id: String,
    display_name: Option<String>,
    transport: Option<String>,
    requested_capabilities: Option<Vec<String>>,
    expires_in_seconds: Option<u64>,
    reissued_from_claim_id: Option<Uuid>,
) -> anyhow::Result<NodeClaimCreateResponse> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        anyhow::bail!("nodeId cannot be empty");
    }
    let token = generate_node_claim_token();
    let token_hint = node_claim_token_hint(&token);
    let now = unix_timestamp_ms();
    let claim = NodeClaimRecord {
        claim_id: Uuid::new_v4(),
        node_id: node_id.clone(),
        display_name: display_name
            .unwrap_or_else(|| format!("Dawn Node {node_id}"))
            .trim()
            .to_string(),
        transport: transport
            .unwrap_or_else(|| "websocket".to_string())
            .trim()
            .to_string(),
        requested_capabilities: normalize_metadata(requested_capabilities.unwrap_or_default()),
        issued_by_session_id: Some(session.session_id),
        issued_by_operator: session.operator_name.clone(),
        status: NodeClaimStatus::Pending,
        expires_at_unix_ms: now
            + u128::from(expires_in_seconds.unwrap_or(1800).max(60)).saturating_mul(1000),
        consumed_at_unix_ms: None,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    save_node_claim(state, &claim, &token).await?;
    let session_url = build_node_session_url(&claim);
    let launch_url = build_node_launch_url(&claim, &token);
    let event_type = if reissued_from_claim_id.is_some() {
        "reissued"
    } else {
        "issued"
    };
    let reissue_detail = reissued_from_claim_id.map(|previous_claim_id| {
        format!(
            "claim reissued by {} from {}",
            session.operator_name, previous_claim_id
        )
    });
    let detail = if let Some(previous_claim_id) = reissued_from_claim_id {
        format!(
            "claim reissued from {} by {}",
            previous_claim_id, session.operator_name
        )
    } else {
        format!("claim issued by {}", session.operator_name)
    };
    record_node_claim_audit_event(
        state,
        &NodeClaimAuditEventRecord {
            event_id: 0,
            claim_id: claim.claim_id,
            node_id: claim.node_id.clone(),
            event_type: event_type.to_string(),
            actor: session.operator_name.clone(),
            detail,
            token_hint: Some(token_hint.clone()),
            session_url: Some(session_url.clone()),
            created_at_unix_ms: now,
        },
    )
    .await?;
    state.emit_console_event(
        "node_onboarding",
        Some(claim.node_id.clone()),
        Some(if reissued_from_claim_id.is_some() {
            "claim_reissued".to_string()
        } else {
            "claim_issued".to_string()
        }),
        if let Some(reissue_detail) = reissue_detail {
            reissue_detail
        } else {
            format!("claim issued by {}", claim.issued_by_operator)
        },
    );
    Ok(NodeClaimCreateResponse {
        claim,
        claim_token: token,
        session_url,
        launch_url,
        token_hint,
        reissued_from_claim_id,
        issued_by: session.operator_name.clone(),
    })
}

fn setup_target_profile(surface: &str, target: &str) -> Option<SetupTargetProfile> {
    match (surface, target) {
        ("model", "openai_codex") => Some(SetupTargetProfile {
            surface: "model",
            target: "openai_codex",
            label: "OpenAI Codex",
            region: "global",
            integration_mode: "live_chatgpt_codex_cli",
            endpoint: "/api/gateway/connectors/model/openai-codex/respond",
            note: "Uses the locally logged-in OpenAI Codex CLI account session.",
            env_hints: vec!["Run `codex login` or `dawn-node models auth-login openai-codex`"],
            env_requirement_groups: vec![],
        }),
        ("model", "openai") => Some(SetupTargetProfile {
            surface: "model",
            target: "openai",
            label: "OpenAI",
            region: "global",
            integration_mode: "live",
            endpoint: "/api/gateway/connectors/model/openai/respond",
            note: "Fastest path for global reasoning and responses.",
            env_hints: vec!["OPENAI_API_KEY"],
            env_requirement_groups: vec![vec!["OPENAI_API_KEY"]],
        }),
        ("model", "deepseek") => Some(SetupTargetProfile {
            surface: "model",
            target: "deepseek",
            label: "DeepSeek",
            region: "china",
            integration_mode: "live",
            endpoint: "/api/gateway/connectors/model/deepseek/respond",
            note: "Primary China-market reasoning provider.",
            env_hints: vec!["DEEPSEEK_API_KEY"],
            env_requirement_groups: vec![vec!["DEEPSEEK_API_KEY"]],
        }),
        ("model", "qwen") => Some(SetupTargetProfile {
            surface: "model",
            target: "qwen",
            label: "Qwen",
            region: "china",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/qwen/respond",
            note: "Strong China-native default for public-facing agents.",
            env_hints: vec!["QWEN_API_KEY or DASHSCOPE_API_KEY"],
            env_requirement_groups: vec![vec!["QWEN_API_KEY"], vec!["DASHSCOPE_API_KEY"]],
        }),
        ("model", "zhipu") => Some(SetupTargetProfile {
            surface: "model",
            target: "zhipu",
            label: "Zhipu",
            region: "china",
            integration_mode: "live",
            endpoint: "/api/gateway/connectors/model/zhipu/respond",
            note: "Useful backup path for domestic deployments.",
            env_hints: vec!["ZHIPU_API_KEY"],
            env_requirement_groups: vec![vec!["ZHIPU_API_KEY"]],
        }),
        ("model", "moonshot") => Some(SetupTargetProfile {
            surface: "model",
            target: "moonshot",
            label: "Moonshot",
            region: "china",
            integration_mode: "live",
            endpoint: "/api/gateway/connectors/model/moonshot/respond",
            note: "Long-context China model path.",
            env_hints: vec!["MOONSHOT_API_KEY"],
            env_requirement_groups: vec![vec!["MOONSHOT_API_KEY"]],
        }),
        ("model", "doubao") => Some(SetupTargetProfile {
            surface: "model",
            target: "doubao",
            label: "Doubao",
            region: "china",
            integration_mode: "live_ark_chat_compatible",
            endpoint: "/api/gateway/connectors/model/doubao/respond",
            note: "Volcengine Ark path for ByteDance-aligned deployments.",
            env_hints: vec!["DOUBAO_API_KEY or ARK_API_KEY"],
            env_requirement_groups: vec![vec!["DOUBAO_API_KEY"], vec!["ARK_API_KEY"]],
        }),
        ("model", "anthropic") => Some(SetupTargetProfile {
            surface: "model",
            target: "anthropic",
            label: "Anthropic Claude",
            region: "global",
            integration_mode: "live",
            endpoint: "/api/gateway/connectors/model/anthropic/respond",
            note: "Claude API key path for Anthropic models.",
            env_hints: vec!["ANTHROPIC_API_KEY"],
            env_requirement_groups: vec![vec!["ANTHROPIC_API_KEY"]],
        }),
        ("model", "google") => Some(SetupTargetProfile {
            surface: "model",
            target: "google",
            label: "Google Gemini",
            region: "global",
            integration_mode: "live",
            endpoint: "/api/gateway/connectors/model/google/respond",
            note: "Gemini API key path for Google models.",
            env_hints: vec!["GEMINI_API_KEY or GOOGLE_API_KEY"],
            env_requirement_groups: vec![vec!["GEMINI_API_KEY"], vec!["GOOGLE_API_KEY"]],
        }),
        ("model", "bedrock") => Some(SetupTargetProfile {
            surface: "model",
            target: "bedrock",
            label: "AWS Bedrock",
            region: "global",
            integration_mode: "live_openai_compatible_bedrock",
            endpoint: "/api/gateway/connectors/model/bedrock/respond",
            note: "OpenAI-compatible Bedrock endpoint path.",
            env_hints: vec![
                "BEDROCK_API_KEY",
                "BEDROCK_CHAT_COMPLETIONS_URL / BEDROCK_BASE_URL / BEDROCK_RUNTIME_ENDPOINT",
            ],
            env_requirement_groups: vec![
                vec!["BEDROCK_API_KEY", "BEDROCK_CHAT_COMPLETIONS_URL"],
                vec!["BEDROCK_API_KEY", "BEDROCK_BASE_URL"],
                vec!["BEDROCK_API_KEY", "BEDROCK_RUNTIME_ENDPOINT"],
            ],
        }),
        ("model", "cloudflare_ai_gateway") => Some(SetupTargetProfile {
            surface: "model",
            target: "cloudflare_ai_gateway",
            label: "Cloudflare AI Gateway",
            region: "global",
            integration_mode: "live_openai_compatible_gateway",
            endpoint: "/api/gateway/connectors/model/cloudflare-ai-gateway/respond",
            note: "Gateway-backed OpenAI-compatible Cloudflare route.",
            env_hints: vec![
                "CLOUDFLARE_AI_GATEWAY_API_KEY or OPENAI_API_KEY",
                "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL / CLOUDFLARE_AI_GATEWAY_BASE_URL / CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID + CLOUDFLARE_AI_GATEWAY_ID",
            ],
            env_requirement_groups: vec![
                vec![
                    "CLOUDFLARE_AI_GATEWAY_API_KEY",
                    "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL",
                ],
                vec![
                    "CLOUDFLARE_AI_GATEWAY_API_KEY",
                    "CLOUDFLARE_AI_GATEWAY_BASE_URL",
                ],
                vec![
                    "CLOUDFLARE_AI_GATEWAY_API_KEY",
                    "CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID",
                    "CLOUDFLARE_AI_GATEWAY_ID",
                ],
                vec![
                    "OPENAI_API_KEY",
                    "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL",
                ],
                vec!["OPENAI_API_KEY", "CLOUDFLARE_AI_GATEWAY_BASE_URL"],
                vec![
                    "OPENAI_API_KEY",
                    "CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID",
                    "CLOUDFLARE_AI_GATEWAY_ID",
                ],
            ],
        }),
        ("model", "github_models") => Some(SetupTargetProfile {
            surface: "model",
            target: "github_models",
            label: "GitHub Models",
            region: "global",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/github-models/respond",
            note: "GitHub Models OpenAI-compatible inference path.",
            env_hints: vec!["GITHUB_MODELS_API_KEY or GITHUB_TOKEN"],
            env_requirement_groups: vec![vec!["GITHUB_MODELS_API_KEY"], vec!["GITHUB_TOKEN"]],
        }),
        ("model", "huggingface") => Some(SetupTargetProfile {
            surface: "model",
            target: "huggingface",
            label: "Hugging Face",
            region: "global",
            integration_mode: "live_openai_compatible_router",
            endpoint: "/api/gateway/connectors/model/huggingface/respond",
            note: "Hugging Face OpenAI-compatible router path.",
            env_hints: vec!["HUGGINGFACE_API_KEY or HF_TOKEN"],
            env_requirement_groups: vec![vec!["HUGGINGFACE_API_KEY"], vec!["HF_TOKEN"]],
        }),
        ("model", "openrouter") => Some(SetupTargetProfile {
            surface: "model",
            target: "openrouter",
            label: "OpenRouter",
            region: "global",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/openrouter/respond",
            note: "OpenRouter OpenAI-compatible model path.",
            env_hints: vec!["OPENROUTER_API_KEY"],
            env_requirement_groups: vec![vec!["OPENROUTER_API_KEY"]],
        }),
        ("model", "groq") => Some(SetupTargetProfile {
            surface: "model",
            target: "groq",
            label: "Groq",
            region: "global",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/groq/respond",
            note: "Groq OpenAI-compatible inference path.",
            env_hints: vec!["GROQ_API_KEY"],
            env_requirement_groups: vec![vec!["GROQ_API_KEY"]],
        }),
        ("model", "together") => Some(SetupTargetProfile {
            surface: "model",
            target: "together",
            label: "Together AI",
            region: "global",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/together/respond",
            note: "Together AI OpenAI-compatible path.",
            env_hints: vec!["TOGETHER_API_KEY"],
            env_requirement_groups: vec![vec!["TOGETHER_API_KEY"]],
        }),
        ("model", "vercel_ai_gateway") => Some(SetupTargetProfile {
            surface: "model",
            target: "vercel_ai_gateway",
            label: "Vercel AI Gateway",
            region: "global",
            integration_mode: "live_openai_compatible_gateway",
            endpoint: "/api/gateway/connectors/model/vercel-ai-gateway/respond",
            note: "Gateway-backed Vercel AI inference path.",
            env_hints: vec![
                "VERCEL_AI_GATEWAY_API_KEY or AI_GATEWAY_API_KEY",
                "VERCEL_AI_GATEWAY_BASE_URL / VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL",
            ],
            env_requirement_groups: vec![
                vec!["VERCEL_AI_GATEWAY_API_KEY"],
                vec!["AI_GATEWAY_API_KEY"],
                vec!["VERCEL_AI_GATEWAY_BASE_URL"],
                vec!["VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL"],
            ],
        }),
        ("model", "vllm") => Some(SetupTargetProfile {
            surface: "model",
            target: "vllm",
            label: "vLLM",
            region: "global",
            integration_mode: "live_local_openai_compatible",
            endpoint: "/api/gateway/connectors/model/vllm/respond",
            note: "Local vLLM OpenAI-compatible path.",
            env_hints: vec!["VLLM_CHAT_COMPLETIONS_URL or VLLM_BASE_URL"],
            env_requirement_groups: vec![vec!["VLLM_CHAT_COMPLETIONS_URL"], vec!["VLLM_BASE_URL"]],
        }),
        ("model", "mistral") => Some(SetupTargetProfile {
            surface: "model",
            target: "mistral",
            label: "Mistral",
            region: "global",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/mistral/respond",
            note: "Mistral OpenAI-compatible inference path.",
            env_hints: vec!["MISTRAL_API_KEY"],
            env_requirement_groups: vec![vec!["MISTRAL_API_KEY"]],
        }),
        ("model", "nvidia") => Some(SetupTargetProfile {
            surface: "model",
            target: "nvidia",
            label: "NVIDIA NIM",
            region: "global",
            integration_mode: "live_openai_compatible",
            endpoint: "/api/gateway/connectors/model/nvidia/respond",
            note: "NVIDIA NIM OpenAI-compatible path.",
            env_hints: vec!["NVIDIA_API_KEY or NVIDIA_NIM_API_KEY"],
            env_requirement_groups: vec![vec!["NVIDIA_API_KEY"], vec!["NVIDIA_NIM_API_KEY"]],
        }),
        ("model", "litellm") => Some(SetupTargetProfile {
            surface: "model",
            target: "litellm",
            label: "LiteLLM",
            region: "global",
            integration_mode: "live_openai_compatible_gateway",
            endpoint: "/api/gateway/connectors/model/litellm/respond",
            note: "LiteLLM gateway path for OpenAI-compatible routing.",
            env_hints: vec!["LITELLM_CHAT_COMPLETIONS_URL or LITELLM_BASE_URL"],
            env_requirement_groups: vec![
                vec!["LITELLM_CHAT_COMPLETIONS_URL"],
                vec!["LITELLM_BASE_URL"],
            ],
        }),
        ("model", "ollama") => Some(SetupTargetProfile {
            surface: "model",
            target: "ollama",
            label: "Ollama",
            region: "global",
            integration_mode: "live_local_openai_compatible",
            endpoint: "/api/gateway/connectors/model/ollama/respond",
            note: "Local Ollama chat path. Optionally set OLLAMA_DEFAULT_MODEL to pin Gemma4 or another local model.",
            env_hints: vec![
                "OLLAMA_CHAT_URL or OLLAMA_BASE_URL",
                "optional: OLLAMA_DEFAULT_MODEL",
            ],
            env_requirement_groups: vec![vec!["OLLAMA_CHAT_URL"], vec!["OLLAMA_BASE_URL"]],
        }),
        ("chat", "telegram") => Some(SetupTargetProfile {
            surface: "chat",
            target: "telegram",
            label: "Telegram",
            region: "global",
            integration_mode: "live_bot",
            endpoint: "/api/gateway/connectors/chat/telegram/send",
            note: "Global outbound bot delivery.",
            env_hints: vec!["TELEGRAM_BOT_TOKEN"],
            env_requirement_groups: vec![vec!["TELEGRAM_BOT_TOKEN"]],
        }),
        ("chat", "slack") => Some(SetupTargetProfile {
            surface: "chat",
            target: "slack",
            label: "Slack",
            region: "global",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/slack/send",
            note: "Slack webhook outbound delivery.",
            env_hints: vec!["SLACK_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["SLACK_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "discord") => Some(SetupTargetProfile {
            surface: "chat",
            target: "discord",
            label: "Discord",
            region: "global",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/discord/send",
            note: "Discord webhook outbound delivery.",
            env_hints: vec!["DISCORD_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["DISCORD_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "mattermost") => Some(SetupTargetProfile {
            surface: "chat",
            target: "mattermost",
            label: "Mattermost",
            region: "global",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/mattermost/send",
            note: "Mattermost webhook outbound delivery.",
            env_hints: vec!["MATTERMOST_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["MATTERMOST_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "msteams") => Some(SetupTargetProfile {
            surface: "chat",
            target: "msteams",
            label: "Microsoft Teams",
            region: "global",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/msteams/send",
            note: "Microsoft Teams webhook outbound delivery.",
            env_hints: vec!["MSTEAMS_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["MSTEAMS_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "whatsapp") => Some(SetupTargetProfile {
            surface: "chat",
            target: "whatsapp",
            label: "WhatsApp",
            region: "global",
            integration_mode: "live_cloud_api",
            endpoint: "/api/gateway/connectors/chat/whatsapp/send",
            note: "WhatsApp Cloud API outbound delivery.",
            env_hints: vec!["WHATSAPP_ACCESS_TOKEN + WHATSAPP_PHONE_NUMBER_ID"],
            env_requirement_groups: vec![vec!["WHATSAPP_ACCESS_TOKEN", "WHATSAPP_PHONE_NUMBER_ID"]],
        }),
        ("chat", "line") => Some(SetupTargetProfile {
            surface: "chat",
            target: "line",
            label: "LINE",
            region: "global",
            integration_mode: "live_messaging_api",
            endpoint: "/api/gateway/connectors/chat/line/send",
            note: "LINE Messaging API outbound delivery.",
            env_hints: vec!["LINE_CHANNEL_ACCESS_TOKEN"],
            env_requirement_groups: vec![vec!["LINE_CHANNEL_ACCESS_TOKEN"]],
        }),
        ("chat", "matrix") => Some(SetupTargetProfile {
            surface: "chat",
            target: "matrix",
            label: "Matrix",
            region: "global",
            integration_mode: "live_matrix_client",
            endpoint: "/api/gateway/connectors/chat/matrix/send",
            note: "Matrix client outbound delivery.",
            env_hints: vec!["MATRIX_ACCESS_TOKEN + MATRIX_HOMESERVER_URL"],
            env_requirement_groups: vec![vec!["MATRIX_ACCESS_TOKEN", "MATRIX_HOMESERVER_URL"]],
        }),
        ("chat", "google_chat") => Some(SetupTargetProfile {
            surface: "chat",
            target: "google_chat",
            label: "Google Chat",
            region: "global",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/google-chat/send",
            note: "Google Chat webhook outbound delivery.",
            env_hints: vec!["GOOGLE_CHAT_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["GOOGLE_CHAT_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "signal") => Some(SetupTargetProfile {
            surface: "chat",
            target: "signal",
            label: "Signal",
            region: "global",
            integration_mode: "live_signal_rest",
            endpoint: "/api/gateway/connectors/chat/signal/send",
            note: "Signal REST outbound delivery and group actions.",
            env_hints: vec![
                "SIGNAL_ACCOUNT or SIGNAL_NUMBER or DAWN_SIGNAL_ACCOUNTS_JSON",
                "SIGNAL_HTTP_URL / SIGNAL_CLI_REST_API_URL",
            ],
            env_requirement_groups: vec![
                vec!["SIGNAL_ACCOUNT", "SIGNAL_HTTP_URL"],
                vec!["SIGNAL_ACCOUNT", "SIGNAL_CLI_REST_API_URL"],
                vec!["SIGNAL_NUMBER", "SIGNAL_HTTP_URL"],
                vec!["SIGNAL_NUMBER", "SIGNAL_CLI_REST_API_URL"],
                vec!["DAWN_SIGNAL_ACCOUNTS_JSON"],
            ],
        }),
        ("chat", "bluebubbles") => Some(SetupTargetProfile {
            surface: "chat",
            target: "bluebubbles",
            label: "BlueBubbles",
            region: "global",
            integration_mode: "live_bluebubbles_rest",
            endpoint: "/api/gateway/connectors/chat/bluebubbles/send",
            note: "BlueBubbles REST outbound delivery and iMessage management.",
            env_hints: vec![
                "BLUEBUBBLES_SERVER_URL or BLUEBUBBLES_SEND_MESSAGE_URL",
                "BLUEBUBBLES_PASSWORD",
            ],
            env_requirement_groups: vec![
                vec!["BLUEBUBBLES_SERVER_URL", "BLUEBUBBLES_PASSWORD"],
                vec!["BLUEBUBBLES_SEND_MESSAGE_URL", "BLUEBUBBLES_PASSWORD"],
                vec!["DAWN_BLUEBUBBLES_ACCOUNTS_JSON"],
            ],
        }),
        ("chat", "feishu") => Some(SetupTargetProfile {
            surface: "chat",
            target: "feishu",
            label: "Feishu",
            region: "china",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/feishu/send",
            note: "China collaboration default with inbound and outbound coverage.",
            env_hints: vec!["FEISHU_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["FEISHU_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "dingtalk") => Some(SetupTargetProfile {
            surface: "chat",
            target: "dingtalk",
            label: "DingTalk",
            region: "china",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/dingtalk/send",
            note: "Best for enterprise China deployment.",
            env_hints: vec!["DINGTALK_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["DINGTALK_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "wecom_bot") | ("chat", "wecom") => Some(SetupTargetProfile {
            surface: "chat",
            target: "wecom_bot",
            label: "WeCom Bot",
            region: "china",
            integration_mode: "live_webhook",
            endpoint: "/api/gateway/connectors/chat/wecom/send",
            note: "Outbound enterprise WeCom webhook path.",
            env_hints: vec!["WECOM_BOT_WEBHOOK_URL"],
            env_requirement_groups: vec![vec!["WECOM_BOT_WEBHOOK_URL"]],
        }),
        ("chat", "wechat_official_account") => Some(SetupTargetProfile {
            surface: "chat",
            target: "wechat_official_account",
            label: "WeChat Official Account",
            region: "china",
            integration_mode: "live_official_account_text",
            endpoint: "/api/gateway/connectors/chat/wechat-official-account/send",
            note: "Consumer-facing China messaging surface.",
            env_hints: vec![
                "WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN",
                "WECHAT_OFFICIAL_ACCOUNT_APP_ID + WECHAT_OFFICIAL_ACCOUNT_APP_SECRET",
            ],
            env_requirement_groups: vec![
                vec!["WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN"],
                vec![
                    "WECHAT_OFFICIAL_ACCOUNT_APP_ID",
                    "WECHAT_OFFICIAL_ACCOUNT_APP_SECRET",
                ],
            ],
        }),
        ("chat", "qq") => Some(SetupTargetProfile {
            surface: "chat",
            target: "qq",
            label: "QQ Bot",
            region: "china",
            integration_mode: "live_openapi_c2c_group",
            endpoint: "/api/gateway/connectors/chat/qq/send",
            note: "Youth and community-oriented China entry point.",
            env_hints: vec!["QQ_BOT_APP_ID + QQ_BOT_CLIENT_SECRET"],
            env_requirement_groups: vec![vec!["QQ_BOT_APP_ID", "QQ_BOT_CLIENT_SECRET"]],
        }),
        ("ingress", "telegram") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "telegram",
            label: "Telegram Webhook",
            region: "global",
            integration_mode: "secret_path_webhook",
            endpoint: "/api/gateway/ingress/telegram/webhook/{secret}",
            note: "Inbound Telegram task creation path.",
            env_hints: vec!["DAWN_TELEGRAM_WEBHOOK_SECRET"],
            env_requirement_groups: vec![vec!["DAWN_TELEGRAM_WEBHOOK_SECRET"]],
        }),
        ("ingress", "feishu") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "feishu",
            label: "Feishu Events",
            region: "china",
            integration_mode: "challenge_callback",
            endpoint: "/api/gateway/ingress/feishu/events",
            note: "Inbound Feishu event challenge and message route.",
            env_hints: vec!["No secret required for basic challenge mode"],
            env_requirement_groups: vec![],
        }),
        ("ingress", "dingtalk") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "dingtalk",
            label: "DingTalk Events",
            region: "china",
            integration_mode: "callback_token",
            endpoint: "/api/gateway/ingress/dingtalk/events",
            note: "Inbound DingTalk task launch route.",
            env_hints: vec!["DAWN_DINGTALK_CALLBACK_TOKEN"],
            env_requirement_groups: vec![vec!["DAWN_DINGTALK_CALLBACK_TOKEN"]],
        }),
        ("ingress", "wecom") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "wecom",
            label: "WeCom Events",
            region: "china",
            integration_mode: "callback_token",
            endpoint: "/api/gateway/ingress/wecom/events",
            note: "Inbound enterprise WeCom route.",
            env_hints: vec!["DAWN_WECOM_CALLBACK_TOKEN"],
            env_requirement_groups: vec![vec!["DAWN_WECOM_CALLBACK_TOKEN"]],
        }),
        ("ingress", "wechat_official_account") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "wechat_official_account",
            label: "WeChat Official Account Events",
            region: "china",
            integration_mode: "token_verification",
            endpoint: "/api/gateway/ingress/wechat-official-account/events",
            note: "Inbound WeChat OA verification and XML message route.",
            env_hints: vec!["DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN"],
            env_requirement_groups: vec![vec!["DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN"]],
        }),
        ("ingress", "qq") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "qq",
            label: "QQ Bot Events",
            region: "china",
            integration_mode: "callback_secret",
            endpoint: "/api/gateway/ingress/qq/events",
            note: "Inbound QQ bot event route.",
            env_hints: vec!["DAWN_QQ_BOT_CALLBACK_SECRET"],
            env_requirement_groups: vec![vec!["DAWN_QQ_BOT_CALLBACK_SECRET"]],
        }),
        ("ingress", "signal") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "signal",
            label: "Signal Events",
            region: "global",
            integration_mode: "secret_path_callback",
            endpoint: "/api/gateway/ingress/signal/events/{secret}",
            note: "Inbound Signal text, attachment, reaction, typing, and receipt events.",
            env_hints: vec!["DAWN_SIGNAL_CALLBACK_SECRET"],
            env_requirement_groups: vec![vec!["DAWN_SIGNAL_CALLBACK_SECRET"]],
        }),
        ("ingress", "bluebubbles") => Some(SetupTargetProfile {
            surface: "ingress",
            target: "bluebubbles",
            label: "BlueBubbles Events",
            region: "global",
            integration_mode: "secret_path_callback",
            endpoint: "/api/gateway/ingress/bluebubbles/events/{secret}",
            note: "Inbound BlueBubbles message, reaction, typing, and group events.",
            env_hints: vec!["DAWN_BLUEBUBBLES_CALLBACK_SECRET"],
            env_requirement_groups: vec![vec!["DAWN_BLUEBUBBLES_CALLBACK_SECRET"]],
        }),
        _ => None,
    }
}

pub(crate) fn setup_target_hint(surface: &str, target: &str) -> Option<Value> {
    let profile = setup_target_profile(surface, target)?;
    Some(json!({
        "surface": profile.surface,
        "target": profile.target,
        "label": profile.label,
        "integrationMode": profile.integration_mode,
        "endpoint": profile.endpoint,
        "note": profile.note,
        "envHints": profile.env_hints,
    }))
}

fn capture_identity_environment_readiness() -> IdentityEnvironmentReadiness {
    IdentityEnvironmentReadiness {
        public_base_url: public_base_url(),
        configured_model_providers: [
            "openai_codex",
            "openai",
            "anthropic",
            "google",
            "bedrock",
            "cloudflare_ai_gateway",
            "github_models",
            "huggingface",
            "openrouter",
            "groq",
            "together",
            "vercel_ai_gateway",
            "vllm",
            "mistral",
            "nvidia",
            "litellm",
            "ollama",
            "deepseek",
            "qwen",
            "zhipu",
            "moonshot",
            "doubao",
        ]
        .into_iter()
        .filter(|provider| is_model_provider_configured(provider))
        .map(ToString::to_string)
        .collect(),
        configured_chat_platforms: [
            "telegram",
            "slack",
            "discord",
            "mattermost",
            "msteams",
            "whatsapp",
            "line",
            "matrix",
            "google_chat",
            "signal",
            "bluebubbles",
            "feishu",
            "dingtalk",
            "wecom_bot",
            "wechat_official_account",
            "qq",
        ]
        .into_iter()
        .filter(|platform| is_chat_platform_configured(platform))
        .map(ToString::to_string)
        .collect(),
        configured_ingress_platforms: [
            "telegram",
            "signal",
            "bluebubbles",
            "feishu",
            "dingtalk",
            "wecom",
            "wechat_official_account",
            "qq",
        ]
        .into_iter()
        .filter(|platform| is_ingress_platform_configured(platform))
        .map(ToString::to_string)
        .collect(),
        present_env_keys: [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "BEDROCK_API_KEY",
            "BEDROCK_CHAT_COMPLETIONS_URL",
            "BEDROCK_BASE_URL",
            "BEDROCK_RUNTIME_ENDPOINT",
            "CLOUDFLARE_AI_GATEWAY_API_KEY",
            "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL",
            "CLOUDFLARE_AI_GATEWAY_BASE_URL",
            "CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID",
            "CLOUDFLARE_AI_GATEWAY_ID",
            "GITHUB_MODELS_API_KEY",
            "GITHUB_TOKEN",
            "GITHUB_MODELS_CHAT_COMPLETIONS_URL",
            "HUGGINGFACE_API_KEY",
            "HF_TOKEN",
            "HUGGINGFACE_CHAT_COMPLETIONS_URL",
            "OPENROUTER_API_KEY",
            "GROQ_API_KEY",
            "TOGETHER_API_KEY",
            "VERCEL_AI_GATEWAY_API_KEY",
            "AI_GATEWAY_API_KEY",
            "VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL",
            "VERCEL_AI_GATEWAY_BASE_URL",
            "VLLM_API_KEY",
            "VLLM_CHAT_COMPLETIONS_URL",
            "VLLM_BASE_URL",
            "MISTRAL_API_KEY",
            "NVIDIA_API_KEY",
            "NVIDIA_NIM_API_KEY",
            "NVIDIA_CHAT_COMPLETIONS_URL",
            "LITELLM_API_KEY",
            "LITELLM_CHAT_COMPLETIONS_URL",
            "LITELLM_BASE_URL",
            "OLLAMA_CHAT_URL",
            "OLLAMA_BASE_URL",
            "DEEPSEEK_API_KEY",
            "QWEN_API_KEY",
            "DASHSCOPE_API_KEY",
            "ZHIPU_API_KEY",
            "MOONSHOT_API_KEY",
            "DOUBAO_API_KEY",
            "ARK_API_KEY",
            "TELEGRAM_BOT_TOKEN",
            "SLACK_BOT_WEBHOOK_URL",
            "DISCORD_BOT_WEBHOOK_URL",
            "MATTERMOST_BOT_WEBHOOK_URL",
            "MSTEAMS_BOT_WEBHOOK_URL",
            "WHATSAPP_ACCESS_TOKEN",
            "WHATSAPP_PHONE_NUMBER_ID",
            "LINE_CHANNEL_ACCESS_TOKEN",
            "MATRIX_ACCESS_TOKEN",
            "MATRIX_HOMESERVER_URL",
            "GOOGLE_CHAT_BOT_WEBHOOK_URL",
            "SIGNAL_ACCOUNT",
            "SIGNAL_NUMBER",
            "SIGNAL_HTTP_URL",
            "SIGNAL_CLI_REST_API_URL",
            "SIGNAL_SEND_API_URL",
            "SIGNAL_REACTION_API_URL",
            "SIGNAL_RECEIPT_API_URL",
            "DAWN_SIGNAL_ACCOUNTS_JSON",
            "BLUEBUBBLES_PASSWORD",
            "BLUEBUBBLES_SERVER_URL",
            "BLUEBUBBLES_SEND_MESSAGE_URL",
            "BLUEBUBBLES_SEND_ATTACHMENT_URL",
            "BLUEBUBBLES_SEND_REACTION_URL",
            "DAWN_BLUEBUBBLES_ACCOUNTS_JSON",
            "FEISHU_BOT_WEBHOOK_URL",
            "DINGTALK_BOT_WEBHOOK_URL",
            "WECOM_BOT_WEBHOOK_URL",
            "WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN",
            "WECHAT_OFFICIAL_ACCOUNT_APP_ID",
            "WECHAT_OFFICIAL_ACCOUNT_APP_SECRET",
            "QQ_BOT_APP_ID",
            "QQ_BOT_CLIENT_SECRET",
            "DAWN_TELEGRAM_WEBHOOK_SECRET",
            "DAWN_SIGNAL_CALLBACK_SECRET",
            "DAWN_BLUEBUBBLES_CALLBACK_SECRET",
            "DAWN_DINGTALK_CALLBACK_TOKEN",
            "DAWN_WECOM_CALLBACK_TOKEN",
            "DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN",
            "DAWN_QQ_BOT_CALLBACK_SECRET",
            "DAWN_PUBLIC_BASE_URL",
            "DAWN_PUBLIC_WS_BASE_URL",
        ]
        .into_iter()
        .filter(|name| env_var_present(name))
        .map(ToString::to_string)
        .collect(),
    }
}

fn build_setup_verification_receipt(
    workspace: &WorkspaceProfileRecord,
    verified_by: &str,
    surface: &str,
    target: &str,
    environment: &IdentityEnvironmentReadiness,
) -> anyhow::Result<SetupVerificationReceiptRecord> {
    let surface = surface.trim().to_ascii_lowercase();
    let target = target.trim().to_ascii_lowercase();
    let profile = setup_target_profile(&surface, &target)
        .ok_or_else(|| anyhow!("unsupported setup target '{surface}:{target}'"))?;
    let env_keys = profile
        .env_requirement_groups
        .iter()
        .flatten()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let configured = profile.env_requirement_groups.is_empty()
        || profile.env_requirement_groups.iter().any(|group| {
            group
                .iter()
                .all(|name| env_var_present_in_environment(environment, name))
        });
    let missing_env_keys = if configured {
        vec![]
    } else {
        profile
            .env_requirement_groups
            .iter()
            .flatten()
            .filter(|name| !env_var_present_in_environment(environment, name))
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>()
    };
    let is_default_path = is_default_setup_target(workspace, &surface, profile.target);
    let status = if configured {
        "ready"
    } else {
        "action_required"
    };
    let summary = if configured {
        if is_default_path {
            format!(
                "{} is configured for the workspace default onboarding path.",
                profile.label
            )
        } else {
            format!(
                "{} is configured, but it is not on the workspace default onboarding path.",
                profile.label
            )
        }
    } else {
        format!(
            "{} is missing credentials or callback secrets required for verification.",
            profile.label
        )
    };
    let detail = if configured {
        if is_default_path {
            format!(
                "{} · {} · verify with {}. {}",
                profile.region, profile.integration_mode, profile.endpoint, profile.note
            )
        } else {
            format!(
                "{} · {} · verify with {}. {} Add this target to workspace defaults if you want it on the main onboarding path.",
                profile.region, profile.integration_mode, profile.endpoint, profile.note
            )
        }
    } else if profile.env_hints.is_empty() {
        format!("{} cannot be verified yet. {}", profile.label, profile.note)
    } else {
        format!(
            "Missing {}. Verify again after configuring {} and reloading the gateway environment. {}",
            missing_env_keys.join(", "),
            profile.env_hints.join(" / "),
            profile.note
        )
    };
    let action = if configured {
        if is_default_path {
            None
        } else {
            Some("This target is healthy. Add it to workspace defaults if you want it on the main go-live path.".to_string())
        }
    } else {
        Some(format!(
            "Configure {} and run Verify Target again.",
            profile.env_hints.join(" / ")
        ))
    };

    Ok(SetupVerificationReceiptRecord {
        receipt_id: Uuid::new_v4(),
        surface: profile.surface.to_string(),
        target: profile.target.to_string(),
        label: profile.label.to_string(),
        region: profile.region.to_string(),
        integration_mode: profile.integration_mode.to_string(),
        status: status.to_string(),
        summary,
        detail,
        action,
        endpoint: profile.endpoint.to_string(),
        env_keys,
        missing_env_keys,
        is_default_path,
        verified_by: verified_by.trim().to_string(),
        created_at_unix_ms: unix_timestamp_ms(),
    })
}

fn build_identity_readiness(
    workspace: &WorkspaceProfileRecord,
    active_sessions: usize,
    claims: &[NodeClaimRecord],
    nodes: &[NodeRecord],
    pending_payment_approvals: usize,
    pending_end_user_sessions: usize,
    environment: &IdentityEnvironmentReadiness,
) -> IdentityReadinessSummary {
    let pending_node_claims = claims
        .iter()
        .filter(|claim| claim.status == NodeClaimStatus::Pending)
        .count();
    let consumed_node_claims = claims
        .iter()
        .filter(|claim| claim.status == NodeClaimStatus::Consumed)
        .count();
    let connected_nodes = nodes.iter().filter(|node| node.connected).count();
    let trusted_nodes = nodes
        .iter()
        .filter(|node| node.connected && node.attestation_verified)
        .count();

    let missing_model_providers = workspace
        .default_model_providers
        .iter()
        .filter(|provider| !environment_has(&environment.configured_model_providers, provider))
        .cloned()
        .collect::<Vec<_>>();
    let missing_chat_platforms = workspace
        .default_chat_platforms
        .iter()
        .filter(|platform| {
            !environment_has(
                &environment.configured_chat_platforms,
                chat_connector_target(platform),
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut ingress_targets = workspace
        .default_chat_platforms
        .iter()
        .filter_map(|platform| ingress_target_for_chat_platform(platform))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    ingress_targets.sort();
    ingress_targets.dedup();
    let missing_ingress_platforms = ingress_targets
        .iter()
        .filter(|platform| !environment_has(&environment.configured_ingress_platforms, platform))
        .cloned()
        .collect::<Vec<_>>();

    let workspace_ready = is_workspace_profile_ready(workspace);
    let public_base_url_ready = environment.public_base_url.is_some();
    let first_missing_model = missing_model_providers.first().cloned();
    let first_missing_chat = missing_chat_platforms
        .first()
        .map(|platform| chat_connector_target(platform).to_string());
    let first_missing_ingress = missing_ingress_platforms.first().cloned();

    let checklist = vec![
        readiness_item(
            "operator_session",
            "Bootstrap Operator Session",
            if active_sessions > 0 {
                "ready"
            } else {
                "action_required"
            },
            if active_sessions > 0 {
                format!("{active_sessions} active operator session(s) can mutate onboarding state.")
            } else {
                "No operator session has been bootstrapped yet.".to_string()
            },
            if active_sessions > 0 {
                None
            } else {
                Some("Bootstrap an operator session from the control center.".to_string())
            },
            None,
            None,
        ),
        readiness_item(
            "workspace_profile",
            "Configure Workspace Identity",
            if workspace_ready {
                "ready"
            } else {
                "action_required"
            },
            if workspace_ready {
                format!(
                    "{} · {} · onboarding status {}",
                    workspace.display_name, workspace.region, workspace.onboarding_status
                )
            } else {
                format!(
                    "Workspace still reports {}. Save tenant, project, region, and a non-bootstrap onboarding status.",
                    workspace.onboarding_status
                )
            },
            if workspace_ready {
                None
            } else {
                Some(
                    "Save the workspace profile and move onboardingStatus past bootstrap_pending."
                        .to_string(),
                )
            },
            None,
            None,
        ),
        readiness_item(
            "public_gateway_url",
            "Publish Gateway Base URL",
            if public_base_url_ready {
                "ready"
            } else {
                "action_required"
            },
            if let Some(base_url) = environment.public_base_url.as_deref() {
                format!(
                    "Public links resolve from {base_url}; end-user approvals and node first-connect links can escape localhost."
                )
            } else {
                "DAWN_PUBLIC_BASE_URL is unset, so approval links stay relative and node onboarding defaults to localhost.".to_string()
            },
            if public_base_url_ready {
                None
            } else {
                Some(
                    "Set DAWN_PUBLIC_BASE_URL to the externally reachable gateway origin."
                        .to_string(),
                )
            },
            None,
            None,
        ),
        readiness_item(
            "default_model_connectors",
            "Wire Default Model Connectors",
            if workspace.default_model_providers.is_empty() {
                "action_required"
            } else if missing_model_providers.is_empty() {
                "ready"
            } else {
                "action_required"
            },
            if workspace.default_model_providers.is_empty() {
                "Workspace defaultModelProviders is empty.".to_string()
            } else if missing_model_providers.is_empty() {
                format!(
                    "All default model providers are credentialed: {}.",
                    workspace.default_model_providers.join(", ")
                )
            } else {
                format!(
                    "Missing connector credentials for: {}.",
                    missing_model_providers.join(", ")
                )
            },
            if workspace.default_model_providers.is_empty() {
                Some(
                    "Add at least one default model provider to the workspace profile.".to_string(),
                )
            } else if missing_model_providers.is_empty() {
                None
            } else {
                Some("Configure the missing model connector credentials.".to_string())
            },
            first_missing_model.as_ref().map(|_| "model".to_string()),
            first_missing_model,
        ),
        readiness_item(
            "default_chat_connectors",
            "Wire Default Chat Connectors",
            if workspace.default_chat_platforms.is_empty() {
                "action_required"
            } else if missing_chat_platforms.is_empty() {
                "ready"
            } else {
                "action_required"
            },
            if workspace.default_chat_platforms.is_empty() {
                "Workspace defaultChatPlatforms is empty.".to_string()
            } else if missing_chat_platforms.is_empty() {
                format!(
                    "All default chat platforms can send outbound replies: {}.",
                    workspace.default_chat_platforms.join(", ")
                )
            } else {
                format!(
                    "Missing outbound connector credentials for: {}.",
                    missing_chat_platforms.join(", ")
                )
            },
            if workspace.default_chat_platforms.is_empty() {
                Some("Add at least one default chat platform to the workspace profile.".to_string())
            } else if missing_chat_platforms.is_empty() {
                None
            } else {
                Some(
                    first_missing_chat
                        .as_deref()
                        .map(chat_connector_action_hint)
                        .unwrap_or_else(|| {
                            "Configure the missing chat connector credentials.".to_string()
                        }),
                )
            },
            first_missing_chat.as_ref().map(|_| "chat".to_string()),
            first_missing_chat,
        ),
        readiness_item(
            "default_ingress_paths",
            "Wire Default Ingress Routes",
            if ingress_targets.is_empty() {
                "action_required"
            } else if missing_ingress_platforms.is_empty() {
                "ready"
            } else {
                "action_required"
            },
            if ingress_targets.is_empty() {
                "No inbound ingress platforms can be derived from the workspace chat defaults."
                    .to_string()
            } else if missing_ingress_platforms.is_empty() {
                format!(
                    "Inbound task creation paths are ready for: {}.",
                    ingress_targets.join(", ")
                )
            } else {
                format!(
                    "Inbound webhook or callback secrets are still missing for: {}.",
                    missing_ingress_platforms.join(", ")
                )
            },
            if ingress_targets.is_empty() {
                Some(
                    "Choose at least one chat platform that also has an ingress route.".to_string(),
                )
            } else if missing_ingress_platforms.is_empty() {
                None
            } else {
                Some(
                    first_missing_ingress
                        .as_deref()
                        .map(ingress_route_action_hint)
                        .unwrap_or_else(|| {
                            "Set the callback secret or token for the missing ingress route."
                                .to_string()
                        }),
                )
            },
            first_missing_ingress
                .as_ref()
                .map(|_| "ingress".to_string()),
            first_missing_ingress,
        ),
        readiness_item(
            "node_claim",
            "Issue First Node Claim",
            if !claims.is_empty() || !nodes.is_empty() {
                if pending_node_claims > 0 && connected_nodes == 0 {
                    "in_progress"
                } else {
                    "ready"
                }
            } else {
                "action_required"
            },
            if !claims.is_empty() || !nodes.is_empty() {
                if pending_node_claims > 0 && connected_nodes == 0 {
                    format!(
                        "{pending_node_claims} pending node claim(s) issued; open the first-connect URL on the target node."
                    )
                } else if consumed_node_claims > 0 {
                    format!(
                        "{consumed_node_claims} node claim(s) have already been consumed by real node sessions."
                    )
                } else {
                    format!(
                        "{} node record(s) already exist in the gateway ledger.",
                        nodes.len()
                    )
                }
            } else {
                "No node claim has been issued yet.".to_string()
            },
            if !claims.is_empty() || !nodes.is_empty() {
                Some("Complete the first-connect URL on the target node if it has not connected yet.".to_string())
            } else {
                Some("Issue a node claim for the first Dawn node.".to_string())
            },
            None,
            None,
        ),
        readiness_item(
            "trusted_node",
            "Bring A Trusted Node Online",
            if trusted_nodes > 0 {
                "ready"
            } else if connected_nodes > 0 || pending_node_claims > 0 {
                "in_progress"
            } else {
                "action_required"
            },
            if trusted_nodes > 0 {
                format!(
                    "{trusted_nodes} connected node(s) have verified attestation and are ready for work."
                )
            } else if connected_nodes > 0 {
                format!(
                    "{connected_nodes} node(s) are connected, but attestation is still unverified."
                )
            } else if pending_node_claims > 0 {
                format!("{pending_node_claims} claimed node(s) are still waiting to connect.")
            } else {
                "No trusted node is online yet.".to_string()
            },
            if trusted_nodes > 0 {
                None
            } else {
                Some("Bring one claimed node online and verify its attestation.".to_string())
            },
            None,
            None,
        ),
        readiness_item(
            "end_user_approval",
            "Verify End-User Approval Path",
            if !public_base_url_ready {
                "action_required"
            } else if pending_payment_approvals > 0 && pending_end_user_sessions == 0 {
                "action_required"
            } else {
                "ready"
            },
            if !public_base_url_ready {
                "End-user approval links are not publicly routable until DAWN_PUBLIC_BASE_URL is set.".to_string()
            } else if pending_payment_approvals > 0 && pending_end_user_sessions == 0 {
                format!(
                    "{pending_payment_approvals} pending payment approval(s) exist, but there are no live end-user approval sessions."
                )
            } else if pending_end_user_sessions > 0 {
                format!(
                    "{pending_end_user_sessions} end-user approval session(s) are currently live."
                )
            } else {
                "The approval portal is routable and there are no pending end-user approvals right now.".to_string()
            },
            if !public_base_url_ready {
                Some(
                    "Set DAWN_PUBLIC_BASE_URL so chat users receive a reachable approval link."
                        .to_string(),
                )
            } else if pending_payment_approvals > 0 && pending_end_user_sessions == 0 {
                Some("Inspect payment authorization flow and ensure approval sessions are issued when AP2 payments pause for end-user consent.".to_string())
            } else {
                None
            },
            None,
            None,
        ),
    ];

    let ready_steps = checklist
        .iter()
        .filter(|item| item.status == "ready")
        .count();
    let total_steps = checklist.len();
    let completion_score = checklist.iter().fold(0.0_f32, |score, item| {
        score
            + match item.status.as_str() {
                "ready" => 1.0,
                "in_progress" => 0.5,
                _ => 0.0,
            }
    });
    let completion_percent = ((completion_score / total_steps.max(1) as f32) * 100.0).round() as u8;
    let next_step = checklist
        .iter()
        .find(|item| item.status != "ready")
        .and_then(|item| item.action.clone().or_else(|| Some(item.label.clone())));

    IdentityReadinessSummary {
        overall_status: if ready_steps == total_steps {
            "ready".to_string()
        } else if ready_steps > 0 || checklist.iter().any(|item| item.status == "in_progress") {
            "in_progress".to_string()
        } else {
            "action_required".to_string()
        },
        completion_percent,
        next_step,
        ready_steps,
        total_steps,
        metrics: IdentityReadinessMetrics {
            active_sessions,
            total_nodes: nodes.len(),
            connected_nodes,
            trusted_nodes,
            issued_node_claims: claims.len(),
            pending_node_claims,
            default_model_providers_ready: workspace
                .default_model_providers
                .len()
                .saturating_sub(missing_model_providers.len()),
            total_default_model_providers: workspace.default_model_providers.len(),
            default_chat_platforms_ready: workspace
                .default_chat_platforms
                .len()
                .saturating_sub(missing_chat_platforms.len()),
            total_default_chat_platforms: workspace.default_chat_platforms.len(),
            ingress_platforms_ready: ingress_targets
                .len()
                .saturating_sub(missing_ingress_platforms.len()),
            total_ingress_platforms: ingress_targets.len(),
            pending_payment_approvals,
            pending_end_user_sessions,
            public_base_url_configured: public_base_url_ready,
        },
        checklist,
    }
}

fn readiness_item(
    key: &str,
    label: &str,
    status: &str,
    detail: String,
    action: Option<String>,
    surface: Option<String>,
    target: Option<String>,
) -> IdentityReadinessItem {
    IdentityReadinessItem {
        key: key.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail,
        action,
        surface,
        target,
    }
}

fn chat_connector_action_hint(platform: &str) -> String {
    match platform {
        "telegram" => {
            "Configure the Telegram bot token, then send /help to verify command discovery."
                .to_string()
        }
        "feishu" => {
            "Set FEISHU_BOT_WEBHOOK_URL, then send `帮助` or `@机器人 /help` in Feishu."
                .to_string()
        }
        "dingtalk" => {
            "Set DINGTALK_BOT_WEBHOOK_URL, then test with `帮助` or `@机器人 /help` in DingTalk."
                .to_string()
        }
        "wecom_bot" | "wecom" => {
            "Set WECOM_BOT_WEBHOOK_URL, then test with `帮助` or `@机器人 /help` in WeCom."
                .to_string()
        }
        "wechat_official_account" => {
            "Set the WeChat Official Account credentials, then test with `帮助` or `／skills`."
                .to_string()
        }
        "qq" => {
            "Set QQ_BOT_APP_ID and QQ_BOT_CLIENT_SECRET, then test with `帮助` or `@机器人 /help`."
                .to_string()
        }
        "signal" => {
            "Configure the Signal account and server URL, then send /help in the paired chat."
                .to_string()
        }
        "bluebubbles" => {
            "Configure the BlueBubbles server URL and password, then send /help in the paired chat."
                .to_string()
        }
        _ => "Configure the missing chat connector credentials.".to_string(),
    }
}

fn ingress_route_action_hint(platform: &str) -> String {
    match platform {
        "telegram" => {
            "Set DAWN_TELEGRAM_WEBHOOK_SECRET or enable polling, then send /help to confirm ingress."
                .to_string()
        }
        "feishu" => {
            "Point the Feishu event callback at /api/gateway/ingress/feishu/events, then send `帮助`."
                .to_string()
        }
        "dingtalk" => {
            "Set DAWN_DINGTALK_CALLBACK_TOKEN, wire the DingTalk callback URL, then send `帮助`."
                .to_string()
        }
        "wecom" => {
            "Set DAWN_WECOM_CALLBACK_TOKEN, wire the WeCom callback URL, then send `帮助`."
                .to_string()
        }
        "wechat_official_account" => {
            "Set DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN, complete the WeChat callback verification, then send `帮助`."
                .to_string()
        }
        "qq" => {
            "Set DAWN_QQ_BOT_CALLBACK_SECRET, wire the QQ bot callback, then send `帮助`."
                .to_string()
        }
        "signal" => {
            "Set DAWN_SIGNAL_CALLBACK_SECRET, expose the Signal callback URL, then send /help."
                .to_string()
        }
        "bluebubbles" => {
            "Set DAWN_BLUEBUBBLES_CALLBACK_SECRET, expose the BlueBubbles callback URL, then send /help."
                .to_string()
        }
        _ => "Set the callback secret or token for the missing ingress route.".to_string(),
    }
}

fn is_workspace_profile_ready(workspace: &WorkspaceProfileRecord) -> bool {
    !workspace.tenant_id.trim().is_empty()
        && !workspace.project_id.trim().is_empty()
        && !workspace.display_name.trim().is_empty()
        && !workspace.region.trim().is_empty()
        && workspace.onboarding_status.trim() != "bootstrap_pending"
}

fn is_default_setup_target(
    workspace: &WorkspaceProfileRecord,
    surface: &str,
    target: &str,
) -> bool {
    match surface {
        "model" => workspace
            .default_model_providers
            .iter()
            .any(|value| value == target),
        "chat" => workspace
            .default_chat_platforms
            .iter()
            .any(|value| chat_connector_target(value) == target),
        "ingress" => workspace
            .default_chat_platforms
            .iter()
            .filter_map(|platform| ingress_target_for_chat_platform(platform))
            .any(|value| value == target),
        _ => false,
    }
}

fn environment_has(configured: &[String], key: &str) -> bool {
    configured.iter().any(|value| value == key)
}

fn env_var_present_in_environment(environment: &IdentityEnvironmentReadiness, name: &str) -> bool {
    environment
        .present_env_keys
        .iter()
        .any(|value| value == name)
}

fn env_var_present(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn resolve_first_present_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn any_env_var_present(names: &[&str]) -> bool {
    names.iter().any(|name| env_var_present(name))
}

fn has_wechat_official_account_credentials() -> bool {
    env_var_present("WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN")
        || (env_var_present("WECHAT_OFFICIAL_ACCOUNT_APP_ID")
            && env_var_present("WECHAT_OFFICIAL_ACCOUNT_APP_SECRET"))
}

fn has_qq_bot_credentials() -> bool {
    env_var_present("QQ_BOT_APP_ID") && env_var_present("QQ_BOT_CLIENT_SECRET")
}

fn has_signal_account_configuration() -> bool {
    env_var_present("SIGNAL_ACCOUNT")
        || env_var_present("SIGNAL_NUMBER")
        || env_var_present("DAWN_SIGNAL_ACCOUNTS_JSON")
}

fn has_bluebubbles_account_configuration() -> bool {
    env_var_present("BLUEBUBBLES_SERVER_URL")
        || env_var_present("BLUEBUBBLES_SEND_MESSAGE_URL")
        || env_var_present("DAWN_BLUEBUBBLES_ACCOUNTS_JSON")
}

fn has_bedrock_configuration() -> bool {
    env_var_present("BEDROCK_API_KEY")
        && (env_var_present("BEDROCK_CHAT_COMPLETIONS_URL")
            || env_var_present("BEDROCK_BASE_URL")
            || env_var_present("BEDROCK_RUNTIME_ENDPOINT"))
}

fn has_cloudflare_ai_gateway_configuration() -> bool {
    resolve_first_present_env(&["CLOUDFLARE_AI_GATEWAY_API_KEY", "OPENAI_API_KEY"]).is_some()
        && (env_var_present("CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL")
            || env_var_present("CLOUDFLARE_AI_GATEWAY_BASE_URL")
            || (env_var_present("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID")
                && env_var_present("CLOUDFLARE_AI_GATEWAY_ID")))
}

fn has_vercel_ai_gateway_configuration() -> bool {
    resolve_first_present_env(&["VERCEL_AI_GATEWAY_API_KEY", "AI_GATEWAY_API_KEY"]).is_some()
        || env_var_present("VERCEL_AI_GATEWAY_BASE_URL")
        || env_var_present("VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL")
}

fn has_vllm_configuration() -> bool {
    env_var_present("VLLM_CHAT_COMPLETIONS_URL") || env_var_present("VLLM_BASE_URL")
}

fn has_litellm_configuration() -> bool {
    env_var_present("LITELLM_CHAT_COMPLETIONS_URL") || env_var_present("LITELLM_BASE_URL")
}

fn has_ollama_configuration() -> bool {
    env_var_present("OLLAMA_CHAT_URL") || env_var_present("OLLAMA_BASE_URL")
}

fn is_model_provider_configured(provider: &str) -> bool {
    match provider {
        "openai_codex" => openai_codex_login_ready(),
        "openai" => env_var_present("OPENAI_API_KEY"),
        "anthropic" => env_var_present("ANTHROPIC_API_KEY"),
        "google" => resolve_first_present_env(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]).is_some(),
        "bedrock" => has_bedrock_configuration(),
        "cloudflare_ai_gateway" => has_cloudflare_ai_gateway_configuration(),
        "github_models" => {
            resolve_first_present_env(&["GITHUB_MODELS_API_KEY", "GITHUB_TOKEN"]).is_some()
        }
        "huggingface" => resolve_first_present_env(&["HUGGINGFACE_API_KEY", "HF_TOKEN"]).is_some(),
        "openrouter" => env_var_present("OPENROUTER_API_KEY"),
        "groq" => env_var_present("GROQ_API_KEY"),
        "together" => env_var_present("TOGETHER_API_KEY"),
        "vercel_ai_gateway" => has_vercel_ai_gateway_configuration(),
        "vllm" => has_vllm_configuration(),
        "mistral" => env_var_present("MISTRAL_API_KEY"),
        "nvidia" => resolve_first_present_env(&["NVIDIA_API_KEY", "NVIDIA_NIM_API_KEY"]).is_some(),
        "litellm" => has_litellm_configuration(),
        "ollama" => has_ollama_configuration(),
        "deepseek" => env_var_present("DEEPSEEK_API_KEY"),
        "qwen" => any_env_var_present(&["QWEN_API_KEY", "DASHSCOPE_API_KEY"]),
        "zhipu" => env_var_present("ZHIPU_API_KEY"),
        "moonshot" => env_var_present("MOONSHOT_API_KEY"),
        "doubao" => any_env_var_present(&["DOUBAO_API_KEY", "ARK_API_KEY"]),
        _ => false,
    }
}

fn openai_codex_login_ready() -> bool {
    if codex_auth_file_present() {
        return true;
    }
    let output = new_codex_command(&["login", "status"]).output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("Logged in using") || stdout.to_ascii_lowercase().contains("logged in")
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
    base.map(|dir| dir.join("auth.json").exists())
        .unwrap_or(false)
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
            if let Some(first) = candidates.into_iter().find(|path| path.exists()) {
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

fn chat_connector_target(platform: &str) -> &str {
    match platform {
        "wecom" => "wecom_bot",
        other => other,
    }
}

fn is_chat_platform_configured(platform: &str) -> bool {
    match chat_connector_target(platform) {
        "telegram" => env_var_present("TELEGRAM_BOT_TOKEN"),
        "feishu" => env_var_present("FEISHU_BOT_WEBHOOK_URL"),
        "dingtalk" => env_var_present("DINGTALK_BOT_WEBHOOK_URL"),
        "wecom_bot" => env_var_present("WECOM_BOT_WEBHOOK_URL"),
        "wechat_official_account" => has_wechat_official_account_credentials(),
        "qq" => has_qq_bot_credentials(),
        "slack" => env_var_present("SLACK_BOT_WEBHOOK_URL"),
        "discord" => env_var_present("DISCORD_BOT_WEBHOOK_URL"),
        "mattermost" => env_var_present("MATTERMOST_BOT_WEBHOOK_URL"),
        "msteams" => env_var_present("MSTEAMS_BOT_WEBHOOK_URL"),
        "whatsapp" => {
            env_var_present("WHATSAPP_ACCESS_TOKEN") && env_var_present("WHATSAPP_PHONE_NUMBER_ID")
        }
        "line" => env_var_present("LINE_CHANNEL_ACCESS_TOKEN"),
        "matrix" => {
            env_var_present("MATRIX_ACCESS_TOKEN") && env_var_present("MATRIX_HOMESERVER_URL")
        }
        "google_chat" => env_var_present("GOOGLE_CHAT_BOT_WEBHOOK_URL"),
        "signal" => has_signal_account_configuration(),
        "bluebubbles" => has_bluebubbles_account_configuration(),
        _ => false,
    }
}

fn ingress_target_for_chat_platform(platform: &str) -> Option<&'static str> {
    match platform {
        "telegram" => Some("telegram"),
        "signal" => Some("signal"),
        "bluebubbles" => Some("bluebubbles"),
        "feishu" => Some("feishu"),
        "dingtalk" => Some("dingtalk"),
        "wecom" | "wecom_bot" => Some("wecom"),
        "wechat_official_account" => Some("wechat_official_account"),
        "qq" => Some("qq"),
        _ => None,
    }
}

fn is_ingress_platform_configured(platform: &str) -> bool {
    match platform {
        "telegram" => env_var_present("DAWN_TELEGRAM_WEBHOOK_SECRET"),
        "feishu" => true,
        "dingtalk" => env_var_present("DAWN_DINGTALK_CALLBACK_TOKEN"),
        "wecom" => env_var_present("DAWN_WECOM_CALLBACK_TOKEN"),
        "wechat_official_account" => env_var_present("DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN"),
        "qq" => env_var_present("DAWN_QQ_BOT_CALLBACK_SECRET"),
        "signal" => env_var_present("DAWN_SIGNAL_CALLBACK_SECRET"),
        "bluebubbles" => env_var_present("DAWN_BLUEBUBBLES_CALLBACK_SECRET"),
        _ => false,
    }
}

async fn save_setup_verification_receipt(
    state: &Arc<AppState>,
    receipt: &SetupVerificationReceiptRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO setup_verification_receipts (
            receipt_id,
            surface,
            target,
            label,
            region,
            integration_mode,
            status,
            summary,
            detail,
            action,
            endpoint,
            env_keys,
            missing_env_keys,
            is_default_path,
            verified_by,
            created_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        "#,
    )
    .bind(receipt.receipt_id.to_string())
    .bind(&receipt.surface)
    .bind(&receipt.target)
    .bind(&receipt.label)
    .bind(&receipt.region)
    .bind(&receipt.integration_mode)
    .bind(&receipt.status)
    .bind(&receipt.summary)
    .bind(&receipt.detail)
    .bind(receipt.action.as_deref())
    .bind(&receipt.endpoint)
    .bind(serde_json::to_string(&receipt.env_keys)?)
    .bind(serde_json::to_string(&receipt.missing_env_keys)?)
    .bind(if receipt.is_default_path {
        1_i64
    } else {
        0_i64
    })
    .bind(&receipt.verified_by)
    .bind(receipt.created_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .context("failed to save setup verification receipt")?;
    Ok(())
}

async fn list_setup_verification_receipts_inner(
    state: &Arc<AppState>,
    query: ListSetupVerificationReceiptsQuery,
) -> anyhow::Result<Vec<SetupVerificationReceiptRecord>> {
    let limit = i64::from(query.limit.unwrap_or(12).clamp(1, 50));
    let rows = sqlx::query_as::<_, SetupVerificationReceiptRow>(
        r#"
        SELECT
            receipt_id,
            surface,
            target,
            label,
            region,
            integration_mode,
            status,
            summary,
            detail,
            action,
            endpoint,
            env_keys,
            missing_env_keys,
            is_default_path,
            verified_by,
            created_at_unix_ms
        FROM setup_verification_receipts
        WHERE (?1 IS NULL OR surface = ?1)
          AND (?2 IS NULL OR target = ?2)
        ORDER BY created_at_unix_ms DESC, rowid DESC
        LIMIT ?3
        "#,
    )
    .bind(query.surface.map(|value| value.trim().to_ascii_lowercase()))
    .bind(query.target.map(|value| value.trim().to_ascii_lowercase()))
    .bind(limit)
    .fetch_all(state.pool())
    .await
    .context("failed to list setup verification receipts")?;
    rows.into_iter()
        .map(SetupVerificationReceiptRecord::try_from)
        .collect()
}

async fn count_pending_end_user_sessions(state: &Arc<AppState>) -> anyhow::Result<usize> {
    let now = unix_timestamp_ms() as i64;
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM end_user_approval_sessions
        WHERE status = ?1
          AND (expires_at_unix_ms IS NULL OR expires_at_unix_ms > ?2)
        "#,
    )
    .bind("pending")
    .bind(now)
    .fetch_one(state.pool())
    .await
    .context("failed to count pending end-user approval sessions")?;
    Ok(count.max(0) as usize)
}

pub(crate) async fn ensure_workspace_profile(
    state: &Arc<AppState>,
) -> anyhow::Result<WorkspaceProfileRecord> {
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

async fn get_workspace_profile(
    state: &Arc<AppState>,
) -> anyhow::Result<Option<WorkspaceProfileRecord>> {
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

pub(crate) async fn resolve_session_by_token(
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

pub(crate) async fn list_operator_session_records(
    state: &Arc<AppState>,
) -> anyhow::Result<Vec<OperatorSessionRecord>> {
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
    rows.into_iter()
        .map(OperatorSessionRecord::try_from)
        .collect()
}

pub(crate) async fn revoke_operator_session_by_id(
    state: &Arc<AppState>,
    session_id: Uuid,
    actor: &str,
    reason: &str,
) -> anyhow::Result<OperatorSessionRecord> {
    let now = unix_timestamp_ms();
    let updated = sqlx::query(
        r#"
        UPDATE operator_sessions
        SET revoked = 1, updated_at_unix_ms = ?2
        WHERE session_id = ?1
        "#,
    )
    .bind(session_id.to_string())
    .bind(now as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to revoke operator session {session_id}"))?;
    if updated.rows_affected() == 0 {
        anyhow::bail!("operator session not found");
    }
    let session = get_operator_session(state, session_id)
        .await?
        .ok_or_else(|| anyhow!("operator session disappeared after revoke"))?;
    state.emit_console_event(
        "identity",
        Some(session.session_id.to_string()),
        Some("session_revoked".to_string()),
        format!("operator session revoked by {actor} · {reason}"),
    );
    Ok(session)
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

async fn save_node_claim_record(
    state: &Arc<AppState>,
    claim: &NodeClaimRecord,
) -> anyhow::Result<()> {
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

async fn record_node_claim_audit_event(
    state: &Arc<AppState>,
    event: &NodeClaimAuditEventRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_claim_audit_events (
            claim_id,
            node_id,
            event_type,
            actor,
            detail,
            token_hint,
            session_url,
            created_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
    )
    .bind(event.claim_id.to_string())
    .bind(&event.node_id)
    .bind(&event.event_type)
    .bind(&event.actor)
    .bind(&event.detail)
    .bind(event.token_hint.as_deref())
    .bind(event.session_url.as_deref())
    .bind(event.created_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .context("failed to insert node claim audit event")?;
    Ok(())
}

async fn list_node_claim_records_inner(
    state: &Arc<AppState>,
) -> anyhow::Result<Vec<NodeClaimRecord>> {
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

async fn list_node_claim_audit_events_inner(
    state: &Arc<AppState>,
    query: ListNodeClaimAuditEventsQuery,
) -> anyhow::Result<Vec<NodeClaimAuditEventRecord>> {
    let limit = i64::from(query.limit.unwrap_or(12).clamp(1, 50));
    let rows = sqlx::query_as::<_, NodeClaimAuditEventRow>(
        r#"
        SELECT
            event_id,
            claim_id,
            node_id,
            event_type,
            actor,
            detail,
            token_hint,
            session_url,
            created_at_unix_ms
        FROM node_claim_audit_events
        WHERE (?1 IS NULL OR claim_id = ?1)
          AND (?2 IS NULL OR node_id = ?2)
        ORDER BY created_at_unix_ms DESC, event_id DESC
        LIMIT ?3
        "#,
    )
    .bind(query.claim_id.map(|value| value.trim().to_string()))
    .bind(query.node_id.map(|value| value.trim().to_string()))
    .bind(limit)
    .fetch_all(state.pool())
    .await
    .context("failed to list node claim audit events")?;
    rows.into_iter()
        .map(NodeClaimAuditEventRecord::try_from)
        .collect()
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

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
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

impl TryFrom<SetupVerificationReceiptRow> for SetupVerificationReceiptRecord {
    type Error = anyhow::Error;

    fn try_from(row: SetupVerificationReceiptRow) -> Result<Self, Self::Error> {
        Ok(Self {
            receipt_id: Uuid::parse_str(&row.receipt_id).with_context(|| {
                format!("invalid setup verification receipt id '{}'", row.receipt_id)
            })?,
            surface: row.surface,
            target: row.target,
            label: row.label,
            region: row.region,
            integration_mode: row.integration_mode,
            status: row.status,
            summary: row.summary,
            detail: row.detail,
            action: row.action,
            endpoint: row.endpoint,
            env_keys: serde_json::from_str(&row.env_keys)
                .context("failed to parse setup verification env_keys")?,
            missing_env_keys: serde_json::from_str(&row.missing_env_keys)
                .context("failed to parse setup verification missing_env_keys")?,
            is_default_path: row.is_default_path != 0,
            verified_by: row.verified_by,
            created_at_unix_ms: row.created_at_unix_ms as u128,
        })
    }
}

impl TryFrom<NodeClaimAuditEventRow> for NodeClaimAuditEventRecord {
    type Error = anyhow::Error;

    fn try_from(row: NodeClaimAuditEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: row.event_id,
            claim_id: Uuid::parse_str(&row.claim_id)
                .with_context(|| format!("invalid node claim audit claim id '{}'", row.claim_id))?,
            node_id: row.node_id,
            event_type: row.event_type,
            actor: row.actor,
            detail: row.detail,
            token_hint: row.token_hint,
            session_url: row.session_url,
            created_at_unix_ms: row.created_at_unix_ms as u128,
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
    use serde_json::{Value, json};
    use tower::util::ServiceExt;
    use uuid::Uuid;
    use wasmtime::Engine;

    use super::{
        AppState, BootstrapSessionRequest, IdentityEnvironmentReadiness, NodeClaimCreateRequest,
        WorkspaceProfileRecord, WorkspaceProfileUpdateRequest, authorize_node_session_open,
        build_identity_readiness, build_setup_verification_receipt, consume_node_session_claim,
        ensure_workspace_profile, router,
    };
    use crate::{
        app_state::{NodeRecord, NodeSessionStatus},
        sandbox,
    };

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

    fn test_workspace(onboarding_status: &str) -> WorkspaceProfileRecord {
        WorkspaceProfileRecord {
            workspace_id: "default".to_string(),
            tenant_id: "dawn-labs".to_string(),
            project_id: "agent-commerce".to_string(),
            display_name: "Dawn Agent Commerce".to_string(),
            region: "china".to_string(),
            default_model_providers: vec!["deepseek".to_string(), "qwen".to_string()],
            default_chat_platforms: vec!["wechat_official_account".to_string()],
            onboarding_status: onboarding_status.to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        }
    }

    fn test_workspace_with_targets(
        onboarding_status: &str,
        model_providers: Vec<String>,
        chat_platforms: Vec<String>,
    ) -> WorkspaceProfileRecord {
        WorkspaceProfileRecord {
            default_model_providers: model_providers,
            default_chat_platforms: chat_platforms,
            ..test_workspace(onboarding_status)
        }
    }

    fn trusted_node(node_id: &str) -> NodeRecord {
        NodeRecord {
            node_id: node_id.to_string(),
            display_name: format!("Node {node_id}"),
            transport: "websocket".to_string(),
            capabilities: vec!["agent_ping".to_string()],
            attestation_issuer_did: Some("did:dawn:test:issuer".to_string()),
            attestation_signature_hex: Some("abcd".to_string()),
            attestation_document_hash: Some("hash".to_string()),
            attestation_issued_at_unix_ms: Some(10),
            attestation_verified: true,
            attestation_verified_at_unix_ms: Some(11),
            attestation_error: None,
            status: NodeSessionStatus::Connected,
            connected: true,
            last_seen_unix_ms: 12,
            created_at_unix_ms: 9,
            updated_at_unix_ms: 12,
        }
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

    #[test]
    fn readiness_flags_missing_bootstrap_and_public_url() {
        let readiness = build_identity_readiness(
            &test_workspace("bootstrap_pending"),
            0,
            &[],
            &[],
            0,
            0,
            &IdentityEnvironmentReadiness {
                public_base_url: None,
                configured_model_providers: vec![],
                configured_chat_platforms: vec![],
                configured_ingress_platforms: vec![],
                present_env_keys: vec![],
            },
        );
        assert_eq!(readiness.overall_status, "action_required");
        assert_eq!(
            readiness.next_step.as_deref(),
            Some("Bootstrap an operator session from the control center.")
        );
        assert_eq!(readiness.checklist[0].key, "operator_session");
        assert_eq!(readiness.checklist[0].status, "action_required");
        assert!(
            readiness
                .checklist
                .iter()
                .any(|item| item.key == "public_gateway_url" && item.status == "action_required")
        );
    }

    #[test]
    fn readiness_marks_identity_path_ready_when_defaults_and_node_are_present() {
        let session = Uuid::new_v4();
        let claim = super::NodeClaimRecord {
            claim_id: Uuid::new_v4(),
            node_id: "node-cn-01".to_string(),
            display_name: "Shanghai Node".to_string(),
            transport: "websocket".to_string(),
            requested_capabilities: vec!["agent_ping".to_string()],
            issued_by_session_id: Some(session),
            issued_by_operator: "alice".to_string(),
            status: super::NodeClaimStatus::Consumed,
            expires_at_unix_ms: 99_999,
            consumed_at_unix_ms: Some(1_000),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        let readiness = build_identity_readiness(
            &test_workspace("identity_ready"),
            1,
            &[claim],
            &[trusted_node("node-cn-01")],
            1,
            1,
            &IdentityEnvironmentReadiness {
                public_base_url: Some("https://dawn.example.com".to_string()),
                configured_model_providers: vec!["deepseek".to_string(), "qwen".to_string()],
                configured_chat_platforms: vec!["wechat_official_account".to_string()],
                configured_ingress_platforms: vec!["wechat_official_account".to_string()],
                present_env_keys: vec![
                    "DEEPSEEK_API_KEY".to_string(),
                    "QWEN_API_KEY".to_string(),
                    "WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN".to_string(),
                    "DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN".to_string(),
                    "DAWN_PUBLIC_BASE_URL".to_string(),
                ],
            },
        );
        assert_eq!(readiness.overall_status, "ready");
        assert_eq!(readiness.completion_percent, 100);
        assert!(readiness.next_step.is_none());
        assert!(
            readiness
                .checklist
                .iter()
                .all(|item| item.status == "ready")
        );
    }

    #[test]
    fn readiness_marks_new_targets_ready_when_workspace_uses_them() {
        let session = Uuid::new_v4();
        let claim = super::NodeClaimRecord {
            claim_id: Uuid::new_v4(),
            node_id: "node-global-01".to_string(),
            display_name: "Global Node".to_string(),
            transport: "websocket".to_string(),
            requested_capabilities: vec!["agent_ping".to_string()],
            issued_by_session_id: Some(session),
            issued_by_operator: "alice".to_string(),
            status: super::NodeClaimStatus::Consumed,
            expires_at_unix_ms: 99_999,
            consumed_at_unix_ms: Some(1_000),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        let readiness = build_identity_readiness(
            &test_workspace_with_targets(
                "identity_ready",
                vec!["anthropic".to_string(), "google".to_string()],
                vec!["signal".to_string(), "bluebubbles".to_string()],
            ),
            1,
            &[claim],
            &[trusted_node("node-global-01")],
            0,
            0,
            &IdentityEnvironmentReadiness {
                public_base_url: Some("https://dawn.example.com".to_string()),
                configured_model_providers: vec!["anthropic".to_string(), "google".to_string()],
                configured_chat_platforms: vec!["signal".to_string(), "bluebubbles".to_string()],
                configured_ingress_platforms: vec!["signal".to_string(), "bluebubbles".to_string()],
                present_env_keys: vec![
                    "ANTHROPIC_API_KEY".to_string(),
                    "GEMINI_API_KEY".to_string(),
                    "SIGNAL_ACCOUNT".to_string(),
                    "SIGNAL_HTTP_URL".to_string(),
                    "DAWN_SIGNAL_CALLBACK_SECRET".to_string(),
                    "BLUEBUBBLES_SERVER_URL".to_string(),
                    "BLUEBUBBLES_PASSWORD".to_string(),
                    "DAWN_BLUEBUBBLES_CALLBACK_SECRET".to_string(),
                    "DAWN_PUBLIC_BASE_URL".to_string(),
                ],
            },
        );
        assert_eq!(readiness.overall_status, "ready");
        assert_eq!(readiness.completion_percent, 100);
        assert!(readiness.next_step.is_none());
        assert!(
            readiness
                .checklist
                .iter()
                .all(|item| item.status == "ready")
        );
    }

    #[test]
    fn readiness_uses_platform_specific_chat_guidance_for_feishu() {
        let readiness = build_identity_readiness(
            &test_workspace_with_targets(
                "identity_ready",
                vec!["openai".to_string()],
                vec!["feishu".to_string()],
            ),
            1,
            &[],
            &[],
            0,
            0,
            &IdentityEnvironmentReadiness {
                public_base_url: Some("https://dawn.example.com".to_string()),
                configured_model_providers: vec!["openai".to_string()],
                configured_chat_platforms: vec![],
                configured_ingress_platforms: vec!["feishu".to_string()],
                present_env_keys: vec!["OPENAI_API_KEY".to_string()],
            },
        );
        let chat_item = readiness
            .checklist
            .iter()
            .find(|item| item.key == "default_chat_connectors")
            .expect("chat readiness item");
        assert_eq!(chat_item.surface.as_deref(), Some("chat"));
        assert_eq!(chat_item.target.as_deref(), Some("feishu"));
        assert!(
            chat_item
                .action
                .as_deref()
                .unwrap_or_default()
                .contains("FEISHU_BOT_WEBHOOK_URL")
        );
    }

    #[test]
    fn readiness_uses_platform_specific_ingress_guidance_for_wechat() {
        let readiness = build_identity_readiness(
            &test_workspace_with_targets(
                "identity_ready",
                vec!["openai".to_string()],
                vec!["wechat_official_account".to_string()],
            ),
            1,
            &[],
            &[],
            0,
            0,
            &IdentityEnvironmentReadiness {
                public_base_url: Some("https://dawn.example.com".to_string()),
                configured_model_providers: vec!["openai".to_string()],
                configured_chat_platforms: vec!["wechat_official_account".to_string()],
                configured_ingress_platforms: vec![],
                present_env_keys: vec![
                    "OPENAI_API_KEY".to_string(),
                    "WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN".to_string(),
                ],
            },
        );
        let ingress_item = readiness
            .checklist
            .iter()
            .find(|item| item.key == "default_ingress_paths")
            .expect("ingress readiness item");
        assert_eq!(ingress_item.surface.as_deref(), Some("ingress"));
        assert_eq!(ingress_item.target.as_deref(), Some("wechat_official_account"));
        assert!(
            ingress_item
                .action
                .as_deref()
                .unwrap_or_default()
                .contains("DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN")
        );
    }

    #[test]
    fn setup_verification_receipt_marks_missing_credentials() {
        let receipt = build_setup_verification_receipt(
            &test_workspace("identity_ready"),
            "alice",
            "model",
            "qwen",
            &IdentityEnvironmentReadiness {
                public_base_url: Some("https://dawn.example.com".to_string()),
                configured_model_providers: vec![],
                configured_chat_platforms: vec![],
                configured_ingress_platforms: vec![],
                present_env_keys: vec![],
            },
        )
        .unwrap();
        assert_eq!(receipt.status, "action_required");
        assert!(
            receipt
                .missing_env_keys
                .iter()
                .any(|value| value == "QWEN_API_KEY")
        );
        assert!(receipt.action.is_some());
    }

    #[test]
    fn setup_verification_receipt_marks_new_model_targets_ready() {
        let receipt = build_setup_verification_receipt(
            &test_workspace_with_targets(
                "identity_ready",
                vec!["anthropic".to_string(), "google".to_string()],
                vec![],
            ),
            "alice",
            "model",
            "anthropic",
            &IdentityEnvironmentReadiness {
                public_base_url: Some("https://dawn.example.com".to_string()),
                configured_model_providers: vec!["anthropic".to_string(), "google".to_string()],
                configured_chat_platforms: vec![],
                configured_ingress_platforms: vec![],
                present_env_keys: vec![
                    "ANTHROPIC_API_KEY".to_string(),
                    "GEMINI_API_KEY".to_string(),
                ],
            },
        )
        .unwrap();
        assert_eq!(receipt.status, "ready");
        assert!(receipt.missing_env_keys.is_empty());
        assert!(receipt.action.is_none());
    }

    #[test]
    fn setup_verification_receipt_marks_new_chat_and_ingress_targets_ready() {
        let readiness = IdentityEnvironmentReadiness {
            public_base_url: Some("https://dawn.example.com".to_string()),
            configured_model_providers: vec![],
            configured_chat_platforms: vec!["signal".to_string(), "bluebubbles".to_string()],
            configured_ingress_platforms: vec!["signal".to_string(), "bluebubbles".to_string()],
            present_env_keys: vec![
                "SIGNAL_ACCOUNT".to_string(),
                "SIGNAL_HTTP_URL".to_string(),
                "DAWN_SIGNAL_CALLBACK_SECRET".to_string(),
                "BLUEBUBBLES_SERVER_URL".to_string(),
                "BLUEBUBBLES_PASSWORD".to_string(),
                "DAWN_BLUEBUBBLES_CALLBACK_SECRET".to_string(),
            ],
        };
        let chat_receipt = build_setup_verification_receipt(
            &test_workspace_with_targets(
                "identity_ready",
                vec![],
                vec!["signal".to_string(), "bluebubbles".to_string()],
            ),
            "alice",
            "chat",
            "signal",
            &readiness,
        )
        .unwrap();
        assert_eq!(chat_receipt.status, "ready");
        assert!(chat_receipt.action.is_none());

        let ingress_receipt = build_setup_verification_receipt(
            &test_workspace_with_targets(
                "identity_ready",
                vec![],
                vec!["signal".to_string(), "bluebubbles".to_string()],
            ),
            "alice",
            "ingress",
            "bluebubbles",
            &readiness,
        )
        .unwrap();
        assert_eq!(ingress_receipt.status, "ready");
        assert!(ingress_receipt.action.is_none());
    }

    #[tokio::test]
    async fn setup_verification_receipts_can_be_recorded_and_listed() -> anyhow::Result<()> {
        let (app, db_path) = test_app().await?;
        let bootstrap = json_request(
            app.clone(),
            "POST",
            "/identity/bootstrap/session",
            serde_json::to_value(BootstrapSessionRequest {
                bootstrap_token: "dawn-dev-bootstrap".to_string(),
                operator_name: "alice".to_string(),
            })?,
        )
        .await?;
        let bootstrap_payload = response_json(bootstrap).await?;
        let session_token = bootstrap_payload["sessionToken"]
            .as_str()
            .unwrap()
            .to_string();

        let create_response = json_request(
            app.clone(),
            "POST",
            "/identity/setup-verifications",
            json!({
                "sessionToken": session_token,
                "surface": "ingress",
                "target": "feishu"
            }),
        )
        .await?;
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_payload = response_json(create_response).await?;
        assert_eq!(create_payload["receipt"]["surface"], "ingress");
        assert_eq!(create_payload["receipt"]["target"], "feishu");
        assert_eq!(create_payload["receipt"]["status"], "ready");

        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/identity/setup-verifications?surface=ingress&target=feishu")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_payload = response_json(list_response).await?;
        assert_eq!(list_payload.as_array().map(Vec::len), Some(1));

        drop(app);
        let _ = fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn node_claims_can_be_reissued_and_audited() -> anyhow::Result<()> {
        let (app, db_path) = test_app().await?;
        let bootstrap = json_request(
            app.clone(),
            "POST",
            "/identity/bootstrap/session",
            serde_json::to_value(BootstrapSessionRequest {
                bootstrap_token: "dawn-dev-bootstrap".to_string(),
                operator_name: "alice".to_string(),
            })?,
        )
        .await?;
        let bootstrap_payload = response_json(bootstrap).await?;
        let session_token = bootstrap_payload["sessionToken"]
            .as_str()
            .unwrap()
            .to_string();

        let issue = json_request(
            app.clone(),
            "POST",
            "/identity/node-claims",
            json!({
                "sessionToken": session_token,
                "nodeId": "node-reissue-01",
                "displayName": "Node Reissue",
                "transport": "websocket",
                "requestedCapabilities": ["agent_ping"],
                "expiresInSeconds": 600
            }),
        )
        .await?;
        let issue_payload = response_json(issue).await?;
        let claim_id = issue_payload["claim"]["claimId"].as_str().unwrap();
        assert!(
            issue_payload["launchUrl"]
                .as_str()
                .unwrap()
                .contains("claimToken=")
        );
        assert!(issue_payload["tokenHint"].as_str().unwrap().len() >= 4);

        let reissue = json_request(
            app.clone(),
            "POST",
            &format!("/identity/node-claims/{claim_id}/reissue"),
            json!({
                "sessionToken": bootstrap_payload["sessionToken"],
                "expiresInSeconds": 900
            }),
        )
        .await?;
        assert_eq!(reissue.status(), StatusCode::OK);
        let reissue_payload = response_json(reissue).await?;
        assert_eq!(reissue_payload["reissuedFromClaimId"], claim_id);
        assert_ne!(
            reissue_payload["claim"]["claimId"].as_str().unwrap(),
            claim_id
        );

        let events_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/identity/node-claim-events?nodeId=node-reissue-01")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(events_response.status(), StatusCode::OK);
        let events_payload = response_json(events_response).await?;
        let event_types = events_payload
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.get("eventType").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(event_types.contains(&"reissued"));
        assert!(event_types.contains(&"reissued_old_revoked"));

        drop(app);
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
        assert!(
            claim_payload["sessionUrl"]
                .as_str()
                .unwrap()
                .contains("claimToken={claimToken}")
        );

        drop(app);
        let _ = fs::remove_file(db_path);
        Ok(())
    }

    #[tokio::test]
    async fn identity_status_exposes_readiness_summary() -> anyhow::Result<()> {
        let (app, db_path) = test_app().await?;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/identity/status")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await?;
        assert!(payload["readiness"]["checklist"].is_array());
        assert!(payload["readiness"]["completionPercent"].is_number());
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

        let preview =
            authorize_node_session_open(&state, "node-first", Some("claim-token")).await?;
        assert!(preview.is_some());

        let consumed =
            consume_node_session_claim(&state, "node-first", Some("claim-token")).await?;
        assert_eq!(
            consumed.as_ref().map(|record| record.status),
            Some(super::NodeClaimStatus::Consumed)
        );

        let second_attempt =
            authorize_node_session_open(&state, "node-first", Some("claim-token")).await;
        assert!(second_attempt.is_err());

        drop(state);
        let _ = fs::remove_file(db_path);
        Ok(())
    }
}
