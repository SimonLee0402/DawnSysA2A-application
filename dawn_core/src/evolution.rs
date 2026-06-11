use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};

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
use tokio::process::Command;
use tokio::time::{sleep, timeout};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::app_state::{
    AgentExperienceListFilter, AgentExperienceRecord, AppState, ChatIngressEventRecord,
    ChatIngressStatus, SkillImplementationExecutionListFilter, SkillImplementationExecutionRecord,
    SkillImplementationPatchListFilter, SkillImplementationPatchRecord,
    SkillImplementationPlanListFilter, SkillImplementationPlanRecord,
    SkillImplementationRunListFilter, SkillImplementationRunRecord, SkillProposalListFilter,
    SkillProposalRecord, unix_timestamp_ms,
};

const VERIFICATION_DEFAULT_TIMEOUT_SECS: u64 = 600;
const VERIFICATION_MIN_TIMEOUT_SECS: u64 = 10;
const VERIFICATION_MAX_TIMEOUT_SECS: u64 = 1_800;
const VERIFICATION_OUTPUT_CHAR_LIMIT: usize = 16_000;
const PATCH_MAX_FILE_CHANGES: usize = 20;
const PATCH_MAX_FILE_BYTES: usize = 500_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvolutionStatus {
    enabled: bool,
    storage: &'static str,
    experience_count: u64,
    skill_proposal_count: u64,
    skill_implementation_plan_count: u64,
    skill_implementation_run_count: u64,
    skill_implementation_execution_count: u64,
    skill_implementation_patch_count: u64,
    autonomous_code_mutation: bool,
    autonomous_publish: bool,
    review_required_for_skill_activation: bool,
    review_required_for_dangerous_actions: bool,
    auto_reflection_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExperienceListQuery {
    limit: Option<u32>,
    source: Option<String>,
    outcome: Option<String>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordExperienceRequest {
    source: Option<String>,
    scope: Option<String>,
    task_kind: Option<String>,
    input_summary: String,
    action_summary: String,
    outcome: String,
    lesson: String,
    reusable_hint: Option<String>,
    evidence: Option<Value>,
    tags: Option<Vec<String>>,
    risk_level: Option<String>,
    related_task_id: Option<Uuid>,
    related_ingress_id: Option<Uuid>,
    created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureIngressExperienceRequest {
    outcome: Option<String>,
    action_summary: Option<String>,
    lesson: String,
    reusable_hint: Option<String>,
    tags: Option<Vec<String>>,
    risk_level: Option<String>,
    created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReflectionRunRequest {
    limit: Option<u32>,
    dry_run: Option<bool>,
    created_by: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReflectionRunResponse {
    scanned: usize,
    created: usize,
    skipped_existing: usize,
    skipped_unsupported: usize,
    dry_run: bool,
    created_experience_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillProposalListQuery {
    limit: Option<u32>,
    status: Option<String>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillProposalRunRequest {
    limit: Option<u32>,
    min_evidence: Option<usize>,
    dry_run: Option<bool>,
    created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillProposalReviewRequest {
    status: String,
    reviewer: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationPlanListQuery {
    limit: Option<u32>,
    status: Option<String>,
    proposal_id: Option<Uuid>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateImplementationPlanRequest {
    created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationPlanReviewRequest {
    status: String,
    reviewer: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationRunListQuery {
    limit: Option<u32>,
    status: Option<String>,
    plan_id: Option<Uuid>,
    proposal_id: Option<Uuid>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateImplementationRunRequest {
    created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationRunReviewRequest {
    status: String,
    reviewer: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationExecutionListQuery {
    limit: Option<u32>,
    status: Option<String>,
    run_id: Option<Uuid>,
    plan_id: Option<Uuid>,
    proposal_id: Option<Uuid>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateImplementationExecutionRequest {
    created_by: Option<String>,
    executor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationExecutionReviewRequest {
    status: String,
    reviewer: Option<String>,
    note: Option<String>,
    result: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunImplementationExecutionVerificationRequest {
    requested_by: Option<String>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationPatchListQuery {
    limit: Option<u32>,
    status: Option<String>,
    execution_id: Option<Uuid>,
    run_id: Option<Uuid>,
    plan_id: Option<Uuid>,
    proposal_id: Option<Uuid>,
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateImplementationPatchRequest {
    created_by: Option<String>,
    summary: Option<String>,
    changed_files: Option<Value>,
    patch_manifest: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillImplementationPatchReviewRequest {
    status: String,
    reviewer: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyImplementationPatchRequest {
    requested_by: Option<String>,
    confirm_patch_id: Option<Uuid>,
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RollbackImplementationPatchRequest {
    requested_by: Option<String>,
    confirm_patch_id: Option<Uuid>,
    dry_run: Option<bool>,
}

#[derive(Debug, Clone)]
struct PatchFileChange {
    path: String,
    new_content: String,
    expected_old_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillProposalRunResponse {
    scanned_experiences: usize,
    candidate_patterns: usize,
    created: usize,
    skipped_existing: usize,
    dry_run: bool,
    created_proposal_ids: Vec<Uuid>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route(
            "/experiences",
            get(list_experiences).post(record_experience),
        )
        .route("/experiences/:experience_id", get(get_experience))
        .route(
            "/experiences/from-ingress/:ingress_id",
            post(capture_ingress_experience),
        )
        .route("/reflections/run", post(run_reflection))
        .route("/skill-proposals", get(list_skill_proposals))
        .route("/skill-proposals/run", post(run_skill_proposals))
        .route(
            "/skill-proposals/:proposal_id/review",
            post(review_skill_proposal),
        )
        .route(
            "/skill-proposals/:proposal_id/implementation-plan",
            post(create_implementation_plan_from_proposal),
        )
        .route("/skill-proposals/:proposal_id", get(get_skill_proposal))
        .route("/implementation-plans", get(list_implementation_plans))
        .route(
            "/implementation-plans/:plan_id/review",
            post(review_implementation_plan),
        )
        .route(
            "/implementation-plans/:plan_id/runs",
            post(create_implementation_run_from_plan),
        )
        .route(
            "/implementation-plans/:plan_id",
            get(get_implementation_plan),
        )
        .route("/implementation-runs", get(list_implementation_runs))
        .route(
            "/implementation-runs/:run_id/executions",
            post(create_implementation_execution_from_run),
        )
        .route(
            "/implementation-runs/:run_id/review",
            post(review_implementation_run),
        )
        .route("/implementation-runs/:run_id", get(get_implementation_run))
        .route(
            "/implementation-executions",
            get(list_implementation_executions),
        )
        .route(
            "/implementation-executions/:execution_id/review",
            post(review_implementation_execution),
        )
        .route(
            "/implementation-executions/:execution_id/verify",
            post(run_implementation_execution_verification),
        )
        .route(
            "/implementation-executions/:execution_id/patch-candidates",
            post(create_implementation_patch_from_execution),
        )
        .route(
            "/implementation-executions/:execution_id",
            get(get_implementation_execution),
        )
        .route(
            "/implementation-patch-candidates",
            get(list_implementation_patches),
        )
        .route(
            "/implementation-patch-candidates/:patch_id/review",
            post(review_implementation_patch),
        )
        .route(
            "/implementation-patch-candidates/:patch_id/apply",
            post(apply_implementation_patch),
        )
        .route(
            "/implementation-patch-candidates/:patch_id/rollback",
            post(rollback_implementation_patch),
        )
        .route(
            "/implementation-patch-candidates/:patch_id",
            get(get_implementation_patch),
        )
}

pub fn spawn_reflection_worker(state: Arc<AppState>) {
    if !auto_reflection_enabled() {
        info!("Dawn evolution reflection worker is disabled");
        return;
    }
    tokio::spawn(async move {
        let interval_secs = std::env::var("DAWN_EVOLUTION_REFLECTION_INTERVAL_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300)
            .max(30);
        sleep(Duration::from_secs(10)).await;
        loop {
            match reflect_recent_ingress_events(&state, 50, false, "auto-reflection").await {
                Ok(response) if response.created > 0 => {
                    info!(
                        created = response.created,
                        scanned = response.scanned,
                        "Dawn evolution reflection worker stored experiences"
                    );
                }
                Ok(_) => {}
                Err(error) => warn!(?error, "Dawn evolution reflection worker failed"),
            }
            sleep(Duration::from_secs(interval_secs)).await;
        }
    });
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<EvolutionStatus>, (StatusCode, Json<Value>)> {
    let experience_count = state
        .count_agent_experiences()
        .await
        .map_err(internal_error)?;
    let skill_proposal_count = state
        .count_skill_proposals()
        .await
        .map_err(internal_error)?;
    let skill_implementation_plan_count = state
        .count_skill_implementation_plans()
        .await
        .map_err(internal_error)?;
    let skill_implementation_run_count = state
        .count_skill_implementation_runs()
        .await
        .map_err(internal_error)?;
    let skill_implementation_execution_count = state
        .count_skill_implementation_executions()
        .await
        .map_err(internal_error)?;
    let skill_implementation_patch_count = state
        .count_skill_implementation_patches()
        .await
        .map_err(internal_error)?;
    Ok(Json(EvolutionStatus {
        enabled: true,
        storage: "sqlite:agent_experiences",
        experience_count,
        skill_proposal_count,
        skill_implementation_plan_count,
        skill_implementation_run_count,
        skill_implementation_execution_count,
        skill_implementation_patch_count,
        autonomous_code_mutation: false,
        autonomous_publish: false,
        review_required_for_skill_activation: true,
        review_required_for_dangerous_actions: true,
        auto_reflection_enabled: auto_reflection_enabled(),
    }))
}

async fn list_experiences(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExperienceListQuery>,
) -> Result<Json<Vec<AgentExperienceRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_agent_experiences(AgentExperienceListFilter {
            limit: query.limit,
            source: query.source,
            outcome: query.outcome,
            query: query.q,
        })
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_experience(
    State(state): State<Arc<AppState>>,
    Path(experience_id): Path<Uuid>,
) -> Result<Json<AgentExperienceRecord>, (StatusCode, Json<Value>)> {
    match state
        .get_agent_experience(experience_id)
        .await
        .map_err(internal_error)?
    {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent experience not found" })),
        )),
    }
}

async fn list_skill_proposals(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillProposalListQuery>,
) -> Result<Json<Vec<SkillProposalRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_skill_proposals(SkillProposalListFilter {
            limit: query.limit,
            status: query.status,
            query: query.q,
        })
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_skill_proposal(
    State(state): State<Arc<AppState>>,
    Path(proposal_id): Path<Uuid>,
) -> Result<Json<SkillProposalRecord>, (StatusCode, Json<Value>)> {
    match state
        .get_skill_proposal(proposal_id)
        .await
        .map_err(internal_error)?
    {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "skill proposal not found" })),
        )),
    }
}

async fn list_implementation_plans(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillImplementationPlanListQuery>,
) -> Result<Json<Vec<SkillImplementationPlanRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_skill_implementation_plans(SkillImplementationPlanListFilter {
            limit: query.limit,
            status: query.status,
            proposal_id: query.proposal_id,
            query: query.q,
        })
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_implementation_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<Uuid>,
) -> Result<Json<SkillImplementationPlanRecord>, (StatusCode, Json<Value>)> {
    match state
        .get_skill_implementation_plan(plan_id)
        .await
        .map_err(internal_error)?
    {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "skill implementation plan not found" })),
        )),
    }
}

async fn list_implementation_runs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillImplementationRunListQuery>,
) -> Result<Json<Vec<SkillImplementationRunRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_skill_implementation_runs(SkillImplementationRunListFilter {
            limit: query.limit,
            status: query.status,
            plan_id: query.plan_id,
            proposal_id: query.proposal_id,
            query: query.q,
        })
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_implementation_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<SkillImplementationRunRecord>, (StatusCode, Json<Value>)> {
    match state
        .get_skill_implementation_run(run_id)
        .await
        .map_err(internal_error)?
    {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "skill implementation run not found" })),
        )),
    }
}

async fn list_implementation_executions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillImplementationExecutionListQuery>,
) -> Result<Json<Vec<SkillImplementationExecutionRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_skill_implementation_executions(SkillImplementationExecutionListFilter {
            limit: query.limit,
            status: query.status,
            run_id: query.run_id,
            plan_id: query.plan_id,
            proposal_id: query.proposal_id,
            query: query.q,
        })
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_implementation_execution(
    State(state): State<Arc<AppState>>,
    Path(execution_id): Path<Uuid>,
) -> Result<Json<SkillImplementationExecutionRecord>, (StatusCode, Json<Value>)> {
    match state
        .get_skill_implementation_execution(execution_id)
        .await
        .map_err(internal_error)?
    {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "skill implementation execution not found" })),
        )),
    }
}

async fn list_implementation_patches(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillImplementationPatchListQuery>,
) -> Result<Json<Vec<SkillImplementationPatchRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_skill_implementation_patches(SkillImplementationPatchListFilter {
            limit: query.limit,
            status: query.status,
            execution_id: query.execution_id,
            run_id: query.run_id,
            plan_id: query.plan_id,
            proposal_id: query.proposal_id,
            query: query.q,
        })
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_implementation_patch(
    State(state): State<Arc<AppState>>,
    Path(patch_id): Path<Uuid>,
) -> Result<Json<SkillImplementationPatchRecord>, (StatusCode, Json<Value>)> {
    match state
        .get_skill_implementation_patch(patch_id)
        .await
        .map_err(internal_error)?
    {
        Some(record) => Ok(Json(record)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "skill implementation patch not found" })),
        )),
    }
}

