use std::{
    path::{Path, PathBuf},
    sync::Arc,
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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::fs;
use wasmtime::Module;

use crate::app_state::{AppState, SkillPublisherTrustRootRecord, unix_timestamp_ms};

pub const SKILL_PUBLISHER_ISSUER_DID_PREFIX: &str = "did:dawn:skill-publisher:";

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

    row.map(skill_from_row).transpose()
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
    let package = reqwest::Client::new()
        .get(&request.package_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch skill package {}", request.package_url))?
        .error_for_status()
        .with_context(|| {
            format!(
                "skill package endpoint returned an error {}",
                request.package_url
            )
        })?
        .json::<SkillPackageResponse>()
        .await
        .with_context(|| format!("failed to decode skill package {}", request.package_url))?;

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
    let Some(_skill) = find_skill(state, skill_id, Some(version)).await? else {
        anyhow::bail!("skill version not found: {skill_id}@{version}");
    };

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

    rows.into_iter().map(skill_from_row).collect()
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

    rows.into_iter().map(skill_from_row).collect()
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

fn i64_to_u128(value: i64, label: &str) -> anyhow::Result<u128> {
    u128::try_from(value).with_context(|| format!("negative {label} in wasm_skills"))
}

fn validate_skill_segment(value: &str, label: &str) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("{label} cannot be empty");
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
    let artifact_dir = skill_artifact_root_dir().join(skill_id).join(version);
    fs::create_dir_all(&artifact_dir).await.with_context(|| {
        format!(
            "failed to create artifact directory {}",
            artifact_dir.display()
        )
    })?;
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
        RegisterSignedSkillRequest, SKILL_PUBLISHER_ISSUER_DID_PREFIX, SignedSkillDocument,
        SignedSkillEnvelope, SkillPublisherTrustRootUpsertRequest, register_signed_skill_inner,
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
}
