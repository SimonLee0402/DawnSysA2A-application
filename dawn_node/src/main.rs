use std::{
    collections::HashMap,
    env,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{process::Command, time::interval};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
struct NodeConfig {
    gateway_ws_url: String,
    node_id: String,
    node_name: String,
    capabilities: Vec<String>,
    allow_shell: bool,
    signing_seed: [u8; 32],
    issuer_did: String,
    uses_derived_identity: bool,
    enforce_trusted_rollout: bool,
    require_signed_skills: bool,
    policy_trust_roots: HashMap<String, String>,
    skill_publisher_trust_roots: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GatewayInboundEnvelope {
    message_type: String,
    node_id: String,
    command_id: Option<String>,
    command_type: Option<String>,
    payload: Option<Value>,
    bundle: Option<GatewayRolloutBundle>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatEnvelope<'a> {
    message_type: &'static str,
    display_name: &'a str,
    capabilities: &'a [String],
    observed_at_unix_ms: u128,
    capability_attestation: SignedNodeCapabilityAttestation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandResultEnvelope {
    message_type: &'static str,
    command_id: String,
    status: &'static str,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeCapabilityAttestationDocument {
    node_id: String,
    issuer_did: String,
    issued_at_unix_ms: u128,
    display_name: String,
    transport: String,
    capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedNodeCapabilityAttestation {
    document: NodeCapabilityAttestationDocument,
    signature_hex: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GatewayRolloutBundle {
    generated_at_unix_ms: u128,
    bundle_hash: String,
    policy_version: u32,
    policy_document_hash: Option<String>,
    skill_distribution_hash: String,
    policy: PolicyDistributionResponse,
    skills: SkillDistributionResponse,
}

#[derive(Debug, Default, Clone)]
struct NodeRuntimeState {
    last_rollout_bundle_hash: Option<String>,
    last_policy_version: Option<u32>,
    last_skill_distribution_hash: Option<String>,
    last_rollout_received_at_unix_ms: Option<u128>,
    last_rollout_verified: bool,
    last_rollout_verification_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RolloutAckEnvelope {
    message_type: &'static str,
    bundle_hash: String,
    accepted: bool,
    policy_version: u32,
    skill_distribution_hash: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PolicyProfileRecord {
    policy_id: String,
    version: u32,
    issuer_did: Option<String>,
    allow_shell_exec: bool,
    allowed_model_providers: Vec<String>,
    allowed_chat_platforms: Vec<String>,
    max_payment_amount: Option<f64>,
    signature_hex: Option<String>,
    document_hash: Option<String>,
    issued_at_unix_ms: Option<u128>,
    updated_reason: String,
    created_at_unix_ms: u128,
    updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PolicyDocument {
    policy_id: String,
    version: u32,
    issuer_did: String,
    issued_at_unix_ms: u128,
    allow_shell_exec: bool,
    allowed_model_providers: Vec<String>,
    allowed_chat_platforms: Vec<String>,
    max_payment_amount: Option<f64>,
    updated_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignedPolicyEnvelope {
    document: PolicyDocument,
    signature_hex: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PolicyDistributionResponse {
    profile: PolicyProfileRecord,
    envelope: Option<SignedPolicyEnvelope>,
    #[serde(rename = "trustedIssuer")]
    _trusted_issuer: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SkillRecord {
    skill_id: String,
    version: String,
    display_name: String,
    description: Option<String>,
    entry_function: String,
    capabilities: Vec<String>,
    artifact_path: String,
    artifact_sha256: String,
    source_kind: String,
    issuer_did: Option<String>,
    signature_hex: Option<String>,
    document_hash: Option<String>,
    issued_at_unix_ms: Option<u128>,
    active: bool,
    created_at_unix_ms: u128,
    updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignedSkillDocument {
    skill_id: String,
    version: String,
    display_name: String,
    description: Option<String>,
    entry_function: String,
    capabilities: Vec<String>,
    artifact_sha256: String,
    issuer_did: String,
    issued_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SkillDistributionResponse {
    skills: Vec<SkillRecord>,
    active_versions: usize,
    signed_versions: usize,
    trusted_publishers: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dawn_node=debug".into()),
        )
        .init();

    let config = load_config();
    info!(
        "Starting DawnNode '{}' against {} (issuerDid={}, derivedIdentity={})",
        config.node_id, config.gateway_ws_url, config.issuer_did, config.uses_derived_identity
    );

    loop {
        if let Err(error) = run_session(&config).await {
            error!("Node session ended with error: {error:#}");
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn run_session(config: &NodeConfig) -> anyhow::Result<()> {
    let url = format!(
        "{}?displayName={}&transport=websocket",
        config.gateway_ws_url,
        url_encode(&config.node_name)
    );
    let (stream, _) = connect_async(&url)
        .await
        .with_context(|| format!("failed to connect to gateway websocket at {url}"))?;
    let (mut writer, mut reader) = stream.split();
    let mut runtime_state = NodeRuntimeState::default();

    let mut heartbeat = interval(Duration::from_secs(15));
    send_heartbeat(&mut writer, config).await?;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                send_heartbeat(&mut writer, config).await?;
            }
            inbound = reader.next() => {
                let Some(inbound) = inbound else {
                    anyhow::bail!("gateway websocket closed");
                };
                match inbound? {
                    Message::Text(text) => {
                        if let Some(reply) =
                            handle_gateway_message(config, &mut runtime_state, &text).await
                        {
                            writer.send(Message::Text(reply.into())).await?;
                        }
                    }
                    Message::Ping(payload) => {
                        writer.send(Message::Pong(payload)).await?;
                    }
                    Message::Close(frame) => {
                        warn!("Gateway requested close: {:?}", frame);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn send_heartbeat<S>(writer: &mut S, config: &NodeConfig) -> anyhow::Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let attestation = build_capability_attestation(config)?;
    let heartbeat = HeartbeatEnvelope {
        message_type: "heartbeat",
        display_name: &config.node_name,
        capabilities: &config.capabilities,
        observed_at_unix_ms: unix_timestamp_ms(),
        capability_attestation: attestation,
    };
    writer
        .send(Message::Text(serde_json::to_string(&heartbeat)?.into()))
        .await?;
    Ok(())
}

async fn handle_gateway_message(
    config: &NodeConfig,
    runtime_state: &mut NodeRuntimeState,
    raw: &str,
) -> Option<String> {
    let Ok(envelope) = serde_json::from_str::<GatewayInboundEnvelope>(raw) else {
        warn!("Ignoring malformed gateway payload");
        return None;
    };

    match envelope.message_type.as_str() {
        "session_ready" => {
            info!("Gateway session is ready for node {}", config.node_id);
            None
        }
        "command_dispatch" => {
            let (Some(command_id), Some(command_type)) = (
                envelope.command_id.clone(),
                envelope.command_type.clone(),
            ) else {
                warn!("Ignoring command_dispatch without command_id/command_type");
                return None;
            };
            info!(
                "Received command {} for node {}",
                command_type, envelope.node_id
            );
            Some(
                execute_command(
                    config,
                    GatewayCommandEnvelope {
                        command_id,
                        command_type,
                        payload: envelope.payload.unwrap_or_else(|| json!({})),
                    },
                )
                .await,
            )
        }
        "rollout_bundle" => {
            let Some(bundle) = envelope.bundle else {
                warn!("Ignoring rollout_bundle without bundle payload");
                return None;
            };
            Some(apply_rollout_bundle(config, runtime_state, bundle))
        }
        other => {
            warn!("Ignoring unsupported gateway message type '{other}'");
            None
        }
    }
}

#[derive(Debug, Clone)]
struct GatewayCommandEnvelope {
    command_id: String,
    command_type: String,
    payload: Value,
}

async fn execute_command(config: &NodeConfig, envelope: GatewayCommandEnvelope) -> String {
    let fallback_command_id = envelope.command_id.clone();
    let response = match envelope.command_type.as_str() {
        "echo" => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(envelope.payload),
            error: None,
        },
        "list_capabilities" => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "nodeId": config.node_id,
                "capabilities": config.capabilities,
                "allowShell": config.allow_shell
            })),
            error: None,
        },
        "agent_ping" => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "nodeId": config.node_id,
                "nodeName": config.node_name,
                "observedAtUnixMs": unix_timestamp_ms()
            })),
            error: None,
        },
        "shell_exec" => execute_shell_command(config, envelope).await,
        other => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!("unsupported command type: {other}")),
        },
    };

    serde_json::to_string(&response).unwrap_or_else(|_| {
        json!({
            "messageType": "command_result",
            "commandId": fallback_command_id,
            "status": "failed",
            "error": "failed to serialize node response"
        })
        .to_string()
    })
}

fn apply_rollout_bundle(
    config: &NodeConfig,
    runtime_state: &mut NodeRuntimeState,
    bundle: GatewayRolloutBundle,
) -> String {
    let verification = verify_rollout_bundle(config, &bundle);
    let skill_count = bundle.skills.skills.len();
    let allowed_model_count = bundle.policy.profile.allowed_model_providers.len();
    let accepted = verification.is_ok() || !config.enforce_trusted_rollout;
    let error_message = verification.err().map(|error| error.to_string());

    if let Some(error_message) = &error_message {
        if accepted {
            warn!(
                "Accepted rollout bundle {} without strict verification: {}",
                bundle.bundle_hash, error_message
            );
        } else {
            warn!(
                "Rejected rollout bundle {} because verification failed: {}",
                bundle.bundle_hash, error_message
            );
        }
    }

    runtime_state.last_rollout_verified = error_message.is_none();
    runtime_state.last_rollout_verification_error = error_message.clone();
    runtime_state.last_rollout_received_at_unix_ms = Some(bundle.generated_at_unix_ms);

    if !accepted {
        return serde_json::to_string(&RolloutAckEnvelope {
            message_type: "rollout_ack",
            bundle_hash: bundle.bundle_hash,
            accepted: false,
            policy_version: bundle.policy_version,
            skill_distribution_hash: bundle.skill_distribution_hash,
            error: error_message,
        })
        .unwrap_or_else(|_| {
            json!({
                "messageType": "rollout_ack",
                "accepted": false,
                "bundleHash": "",
                "policyVersion": 0,
                "skillDistributionHash": "",
                "error": "failed to serialize rollout rejection"
            })
            .to_string()
        });
    }

    info!(
        "Applied rollout bundle {} (policyVersion={}, skills={}, allowedModels={}, policyHash={}, verified={})",
        bundle.bundle_hash,
        bundle.policy_version,
        skill_count,
        allowed_model_count,
        bundle
            .policy_document_hash
            .clone()
            .unwrap_or_else(|| "unsigned".to_string()),
        runtime_state.last_rollout_verified
    );
    runtime_state.last_rollout_bundle_hash = Some(bundle.bundle_hash.clone());
    runtime_state.last_policy_version = Some(bundle.policy_version);
    runtime_state.last_skill_distribution_hash = Some(bundle.skill_distribution_hash.clone());

    serde_json::to_string(&RolloutAckEnvelope {
        message_type: "rollout_ack",
        bundle_hash: bundle.bundle_hash,
        accepted: true,
        policy_version: bundle.policy_version,
        skill_distribution_hash: bundle.skill_distribution_hash,
        error: error_message,
    })
    .unwrap_or_else(|_| {
        json!({
            "messageType": "rollout_ack",
            "accepted": false,
            "bundleHash": "",
            "policyVersion": 0,
            "skillDistributionHash": "",
            "error": "failed to serialize rollout ack"
        })
        .to_string()
    })
}

async fn execute_shell_command(
    config: &NodeConfig,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    if !config.allow_shell {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "shell execution is disabled. Set DAWN_NODE_ALLOW_SHELL=1 to enable it."
                    .to_string(),
            ),
        };
    }

    let command = envelope
        .payload
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if command.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("shell_exec requires payload.command".to_string()),
        };
    }

    let output = if cfg!(target_os = "windows") {
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(command)
            .output()
            .await
    } else {
        Command::new("sh").arg("-lc").arg(command).output().await
    };

    match output {
        Ok(output) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: if output.status.success() {
                "succeeded"
            } else {
                "failed"
            },
            result: Some(json!({
                "exitCode": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr)
            })),
            error: None,
        },
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(error.to_string()),
        },
    }
}