async fn record_experience(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RecordExperienceRequest>,
) -> Result<Json<AgentExperienceRecord>, (StatusCode, Json<Value>)> {
    let record = build_manual_experience(request).map_err(bad_request)?;
    state
        .upsert_agent_experience(record)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn capture_ingress_experience(
    State(state): State<Arc<AppState>>,
    Path(ingress_id): Path<Uuid>,
    Json(request): Json<CaptureIngressExperienceRequest>,
) -> Result<Json<AgentExperienceRecord>, (StatusCode, Json<Value>)> {
    let event = state
        .get_chat_ingress_event(ingress_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "chat ingress event not found" })),
            )
        })?;
    let record = build_ingress_experience(event, request).map_err(bad_request)?;
    state
        .upsert_agent_experience(record)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn run_reflection(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ReflectionRunRequest>,
) -> Result<Json<ReflectionRunResponse>, (StatusCode, Json<Value>)> {
    reflect_recent_ingress_events(
        &state,
        request.limit.unwrap_or(50),
        request.dry_run.unwrap_or(false),
        request.created_by.as_deref().unwrap_or("manual-reflection"),
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn run_skill_proposals(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SkillProposalRunRequest>,
) -> Result<Json<SkillProposalRunResponse>, (StatusCode, Json<Value>)> {
    propose_skills_from_experiences(
        &state,
        request.limit.unwrap_or(200),
        request.min_evidence.unwrap_or(2),
        request.dry_run.unwrap_or(false),
        request
            .created_by
            .as_deref()
            .unwrap_or("skill-proposal-runner"),
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn review_skill_proposal(
    State(state): State<Arc<AppState>>,
    Path(proposal_id): Path<Uuid>,
    Json(request): Json<SkillProposalReviewRequest>,
) -> Result<Json<SkillProposalRecord>, (StatusCode, Json<Value>)> {
    let proposal = state
        .get_skill_proposal(proposal_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill proposal not found" })),
            )
        })?;
    let reviewed =
        apply_skill_proposal_review(proposal, request, unix_timestamp_ms()).map_err(bad_request)?;
    state
        .update_skill_proposal_review(reviewed)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn create_implementation_plan_from_proposal(
    State(state): State<Arc<AppState>>,
    Path(proposal_id): Path<Uuid>,
    Json(request): Json<CreateImplementationPlanRequest>,
) -> Result<Json<SkillImplementationPlanRecord>, (StatusCode, Json<Value>)> {
    if let Some(existing) = state
        .get_skill_implementation_plan_by_proposal(proposal_id)
        .await
        .map_err(internal_error)?
    {
        return Ok(Json(existing));
    }
    let proposal = state
        .get_skill_proposal(proposal_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill proposal not found" })),
            )
        })?;
    let plan = build_skill_implementation_plan(
        proposal,
        request
            .created_by
            .as_deref()
            .unwrap_or("implementation-planner"),
        unix_timestamp_ms(),
    )
    .map_err(bad_request)?;
    state
        .upsert_skill_implementation_plan(plan)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn review_implementation_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<Uuid>,
    Json(request): Json<SkillImplementationPlanReviewRequest>,
) -> Result<Json<SkillImplementationPlanRecord>, (StatusCode, Json<Value>)> {
    let plan = state
        .get_skill_implementation_plan(plan_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation plan not found" })),
            )
        })?;
    let reviewed = apply_skill_implementation_plan_review(plan, request, unix_timestamp_ms())
        .map_err(bad_request)?;
    state
        .update_skill_implementation_plan_review(reviewed)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn create_implementation_run_from_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<Uuid>,
    Json(request): Json<CreateImplementationRunRequest>,
) -> Result<Json<SkillImplementationRunRecord>, (StatusCode, Json<Value>)> {
    if let Some(existing) = state
        .get_skill_implementation_run_by_plan(plan_id)
        .await
        .map_err(internal_error)?
    {
        return Ok(Json(existing));
    }
    let plan = state
        .get_skill_implementation_plan(plan_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation plan not found" })),
            )
        })?;
    let run = build_skill_implementation_run(
        plan,
        request
            .created_by
            .as_deref()
            .unwrap_or("implementation-runner"),
        unix_timestamp_ms(),
    )
    .map_err(bad_request)?;
    state
        .upsert_skill_implementation_run(run)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn review_implementation_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<Uuid>,
    Json(request): Json<SkillImplementationRunReviewRequest>,
) -> Result<Json<SkillImplementationRunRecord>, (StatusCode, Json<Value>)> {
    let run = state
        .get_skill_implementation_run(run_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation run not found" })),
            )
        })?;
    let reviewed = apply_skill_implementation_run_review(run, request, unix_timestamp_ms())
        .map_err(bad_request)?;
    state
        .update_skill_implementation_run_review(reviewed)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn create_implementation_execution_from_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<Uuid>,
    Json(request): Json<CreateImplementationExecutionRequest>,
) -> Result<Json<SkillImplementationExecutionRecord>, (StatusCode, Json<Value>)> {
    let run = state
        .get_skill_implementation_run(run_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation run not found" })),
            )
        })?;
    let execution = build_skill_implementation_execution(
        run,
        request
            .created_by
            .as_deref()
            .unwrap_or("implementation-execution-planner"),
        request
            .executor
            .as_deref()
            .unwrap_or("operator_or_guarded_agent"),
        unix_timestamp_ms(),
    )
    .map_err(bad_request)?;
    state
        .upsert_skill_implementation_execution(execution)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn review_implementation_execution(
    State(state): State<Arc<AppState>>,
    Path(execution_id): Path<Uuid>,
    Json(request): Json<SkillImplementationExecutionReviewRequest>,
) -> Result<Json<SkillImplementationExecutionRecord>, (StatusCode, Json<Value>)> {
    let execution = state
        .get_skill_implementation_execution(execution_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation execution not found" })),
            )
        })?;
    let reviewed =
        apply_skill_implementation_execution_review(execution, request, unix_timestamp_ms())
            .map_err(bad_request)?;
    state
        .update_skill_implementation_execution_review(reviewed)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn run_implementation_execution_verification(
    State(state): State<Arc<AppState>>,
    Path(execution_id): Path<Uuid>,
    Json(request): Json<RunImplementationExecutionVerificationRequest>,
) -> Result<Json<SkillImplementationExecutionRecord>, (StatusCode, Json<Value>)> {
    let execution = state
        .get_skill_implementation_execution(execution_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation execution not found" })),
            )
        })?;
    let verified = run_skill_implementation_execution_verification(
        execution,
        request,
        verification_workspace_root(),
    )
    .await
    .map_err(bad_request)?;
    state
        .update_skill_implementation_execution_review(verified)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn create_implementation_patch_from_execution(
    State(state): State<Arc<AppState>>,
    Path(execution_id): Path<Uuid>,
    Json(request): Json<CreateImplementationPatchRequest>,
) -> Result<Json<SkillImplementationPatchRecord>, (StatusCode, Json<Value>)> {
    let execution = state
        .get_skill_implementation_execution(execution_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation execution not found" })),
            )
        })?;
    let patch = build_skill_implementation_patch(execution, request, unix_timestamp_ms())
        .map_err(bad_request)?;
    state
        .upsert_skill_implementation_patch(patch)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn review_implementation_patch(
    State(state): State<Arc<AppState>>,
    Path(patch_id): Path<Uuid>,
    Json(request): Json<SkillImplementationPatchReviewRequest>,
) -> Result<Json<SkillImplementationPatchRecord>, (StatusCode, Json<Value>)> {
    let patch = state
        .get_skill_implementation_patch(patch_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation patch not found" })),
            )
        })?;
    let reviewed = apply_skill_implementation_patch_review(patch, request, unix_timestamp_ms())
        .map_err(bad_request)?;
    state
        .update_skill_implementation_patch_review(reviewed)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn apply_implementation_patch(
    State(state): State<Arc<AppState>>,
    Path(patch_id): Path<Uuid>,
    Json(request): Json<ApplyImplementationPatchRequest>,
) -> Result<Json<SkillImplementationPatchRecord>, (StatusCode, Json<Value>)> {
    let patch = state
        .get_skill_implementation_patch(patch_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation patch not found" })),
            )
        })?;
    let applied =
        apply_skill_implementation_patch_candidate(patch, request, verification_workspace_root())
            .await
            .map_err(bad_request)?;
    state
        .update_skill_implementation_patch_runtime(applied)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn rollback_implementation_patch(
    State(state): State<Arc<AppState>>,
    Path(patch_id): Path<Uuid>,
    Json(request): Json<RollbackImplementationPatchRequest>,
) -> Result<Json<SkillImplementationPatchRecord>, (StatusCode, Json<Value>)> {
    let patch = state
        .get_skill_implementation_patch(patch_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "skill implementation patch not found" })),
            )
        })?;
    let rolled_back = rollback_skill_implementation_patch_candidate(
        patch,
        request,
        verification_workspace_root(),
    )
    .await
    .map_err(bad_request)?;
    state
        .update_skill_implementation_patch_runtime(rolled_back)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn reflect_recent_ingress_events(
    state: &Arc<AppState>,
    limit: u32,
    dry_run: bool,
    created_by: &str,
) -> anyhow::Result<ReflectionRunResponse> {
    let events = state
        .list_chat_ingress_events(Some(limit.clamp(1, 200)))
        .await?;
    let mut response = ReflectionRunResponse {
        scanned: events.len(),
        created: 0,
        skipped_existing: 0,
        skipped_unsupported: 0,
        dry_run,
        created_experience_ids: Vec::new(),
    };
    for event in events {
        if !should_reflect_ingress_event(event.status) {
            response.skipped_unsupported += 1;
            continue;
        }
        if state
            .has_agent_experience_for_ingress(event.ingress_id)
            .await?
        {
            response.skipped_existing += 1;
            continue;
        }
        let experience = build_auto_ingress_experience(event, created_by)?;
        response.created += 1;
        response
            .created_experience_ids
            .push(experience.experience_id);
        if !dry_run {
            state.upsert_agent_experience(experience).await?;
        }
    }
    Ok(response)
}

async fn propose_skills_from_experiences(
    state: &Arc<AppState>,
    limit: u32,
    min_evidence: usize,
    dry_run: bool,
    created_by: &str,
) -> anyhow::Result<SkillProposalRunResponse> {
    let experiences = state
        .list_agent_experiences(AgentExperienceListFilter {
            limit: Some(limit.clamp(1, 200)),
            ..Default::default()
        })
        .await?;
    let candidates =
        build_skill_proposal_candidates(&experiences, min_evidence.max(2), created_by)?;
    let mut response = SkillProposalRunResponse {
        scanned_experiences: experiences.len(),
        candidate_patterns: candidates.len(),
        created: 0,
        skipped_existing: 0,
        dry_run,
        created_proposal_ids: Vec::new(),
    };
    for candidate in candidates {
        if state
            .get_skill_proposal_by_key(&candidate.proposal_key)
            .await?
            .is_some()
        {
            response.skipped_existing += 1;
            continue;
        }
        response.created += 1;
        response.created_proposal_ids.push(candidate.proposal_id);
        if !dry_run {
            state.upsert_skill_proposal(candidate).await?;
        }
    }
    Ok(response)
}

fn build_skill_proposal_candidates(
    experiences: &[AgentExperienceRecord],
    min_evidence: usize,
    created_by: &str,
) -> anyhow::Result<Vec<SkillProposalRecord>> {
    let mut by_kind: HashMap<String, Vec<&AgentExperienceRecord>> = HashMap::new();
    for experience in experiences {
        let task_kind = experience.task_kind.trim();
        if !is_skill_proposal_task_kind(task_kind) {
            continue;
        }
        by_kind
            .entry(task_kind.to_string())
            .or_default()
            .push(experience);
    }

    let mut proposals = Vec::new();
    let now = unix_timestamp_ms();
    for (task_kind, mut group) in by_kind {
        if group.len() < min_evidence {
            continue;
        }
        group.sort_by(|left, right| right.updated_at_unix_ms.cmp(&left.updated_at_unix_ms));
        let evidence_group = group.into_iter().take(5).collect::<Vec<_>>();
        let suggested_skill_id =
            format!("dawn.{}-assistant", slugify_skill_id_component(&task_kind));
        let proposal_key = format!("experience-task-kind:{task_kind}");
        let lessons = evidence_group
            .iter()
            .map(|experience| truncate_summary(&experience.lesson, 240))
            .collect::<Vec<_>>();
        let experience_ids = evidence_group
            .iter()
            .map(|experience| experience.experience_id)
            .collect::<Vec<_>>();
        let tags = proposal_tags_for_experiences(&evidence_group);
        let risk_level = proposal_risk_for_experiences(&evidence_group).to_string();
        let count = evidence_group.len();
        proposals.push(SkillProposalRecord {
            proposal_id: Uuid::new_v4(),
            proposal_key,
            title: format!("Propose reviewed skill for {task_kind}"),
            summary: format!(
                "{count} recent experiences share taskKind `{task_kind}`; this may deserve a reviewed reusable skill."
            ),
            rationale: "Repeated experiences indicate a stable workflow candidate. This proposal is advisory only and requires human review before any skill is implemented or activated.".to_string(),
            suggested_skill_id,
            source: "experience-pattern".to_string(),
            status: "proposed".to_string(),
            confidence: proposal_confidence(count),
            evidence: json!({
                "pattern": "taskKind",
                "taskKind": task_kind,
                "experienceCount": count,
                "experienceIds": experience_ids,
                "lessons": lessons,
            }),
            tags,
            risk_level,
            created_by: optional_label(Some(created_by.to_string()), "skill-proposal-runner")?,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        });
    }
    proposals.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(proposals)
}

fn is_skill_proposal_task_kind(task_kind: &str) -> bool {
    let normalized = task_kind.trim();
    !normalized.is_empty()
        && !matches!(
            normalized,
            "conversation" | "unknown" | "task_routing" | "model_reply"
        )
}

fn slugify_skill_id_component(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '_' || ch == '-' || ch.is_whitespace() {
            Some('-')
        } else {
            None
        };
        let Some(next) = next else {
            continue;
        };
        if next == '-' {
            if slug.is_empty() || last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        slug.push(next);
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "workflow".to_string()
    } else {
        slug
    }
}

fn proposal_tags_for_experiences(experiences: &[&AgentExperienceRecord]) -> Vec<String> {
    let mut tags = vec!["skill-proposal".to_string()];
    for experience in experiences {
        for tag in &experience.tags {
            let tag = tag.trim().to_ascii_lowercase();
            if tag.is_empty() || tags.iter().any(|value| value == &tag) {
                continue;
            }
            tags.push(tag);
            if tags.len() >= 12 {
                return tags;
            }
        }
    }
    tags
}

fn proposal_risk_for_experiences(experiences: &[&AgentExperienceRecord]) -> &'static str {
    if experiences
        .iter()
        .any(|experience| matches!(experience.risk_level.as_str(), "high" | "blocked"))
    {
        "high"
    } else if experiences
        .iter()
        .any(|experience| experience.risk_level == "guarded")
    {
        "guarded"
    } else {
        "low"
    }
}

fn proposal_confidence(evidence_count: usize) -> f64 {
    match evidence_count {
        0 | 1 => 0.0,
        2 => 0.62,
        3 => 0.72,
        4 => 0.8,
        _ => 0.86,
    }
}

fn apply_skill_proposal_review(
    mut proposal: SkillProposalRecord,
    request: SkillProposalReviewRequest,
    now: u128,
) -> anyhow::Result<SkillProposalRecord> {
    let status = normalize_skill_proposal_review_status(&request.status)?;
    let reviewer = optional_label(request.reviewer, "operator")?;
    let note = optional_summary(request.note);
    proposal.status = status.to_string();
    proposal.evidence = append_skill_proposal_review_event(
        proposal.evidence,
        status,
        &reviewer,
        note.as_deref(),
        now,
    );
    proposal.updated_at_unix_ms = now;
    Ok(proposal)
}

fn normalize_skill_proposal_review_status(status: &str) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "proposed" | "reopen" | "reopened" => Ok("proposed"),
        "approved" | "approve" => Ok("approved"),
        "rejected" | "reject" => Ok("rejected"),
        "deferred" | "defer" | "needs_more_evidence" => Ok("deferred"),
        _ => Err(anyhow!(
            "status must be one of proposed, approved, rejected, or deferred"
        )),
    }
}

fn append_skill_proposal_review_event(
    evidence: Value,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
    now: u128,
) -> Value {
    let mut evidence = match evidence {
        Value::Object(_) => evidence,
        other => json!({ "previousEvidence": other }),
    };
    let review = json!({
        "status": status,
        "reviewer": reviewer,
        "note": note.map(|value| truncate_summary(value, 1000)),
        "reviewedAtUnixMs": now,
        "activation": "not_activated",
    });
    let Some(object) = evidence.as_object_mut() else {
        return json!({ "reviewTrail": [review] });
    };
    let trail = object
        .entry("reviewTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(review);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![review]);
        }
    }
    evidence
}

