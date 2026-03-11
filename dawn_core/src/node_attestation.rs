use std::sync::Arc;

use anyhow::Context;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_state::{
    AppState, NodeAttestationState, NodeRecord, NodeTrustRootRecord, unix_timestamp_ms,
};

pub const NODE_ISSUER_DID_PREFIX: &str = "did:dawn:node:";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeCapabilityAttestationDocument {
    pub node_id: String,
    pub issuer_did: String,
    pub issued_at_unix_ms: u128,
    pub display_name: String,
    pub transport: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SignedNodeCapabilityAttestation {
    pub document: NodeCapabilityAttestationDocument,
    pub signature_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTrustRootUpsertRequest {
    pub actor: String,
    pub reason: String,
    pub issuer_did: String,
    pub label: String,
    pub public_key_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTrustRootUpsertResponse {
    pub trust_root: NodeTrustRootRecord,
}

pub async fn list_trust_roots(state: &Arc<AppState>) -> anyhow::Result<Vec<NodeTrustRootRecord>> {
    state.list_node_trust_roots().await
}

pub async fn upsert_trust_root(
    state: &Arc<AppState>,
    request: NodeTrustRootUpsertRequest,
) -> anyhow::Result<NodeTrustRootUpsertResponse> {
    let public_key_hex = normalize_hex(&request.public_key_hex)?;
    validate_node_issuer_did(&request.issuer_did, &public_key_hex)?;

    let existing = state.get_node_trust_root(&request.issuer_did).await?;
    let now = unix_timestamp_ms();
    let trust_root = NodeTrustRootRecord {
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

    let trust_root = state.save_node_trust_root(&trust_root).await?;
    Ok(NodeTrustRootUpsertResponse { trust_root })
}

pub async fn resolve_attestation_update(
    state: &Arc<AppState>,
    expected_node_id: &str,
    attestation: SignedNodeCapabilityAttestation,
) -> anyhow::Result<NodeAttestationState> {
    let normalized_signature_hex = normalize_hex(&attestation.signature_hex)?;
    let normalized_issuer_did = attestation.document.issuer_did.to_ascii_lowercase();
    let document_hash = attestation_hash(&attestation.document)?;

    let mut update = NodeAttestationState {
        issuer_did: normalized_issuer_did.clone(),
        signature_hex: normalized_signature_hex.clone(),
        document_hash,
        issued_at_unix_ms: attestation.document.issued_at_unix_ms,
        verified: false,
        verified_at_unix_ms: None,
        attestation_error: None,
        verified_capabilities: None,
        display_name: Some(attestation.document.display_name.clone()),
        transport: Some(attestation.document.transport.clone()),
    };

    if attestation.document.node_id != expected_node_id {
        update.attestation_error = Some(format!(
            "attestation nodeId '{}' does not match expected node '{}'",
            attestation.document.node_id, expected_node_id
        ));
        return Ok(update);
    }

    let public_key_hex = match public_key_hex_from_node_did(&normalized_issuer_did) {
        Ok(value) => value,
        Err(error) => {
            update.attestation_error = Some(error.to_string());
            return Ok(update);
        }
    };
    if let Err(error) = validate_node_issuer_did(&normalized_issuer_did, &public_key_hex) {
        update.attestation_error = Some(error.to_string());
        return Ok(update);
    }

    let Some(trust_root) = state.get_node_trust_root(&normalized_issuer_did).await? else {
        update.attestation_error = Some(format!(
            "node issuer '{normalized_issuer_did}' is not trusted yet"
        ));
        return Ok(update);
    };

    match verify_attestation_signature(&attestation, &trust_root, &normalized_signature_hex) {
        Ok(()) => {
            update.verified = true;
            update.verified_at_unix_ms = Some(unix_timestamp_ms());
            update.verified_capabilities = Some(attestation.document.capabilities);
            update.attestation_error = None;
        }
        Err(error) => {
            update.attestation_error = Some(error.to_string());
        }
    }

    Ok(update)
}

pub fn authorize_node_command(node: &NodeRecord, command_type: &str) -> anyhow::Result<()> {
    if !node.attestation_verified {
        anyhow::bail!(
            "node '{}' is not attested by a trusted issuer; command dispatch is blocked",
            node.node_id
        );
    }

    if !node
        .capabilities
        .iter()
        .any(|capability| capability == command_type)
    {
        anyhow::bail!(
            "node '{}' attestation does not include capability '{}'",
            node.node_id,
            command_type
        );
    }

    Ok(())
}

pub fn node_issuer_did_from_public_key_hex(public_key_hex: &str) -> anyhow::Result<String> {
    let bytes = decode_fixed_hex::<32>(public_key_hex, "node public key")?;
    Ok(format!("{NODE_ISSUER_DID_PREFIX}{}", hex::encode(bytes)))
}

fn verify_attestation_signature(
    attestation: &SignedNodeCapabilityAttestation,
    trust_root: &NodeTrustRootRecord,
    normalized_signature_hex: &str,
) -> anyhow::Result<()> {
    validate_node_issuer_did(&trust_root.issuer_did, &trust_root.public_key_hex)?;
    if attestation.document.issuer_did.to_ascii_lowercase()
        != trust_root.issuer_did.to_ascii_lowercase()
    {
        anyhow::bail!(
            "attestation issuer '{}' does not match trust root '{}'",
            attestation.document.issuer_did,
            trust_root.issuer_did
        );
    }

    let verifying_key = decode_verifying_key(&trust_root.public_key_hex)?;
    let payload = serde_json::to_vec(&attestation.document)
        .context("failed to serialize node capability attestation")?;
    let signature_bytes =
        decode_fixed_hex::<64>(normalized_signature_hex, "node attestation signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(&payload, &signature)
        .context("node capability attestation signature verification failed")?;
    Ok(())
}

fn attestation_hash(document: &NodeCapabilityAttestationDocument) -> anyhow::Result<String> {
    let payload =
        serde_json::to_vec(document).context("failed to serialize attestation document hash")?;
    Ok(hex::encode(Sha256::digest(payload)))
}

fn validate_node_issuer_did(issuer_did: &str, public_key_hex: &str) -> anyhow::Result<()> {
    let expected = node_issuer_did_from_public_key_hex(public_key_hex)?;
    let normalized = issuer_did.to_ascii_lowercase();
    if normalized != expected {
        anyhow::bail!(
            "node issuer DID '{}' does not match public key; expected '{}'",
            issuer_did,
            expected
        );
    }
    Ok(())
}

fn public_key_hex_from_node_did(issuer_did: &str) -> anyhow::Result<String> {
    issuer_did
        .to_ascii_lowercase()
        .strip_prefix(NODE_ISSUER_DID_PREFIX)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("unexpected node issuer DID format"))
}

fn decode_verifying_key(public_key_hex: &str) -> anyhow::Result<VerifyingKey> {
    let public_key_bytes = decode_fixed_hex::<32>(public_key_hex, "node public key")?;
    VerifyingKey::from_bytes(&public_key_bytes).context("invalid Ed25519 node public key")
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
        NODE_ISSUER_DID_PREFIX, NodeCapabilityAttestationDocument, NodeTrustRootUpsertRequest,
        SignedNodeCapabilityAttestation, node_issuer_did_from_public_key_hex,
        resolve_attestation_update, upsert_trust_root,
    };
    use crate::{app_state::AppState, sandbox};

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-node-attest-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    #[test]
    fn derives_self_certifying_node_did() {
        let did = node_issuer_did_from_public_key_hex(&"cd".repeat(32)).unwrap();
        assert_eq!(did, format!("{NODE_ISSUER_DID_PREFIX}{}", "cd".repeat(32)));
    }

    #[tokio::test]
    async fn verifies_attested_capabilities_for_trusted_node() {
        let (state, db_path) = test_state().await.unwrap();
        let signing_key = SigningKey::from_bytes(&[23_u8; 32]);
        let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());
        let issuer_did = node_issuer_did_from_public_key_hex(&public_key_hex).unwrap();
        let node_id = "node-alpha";

        state
            .upsert_node(
                node_id.to_string(),
                "Node Alpha".to_string(),
                "websocket".to_string(),
                Vec::new(),
            )
            .await
            .unwrap();
        upsert_trust_root(
            &state,
            NodeTrustRootUpsertRequest {
                actor: "test-suite".to_string(),
                reason: "seed trusted node issuer".to_string(),
                issuer_did: issuer_did.clone(),
                label: "test node issuer".to_string(),
                public_key_hex,
            },
        )
        .await
        .unwrap();

        let document = NodeCapabilityAttestationDocument {
            node_id: node_id.to_string(),
            issuer_did,
            issued_at_unix_ms: 1_700_000_000_001,
            display_name: "Node Alpha".to_string(),
            transport: "websocket".to_string(),
            capabilities: vec!["agent_ping".to_string(), "echo".to_string()],
        };
        let signature = signing_key.sign(&serde_json::to_vec(&document).unwrap());
        let update = resolve_attestation_update(
            &state,
            node_id,
            SignedNodeCapabilityAttestation {
                document,
                signature_hex: hex::encode(signature.to_bytes()),
            },
        )
        .await
        .unwrap();

        assert!(update.verified);
        assert_eq!(
            update.verified_capabilities.unwrap(),
            vec!["agent_ping".to_string(), "echo".to_string()]
        );

        drop(state);
        fs::remove_file(db_path).ok();
    }
}