fn load_config() -> NodeConfig {
    let node_id = env::var("DAWN_NODE_ID").unwrap_or_else(|_| "node-local".to_string());
    let node_name = env::var("DAWN_NODE_NAME").unwrap_or_else(|_| "Dawn Local Node".to_string());
    let gateway_ws_url = env::var("DAWN_GATEWAY_WS_URL").unwrap_or_else(|_| {
        format!("ws://127.0.0.1:8000/api/gateway/control-plane/nodes/{node_id}/session")
    });
    let capabilities = env::var("DAWN_NODE_CAPABILITIES")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| default_capabilities());
    let allow_shell = env::var("DAWN_NODE_ALLOW_SHELL")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    let enforce_trusted_rollout = env::var("DAWN_NODE_ENFORCE_TRUSTED_ROLLOUT")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    let require_signed_skills = env::var("DAWN_NODE_REQUIRE_SIGNED_SKILLS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    let capabilities = normalize_capabilities(capabilities, allow_shell);
    let (signing_seed, uses_derived_identity) = load_signing_seed(&node_id);
    let signing_key = SigningKey::from_bytes(&signing_seed);
    let issuer_did = format!(
        "did:dawn:node:{}",
        hex::encode(signing_key.verifying_key().as_bytes())
    );
    let policy_trust_roots = load_trust_roots_from_env("DAWN_NODE_POLICY_TRUST_ROOTS");
    let skill_publisher_trust_roots =
        load_trust_roots_from_env("DAWN_NODE_SKILL_PUBLISHER_TRUST_ROOTS");

    NodeConfig {
        gateway_ws_url,
        node_id,
        node_name,
        capabilities,
        allow_shell,
        signing_seed,
        issuer_did,
        uses_derived_identity,
        enforce_trusted_rollout,
        require_signed_skills,
        policy_trust_roots,
        skill_publisher_trust_roots,
    }
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn url_encode(input: &str) -> String {
    input.replace(' ', "%20")
}

fn build_capability_attestation(
    config: &NodeConfig,
) -> anyhow::Result<SignedNodeCapabilityAttestation> {
    let document = NodeCapabilityAttestationDocument {
        node_id: config.node_id.clone(),
        issuer_did: config.issuer_did.clone(),
        issued_at_unix_ms: unix_timestamp_ms(),
        display_name: config.node_name.clone(),
        transport: "websocket".to_string(),
        capabilities: config.capabilities.clone(),
    };
    let signing_key = SigningKey::from_bytes(&config.signing_seed);
    let signature = signing_key.sign(&serde_json::to_vec(&document)?);
    Ok(SignedNodeCapabilityAttestation {
        document,
        signature_hex: hex::encode(signature.to_bytes()),
    })
}

fn default_capabilities() -> Vec<String> {
    vec![
        "echo".to_string(),
        "list_capabilities".to_string(),
        "agent_ping".to_string(),
    ]
}

fn normalize_capabilities(mut capabilities: Vec<String>, allow_shell: bool) -> Vec<String> {
    if allow_shell {
        if !capabilities.iter().any(|value| value == "shell_exec") {
            capabilities.push("shell_exec".to_string());
        }
    } else {
        capabilities.retain(|value| value != "shell_exec");
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn load_signing_seed(node_id: &str) -> ([u8; 32], bool) {
    if let Ok(raw) = env::var("DAWN_NODE_SIGNING_SEED_HEX") {
        if let Ok(bytes) = hex::decode(raw.trim()) {
            if let Ok(seed) = <[u8; 32]>::try_from(bytes) {
                return (seed, false);
            }
        }
        warn!("DAWN_NODE_SIGNING_SEED_HEX is invalid; falling back to derived node identity");
    }

    let digest = Sha256::digest(format!("dawn-node:{node_id}").as_bytes());
    let mut seed = [0_u8; 32];
    seed.copy_from_slice(&digest[..32]);
    (seed, true)
}

fn load_trust_roots_from_env(var_name: &str) -> HashMap<String, String> {
    env::var(var_name)
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|entry| {
                    let trimmed = entry.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let (issuer_did, public_key_hex) = trimmed.split_once('=')?;
                    Some((
                        issuer_did.trim().to_ascii_lowercase(),
                        normalize_hex(public_key_hex).ok()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn verify_rollout_bundle(config: &NodeConfig, bundle: &GatewayRolloutBundle) -> anyhow::Result<()> {
    verify_policy_distribution(config, bundle)?;
    verify_skill_distribution(config, bundle)?;

    let expected_skill_hash = hash_json_value(&bundle.skills)?;
    if expected_skill_hash != bundle.skill_distribution_hash {
        anyhow::bail!(
            "skill distribution hash mismatch: bundle has '{}', node computed '{}'",
            bundle.skill_distribution_hash,
            expected_skill_hash
        );
    }

    let expected_policy_hash = match &bundle.policy.profile.document_hash {
        Some(hash) => hash.clone(),
        None => hash_json_value(&bundle.policy.profile)?,
    };
    if bundle
        .policy_document_hash
        .as_ref()
        .is_some_and(|hash| hash != &expected_policy_hash)
    {
        anyhow::bail!(
            "policy document hash mismatch: bundle has '{:?}', node computed '{}'",
            bundle.policy_document_hash,
            expected_policy_hash
        );
    }

    let expected_bundle_hash = hash_json_value(&json!({
        "policyVersion": bundle.policy_version,
        "policyDocumentHash": &bundle.policy_document_hash,
        "skillDistributionHash": &bundle.skill_distribution_hash
    }))?;
    if expected_bundle_hash != bundle.bundle_hash {
        anyhow::bail!(
            "bundle hash mismatch: bundle has '{}', node computed '{}'",
            bundle.bundle_hash,
            expected_bundle_hash
        );
    }

    Ok(())
}

fn verify_policy_distribution(config: &NodeConfig, bundle: &GatewayRolloutBundle) -> anyhow::Result<()> {
    let profile = &bundle.policy.profile;
    if profile.version != bundle.policy_version {
        anyhow::bail!(
            "rollout policy version {} does not match embedded policy profile version {}",
            bundle.policy_version,
            profile.version
        );
    }

    match &bundle.policy.envelope {
        Some(envelope) => {
            let issuer_did = envelope.document.issuer_did.to_ascii_lowercase();
            let public_key_hex = config
                .policy_trust_roots
                .get(&issuer_did)
                .ok_or_else(|| anyhow::anyhow!("policy issuer '{}' is not trusted by this node", issuer_did))?;
            validate_self_certifying_did(&issuer_did, public_key_hex, "did:dawn:policy:")?;
            verify_signature(public_key_hex, &serde_json::to_vec(&envelope.document)?, &envelope.signature_hex)
                .context("policy signature verification failed on node")?;

            if profile.issuer_did.as_deref().map(str::to_ascii_lowercase) != Some(issuer_did.clone()) {
                anyhow::bail!("policy profile issuer does not match signed policy issuer");
            }
            if profile.version != envelope.document.version
                || profile.allow_shell_exec != envelope.document.allow_shell_exec
                || profile.allowed_model_providers != envelope.document.allowed_model_providers
                || profile.allowed_chat_platforms != envelope.document.allowed_chat_platforms
                || profile.max_payment_amount != envelope.document.max_payment_amount
                || profile.updated_reason != envelope.document.updated_reason
            {
                anyhow::bail!("policy profile does not match signed policy document");
            }
            let document_hash = hash_json_value(&envelope.document)?;
            if profile.document_hash.as_deref() != Some(document_hash.as_str()) {
                anyhow::bail!("policy profile document hash does not match signed document");
            }
        }
        None => {
            if !config.policy_trust_roots.is_empty() || config.enforce_trusted_rollout {
                anyhow::bail!("rollout policy envelope is missing");
            }
        }
    }

    Ok(())
}

fn verify_skill_distribution(config: &NodeConfig, bundle: &GatewayRolloutBundle) -> anyhow::Result<()> {
    for skill in &bundle.skills.skills {
        match skill.source_kind.as_str() {
            "signed_publisher" => {
                let issuer_did = skill
                    .issuer_did
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("signed skill '{}' is missing issuerDid", skill.skill_id))?
                    .to_ascii_lowercase();
                let signature_hex = skill
                    .signature_hex
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("signed skill '{}' is missing signatureHex", skill.skill_id))?;
                let issued_at_unix_ms = skill
                    .issued_at_unix_ms
                    .ok_or_else(|| anyhow::anyhow!("signed skill '{}' is missing issuedAtUnixMs", skill.skill_id))?;
                let public_key_hex = config
                    .skill_publisher_trust_roots
                    .get(&issuer_did)
                    .ok_or_else(|| anyhow::anyhow!("skill publisher '{}' is not trusted by this node", issuer_did))?;
                validate_self_certifying_did(
                    &issuer_did,
                    public_key_hex,
                    "did:dawn:skill-publisher:",
                )?;
                let document = SignedSkillDocument {
                    skill_id: skill.skill_id.clone(),
                    version: skill.version.clone(),
                    display_name: skill.display_name.clone(),
                    description: skill.description.clone(),
                    entry_function: skill.entry_function.clone(),
                    capabilities: skill.capabilities.clone(),
                    artifact_sha256: skill.artifact_sha256.clone(),
                    issuer_did: issuer_did.clone(),
                    issued_at_unix_ms,
                };
                verify_signature(public_key_hex, &serde_json::to_vec(&document)?, &signature_hex)
                    .context("skill signature verification failed on node")?;
                let document_hash = hash_json_value(&document)?;
                if skill.document_hash.as_deref() != Some(document_hash.as_str()) {
                    anyhow::bail!(
                        "signed skill '{}@{}' has a mismatched document hash",
                        skill.skill_id,
                        skill.version
                    );
                }
            }
            _ => {
                if config.require_signed_skills {
                    anyhow::bail!(
                        "unsigned skill '{}@{}' is not allowed when DAWN_NODE_REQUIRE_SIGNED_SKILLS=1",
                        skill.skill_id,
                        skill.version
                    );
                }
            }
        }
    }

    Ok(())
}

fn validate_self_certifying_did(
    issuer_did: &str,
    public_key_hex: &str,
    prefix: &str,
) -> anyhow::Result<()> {
    let normalized_issuer = issuer_did.to_ascii_lowercase();
    let normalized_key = normalize_hex(public_key_hex)?;
    let expected = format!("{prefix}{normalized_key}");
    if normalized_issuer != expected {
        anyhow::bail!(
            "issuer DID '{}' does not match expected self-certifying DID '{}'",
            issuer_did,
            expected
        );
    }
    Ok(())
}

fn verify_signature(public_key_hex: &str, payload: &[u8], signature_hex: &str) -> anyhow::Result<()> {
    let public_key_bytes = decode_fixed_hex::<32>(public_key_hex, "public key")?;
    let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
        .context("public key must be a valid Ed25519 verifying key")?;
    let signature_bytes = decode_fixed_hex::<64>(signature_hex, "signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(payload, &signature)
        .context("Ed25519 verification failed")?;
    Ok(())
}

fn decode_fixed_hex<const N: usize>(raw: &str, label: &str) -> anyhow::Result<[u8; N]> {
    let bytes = hex::decode(normalize_hex(raw)?).with_context(|| format!("{label} must be valid hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} must be {N} bytes"))
}

fn normalize_hex(raw: &str) -> anyhow::Result<String> {
    Ok(hex::encode(
        hex::decode(raw.trim()).context("value must be valid hex")?,
    ))
}

fn hash_json_value(value: &impl Serialize) -> anyhow::Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> NodeConfig {
        NodeConfig {
            gateway_ws_url: "ws://127.0.0.1:8000/api/gateway/control-plane/nodes/node-test/session"
                .to_string(),
            node_id: "node-test".to_string(),
            node_name: "Node Test".to_string(),
            capabilities: vec!["agent_ping".to_string()],
            allow_shell: false,
            signing_seed: [7_u8; 32],
            issuer_did: "did:dawn:node:test".to_string(),
            uses_derived_identity: false,
            enforce_trusted_rollout: true,
            require_signed_skills: false,
            policy_trust_roots: HashMap::new(),
            skill_publisher_trust_roots: HashMap::new(),
        }
    }

    fn signed_rollout_bundle() -> (NodeConfig, GatewayRolloutBundle) {
        let policy_signing_key = SigningKey::from_bytes(&[11_u8; 32]);
        let policy_public_key_hex = hex::encode(policy_signing_key.verifying_key().as_bytes());
        let policy_issuer_did = format!("did:dawn:policy:{policy_public_key_hex}");
        let skill_signing_key = SigningKey::from_bytes(&[19_u8; 32]);
        let skill_public_key_hex = hex::encode(skill_signing_key.verifying_key().as_bytes());
        let skill_issuer_did = format!("did:dawn:skill-publisher:{skill_public_key_hex}");

        let policy_document = PolicyDocument {
            policy_id: "default".to_string(),
            version: 7,
            issuer_did: policy_issuer_did.clone(),
            issued_at_unix_ms: 1_700_000_000_000,
            allow_shell_exec: false,
            allowed_model_providers: vec!["deepseek".to_string()],
            allowed_chat_platforms: vec!["feishu".to_string()],
            max_payment_amount: Some(10.0),
            updated_reason: "signed rollout".to_string(),
        };
        let policy_signature = policy_signing_key.sign(&serde_json::to_vec(&policy_document).unwrap());
        let policy_document_hash = hash_json_value(&policy_document).unwrap();
        let policy_distribution = PolicyDistributionResponse {
            profile: PolicyProfileRecord {
                policy_id: "default".to_string(),
                version: 7,
                issuer_did: Some(policy_issuer_did.clone()),
                allow_shell_exec: false,
                allowed_model_providers: vec!["deepseek".to_string()],
                allowed_chat_platforms: vec!["feishu".to_string()],
                max_payment_amount: Some(10.0),
                signature_hex: Some(hex::encode(policy_signature.to_bytes())),
                document_hash: Some(policy_document_hash.clone()),
                issued_at_unix_ms: Some(1_700_000_000_000),
                updated_reason: "signed rollout".to_string(),
                created_at_unix_ms: 1_700_000_000_000,
                updated_at_unix_ms: 1_700_000_000_010,
            },
            envelope: Some(SignedPolicyEnvelope {
                document: policy_document,
                signature_hex: hex::encode(policy_signature.to_bytes()),
            }),
            _trusted_issuer: true,
        };

        let skill_document = SignedSkillDocument {
            skill_id: "echo-skill".to_string(),
            version: "1.0.0".to_string(),
            display_name: "Echo Skill".to_string(),
            description: Some("signed test skill".to_string()),
            entry_function: "run_skill".to_string(),
            capabilities: vec!["echo".to_string()],
            artifact_sha256: "ab".repeat(32),
            issuer_did: skill_issuer_did.clone(),
            issued_at_unix_ms: 1_700_000_000_000,
        };
        let skill_signature = skill_signing_key.sign(&serde_json::to_vec(&skill_document).unwrap());
        let skill_record = SkillRecord {
            skill_id: skill_document.skill_id.clone(),
            version: skill_document.version.clone(),
            display_name: skill_document.display_name.clone(),
            description: skill_document.description.clone(),
            entry_function: skill_document.entry_function.clone(),
            capabilities: skill_document.capabilities.clone(),
            artifact_path: "data/skills/echo-skill/1.0.0/module.wasm".to_string(),
            artifact_sha256: skill_document.artifact_sha256.clone(),
            source_kind: "signed_publisher".to_string(),
            issuer_did: Some(skill_issuer_did.clone()),
            signature_hex: Some(hex::encode(skill_signature.to_bytes())),
            document_hash: Some(hash_json_value(&skill_document).unwrap()),
            issued_at_unix_ms: Some(skill_document.issued_at_unix_ms),
            active: true,
            created_at_unix_ms: 1_700_000_000_000,
            updated_at_unix_ms: 1_700_000_000_010,
        };
        let skill_distribution = SkillDistributionResponse {
            skills: vec![skill_record],
            active_versions: 1,
            signed_versions: 1,
            trusted_publishers: 1,
        };
        let skill_distribution_hash = hash_json_value(&skill_distribution).unwrap();
        let bundle_hash = hash_json_value(&json!({
            "policyVersion": policy_distribution.profile.version,
            "policyDocumentHash": &policy_document_hash,
            "skillDistributionHash": &skill_distribution_hash
        }))
        .unwrap();

        let mut config = base_config();
        config
            .policy_trust_roots
            .insert(policy_issuer_did, policy_public_key_hex);
        config
            .skill_publisher_trust_roots
            .insert(skill_issuer_did, skill_public_key_hex);

        (
            config,
            GatewayRolloutBundle {
                generated_at_unix_ms: 1_700_000_000_100,
                bundle_hash,
                policy_version: 7,
                policy_document_hash: Some(policy_document_hash),
                skill_distribution_hash,
                policy: policy_distribution,
                skills: skill_distribution,
            },
        )
    }

    #[test]
    fn verifies_signed_rollout_bundle_with_local_trust_roots() {
        let (config, bundle) = signed_rollout_bundle();
        verify_rollout_bundle(&config, &bundle).unwrap();
    }

    #[test]
    fn rejects_unsigned_skill_when_strict_signed_skills_enabled() {
        let (mut config, mut bundle) = signed_rollout_bundle();
        config.require_signed_skills = true;
        bundle.skills.skills.push(SkillRecord {
            skill_id: "local-dev-skill".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Local Dev Skill".to_string(),
            description: None,
            entry_function: "run_skill".to_string(),
            capabilities: vec!["dev".to_string()],
            artifact_path: "data/skills/local-dev-skill/0.1.0/module.wasm".to_string(),
            artifact_sha256: "cd".repeat(32),
            source_kind: "unsigned_local".to_string(),
            issuer_did: None,
            signature_hex: None,
            document_hash: None,
            issued_at_unix_ms: None,
            active: false,
            created_at_unix_ms: 1_700_000_000_000,
            updated_at_unix_ms: 1_700_000_000_010,
        });
        bundle.skill_distribution_hash = hash_json_value(&bundle.skills).unwrap();
        bundle.bundle_hash = hash_json_value(&json!({
            "policyVersion": bundle.policy_version,
            "policyDocumentHash": &bundle.policy_document_hash,
            "skillDistributionHash": &bundle.skill_distribution_hash
        }))
        .unwrap();

        let error = verify_rollout_bundle(&config, &bundle).unwrap_err().to_string();
        assert!(error.contains("unsigned skill"));
    }
}