fn build_skill_implementation_plan(
    proposal: SkillProposalRecord,
    created_by: &str,
    now: u128,
) -> anyhow::Result<SkillImplementationPlanRecord> {
    if proposal.status != "approved" {
        return Err(anyhow!(
            "skill proposal must be approved before an implementation plan can be created"
        ));
    }
    let created_by = optional_label(Some(created_by.to_string()), "implementation-planner")?;
    let skill_id = proposal.suggested_skill_id.clone();
    let risk_level = proposal.risk_level.clone();
    Ok(SkillImplementationPlanRecord {
        plan_id: Uuid::new_v4(),
        proposal_id: proposal.proposal_id,
        suggested_skill_id: skill_id.clone(),
        title: format!("Draft implementation plan for {skill_id}"),
        summary: format!(
            "Review-only implementation plan for approved proposal `{}`. This plan does not generate code, register a skill, or change permissions.",
            proposal.proposal_key
        ),
        status: "draft".to_string(),
        steps: json!([
            {
                "order": 1,
                "name": "Confirm the workflow boundary",
                "detail": "Restate the user-visible workflow, supported chat platforms, expected inputs, and explicit non-goals before implementation."
            },
            {
                "order": 2,
                "name": "Reuse existing runtime surfaces",
                "detail": "Inspect existing chat ingress, skill registry, approval center, and control-plane APIs; prefer native builtins or existing connectors over new subsystems."
            },
            {
                "order": 3,
                "name": "Design guarded execution",
                "detail": "Keep dangerous desktop, filesystem, credential, payment, and network actions behind the existing approval gates and attested node-command path."
            },
            {
                "order": 4,
                "name": "Implement behind review gates",
                "detail": "Make the smallest scoped code change required for the approved workflow; do not auto-activate or publish the skill as part of this plan."
            },
            {
                "order": 5,
                "name": "Verify regressions",
                "detail": "Run targeted unit tests, non-QGIS cargo tests, and at least one runtime smoke test that proves existing chat ingress still works."
            }
        ]),
        acceptance_criteria: json!([
            "Existing QQ, Telegram, WeChat, and other chat ingress routes continue to create tasks or replies according to their current mode.",
            "The implementation has tests covering routing, approval behavior, and failure messages.",
            "No skill is activated, published, or granted new permissions without a separate explicit review step.",
            "Runtime smoke evidence shows the gateway status is healthy after the change.",
            "The operator can identify rollback scope from the changed files and generated audit events."
        ]),
        guardrails: json!({
            "autonomousCodeMutation": false,
            "autonomousPublish": false,
            "requiresHumanReviewBeforeActivation": true,
            "requiresExistingApprovalGates": true,
            "riskLevel": risk_level,
            "sourceProposalId": proposal.proposal_id,
            "sourceProposalStatus": proposal.status,
            "sourceSuggestedSkillId": proposal.suggested_skill_id,
            "forbiddenWithoutSeparateApproval": [
                "skill activation",
                "release publication",
                "credential changes",
                "destructive filesystem operations",
                "payment authorization",
                "bypassing chat pairing or approval gates"
            ]
        }),
        created_by,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn apply_skill_implementation_plan_review(
    mut plan: SkillImplementationPlanRecord,
    request: SkillImplementationPlanReviewRequest,
    now: u128,
) -> anyhow::Result<SkillImplementationPlanRecord> {
    let status = normalize_skill_implementation_plan_review_status(&request.status)?;
    let reviewer = optional_label(request.reviewer, "operator")?;
    let note = optional_summary(request.note);
    plan.status = status.to_string();
    plan.guardrails = append_skill_implementation_plan_review_event(
        plan.guardrails,
        status,
        &reviewer,
        note.as_deref(),
        now,
    );
    plan.updated_at_unix_ms = now;
    Ok(plan)
}

fn normalize_skill_implementation_plan_review_status(status: &str) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "draft" | "reopen" | "reopened" => Ok("draft"),
        "approved" | "approve" => Ok("approved"),
        "rejected" | "reject" => Ok("rejected"),
        "deferred" | "defer" | "needs_more_evidence" => Ok("deferred"),
        _ => Err(anyhow!(
            "status must be one of draft, approved, rejected, or deferred"
        )),
    }
}

fn append_skill_implementation_plan_review_event(
    guardrails: Value,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
    now: u128,
) -> Value {
    let mut guardrails = match guardrails {
        Value::Object(_) => guardrails,
        other => json!({ "previousGuardrails": other }),
    };
    let review = json!({
        "status": status,
        "reviewer": reviewer,
        "note": note.map(|value| truncate_summary(value, 1000)),
        "reviewedAtUnixMs": now,
        "execution": "not_started",
        "activation": "not_activated",
    });
    let Some(object) = guardrails.as_object_mut() else {
        return json!({ "reviewTrail": [review] });
    };
    let trail = object
        .entry("reviewTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(review);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![review]);
        }
    }
    guardrails
}

fn build_skill_implementation_run(
    plan: SkillImplementationPlanRecord,
    created_by: &str,
    now: u128,
) -> anyhow::Result<SkillImplementationRunRecord> {
    if plan.status != "approved" {
        return Err(anyhow!(
            "skill implementation plan must be approved before a run package can be created"
        ));
    }
    let created_by = optional_label(Some(created_by.to_string()), "implementation-runner")?;
    let suggested_skill_id = plan.suggested_skill_id.clone();
    Ok(SkillImplementationRunRecord {
        run_id: Uuid::new_v4(),
        plan_id: plan.plan_id,
        proposal_id: plan.proposal_id,
        suggested_skill_id: suggested_skill_id.clone(),
        status: "prepared".to_string(),
        execution_mode: "guarded_manual_or_future_agent".to_string(),
        change_package: json!({
            "kind": "implementation_change_package",
            "sourcePlanId": plan.plan_id,
            "sourceProposalId": plan.proposal_id,
            "suggestedSkillId": suggested_skill_id,
            "objective": plan.summary,
            "allowedTargetAreas": [
                "dawn_core/src",
                "workflow/native_skills",
                "README.md",
                "docs"
            ],
            "protectedAreas": [
                "user-created files outside the task scope",
                "QGIS files unless explicitly approved",
                "credentials and environment files",
                "Telegram, QQ, WeChat connector secrets"
            ],
            "implementationSteps": plan.steps,
            "acceptanceCriteria": plan.acceptance_criteria,
            "workspacePolicy": "inspect current diff first; do not revert unrelated user changes"
        }),
        verification: json!({
            "requiredCommands": [
                "cargo check --manifest-path dawn_core/Cargo.toml",
                "cargo test --manifest-path dawn_core/Cargo.toml -- --skip qgis_generates_real_contours_and_exports_real_map"
            ],
            "runtimeSmoke": [
                "GET /api/gateway/evolution/status",
                "GET /api/gateway/ingress/status",
                "one ordinary chat route and one task-creation route remain healthy when the gateway is running"
            ],
            "evidenceRequiredBeforeActivation": true
        }),
        rollback: json!({
            "strategy": "prepare a reviewable patch boundary before applying implementation changes",
            "requiredNotes": [
                "list changed files",
                "list generated runtime records",
                "record commands used for verification",
                "do not use destructive git reset against user changes"
            ],
            "manualRollbackOnly": true
        }),
        guardrails: json!({
            "autonomousCodeMutation": false,
            "autonomousPublish": false,
            "execution": "not_executed",
            "activation": "not_activated",
            "requiresSeparateExecutionApproval": true,
            "requiresSeparateSkillActivationApproval": true,
            "sourcePlanStatus": plan.status,
            "sourcePlanGuardrails": plan.guardrails,
            "forbiddenWithoutSeparateApproval": [
                "modifying files",
                "running generated code",
                "skill activation",
                "release publication",
                "credential changes",
                "destructive filesystem operations",
                "payment authorization",
                "bypassing chat pairing or approval gates"
            ]
        }),
        created_by,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn apply_skill_implementation_run_review(
    mut run: SkillImplementationRunRecord,
    request: SkillImplementationRunReviewRequest,
    now: u128,
) -> anyhow::Result<SkillImplementationRunRecord> {
    let status = normalize_skill_implementation_run_review_status(&request.status)?;
    let reviewer = optional_label(request.reviewer, "operator")?;
    let note = optional_summary(request.note);
    run.status = status.to_string();
    run.guardrails = append_skill_implementation_run_review_event(
        run.guardrails,
        status,
        &reviewer,
        note.as_deref(),
        now,
    );
    run.updated_at_unix_ms = now;
    Ok(run)
}

fn normalize_skill_implementation_run_review_status(status: &str) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "prepared" | "reopen" | "reopened" => Ok("prepared"),
        "approved_for_execution" | "approve" | "approved" => Ok("approved_for_execution"),
        "rejected" | "reject" => Ok("rejected"),
        "deferred" | "defer" | "needs_more_evidence" => Ok("deferred"),
        _ => Err(anyhow!(
            "status must be one of prepared, approved_for_execution, rejected, or deferred"
        )),
    }
}

fn append_skill_implementation_run_review_event(
    guardrails: Value,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
    now: u128,
) -> Value {
    let mut guardrails = match guardrails {
        Value::Object(_) => guardrails,
        other => json!({ "previousGuardrails": other }),
    };
    let review = json!({
        "status": status,
        "reviewer": reviewer,
        "note": note.map(|value| truncate_summary(value, 1000)),
        "reviewedAtUnixMs": now,
        "execution": "not_executed",
        "activation": "not_activated",
    });
    let Some(object) = guardrails.as_object_mut() else {
        return json!({ "reviewTrail": [review] });
    };
    let trail = object
        .entry("reviewTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(review);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![review]);
        }
    }
    guardrails
}

fn build_skill_implementation_execution(
    run: SkillImplementationRunRecord,
    created_by: &str,
    executor: &str,
    now: u128,
) -> anyhow::Result<SkillImplementationExecutionRecord> {
    if run.status != "approved_for_execution" {
        return Err(anyhow!(
            "skill implementation run must be approved_for_execution before an execution record can be created"
        ));
    }
    let created_by = optional_label(
        Some(created_by.to_string()),
        "implementation-execution-planner",
    )?;
    let executor = optional_label(Some(executor.to_string()), "operator_or_guarded_agent")?;
    let required_commands = match run.verification.get("requiredCommands") {
        Some(Value::Array(items)) if !items.is_empty() => Value::Array(items.clone()),
        _ => {
            return Err(anyhow!(
                "skill implementation run verification.requiredCommands must be a non-empty array"
            ));
        }
    };
    let allowed_target_areas = run
        .change_package
        .get("allowedTargetAreas")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let protected_areas = run
        .change_package
        .get("protectedAreas")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));

    Ok(SkillImplementationExecutionRecord {
        execution_id: Uuid::new_v4(),
        run_id: run.run_id,
        plan_id: run.plan_id,
        proposal_id: run.proposal_id,
        suggested_skill_id: run.suggested_skill_id,
        status: "ready_for_execution".to_string(),
        executor,
        preflight_report: json!({
            "status": "passed",
            "checkedAtUnixMs": now,
            "sourceRunStatus": run.status,
            "apiExecutedCommands": false,
            "checks": [
                {
                    "name": "run approval",
                    "passed": true,
                    "evidence": "source run status is approved_for_execution"
                },
                {
                    "name": "required verification commands",
                    "passed": true,
                    "evidence": required_commands
                },
                {
                    "name": "execution boundary",
                    "passed": true,
                    "evidence": "this API creates an audit record and does not run shell commands"
                }
            ]
        }),
        command_plan: json!({
            "requiredCommands": required_commands,
            "allowedTargetAreas": allowed_target_areas,
            "protectedAreas": protected_areas,
            "executionPolicy": "gateway may run only allowlisted verification commands through the /verify endpoint; implementation mutation remains outside this API",
            "requiresSeparateOperatorAction": true,
            "apiExecutedCommands": false
        }),
        result: json!({
            "execution": "not_started",
            "activation": "not_activated",
            "apiExecutedCommands": false,
            "artifacts": []
        }),
        guardrails: json!({
            "autonomousCodeMutation": false,
            "autonomousPublish": false,
            "execution": "not_started",
            "activation": "not_activated",
            "apiExecutedCommands": false,
            "requiresSeparateOperatorAction": true,
            "requiresSeparateSkillActivationApproval": true,
            "sourceRunGuardrails": run.guardrails,
            "forbiddenWithoutSeparateApproval": [
                "running shell commands",
                "modifying files",
                "activating skills",
                "publishing releases",
                "credential changes",
                "destructive filesystem operations",
                "bypassing chat pairing or approval gates"
            ]
        }),
        created_by,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn apply_skill_implementation_execution_review(
    mut execution: SkillImplementationExecutionRecord,
    request: SkillImplementationExecutionReviewRequest,
    now: u128,
) -> anyhow::Result<SkillImplementationExecutionRecord> {
    let status = normalize_skill_implementation_execution_review_status(&request.status)?;
    let reviewer = optional_label(request.reviewer, "operator")?;
    let note = optional_summary(request.note);
    let reported_result = request.result.unwrap_or_else(|| json!({}));
    execution.status = status.to_string();
    execution.result = append_skill_implementation_execution_result(
        execution.result,
        status,
        reported_result,
        now,
    );
    execution.guardrails = append_skill_implementation_execution_review_event(
        execution.guardrails,
        status,
        &reviewer,
        note.as_deref(),
        now,
    );
    execution.updated_at_unix_ms = now;
    Ok(execution)
}

fn normalize_skill_implementation_execution_review_status(
    status: &str,
) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "ready_for_execution" | "ready" | "reopen" | "reopened" => Ok("ready_for_execution"),
        "approved_for_manual_execution" | "approve" | "approved" => {
            Ok("approved_for_manual_execution")
        }
        "completed_manual" | "completed" | "manual_complete" => Ok("completed_manual"),
        "failed_manual" | "failed" | "manual_failed" => Ok("failed_manual"),
        "rejected" | "reject" => Ok("rejected"),
        "deferred" | "defer" | "needs_more_evidence" => Ok("deferred"),
        _ => Err(anyhow!(
            "status must be one of ready_for_execution, approved_for_manual_execution, completed_manual, failed_manual, rejected, or deferred"
        )),
    }
}

fn append_skill_implementation_execution_result(
    result: Value,
    status: &str,
    reported_result: Value,
    now: u128,
) -> Value {
    let mut result = match result {
        Value::Object(_) => result,
        other => json!({ "previousResult": other }),
    };
    let event = json!({
        "status": status,
        "reportedAtUnixMs": now,
        "reportedResult": reported_result,
        "apiExecutedCommands": false,
        "activation": "not_activated",
    });
    let Some(object) = result.as_object_mut() else {
        return json!({ "reviewedResults": [event] });
    };
    object.insert("apiExecutedCommands".to_string(), Value::Bool(false));
    object.insert(
        "operatorReportedStatus".to_string(),
        Value::String(status.to_string()),
    );
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    let trail = object
        .entry("reviewedResults".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(event);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![event]);
        }
    }
    result
}

fn append_skill_implementation_execution_review_event(
    guardrails: Value,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
    now: u128,
) -> Value {
    let mut guardrails = match guardrails {
        Value::Object(_) => guardrails,
        other => json!({ "previousGuardrails": other }),
    };
    let review = json!({
        "status": status,
        "reviewer": reviewer,
        "note": note.map(|value| truncate_summary(value, 1000)),
        "reviewedAtUnixMs": now,
        "apiExecutedCommands": false,
        "activation": "not_activated",
    });
    let Some(object) = guardrails.as_object_mut() else {
        return json!({ "reviewTrail": [review] });
    };
    object.insert("apiExecutedCommands".to_string(), Value::Bool(false));
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    let trail = object
        .entry("reviewTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(review);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![review]);
        }
    }
    guardrails
}

#[derive(Debug, Clone)]
struct AllowedVerificationCommand {
    display: String,
    program: String,
    args: Vec<String>,
}

async fn run_skill_implementation_execution_verification(
    mut execution: SkillImplementationExecutionRecord,
    request: RunImplementationExecutionVerificationRequest,
    workspace_root: PathBuf,
) -> anyhow::Result<SkillImplementationExecutionRecord> {
    if execution.status != "approved_for_manual_execution" {
        return Err(anyhow!(
            "skill implementation execution must be approved_for_manual_execution before gateway verification can run"
        ));
    }
    let requested_by = optional_label(request.requested_by, "verification-runner")?;
    let timeout_secs = request
        .timeout_secs
        .unwrap_or(VERIFICATION_DEFAULT_TIMEOUT_SECS)
        .clamp(VERIFICATION_MIN_TIMEOUT_SECS, VERIFICATION_MAX_TIMEOUT_SECS);
    let commands = allowed_verification_commands_from_execution(&execution)?;
    let started_at = unix_timestamp_ms();
    let mut command_results = Vec::with_capacity(commands.len());
    for command in commands {
        command_results
            .push(run_allowed_verification_command(&command, &workspace_root, timeout_secs).await);
    }
    let completed_at = unix_timestamp_ms();
    let succeeded = command_results
        .iter()
        .all(|result| result.get("status").and_then(Value::as_str) == Some("succeeded"));
    let status = if succeeded {
        "verification_succeeded"
    } else {
        "verification_failed"
    };
    execution.status = status.to_string();
    execution.result = append_gateway_verification_result(
        execution.result,
        status,
        &requested_by,
        started_at,
        completed_at,
        timeout_secs,
        command_results,
    );
    execution.guardrails = append_gateway_verification_guardrail_event(
        execution.guardrails,
        status,
        &requested_by,
        started_at,
        completed_at,
    );
    execution.updated_at_unix_ms = completed_at;
    Ok(execution)
}

