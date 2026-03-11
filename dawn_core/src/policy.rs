use std::sync::Arc;

use anyhow::Context;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app_state::{
    AppState, PolicyAuditEventRecord, PolicyProfileRecord, PolicyTrustRootRecord, unix_timestamp_ms,
};

pub const POLICY_ISSUER_DID_PREFIX: &str = "did:dawn:policy:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyEffect {
    Allow,
    Deny,
}

#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub effect: PolicyEffect,
    pub reason: String,
}

impl PolicyDecision {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            effect: PolicyEffect::Allow,
            reason: reason.into(),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            effect: PolicyEffect::Deny,
            reason: reason.into(),
        }
    }

    pub fn ensure_allowed(&self) -> anyhow::Result<()> {
        if self.effect == PolicyEffect::Allow {
            Ok(())
        } else {
            anyhow::bail!("{}", self.reason)
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDocument {
    pub policy_id: String,
    pub version: u32,
    pub issuer_did: String,
    pub issued_at_unix_ms: u128,
    pub allow_shell_exec: bool,
    pub allowed_model_providers: Vec<String>,
    pub allowed_chat_platforms: Vec<String>,
    pub max_payment_amount: Option<f64>,
    pub updated_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SignedPolicyEnvelope {
    pub document: PolicyDocument,
    pub signature_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyUpdateRequest {
    pub actor: String,
    pub reason: String,
    pub allow_shell_exec: Option<bool>,
    pub allowed_model_providers: Option<Vec<String>>,
    pub allowed_chat_platforms: Option<Vec<String>>,
    pub max_payment_amount: Option<Option<f64>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedPolicyActivationRequest {
    pub actor: String,
    pub reason: String,
    pub envelope: SignedPolicyEnvelope,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyTrustRootUpsertRequest {
    pub actor: String,
    pub reason: String,
    pub issuer_did: String,
    pub label: String,
    pub public_key_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyUpdateResponse {
    pub profile: PolicyProfileRecord,
    pub audit_event: PolicyAuditEventRecord,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedPolicyActivationResponse {
    pub profile: PolicyProfileRecord,
    pub audit_event: PolicyAuditEventRecord,
    pub trust_root: PolicyTrustRootRecord,
    pub envelope: SignedPolicyEnvelope,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyTrustRootUpsertResponse {
    pub trust_root: PolicyTrustRootRecord,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDistributionResponse {
    pub profile: PolicyProfileRecord,
    pub envelope: Option<SignedPolicyEnvelope>,
    pub trusted_issuer: bool,
}

pub async fn current_profile(state: &Arc<AppState>) -> anyhow::Result<PolicyProfileRecord> {
    state.get_policy_profile().await
}

pub async fn current_distribution(
    state: &Arc<AppState>,
) -> anyhow::Result<PolicyDistributionResponse> {
    let profile = state.get_policy_profile().await?;
    let envelope = signed_envelope_from_profile(&profile)?;
    let trusted_issuer = match &profile.issuer_did {
        Some(issuer_did) => state.get_policy_trust_root(issuer_did).await?.is_some(),
        None => false,
    };

    Ok(PolicyDistributionResponse {
        profile,
        envelope,
        trusted_issuer,
    })
}

pub async fn update_profile(
    state: &Arc<AppState>,
    request: PolicyUpdateRequest,
) -> anyhow::Result<PolicyUpdateResponse> {
    let existing = state.get_policy_profile().await?;
    let now = unix_timestamp_ms();
    let profile = PolicyProfileRecord {
        policy_id: existing.policy_id.clone(),
        version: existing.version.saturating_add(1),
        issuer_did: None,
        allow_shell_exec: request
            .allow_shell_exec
            .unwrap_or(existing.allow_shell_exec),
        allowed_model_providers: request
            .allowed_model_providers
            .unwrap_or(existing.allowed_model_providers),
        allowed_chat_platforms: request
            .allowed_chat_platforms
            .unwrap_or(existing.allowed_chat_platforms),
        max_payment_amount: request
            .max_payment_amount
            .unwrap_or(existing.max_payment_amount),
        signature_hex: None,
        document_hash: None,
        issued_at_unix_ms: None,
        updated_reason: request.reason.clone(),
        created_at_unix_ms: existing.created_at_unix_ms,
        updated_at_unix_ms: now,
    };

    let profile = state.save_policy_profile(&profile).await?;
    let audit_event = state
        .record_policy_audit_event(
            &profile.policy_id,
            profile.version,
            request.actor,
            format!("manual policy update: {}", profile.updated_reason),
            &to_value(&profile)?,
        )
        .await?;

    Ok(PolicyUpdateResponse {
        profile,
        audit_event,
    })
}

pub async fn activate_signed_profile(
    state: &Arc<AppState>,
    request: SignedPolicyActivationRequest,
) -> anyhow::Result<SignedPolicyActivationResponse> {
    let existing = state.get_policy_profile().await?;
    let trust_root = state
        .get_policy_trust_root(&request.envelope.document.issuer_did)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "issuer '{}' is not present in gateway policy trust roots",
                request.envelope.document.issuer_did
            )
        })?;

    if request.envelope.document.policy_id != existing.policy_id {
        anyhow::bail!(
            "signed policy targets '{}', expected '{}'",
            request.envelope.document.policy_id,
            existing.policy_id
        );
    }

    if request.envelope.document.version <= existing.version {
        anyhow::bail!(
            "signed policy version {} must be greater than current version {}",
            request.envelope.document.version,
            existing.version
        );
    }

    let normalized_signature_hex = normalize_hex(&request.envelope.signature_hex)?;
    let verified_hash = verify_signed_envelope(
        &SignedPolicyEnvelope {
            document: request.envelope.document.clone(),
            signature_hex: normalized_signature_hex.clone(),
        },
        &trust_root,
    )?;

    let now = unix_timestamp_ms();
    let profile = PolicyProfileRecord {
        policy_id: request.envelope.document.policy_id.clone(),
        version: request.envelope.document.version,
        issuer_did: Some(request.envelope.document.issuer_did.clone()),
        allow_shell_exec: request.envelope.document.allow_shell_exec,
        allowed_model_providers: request.envelope.document.allowed_model_providers.clone(),
        allowed_chat_platforms: request.envelope.document.allowed_chat_platforms.clone(),
        max_payment_amount: request.envelope.document.max_payment_amount,
        signature_hex: Some(normalized_signature_hex.clone()),
        document_hash: Some(verified_hash),
        issued_at_unix_ms: Some(request.envelope.document.issued_at_unix_ms),
        updated_reason: request.envelope.document.updated_reason.clone(),
        created_at_unix_ms: existing.created_at_unix_ms,
        updated_at_unix_ms: now,
    };

    let profile = state.save_policy_profile(&profile).await?;
    let envelope = SignedPolicyEnvelope {
        document: request.envelope.document,
        signature_hex: normalized_signature_hex,
    };
    let audit_snapshot = json!({
        "activation": {
            "actor": request.actor,
            "reason": request.reason,
        },
        "trustRoot": trust_root,
        "envelope": envelope,
        "profile": profile,
    });
    let audit_event = state
        .record_policy_audit_event(
            &profile.policy_id,
            profile.version,
            audit_snapshot["activation"]["actor"]
                .as_str()
                .unwrap_or("unknown"),
            format!(
                "activated signed policy version {} from issuer {}",
                profile.version,
                profile
                    .issuer_did
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            &audit_snapshot,
        )
        .await?;

    Ok(SignedPolicyActivationResponse {
        profile,
        audit_event,
        trust_root,
        envelope,
    })
}

pub async fn list_trust_roots(state: &Arc<AppState>) -> anyhow::Result<Vec<PolicyTrustRootRecord>> {
    state.list_policy_trust_roots().await
}

pub async fn upsert_trust_root(
    state: &Arc<AppState>,
    request: PolicyTrustRootUpsertRequest,
) -> anyhow::Result<PolicyTrustRootUpsertResponse> {
    let public_key_hex = normalize_hex(&request.public_key_hex)?;
    validate_policy_issuer_did(&request.issuer_did, &public_key_hex)?;

    let existing = state.get_policy_trust_root(&request.issuer_did).await?;
    let now = unix_timestamp_ms();
    let trust_root = PolicyTrustRootRecord {
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

    let trust_root = state.save_policy_trust_root(&trust_root).await?;
    Ok(PolicyTrustRootUpsertResponse { trust_root })
}

pub fn evaluate_node_command(policy: &PolicyProfileRecord, command_type: &str) -> PolicyDecision {
    if command_type == "shell_exec" && !policy.allow_shell_exec {
        return PolicyDecision::deny(
            "policy denied shell_exec because allowShellExec is false in the active gateway policy",
        );
    }

    PolicyDecision::allow(format!("policy allowed node command '{command_type}'"))
}

pub fn evaluate_model_provider(policy: &PolicyProfileRecord, provider: &str) -> PolicyDecision {
    if !policy.allowed_model_providers.is_empty()
        && !policy
            .allowed_model_providers
            .iter()
            .any(|candidate| candidate == provider)
    {
        return PolicyDecision::deny(format!(
            "policy denied model provider '{provider}' because it is not in allowedModelProviders"
        ));
    }

    PolicyDecision::allow(format!("policy allowed model provider '{provider}'"))
}

pub fn evaluate_chat_platform(policy: &PolicyProfileRecord, platform: &str) -> PolicyDecision {
    if !policy.allowed_chat_platforms.is_empty()
        && !policy
            .allowed_chat_platforms
            .iter()
            .any(|candidate| candidate == platform)
    {
        return PolicyDecision::deny(format!(
            "policy denied chat platform '{platform}' because it is not in allowedChatPlatforms"
        ));
    }

    PolicyDecision::allow(format!("policy allowed chat platform '{platform}'"))
}

pub fn evaluate_payment(
    policy: &PolicyProfileRecord,
    mandate_id: Uuid,
    amount: f64,
    description: &str,
) -> PolicyDecision {
    if amount <= 0.0 {
        return PolicyDecision::deny("policy denied payment because amount must be positive");
    }

    if let Some(max_amount) = policy.max_payment_amount {
        if amount > max_amount {
            return PolicyDecision::deny(format!(
                "policy denied payment {amount:.2} because it exceeds maxPaymentAmount={max_amount:.2}"
            ));
        }
    }

    PolicyDecision::allow(format!(
        "policy approved AP2 checkpoint for mandate {mandate_id} amount {amount:.2} ({description})"
    ))
}

pub fn policy_issuer_did_from_public_key_hex(public_key_hex: &str) -> anyhow::Result<String> {
    let bytes = decode_fixed_hex::<32>(public_key_hex, "policy public key")?;
    Ok(format!("{POLICY_ISSUER_DID_PREFIX}{}", hex::encode(bytes)))
}

fn validate_policy_issuer_did(issuer_did: &str, public_key_hex: &str) -> anyhow::Result<()> {
    let expected = policy_issuer_did_from_public_key_hex(public_key_hex)?;
    let normalized = issuer_did.to_ascii_lowercase();
    if normalized != expected {
        anyhow::bail!(
            "issuer DID '{}' does not match public key; expected '{}'",
            issuer_did,
            expected
        );
    }
    Ok(())
}

fn signed_envelope_from_profile(
    profile: &PolicyProfileRecord,
) -> anyhow::Result<Option<SignedPolicyEnvelope>> {
    match (
        profile.issuer_did.clone(),
        profile.signature_hex.clone(),
        profile.issued_at_unix_ms,
    ) {
        (None, None, None) => Ok(None),
        (Some(issuer_did), Some(signature_hex), Some(issued_at_unix_ms)) => {
            Ok(Some(SignedPolicyEnvelope {
                document: PolicyDocument {
                    policy_id: profile.policy_id.clone(),
                    version: profile.version,
                    issuer_did,
                    issued_at_unix_ms,
                    allow_shell_exec: profile.allow_shell_exec,
                    allowed_model_providers: profile.allowed_model_providers.clone(),
                    allowed_chat_platforms: profile.allowed_chat_platforms.clone(),
                    max_payment_amount: profile.max_payment_amount,
                    updated_reason: profile.updated_reason.clone(),
                },
                signature_hex,
            }))
        }
        _ => anyhow::bail!(
            "policy profile {} is partially signed; issuer, signature, and issuedAt must move together",
            profile.policy_id
        ),
    }
}

fn verify_signed_envelope(
    envelope: &SignedPolicyEnvelope,
    trust_root: &PolicyTrustRootRecord,
) -> anyhow::Result<String> {
    validate_policy_issuer_did(&trust_root.issuer_did, &trust_root.public_key_hex)?;
    if envelope.document.issuer_did.to_ascii_lowercase()
        != trust_root.issuer_did.to_ascii_lowercase()
    {
        anyhow::bail!(
            "policy issuer '{}' does not match trusted issuer '{}'",
            envelope.document.issuer_did,
            trust_root.issuer_did
        );
    }

    let verifying_key = decode_verifying_key(&trust_root.public_key_hex)?;
    let payload = signed_policy_payload(&envelope.document)?;
    let signature_bytes = decode_fixed_hex::<64>(&envelope.signature_hex, "policy signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(&payload, &signature)
        .context("policy signature verification failed")?;
    signed_policy_hash(&envelope.document)
}

fn signed_policy_payload(document: &PolicyDocument) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(document).context("failed to serialize policy document for signing")
}

fn signed_policy_hash(document: &PolicyDocument) -> anyhow::Result<String> {
    let payload = signed_policy_payload(document)?;
    Ok(hex::encode(Sha256::digest(payload)))
}

fn decode_verifying_key(public_key_hex: &str) -> anyhow::Result<VerifyingKey> {
    let public_key_bytes = decode_fixed_hex::<32>(public_key_hex, "policy public key")?;
    VerifyingKey::from_bytes(&public_key_bytes).context("invalid Ed25519 policy public key")
}

fn decode_fixed_hex<const N: usize>(raw: &str, label: &str) -> anyhow::Result<[u8; N]> {
    let bytes = hex::decode(raw).with_context(|| format!("failed to decode {label} as hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} must be {N} bytes"))
}

fn normalize_hex(raw: &str) -> anyhow::Result<String> {
    Ok(hex::encode(
        hex::decode(raw.trim()).context("value must be valid hex")?,
    ))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use ed25519_dalek::{Signer, SigningKey};
    use uuid::Uuid;
    use wasmtime::Engine;

    use super::{
        POLICY_ISSUER_DID_PREFIX, PolicyDocument, PolicyEffect, PolicyTrustRootUpsertRequest,
        SignedPolicyActivationRequest, SignedPolicyEnvelope, activate_signed_profile,
        evaluate_payment, policy_issuer_did_from_public_key_hex, upsert_trust_root,
    };
    use crate::{
        app_state::{AppState, PolicyProfileRecord},
        sandbox,
    };

    fn profile() -> PolicyProfileRecord {
        PolicyProfileRecord {
            policy_id: "default".to_string(),
            version: 1,
            issuer_did: None,
            allow_shell_exec: false,
            allowed_model_providers: Vec::new(),
            allowed_chat_platforms: Vec::new(),
            max_payment_amount: Some(10.0),
            signature_hex: None,
            document_hash: None,
            issued_at_unix_ms: None,
            updated_reason: "test".to_string(),
            created_at_unix_ms: 0,
            updated_at_unix_ms: 0,
        }
    }

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-policy-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    #[test]
    fn denies_non_positive_payments() {
        let decision = evaluate_payment(&profile(), uuid::Uuid::nil(), 0.0, "invalid");
        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    #[test]
    fn denies_payments_over_cap() {
        let decision = evaluate_payment(&profile(), uuid::Uuid::nil(), 11.0, "over cap");
        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    #[test]
    fn derives_self_certifying_policy_did() {
        let did = policy_issuer_did_from_public_key_hex(&"ab".repeat(32)).unwrap();
        assert_eq!(
            did,
            format!("{POLICY_ISSUER_DID_PREFIX}{}", "ab".repeat(32))
        );
    }

    #[tokio::test]
    async fn activates_signed_policy_from_trusted_issuer() {
        let (state, db_path) = test_state().await.unwrap();
        let signing_key = SigningKey::from_bytes(&[13_u8; 32]);
        let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());
        let issuer_did = policy_issuer_did_from_public_key_hex(&public_key_hex).unwrap();

        let trust_root = upsert_trust_root(
            &state,
            PolicyTrustRootUpsertRequest {
                actor: "test-suite".to_string(),
                reason: "seed trusted issuer".to_string(),
                issuer_did: issuer_did.clone(),
                label: "test issuer".to_string(),
                public_key_hex: public_key_hex.clone(),
            },
        )
        .await
        .unwrap()
        .trust_root;
        assert_eq!(trust_root.issuer_did, issuer_did);

        let document = PolicyDocument {
            policy_id: "default".to_string(),
            version: 2,
            issuer_did,
            issued_at_unix_ms: 1_700_000_000_000,
            allow_shell_exec: false,
            allowed_model_providers: vec!["deepseek".to_string()],
            allowed_chat_platforms: vec!["feishu".to_string()],
            max_payment_amount: Some(15.0),
            updated_reason: "signed rollout".to_string(),
        };
        let signature = signing_key.sign(&serde_json::to_vec(&document).unwrap());
        let response = activate_signed_profile(
            &state,
            SignedPolicyActivationRequest {
                actor: "test-suite".to_string(),
                reason: "activate signed policy".to_string(),
                envelope: SignedPolicyEnvelope {
                    document,
                    signature_hex: hex::encode(signature.to_bytes()),
                },
            },
        )
        .await
        .unwrap();

        assert_eq!(response.profile.version, 2);
        assert_eq!(
            response.profile.allowed_model_providers,
            vec!["deepseek".to_string()]
        );
        assert_eq!(
            response.profile.issuer_did,
            Some(response.trust_root.issuer_did.clone())
        );
        assert!(response.profile.signature_hex.is_some());
        assert!(response.profile.document_hash.is_some());

        drop(state);
        fs::remove_file(db_path).ok();
    }
}
