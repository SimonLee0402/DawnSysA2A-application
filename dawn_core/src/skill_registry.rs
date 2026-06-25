use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::{get, post},
};
use base64::prelude::*;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::fs;
use wasmtime::Module;

use crate::app_state::{
    AppState, SkillProposalRecord, SkillPublisherTrustRootRecord, unix_timestamp_ms,
};
use uuid::Uuid;

pub const SKILL_PUBLISHER_ISSUER_DID_PREFIX: &str = "did:dawn:skill-publisher:";
pub const NATIVE_BUILTIN_SOURCE_KIND: &str = "native_builtin";
const SKILL_INTAKE_MAX_BYTES: usize = 1_048_576;
const SKILL_PACKAGE_MAX_BYTES: usize = 16 * 1_048_576;

struct NativeBuiltinSkillSpec {
    skill_id: &'static str,
    version: &'static str,
    display_name: &'static str,
    description: &'static str,
    capabilities: &'static [&'static str],
    artifact_relative_path: &'static str,
}

const NATIVE_BUILTIN_SKILLS: &[NativeBuiltinSkillSpec] = &[
    NativeBuiltinSkillSpec {
        skill_id: "agent-card-discoverer",
        version: "native",
        display_name: "Agent Card Discoverer",
        description: "Dawn native skill for discovering, validating, and operationalizing A2A Agent Cards through the local marketplace, federated catalogs, and operator workflows.",
        capabilities: &[
            "a2a",
            "agent_cards",
            "marketplace_search",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/agent-card-discoverer/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "bayesian-skill-set",
        version: "native",
        display_name: "Bayesian Skill Set",
        description: "Dawn native skill for uncertainty-aware planning, evidence fusion, and safe next-step selection across #chat/#observe/#assist/#autopilot workflows.",
        capabilities: &[
            "bayesian_planning",
            "decision_support",
            "safety_modes",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/bayesian-skill-set/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-orchestrator",
        version: "native",
        display_name: "Dawn Orchestrator",
        description: "Dawn native skill for turning user or operator intent into tasks, subtask delegation, workflow execution, and result stitching across the local gateway.",
        capabilities: &[
            "task_orchestration",
            "delegation",
            "workflow_execution",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/dawn-orchestrator/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-chat-bridge",
        version: "native",
        display_name: "Dawn Chat Bridge",
        description: "Dawn native skill for normalizing inbound chat commands and routing replies across supported messaging channels.",
        capabilities: &[
            "chat_ingress",
            "chat_dispatch",
            "multi_channel",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/dawn-chat-bridge/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-desktop-control",
        version: "native",
        display_name: "Dawn Desktop Control",
        description: "Dawn native skill for guarded desktop observation, mouse positioning, and click control from approved chat and node-command workflows.",
        capabilities: &[
            "desktop_control",
            "mouse_control",
            "screen_observation",
            "approval_required",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/dawn-desktop-control/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-model-router",
        version: "native",
        display_name: "Dawn Model Router",
        description: "Dawn native skill for selecting, routing, and operating cloud or local models through unified connectors, including Ollama-hosted local models.",
        capabilities: &[
            "model_routing",
            "connector_ops",
            "local_models",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/dawn-model-router/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-node-operator",
        version: "native",
        display_name: "Dawn Node Operator",
        description: "Dawn native skill for operating the local Dawn node, health checks, rollout handoff, and workspace execution on the desktop machine.",
        capabilities: &["node_ops", "health_checks", "rollout", "native_workflow"],
        artifact_relative_path: "../workflow/native_skills/dawn-node-operator/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-approval-guard",
        version: "native",
        display_name: "Dawn Approval Guard",
        description: "Dawn native skill for human-in-the-loop approvals, AP2 authorization, and guarded execution of sensitive actions.",
        capabilities: &[
            "approval_center",
            "payment_authorization",
            "guardrails",
            "native_workflow",
        ],
        artifact_relative_path: "../workflow/native_skills/dawn-approval-guard/SKILL.md",
    },
    NativeBuiltinSkillSpec {
        skill_id: "dawn-marketplace-operator",
        version: "native",
        display_name: "Dawn Marketplace Operator",
        description: "Dawn native skill for publishing, searching, installing, and federating agent cards and signed skills across Dawn gateways.",
        capabilities: &[
            "marketplace",
            "skill_distribution",
            "agent_cards",
            "federation",
        ],
        artifact_relative_path: "../workflow/native_skills/dawn-marketplace-operator/SKILL.md",
    },
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub skill_id: String,
    pub version: String,
    pub display_name: String,
    pub description: Option<String>,
    pub entry_function: String,
    pub capabilities: Vec<String>,
    pub artifact_path: String,
    pub artifact_sha256: String,
    pub source_kind: String,
    pub issuer_did: Option<String>,
    pub signature_hex: Option<String>,
    pub document_hash: Option<String>,
    pub issued_at_unix_ms: Option<u128>,
    pub active: bool,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, FromRow)]
struct SkillRow {
    skill_id: String,
    version: String,
    display_name: String,
    description: Option<String>,
    entry_function: String,
    capabilities: String,
    artifact_path: String,
    artifact_sha256: String,
    source_kind: String,
    issuer_did: Option<String>,
    signature_hex: Option<String>,
    document_hash: Option<String>,
    issued_at_unix_ms: Option<i64>,
    active: i64,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillRegistryStatus {
    artifact_root: String,
    total_versions: usize,
    active_versions: usize,
    signed_versions: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSkillRequest {
    pub skill_id: String,
    pub version: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub entry_function: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub wasm_base64: String,
    pub activate: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SignedSkillDocument {
    pub skill_id: String,
    pub version: String,
    pub display_name: String,
    pub description: Option<String>,
    pub entry_function: String,
    pub capabilities: Vec<String>,
    pub artifact_sha256: String,
    pub issuer_did: String,
    pub issued_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SignedSkillEnvelope {
    pub document: SignedSkillDocument,
    pub signature_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSignedSkillRequest {
    pub envelope: SignedSkillEnvelope,
    pub wasm_base64: String,
    pub activate: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLookupResponse {
    pub active: Option<SkillRecord>,
    pub versions: Vec<SkillRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillDistributionResponse {
    pub skills: Vec<SkillRecord>,
    pub active_versions: usize,
    pub signed_versions: usize,
    pub trusted_publishers: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillPackageResponse {
    pub skill: SkillRecord,
    pub envelope: Option<SignedSkillEnvelope>,
    pub wasm_base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSkillPackageRequest {
    pub package_url: String,
    pub activate: Option<bool>,
    pub allow_unsigned: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillIntakeRequest {
    pub source_url: String,
    pub source_kind_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillIntakeProposalRequest {
    pub source_url: String,
    pub source_kind_hint: Option<String>,
    pub actor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillIntakeResponse {
    pub source_url: String,
    pub content_type: Option<String>,
    pub source_sha256: Option<String>,
    pub source_preview: Option<String>,
    pub detected_kind: String,
    pub confidence: f32,
    pub installability: String,
    pub direct_install_url: Option<String>,
    pub conversion_required: bool,
    pub requires_trusted_publisher: bool,
    pub allow_unsigned_supported: bool,
    pub recommended_action: String,
    pub findings: Vec<String>,
    pub warnings: Vec<String>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillIntakeProposalResponse {
    pub intake: SkillIntakeResponse,
    pub proposal: Option<SkillProposalRecord>,
    pub created: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillActivationResponse {
    pub skill: SkillRecord,
    pub activated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublisherTrustRootUpsertRequest {
    pub actor: String,
    pub reason: String,
    pub issuer_did: String,
    pub label: String,
    pub public_key_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublisherTrustRootUpsertResponse {
    pub trust_root: SkillPublisherTrustRootRecord,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/distribution", get(distribution))
        .route("/", get(list_skills))
        .route("/register", post(register_skill))
        .route("/register/signed", post(register_signed_skill))
        .route("/install", post(install_skill_package))
        .route("/intake", post(intake_skill_source))
        .route("/intake/proposal", post(propose_skill_from_intake))
        .route(
            "/trust-roots",
            get(list_skill_publisher_trust_roots).post(upsert_skill_publisher_trust_root),
        )
        .route("/:skill_id", get(get_skill_versions))
        .route("/:skill_id/:version", get(get_skill_version))
        .route("/:skill_id/:version/package", get(get_skill_package))
        .route("/:skill_id/:version/activate", post(activate_skill_version))
}

pub async fn find_skill(
    state: &AppState,
    skill_id: &str,
    version: Option<&str>,
) -> anyhow::Result<Option<SkillRecord>> {
    let row = if let Some(version) = version {
        sqlx::query_as::<_, SkillRow>(
            r#"
            SELECT
                skill_id,
                version,
                display_name,
                description,
                entry_function,
                capabilities,
                artifact_path,
                artifact_sha256,
                source_kind,
                issuer_did,
                signature_hex,
                document_hash,
                issued_at_unix_ms,
                active,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM wasm_skills
            WHERE skill_id = ?1 AND version = ?2
            "#,
        )
        .bind(skill_id)
        .bind(version)
        .fetch_optional(state.pool())
        .await
        .with_context(|| format!("failed to fetch skill {skill_id}@{version}"))?
    } else {
        sqlx::query_as::<_, SkillRow>(
            r#"
            SELECT
                skill_id,
                version,
                display_name,
                description,
                entry_function,
                capabilities,
                artifact_path,
                artifact_sha256,
                source_kind,
                issuer_did,
                signature_hex,
                document_hash,
                issued_at_unix_ms,
                active,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM wasm_skills
            WHERE skill_id = ?1 AND active = 1
            ORDER BY updated_at_unix_ms DESC
            LIMIT 1
            "#,
        )
        .bind(skill_id)
        .fetch_optional(state.pool())
        .await
        .with_context(|| format!("failed to fetch active skill {skill_id}"))?
    };

    if let Some(skill) = row.map(skill_from_row).transpose()? {
        return Ok(Some(skill));
    }
    native_builtin_skill(skill_id, version)
}

pub async fn current_distribution(
    state: &Arc<AppState>,
) -> anyhow::Result<SkillDistributionResponse> {
    let skills = list_skill_records(state).await?;
    let active_versions = skills.iter().filter(|skill| skill.active).count();
    let signed_versions = skills
        .iter()
        .filter(|skill| skill.signature_hex.is_some() && skill.issuer_did.is_some())
        .count();
    let trusted_publishers = state.list_skill_publisher_trust_roots().await?.len();
    Ok(SkillDistributionResponse {
        skills,
        active_versions,
        signed_versions,
        trusted_publishers,
    })
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SkillRegistryStatus>, (StatusCode, Json<Value>)> {
    let skills = list_skill_records(&state).await.map_err(internal_error)?;
    let active_versions = skills.iter().filter(|skill| skill.active).count();
    let signed_versions = skills
        .iter()
        .filter(|skill| skill.signature_hex.is_some() && skill.issuer_did.is_some())
        .count();
    Ok(Json(SkillRegistryStatus {
        artifact_root: skill_artifact_root_dir().display().to_string(),
        total_versions: skills.len(),
        active_versions,
        signed_versions,
    }))
}

async fn distribution(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SkillDistributionResponse>, (StatusCode, Json<Value>)> {
    current_distribution(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn list_skills(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SkillRecord>>, (StatusCode, Json<Value>)> {
    list_skill_records(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_skill_versions(
    State(state): State<Arc<AppState>>,
    AxumPath(skill_id): AxumPath<String>,
) -> Result<Json<SkillLookupResponse>, (StatusCode, Json<Value>)> {
    let versions = list_skill_versions(&state, &skill_id)
        .await
        .map_err(internal_error)?;
    if versions.is_empty() {
        return Err(not_found("skill not found"));
    }
    let active = versions.iter().find(|skill| skill.active).cloned();
    Ok(Json(SkillLookupResponse { active, versions }))
}

async fn get_skill_version(
    State(state): State<Arc<AppState>>,
    AxumPath((skill_id, version)): AxumPath<(String, String)>,
) -> Result<Json<SkillRecord>, (StatusCode, Json<Value>)> {
    find_skill(&state, &skill_id, Some(&version))
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("skill version not found"))
}

async fn get_skill_package(
    State(state): State<Arc<AppState>>,
    AxumPath((skill_id, version)): AxumPath<(String, String)>,
) -> Result<Json<SkillPackageResponse>, (StatusCode, Json<Value>)> {
    export_skill_package(&state, &skill_id, &version)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn register_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RegisterSkillRequest>,
) -> Result<Json<SkillActivationResponse>, (StatusCode, Json<Value>)> {
    register_skill_inner(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn register_signed_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RegisterSignedSkillRequest>,
) -> Result<Json<SkillActivationResponse>, (StatusCode, Json<Value>)> {
    register_signed_skill_inner(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn install_skill_package(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallSkillPackageRequest>,
) -> Result<Json<SkillActivationResponse>, (StatusCode, Json<Value>)> {
    install_skill_package_from_url(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn intake_skill_source(
    Json(request): Json<SkillIntakeRequest>,
) -> Result<Json<SkillIntakeResponse>, (StatusCode, Json<Value>)> {
    inspect_skill_source_from_url(request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn propose_skill_from_intake(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SkillIntakeProposalRequest>,
) -> Result<Json<SkillIntakeProposalResponse>, (StatusCode, Json<Value>)> {
    create_skill_intake_proposal(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn activate_skill_version(
    State(state): State<Arc<AppState>>,
    AxumPath((skill_id, version)): AxumPath<(String, String)>,
) -> Result<Json<SkillActivationResponse>, (StatusCode, Json<Value>)> {
    let skill = activate_skill_version_inner(&state, &skill_id, &version)
        .await
        .map_err(internal_error)?;
    Ok(Json(SkillActivationResponse {
        skill,
        activated: true,
    }))
}

pub async fn export_skill_package(
    state: &AppState,
    skill_id: &str,
    version: &str,
) -> anyhow::Result<SkillPackageResponse> {
    let skill = find_skill(state, skill_id, Some(version))
        .await?
        .ok_or_else(|| anyhow!("skill version not found: {skill_id}@{version}"))?;
    if skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND {
        return Ok(SkillPackageResponse {
            skill,
            envelope: None,
            wasm_base64: String::new(),
        });
    }
    let wasm_bytes = fs::read(&skill.artifact_path)
        .await
        .with_context(|| format!("failed to read skill artifact {}", skill.artifact_path))?;
    let wasm_base64 = BASE64_STANDARD.encode(wasm_bytes);
    let envelope = match (&skill.issuer_did, &skill.signature_hex) {
        (Some(issuer_did), Some(signature_hex)) => Some(SignedSkillEnvelope {
            document: SignedSkillDocument {
                skill_id: skill.skill_id.clone(),
                version: skill.version.clone(),
                display_name: skill.display_name.clone(),
                description: skill.description.clone(),
                entry_function: skill.entry_function.clone(),
                capabilities: skill.capabilities.clone(),
                artifact_sha256: skill.artifact_sha256.clone(),
                issuer_did: issuer_did.clone(),
                issued_at_unix_ms: skill.issued_at_unix_ms.unwrap_or(skill.created_at_unix_ms),
            },
            signature_hex: signature_hex.clone(),
        }),
        _ => None,
    };
    Ok(SkillPackageResponse {
        skill,
        envelope,
        wasm_base64,
    })
}

pub async fn install_skill_package_from_url(
    state: &Arc<AppState>,
    request: InstallSkillPackageRequest,
) -> anyhow::Result<SkillActivationResponse> {
    let package_url =
        crate::security::validate_public_http_url(&request.package_url, "packageUrl")?;
    let package_url_display = package_url.to_string();
    let package = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?
        .get(package_url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .with_context(|| format!("failed to fetch skill package {}", package_url_display))?
        .error_for_status()
        .with_context(|| {
            format!(
                "skill package endpoint returned an error {}",
                package_url_display
            )
        })?;
    let package_body =
        read_limited_response_body(package, SKILL_PACKAGE_MAX_BYTES, "skill package").await?;
    let package = serde_json::from_slice::<SkillPackageResponse>(&package_body)
        .with_context(|| format!("failed to decode skill package {}", package_url_display))?;

    if package.skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND {
        return Ok(SkillActivationResponse {
            skill: package.skill,
            activated: true,
        });
    }
    if let Some(envelope) = package.envelope {
        register_signed_skill_inner(
            state,
            RegisterSignedSkillRequest {
                envelope,
                wasm_base64: package.wasm_base64,
                activate: request.activate,
            },
        )
        .await
    } else if request.allow_unsigned.unwrap_or(false) {
        register_skill_inner(
            state,
            RegisterSkillRequest {
                skill_id: package.skill.skill_id,
                version: package.skill.version,
                display_name: Some(package.skill.display_name),
                description: package.skill.description,
                entry_function: Some(package.skill.entry_function),
                capabilities: Some(package.skill.capabilities),
                wasm_base64: package.wasm_base64,
                activate: request.activate,
            },
        )
        .await
    } else {
        anyhow::bail!("remote skill package is unsigned; set allowUnsigned=true to install it")
    }
}

pub async fn inspect_skill_source_from_url(
    request: SkillIntakeRequest,
) -> anyhow::Result<SkillIntakeResponse> {
    let source_url = crate::security::validate_public_http_url(&request.source_url, "sourceUrl")?;
    let source_url_display = source_url.to_string();
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(12))
        .build()?
        .get(source_url.clone())
        .header(
            ACCEPT,
            "application/json, text/markdown, text/plain, text/x-python, text/x-toml, */*;q=0.2",
        )
        .send()
        .await
        .with_context(|| format!("failed to fetch skill intake source {source_url_display}"))?
        .error_for_status()
        .with_context(|| {
            format!("skill intake source endpoint returned an error {source_url_display}")
        })?;

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body =
        read_limited_response_body(response, SKILL_INTAKE_MAX_BYTES, "skill intake source").await?;

    let source_sha256 = hex::encode(Sha256::digest(&body));
    let text = String::from_utf8_lossy(&body);
    Ok(inspect_skill_source_text_with_evidence(
        &source_url_display,
        content_type,
        &text,
        request.source_kind_hint.as_deref(),
        source_sha256,
        skill_source_preview(&text),
    ))
}

async fn read_limited_response_body(
    response: reqwest::Response,
    max_bytes: usize,
    label: &str,
) -> anyhow::Result<Vec<u8>> {
    if let Some(length) = response.content_length() {
        if length > max_bytes as u64 {
            anyhow::bail!("{label} is too large: {length} bytes exceeds {max_bytes}");
        }
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("failed while reading {label}"))?;
        if body.len() + chunk.len() > max_bytes {
            anyhow::bail!("{label} exceeded {max_bytes} bytes while reading");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn create_skill_intake_proposal(
    state: &Arc<AppState>,
    request: SkillIntakeProposalRequest,
) -> anyhow::Result<SkillIntakeProposalResponse> {
    let intake = inspect_skill_source_from_url(SkillIntakeRequest {
        source_url: request.source_url,
        source_kind_hint: request.source_kind_hint,
    })
    .await?;

    if !intake.conversion_required {
        return Ok(SkillIntakeProposalResponse {
            intake,
            proposal: None,
            created: false,
            message:
                "No conversion proposal was created because this source is directly installable or already available."
                    .to_string(),
        });
    }

    let proposal_key = skill_intake_proposal_key(&intake);
    if let Some(existing) = state.get_skill_proposal_by_key(&proposal_key).await? {
        return Ok(SkillIntakeProposalResponse {
            intake,
            proposal: Some(existing),
            created: false,
            message: "Existing conversion proposal returned for this source URL and content hash."
                .to_string(),
        });
    }

    let proposal = build_skill_intake_conversion_proposal(
        &intake,
        request.actor.as_deref(),
        Uuid::new_v4(),
        unix_timestamp_ms(),
    )
    .ok_or_else(|| anyhow!("intake source did not require conversion"))?;
    let proposal = state.upsert_skill_proposal(proposal).await?;
    Ok(SkillIntakeProposalResponse {
        intake,
        proposal: Some(proposal),
        created: true,
        message: "Created a reviewed conversion proposal for this online skill source.".to_string(),
    })
}

#[cfg(test)]
fn inspect_skill_source_text(
    source_url: &str,
    content_type: Option<String>,
    text: &str,
    source_kind_hint: Option<&str>,
) -> SkillIntakeResponse {
    inspect_skill_source_text_with_evidence(
        source_url,
        content_type,
        text,
        source_kind_hint,
        hex::encode(Sha256::digest(text.as_bytes())),
        skill_source_preview(text),
    )
}

fn inspect_skill_source_text_with_evidence(
    source_url: &str,
    content_type: Option<String>,
    text: &str,
    source_kind_hint: Option<&str>,
    source_sha256: String,
    source_preview: Option<String>,
) -> SkillIntakeResponse {
    let mut response = classify_skill_source_text(source_url, content_type, text, source_kind_hint);
    response.source_sha256 = Some(source_sha256);
    response.source_preview = source_preview;
    response
}

fn classify_skill_source_text(
    source_url: &str,
    content_type: Option<String>,
    text: &str,
    source_kind_hint: Option<&str>,
) -> SkillIntakeResponse {
    let hint = source_kind_hint
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let parsed_json = serde_json::from_str::<Value>(text).ok();
    if let Some(value) = parsed_json.as_ref() {
        if looks_like_dawn_skill_package(value) {
            return dawn_skill_package_intake_response(source_url, content_type, value);
        }
        if looks_like_marketplace_catalog(value) {
            return skill_intake_response(
                source_url,
                content_type,
                "dawn_marketplace_catalog",
                0.96,
                "catalog_select_then_install",
                None,
                true,
                false,
                false,
                "Search the catalog and install a selected signed skill entry through `dawn-node skills install --federated` or the local marketplace UI.",
                vec!["The source exposes a Dawn marketplace catalog.".to_string()],
                vec![],
                vec![
                    "Add or enable the marketplace peer if this catalog is remote.".to_string(),
                    "Run `dawn-node skills search <query> --federated` to select a concrete skill package.".to_string(),
                ],
            );
        }
        if looks_like_browser_extension_manifest(value) {
            return skill_intake_response(
                source_url,
                content_type,
                "browser_extension",
                0.92,
                "conversion_required",
                None,
                true,
                false,
                false,
                "Convert the extension into a reviewed Dawn browser-control skill or keep it as a browser extension managed outside the skill registry.",
                vec!["The source looks like a browser extension manifest.".to_string()],
                vec![
                    "Browser extensions are not safe to install as Dawn Wasm skills directly."
                        .to_string(),
                ],
                vec![
                    "Review requested browser permissions.".to_string(),
                    "Wrap only the required workflow as a signed Dawn skill or native workflow."
                        .to_string(),
                ],
            );
        }
        if looks_like_mcp_package_json(value) {
            return skill_intake_response(
                source_url,
                content_type,
                "mcp_server_project",
                0.94,
                "conversion_required",
                None,
                true,
                false,
                false,
                "Create a Dawn adapter that runs this MCP server through an approved connector boundary, then publish the adapter as a signed skill.",
                vec!["The source looks like a Node package for an MCP server.".to_string()],
                vec!["MCP servers are long-running tools, not portable Wasm skills.".to_string()],
                vec![
                    "Pin dependencies and define allowed tools/resources.".to_string(),
                    "Add sandbox startup and health checks.".to_string(),
                    "Publish a signed Dawn wrapper skill after review.".to_string(),
                ],
            );
        }
    }

    let normalized_url = source_url.to_ascii_lowercase();
    let normalized_text = text.to_ascii_lowercase();
    if hint.as_deref() == Some("codex-skill")
        || normalized_url.ends_with("/skill.md")
        || looks_like_codex_skill_markdown(text)
    {
        return skill_intake_response(
            source_url,
            content_type,
            "codex_skill_markdown",
            0.9,
            "conversion_required",
            None,
            true,
            false,
            false,
            "Convert this Codex SKILL.md into a Dawn native workflow or a signed Wasm skill package before activation.",
            vec!["The source looks like a Codex-style SKILL.md instruction file.".to_string()],
            vec!["Instruction files may contain operational guidance but are not executable Dawn skill artifacts.".to_string()],
            vec![
                "Extract allowed commands, inputs, outputs, and safety constraints.".to_string(),
                "Generate tests for the workflow.".to_string(),
                "Package the result as a signed Dawn skill or native builtin proposal.".to_string(),
            ],
        );
    }
    if hint.as_deref() == Some("mcp")
        || normalized_text.contains("@modelcontextprotocol/sdk")
        || normalized_text.contains("model context protocol")
    {
        return skill_intake_response(
            source_url,
            content_type,
            "mcp_server_project",
            0.82,
            "conversion_required",
            None,
            true,
            false,
            false,
            "Wrap this MCP project behind a Dawn-approved connector boundary before making it available as an agent skill.",
            vec!["The source references MCP.".to_string()],
            vec!["MCP projects need runtime supervision and tool allowlisting.".to_string()],
            vec![
                "Inspect the exposed MCP tools and resource templates.".to_string(),
                "Define a least-privilege Dawn skill wrapper.".to_string(),
            ],
        );
    }
    if hint.as_deref() == Some("python")
        || normalized_url.ends_with("pyproject.toml")
        || normalized_url.ends_with("setup.py")
        || normalized_url.ends_with("requirements.txt")
        || normalized_text.contains("[project]")
        || normalized_text.contains("setup(")
    {
        return skill_intake_response(
            source_url,
            content_type,
            "python_tooling_project",
            0.82,
            "conversion_required",
            None,
            true,
            false,
            false,
            "Build a sandboxed Python runner or compile a narrow Wasm-compatible wrapper instead of installing this project directly.",
            vec!["The source looks like Python tooling or package metadata.".to_string()],
            vec!["Python packages can execute arbitrary install-time or runtime code.".to_string()],
            vec![
                "Pin dependencies and choose a sandbox profile.".to_string(),
                "Expose a small JSON input/output contract.".to_string(),
                "Require review before activation.".to_string(),
            ],
        );
    }
    if normalized_url.contains("github.com/") || normalized_url.contains("gitlab.com/") {
        return skill_intake_response(
            source_url,
            content_type,
            "git_repository",
            0.72,
            "conversion_required",
            None,
            true,
            false,
            false,
            "Inspect repository contents and generate a Dawn skill proposal; do not install repository code directly.",
            vec!["The source is a Git hosting URL.".to_string()],
            vec!["Repository pages are not Dawn skill packages and may contain multiple unrelated projects.".to_string()],
            vec![
                "Locate a Dawn package, SKILL.md, MCP manifest, pyproject.toml, or plugin manifest inside the repository.".to_string(),
                "Run intake again on the exact raw manifest or package URL.".to_string(),
            ],
        );
    }

    skill_intake_response(
        source_url,
        content_type,
        "unknown_online_resource",
        0.35,
        "unsupported_unknown",
        None,
        true,
        false,
        false,
        "Manual review is required before this source can become a Dawn skill.",
        vec!["The source did not match a known skill package or adapter pattern.".to_string()],
        vec!["No automatic install path is available for this source.".to_string()],
        vec![
            "Provide a Dawn SkillPackage JSON URL for direct install.".to_string(),
            "Or provide a raw SKILL.md, package.json, pyproject.toml, or browser manifest for conversion planning.".to_string(),
        ],
    )
}

fn dawn_skill_package_intake_response(
    source_url: &str,
    content_type: Option<String>,
    value: &Value,
) -> SkillIntakeResponse {
    let skill_id = value
        .pointer("/skill/skillId")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let version = value
        .pointer("/skill/version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if value
        .pointer("/skill/sourceKind")
        .and_then(Value::as_str)
        .is_some_and(|source_kind| source_kind == NATIVE_BUILTIN_SOURCE_KIND)
    {
        return skill_intake_response(
            source_url,
            content_type,
            "dawn_native_builtin_reference",
            0.99,
            "already_available",
            Some(source_url.to_string()),
            false,
            false,
            false,
            "No install is required; this native Dawn skill is already provided by the local gateway build.",
            vec![format!(
                "The source references Dawn native builtin skill {skill_id}@{version}."
            )],
            vec![],
            vec!["Run `dawn-node skills search <skill-id> --all` or use `/skills` from chat to confirm availability.".to_string()],
        );
    }
    let signed = value.get("envelope").is_some_and(|value| !value.is_null());
    if signed {
        skill_intake_response(
            source_url,
            content_type,
            "dawn_signed_wasm_skill_package",
            0.99,
            "direct_install",
            Some(source_url.to_string()),
            false,
            true,
            false,
            "Install with `dawn-node skills install-url <package-url>` after the publisher trust root is configured.",
            vec![format!(
                "The source is a Dawn signed Wasm skill package for {skill_id}@{version}."
            )],
            vec![],
            vec![
                "Confirm the skill publisher trust root exists locally.".to_string(),
                "Run `dawn-node skills install-url <package-url>`.".to_string(),
            ],
        )
    } else {
        skill_intake_response(
            source_url,
            content_type,
            "dawn_unsigned_wasm_skill_package",
            0.96,
            "direct_install_requires_allow_unsigned",
            Some(source_url.to_string()),
            false,
            false,
            true,
            "Install only for development with `dawn-node skills install-url <package-url> --allow-unsigned`.",
            vec![format!(
                "The source is an unsigned Dawn Wasm skill package for {skill_id}@{version}."
            )],
            vec!["Unsigned remote skills should not be activated for normal users.".to_string()],
            vec![
                "Prefer asking the publisher for a signed package.".to_string(),
                "Use `--allow-unsigned` only in a development or sandbox profile.".to_string(),
            ],
        )
    }
}

fn skill_intake_response(
    source_url: &str,
    content_type: Option<String>,
    detected_kind: &str,
    confidence: f32,
    installability: &str,
    direct_install_url: Option<String>,
    conversion_required: bool,
    requires_trusted_publisher: bool,
    allow_unsigned_supported: bool,
    recommended_action: &str,
    findings: Vec<String>,
    warnings: Vec<String>,
    next_steps: Vec<String>,
) -> SkillIntakeResponse {
    SkillIntakeResponse {
        source_url: source_url.to_string(),
        content_type,
        source_sha256: None,
        source_preview: None,
        detected_kind: detected_kind.to_string(),
        confidence,
        installability: installability.to_string(),
        direct_install_url,
        conversion_required,
        requires_trusted_publisher,
        allow_unsigned_supported,
        recommended_action: recommended_action.to_string(),
        findings,
        warnings,
        next_steps,
    }
}

fn build_skill_intake_conversion_proposal(
    intake: &SkillIntakeResponse,
    actor: Option<&str>,
    proposal_id: Uuid,
    now: u128,
) -> Option<SkillProposalRecord> {
    if !intake.conversion_required {
        return None;
    }
    let suggested_skill_id = suggested_skill_id_for_intake(intake);
    let risk_level = intake_conversion_risk_level(&intake.detected_kind).to_string();
    let proposal_key = skill_intake_proposal_key(intake);
    let conversion_spec = conversion_spec_for_intake(intake);
    let tags = vec![
        "skill-intake".to_string(),
        "conversion-required".to_string(),
        intake.detected_kind.clone(),
        intake.installability.clone(),
    ];
    Some(SkillProposalRecord {
        proposal_id,
        proposal_key,
        title: format!("Convert {} into a reviewed Dawn skill", intake.detected_kind),
        summary: format!(
            "Online source `{}` was classified as `{}` and requires a reviewed conversion before it can run as a Dawn skill.",
            truncate_for_proposal(&intake.source_url, 160),
            intake.detected_kind
        ),
        rationale: "This source is not a directly installable Dawn signed Wasm package. Convert it through a reviewed plan with sandbox tests, a narrow JSON contract, publisher signing, and explicit activation approval.".to_string(),
        suggested_skill_id,
        source: "skill-intake".to_string(),
        status: "proposed".to_string(),
        confidence: f64::from(intake.confidence),
        evidence: json!({
            "intake": intake,
            "conversionPlan": {
                "planKind": "online_skill_source_conversion",
                "sourceKind": intake.detected_kind,
                "installability": intake.installability,
                "recommendedAction": intake.recommended_action,
                "requiredSteps": intake.next_steps,
                "warnings": intake.warnings,
                "conversionSpec": conversion_spec,
                "guardrails": [
                    "no automatic activation",
                    "sandbox tests required",
                    "signed package or native builtin review required",
                    "least privilege input/output contract required"
                ]
            }
        }),
        tags,
        risk_level,
        created_by: normalized_actor(actor),
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    })
}

fn skill_intake_proposal_key(intake: &SkillIntakeResponse) -> String {
    let mut key_material = intake.source_url.trim().to_string();
    key_material.push('\n');
    key_material.push_str(
        intake
            .source_sha256
            .as_deref()
            .unwrap_or("missing-source-sha256"),
    );
    let digest = Sha256::digest(key_material.as_bytes());
    format!("skill-intake:{}", hex::encode(digest))
}

fn suggested_skill_id_for_intake(intake: &SkillIntakeResponse) -> String {
    let source_part = reqwest::Url::parse(&intake.source_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|segments| {
                    segments
                        .rev()
                        .find(|segment| !segment.trim().is_empty())
                        .map(str::to_string)
                })
                .or_else(|| url.host_str().map(str::to_string))
        })
        .unwrap_or_else(|| "online-source".to_string());
    format!(
        "import.{}.{}",
        slugify_skill_component(&intake.detected_kind),
        slugify_skill_component(&source_part)
    )
}

fn slugify_skill_component(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_separator = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_separator = false;
        } else if !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "source".to_string()
    } else {
        slug.to_string()
    }
}

fn skill_source_preview(text: &str) -> Option<String> {
    let preview = text
        .chars()
        .map(|ch| match ch {
            '\r' | '\n' | '\t' => ' ',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .take(80)
        .collect::<Vec<_>>()
        .join(" ");
    if preview.is_empty() {
        None
    } else {
        Some(preview.chars().take(800).collect())
    }
}

fn intake_conversion_risk_level(detected_kind: &str) -> &'static str {
    match detected_kind {
        "unknown_online_resource" => "high",
        "browser_extension"
        | "mcp_server_project"
        | "python_tooling_project"
        | "git_repository" => "guarded",
        _ => "guarded",
    }
}

fn conversion_spec_for_intake(intake: &SkillIntakeResponse) -> Value {
    let common = json!({
        "sourceKind": intake.detected_kind,
        "sourceUrl": intake.source_url,
        "installability": intake.installability,
        "reviewRequired": true,
        "automaticActivation": false,
        "artifactTargets": [
            "reviewed_native_skill_draft",
            "signed_wasm_skill_package"
        ],
        "sharedGuardrails": [
            "do not execute fetched source code during intake or planning",
            "do not install dependencies globally",
            "preserve chat pairing, signature validation, and approval gates",
            "require sandbox tests before activation",
            "require trusted publisher signature before normal distribution"
        ]
    });
    let mut spec = match intake.detected_kind.as_str() {
        "codex_skill_markdown" => json!({
            "adapterKind": "codex_skill_markdown_adapter",
            "adapterGoal": "Translate Codex SKILL.md operational instructions into a Dawn native workflow draft, then optionally package a narrow Wasm wrapper after tests.",
            "extractionTargets": [
                "skill purpose and trigger phrases",
                "allowed commands or APIs",
                "required local files and environment variables",
                "forbidden actions and approval boundaries",
                "expected input and output examples"
            ],
            "inputContract": {
                "required": ["instruction"],
                "optional": ["arguments", "workspaceContext"]
            },
            "runtimeBoundary": "Dawn executes only reviewed adapter commands; SKILL.md text remains documentation and is never executed directly.",
            "permissionReview": [
                "filesystem reads/writes",
                "network calls",
                "shell commands",
                "desktop control",
                "credential access"
            ],
            "sandboxCases": [
                "valid instruction follows allowed workflow",
                "instruction requesting forbidden command is denied",
                "missing local dependency fails closed",
                "chat ingress regression remains healthy"
            ],
            "packagingPlan": [
                "materialize contract.json and SKILL.md draft",
                "implement reviewed adapter code if automation is needed",
                "pack adapter Wasm with dawn-node skills pack-wasm",
                "trust publisher and install signed package"
            ]
        }),
        "mcp_server_project" => json!({
            "adapterKind": "mcp_supervised_connector_adapter",
            "adapterGoal": "Expose selected MCP tools through a supervised Dawn connector boundary instead of installing the server as a direct skill.",
            "extractionTargets": [
                "server startup command",
                "transport type",
                "tool names and JSON schemas",
                "resource templates",
                "dependency lockfiles",
                "health check endpoint or handshake"
            ],
            "inputContract": {
                "required": ["toolName", "arguments"],
                "optional": ["resourceUri", "timeoutMs"]
            },
            "runtimeBoundary": "Dawn starts or connects to the MCP server only through an approved connector with tool allowlisting and timeout controls.",
            "permissionReview": [
                "allowed MCP tools",
                "allowed resource URI patterns",
                "network binding",
                "process lifetime",
                "dependency install location"
            ],
            "sandboxCases": [
                "allowed tool invocation succeeds",
                "unknown tool is denied",
                "resource outside allowlist is denied",
                "server startup failure is reported safely"
            ],
            "packagingPlan": [
                "generate connector allowlist manifest",
                "generate Dawn wrapper skill contract",
                "add supervised startup and health checks",
                "package wrapper only after dependency pin review"
            ]
        }),
        "python_tooling_project" => json!({
            "adapterKind": "python_sandbox_runner_adapter",
            "adapterGoal": "Wrap selected Python functions or CLI entry points behind a sandboxed Dawn runner with pinned dependencies.",
            "extractionTargets": [
                "pyproject or setup metadata",
                "console_scripts entry points",
                "requirements and lockfiles",
                "filesystem and network usage",
                "sample commands and outputs"
            ],
            "inputContract": {
                "required": ["operation", "arguments"],
                "optional": ["workingDirectory", "timeoutMs"]
            },
            "runtimeBoundary": "Python code runs only in a reviewed sandbox or venv; setup hooks are not executed during intake.",
            "permissionReview": [
                "dependency source and hashes",
                "filesystem scope",
                "network scope",
                "subprocess use",
                "large output and artifact paths"
            ],
            "sandboxCases": [
                "allowed operation succeeds",
                "invalid operation is rejected",
                "dependency missing fails closed",
                "attempted credential read is blocked"
            ],
            "packagingPlan": [
                "generate Python runner manifest",
                "pin dependency hashes",
                "write Dawn wrapper contract",
                "package wrapper after sandbox tests"
            ]
        }),
        "browser_extension" => json!({
            "adapterKind": "browser_control_workflow_adapter",
            "adapterGoal": "Convert extension behavior into explicit Dawn browser-control workflow steps without installing extension code.",
            "extractionTargets": [
                "manifest version",
                "permissions and host_permissions",
                "content script matches",
                "background service worker",
                "commands and user actions"
            ],
            "inputContract": {
                "required": ["browserAction", "target"],
                "optional": ["selectors", "hostPattern", "arguments"]
            },
            "runtimeBoundary": "Dawn uses reviewed browser automation commands; extension scripts are not loaded or executed as trusted skill code.",
            "permissionReview": [
                "host permissions",
                "tab and scripting access",
                "storage access",
                "clipboard access",
                "native messaging"
            ],
            "sandboxCases": [
                "allowed host workflow succeeds",
                "disallowed host is denied",
                "selector not found fails safely",
                "extension-only privileged action is rejected"
            ],
            "packagingPlan": [
                "generate browser workflow contract",
                "replace broad host permissions with explicit URL allowlist",
                "write native skill workflow draft",
                "package wrapper after browser regression tests"
            ]
        }),
        "git_repository" => json!({
            "adapterKind": "repository_probe_adapter",
            "adapterGoal": "Identify a concrete package, SKILL.md, MCP manifest, Python manifest, or browser manifest inside the repository before conversion.",
            "extractionTargets": [
                "repository tree",
                "README usage section",
                "package manifests",
                "license",
                "release artifacts"
            ],
            "inputContract": {
                "required": ["repositoryUrl", "selectedPath"],
                "optional": ["revision"]
            },
            "runtimeBoundary": "Repository code is not executed; rerun intake on the exact selected raw file or package URL.",
            "permissionReview": [
                "selected artifact type",
                "license",
                "dependency sources",
                "publisher identity"
            ],
            "sandboxCases": [
                "repository source selection is deterministic",
                "ambiguous repository requires operator choice",
                "unsafe generated files are rejected"
            ],
            "packagingPlan": [
                "select exact source file or release package",
                "rerun intake on selected raw URL",
                "continue through source-specific adapter plan"
            ]
        }),
        _ => json!({
            "adapterKind": "manual_review_adapter",
            "adapterGoal": "Classify the source and create a narrow Dawn adapter only after the trust boundary is understood.",
            "extractionTargets": [
                "source format",
                "publisher identity",
                "expected runtime",
                "required permissions"
            ],
            "inputContract": {
                "required": ["instruction"],
                "optional": ["arguments"]
            },
            "runtimeBoundary": "No execution until a reviewed adapter and sandbox test plan exist.",
            "permissionReview": [
                "filesystem",
                "network",
                "process execution",
                "desktop control",
                "credentials"
            ],
            "sandboxCases": [
                "manual classification completed",
                "unsafe runtime request is denied"
            ],
            "packagingPlan": [
                "classify exact source",
                "create source-specific adapter plan",
                "package only after tests and signing"
            ]
        }),
    };
    if let (Some(spec_object), Some(common_object)) = (spec.as_object_mut(), common.as_object()) {
        for (key, value) in common_object {
            spec_object
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    spec
}

fn normalized_actor(actor: Option<&str>) -> String {
    actor
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("skill-intake")
        .chars()
        .take(80)
        .collect()
}

fn truncate_for_proposal(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in value.chars().take(max_chars) {
        output.push(ch);
    }
    if value.chars().count() > max_chars {
        output.push('…');
    }
    output
}

fn looks_like_dawn_skill_package(value: &Value) -> bool {
    value.get("wasmBase64").and_then(Value::as_str).is_some()
        && value
            .pointer("/skill/skillId")
            .and_then(Value::as_str)
            .is_some()
        && value
            .pointer("/skill/version")
            .and_then(Value::as_str)
            .is_some()
}

fn looks_like_marketplace_catalog(value: &Value) -> bool {
    value.get("skills").and_then(Value::as_array).is_some()
        && value.get("agentCards").and_then(Value::as_array).is_some()
}

fn looks_like_mcp_package_json(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object
        .get("dependencies")
        .is_some_and(json_has_mcp_dependency)
        || object
            .get("devDependencies")
            .is_some_and(json_has_mcp_dependency)
    {
        return true;
    }
    object
        .get("keywords")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case("mcp"))
            })
        })
}

fn json_has_mcp_dependency(value: &Value) -> bool {
    value.as_object().is_some_and(|dependencies| {
        dependencies
            .keys()
            .any(|key| key == "@modelcontextprotocol/sdk" || key.contains("mcp"))
    })
}

fn looks_like_browser_extension_manifest(value: &Value) -> bool {
    value
        .get("manifest_version")
        .and_then(Value::as_i64)
        .is_some()
        && (value.get("permissions").is_some()
            || value.get("background").is_some()
            || value.get("content_scripts").is_some())
}

fn looks_like_codex_skill_markdown(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("---")
        && text.contains("\nname:")
        && text.contains("\ndescription:")
        && text.contains("#")
}

async fn list_skill_publisher_trust_roots(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SkillPublisherTrustRootRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_skill_publisher_trust_roots()
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn upsert_skill_publisher_trust_root(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SkillPublisherTrustRootUpsertRequest>,
) -> Result<Json<SkillPublisherTrustRootUpsertResponse>, (StatusCode, Json<Value>)> {
    upsert_skill_publisher_trust_root_inner(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

struct SkillRegistrationSpec {
    skill_id: String,
    version: String,
    display_name: String,
    description: Option<String>,
    entry_function: String,
    capabilities: Vec<String>,
    source_kind: String,
    issuer_did: Option<String>,
    signature_hex: Option<String>,
    document_hash: Option<String>,
    issued_at_unix_ms: Option<u128>,
    active: bool,
}

async fn register_skill_inner(
    state: &AppState,
    request: RegisterSkillRequest,
) -> anyhow::Result<SkillActivationResponse> {
    validate_skill_segment(&request.skill_id, "skill_id")?;
    validate_skill_segment(&request.version, "version")?;

    let wasm_bytes = decode_and_validate_wasm(state, &request.wasm_base64)?;
    persist_registered_skill(
        state,
        SkillRegistrationSpec {
            skill_id: request.skill_id,
            version: request.version,
            display_name: request
                .display_name
                .unwrap_or_else(|| "Unnamed Skill".to_string()),
            description: request.description,
            entry_function: request
                .entry_function
                .unwrap_or_else(|| "run_skill".to_string()),
            capabilities: request.capabilities.unwrap_or_default(),
            source_kind: "unsigned_local".to_string(),
            issuer_did: None,
            signature_hex: None,
            document_hash: None,
            issued_at_unix_ms: None,
            active: request.activate.unwrap_or(true),
        },
        &wasm_bytes,
    )
    .await
}

pub(crate) async fn register_signed_skill_inner(
    state: &AppState,
    request: RegisterSignedSkillRequest,
) -> anyhow::Result<SkillActivationResponse> {
    validate_skill_segment(&request.envelope.document.skill_id, "skill_id")?;
    validate_skill_segment(&request.envelope.document.version, "version")?;

    let wasm_bytes = decode_and_validate_wasm(state, &request.wasm_base64)?;
    let computed_artifact_sha256 = hex::encode(Sha256::digest(&wasm_bytes));
    let declared_artifact_sha256 = normalize_hex(&request.envelope.document.artifact_sha256)?;
    if computed_artifact_sha256 != declared_artifact_sha256 {
        anyhow::bail!(
            "signed skill artifact hash mismatch: computed '{}' but envelope declared '{}'",
            computed_artifact_sha256,
            declared_artifact_sha256
        );
    }

    let normalized_signature_hex = normalize_hex(&request.envelope.signature_hex)?;
    let normalized_issuer_did = request.envelope.document.issuer_did.to_ascii_lowercase();
    let trust_root = state
        .get_skill_publisher_trust_root(&normalized_issuer_did)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "skill publisher '{}' is not present in gateway skill trust roots",
                normalized_issuer_did
            )
        })?;

    let normalized_document = SignedSkillDocument {
        skill_id: request.envelope.document.skill_id,
        version: request.envelope.document.version,
        display_name: request.envelope.document.display_name,
        description: request.envelope.document.description,
        entry_function: request.envelope.document.entry_function,
        capabilities: request.envelope.document.capabilities,
        artifact_sha256: declared_artifact_sha256,
        issuer_did: normalized_issuer_did,
        issued_at_unix_ms: request.envelope.document.issued_at_unix_ms,
    };

    let verified_hash = verify_signed_skill_envelope(
        &SignedSkillEnvelope {
            document: normalized_document.clone(),
            signature_hex: normalized_signature_hex.clone(),
        },
        &trust_root,
    )?;

    persist_registered_skill(
        state,
        SkillRegistrationSpec {
            skill_id: normalized_document.skill_id,
            version: normalized_document.version,
            display_name: normalized_document.display_name,
            description: normalized_document.description,
            entry_function: normalized_document.entry_function,
            capabilities: normalized_document.capabilities,
            source_kind: "signed_publisher".to_string(),
            issuer_did: Some(normalized_document.issuer_did),
            signature_hex: Some(normalized_signature_hex),
            document_hash: Some(verified_hash),
            issued_at_unix_ms: Some(normalized_document.issued_at_unix_ms),
            active: request.activate.unwrap_or(true),
        },
        &wasm_bytes,
    )
    .await
}

fn decode_and_validate_wasm(state: &AppState, wasm_base64: &str) -> anyhow::Result<Vec<u8>> {
    let wasm_bytes = BASE64_STANDARD
        .decode(wasm_base64.as_bytes())
        .context("failed to decode wasmBase64")?;
    Module::new(&state.engine, &wasm_bytes).context("registered wasm skill failed validation")?;
    Ok(wasm_bytes)
}

async fn persist_registered_skill(
    state: &AppState,
    spec: SkillRegistrationSpec,
    wasm_bytes: &[u8],
) -> anyhow::Result<SkillActivationResponse> {
    let now = unix_timestamp_ms();
    let artifact_path = persist_skill_artifact(&spec.skill_id, &spec.version, wasm_bytes)
        .await?
        .display()
        .to_string();
    let artifact_sha256 = hex::encode(Sha256::digest(wasm_bytes));
    let active = spec.active;
    let skill = SkillRecord {
        skill_id: spec.skill_id,
        version: spec.version,
        display_name: spec.display_name,
        description: spec.description,
        entry_function: spec.entry_function,
        capabilities: spec.capabilities,
        artifact_path,
        artifact_sha256,
        source_kind: spec.source_kind,
        issuer_did: spec.issuer_did,
        signature_hex: spec.signature_hex,
        document_hash: spec.document_hash,
        issued_at_unix_ms: spec.issued_at_unix_ms,
        active,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };

    save_skill_record(state, &skill).await?;
    let resolved = find_skill(state, &skill.skill_id, Some(&skill.version))
        .await?
        .ok_or_else(|| anyhow!("skill disappeared after registration"))?;
    Ok(SkillActivationResponse {
        skill: resolved,
        activated: active,
    })
}

pub(crate) async fn upsert_skill_publisher_trust_root_inner(
    state: &Arc<AppState>,
    request: SkillPublisherTrustRootUpsertRequest,
) -> anyhow::Result<SkillPublisherTrustRootUpsertResponse> {
    let public_key_hex = normalize_hex(&request.public_key_hex)?;
    validate_skill_publisher_issuer_did(&request.issuer_did, &public_key_hex)?;

    let existing = state
        .get_skill_publisher_trust_root(&request.issuer_did)
        .await?;
    let now = unix_timestamp_ms();
    let trust_root = SkillPublisherTrustRootRecord {
        issuer_did: request.issuer_did.to_ascii_lowercase(),
        label: request.label,
        public_key_hex,
        updated_by: request.actor,
        updated_reason: request.reason,
        created_at_unix_ms: existing
            .as_ref()
            .map(|record| record.created_at_unix_ms)
            .unwrap_or(now),
        updated_at_unix_ms: now,
    };

    let trust_root = state.save_skill_publisher_trust_root(&trust_root).await?;
    Ok(SkillPublisherTrustRootUpsertResponse { trust_root })
}

async fn activate_skill_version_inner(
    state: &AppState,
    skill_id: &str,
    version: &str,
) -> anyhow::Result<SkillRecord> {
    let Some(skill) = find_skill(state, skill_id, Some(version)).await? else {
        anyhow::bail!("skill version not found: {skill_id}@{version}");
    };
    if skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND {
        return Ok(skill);
    }

    sqlx::query(
        r#"
        UPDATE wasm_skills
        SET active = CASE WHEN version = ?2 THEN 1 ELSE 0 END,
            updated_at_unix_ms = ?3
        WHERE skill_id = ?1
        "#,
    )
    .bind(skill_id)
    .bind(version)
    .bind(unix_timestamp_ms() as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to activate skill {skill_id}@{version}"))?;

    find_skill(state, skill_id, Some(version))
        .await?
        .ok_or_else(|| anyhow!("skill disappeared after activation"))
        .map(|mut record| {
            record.active = true;
            record
        })
}

async fn save_skill_record(state: &AppState, skill: &SkillRecord) -> anyhow::Result<()> {
    if skill.active {
        sqlx::query(
            r#"
            UPDATE wasm_skills
            SET active = 0
            WHERE skill_id = ?1
            "#,
        )
        .bind(&skill.skill_id)
        .execute(state.pool())
        .await
        .with_context(|| {
            format!(
                "failed to clear active skill versions for {}",
                skill.skill_id
            )
        })?;
    }

    sqlx::query(
        r#"
        INSERT INTO wasm_skills (
            skill_id,
            version,
            display_name,
            description,
            entry_function,
            capabilities,
            artifact_path,
            artifact_sha256,
            source_kind,
            issuer_did,
            signature_hex,
            document_hash,
            issued_at_unix_ms,
            active,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ON CONFLICT(skill_id, version) DO UPDATE SET
            display_name = excluded.display_name,
            description = excluded.description,
            entry_function = excluded.entry_function,
            capabilities = excluded.capabilities,
            artifact_path = excluded.artifact_path,
            artifact_sha256 = excluded.artifact_sha256,
            source_kind = excluded.source_kind,
            issuer_did = excluded.issuer_did,
            signature_hex = excluded.signature_hex,
            document_hash = excluded.document_hash,
            issued_at_unix_ms = excluded.issued_at_unix_ms,
            active = excluded.active,
            created_at_unix_ms = wasm_skills.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&skill.skill_id)
    .bind(&skill.version)
    .bind(&skill.display_name)
    .bind(&skill.description)
    .bind(&skill.entry_function)
    .bind(serde_json::to_string(&skill.capabilities)?)
    .bind(&skill.artifact_path)
    .bind(&skill.artifact_sha256)
    .bind(&skill.source_kind)
    .bind(&skill.issuer_did)
    .bind(&skill.signature_hex)
    .bind(&skill.document_hash)
    .bind(skill.issued_at_unix_ms.map(|value| value as i64))
    .bind(skill.active)
    .bind(skill.created_at_unix_ms as i64)
    .bind(skill.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to save skill {}@{}", skill.skill_id, skill.version))?;

    Ok(())
}

async fn list_skill_records(state: &AppState) -> anyhow::Result<Vec<SkillRecord>> {
    let rows = sqlx::query_as::<_, SkillRow>(
        r#"
        SELECT
            skill_id,
            version,
            display_name,
            description,
            entry_function,
            capabilities,
            artifact_path,
            artifact_sha256,
            source_kind,
            issuer_did,
            signature_hex,
            document_hash,
            issued_at_unix_ms,
            active,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM wasm_skills
        ORDER BY skill_id ASC, active DESC, updated_at_unix_ms DESC, version DESC
        "#,
    )
    .fetch_all(state.pool())
    .await
    .context("failed to list wasm skills")?;

    let mut skills = rows
        .into_iter()
        .map(skill_from_row)
        .collect::<anyhow::Result<Vec<_>>>()?;
    for native in native_builtin_skills()? {
        if !skills
            .iter()
            .any(|skill| skill.skill_id == native.skill_id && skill.version == native.version)
        {
            skills.push(native);
        }
    }
    skills.sort_by(|left, right| {
        left.skill_id
            .cmp(&right.skill_id)
            .then_with(|| right.active.cmp(&left.active))
            .then_with(|| right.updated_at_unix_ms.cmp(&left.updated_at_unix_ms))
            .then_with(|| right.version.cmp(&left.version))
    });
    Ok(skills)
}

async fn list_skill_versions(state: &AppState, skill_id: &str) -> anyhow::Result<Vec<SkillRecord>> {
    let rows = sqlx::query_as::<_, SkillRow>(
        r#"
        SELECT
            skill_id,
            version,
            display_name,
            description,
            entry_function,
            capabilities,
            artifact_path,
            artifact_sha256,
            source_kind,
            issuer_did,
            signature_hex,
            document_hash,
            issued_at_unix_ms,
            active,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM wasm_skills
        WHERE skill_id = ?1
        ORDER BY active DESC, updated_at_unix_ms DESC, version DESC
        "#,
    )
    .bind(skill_id)
    .fetch_all(state.pool())
    .await
    .with_context(|| format!("failed to list versions for skill {skill_id}"))?;

    let mut skills = rows
        .into_iter()
        .map(skill_from_row)
        .collect::<anyhow::Result<Vec<_>>>()?;
    if let Some(native) = native_builtin_skill(skill_id, None)? {
        if !skills
            .iter()
            .any(|skill| skill.skill_id == native.skill_id && skill.version == native.version)
        {
            skills.push(native);
        }
    }
    skills.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| right.updated_at_unix_ms.cmp(&left.updated_at_unix_ms))
            .then_with(|| right.version.cmp(&left.version))
    });
    Ok(skills)
}

pub fn is_native_builtin_skill(skill: &SkillRecord) -> bool {
    skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND
}

pub fn native_builtin_skill_usage(skill_id: &str) -> Option<String> {
    match skill_id {
        "agent-card-discoverer" => Some(
            "这是 Dawn 的原生技能 `Agent Card Discoverer`，默认可用，不需要安装。\n\n本机使用方式：\n- CLI: `dawn.cmd agents search <关键词> --federated`\n- /app: 打开 Agent Cards 面板或 Command Studio\n- 聊天: 使用 `/skills` 查看技能，再使用 `/status`、`/task`、`/delegate` 组合运营 Agent Card 流程\n\n它的职责是帮助你发现、筛选和验证 A2A Agent Card，而不是执行 Wasm。".to_string(),
        ),
        "bayesian-skill-set" => Some(
            "这是 Dawn 的原生技能 `Bayesian Skill Set`，默认可用，不需要安装。\n\n本机使用方式：\n- 聊天/WorkBench: `#chat` / `#observe` / `#assist` / `#autopilot`\n- /app Command Studio: 用它来切档、观察和规划下一步\n- CLI: 结合 `dawn.cmd doctor --deep`、`dawn.cmd status` 和现有对话链做不确定性收敛\n\n它的职责是做不确定场景下的分级决策和下一步规划，而不是执行 Wasm。".to_string(),
        ),
        "dawn-orchestrator" => Some(
            "这是 Dawn 的原生技能 `Dawn Orchestrator`，默认可用，不需要安装。\n\n本机使用方式：\n- 聊天: `/task`、`/delegate`、`/status`\n- /app: Command Studio 和任务工作台\n- A2A: 由网关把用户意图转成任务、子任务和委托链路\n\n它的职责是做任务编排和执行落地，而不是执行 Wasm。".to_string(),
        ),
        "dawn-chat-bridge" => Some(
            "这是 Dawn 的原生技能 `Dawn Chat Bridge`，默认可用，不需要安装。\n\n本机使用方式：\n- 聊天平台: Telegram、Slack、Discord、Signal、飞书、钉钉、企微、QQ Bot\n- 命令入口: `/help`、`/skills`、`/status`\n- 网关: 统一接收入站消息并把结果回写到原通道\n\n它的职责是做多通道消息归一化和回传，而不是执行 Wasm。".to_string(),
        ),
        "dawn-desktop-control" => Some(
            "这是 Dawn 的原生技能 `Dawn Desktop Control`，默认可用，不需要安装。\n\n本机使用方式：\n- 聊天: `#assist` 预览，`#autopilot` 后发送 `看一下屏幕`、`鼠标位置`、`移动鼠标到 400,300`、`点击 400,300`\n- /console: Node Command Console 和 Approval Center\n- CLI: `dawn-node node-command dispatch --type desktop_mouse_click --payload '{\"x\":400,\"y\":300,\"button\":\"left\"}'`\n\n它的职责是把手机端聊天意图安全地桥接到桌面观察和鼠标动作；桌面动作仍然走节点能力、可信 attestation 和审批链。".to_string(),
        ),
        "dawn-model-router" => Some(
            "这是 Dawn 的原生技能 `Dawn Model Router`，默认可用，不需要安装。\n\n本机使用方式：\n- 聊天: `/model`\n- CLI: `dawn.ps1 connectors status`、`dawn.ps1 models test ollama`\n- 工作流: `model_connector` 步骤统一接入云模型和本地模型\n\n它的职责是做模型路由和连接器落点控制，而不是执行 Wasm。".to_string(),
        ),
        "dawn-node-operator" => Some(
            "这是 Dawn 的原生技能 `Dawn Node Operator`，默认可用，不需要安装。\n\n本机使用方式：\n- CLI: `dawn.ps1 status`、`dawn-node setup`、`dawn.cmd doctor --deep`\n- /console: 查看节点状态、工作区和 rollout 执行面\n- 本机: 负责把桌面节点持续挂到 Dawn 网关\n\n它的职责是做本地节点运行与检查，而不是执行 Wasm。".to_string(),
        ),
        "dawn-approval-guard" => Some(
            "这是 Dawn 的原生技能 `Dawn Approval Guard`，默认可用，不需要安装。\n\n本机使用方式：\n- /app 与 /console: Approval Center\n- 关键动作: 审批、授权、敏感操作门禁\n- AP2: 配合授权链路做人在环确认\n\n它的职责是做审批和高风险动作治理，而不是执行 Wasm。".to_string(),
        ),
        "dawn-marketplace-operator" => Some(
            "这是 Dawn 的原生技能 `Dawn Marketplace Operator`，默认可用，不需要安装。\n\n本机使用方式：\n- CLI: `dawn.cmd agents search <关键词> --federated`、`dawn-node skills install`\n- /app: Marketplace 和 Agent Cards 面板\n- 联邦目录: 搜索、导入、安装与发布技能和 Agent\n\n它的职责是做 Dawn 能力分发和市场运营，而不是执行 Wasm。".to_string(),
        ),
        _ => None,
    }
}

fn skill_from_row(row: SkillRow) -> anyhow::Result<SkillRecord> {
    Ok(SkillRecord {
        skill_id: row.skill_id,
        version: row.version,
        display_name: row.display_name,
        description: row.description,
        entry_function: row.entry_function,
        capabilities: serde_json::from_str(&row.capabilities)
            .context("failed to parse skill capabilities")?,
        artifact_path: row.artifact_path,
        artifact_sha256: row.artifact_sha256,
        source_kind: row.source_kind,
        issuer_did: row.issuer_did,
        signature_hex: row.signature_hex,
        document_hash: row.document_hash,
        issued_at_unix_ms: row
            .issued_at_unix_ms
            .map(|value| i64_to_u128(value, "issued_at_unix_ms"))
            .transpose()?,
        active: row.active != 0,
        created_at_unix_ms: i64_to_u128(row.created_at_unix_ms, "created_at_unix_ms")?,
        updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms, "updated_at_unix_ms")?,
    })
}

fn native_builtin_skills() -> anyhow::Result<Vec<SkillRecord>> {
    NATIVE_BUILTIN_SKILLS
        .iter()
        .map(native_builtin_skill_record)
        .collect()
}

fn native_builtin_skill(
    skill_id: &str,
    version: Option<&str>,
) -> anyhow::Result<Option<SkillRecord>> {
    let Some(spec) = NATIVE_BUILTIN_SKILLS.iter().find(|spec| {
        spec.skill_id == skill_id && version.is_none_or(|value| value == spec.version)
    }) else {
        return Ok(None);
    };
    native_builtin_skill_record(spec).map(Some)
}

fn native_builtin_skill_record(spec: &NativeBuiltinSkillSpec) -> anyhow::Result<SkillRecord> {
    let artifact_path = native_builtin_skill_path(spec);
    let artifact_bytes = std::fs::read(&artifact_path).with_context(|| {
        format!(
            "failed to read native builtin skill file {}",
            artifact_path.display()
        )
    })?;
    let metadata = std::fs::metadata(&artifact_path).with_context(|| {
        format!(
            "failed to stat native builtin skill file {}",
            artifact_path.display()
        )
    })?;
    let updated_at_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(1_770_000_000_000);
    Ok(SkillRecord {
        skill_id: spec.skill_id.to_string(),
        version: spec.version.to_string(),
        display_name: spec.display_name.to_string(),
        description: Some(spec.description.to_string()),
        entry_function: "native_entry".to_string(),
        capabilities: spec
            .capabilities
            .iter()
            .map(|value| value.to_string())
            .collect(),
        artifact_path: artifact_path.display().to_string(),
        artifact_sha256: hex::encode(Sha256::digest(&artifact_bytes)),
        source_kind: NATIVE_BUILTIN_SOURCE_KIND.to_string(),
        issuer_did: None,
        signature_hex: None,
        document_hash: None,
        issued_at_unix_ms: None,
        active: true,
        created_at_unix_ms: updated_at_unix_ms,
        updated_at_unix_ms,
    })
}

fn native_builtin_skill_path(spec: &NativeBuiltinSkillSpec) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(spec.artifact_relative_path)
        .components()
        .collect()
}

fn i64_to_u128(value: i64, label: &str) -> anyhow::Result<u128> {
    u128::try_from(value).with_context(|| format!("negative {label} in wasm_skills"))
}

fn validate_skill_segment(value: &str, label: &str) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("{label} cannot be empty");
    }
    if matches!(value, "." | "..") {
        anyhow::bail!("{label} cannot be a path traversal segment");
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Ok(());
    }
    anyhow::bail!("{label} may only contain ASCII letters, digits, dash, underscore, and period");
}

fn skill_publisher_issuer_did_from_public_key_hex(public_key_hex: &str) -> anyhow::Result<String> {
    let bytes = decode_fixed_hex::<32>(public_key_hex, "skill publisher public key")?;
    Ok(format!(
        "{SKILL_PUBLISHER_ISSUER_DID_PREFIX}{}",
        hex::encode(bytes)
    ))
}

fn validate_skill_publisher_issuer_did(
    issuer_did: &str,
    public_key_hex: &str,
) -> anyhow::Result<()> {
    let expected = skill_publisher_issuer_did_from_public_key_hex(public_key_hex)?;
    let normalized = issuer_did.to_ascii_lowercase();
    if normalized != expected {
        anyhow::bail!(
            "skill publisher DID '{}' does not match public key; expected '{}'",
            issuer_did,
            expected
        );
    }
    Ok(())
}

fn verify_signed_skill_envelope(
    envelope: &SignedSkillEnvelope,
    trust_root: &SkillPublisherTrustRootRecord,
) -> anyhow::Result<String> {
    validate_skill_publisher_issuer_did(&trust_root.issuer_did, &trust_root.public_key_hex)?;
    if envelope.document.issuer_did.to_ascii_lowercase()
        != trust_root.issuer_did.to_ascii_lowercase()
    {
        anyhow::bail!(
            "skill publisher '{}' does not match trusted issuer '{}'",
            envelope.document.issuer_did,
            trust_root.issuer_did
        );
    }

    let verifying_key = decode_verifying_key(&trust_root.public_key_hex)?;
    let payload = signed_skill_payload(&envelope.document)?;
    let signature_bytes = decode_fixed_hex::<64>(&envelope.signature_hex, "skill signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(&payload, &signature)
        .context("signed skill verification failed")?;
    signed_skill_hash(&envelope.document)
}

fn signed_skill_payload(document: &SignedSkillDocument) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(document).context("failed to serialize signed skill document")
}

fn signed_skill_hash(document: &SignedSkillDocument) -> anyhow::Result<String> {
    let payload = signed_skill_payload(document)?;
    Ok(hex::encode(Sha256::digest(payload)))
}

fn decode_verifying_key(public_key_hex: &str) -> anyhow::Result<VerifyingKey> {
    let public_key_bytes = decode_fixed_hex::<32>(public_key_hex, "skill publisher public key")?;
    VerifyingKey::from_bytes(&public_key_bytes)
        .context("skill publisher public key must be a valid Ed25519 verifying key")
}

fn decode_fixed_hex<const N: usize>(raw: &str, label: &str) -> anyhow::Result<[u8; N]> {
    let normalized = normalize_hex(raw)?;
    let bytes = hex::decode(normalized).with_context(|| format!("{label} must be valid hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("{label} must be {} bytes long", N))
}

fn normalize_hex(raw: &str) -> anyhow::Result<String> {
    Ok(hex::encode(
        hex::decode(raw.trim()).context("value must be valid hex")?,
    ))
}

async fn persist_skill_artifact(
    skill_id: &str,
    version: &str,
    wasm_bytes: &[u8],
) -> anyhow::Result<PathBuf> {
    let artifact_root = skill_artifact_root_dir();
    fs::create_dir_all(&artifact_root)
        .await
        .with_context(|| format!("failed to create artifact root {}", artifact_root.display()))?;
    let artifact_root = fs::canonicalize(&artifact_root).await.with_context(|| {
        format!(
            "failed to canonicalize artifact root {}",
            artifact_root.display()
        )
    })?;
    let artifact_dir = artifact_root.join(skill_id).join(version);
    fs::create_dir_all(&artifact_dir).await.with_context(|| {
        format!(
            "failed to create artifact directory {}",
            artifact_dir.display()
        )
    })?;
    let artifact_dir = fs::canonicalize(&artifact_dir).await.with_context(|| {
        format!(
            "failed to canonicalize artifact directory {}",
            artifact_dir.display()
        )
    })?;
    if !artifact_dir.starts_with(&artifact_root) {
        anyhow::bail!(
            "skill artifact directory {} escaped artifact root {}",
            artifact_dir.display(),
            artifact_root.display()
        );
    }
    let artifact_path = artifact_dir.join("module.wasm");
    fs::write(&artifact_path, wasm_bytes)
        .await
        .with_context(|| format!("failed to write artifact {}", artifact_path.display()))?;
    Ok(artifact_path)
}

fn skill_artifact_root_dir() -> PathBuf {
    std::env::var("DAWN_SKILL_ARTIFACTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("data").join("skills"))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

fn not_found(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": message.into()
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use base64::{Engine as _, prelude::*};
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use wasmtime::Engine;

    use super::{
        NATIVE_BUILTIN_SOURCE_KIND, RegisterSignedSkillRequest, SKILL_PUBLISHER_ISSUER_DID_PREFIX,
        SignedSkillDocument, SignedSkillEnvelope, SkillPublisherTrustRootUpsertRequest,
        build_skill_intake_conversion_proposal, current_distribution, inspect_skill_source_text,
        native_builtin_skill_usage, register_signed_skill_inner, skill_intake_proposal_key,
        skill_publisher_issuer_did_from_public_key_hex, upsert_skill_publisher_trust_root_inner,
        validate_skill_segment,
    };
    use crate::{app_state::AppState, sandbox};
    use uuid::Uuid;

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "dawn-core-skill-registry-test-{}.db",
            Uuid::new_v4()
        ));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    #[test]
    fn accepts_simple_skill_segments() {
        assert!(validate_skill_segment("echo-skill_1.0", "skill_id").is_ok());
    }

    #[test]
    fn rejects_unsafe_skill_segments() {
        assert!(validate_skill_segment("../escape", "skill_id").is_err());
    }

    #[test]
    fn derives_self_certifying_skill_publisher_did() {
        let did = skill_publisher_issuer_did_from_public_key_hex(&"ef".repeat(32)).unwrap();
        assert_eq!(
            did,
            format!("{SKILL_PUBLISHER_ISSUER_DID_PREFIX}{}", "ef".repeat(32))
        );
    }

    #[tokio::test]
    async fn registers_signed_skill_from_trusted_publisher() {
        let (state, db_path) = test_state().await.unwrap();
        let signing_key = SigningKey::from_bytes(&[31_u8; 32]);
        let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());
        let issuer_did = skill_publisher_issuer_did_from_public_key_hex(&public_key_hex).unwrap();

        let trust_root = upsert_skill_publisher_trust_root_inner(
            &state,
            SkillPublisherTrustRootUpsertRequest {
                actor: "test-suite".to_string(),
                reason: "seed trusted skill publisher".to_string(),
                issuer_did: issuer_did.clone(),
                label: "test publisher".to_string(),
                public_key_hex: public_key_hex.clone(),
            },
        )
        .await
        .unwrap()
        .trust_root;
        assert_eq!(trust_root.issuer_did, issuer_did);

        let wasm_bytes = BASE64_STANDARD
            .decode(b"AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL")
            .unwrap();
        let artifact_sha256 = hex::encode(Sha256::digest(&wasm_bytes));
        let document = SignedSkillDocument {
            skill_id: "echo-skill".to_string(),
            version: "1.0.0".to_string(),
            display_name: "Echo Skill".to_string(),
            description: Some("signed smoke skill".to_string()),
            entry_function: "run_skill".to_string(),
            capabilities: vec!["echo".to_string()],
            artifact_sha256,
            issuer_did,
            issued_at_unix_ms: 1_700_000_000_000,
        };
        let signature = signing_key.sign(&serde_json::to_vec(&document).unwrap());
        let response = register_signed_skill_inner(
            &state,
            RegisterSignedSkillRequest {
                envelope: SignedSkillEnvelope {
                    document,
                    signature_hex: hex::encode(signature.to_bytes()),
                },
                wasm_base64: "AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL".to_string(),
                activate: Some(true),
            },
        )
        .await
        .unwrap();

        assert_eq!(response.skill.skill_id, "echo-skill");
        assert_eq!(response.skill.source_kind, "signed_publisher");
        assert!(response.skill.signature_hex.is_some());
        assert!(response.skill.document_hash.is_some());
        assert_eq!(response.skill.issuer_did, Some(trust_root.issuer_did));

        drop(state);
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn distribution_includes_native_builtin_skills() {
        let (state, db_path) = test_state().await.unwrap();
        let distribution = current_distribution(&state).await.unwrap();
        assert!(
            distribution
                .skills
                .iter()
                .any(|skill| skill.skill_id == "agent-card-discoverer"
                    && skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND)
        );
        assert!(
            distribution
                .skills
                .iter()
                .any(|skill| skill.skill_id == "bayesian-skill-set"
                    && skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND)
        );
        assert!(
            distribution
                .skills
                .iter()
                .any(|skill| skill.skill_id == "dawn-orchestrator"
                    && skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND)
        );
        assert!(
            distribution
                .skills
                .iter()
                .any(|skill| skill.skill_id == "dawn-model-router"
                    && skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND)
        );
        assert!(
            distribution
                .skills
                .iter()
                .any(|skill| skill.skill_id == "dawn-desktop-control"
                    && skill.source_kind == NATIVE_BUILTIN_SOURCE_KIND)
        );
        drop(state);
        fs::remove_file(db_path).ok();
    }

    #[test]
    fn native_builtin_skill_usage_is_available() {
        assert!(native_builtin_skill_usage("agent-card-discoverer").is_some());
        assert!(native_builtin_skill_usage("bayesian-skill-set").is_some());
        assert!(native_builtin_skill_usage("dawn-orchestrator").is_some());
        assert!(native_builtin_skill_usage("dawn-chat-bridge").is_some());
        assert!(native_builtin_skill_usage("dawn-desktop-control").is_some());
        assert!(native_builtin_skill_usage("dawn-model-router").is_some());
        assert!(native_builtin_skill_usage("dawn-node-operator").is_some());
        assert!(native_builtin_skill_usage("dawn-approval-guard").is_some());
        assert!(native_builtin_skill_usage("dawn-marketplace-operator").is_some());
    }

    #[test]
    fn intake_detects_signed_dawn_skill_package() {
        let package = serde_json::json!({
            "skill": {
                "skillId": "echo-skill",
                "version": "1.0.0",
                "displayName": "Echo Skill",
                "entryFunction": "run_skill",
                "capabilities": ["echo"],
                "sourceKind": "signed_publisher"
            },
            "envelope": {
                "document": {
                    "skillId": "echo-skill",
                    "version": "1.0.0",
                    "displayName": "Echo Skill",
                    "entryFunction": "run_skill",
                    "capabilities": ["echo"],
                    "artifactSha256": "00",
                    "issuerDid": "did:dawn:skill-publisher:00",
                    "issuedAtUnixMs": 1
                },
                "signatureHex": "00"
            },
            "wasmBase64": "AGFzbQE="
        });
        let response = inspect_skill_source_text(
            "https://example.com/skills/echo/package",
            Some("application/json".to_string()),
            &package.to_string(),
            None,
        );
        assert_eq!(response.detected_kind, "dawn_signed_wasm_skill_package");
        assert_eq!(response.installability, "direct_install");
        assert_eq!(
            response.direct_install_url.as_deref(),
            Some("https://example.com/skills/echo/package")
        );
        assert!(response.requires_trusted_publisher);
        assert!(!response.conversion_required);
    }

    #[test]
    fn intake_detects_unsigned_dawn_skill_package() {
        let package = serde_json::json!({
            "skill": {
                "skillId": "dev-skill",
                "version": "0.1.0",
                "displayName": "Dev Skill",
                "entryFunction": "run_skill",
                "capabilities": ["dev"],
                "sourceKind": "unsigned_local"
            },
            "wasmBase64": "AGFzbQE="
        });
        let response = inspect_skill_source_text(
            "https://example.com/skills/dev/package",
            Some("application/json".to_string()),
            &package.to_string(),
            None,
        );
        assert_eq!(response.detected_kind, "dawn_unsigned_wasm_skill_package");
        assert_eq!(
            response.installability,
            "direct_install_requires_allow_unsigned"
        );
        assert!(response.allow_unsigned_supported);
        assert!(!response.requires_trusted_publisher);
    }

    #[test]
    fn intake_detects_native_builtin_reference() {
        let package = serde_json::json!({
            "skill": {
                "skillId": "dawn-desktop-control",
                "version": "native",
                "displayName": "Dawn Desktop Control",
                "entryFunction": "native",
                "capabilities": ["desktop_control"],
                "sourceKind": "native_builtin"
            },
            "wasmBase64": ""
        });
        let response = inspect_skill_source_text(
            "https://example.com/skills/dawn-desktop-control/native/package",
            Some("application/json".to_string()),
            &package.to_string(),
            None,
        );
        assert_eq!(response.detected_kind, "dawn_native_builtin_reference");
        assert_eq!(response.installability, "already_available");
        assert!(!response.conversion_required);
    }

    #[test]
    fn intake_detects_codex_skill_markdown() {
        let text = r#"---
name: sample-skill
description: Sample Codex skill
---

# Sample Skill

Use when a task needs a sample workflow.
"#;
        let response = inspect_skill_source_text(
            "https://raw.githubusercontent.com/example/repo/main/SKILL.md",
            Some("text/markdown".to_string()),
            text,
            None,
        );
        assert_eq!(response.detected_kind, "codex_skill_markdown");
        assert_eq!(response.installability, "conversion_required");
        assert!(response.conversion_required);
    }

    #[test]
    fn intake_detects_mcp_package_json() {
        let package = serde_json::json!({
            "name": "filesystem-mcp",
            "dependencies": {
                "@modelcontextprotocol/sdk": "^1.0.0"
            }
        });
        let response = inspect_skill_source_text(
            "https://example.com/package.json",
            Some("application/json".to_string()),
            &package.to_string(),
            None,
        );
        assert_eq!(response.detected_kind, "mcp_server_project");
        assert!(response.conversion_required);
    }

    #[test]
    fn intake_detects_python_project_metadata() {
        let text = r#"[project]
name = "gis-helper"
version = "0.1.0"
"#;
        let response = inspect_skill_source_text(
            "https://example.com/pyproject.toml",
            Some("text/x-toml".to_string()),
            text,
            None,
        );
        assert_eq!(response.detected_kind, "python_tooling_project");
        assert!(response.conversion_required);
    }

    #[test]
    fn intake_detects_browser_extension_manifest() {
        let manifest = serde_json::json!({
            "manifest_version": 3,
            "name": "Browser Helper",
            "permissions": ["tabs"],
            "background": { "service_worker": "background.js" }
        });
        let response = inspect_skill_source_text(
            "https://example.com/manifest.json",
            Some("application/json".to_string()),
            &manifest.to_string(),
            None,
        );
        assert_eq!(response.detected_kind, "browser_extension");
        assert!(response.conversion_required);
    }

    #[test]
    fn intake_conversion_builds_reviewable_skill_proposal() {
        let text = r#"---
name: sample-skill
description: Sample Codex skill
---

# Sample Skill
"#;
        let intake = inspect_skill_source_text(
            "https://raw.githubusercontent.com/example/repo/main/SKILL.md",
            Some("text/markdown".to_string()),
            text,
            None,
        );
        let proposal_id = Uuid::new_v4();
        let proposal =
            build_skill_intake_conversion_proposal(&intake, Some("operator"), proposal_id, 42)
                .expect("conversion intake should produce proposal");
        assert_eq!(proposal.proposal_id, proposal_id);
        assert_eq!(proposal.source, "skill-intake");
        assert_eq!(proposal.status, "proposed");
        assert_eq!(proposal.created_by, "operator");
        assert!(proposal.suggested_skill_id.starts_with("import."));
        assert!(proposal.tags.contains(&"skill-intake".to_string()));
        let expected_source_sha256 = hex::encode(Sha256::digest(text.as_bytes()));
        assert_eq!(
            intake.source_sha256.as_deref(),
            Some(expected_source_sha256.as_str())
        );
        assert!(
            intake
                .source_preview
                .as_deref()
                .unwrap_or_default()
                .contains("Sample Skill")
        );
        assert_eq!(
            proposal.evidence["conversionPlan"]["planKind"],
            "online_skill_source_conversion"
        );
        assert_eq!(
            proposal.evidence["intake"]["sourceSha256"],
            expected_source_sha256
        );
        assert_eq!(
            proposal.evidence["intake"]["detectedKind"],
            "codex_skill_markdown"
        );
        assert_eq!(
            proposal.evidence["conversionPlan"]["conversionSpec"]["adapterKind"],
            "codex_skill_markdown_adapter"
        );
        assert!(
            proposal.evidence["conversionPlan"]["conversionSpec"]["extractionTargets"]
                .to_string()
                .contains("allowed commands")
        );
    }

    #[test]
    fn intake_proposal_key_tracks_source_content_hash() {
        let source_url = "https://raw.githubusercontent.com/example/repo/main/SKILL.md";
        let text_a = r#"---
name: sample-skill
description: Sample Codex skill
---

# Sample Skill

Original workflow.
"#;
        let text_b = r#"---
name: sample-skill
description: Sample Codex skill
---

# Sample Skill

Changed workflow.
"#;
        let intake_a =
            inspect_skill_source_text(source_url, Some("text/markdown".to_string()), text_a, None);
        let intake_a_again =
            inspect_skill_source_text(source_url, Some("text/markdown".to_string()), text_a, None);
        let intake_b =
            inspect_skill_source_text(source_url, Some("text/markdown".to_string()), text_b, None);

        assert_eq!(intake_a.source_url, intake_b.source_url);
        assert_eq!(intake_a.source_sha256, intake_a_again.source_sha256);
        assert_ne!(intake_a.source_sha256, intake_b.source_sha256);
        assert_eq!(
            skill_intake_proposal_key(&intake_a),
            skill_intake_proposal_key(&intake_a_again)
        );
        assert_ne!(
            skill_intake_proposal_key(&intake_a),
            skill_intake_proposal_key(&intake_b)
        );

        let proposal_a =
            build_skill_intake_conversion_proposal(&intake_a, Some("operator"), Uuid::new_v4(), 42)
                .expect("first conversion intake should produce proposal");
        let proposal_b =
            build_skill_intake_conversion_proposal(&intake_b, Some("operator"), Uuid::new_v4(), 43)
                .expect("changed conversion intake should produce proposal");
        assert_ne!(proposal_a.proposal_key, proposal_b.proposal_key);
    }

    #[test]
    fn intake_conversion_skips_direct_install_package() {
        let package = serde_json::json!({
            "skill": {
                "skillId": "echo-skill",
                "version": "1.0.0",
                "displayName": "Echo Skill",
                "entryFunction": "run_skill",
                "capabilities": ["echo"],
                "sourceKind": "signed_publisher"
            },
            "envelope": {
                "document": {
                    "skillId": "echo-skill",
                    "version": "1.0.0",
                    "displayName": "Echo Skill",
                    "entryFunction": "run_skill",
                    "capabilities": ["echo"],
                    "artifactSha256": "00",
                    "issuerDid": "did:dawn:skill-publisher:00",
                    "issuedAtUnixMs": 1
                },
                "signatureHex": "00"
            },
            "wasmBase64": "AGFzbQE="
        });
        let intake = inspect_skill_source_text(
            "https://example.com/skills/echo/package",
            Some("application/json".to_string()),
            &package.to_string(),
            None,
        );
        assert!(
            build_skill_intake_conversion_proposal(&intake, Some("operator"), Uuid::new_v4(), 42)
                .is_none()
        );
    }
}