async fn run_allowed_verification_command(
    command: &AllowedVerificationCommand,
    workspace_root: &PathBuf,
    timeout_secs: u64,
) -> Value {
    let started_at = unix_timestamp_ms();
    let output = timeout(
        Duration::from_secs(timeout_secs),
        Command::new(&command.program)
            .args(&command.args)
            .current_dir(workspace_root)
            .output(),
    )
    .await;
    let completed_at = unix_timestamp_ms();
    match output {
        Ok(Ok(output)) => json!({
            "command": command.display.clone(),
            "program": command.program.clone(),
            "args": command.args.clone(),
            "startedAtUnixMs": started_at,
            "completedAtUnixMs": completed_at,
            "status": if output.status.success() { "succeeded" } else { "failed" },
            "exitCode": output.status.code(),
            "stdout": truncate_command_output(&String::from_utf8_lossy(&output.stdout)),
            "stderr": truncate_command_output(&String::from_utf8_lossy(&output.stderr)),
            "shell": false,
            "mutationAllowed": false
        }),
        Ok(Err(error)) => json!({
            "command": command.display.clone(),
            "program": command.program.clone(),
            "args": command.args.clone(),
            "startedAtUnixMs": started_at,
            "completedAtUnixMs": completed_at,
            "status": "spawn_failed",
            "error": error.to_string(),
            "shell": false,
            "mutationAllowed": false
        }),
        Err(_) => json!({
            "command": command.display.clone(),
            "program": command.program.clone(),
            "args": command.args.clone(),
            "startedAtUnixMs": started_at,
            "completedAtUnixMs": completed_at,
            "status": "timed_out",
            "timeoutSecs": timeout_secs,
            "shell": false,
            "mutationAllowed": false
        }),
    }
}

fn allowed_verification_commands_from_execution(
    execution: &SkillImplementationExecutionRecord,
) -> anyhow::Result<Vec<AllowedVerificationCommand>> {
    let commands = execution
        .command_plan
        .get("requiredCommands")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("execution commandPlan.requiredCommands must be an array"))?;
    if commands.is_empty() {
        return Err(anyhow!(
            "execution commandPlan.requiredCommands must not be empty"
        ));
    }
    commands
        .iter()
        .map(|command| {
            let raw = command
                .as_str()
                .ok_or_else(|| anyhow!("required verification commands must be strings"))?;
            parse_allowed_verification_command(raw)
        })
        .collect()
}

fn parse_allowed_verification_command(raw: &str) -> anyhow::Result<AllowedVerificationCommand> {
    let normalized = normalize_verification_command(raw);
    let args = match normalized.as_str() {
        "cargo check --manifest-path dawn_core/Cargo.toml" => {
            vec!["check", "--manifest-path", "dawn_core/Cargo.toml"]
        }
        "cargo test --manifest-path dawn_core/Cargo.toml -- --skip qgis_generates_real_contours_and_exports_real_map" =>
        {
            vec![
                "test",
                "--manifest-path",
                "dawn_core/Cargo.toml",
                "--",
                "--skip",
                "qgis_generates_real_contours_and_exports_real_map",
            ]
        }
        _ => {
            return Err(anyhow!(
                "verification command is not allowlisted: {normalized}"
            ));
        }
    };
    Ok(AllowedVerificationCommand {
        display: normalized,
        program: "cargo".to_string(),
        args: args.into_iter().map(str::to_string).collect(),
    })
}

fn normalize_verification_command(raw: &str) -> String {
    raw.replace('\\', "/")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn append_gateway_verification_result(
    result: Value,
    status: &str,
    requested_by: &str,
    started_at: u128,
    completed_at: u128,
    timeout_secs: u64,
    command_results: Vec<Value>,
) -> Value {
    let mut result = match result {
        Value::Object(_) => result,
        other => json!({ "previousResult": other }),
    };
    let event = json!({
        "status": status,
        "requestedBy": requested_by,
        "startedAtUnixMs": started_at,
        "completedAtUnixMs": completed_at,
        "timeoutSecs": timeout_secs,
        "commands": command_results,
        "apiExecutedCommands": true,
        "apiExecutedMutationCommands": false,
        "activation": "not_activated"
    });
    let Some(object) = result.as_object_mut() else {
        return json!({ "gatewayVerificationRuns": [event] });
    };
    object.insert("execution".to_string(), Value::String(status.to_string()));
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    object.insert("apiExecutedCommands".to_string(), Value::Bool(true));
    object.insert(
        "apiExecutedMutationCommands".to_string(),
        Value::Bool(false),
    );
    object.insert(
        "commandExecutionScope".to_string(),
        Value::String("allowlisted_verification_only".to_string()),
    );
    let trail = object
        .entry("gatewayVerificationRuns".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(event);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![event]);
        }
    }
    result
}

fn append_gateway_verification_guardrail_event(
    guardrails: Value,
    status: &str,
    requested_by: &str,
    started_at: u128,
    completed_at: u128,
) -> Value {
    let mut guardrails = match guardrails {
        Value::Object(_) => guardrails,
        other => json!({ "previousGuardrails": other }),
    };
    let event = json!({
        "status": status,
        "requestedBy": requested_by,
        "startedAtUnixMs": started_at,
        "completedAtUnixMs": completed_at,
        "apiExecutedCommands": true,
        "apiExecutedMutationCommands": false,
        "activation": "not_activated"
    });
    let Some(object) = guardrails.as_object_mut() else {
        return json!({ "gatewayVerificationTrail": [event] });
    };
    object.insert("autonomousCodeMutation".to_string(), Value::Bool(false));
    object.insert("apiExecutedCommands".to_string(), Value::Bool(true));
    object.insert(
        "apiExecutedMutationCommands".to_string(),
        Value::Bool(false),
    );
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    object.insert(
        "commandExecutionScope".to_string(),
        Value::String("allowlisted_verification_only".to_string()),
    );
    let trail = object
        .entry("gatewayVerificationTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(event);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![event]);
        }
    }
    guardrails
}

fn truncate_command_output(value: &str) -> String {
    value.chars().take(VERIFICATION_OUTPUT_CHAR_LIMIT).collect()
}

fn verification_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn build_skill_implementation_patch(
    execution: SkillImplementationExecutionRecord,
    request: CreateImplementationPatchRequest,
    now: u128,
) -> anyhow::Result<SkillImplementationPatchRecord> {
    if execution.status != "verification_succeeded" {
        return Err(anyhow!(
            "skill implementation execution must be verification_succeeded before a patch candidate can be created"
        ));
    }
    let created_by = optional_label(request.created_by, "patch-candidate-planner")?;
    let summary = optional_summary(request.summary).unwrap_or_else(|| {
        format!(
            "Review-only patch candidate for `{}`. This record does not apply files, activate skills, or publish releases.",
            execution.suggested_skill_id
        )
    });
    let changed_files = normalize_patch_changed_files(request.changed_files)?;
    let allowed_target_areas = execution
        .command_plan
        .get("allowedTargetAreas")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let protected_areas = execution
        .command_plan
        .get("protectedAreas")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let required_commands = execution
        .command_plan
        .get("requiredCommands")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let mut patch_manifest = request.patch_manifest.unwrap_or_else(|| json!({}));
    patch_manifest = normalize_patch_manifest(
        patch_manifest,
        execution.execution_id,
        &execution.suggested_skill_id,
        &allowed_target_areas,
        &protected_areas,
        &required_commands,
    );

    Ok(SkillImplementationPatchRecord {
        patch_id: Uuid::new_v4(),
        execution_id: execution.execution_id,
        run_id: execution.run_id,
        plan_id: execution.plan_id,
        proposal_id: execution.proposal_id,
        suggested_skill_id: execution.suggested_skill_id,
        status: "draft".to_string(),
        patch_kind: "review_only_candidate".to_string(),
        summary,
        changed_files,
        patch_manifest,
        rollback_plan: json!({
            "strategy": "review rollback scope before any future patch application",
            "manualRollbackOnly": true,
            "apiRollbackExecuted": false,
            "requiresPreApplySnapshot": true,
            "requiredNotes": [
                "list exact files to be changed",
                "capture pre-apply diff or file snapshots",
                "run verification commands after any future application",
                "do not use destructive git reset against unrelated user changes"
            ]
        }),
        verification_evidence: json!({
            "sourceExecutionId": execution.execution_id,
            "sourceExecutionStatus": execution.status,
            "sourceVerificationResult": execution.result,
            "verificationRequiredAfterFutureApply": true
        }),
        guardrails: json!({
            "autonomousCodeMutation": false,
            "autonomousPublish": false,
            "apiGeneratedPatch": false,
            "apiAppliedPatch": false,
            "apiRollbackExecuted": false,
            "activation": "not_activated",
            "requiresSeparatePatchApplyApproval": true,
            "requiresSeparateSkillActivationApproval": true,
            "forbiddenWithoutSeparateApproval": [
                "applying patch content",
                "modifying files",
                "running mutation commands",
                "activating skills",
                "publishing releases",
                "credential changes",
                "destructive filesystem operations"
            ]
        }),
        created_by,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn normalize_patch_manifest(
    patch_manifest: Value,
    execution_id: Uuid,
    suggested_skill_id: &str,
    allowed_target_areas: &Value,
    protected_areas: &Value,
    required_commands: &Value,
) -> Value {
    let mut manifest = match patch_manifest {
        Value::Object(_) => patch_manifest,
        other => json!({ "operatorProvidedManifest": other }),
    };
    let Some(object) = manifest.as_object_mut() else {
        return json!({
            "kind": "implementation_patch_candidate",
            "sourceExecutionId": execution_id,
            "suggestedSkillId": suggested_skill_id,
            "apiAppliedPatch": false,
            "patchContentRequiredBeforeApplyApproval": true
        });
    };
    object
        .entry("kind".to_string())
        .or_insert_with(|| Value::String("implementation_patch_candidate".to_string()));
    object
        .entry("sourceExecutionId".to_string())
        .or_insert_with(|| Value::String(execution_id.to_string()));
    object
        .entry("suggestedSkillId".to_string())
        .or_insert_with(|| Value::String(suggested_skill_id.to_string()));
    object
        .entry("allowedTargetAreas".to_string())
        .or_insert_with(|| allowed_target_areas.clone());
    object
        .entry("protectedAreas".to_string())
        .or_insert_with(|| protected_areas.clone());
    object
        .entry("requiredVerificationCommands".to_string())
        .or_insert_with(|| required_commands.clone());
    object.insert("apiAppliedPatch".to_string(), Value::Bool(false));
    object.insert(
        "patchContentRequiredBeforeApplyApproval".to_string(),
        Value::Bool(true),
    );
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    manifest
}

fn normalize_patch_changed_files(value: Option<Value>) -> anyhow::Result<Value> {
    let Some(value) = value else {
        return Ok(Value::Array(Vec::new()));
    };
    let Value::Array(items) = value else {
        return Err(anyhow!("changedFiles must be an array of relative paths"));
    };
    let mut normalized = Vec::new();
    for item in items {
        let Some(path) = item.as_str().map(str::trim).filter(|path| !path.is_empty()) else {
            return Err(anyhow!("changedFiles entries must be non-empty strings"));
        };
        if path.len() > 240
            || path.starts_with('/')
            || path.starts_with('\\')
            || path.contains("..")
            || path.contains(':')
        {
            return Err(anyhow!("changedFiles entries must be safe relative paths"));
        }
        normalized.push(Value::String(path.replace('\\', "/")));
        if normalized.len() > 100 {
            return Err(anyhow!("changedFiles contains too many entries"));
        }
    }
    Ok(Value::Array(normalized))
}

fn apply_skill_implementation_patch_review(
    mut patch: SkillImplementationPatchRecord,
    request: SkillImplementationPatchReviewRequest,
    now: u128,
) -> anyhow::Result<SkillImplementationPatchRecord> {
    let status = normalize_skill_implementation_patch_review_status(&request.status)?;
    let reviewer = optional_label(request.reviewer, "operator")?;
    let note = optional_summary(request.note);
    patch.status = status.to_string();
    patch.verification_evidence = append_skill_implementation_patch_review_evidence(
        patch.verification_evidence,
        status,
        &reviewer,
        note.as_deref(),
        now,
    );
    patch.guardrails = append_skill_implementation_patch_review_event(
        patch.guardrails,
        status,
        &reviewer,
        note.as_deref(),
        now,
    );
    patch.updated_at_unix_ms = now;
    Ok(patch)
}

fn normalize_skill_implementation_patch_review_status(
    status: &str,
) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "draft" | "reopen" | "reopened" => Ok("draft"),
        "approved_for_apply" | "approve" | "approved" => Ok("approved_for_apply"),
        "rejected" | "reject" => Ok("rejected"),
        "deferred" | "defer" | "needs_more_evidence" => Ok("deferred"),
        _ => Err(anyhow!(
            "status must be one of draft, approved_for_apply, rejected, or deferred"
        )),
    }
}

fn append_skill_implementation_patch_review_evidence(
    evidence: Value,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
    now: u128,
) -> Value {
    let mut evidence = match evidence {
        Value::Object(_) => evidence,
        other => json!({ "previousEvidence": other }),
    };
    let review = json!({
        "status": status,
        "reviewer": reviewer,
        "note": note.map(|value| truncate_summary(value, 1000)),
        "reviewedAtUnixMs": now,
        "apiAppliedPatch": false,
        "activation": "not_activated",
    });
    let Some(object) = evidence.as_object_mut() else {
        return json!({ "reviewTrail": [review] });
    };
    let trail = object
        .entry("reviewTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(review);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![review]);
        }
    }
    evidence
}

fn append_skill_implementation_patch_review_event(
    guardrails: Value,
    status: &str,
    reviewer: &str,
    note: Option<&str>,
    now: u128,
) -> Value {
    let mut guardrails = match guardrails {
        Value::Object(_) => guardrails,
        other => json!({ "previousGuardrails": other }),
    };
    let review = json!({
        "status": status,
        "reviewer": reviewer,
        "note": note.map(|value| truncate_summary(value, 1000)),
        "reviewedAtUnixMs": now,
        "apiAppliedPatch": false,
        "apiRollbackExecuted": false,
        "activation": "not_activated",
    });
    let Some(object) = guardrails.as_object_mut() else {
        return json!({ "reviewTrail": [review] });
    };
    object.insert("autonomousCodeMutation".to_string(), Value::Bool(false));
    object.insert("apiAppliedPatch".to_string(), Value::Bool(false));
    object.insert("apiRollbackExecuted".to_string(), Value::Bool(false));
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    let trail = object
        .entry("reviewTrail".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(review);
            if items.len() > 20 {
                let remove_count = items.len() - 20;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![review]);
        }
    }
    guardrails
}

async fn apply_skill_implementation_patch_candidate(
    mut patch: SkillImplementationPatchRecord,
    request: ApplyImplementationPatchRequest,
    workspace_root: PathBuf,
) -> anyhow::Result<SkillImplementationPatchRecord> {
    if !matches!(
        patch.status.as_str(),
        "approved_for_apply" | "apply_dry_run_succeeded"
    ) {
        return Err(anyhow!(
            "skill implementation patch must be approved_for_apply before apply"
        ));
    }
    let dry_run = request.dry_run.unwrap_or(true);
    validate_patch_operation_confirmation(
        patch.patch_id,
        request.confirm_patch_id,
        dry_run,
        "apply",
    )?;
    let requested_by = optional_label(request.requested_by, "operator")?;
    let allowed_target_areas = patch_allowed_target_areas(&patch.patch_manifest);
    let changes = parse_patch_file_changes(&patch.patch_manifest, &allowed_target_areas)?;
    validate_patch_changed_files_cover_manifest(&patch.changed_files, &changes)?;

    let mut snapshots = Vec::new();
    for change in &changes {
        let target = workspace_root.join(&change.path);
        let (existed, old_content, old_sha256) =
            read_patch_file_snapshot(&target, &change.path).await?;
        if let Some(expected) = &change.expected_old_sha256 {
            let actual = old_sha256.as_deref().unwrap_or("missing");
            if !expected.eq_ignore_ascii_case(actual) {
                return Err(anyhow!(
                    "expectedOldSha256 mismatch for {}: expected {}, actual {}",
                    change.path,
                    expected,
                    actual
                ));
            }
        }
        snapshots.push(json!({
            "path": change.path,
            "existed": existed,
            "oldSha256": old_sha256,
            "oldContent": old_content,
            "newSha256": sha256_hex(change.new_content.as_bytes()),
        }));
    }

    if !dry_run {
        for change in &changes {
            write_patch_file(&workspace_root, change).await?;
        }
    }

    let now = unix_timestamp_ms();
    let status = if dry_run {
        "apply_dry_run_succeeded"
    } else {
        "applied_pending_verification"
    };
    patch.status = status.to_string();
    patch.patch_manifest = append_patch_apply_manifest(
        patch.patch_manifest,
        dry_run,
        &requested_by,
        changes.len(),
        now,
    );
    patch.rollback_plan = append_patch_apply_rollback_plan(
        patch.rollback_plan,
        dry_run,
        &requested_by,
        snapshots.clone(),
        now,
    );
    patch.verification_evidence = append_patch_runtime_evidence(
        patch.verification_evidence,
        "apply",
        status,
        dry_run,
        &requested_by,
        json!({
            "fileChangeCount": changes.len(),
            "changedFiles": changes.iter().map(|change| change.path.clone()).collect::<Vec<_>>(),
        }),
        now,
    );
    patch.guardrails = append_patch_runtime_guardrail_event(
        patch.guardrails,
        "apply",
        status,
        dry_run,
        &requested_by,
        !dry_run,
        false,
        now,
    );
    patch.updated_at_unix_ms = now;
    Ok(patch)
}

async fn rollback_skill_implementation_patch_candidate(
    mut patch: SkillImplementationPatchRecord,
    request: RollbackImplementationPatchRequest,
    workspace_root: PathBuf,
) -> anyhow::Result<SkillImplementationPatchRecord> {
    if !matches!(
        patch.status.as_str(),
        "applied_pending_verification" | "rollback_dry_run_succeeded"
    ) {
        return Err(anyhow!(
            "skill implementation patch must be applied_pending_verification before rollback"
        ));
    }
    let dry_run = request.dry_run.unwrap_or(true);
    validate_patch_operation_confirmation(
        patch.patch_id,
        request.confirm_patch_id,
        dry_run,
        "rollback",
    )?;
    let requested_by = optional_label(request.requested_by, "operator")?;
    let allowed_target_areas = patch_allowed_target_areas(&patch.patch_manifest);
    let snapshots = parse_patch_rollback_snapshots(&patch.rollback_plan, &allowed_target_areas)?;
    for snapshot in &snapshots {
        validate_rollback_current_state(&workspace_root, snapshot).await?;
    }

    if !dry_run {
        for snapshot in &snapshots {
            restore_patch_snapshot(&workspace_root, snapshot).await?;
        }
    }

    let now = unix_timestamp_ms();
    let status = if dry_run {
        "rollback_dry_run_succeeded"
    } else {
        "rollback_succeeded"
    };
    patch.status = status.to_string();
    patch.patch_manifest =
        append_patch_rollback_manifest(patch.patch_manifest, dry_run, &requested_by, now);
    patch.rollback_plan = append_patch_rollback_plan(
        patch.rollback_plan,
        dry_run,
        &requested_by,
        snapshots.len(),
        now,
    );
    patch.verification_evidence = append_patch_runtime_evidence(
        patch.verification_evidence,
        "rollback",
        status,
        dry_run,
        &requested_by,
        json!({
            "snapshotCount": snapshots.len(),
            "changedFiles": snapshots.iter().map(|snapshot| snapshot.path.clone()).collect::<Vec<_>>(),
        }),
        now,
    );
    patch.guardrails = append_patch_runtime_guardrail_event(
        patch.guardrails,
        "rollback",
        status,
        dry_run,
        &requested_by,
        dry_run,
        !dry_run,
        now,
    );
    patch.updated_at_unix_ms = now;
    Ok(patch)
}

fn validate_patch_operation_confirmation(
    patch_id: Uuid,
    confirm_patch_id: Option<Uuid>,
    dry_run: bool,
    operation: &str,
) -> anyhow::Result<()> {
    if dry_run {
        return Ok(());
    }
    if confirm_patch_id == Some(patch_id) {
        Ok(())
    } else {
        Err(anyhow!(
            "real patch {operation} requires confirmPatchId to equal the patch id"
        ))
    }
}

fn parse_patch_file_changes(
    manifest: &Value,
    allowed_target_areas: &[String],
) -> anyhow::Result<Vec<PatchFileChange>> {
    let file_changes = manifest
        .get("fileChanges")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("patchManifest.fileChanges must be an array before apply"))?;
    if file_changes.is_empty() {
        return Err(anyhow!(
            "patchManifest.fileChanges must include at least one file change"
        ));
    }
    if file_changes.len() > PATCH_MAX_FILE_CHANGES {
        return Err(anyhow!(
            "patchManifest.fileChanges contains too many file changes"
        ));
    }

    let mut changes = Vec::new();
    for item in file_changes {
        let Some(object) = item.as_object() else {
            return Err(anyhow!("patchManifest.fileChanges entries must be objects"));
        };
        let raw_path = object
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("patchManifest.fileChanges[].path is required"))?;
        let path = normalize_patch_relative_path(raw_path, allowed_target_areas)?;
        let new_content = object
            .get("newContent")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("patchManifest.fileChanges[].newContent is required"))?
            .to_string();
        if new_content.len() > PATCH_MAX_FILE_BYTES {
            return Err(anyhow!(
                "patchManifest.fileChanges[].newContent exceeds the size limit"
            ));
        }
        let expected_old_sha256 = object
            .get("expectedOldSha256")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(validate_sha256_hex)
            .transpose()?;
        changes.push(PatchFileChange {
            path,
            new_content,
            expected_old_sha256,
        });
    }
    Ok(changes)
}

#[derive(Debug, Clone)]
struct PatchRollbackSnapshot {
    path: String,
    existed: bool,
    old_content: Option<String>,
    new_sha256: Option<String>,
}

fn parse_patch_rollback_snapshots(
    rollback_plan: &Value,
    allowed_target_areas: &[String],
) -> anyhow::Result<Vec<PatchRollbackSnapshot>> {
    let snapshots = rollback_plan
        .get("snapshots")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("rollbackPlan.snapshots are required before rollback"))?;
    if snapshots.is_empty() {
        return Err(anyhow!("rollbackPlan.snapshots cannot be empty"));
    }
    if snapshots.len() > PATCH_MAX_FILE_CHANGES {
        return Err(anyhow!("rollbackPlan.snapshots contains too many files"));
    }

    let mut parsed = Vec::new();
    for item in snapshots {
        let Some(object) = item.as_object() else {
            return Err(anyhow!("rollbackPlan.snapshots entries must be objects"));
        };
        let raw_path = object
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("rollbackPlan.snapshots[].path is required"))?;
        let path = normalize_patch_relative_path(raw_path, allowed_target_areas)?;
        let existed = object
            .get("existed")
            .and_then(Value::as_bool)
            .ok_or_else(|| anyhow!("rollbackPlan.snapshots[].existed is required"))?;
        let old_content = object
            .get("oldContent")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        if existed && old_content.is_none() {
            return Err(anyhow!(
                "rollbackPlan.snapshots[].oldContent is required for existing files"
            ));
        }
        let new_sha256 = object
            .get("newSha256")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(validate_sha256_hex)
            .transpose()?;
        parsed.push(PatchRollbackSnapshot {
            path,
            existed,
            old_content,
            new_sha256,
        });
    }
    Ok(parsed)
}

fn validate_sha256_hex(value: &str) -> anyhow::Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() == 64 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(normalized)
    } else {
        Err(anyhow!(
            "sha256 values must be 64 lowercase or uppercase hex characters"
        ))
    }
}

fn validate_patch_changed_files_cover_manifest(
    changed_files: &Value,
    changes: &[PatchFileChange],
) -> anyhow::Result<()> {
    let Some(items) = changed_files.as_array() else {
        return Ok(());
    };
    if items.is_empty() {
        return Ok(());
    }
    let declared = items
        .iter()
        .filter_map(Value::as_str)
        .map(|value| value.replace('\\', "/"))
        .collect::<Vec<_>>();
    for change in changes {
        if !declared.iter().any(|path| path == &change.path) {
            return Err(anyhow!(
                "patchManifest.fileChanges includes {} but changedFiles does not",
                change.path
            ));
        }
    }
    Ok(())
}

fn patch_allowed_target_areas(manifest: &Value) -> Vec<String> {
    let mut areas = manifest
        .get("allowedTargetAreas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(normalize_patch_allowed_area)
        .collect::<Vec<_>>();
    if areas.is_empty() {
        areas = vec![
            "dawn_core/src".to_string(),
            "workflow/native_skills".to_string(),
            "README.md".to_string(),
            "docs".to_string(),
        ];
    }
    areas
}

fn normalize_patch_allowed_area(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .to_string();
    if normalized.is_empty()
        || normalized == "."
        || normalized.contains("..")
        || normalized.contains(':')
    {
        return None;
    }
    Some(normalized)
}

fn normalize_patch_relative_path(
    value: &str,
    allowed_target_areas: &[String],
) -> anyhow::Result<String> {
    let normalized = value.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.len() > 240
        || normalized.starts_with('/')
        || normalized.contains(':')
    {
        return Err(anyhow!("patch file paths must be safe relative paths"));
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(anyhow!(
                "patch file paths must not contain empty or parent components"
            ));
        }
        parts.push(part);
    }
    let path = parts.join("/");
    if is_forbidden_patch_path(&path) {
        return Err(anyhow!("patch file path is protected: {path}"));
    }
    if !allowed_target_areas
        .iter()
        .any(|area| path == *area || path.starts_with(&format!("{area}/")))
    {
        return Err(anyhow!(
            "patch file path is outside allowed target areas: {path}"
        ));
    }
    Ok(path)
}

fn is_forbidden_patch_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.contains("qgis")
        || lower == ".git"
        || lower.starts_with(".git/")
        || lower == ".cache"
        || lower.starts_with(".cache/")
        || lower == "data"
        || lower.starts_with("data/")
        || lower == "tmp"
        || lower.starts_with("tmp/")
        || lower == "target"
        || lower.starts_with("target/")
        || lower == "dawn_core/output"
        || lower.starts_with("dawn_core/output/")
        || lower == "dawn_core/target"
        || lower.starts_with("dawn_core/target/")
    {
        return true;
    }
    let file_name = lower.rsplit('/').next().unwrap_or(&lower);
    file_name == ".env"
        || file_name.starts_with(".env.")
        || file_name.ends_with(".pem")
        || file_name.ends_with(".key")
        || file_name.ends_with(".pfx")
        || file_name.ends_with(".p12")
        || file_name.contains("id_rsa")
        || file_name.contains("id_ed25519")
}

async fn read_patch_file_snapshot(
    target: &FsPath,
    display_path: &str,
) -> anyhow::Result<(bool, Option<String>, Option<String>)> {
    match tokio::fs::read(target).await {
        Ok(bytes) => {
            if bytes.len() > PATCH_MAX_FILE_BYTES {
                return Err(anyhow!(
                    "existing file exceeds patch snapshot size: {display_path}"
                ));
            }
            let sha256 = sha256_hex(&bytes);
            let content = String::from_utf8(bytes)
                .with_context(|| format!("patch apply only supports UTF-8 text: {display_path}"))?;
            Ok((true, Some(content), Some(sha256)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((false, None, None)),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read patch snapshot for {display_path}"))
        }
    }
}

async fn write_patch_file(workspace_root: &FsPath, change: &PatchFileChange) -> anyhow::Result<()> {
    let target = workspace_root.join(&change.path);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create patch parent for {}", change.path))?;
    }
    tokio::fs::write(&target, change.new_content.as_bytes())
        .await
        .with_context(|| format!("failed to write patch file {}", change.path))
}

async fn validate_rollback_current_state(
    workspace_root: &FsPath,
    snapshot: &PatchRollbackSnapshot,
) -> anyhow::Result<()> {
    let target = workspace_root.join(&snapshot.path);
    let (exists, _content, current_sha256) =
        read_patch_file_snapshot(&target, &snapshot.path).await?;
    let Some(expected_new_sha256) = snapshot.new_sha256.as_deref() else {
        return Ok(());
    };
    if !exists {
        return Ok(());
    }
    let actual = current_sha256.as_deref().unwrap_or("missing");
    if !expected_new_sha256.eq_ignore_ascii_case(actual) {
        return Err(anyhow!(
            "rollback refused because {} changed after patch apply: expected {}, actual {}",
            snapshot.path,
            expected_new_sha256,
            actual
        ));
    }
    Ok(())
}

async fn restore_patch_snapshot(
    workspace_root: &FsPath,
    snapshot: &PatchRollbackSnapshot,
) -> anyhow::Result<()> {
    let target = workspace_root.join(&snapshot.path);
    if snapshot.existed {
        let content = snapshot.old_content.as_deref().ok_or_else(|| {
            anyhow!(
                "rollback snapshot for {} is missing oldContent",
                snapshot.path
            )
        })?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create rollback parent for {}", snapshot.path)
            })?;
        }
        tokio::fs::write(&target, content.as_bytes())
            .await
            .with_context(|| format!("failed to restore patch file {}", snapshot.path))?;
    } else {
        match tokio::fs::remove_file(&target).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to remove patch-created file {}", snapshot.path)
                });
            }
        }
    }
    Ok(())
}

fn append_patch_apply_manifest(
    manifest: Value,
    dry_run: bool,
    requested_by: &str,
    file_change_count: usize,
    now: u128,
) -> Value {
    let mut manifest = match manifest {
        Value::Object(_) => manifest,
        other => json!({ "previousManifest": other }),
    };
    let Some(object) = manifest.as_object_mut() else {
        return json!({});
    };
    object.insert("apiAppliedPatch".to_string(), Value::Bool(!dry_run));
    object.insert("lastApplyDryRun".to_string(), Value::Bool(dry_run));
    object.insert(
        "lastApplyRequestedBy".to_string(),
        Value::String(requested_by.to_string()),
    );
    object.insert(
        "lastApplyAtUnixMs".to_string(),
        Value::String(now.to_string()),
    );
    object.insert("fileChangeCount".to_string(), json!(file_change_count));
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    manifest
}

fn append_patch_rollback_manifest(
    manifest: Value,
    dry_run: bool,
    requested_by: &str,
    now: u128,
) -> Value {
    let mut manifest = match manifest {
        Value::Object(_) => manifest,
        other => json!({ "previousManifest": other }),
    };
    let Some(object) = manifest.as_object_mut() else {
        return json!({});
    };
    if !dry_run {
        object.insert("apiAppliedPatch".to_string(), Value::Bool(false));
    }
    object.insert("lastRollbackDryRun".to_string(), Value::Bool(dry_run));
    object.insert(
        "lastRollbackRequestedBy".to_string(),
        Value::String(requested_by.to_string()),
    );
    object.insert(
        "lastRollbackAtUnixMs".to_string(),
        Value::String(now.to_string()),
    );
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    manifest
}

fn append_patch_apply_rollback_plan(
    rollback_plan: Value,
    dry_run: bool,
    requested_by: &str,
    snapshots: Vec<Value>,
    now: u128,
) -> Value {
    let mut rollback_plan = match rollback_plan {
        Value::Object(_) => rollback_plan,
        other => json!({ "previousRollbackPlan": other }),
    };
    let Some(object) = rollback_plan.as_object_mut() else {
        return json!({});
    };
    object.insert(
        "strategy".to_string(),
        Value::String("restore captured pre-apply file snapshots".to_string()),
    );
    object.insert("manualRollbackOnly".to_string(), Value::Bool(false));
    object.insert("apiRollbackAvailable".to_string(), Value::Bool(!dry_run));
    object.insert("apiRollbackExecuted".to_string(), Value::Bool(false));
    object.insert("lastApplyDryRun".to_string(), Value::Bool(dry_run));
    object.insert(
        "lastApplyRequestedBy".to_string(),
        Value::String(requested_by.to_string()),
    );
    object.insert(
        "lastApplyAtUnixMs".to_string(),
        Value::String(now.to_string()),
    );
    if dry_run {
        object.insert(
            "lastApplyDryRunSnapshots".to_string(),
            Value::Array(snapshots),
        );
    } else {
        object.insert("snapshots".to_string(), Value::Array(snapshots));
    }
    rollback_plan
}

fn append_patch_rollback_plan(
    rollback_plan: Value,
    dry_run: bool,
    requested_by: &str,
    snapshot_count: usize,
    now: u128,
) -> Value {
    let mut rollback_plan = match rollback_plan {
        Value::Object(_) => rollback_plan,
        other => json!({ "previousRollbackPlan": other }),
    };
    let Some(object) = rollback_plan.as_object_mut() else {
        return json!({});
    };
    object.insert("lastRollbackDryRun".to_string(), Value::Bool(dry_run));
    object.insert(
        "lastRollbackRequestedBy".to_string(),
        Value::String(requested_by.to_string()),
    );
    object.insert(
        "lastRollbackAtUnixMs".to_string(),
        Value::String(now.to_string()),
    );
    object.insert(
        "lastRollbackSnapshotCount".to_string(),
        json!(snapshot_count),
    );
    if !dry_run {
        object.insert("apiRollbackAvailable".to_string(), Value::Bool(false));
        object.insert("apiRollbackExecuted".to_string(), Value::Bool(true));
    }
    rollback_plan
}

fn append_patch_runtime_evidence(
    evidence: Value,
    operation: &str,
    status: &str,
    dry_run: bool,
    requested_by: &str,
    detail: Value,
    now: u128,
) -> Value {
    let mut evidence = match evidence {
        Value::Object(_) => evidence,
        other => json!({ "previousEvidence": other }),
    };
    let event = json!({
        "operation": operation,
        "status": status,
        "dryRun": dry_run,
        "requestedBy": requested_by,
        "detail": detail,
        "recordedAtUnixMs": now,
        "activation": "not_activated",
    });
    let Some(object) = evidence.as_object_mut() else {
        return json!({ "runtimeTrail": [event] });
    };
    append_limited_json_array(object, "runtimeTrail", event, 20);
    evidence
}

fn append_patch_runtime_guardrail_event(
    guardrails: Value,
    operation: &str,
    status: &str,
    dry_run: bool,
    requested_by: &str,
    api_applied_patch: bool,
    api_rollback_executed: bool,
    now: u128,
) -> Value {
    let mut guardrails = match guardrails {
        Value::Object(_) => guardrails,
        other => json!({ "previousGuardrails": other }),
    };
    let Some(object) = guardrails.as_object_mut() else {
        return json!({});
    };
    object.insert("autonomousCodeMutation".to_string(), Value::Bool(false));
    object.insert(
        "apiAppliedPatch".to_string(),
        Value::Bool(api_applied_patch),
    );
    object.insert(
        "apiRollbackExecuted".to_string(),
        Value::Bool(api_rollback_executed),
    );
    object.insert(
        "activation".to_string(),
        Value::String("not_activated".to_string()),
    );
    object.insert(
        "requiresPostApplyVerification".to_string(),
        Value::Bool(operation == "apply" && !dry_run),
    );
    let event = json!({
        "operation": operation,
        "status": status,
        "dryRun": dry_run,
        "requestedBy": requested_by,
        "apiAppliedPatch": api_applied_patch,
        "apiRollbackExecuted": api_rollback_executed,
        "recordedAtUnixMs": now,
        "activation": "not_activated",
    });
    append_limited_json_array(object, "runtimeTrail", event, 20);
    guardrails
}

fn append_limited_json_array(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Value,
    max_len: usize,
) {
    let trail = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match trail {
        Value::Array(items) => {
            items.push(value);
            if items.len() > max_len {
                let remove_count = items.len() - max_len;
                items.drain(0..remove_count);
            }
        }
        other => {
            *other = Value::Array(vec![value]);
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn build_manual_experience(
    request: RecordExperienceRequest,
) -> anyhow::Result<AgentExperienceRecord> {
    let now = unix_timestamp_ms();
    Ok(AgentExperienceRecord {
        experience_id: Uuid::new_v4(),
        source: optional_label(request.source, "manual")?,
        scope: optional_label(request.scope, "general")?,
        task_kind: optional_label(request.task_kind, "unknown")?,
        input_summary: required_summary(&request.input_summary, "inputSummary")?,
        action_summary: required_summary(&request.action_summary, "actionSummary")?,
        outcome: required_label(&request.outcome, "outcome")?,
        lesson: required_summary(&request.lesson, "lesson")?,
        reusable_hint: optional_summary(request.reusable_hint),
        evidence: request.evidence.unwrap_or_else(|| json!({})),
        tags: normalize_tags(request.tags.unwrap_or_default()),
        risk_level: normalize_risk_level(request.risk_level)?,
        related_task_id: request.related_task_id,
        related_ingress_id: request.related_ingress_id,
        created_by: optional_label(request.created_by, "agent")?,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn build_auto_ingress_experience(
    event: ChatIngressEventRecord,
    created_by: &str,
) -> anyhow::Result<AgentExperienceRecord> {
    let lesson = auto_lesson_for_ingress(&event);
    let hint = auto_hint_for_ingress(&event);
    let status = event.status;
    build_ingress_experience(
        event,
        CaptureIngressExperienceRequest {
            outcome: Some(default_outcome_for_ingress(status)),
            action_summary: None,
            lesson,
            reusable_hint: Some(hint),
            tags: Some(vec![
                "auto-reflection".to_string(),
                "chat-ingress".to_string(),
                ingress_status_label(status).to_string(),
            ]),
            risk_level: Some(auto_risk_for_ingress(status).to_string()),
            created_by: Some(created_by.to_string()),
        },
    )
}

fn build_ingress_experience(
    event: ChatIngressEventRecord,
    request: CaptureIngressExperienceRequest,
) -> anyhow::Result<AgentExperienceRecord> {
    let now = unix_timestamp_ms();
    let outcome = request
        .outcome
        .map(|value| required_label(&value, "outcome"))
        .transpose()?
        .unwrap_or_else(|| default_outcome_for_ingress(event.status));
    let created_by = request
        .created_by
        .or(event.sender_display.clone())
        .or(event.sender_id.clone())
        .unwrap_or_else(|| "agent".to_string());
    let action_summary = request
        .action_summary
        .or(event.reply_text.clone())
        .or(event.error.clone())
        .unwrap_or_else(|| {
            format!(
                "ingress event ended with {}",
                ingress_status_label(event.status)
            )
        });

    Ok(AgentExperienceRecord {
        experience_id: Uuid::new_v4(),
        source: format!("chat_ingress:{}", event.platform),
        scope: "chat".to_string(),
        task_kind: if event.linked_task_id.is_some() {
            "task_routing".to_string()
        } else {
            "conversation".to_string()
        },
        input_summary: truncate_summary(&event.text, 900),
        action_summary: required_summary(&action_summary, "actionSummary")?,
        outcome,
        lesson: required_summary(&request.lesson, "lesson")?,
        reusable_hint: optional_summary(request.reusable_hint),
        evidence: json!({
            "ingressId": event.ingress_id,
            "platform": event.platform,
            "eventType": event.event_type,
            "status": ingress_status_label(event.status),
            "linkedTaskId": event.linked_task_id,
            "hadReply": event.reply_text.is_some(),
            "hadError": event.error.is_some()
        }),
        tags: normalize_tags(
            request
                .tags
                .unwrap_or_else(|| vec!["chat_ingress".to_string()]),
        ),
        risk_level: normalize_risk_level(request.risk_level)?,
        related_task_id: event.linked_task_id,
        related_ingress_id: Some(event.ingress_id),
        created_by: optional_label(Some(created_by), "agent")?,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn should_reflect_ingress_event(status: ChatIngressStatus) -> bool {
    matches!(
        status,
        ChatIngressStatus::Replied
            | ChatIngressStatus::TaskCreated
            | ChatIngressStatus::Failed
            | ChatIngressStatus::PendingApproval
    )
}

fn auto_lesson_for_ingress(event: &ChatIngressEventRecord) -> String {
    match event.status {
        ChatIngressStatus::Replied => {
            "类似聊天已能直接回复时，下次应优先保持对话路径，并只把相关经验作为提示，不创建任务。"
                .to_string()
        }
        ChatIngressStatus::TaskCreated => {
            "聊天事件创建任务后，下次应确认用户是否显式要求任务或技能调用，并继续跟踪任务是否完成。"
                .to_string()
        }
        ChatIngressStatus::Failed => {
            "聊天回复失败时，下次应返回短错误说明并保留诊断摘要，避免把长日志直接发回聊天通道。"
                .to_string()
        }
        ChatIngressStatus::PendingApproval => {
            "需要审批的聊天动作应保持等待审批状态，不能因为自动复盘而提升权限或继续执行。"
                .to_string()
        }
        ChatIngressStatus::Ignored | ChatIngressStatus::Received => {
            "该聊天事件不适合自动沉淀为可复用经验。".to_string()
        }
    }
}

fn auto_hint_for_ingress(event: &ChatIngressEventRecord) -> String {
    match event.status {
        ChatIngressStatus::Replied => {
            "检查可用模型、聊天模式和经验检索结果；保持简短直接回复。".to_string()
        }
        ChatIngressStatus::TaskCreated => {
            "检查 linkedTaskId、技能绑定状态和是否需要用户明确 /task 或 /skill。".to_string()
        }
        ChatIngressStatus::Failed => {
            "检查模型连接器、聊天发送限制和短错误回执是否成功投递。".to_string()
        }
        ChatIngressStatus::PendingApproval => {
            "检查 approval 队列和用户审批状态；不要自动绕过审批。".to_string()
        }
        ChatIngressStatus::Ignored | ChatIngressStatus::Received => "无需复用。".to_string(),
    }
}

fn auto_risk_for_ingress(status: ChatIngressStatus) -> &'static str {
    match status {
        ChatIngressStatus::PendingApproval => "guarded",
        ChatIngressStatus::Failed => "low",
        ChatIngressStatus::TaskCreated => "guarded",
        ChatIngressStatus::Replied | ChatIngressStatus::Ignored | ChatIngressStatus::Received => {
            "low"
        }
    }
}

fn default_outcome_for_ingress(status: ChatIngressStatus) -> String {
    match status {
        ChatIngressStatus::Replied => "success",
        ChatIngressStatus::TaskCreated => "needs_skill_binding",
        ChatIngressStatus::Failed => "failed",
        ChatIngressStatus::PendingApproval => "pending_approval",
        ChatIngressStatus::Ignored => "ignored",
        ChatIngressStatus::Received => "observed",
    }
    .to_string()
}

fn auto_reflection_enabled() -> bool {
    std::env::var("DAWN_EVOLUTION_AUTO_REFLECTION")
        .ok()
        .map(|value| !matches!(value.as_str(), "0" | "false" | "FALSE" | "off" | "OFF"))
        .unwrap_or(true)
}

fn ingress_status_label(status: ChatIngressStatus) -> &'static str {
    match status {
        ChatIngressStatus::Received => "received",
        ChatIngressStatus::PendingApproval => "pending_approval",
        ChatIngressStatus::TaskCreated => "task_created",
        ChatIngressStatus::Replied => "replied",
        ChatIngressStatus::Ignored => "ignored",
        ChatIngressStatus::Failed => "failed",
    }
}

fn optional_label(value: Option<String>, fallback: &str) -> anyhow::Result<String> {
    let value = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if value.len() > 120 {
        return Err(anyhow!("label is too long"));
    }
    Ok(value.to_string())
}

fn required_label(value: &str, field: &str) -> anyhow::Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("{field} is required"));
    }
    if value.len() > 120 {
        return Err(anyhow!("{field} is too long"));
    }
    Ok(value.to_string())
}

fn required_summary(value: &str, field: &str) -> anyhow::Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("{field} is required"));
    }
    Ok(truncate_summary(value, 2000))
}

fn optional_summary(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_summary(value, 2000))
}

fn truncate_summary(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_ascii_lowercase();
        if tag.is_empty() || tag.len() > 64 || normalized.iter().any(|value| value == &tag) {
            continue;
        }
        normalized.push(tag);
        if normalized.len() >= 16 {
            break;
        }
    }
    normalized
}

fn normalize_risk_level(value: Option<String>) -> anyhow::Result<String> {
    let risk = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("low")
        .to_ascii_lowercase();
    match risk.as_str() {
        "low" | "guarded" | "high" | "blocked" => Ok(risk),
        _ => Err(anyhow!(
            "riskLevel must be one of low, guarded, high, or blocked"
        )),
    }
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
    error!(?error, "Evolution API failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        ApplyImplementationPatchRequest, CaptureIngressExperienceRequest,
        CreateImplementationPatchRequest, RecordExperienceRequest,
        RollbackImplementationPatchRequest, RunImplementationExecutionVerificationRequest,
        SkillImplementationExecutionReviewRequest, SkillImplementationPatchReviewRequest,
        SkillImplementationPlanReviewRequest, SkillImplementationRunReviewRequest,
        SkillProposalReviewRequest, append_gateway_verification_guardrail_event,
        append_gateway_verification_result, apply_skill_implementation_execution_review,
        apply_skill_implementation_patch_candidate, apply_skill_implementation_patch_review,
        apply_skill_implementation_plan_review, apply_skill_implementation_run_review,
        apply_skill_proposal_review, auto_hint_for_ingress, auto_lesson_for_ingress,
        build_auto_ingress_experience, build_ingress_experience, build_manual_experience,
        build_skill_implementation_execution, build_skill_implementation_patch,
        build_skill_implementation_plan, build_skill_implementation_run,
        build_skill_proposal_candidates, normalize_patch_changed_files,
        normalize_patch_relative_path, normalize_skill_implementation_execution_review_status,
        normalize_skill_implementation_patch_review_status,
        normalize_skill_implementation_plan_review_status,
        normalize_skill_implementation_run_review_status, normalize_skill_proposal_review_status,
        parse_allowed_verification_command, rollback_skill_implementation_patch_candidate,
        run_skill_implementation_execution_verification, sha256_hex, should_reflect_ingress_event,
        slugify_skill_id_component,
    };
    use crate::app_state::{
        AgentExperienceRecord, ChatIngressEventRecord, ChatIngressStatus,
        SkillImplementationExecutionRecord, SkillImplementationPatchRecord,
        SkillImplementationPlanRecord, SkillImplementationRunRecord, SkillProposalRecord,
        unix_timestamp_ms,
    };

    #[test]
    fn builds_manual_experience_with_normalized_tags() {
        let record = build_manual_experience(RecordExperienceRequest {
            source: Some("reflection".to_string()),
            scope: None,
            task_kind: Some("desktop_control".to_string()),
            input_summary: "打开微信开发者工具".to_string(),
            action_summary: "检查窗口后点击".to_string(),
            outcome: "success".to_string(),
            lesson: "下次先检查窗口标题".to_string(),
            reusable_hint: Some("用窗口标题验证".to_string()),
            evidence: Some(json!({"verified": true})),
            tags: Some(vec!["Desktop".to_string(), "desktop".to_string()]),
            risk_level: Some("guarded".to_string()),
            related_task_id: None,
            related_ingress_id: None,
            created_by: None,
        })
        .expect("experience should build");

        assert_eq!(record.source, "reflection");
        assert_eq!(record.scope, "general");
        assert_eq!(record.tags, vec!["desktop"]);
        assert_eq!(record.risk_level, "guarded");
    }

    #[test]
    fn captures_chat_ingress_experience_without_raw_payload() {
        let ingress_id = Uuid::new_v4();
        let record = build_ingress_experience(
            ChatIngressEventRecord {
                ingress_id,
                platform: "telegram".to_string(),
                event_type: "telegram.message.1".to_string(),
                chat_id: Some("1".to_string()),
                sender_id: Some("42".to_string()),
                sender_display: Some("alice".to_string()),
                text: "你是谁".to_string(),
                raw_payload: json!({"secret": "not copied"}),
                linked_task_id: None,
                reply_text: Some("我是 Dawn".to_string()),
                status: ChatIngressStatus::Replied,
                error: None,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
            },
            CaptureIngressExperienceRequest {
                outcome: None,
                action_summary: None,
                lesson: "普通聊天应直接回复，不应创建任务".to_string(),
                reusable_hint: None,
                tags: None,
                risk_level: None,
                created_by: None,
            },
        )
        .expect("ingress experience should build");

        assert_eq!(record.outcome, "success");
        assert_eq!(record.related_ingress_id, Some(ingress_id));
        assert!(record.evidence.get("secret").is_none());
        assert_eq!(record.created_by, "alice");
    }

    #[test]
    fn auto_reflection_builds_guarded_task_routing_lesson() {
        let task_id = Uuid::new_v4();
        let record = ChatIngressEventRecord {
            ingress_id: Uuid::new_v4(),
            platform: "telegram".to_string(),
            event_type: "telegram.message.2".to_string(),
            chat_id: Some("1".to_string()),
            sender_id: Some("42".to_string()),
            sender_display: Some("alice".to_string()),
            text: "打开微信开发者工具".to_string(),
            raw_payload: json!({"secret": "not copied"}),
            linked_task_id: Some(task_id),
            reply_text: Some("Task accepted".to_string()),
            status: ChatIngressStatus::TaskCreated,
            error: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };
        let experience = build_auto_ingress_experience(record, "auto-reflection")
            .expect("auto reflection should build");

        assert_eq!(experience.task_kind, "task_routing");
        assert_eq!(experience.outcome, "needs_skill_binding");
        assert_eq!(experience.risk_level, "guarded");
        assert_eq!(experience.related_task_id, Some(task_id));
        assert!(experience.tags.contains(&"auto-reflection".to_string()));
        assert!(experience.lesson.contains("确认用户是否显式要求任务"));
        assert!(experience.evidence.get("secret").is_none());
    }

    #[test]
    fn auto_reflection_only_accepts_terminal_useful_ingress_states() {
        assert!(should_reflect_ingress_event(ChatIngressStatus::Replied));
        assert!(should_reflect_ingress_event(ChatIngressStatus::TaskCreated));
        assert!(should_reflect_ingress_event(ChatIngressStatus::Failed));
        assert!(should_reflect_ingress_event(
            ChatIngressStatus::PendingApproval
        ));
        assert!(!should_reflect_ingress_event(ChatIngressStatus::Received));
        assert!(!should_reflect_ingress_event(ChatIngressStatus::Ignored));

        let event = ChatIngressEventRecord {
            ingress_id: Uuid::new_v4(),
            platform: "telegram".to_string(),
            event_type: "telegram.message.3".to_string(),
            chat_id: None,
            sender_id: None,
            sender_display: None,
            text: "你是谁".to_string(),
            raw_payload: json!({}),
            linked_task_id: None,
            reply_text: Some("我是 Dawn".to_string()),
            status: ChatIngressStatus::Replied,
            error: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };
        assert!(auto_lesson_for_ingress(&event).contains("直接回复"));
        assert!(auto_hint_for_ingress(&event).contains("可用模型"));
    }

    #[test]
    fn proposes_skill_from_repeated_non_conversation_experiences() {
        let now = unix_timestamp_ms();
        let experiences = vec![
            AgentExperienceRecord {
                experience_id: Uuid::new_v4(),
                source: "auto-reflection".to_string(),
                scope: "chat".to_string(),
                task_kind: "desktop_control".to_string(),
                input_summary: "打开微信开发者工具".to_string(),
                action_summary: "等待审批后执行".to_string(),
                outcome: "needs_skill_binding".to_string(),
                lesson: "桌面控制请求应先确认窗口".to_string(),
                reusable_hint: Some("检查窗口标题".to_string()),
                evidence: json!({"safe": true}),
                tags: vec!["desktop".to_string()],
                risk_level: "guarded".to_string(),
                related_task_id: None,
                related_ingress_id: None,
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            },
            AgentExperienceRecord {
                experience_id: Uuid::new_v4(),
                source: "auto-reflection".to_string(),
                scope: "chat".to_string(),
                task_kind: "desktop_control".to_string(),
                input_summary: "点击目标窗口".to_string(),
                action_summary: "等待审批后执行".to_string(),
                outcome: "needs_skill_binding".to_string(),
                lesson: "点击前应确认坐标和目标应用".to_string(),
                reusable_hint: Some("检查鼠标位置".to_string()),
                evidence: json!({"safe": true}),
                tags: vec!["desktop".to_string(), "mouse".to_string()],
                risk_level: "guarded".to_string(),
                related_task_id: None,
                related_ingress_id: None,
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now + 1,
            },
            AgentExperienceRecord {
                experience_id: Uuid::new_v4(),
                source: "auto-reflection".to_string(),
                scope: "chat".to_string(),
                task_kind: "conversation".to_string(),
                input_summary: "你是谁".to_string(),
                action_summary: "直接回复".to_string(),
                outcome: "success".to_string(),
                lesson: "普通聊天直接回复".to_string(),
                reusable_hint: None,
                evidence: json!({}),
                tags: vec!["chat".to_string()],
                risk_level: "low".to_string(),
                related_task_id: None,
                related_ingress_id: None,
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            },
        ];

        let proposals = build_skill_proposal_candidates(&experiences, 2, "test")
            .expect("proposal candidates should build");

        assert_eq!(proposals.len(), 1);
        assert_eq!(
            proposals[0].proposal_key,
            "experience-task-kind:desktop_control"
        );
        assert_eq!(
            proposals[0].suggested_skill_id,
            "dawn.desktop-control-assistant"
        );
        assert_eq!(proposals[0].risk_level, "guarded");
        assert!(proposals[0].evidence["lessons"].is_array());
        assert!(!proposals[0].evidence.to_string().contains("rawPayload"));
    }

    #[test]
    fn slugifies_skill_id_components() {
        assert_eq!(
            slugify_skill_id_component("desktop_control"),
            "desktop-control"
        );
        assert_eq!(
            slugify_skill_id_component("  Desktop Control  "),
            "desktop-control"
        );
        assert_eq!(slugify_skill_id_component("地图/渲染"), "workflow");
    }

    #[test]
    fn reviews_skill_proposal_without_activation() {
        let now = unix_timestamp_ms();
        let proposal = SkillProposalRecord {
            proposal_id: Uuid::new_v4(),
            proposal_key: "experience-task-kind:desktop_control".to_string(),
            title: "Propose reviewed skill for desktop_control".to_string(),
            summary: "Repeated desktop control requests.".to_string(),
            rationale: "Advisory only.".to_string(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            source: "experience-pattern".to_string(),
            status: "proposed".to_string(),
            confidence: 0.62,
            evidence: json!({"experienceCount": 2}),
            tags: vec!["skill-proposal".to_string(), "desktop".to_string()],
            risk_level: "guarded".to_string(),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let reviewed = apply_skill_proposal_review(
            proposal,
            SkillProposalReviewRequest {
                status: "approved".to_string(),
                reviewer: Some("operator".to_string()),
                note: Some("Looks useful; implementation still needs separate review.".to_string()),
            },
            now + 1,
        )
        .expect("review should apply");

        assert_eq!(reviewed.status, "approved");
        assert_eq!(
            reviewed.suggested_skill_id,
            "dawn.desktop-control-assistant"
        );
        assert_eq!(reviewed.updated_at_unix_ms, now + 1);
        let review_trail = reviewed.evidence["reviewTrail"]
            .as_array()
            .expect("review trail should exist");
        assert_eq!(review_trail.len(), 1);
        assert_eq!(review_trail[0]["status"], "approved");
        assert_eq!(review_trail[0]["activation"], "not_activated");
    }

    #[test]
    fn rejects_skill_proposal_activation_status() {
        assert_eq!(
            normalize_skill_proposal_review_status("approve").unwrap(),
            "approved"
        );
        assert!(normalize_skill_proposal_review_status("activated").is_err());
        assert!(normalize_skill_proposal_review_status("implemented").is_err());
    }

    #[test]
    fn builds_implementation_plan_for_approved_proposal() {
        let now = unix_timestamp_ms();
        let proposal = SkillProposalRecord {
            proposal_id: Uuid::new_v4(),
            proposal_key: "experience-task-kind:desktop_control".to_string(),
            title: "Propose reviewed skill for desktop_control".to_string(),
            summary: "Repeated desktop control requests.".to_string(),
            rationale: "Advisory only.".to_string(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            source: "experience-pattern".to_string(),
            status: "approved".to_string(),
            confidence: 0.62,
            evidence: json!({"experienceCount": 2}),
            tags: vec!["skill-proposal".to_string(), "desktop".to_string()],
            risk_level: "guarded".to_string(),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let plan = build_skill_implementation_plan(proposal, "operator", now + 1)
            .expect("approved proposal should produce a plan");

        assert_eq!(plan.status, "draft");
        assert_eq!(plan.created_by, "operator");
        assert_eq!(plan.suggested_skill_id, "dawn.desktop-control-assistant");
        assert_eq!(plan.guardrails["autonomousCodeMutation"], false);
        assert_eq!(plan.guardrails["requiresHumanReviewBeforeActivation"], true);
        assert!(plan.steps.as_array().expect("steps should be array").len() >= 5);
        assert!(plan.acceptance_criteria.to_string().contains("Existing QQ"));
    }

    #[test]
    fn rejects_implementation_plan_for_unapproved_proposal() {
        let now = unix_timestamp_ms();
        let proposal = SkillProposalRecord {
            proposal_id: Uuid::new_v4(),
            proposal_key: "experience-task-kind:desktop_control".to_string(),
            title: "Propose reviewed skill for desktop_control".to_string(),
            summary: "Repeated desktop control requests.".to_string(),
            rationale: "Advisory only.".to_string(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            source: "experience-pattern".to_string(),
            status: "proposed".to_string(),
            confidence: 0.62,
            evidence: json!({"experienceCount": 2}),
            tags: vec!["skill-proposal".to_string(), "desktop".to_string()],
            risk_level: "guarded".to_string(),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        assert!(build_skill_implementation_plan(proposal, "operator", now + 1).is_err());
    }

    #[test]
    fn reviews_implementation_plan_without_execution() {
        let now = unix_timestamp_ms();
        let plan = SkillImplementationPlanRecord {
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            title: "Draft implementation plan".to_string(),
            summary: "Plan only.".to_string(),
            status: "draft".to_string(),
            steps: json!([{"order": 1, "name": "Confirm scope"}]),
            acceptance_criteria: json!(["Existing chat ingress behavior is preserved."]),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "requiresHumanReviewBeforeActivation": true
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let reviewed = apply_skill_implementation_plan_review(
            plan,
            SkillImplementationPlanReviewRequest {
                status: "approved".to_string(),
                reviewer: Some("operator".to_string()),
                note: Some("Ready for separate implementation work.".to_string()),
            },
            now + 1,
        )
        .expect("plan review should apply");

        assert_eq!(reviewed.status, "approved");
        assert_eq!(reviewed.updated_at_unix_ms, now + 1);
        let review_trail = reviewed.guardrails["reviewTrail"]
            .as_array()
            .expect("review trail should exist");
        assert_eq!(review_trail[0]["status"], "approved");
        assert_eq!(review_trail[0]["execution"], "not_started");
        assert_eq!(review_trail[0]["activation"], "not_activated");
    }

    #[test]
    fn rejects_implementation_plan_execution_status() {
        assert_eq!(
            normalize_skill_implementation_plan_review_status("approve").unwrap(),
            "approved"
        );
        assert!(normalize_skill_implementation_plan_review_status("implemented").is_err());
        assert!(normalize_skill_implementation_plan_review_status("activated").is_err());
    }

    #[test]
    fn builds_implementation_run_from_approved_plan_without_execution() {
        let now = unix_timestamp_ms();
        let plan = SkillImplementationPlanRecord {
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            title: "Draft implementation plan".to_string(),
            summary: "Plan only.".to_string(),
            status: "approved".to_string(),
            steps: json!([{"order": 1, "name": "Confirm scope"}]),
            acceptance_criteria: json!(["Existing chat ingress behavior is preserved."]),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "requiresHumanReviewBeforeActivation": true
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let run = build_skill_implementation_run(plan, "operator", now + 1)
            .expect("approved plan should produce a run package");

        assert_eq!(run.status, "prepared");
        assert_eq!(run.execution_mode, "guarded_manual_or_future_agent");
        assert_eq!(run.guardrails["autonomousCodeMutation"], false);
        assert_eq!(run.guardrails["execution"], "not_executed");
        assert_eq!(run.guardrails["activation"], "not_activated");
        assert!(
            run.verification["requiredCommands"]
                .to_string()
                .contains("cargo check")
        );
        assert!(
            run.rollback["manualRollbackOnly"]
                .as_bool()
                .unwrap_or(false)
        );
    }

    #[test]
    fn rejects_implementation_run_for_unapproved_plan() {
        let now = unix_timestamp_ms();
        let plan = SkillImplementationPlanRecord {
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            title: "Draft implementation plan".to_string(),
            summary: "Plan only.".to_string(),
            status: "draft".to_string(),
            steps: json!([]),
            acceptance_criteria: json!([]),
            guardrails: json!({}),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        assert!(build_skill_implementation_run(plan, "operator", now + 1).is_err());
    }

    #[test]
    fn reviews_implementation_run_without_execution() {
        let now = unix_timestamp_ms();
        let run = SkillImplementationRunRecord {
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "prepared".to_string(),
            execution_mode: "guarded_manual_or_future_agent".to_string(),
            change_package: json!({"allowedTargetAreas": ["dawn_core/src"]}),
            verification: json!({"requiredCommands": ["cargo check"]}),
            rollback: json!({"manualRollbackOnly": true}),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "execution": "not_executed",
                "activation": "not_activated"
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let reviewed = apply_skill_implementation_run_review(
            run,
            SkillImplementationRunReviewRequest {
                status: "approved_for_execution".to_string(),
                reviewer: Some("operator".to_string()),
                note: Some("Approved as a run package only.".to_string()),
            },
            now + 1,
        )
        .expect("run review should apply");

        assert_eq!(reviewed.status, "approved_for_execution");
        let review_trail = reviewed.guardrails["reviewTrail"]
            .as_array()
            .expect("review trail should exist");
        assert_eq!(review_trail[0]["execution"], "not_executed");
        assert_eq!(review_trail[0]["activation"], "not_activated");
    }

    #[test]
    fn rejects_implementation_run_executed_status() {
        assert_eq!(
            normalize_skill_implementation_run_review_status("approve").unwrap(),
            "approved_for_execution"
        );
        assert!(normalize_skill_implementation_run_review_status("executed").is_err());
        assert!(normalize_skill_implementation_run_review_status("activated").is_err());
    }

    #[test]
    fn builds_execution_from_approved_run_without_api_command_execution() {
        let now = unix_timestamp_ms();
        let run = SkillImplementationRunRecord {
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "approved_for_execution".to_string(),
            execution_mode: "guarded_manual_or_future_agent".to_string(),
            change_package: json!({
                "allowedTargetAreas": ["dawn_core/src"],
                "protectedAreas": ["credentials and environment files"]
            }),
            verification: json!({
                "requiredCommands": [
                    "cargo check --manifest-path dawn_core/Cargo.toml"
                ]
            }),
            rollback: json!({"manualRollbackOnly": true}),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "execution": "not_executed",
                "activation": "not_activated"
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let execution =
            build_skill_implementation_execution(run, "operator", "guarded-agent", now + 1)
                .expect("approved run should produce an execution record");

        assert_eq!(execution.status, "ready_for_execution");
        assert_eq!(execution.executor, "guarded-agent");
        assert_eq!(execution.preflight_report["status"], "passed");
        assert_eq!(execution.preflight_report["apiExecutedCommands"], false);
        assert_eq!(execution.command_plan["apiExecutedCommands"], false);
        assert_eq!(execution.result["execution"], "not_started");
        assert_eq!(execution.guardrails["autonomousCodeMutation"], false);
    }

    #[test]
    fn rejects_execution_for_unapproved_run() {
        let now = unix_timestamp_ms();
        let run = SkillImplementationRunRecord {
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "prepared".to_string(),
            execution_mode: "guarded_manual_or_future_agent".to_string(),
            change_package: json!({"allowedTargetAreas": ["dawn_core/src"]}),
            verification: json!({"requiredCommands": ["cargo check"]}),
            rollback: json!({"manualRollbackOnly": true}),
            guardrails: json!({}),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        assert!(
            build_skill_implementation_execution(run, "operator", "guarded-agent", now + 1)
                .is_err()
        );
    }

    #[test]
    fn reviews_execution_without_api_command_execution() {
        let now = unix_timestamp_ms();
        let execution = SkillImplementationExecutionRecord {
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "ready_for_execution".to_string(),
            executor: "operator_or_guarded_agent".to_string(),
            preflight_report: json!({
                "status": "passed",
                "apiExecutedCommands": false
            }),
            command_plan: json!({
                "requiredCommands": ["cargo check"],
                "apiExecutedCommands": false
            }),
            result: json!({
                "execution": "not_started",
                "activation": "not_activated",
                "apiExecutedCommands": false
            }),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "apiExecutedCommands": false,
                "activation": "not_activated"
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let reviewed = apply_skill_implementation_execution_review(
            execution,
            SkillImplementationExecutionReviewRequest {
                status: "approved_for_manual_execution".to_string(),
                reviewer: Some("operator".to_string()),
                note: Some("Operator may run the command plan outside this API.".to_string()),
                result: Some(json!({"operatorDecision": "approved"})),
            },
            now + 1,
        )
        .expect("execution review should apply");

        assert_eq!(reviewed.status, "approved_for_manual_execution");
        assert_eq!(reviewed.result["apiExecutedCommands"], false);
        assert_eq!(
            reviewed.result["operatorReportedStatus"],
            "approved_for_manual_execution"
        );
        let review_trail = reviewed.guardrails["reviewTrail"]
            .as_array()
            .expect("review trail should exist");
        assert_eq!(review_trail[0]["apiExecutedCommands"], false);
        assert_eq!(review_trail[0]["activation"], "not_activated");
    }

    #[test]
    fn rejects_execution_executed_status() {
        assert_eq!(
            normalize_skill_implementation_execution_review_status("approve").unwrap(),
            "approved_for_manual_execution"
        );
        assert!(normalize_skill_implementation_execution_review_status("executed").is_err());
        assert!(normalize_skill_implementation_execution_review_status("activated").is_err());
        assert!(normalize_skill_implementation_execution_review_status("published").is_err());
    }

    #[test]
    fn allows_only_known_gateway_verification_commands() {
        let check =
            parse_allowed_verification_command("cargo check --manifest-path dawn_core\\Cargo.toml")
                .expect("cargo check should be allowlisted");
        assert_eq!(check.program, "cargo");
        assert_eq!(
            check.args,
            vec!["check", "--manifest-path", "dawn_core/Cargo.toml"]
        );

        let test = parse_allowed_verification_command(
            "cargo test --manifest-path dawn_core/Cargo.toml -- --skip qgis_generates_real_contours_and_exports_real_map",
        )
        .expect("non-QGIS cargo test should be allowlisted");
        assert_eq!(test.args[0], "test");

        assert!(
            parse_allowed_verification_command(
                "cargo check --manifest-path dawn_core/Cargo.toml && del data"
            )
            .is_err()
        );
        assert!(
            parse_allowed_verification_command(
                "cmd /c cargo check --manifest-path dawn_core/Cargo.toml"
            )
            .is_err()
        );
        assert!(
            parse_allowed_verification_command("cargo test --manifest-path dawn_node/Cargo.toml")
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_gateway_verification_for_unapproved_execution_without_running_commands() {
        let now = unix_timestamp_ms();
        let execution = SkillImplementationExecutionRecord {
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "ready_for_execution".to_string(),
            executor: "operator_or_guarded_agent".to_string(),
            preflight_report: json!({"status": "passed"}),
            command_plan: json!({
                "requiredCommands": ["cargo check --manifest-path dawn_core/Cargo.toml"]
            }),
            result: json!({"execution": "not_started"}),
            guardrails: json!({"autonomousCodeMutation": false}),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let result = run_skill_implementation_execution_verification(
            execution,
            RunImplementationExecutionVerificationRequest {
                requested_by: Some("operator".to_string()),
                timeout_secs: Some(10),
            },
            std::path::PathBuf::from("."),
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn records_gateway_verification_result_without_mutation_permission() {
        let result = append_gateway_verification_result(
            json!({"execution": "not_started"}),
            "verification_succeeded",
            "operator",
            10,
            20,
            30,
            vec![json!({
                "command": "cargo check --manifest-path dawn_core/Cargo.toml",
                "status": "succeeded"
            })],
        );

        assert_eq!(result["execution"], "verification_succeeded");
        assert_eq!(result["apiExecutedCommands"], true);
        assert_eq!(result["apiExecutedMutationCommands"], false);
        assert_eq!(result["activation"], "not_activated");
        assert_eq!(
            result["commandExecutionScope"],
            "allowlisted_verification_only"
        );

        let guardrails = append_gateway_verification_guardrail_event(
            json!({"autonomousCodeMutation": false}),
            "verification_succeeded",
            "operator",
            10,
            20,
        );
        assert_eq!(guardrails["autonomousCodeMutation"], false);
        assert_eq!(guardrails["apiExecutedCommands"], true);
        assert_eq!(guardrails["apiExecutedMutationCommands"], false);
        assert_eq!(guardrails["activation"], "not_activated");
    }

    #[test]
    fn builds_patch_candidate_from_verified_execution_without_applying_patch() {
        let now = unix_timestamp_ms();
        let execution = SkillImplementationExecutionRecord {
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "verification_succeeded".to_string(),
            executor: "gateway-verification".to_string(),
            preflight_report: json!({"status": "passed"}),
            command_plan: json!({
                "requiredCommands": ["cargo check --manifest-path dawn_core/Cargo.toml"],
                "allowedTargetAreas": ["dawn_core/src"],
                "protectedAreas": ["credentials and environment files"]
            }),
            result: json!({
                "execution": "verification_succeeded",
                "apiExecutedMutationCommands": false
            }),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "activation": "not_activated"
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let patch = build_skill_implementation_patch(
            execution,
            CreateImplementationPatchRequest {
                created_by: Some("operator".to_string()),
                summary: Some("Candidate package only.".to_string()),
                changed_files: Some(json!(["dawn_core/src/evolution.rs"])),
                patch_manifest: Some(json!({
                    "apiAppliedPatch": true,
                    "notes": "malicious or stale input must be normalized"
                })),
            },
            now + 1,
        )
        .expect("verified execution should produce a patch candidate");

        assert_eq!(patch.status, "draft");
        assert_eq!(patch.patch_kind, "review_only_candidate");
        assert_eq!(patch.patch_manifest["apiAppliedPatch"], false);
        assert_eq!(
            patch.patch_manifest["patchContentRequiredBeforeApplyApproval"],
            true
        );
        assert_eq!(patch.guardrails["apiAppliedPatch"], false);
        assert_eq!(patch.guardrails["activation"], "not_activated");
        assert_eq!(patch.rollback_plan["manualRollbackOnly"], true);
    }

    #[test]
    fn rejects_patch_candidate_for_unverified_execution() {
        let now = unix_timestamp_ms();
        let execution = SkillImplementationExecutionRecord {
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "approved_for_manual_execution".to_string(),
            executor: "operator".to_string(),
            preflight_report: json!({}),
            command_plan: json!({}),
            result: json!({}),
            guardrails: json!({}),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        assert!(
            build_skill_implementation_patch(
                execution,
                CreateImplementationPatchRequest {
                    created_by: Some("operator".to_string()),
                    summary: None,
                    changed_files: None,
                    patch_manifest: None,
                },
                now + 1,
            )
            .is_err()
        );
    }

    #[test]
    fn reviews_patch_candidate_without_applying_patch() {
        let now = unix_timestamp_ms();
        let patch = SkillImplementationPatchRecord {
            patch_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: "draft".to_string(),
            patch_kind: "review_only_candidate".to_string(),
            summary: "Candidate only.".to_string(),
            changed_files: json!(["dawn_core/src/evolution.rs"]),
            patch_manifest: json!({"apiAppliedPatch": false}),
            rollback_plan: json!({"manualRollbackOnly": true}),
            verification_evidence: json!({"sourceExecutionStatus": "verification_succeeded"}),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "apiAppliedPatch": false,
                "activation": "not_activated"
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };

        let reviewed = apply_skill_implementation_patch_review(
            patch,
            SkillImplementationPatchReviewRequest {
                status: "approved_for_apply".to_string(),
                reviewer: Some("operator".to_string()),
                note: Some("Approved as a patch candidate only.".to_string()),
            },
            now + 1,
        )
        .expect("patch review should apply");

        assert_eq!(reviewed.status, "approved_for_apply");
        assert_eq!(reviewed.guardrails["apiAppliedPatch"], false);
        assert_eq!(reviewed.guardrails["apiRollbackExecuted"], false);
        let review_trail = reviewed.guardrails["reviewTrail"]
            .as_array()
            .expect("review trail should exist");
        assert_eq!(review_trail[0]["apiAppliedPatch"], false);
        assert_eq!(review_trail[0]["activation"], "not_activated");
    }

    #[test]
    fn rejects_patch_applied_or_activated_status() {
        assert_eq!(
            normalize_skill_implementation_patch_review_status("approve").unwrap(),
            "approved_for_apply"
        );
        assert!(normalize_skill_implementation_patch_review_status("applied").is_err());
        assert!(normalize_skill_implementation_patch_review_status("activated").is_err());
        assert!(normalize_skill_implementation_patch_review_status("published").is_err());
    }

    #[test]
    fn validates_patch_changed_files_are_safe_relative_paths() {
        let files = normalize_patch_changed_files(Some(json!([
            "dawn_core/src/evolution.rs",
            "workflow/native_skills/example/SKILL.md"
        ])))
        .expect("safe relative paths should pass");
        assert_eq!(
            files.as_array().expect("files should be array")[0],
            "dawn_core/src/evolution.rs"
        );

        assert!(normalize_patch_changed_files(Some(json!("../secret.env"))).is_err());
        assert!(normalize_patch_changed_files(Some(json!(["../secret.env"]))).is_err());
        assert!(normalize_patch_changed_files(Some(json!(["C:/secret.env"]))).is_err());
        assert!(normalize_patch_changed_files(Some(json!([""]))).is_err());
    }

    #[test]
    fn rejects_apply_paths_for_qgis_and_protected_locations() {
        let allowed = vec![
            "dawn_core/src".to_string(),
            "workflow/native_skills".to_string(),
            "README.md".to_string(),
            "docs".to_string(),
        ];

        assert!(normalize_patch_relative_path("dawn_core/src/evolution.rs", &allowed).is_ok());
        assert!(normalize_patch_relative_path("dawn_core/src/qgis.rs", &allowed).is_err());
        assert!(
            normalize_patch_relative_path(
                "workflow/native_skills/qgis-map-skills/SKILL.md",
                &allowed
            )
            .is_err()
        );
        assert!(normalize_patch_relative_path("data/dawn_core.db", &allowed).is_err());
        assert!(normalize_patch_relative_path(".git/config", &allowed).is_err());
        assert!(normalize_patch_relative_path("target/debug/app.exe", &allowed).is_err());
        assert!(normalize_patch_relative_path("../README.md", &allowed).is_err());
        assert!(normalize_patch_relative_path("docs/.env", &allowed).is_err());
    }

    #[tokio::test]
    async fn apply_patch_dry_run_does_not_write_files() {
        let workspace = temp_patch_workspace();
        let file_path = workspace.join("README.md");
        tokio::fs::write(&file_path, "old\n")
            .await
            .expect("seed file should be written");
        let old_sha256 = sha256_hex("old\n".as_bytes());
        let patch_id = Uuid::new_v4();
        let patch = sample_patch_record(
            patch_id,
            "approved_for_apply",
            json!(["README.md"]),
            json!({
                "allowedTargetAreas": ["README.md"],
                "fileChanges": [{
                    "path": "README.md",
                    "newContent": "new\n",
                    "expectedOldSha256": old_sha256
                }]
            }),
        );

        let applied = apply_skill_implementation_patch_candidate(
            patch,
            ApplyImplementationPatchRequest {
                requested_by: Some("operator".to_string()),
                confirm_patch_id: None,
                dry_run: Some(true),
            },
            workspace.clone(),
        )
        .await
        .expect("dry-run apply should succeed");

        assert_eq!(applied.status, "apply_dry_run_succeeded");
        assert_eq!(applied.guardrails["apiAppliedPatch"], false);
        assert_eq!(applied.rollback_plan["apiRollbackAvailable"], false);
        assert!(applied.rollback_plan["lastApplyDryRunSnapshots"].is_array());
        assert_eq!(
            tokio::fs::read_to_string(&file_path)
                .await
                .expect("file should still exist"),
            "old\n"
        );
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn apply_patch_writes_then_rollback_restores_snapshot() {
        let workspace = temp_patch_workspace();
        let docs_dir = workspace.join("docs");
        tokio::fs::create_dir_all(&docs_dir)
            .await
            .expect("docs dir should be created");
        let file_path = docs_dir.join("autonomous-evolution-test.md");
        tokio::fs::write(&file_path, "old\n")
            .await
            .expect("seed file should be written");
        let old_sha256 = sha256_hex("old\n".as_bytes());
        let patch_id = Uuid::new_v4();
        let patch = sample_patch_record(
            patch_id,
            "approved_for_apply",
            json!(["docs/autonomous-evolution-test.md"]),
            json!({
                "allowedTargetAreas": ["docs"],
                "fileChanges": [{
                    "path": "docs/autonomous-evolution-test.md",
                    "newContent": "new\n",
                    "expectedOldSha256": old_sha256
                }]
            }),
        );

        let applied = apply_skill_implementation_patch_candidate(
            patch,
            ApplyImplementationPatchRequest {
                requested_by: Some("operator".to_string()),
                confirm_patch_id: Some(patch_id),
                dry_run: Some(false),
            },
            workspace.clone(),
        )
        .await
        .expect("real apply should succeed");

        assert_eq!(applied.status, "applied_pending_verification");
        assert_eq!(applied.guardrails["apiAppliedPatch"], true);
        assert_eq!(applied.guardrails["requiresPostApplyVerification"], true);
        assert!(applied.rollback_plan["snapshots"].is_array());
        assert_eq!(
            tokio::fs::read_to_string(&file_path)
                .await
                .expect("file should be updated"),
            "new\n"
        );

        let rolled_back = rollback_skill_implementation_patch_candidate(
            applied,
            RollbackImplementationPatchRequest {
                requested_by: Some("operator".to_string()),
                confirm_patch_id: Some(patch_id),
                dry_run: Some(false),
            },
            workspace.clone(),
        )
        .await
        .expect("real rollback should succeed");

        assert_eq!(rolled_back.status, "rollback_succeeded");
        assert_eq!(rolled_back.guardrails["apiAppliedPatch"], false);
        assert_eq!(rolled_back.guardrails["apiRollbackExecuted"], true);
        assert_eq!(
            tokio::fs::read_to_string(&file_path)
                .await
                .expect("file should be restored"),
            "old\n"
        );
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn rejects_apply_without_approved_patch_status() {
        let workspace = temp_patch_workspace();
        let patch = sample_patch_record(
            Uuid::new_v4(),
            "draft",
            json!(["README.md"]),
            json!({
                "allowedTargetAreas": ["README.md"],
                "fileChanges": [{
                    "path": "README.md",
                    "newContent": "new\n"
                }]
            }),
        );

        let result = apply_skill_implementation_patch_candidate(
            patch,
            ApplyImplementationPatchRequest {
                requested_by: None,
                confirm_patch_id: None,
                dry_run: Some(true),
            },
            workspace.clone(),
        )
        .await;

        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(workspace);
    }

    fn temp_patch_workspace() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("dawn-patch-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("temp workspace should be created");
        path
    }

    fn sample_patch_record(
        patch_id: Uuid,
        status: &str,
        changed_files: serde_json::Value,
        patch_manifest: serde_json::Value,
    ) -> SkillImplementationPatchRecord {
        let now = unix_timestamp_ms();
        SkillImplementationPatchRecord {
            patch_id,
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            proposal_id: Uuid::new_v4(),
            suggested_skill_id: "dawn.desktop-control-assistant".to_string(),
            status: status.to_string(),
            patch_kind: "review_only_candidate".to_string(),
            summary: "Candidate only.".to_string(),
            changed_files,
            patch_manifest,
            rollback_plan: json!({
                "manualRollbackOnly": true,
                "apiRollbackExecuted": false
            }),
            verification_evidence: json!({
                "sourceExecutionStatus": "verification_succeeded"
            }),
            guardrails: json!({
                "autonomousCodeMutation": false,
                "apiAppliedPatch": false,
                "apiRollbackExecuted": false,
                "activation": "not_activated"
            }),
            created_by: "test".to_string(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        }
    }
}
