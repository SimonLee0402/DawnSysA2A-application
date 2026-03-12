mod cli;
mod profile;

use std::{
    collections::{BTreeMap, HashMap},
    env,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, Method, Url};
use scraper::{Html, Selector};
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
    claim_token: Option<String>,
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
    active_browser_session_id: Option<String>,
    browser_sessions: HashMap<String, BrowserSession>,
}

#[derive(Debug, Clone)]
struct BrowserSession {
    client: Client,
    current_url: String,
    page_html: String,
    title: Option<String>,
    status_code: u16,
    content_type: Option<String>,
    last_action: String,
    loaded_at_unix_ms: u128,
    history: Vec<String>,
    pending_form_selector: Option<String>,
    pending_form_fields: BTreeMap<String, String>,
    pending_form_uploads: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSessionSummary {
    session_id: String,
    current_url: String,
    title: Option<String>,
    status_code: u16,
    content_type: Option<String>,
    last_action: String,
    loaded_at_unix_ms: u128,
    link_count: usize,
    text_preview: String,
    history_depth: usize,
    pending_form_field_count: usize,
    pending_form_upload_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSelectorMatch {
    index: usize,
    tag: String,
    text: String,
    href: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserFormTarget {
    form_index: usize,
    method: String,
    action_url: String,
    selector: String,
    field_names: Vec<String>,
    file_field_names: Vec<String>,
    default_fields: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserPageSnapshot {
    session_id: String,
    current_url: String,
    title: Option<String>,
    text_preview: String,
    headings: Vec<BrowserSelectorMatch>,
    links: Vec<BrowserSelectorMatch>,
    buttons: Vec<BrowserSelectorMatch>,
    forms: Vec<BrowserFormTarget>,
    pending_form_selector: Option<String>,
    pending_form_fields: BTreeMap<String, String>,
    pending_form_uploads: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserTabSummary {
    session_id: String,
    current_url: String,
    title: Option<String>,
    last_action: String,
    loaded_at_unix_ms: u128,
    history_depth: usize,
    pending_form_field_count: usize,
    pending_form_upload_count: usize,
    active: bool,
}

#[derive(Debug)]
struct BrowserTypeTarget {
    form: BrowserFormTarget,
    field_name: String,
    field_tag: String,
    field_type: Option<String>,
    current_value: String,
}

#[derive(Debug)]
struct BrowserUploadTarget {
    form: BrowserFormTarget,
    field_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserDownloadResult {
    session_id: String,
    source_url: String,
    saved_path: String,
    bytes_written: usize,
    content_type: Option<String>,
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

    if let cli::CliOutcome::Exit = cli::dispatch_from_args().await? {
        return Ok(());
    }

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
    let mut url = format!(
        "{}?displayName={}&transport=websocket",
        config.gateway_ws_url,
        url_encode(&config.node_name)
    );
    if let Some(claim_token) = config
        .claim_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        url.push_str("&claimToken=");
        url.push_str(&url_encode(claim_token));
    }
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
            let (Some(command_id), Some(command_type)) =
                (envelope.command_id.clone(), envelope.command_type.clone())
            else {
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
                    runtime_state,
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

async fn execute_command(
    config: &NodeConfig,
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> String {
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
        "system_info" => execute_system_info_command(config, envelope).await,
        "list_directory" => execute_list_directory_command(envelope).await,
        "read_file_preview" => execute_read_file_preview_command(envelope).await,
        "stat_path" => execute_stat_path_command(envelope).await,
        "process_snapshot" => execute_process_snapshot_command(envelope).await,
        "browser_navigate" => execute_browser_navigate_command(runtime_state, envelope).await,
        "browser_extract" => execute_browser_extract_command(runtime_state, envelope).await,
        "browser_click" => execute_browser_click_command(runtime_state, envelope).await,
        "browser_back" => execute_browser_back_command(runtime_state, envelope).await,
        "browser_focus" => execute_browser_focus_command(runtime_state, envelope).await,
        "browser_close" => execute_browser_close_command(runtime_state, envelope).await,
        "browser_tabs" => execute_browser_tabs_command(runtime_state, envelope).await,
        "browser_snapshot" => execute_browser_snapshot_command(runtime_state, envelope).await,
        "browser_type" => execute_browser_type_command(runtime_state, envelope).await,
        "browser_upload" => execute_browser_upload_command(runtime_state, envelope).await,
        "browser_download" => execute_browser_download_command(runtime_state, envelope).await,
        "browser_form_fill" => execute_browser_form_fill_command(runtime_state, envelope).await,
        "browser_form_submit" => execute_browser_form_submit_command(runtime_state, envelope).await,
        "browser_open" => execute_browser_open_command(envelope).await,
        "browser_search" => execute_browser_search_command(envelope).await,
        "desktop_open" => execute_desktop_open_command(envelope).await,
        "desktop_clipboard_set" => execute_desktop_clipboard_set_command(envelope).await,
        "desktop_type_text" => execute_desktop_type_text_command(envelope).await,
        "desktop_key_press" => execute_desktop_key_press_command(envelope).await,
        "desktop_windows_list" => execute_desktop_windows_list_command(envelope).await,
        "desktop_window_focus" => execute_desktop_window_focus_command(envelope).await,
        "desktop_wait_for_window" => execute_desktop_wait_for_window_command(envelope).await,
        "desktop_focus_app" => execute_desktop_focus_app_command(envelope).await,
        "desktop_launch_and_focus" => execute_desktop_launch_and_focus_command(envelope).await,
        "desktop_mouse_move" => execute_desktop_mouse_move_command(envelope).await,
        "desktop_mouse_click" => execute_desktop_mouse_click_command(envelope).await,
        "desktop_screenshot" => execute_desktop_screenshot_command(envelope).await,
        "desktop_accessibility_snapshot" => {
            execute_desktop_accessibility_snapshot_command(envelope).await
        }
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

async fn execute_system_info_command(
    config: &NodeConfig,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let current_dir = env::current_dir()
        .ok()
        .map(|path| path.display().to_string());
    let current_exe = env::current_exe()
        .ok()
        .map(|path| path.display().to_string());
    let cpu_count = std::thread::available_parallelism()
        .ok()
        .map(|count| count.get());
    let username = env::var("USERNAME").ok().or_else(|| env::var("USER").ok());
    let hostname = env::var("COMPUTERNAME")
        .ok()
        .or_else(|| env::var("HOSTNAME").ok());

    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "nodeId": config.node_id,
            "nodeName": config.node_name,
            "issuerDid": config.issuer_did,
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "family": env::consts::FAMILY,
            "currentDir": current_dir,
            "currentExe": current_exe,
            "cpuCount": cpu_count,
            "username": username,
            "hostname": hostname,
            "allowShell": config.allow_shell,
            "capabilities": config.capabilities,
            "observedAtUnixMs": unix_timestamp_ms()
        })),
        error: None,
    }
}

async fn execute_list_directory_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let limit = payload_usize(&envelope.payload, "limit", 50, 500);
    let path_buf = PathBuf::from(path);
    let display_path = path_buf.display().to_string();

    let mut read_dir = match tokio::fs::read_dir(&path_buf).await {
        Ok(read_dir) => read_dir,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "failed to read directory '{}': {error}",
                    display_path
                )),
            };
        }
    };

    let mut entries = Vec::new();
    let mut truncated = false;
    loop {
        match read_dir.next_entry().await {
            Ok(Some(entry)) => {
                if entries.len() >= limit {
                    truncated = true;
                    break;
                }
                let entry_path = entry.path();
                let metadata = entry.metadata().await.ok();
                entries.push(json!({
                    "name": entry.file_name().to_string_lossy().to_string(),
                    "path": entry_path.display().to_string(),
                    "isDir": metadata.as_ref().is_some_and(|meta| meta.is_dir()),
                    "isFile": metadata.as_ref().is_some_and(|meta| meta.is_file()),
                    "len": metadata.as_ref().map(|meta| meta.len()),
                    "modifiedAtUnixMs": metadata
                        .as_ref()
                        .and_then(|meta| meta.modified().ok())
                        .map(system_time_to_unix_ms)
                }));
            }
            Ok(None) => break,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "failed while iterating directory '{}': {error}",
                        display_path
                    )),
                };
            }
        }
    }

    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "path": display_path,
            "entries": entries,
            "count": entries.len(),
            "truncated": truncated
        })),
        error: None,
    }
}

async fn execute_read_file_preview_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(path) = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("read_file_preview requires payload.path".to_string()),
        };
    };

    let max_bytes = payload_usize(&envelope.payload, "maxBytes", 4096, 65_536);
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let preview_len = bytes.len().min(max_bytes);
            let preview = String::from_utf8_lossy(&bytes[..preview_len]).to_string();
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "path": path,
                    "sizeBytes": bytes.len(),
                    "preview": preview,
                    "previewBytes": preview_len,
                    "truncated": bytes.len() > max_bytes
                })),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!("failed to read file '{}': {error}", path)),
        },
    }
}

async fn execute_stat_path_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let Some(path) = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("stat_path requires payload.path".to_string()),
        };
    };

    match tokio::fs::metadata(path).await {
        Ok(metadata) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "path": path,
                "isDir": metadata.is_dir(),
                "isFile": metadata.is_file(),
                "len": metadata.len(),
                "readonly": metadata.permissions().readonly(),
                "modifiedAtUnixMs": metadata.modified().ok().map(system_time_to_unix_ms),
                "createdAtUnixMs": metadata.created().ok().map(system_time_to_unix_ms)
            })),
            error: None,
        },
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!("failed to stat path '{}': {error}", path)),
        },
    }
}

async fn execute_process_snapshot_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let limit = payload_usize(&envelope.payload, "limit", 50, 500);
    let output = if cfg!(target_os = "windows") {
        Command::new("tasklist")
            .arg("/FO")
            .arg("CSV")
            .arg("/NH")
            .output()
            .await
    } else {
        Command::new("ps")
            .arg("-eo")
            .arg("pid=,comm=")
            .output()
            .await
    };

    match output {
        Ok(output) => {
            if !output.status.success() {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: Some(json!({
                        "exitCode": output.status.code(),
                        "stdout": String::from_utf8_lossy(&output.stdout),
                        "stderr": String::from_utf8_lossy(&output.stderr)
                    })),
                    error: Some("process snapshot command failed".to_string()),
                };
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let processes = if cfg!(target_os = "windows") {
                parse_windows_tasklist_snapshot(&stdout, limit)
            } else {
                parse_unix_process_snapshot(&stdout, limit)
            };

            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "count": processes.len(),
                    "limit": limit,
                    "processes": processes
                })),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!("failed to gather process snapshot: {error}")),
        },
    }
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

async fn execute_browser_open_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let raw_url = envelope
        .payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_url.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_open requires payload.url".to_string()),
        };
    }

    let normalized_url = match normalize_browser_url(&raw_url) {
        Ok(url) => url,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    match launch_browser_url(&normalized_url).await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "browser_open",
                "openedUrl": normalized_url,
                "launcher": launcher,
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

async fn execute_browser_search_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let query = envelope
        .payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if query.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_search requires payload.query".to_string()),
        };
    }

    let engine = envelope
        .payload
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("google")
        .trim()
        .to_ascii_lowercase();
    let search_url = match build_browser_search_url(&query, &engine) {
        Ok(url) => url,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    match launch_browser_url(&search_url).await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "browser_search",
                "query": query,
                "engine": engine,
                "openedUrl": search_url,
                "launcher": launcher,
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

async fn execute_desktop_open_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let target = envelope
        .payload
        .get("target")
        .or_else(|| envelope.payload.get("url"))
        .or_else(|| envelope.payload.get("path"))
        .or_else(|| envelope.payload.get("app"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if target.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("desktop_open requires payload.target".to_string()),
        };
    }
    let args = envelope
        .payload
        .get("args")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    match launch_desktop_target(&target, &args).await {
        Ok(result) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_open",
                "target": target,
                "args": args,
                "launcher": result.launcher,
                "mode": result.mode,
                "pid": result.pid,
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

async fn execute_desktop_clipboard_set_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let text = envelope
        .payload
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if text.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("desktop_clipboard_set requires payload.text".to_string()),
        };
    }

    match set_desktop_clipboard(&text).await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_clipboard_set",
                "launcher": launcher,
                "length": text.chars().count(),
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

async fn execute_desktop_type_text_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let text = envelope
        .payload
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if text.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("desktop_type_text requires payload.text".to_string()),
        };
    }
    let delay_ms = envelope
        .payload
        .get("delayMs")
        .and_then(Value::as_u64)
        .unwrap_or(250);

    match send_desktop_text(&text, delay_ms).await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_type_text",
                "launcher": launcher,
                "length": text.chars().count(),
                "delayMs": delay_ms,
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

async fn execute_desktop_key_press_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let keys = envelope
        .payload
        .get("keys")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if keys.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("desktop_key_press requires payload.keys".to_string()),
        };
    }
    let delay_ms = envelope
        .payload
        .get("delayMs")
        .and_then(Value::as_u64)
        .unwrap_or(150);

    match send_desktop_key_press(&keys, delay_ms).await {
        Ok((launcher, send_keys)) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_key_press",
                "launcher": launcher,
                "keys": keys,
                "delayMs": delay_ms,
                "sendKeys": send_keys,
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

#[derive(Debug)]
struct DesktopOpenResult {
    launcher: String,
    mode: String,
    pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopWindowEntry {
    handle: String,
    title: String,
    process_id: u32,
    process_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopScreenshotResult {
    path: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Default)]
struct DesktopWindowSelector {
    title: Option<String>,
    handle: Option<String>,
    process_name: Option<String>,
}

async fn execute_desktop_windows_list_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(DEFAULT_DESKTOP_WINDOW_LIMIT);

    match list_desktop_windows(limit).await {
        Ok(windows) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_windows_list",
                "windowCount": windows.len(),
                "windows": windows,
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

async fn execute_desktop_window_focus_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let selector = desktop_window_selector_from_payload(&envelope.payload);
    if selector.title.is_none() && selector.handle.is_none() && selector.process_name.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_window_focus requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }

    match focus_desktop_window(
        selector.title.as_deref(),
        selector.handle.as_deref(),
        selector.process_name.as_deref(),
    )
    .await
    {
        Ok(window) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_window_focus",
                "window": window,
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

async fn execute_desktop_wait_for_window_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let selector = desktop_window_selector_from_payload(&envelope.payload);
    if selector.title.is_none() && selector.handle.is_none() && selector.process_name.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_wait_for_window requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }
    let timeout_ms = envelope
        .payload
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .unwrap_or(8_000);
    let poll_ms = envelope
        .payload
        .get("pollMs")
        .and_then(Value::as_u64)
        .unwrap_or(250);

    match wait_for_desktop_window(
        selector.title.as_deref(),
        selector.handle.as_deref(),
        selector.process_name.as_deref(),
        timeout_ms,
        poll_ms,
    )
    .await
    {
        Ok(window) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_wait_for_window",
                "timeoutMs": timeout_ms,
                "pollMs": poll_ms,
                "window": window,
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

async fn execute_desktop_focus_app_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let process_name = envelope
        .payload
        .get("processName")
        .or_else(|| envelope.payload.get("app"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if process_name.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_focus_app requires payload.processName or payload.app".to_string(),
            ),
        };
    }

    match focus_desktop_window(None, None, process_name.as_deref()).await {
        Ok(window) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_focus_app",
                "processName": process_name,
                "window": window,
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

async fn execute_desktop_launch_and_focus_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let target = envelope
        .payload
        .get("target")
        .or_else(|| envelope.payload.get("url"))
        .or_else(|| envelope.payload.get("path"))
        .or_else(|| envelope.payload.get("app"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if target.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("desktop_launch_and_focus requires payload.target".to_string()),
        };
    }
    let args = envelope
        .payload
        .get("args")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut selector = desktop_window_selector_from_payload(&envelope.payload);
    if selector.process_name.is_none() {
        selector.process_name = infer_process_name_from_target(&target);
    }
    if selector.title.is_none() && selector.handle.is_none() && selector.process_name.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_launch_and_focus requires payload.title, payload.handle, payload.processName, or an executable-like payload.target"
                    .to_string(),
            ),
        };
    }
    let timeout_ms = envelope
        .payload
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .unwrap_or(10_000);
    let poll_ms = envelope
        .payload
        .get("pollMs")
        .and_then(Value::as_u64)
        .unwrap_or(250);

    match launch_desktop_target(&target, &args).await {
        Ok(launch) => match wait_for_desktop_window(
            selector.title.as_deref(),
            selector.handle.as_deref(),
            selector.process_name.as_deref(),
            timeout_ms,
            poll_ms,
        )
        .await
        {
            Ok(window) => CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "action": "desktop_launch_and_focus",
                    "target": target,
                    "args": args,
                    "launch": {
                        "launcher": launch.launcher,
                        "mode": launch.mode,
                        "pid": launch.pid,
                    },
                    "window": window,
                    "timeoutMs": timeout_ms,
                    "pollMs": poll_ms,
                })),
                error: None,
            },
            Err(error) => CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: Some(json!({
                    "action": "desktop_launch_and_focus",
                    "target": target,
                    "args": args,
                    "launch": {
                        "launcher": launch.launcher,
                        "mode": launch.mode,
                        "pid": launch.pid,
                    },
                })),
                error: Some(error.to_string()),
            },
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

async fn execute_desktop_mouse_move_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let point = match resolve_desktop_point(&envelope.payload) {
        Ok(point) => point,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    match move_desktop_mouse(point.0, point.1).await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_mouse_move",
                "launcher": launcher,
                "x": point.0,
                "y": point.1,
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

async fn execute_desktop_accessibility_snapshot_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let selector = desktop_window_selector_from_payload(&envelope.payload);
    if selector.title.is_none() && selector.handle.is_none() && selector.process_name.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_snapshot requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }
    let depth = envelope
        .payload
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(2);
    let children_limit = envelope
        .payload
        .get("childrenLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(20);

    match accessibility_snapshot_for_window(
        selector.title.as_deref(),
        selector.handle.as_deref(),
        selector.process_name.as_deref(),
        depth,
        children_limit,
    )
    .await
    {
        Ok((window, snapshot)) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_accessibility_snapshot",
                "window": window,
                "depth": depth,
                "childrenLimit": children_limit,
                "snapshot": snapshot,
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

async fn execute_desktop_mouse_click_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let button = envelope
        .payload
        .get("button")
        .and_then(Value::as_str)
        .unwrap_or("left");
    let button = match normalize_desktop_mouse_button(button) {
        Ok(button) => button,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };
    let double_click = envelope
        .payload
        .get("doubleClick")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let point = match resolve_optional_desktop_point(&envelope.payload) {
        Ok(point) => point,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    match click_desktop_mouse(button, double_click, point).await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_mouse_click",
                "launcher": launcher,
                "button": button,
                "doubleClick": double_click,
                "x": point.map(|value| value.0),
                "y": point.map(|value| value.1),
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

async fn execute_desktop_screenshot_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let screenshot_path = resolve_desktop_screenshot_path(
        envelope
            .payload
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim),
    );
    let region = match resolve_desktop_capture_region(&envelope.payload) {
        Ok(region) => region,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    match capture_desktop_screenshot(&screenshot_path, region).await {
        Ok(result) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_screenshot",
                "screenshot": result,
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

const DEFAULT_BROWSER_SESSION_ID: &str = "browser-default";
const MAX_BROWSER_HTML_BYTES: usize = 1_500_000;
const DEFAULT_BROWSER_EXTRACT_LIMIT: usize = 5;
const DEFAULT_BROWSER_TEXT_LIMIT_CHARS: usize = 1_200;
const DEFAULT_BROWSER_SNAPSHOT_LIMIT: usize = 6;
const DEFAULT_DESKTOP_WINDOW_LIMIT: usize = 10;

async fn execute_browser_navigate_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let raw_url = envelope
        .payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_url.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_navigate requires payload.url".to_string()),
        };
    }

    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let normalized_url = match normalize_browser_url(&raw_url) {
        Ok(url) => url,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    match create_browser_session(&normalized_url, "navigate").await {
        Ok(session) => {
            let summary = browser_session_summary(&session_id, &session);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), session);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(serde_json::to_value(summary).unwrap_or_else(|_| json!({}))),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(error.to_string()),
        },
    }
}

async fn execute_browser_back_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    let Some(previous_url) = session.history.last().cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` has no previous page in history"
            )),
        };
    };
    let mut history = session.history.clone();
    history.pop();

    match fetch_browser_session_with_client(
        session.client.clone(),
        &previous_url,
        "back",
        history,
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    )
    .await
    {
        Ok(next_session) => {
            let summary = browser_session_summary(&session_id, &next_session);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), next_session);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "action": "browser_back",
                    "page": summary,
                })),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(error.to_string()),
        },
    }
}

async fn execute_browser_focus_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(session_id) = requested_browser_session_id(&envelope.payload) else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_focus requires payload.sessionId".to_string()),
        };
    };
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    set_active_browser_session(runtime_state, &session_id);
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "sessionId": session_id,
            "active": true,
            "page": browser_session_summary(&session_id, &session),
        })),
        error: None,
    }
}

async fn execute_browser_close_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let Some(session) = runtime_state.browser_sessions.remove(&session_id) else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    runtime_state.active_browser_session_id = next_active_browser_session_id(
        &runtime_state.browser_sessions,
        runtime_state.active_browser_session_id.as_deref(),
        &session_id,
    );
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "closedSessionId": session_id,
            "closedPage": {
                "currentUrl": session.current_url,
                "title": session.title,
            },
            "activeSessionId": runtime_state.active_browser_session_id,
            "remainingTabs": browser_tab_summaries(
                &runtime_state.browser_sessions,
                runtime_state.active_browser_session_id.as_deref(),
            ),
        })),
        error: None,
    }
}

async fn execute_browser_tabs_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let requested_session_id = requested_browser_session_id(&envelope.payload)
        .or_else(|| runtime_state.active_browser_session_id.clone())
        .or_else(|| {
            runtime_state
                .browser_sessions
                .contains_key(DEFAULT_BROWSER_SESSION_ID)
                .then(|| DEFAULT_BROWSER_SESSION_ID.to_string())
        });
    let tabs = browser_tab_summaries(
        &runtime_state.browser_sessions,
        requested_session_id.as_deref(),
    );
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "activeSessionId": requested_session_id,
            "tabs": tabs,
            "count": tabs.len(),
        })),
        error: None,
    }
}

async fn execute_browser_snapshot_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(DEFAULT_BROWSER_SNAPSHOT_LIMIT);
    let limit_chars = envelope
        .payload
        .get("limitChars")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(DEFAULT_BROWSER_TEXT_LIMIT_CHARS);
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    match build_browser_snapshot(&session_id, &session, limit, limit_chars) {
        Ok(snapshot) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(serde_json::to_value(snapshot).unwrap_or_else(|_| json!({}))),
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

async fn execute_browser_type_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let selector = envelope
        .payload
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if selector.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_type requires payload.selector".to_string()),
        };
    }
    let text = envelope
        .payload
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| envelope.payload.get("value").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    let append = envelope
        .payload
        .get("append")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let submit = envelope
        .payload
        .get("submit")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let type_target = {
        let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "browser session `{session_id}` not found. Run browser_navigate first."
                )),
            };
        };
        match resolve_browser_type_target(&session.page_html, &session.current_url, &selector) {
            Ok(target) => (session, target),
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        }
    };
    let (
        staged_session,
        field_name,
        field_tag,
        field_type,
        form_selector,
        form_action_url,
        staged_value,
    ) = {
        let Some(session) = runtime_state.browser_sessions.get_mut(&session_id) else {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "browser session `{session_id}` not found. Run browser_navigate first."
                )),
            };
        };
        let (_, target) = &type_target;
        let base_value = session
            .pending_form_fields
            .get(&target.field_name)
            .cloned()
            .unwrap_or_else(|| target.current_value.clone());
        let staged_value = if append {
            format!("{base_value}{text}")
        } else {
            text.clone()
        };
        session.pending_form_selector = Some(target.form.selector.clone());
        session
            .pending_form_fields
            .insert(target.field_name.clone(), staged_value.clone());
        (
            session.clone(),
            target.field_name.clone(),
            target.field_tag.clone(),
            target.field_type.clone(),
            target.form.selector.clone(),
            target.form.action_url.clone(),
            staged_value,
        )
    };
    if submit {
        let form_selector = staged_session
            .pending_form_selector
            .clone()
            .unwrap_or_else(|| "form".to_string());
        let form = match resolve_browser_form_target(
            &staged_session.page_html,
            &staged_session.current_url,
            &form_selector,
        ) {
            Ok(form) => form,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        };
        let mut history = staged_session.history.clone();
        history.push(staged_session.current_url.clone());
        let merged_fields = staged_session.pending_form_fields.clone();
        let merged_uploads = staged_session.pending_form_uploads.clone();
        match submit_browser_form(
            &staged_session,
            &form,
            merged_fields.clone(),
            merged_uploads.clone(),
            history,
        )
        .await
        {
            Ok(next_session) => {
                let summary = browser_session_summary(&session_id, &next_session);
                runtime_state
                    .browser_sessions
                    .insert(session_id.clone(), next_session);
                set_active_browser_session(runtime_state, &session_id);
                CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "succeeded",
                    result: Some(json!({
                        "sessionId": session_id,
                        "action": "browser_type",
                        "typed": {
                            "selector": selector,
                            "fieldName": field_name,
                            "fieldTag": field_tag,
                            "fieldType": field_type,
                            "value": staged_value,
                            "append": append,
                            "submitted": true,
                        },
                        "form": {
                            "selector": form.selector,
                            "actionUrl": form.action_url,
                            "method": form.method,
                        },
                        "submittedFields": merged_fields,
                        "submittedUploads": merged_uploads,
                        "page": summary,
                    })),
                    error: None,
                }
            }
            Err(error) => CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            },
        }
    } else {
        set_active_browser_session(runtime_state, &session_id);
        let summary = runtime_state
            .browser_sessions
            .get(&session_id)
            .map(|session| browser_session_summary(&session_id, session))
            .unwrap_or_else(|| browser_session_summary(&session_id, &staged_session));
        CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "sessionId": session_id,
                "action": "browser_type",
                "typed": {
                    "selector": selector,
                    "fieldName": field_name,
                    "fieldTag": field_tag,
                    "fieldType": field_type,
                    "value": staged_value,
                    "append": append,
                    "submitted": false,
                },
                "form": {
                    "selector": form_selector,
                    "actionUrl": form_action_url,
                },
                "page": summary,
            })),
            error: None,
        }
    }
}

async fn execute_browser_upload_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let selector = envelope
        .payload
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if selector.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_upload requires payload.selector".to_string()),
        };
    }
    let raw_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if raw_path.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_upload requires payload.path".to_string()),
        };
    }
    let upload_path = PathBuf::from(&raw_path);
    if !upload_path.is_file() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser_upload path `{}` does not exist or is not a file",
                upload_path.display()
            )),
        };
    }
    let submit = envelope
        .payload
        .get("submit")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let staged_path = upload_path.display().to_string();
    let upload_target = {
        let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "browser session `{session_id}` not found. Run browser_navigate first."
                )),
            };
        };
        match resolve_browser_upload_target(&session.page_html, &session.current_url, &selector) {
            Ok(target) => (session, target),
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        }
    };
    let (staged_session, field_name, form_selector, form_action_url) = {
        let Some(session) = runtime_state.browser_sessions.get_mut(&session_id) else {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "browser session `{session_id}` not found. Run browser_navigate first."
                )),
            };
        };
        let (_, target) = &upload_target;
        session.pending_form_selector = Some(target.form.selector.clone());
        session
            .pending_form_uploads
            .insert(target.field_name.clone(), staged_path.clone());
        (
            session.clone(),
            target.field_name.clone(),
            target.form.selector.clone(),
            target.form.action_url.clone(),
        )
    };
    if submit {
        let form_selector = staged_session
            .pending_form_selector
            .clone()
            .unwrap_or_else(|| "form".to_string());
        let form = match resolve_browser_form_target(
            &staged_session.page_html,
            &staged_session.current_url,
            &form_selector,
        ) {
            Ok(form) => form,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        };
        let mut history = staged_session.history.clone();
        history.push(staged_session.current_url.clone());
        let merged_fields = staged_session.pending_form_fields.clone();
        let merged_uploads = staged_session.pending_form_uploads.clone();
        match submit_browser_form(
            &staged_session,
            &form,
            merged_fields.clone(),
            merged_uploads.clone(),
            history,
        )
        .await
        {
            Ok(next_session) => {
                let summary = browser_session_summary(&session_id, &next_session);
                runtime_state
                    .browser_sessions
                    .insert(session_id.clone(), next_session);
                set_active_browser_session(runtime_state, &session_id);
                CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "succeeded",
                    result: Some(json!({
                        "sessionId": session_id,
                        "action": "browser_upload",
                        "uploaded": {
                            "selector": selector,
                            "fieldName": field_name,
                            "path": staged_path,
                            "submitted": true,
                        },
                        "form": {
                            "selector": form.selector,
                            "actionUrl": form.action_url,
                            "method": form.method,
                        },
                        "submittedFields": merged_fields,
                        "submittedUploads": merged_uploads,
                        "page": summary,
                    })),
                    error: None,
                }
            }
            Err(error) => CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            },
        }
    } else {
        set_active_browser_session(runtime_state, &session_id);
        let summary = runtime_state
            .browser_sessions
            .get(&session_id)
            .map(|session| browser_session_summary(&session_id, session))
            .unwrap_or_else(|| browser_session_summary(&session_id, &staged_session));
        CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "sessionId": session_id,
                "action": "browser_upload",
                "uploaded": {
                    "selector": selector,
                    "fieldName": field_name,
                    "path": staged_path,
                    "submitted": false,
                },
                "form": {
                    "selector": form_selector,
                    "actionUrl": form_action_url,
                },
                "page": summary,
            })),
            error: None,
        }
    }
}

async fn execute_browser_download_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    let source_url = if let Some(raw_url) = envelope
        .payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match resolve_browser_download_source(&session.current_url, raw_url) {
            Ok(url) => url,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        }
    } else {
        let selector = envelope
            .payload
            .get("selector")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string();
        if selector.is_empty() {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(
                    "browser_download requires payload.url or payload.selector".to_string(),
                ),
            };
        }
        let element_index = envelope
            .payload
            .get("elementIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        match resolve_browser_click_target(
            &session.page_html,
            &session.current_url,
            &selector,
            element_index,
        ) {
            Ok(target) => target.href,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        }
    };
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let output_path = resolve_browser_download_path(requested_path, &source_url);
    let result =
        match download_browser_resource(&session, &session_id, &source_url, &output_path).await {
            Ok(result) => result,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        };
    set_active_browser_session(runtime_state, &session_id);
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(serde_json::to_value(result).unwrap_or_else(|_| json!({}))),
        error: None,
    }
}

async fn execute_browser_extract_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let selector = envelope
        .payload
        .get("selector")
        .and_then(Value::as_str)
        .unwrap_or("body")
        .trim()
        .to_string();
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(DEFAULT_BROWSER_EXTRACT_LIMIT);
    let limit_chars = envelope
        .payload
        .get("limitChars")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(DEFAULT_BROWSER_TEXT_LIMIT_CHARS);
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };

    match extract_browser_matches(
        &session.page_html,
        &session.current_url,
        &selector,
        limit,
        limit_chars,
    ) {
        Ok(matches) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "sessionId": session_id,
                "currentUrl": session.current_url,
                "title": session.title,
                "selector": selector,
                "matchCount": matches.len(),
                "matches": matches,
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

async fn execute_browser_form_fill_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let form_selector = envelope
        .payload
        .get("formSelector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("form")
        .to_string();
    let Some(session) = runtime_state.browser_sessions.get_mut(&session_id) else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    let fields = match payload_string_map(&envelope.payload, "fields") {
        Ok(fields) if !fields.is_empty() => fields,
        Ok(_) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some("browser_form_fill requires payload.fields".to_string()),
            };
        }
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };
    let form =
        match resolve_browser_form_target(&session.page_html, &session.current_url, &form_selector)
        {
            Ok(form) => form,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        };
    session.pending_form_selector = Some(form_selector);
    for (key, value) in fields {
        session.pending_form_fields.insert(key, value);
    }
    let summary = browser_session_summary(&session_id, session);
    let pending_fields = session.pending_form_fields.clone();
    set_active_browser_session(runtime_state, &session_id);
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "sessionId": session_id,
            "action": "browser_form_fill",
            "form": form,
            "pendingFields": pending_fields,
            "page": summary,
        })),
        error: None,
    }
}

async fn execute_browser_form_submit_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    let form_selector = envelope
        .payload
        .get("formSelector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| session.pending_form_selector.clone())
        .unwrap_or_else(|| "form".to_string());
    let form =
        match resolve_browser_form_target(&session.page_html, &session.current_url, &form_selector)
        {
            Ok(form) => form,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(error.to_string()),
                };
            }
        };
    let inline_fields = match payload_string_map(&envelope.payload, "fields") {
        Ok(fields) => fields,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };
    let inline_uploads = match payload_string_map(&envelope.payload, "uploads") {
        Ok(uploads) => uploads,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };
    let mut merged_fields = form.default_fields.clone();
    for (key, value) in &session.pending_form_fields {
        merged_fields.insert(key.clone(), value.clone());
    }
    for (key, value) in inline_fields {
        merged_fields.insert(key, value);
    }
    let mut merged_uploads = session.pending_form_uploads.clone();
    for (key, value) in inline_uploads {
        merged_uploads.insert(key, value);
    }
    if let Some(submit_name) = envelope.payload.get("submitName").and_then(Value::as_str) {
        let submit_name = submit_name.trim();
        if !submit_name.is_empty() {
            let submit_value = envelope
                .payload
                .get("submitValue")
                .and_then(Value::as_str)
                .unwrap_or("submit")
                .to_string();
            merged_fields.insert(submit_name.to_string(), submit_value);
        }
    }

    let mut history = session.history.clone();
    history.push(session.current_url.clone());
    match submit_browser_form(
        &session,
        &form,
        merged_fields.clone(),
        merged_uploads.clone(),
        history,
    )
    .await
    {
        Ok(next_session) => {
            let summary = browser_session_summary(&session_id, &next_session);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), next_session);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "action": "browser_form_submit",
                    "form": {
                        "selector": form.selector,
                        "method": form.method,
                        "actionUrl": form.action_url,
                    },
                    "submittedFields": merged_fields,
                    "submittedUploads": merged_uploads,
                    "page": summary,
                })),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(error.to_string()),
        },
    }
}

async fn execute_browser_click_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let selector = envelope
        .payload
        .get("selector")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if selector.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_click requires payload.selector".to_string()),
        };
    }
    let element_index = envelope
        .payload
        .get("elementIndex")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let Some(session) = runtime_state.browser_sessions.get(&session_id).cloned() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` not found. Run browser_navigate first."
            )),
        };
    };

    let target = match resolve_browser_click_target(
        &session.page_html,
        &session.current_url,
        &selector,
        element_index,
    ) {
        Ok(target) => target,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    let mut history = session.history.clone();
    history.push(session.current_url.clone());

    match fetch_browser_session_with_client(
        session.client.clone(),
        &target.href,
        "click",
        history,
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    )
    .await
    {
        Ok(next_session) => {
            let summary = browser_session_summary(&session_id, &next_session);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), next_session);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "clicked": {
                        "selector": selector,
                        "elementIndex": element_index,
                        "tag": target.tag,
                        "text": target.text,
                        "href": target.href,
                    },
                    "page": summary,
                })),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(error.to_string()),
        },
    }
}

#[derive(Debug, Clone)]
struct BrowserClickTarget {
    tag: String,
    text: String,
    href: String,
}

fn requested_browser_session_id(payload: &Value) -> Option<String> {
    payload
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn resolve_browser_session_id(runtime_state: &NodeRuntimeState, payload: &Value) -> String {
    requested_browser_session_id(payload)
        .or_else(|| runtime_state.active_browser_session_id.clone())
        .unwrap_or_else(|| DEFAULT_BROWSER_SESSION_ID.to_string())
}

fn set_active_browser_session(runtime_state: &mut NodeRuntimeState, session_id: &str) {
    runtime_state.active_browser_session_id = Some(session_id.to_string());
}

fn next_active_browser_session_id(
    browser_sessions: &HashMap<String, BrowserSession>,
    current_active_session_id: Option<&str>,
    closed_session_id: &str,
) -> Option<String> {
    if let Some(active_session_id) = current_active_session_id
        .filter(|active_session_id| *active_session_id != closed_session_id)
        .filter(|active_session_id| browser_sessions.contains_key(*active_session_id))
    {
        return Some(active_session_id.to_string());
    }
    if browser_sessions.contains_key(DEFAULT_BROWSER_SESSION_ID) {
        return Some(DEFAULT_BROWSER_SESSION_ID.to_string());
    }
    browser_sessions
        .iter()
        .max_by(|left, right| {
            left.1
                .loaded_at_unix_ms
                .cmp(&right.1.loaded_at_unix_ms)
                .then_with(|| left.0.cmp(right.0))
        })
        .map(|(session_id, _)| session_id.clone())
}

fn resolve_browser_download_source(current_url: &str, raw: &str) -> anyhow::Result<String> {
    match Url::parse(raw) {
        Ok(url) => match url.scheme() {
            "http" | "https" => Ok(url.to_string()),
            other => anyhow::bail!("browser_download only supports http/https URLs, got `{other}`"),
        },
        Err(_) => resolve_browser_href(current_url, raw),
    }
}

fn resolve_browser_download_path(requested: Option<&str>, source_url: &str) -> PathBuf {
    let inferred_name = infer_browser_download_file_name(source_url);
    match requested.map(PathBuf::from) {
        Some(path) if path.is_dir() => path.join(inferred_name),
        Some(path) if path.to_string_lossy().ends_with(['\\', '/']) => path.join(inferred_name),
        Some(path) => path,
        None => env::temp_dir().join(inferred_name),
    }
}

fn infer_browser_download_file_name(source_url: &str) -> String {
    let candidate = Url::parse(source_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|segments| {
                    segments
                        .filter(|segment| !segment.trim().is_empty())
                        .next_back()
                        .map(ToString::to_string)
                })
                .filter(|segment| !segment.trim().is_empty())
        })
        .unwrap_or_else(|| format!("browser-download-{}.bin", unix_timestamp_ms()));
    sanitize_browser_download_file_name(&candidate)
}

fn sanitize_browser_download_file_name(raw: &str) -> String {
    let sanitized = raw
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            other if other.is_control() => '_',
            other => other,
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        format!("browser-download-{}.bin", unix_timestamp_ms())
    } else {
        sanitized
    }
}

async fn download_browser_resource(
    session: &BrowserSession,
    session_id: &str,
    source_url: &str,
    output_path: &PathBuf,
) -> anyhow::Result<BrowserDownloadResult> {
    let response = session
        .client
        .get(source_url)
        .header(
            reqwest::header::USER_AGENT,
            "DawnNode/0.1 browser-session-mvp",
        )
        .send()
        .await
        .with_context(|| format!("failed to download browser resource {source_url}"))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("browser_download received HTTP {}", status.as_u16());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read browser download body from {source_url}"))?;
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create download directory {}", parent.display()))?;
    }
    tokio::fs::write(output_path, &bytes)
        .await
        .with_context(|| {
            format!(
                "failed to save browser download to {}",
                output_path.display()
            )
        })?;
    Ok(BrowserDownloadResult {
        session_id: session_id.to_string(),
        source_url: source_url.to_string(),
        saved_path: output_path.display().to_string(),
        bytes_written: bytes.len(),
        content_type,
    })
}

async fn fetch_browser_session(url: &str, action: &str) -> anyhow::Result<BrowserSession> {
    let client = new_browser_client()?;
    fetch_browser_session_with_client(
        client,
        url,
        action,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    )
    .await
}

fn new_browser_client() -> anyhow::Result<Client> {
    Client::builder()
        .cookie_store(true)
        .build()
        .context("failed to build browser session HTTP client")
}

async fn create_browser_session(url: &str, action: &str) -> anyhow::Result<BrowserSession> {
    fetch_browser_session(url, action).await
}

async fn fetch_browser_session_with_client(
    client: Client,
    url: &str,
    action: &str,
    history: Vec<String>,
    pending_form_fields: BTreeMap<String, String>,
    pending_form_uploads: BTreeMap<String, String>,
    pending_form_selector: Option<String>,
) -> anyhow::Result<BrowserSession> {
    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "DawnNode/0.1 browser-session-mvp",
        )
        .send()
        .await
        .with_context(|| format!("failed to fetch browser URL {url}"))?;
    finalize_browser_response(
        client,
        response,
        action,
        history,
        pending_form_fields,
        pending_form_uploads,
        pending_form_selector,
    )
    .await
}

async fn finalize_browser_response(
    client: Client,
    response: reqwest::Response,
    action: &str,
    history: Vec<String>,
    pending_form_fields: BTreeMap<String, String>,
    pending_form_uploads: BTreeMap<String, String>,
    pending_form_selector: Option<String>,
) -> anyhow::Result<BrowserSession> {
    let status = response.status();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let body = response.bytes().await?;
    if !status.is_success() {
        anyhow::bail!(
            "browser request failed with status {} for {}",
            status,
            final_url
        );
    }

    let page_html =
        String::from_utf8_lossy(&body[..body.len().min(MAX_BROWSER_HTML_BYTES)]).to_string();
    let document = Html::parse_document(&page_html);
    let title = extract_browser_title(&document);

    Ok(BrowserSession {
        client,
        current_url: final_url,
        page_html,
        title,
        status_code: status.as_u16(),
        content_type,
        last_action: action.to_string(),
        loaded_at_unix_ms: unix_timestamp_ms(),
        history,
        pending_form_selector,
        pending_form_fields,
        pending_form_uploads,
    })
}

fn browser_session_summary(session_id: &str, session: &BrowserSession) -> BrowserSessionSummary {
    let document = Html::parse_document(&session.page_html);
    BrowserSessionSummary {
        session_id: session_id.to_string(),
        current_url: session.current_url.clone(),
        title: session.title.clone(),
        status_code: session.status_code,
        content_type: session.content_type.clone(),
        last_action: session.last_action.clone(),
        loaded_at_unix_ms: session.loaded_at_unix_ms,
        link_count: count_browser_links(&document),
        text_preview: extract_document_text(&document, DEFAULT_BROWSER_TEXT_LIMIT_CHARS),
        history_depth: session.history.len(),
        pending_form_field_count: session.pending_form_fields.len(),
        pending_form_upload_count: session.pending_form_uploads.len(),
    }
}

fn browser_tab_summaries(
    browser_sessions: &HashMap<String, BrowserSession>,
    active_session_id: Option<&str>,
) -> Vec<BrowserTabSummary> {
    let mut tabs = browser_sessions
        .iter()
        .map(|(session_id, session)| BrowserTabSummary {
            session_id: session_id.clone(),
            current_url: session.current_url.clone(),
            title: session.title.clone(),
            last_action: session.last_action.clone(),
            loaded_at_unix_ms: session.loaded_at_unix_ms,
            history_depth: session.history.len(),
            pending_form_field_count: session.pending_form_fields.len(),
            pending_form_upload_count: session.pending_form_uploads.len(),
            active: active_session_id
                .map(|active_id| active_id == session_id)
                .unwrap_or(false),
        })
        .collect::<Vec<_>>();
    tabs.sort_by(|left, right| {
        right
            .loaded_at_unix_ms
            .cmp(&left.loaded_at_unix_ms)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    tabs
}

fn build_browser_snapshot(
    session_id: &str,
    session: &BrowserSession,
    limit: usize,
    limit_chars: usize,
) -> anyhow::Result<BrowserPageSnapshot> {
    let document = Html::parse_document(&session.page_html);
    Ok(BrowserPageSnapshot {
        session_id: session_id.to_string(),
        current_url: session.current_url.clone(),
        title: session.title.clone(),
        text_preview: extract_document_text(&document, limit_chars),
        headings: extract_browser_matches(
            &session.page_html,
            &session.current_url,
            "h1, h2, h3",
            limit,
            limit_chars,
        )?,
        links: extract_browser_matches(
            &session.page_html,
            &session.current_url,
            "a[href]",
            limit,
            limit_chars,
        )?,
        buttons: extract_browser_matches(
            &session.page_html,
            &session.current_url,
            "button, input[type=\"submit\"], input[type=\"button\"]",
            limit,
            limit_chars,
        )?,
        forms: extract_browser_forms(&session.page_html, &session.current_url, limit)?,
        pending_form_selector: session.pending_form_selector.clone(),
        pending_form_fields: session.pending_form_fields.clone(),
        pending_form_uploads: session.pending_form_uploads.clone(),
    })
}

fn extract_browser_matches(
    html: &str,
    current_url: &str,
    selector: &str,
    limit: usize,
    limit_chars: usize,
) -> anyhow::Result<Vec<BrowserSelectorMatch>> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(selector)
        .map_err(|_| anyhow::anyhow!("invalid CSS selector `{selector}`"))?;
    Ok(document
        .select(&selector)
        .take(limit.max(1))
        .enumerate()
        .map(|(index, element)| BrowserSelectorMatch {
            index,
            tag: element.value().name().to_string(),
            text: truncate_chars(
                &normalize_browser_text(&element.text().collect::<Vec<_>>().join(" ")),
                limit_chars,
            ),
            href: element
                .value()
                .attr("href")
                .and_then(|href| resolve_browser_href(current_url, href).ok()),
        })
        .collect())
}

fn resolve_browser_click_target(
    html: &str,
    current_url: &str,
    selector: &str,
    element_index: usize,
) -> anyhow::Result<BrowserClickTarget> {
    let document = Html::parse_document(html);
    let selector_text = selector.to_string();
    let selector = Selector::parse(&selector_text)
        .map_err(|_| anyhow::anyhow!("invalid CSS selector `{selector_text}`"))?;
    let Some(element) = document.select(&selector).nth(element_index) else {
        anyhow::bail!("no element matched selector `{selector_text}` at index {element_index}");
    };
    let href = element.value().attr("href").ok_or_else(|| {
        anyhow::anyhow!("browser_click currently supports link elements with href attributes")
    })?;
    Ok(BrowserClickTarget {
        tag: element.value().name().to_string(),
        text: normalize_browser_text(&element.text().collect::<Vec<_>>().join(" ")),
        href: resolve_browser_href(current_url, href)?,
    })
}

fn resolve_browser_form_target(
    html: &str,
    current_url: &str,
    selector: &str,
) -> anyhow::Result<BrowserFormTarget> {
    let document = Html::parse_document(html);
    let selector_text = selector.trim();
    let Some((form_index, form)) = select_browser_form(&document, selector_text)? else {
        anyhow::bail!("no form matched selector `{selector_text}`");
    };
    if form.value().name() != "form" {
        anyhow::bail!("browser form selector `{selector_text}` must match a <form> element");
    }
    let method = normalize_browser_form_method(form.value().attr("method"));
    let action_url = resolve_browser_form_action(current_url, form.value().attr("action"))?;
    let default_fields = extract_browser_form_defaults(&form);
    let file_field_names = extract_browser_form_file_field_names(&form);
    let field_names = default_fields.keys().cloned().collect::<Vec<_>>();
    Ok(BrowserFormTarget {
        form_index,
        method: method.as_str().to_string(),
        action_url,
        selector: selector_text.to_string(),
        field_names,
        file_field_names,
        default_fields,
    })
}

fn extract_browser_forms(
    html: &str,
    current_url: &str,
    limit: usize,
) -> anyhow::Result<Vec<BrowserFormTarget>> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("form").expect("valid form selector");
    Ok(document
        .select(&selector)
        .enumerate()
        .take(limit.max(1))
        .map(|(form_index, form)| {
            let method = normalize_browser_form_method(form.value().attr("method"));
            let action_url = resolve_browser_form_action(current_url, form.value().attr("action"))?;
            let default_fields = extract_browser_form_defaults(&form);
            let file_field_names = extract_browser_form_file_field_names(&form);
            let field_names = default_fields.keys().cloned().collect::<Vec<_>>();
            Ok(BrowserFormTarget {
                form_index,
                method: method.as_str().to_string(),
                action_url,
                selector: browser_form_selector_for_index(&form, form_index),
                field_names,
                file_field_names,
                default_fields,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?)
}

fn resolve_browser_type_target(
    html: &str,
    current_url: &str,
    selector: &str,
) -> anyhow::Result<BrowserTypeTarget> {
    let document = Html::parse_document(html);
    let selector_text = selector.trim();
    let field_selector = Selector::parse(selector_text)
        .map_err(|_| anyhow::anyhow!("invalid CSS selector `{selector_text}`"))?;
    let Some(field) = document.select(&field_selector).next() else {
        anyhow::bail!("no field matched selector `{selector_text}`");
    };
    if field.value().attr("disabled").is_some() {
        anyhow::bail!("browser_type cannot target disabled fields");
    }
    let field_tag = field.value().name().to_string();
    if !matches!(field_tag.as_str(), "input" | "textarea" | "select") {
        anyhow::bail!("browser_type currently supports input, textarea, and select elements");
    }
    let field_type = field
        .value()
        .attr("type")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if field_type.as_deref() == Some("file") {
        anyhow::bail!("browser_type does not support file inputs; use browser_upload instead");
    }
    let field_name = field
        .value()
        .attr("name")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("browser_type requires the target field to have a name attribute")
        })?
        .to_string();
    let form_selector = resolve_browser_form_selector_for_field(&document, selector_text, &field)?;
    let form = resolve_browser_form_target(html, current_url, &form_selector)?;
    Ok(BrowserTypeTarget {
        form,
        field_name,
        field_tag,
        field_type,
        current_value: extract_browser_field_value(&field),
    })
}

fn resolve_browser_upload_target(
    html: &str,
    current_url: &str,
    selector: &str,
) -> anyhow::Result<BrowserUploadTarget> {
    let document = Html::parse_document(html);
    let selector_text = selector.trim();
    let field_selector = Selector::parse(selector_text)
        .map_err(|_| anyhow::anyhow!("invalid CSS selector `{selector_text}`"))?;
    let Some(field) = document.select(&field_selector).next() else {
        anyhow::bail!("no field matched selector `{selector_text}`");
    };
    if field.value().attr("disabled").is_some() {
        anyhow::bail!("browser_upload cannot target disabled fields");
    }
    if field.value().name() != "input" {
        anyhow::bail!("browser_upload currently supports <input type=\"file\"> elements only");
    }
    let input_type = field
        .value()
        .attr("type")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if input_type != "file" {
        anyhow::bail!("browser_upload requires a file input selector");
    }
    let field_name = field
        .value()
        .attr("name")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("browser_upload requires the target input to have a name attribute")
        })?
        .to_string();
    let form_selector = resolve_browser_form_selector_for_field(&document, selector_text, &field)?;
    let form = resolve_browser_form_target(html, current_url, &form_selector)?;
    Ok(BrowserUploadTarget { form, field_name })
}

fn extract_browser_form_defaults(
    form: &scraper::element_ref::ElementRef<'_>,
) -> BTreeMap<String, String> {
    let input_selector = Selector::parse("input[name]").ok();
    let textarea_selector = Selector::parse("textarea[name]").ok();
    let select_selector = Selector::parse("select[name]").ok();
    let option_selector = Selector::parse("option").ok();
    let mut fields = BTreeMap::new();

    if let Some(selector) = input_selector {
        for input in form.select(&selector) {
            let Some(name) = input
                .value()
                .attr("name")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if input.value().attr("disabled").is_some() {
                continue;
            }
            let input_type = input
                .value()
                .attr("type")
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            match input_type.as_str() {
                "checkbox" | "radio" => {
                    if input.value().attr("checked").is_some() {
                        fields.insert(
                            name.to_string(),
                            input.value().attr("value").unwrap_or("on").to_string(),
                        );
                    }
                }
                "submit" | "button" | "image" | "file" => {}
                _ => {
                    fields.insert(
                        name.to_string(),
                        input.value().attr("value").unwrap_or_default().to_string(),
                    );
                }
            }
        }
    }

    if let Some(selector) = textarea_selector {
        for textarea in form.select(&selector) {
            let Some(name) = textarea
                .value()
                .attr("name")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if textarea.value().attr("disabled").is_some() {
                continue;
            }
            fields.insert(
                name.to_string(),
                normalize_browser_text(&textarea.text().collect::<Vec<_>>().join(" ")),
            );
        }
    }

    if let (Some(select_selector), Some(option_selector)) = (select_selector, option_selector) {
        for select in form.select(&select_selector) {
            let Some(name) = select
                .value()
                .attr("name")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if select.value().attr("disabled").is_some() {
                continue;
            }
            let mut chosen = None;
            for option in select.select(&option_selector) {
                if option.value().attr("selected").is_some() {
                    chosen = Some(
                        option
                            .value()
                            .attr("value")
                            .map(ToString::to_string)
                            .unwrap_or_else(|| {
                                normalize_browser_text(&option.text().collect::<Vec<_>>().join(" "))
                            }),
                    );
                    break;
                }
                if chosen.is_none() {
                    chosen = Some(
                        option
                            .value()
                            .attr("value")
                            .map(ToString::to_string)
                            .unwrap_or_else(|| {
                                normalize_browser_text(&option.text().collect::<Vec<_>>().join(" "))
                            }),
                    );
                }
            }
            fields.insert(name.to_string(), chosen.unwrap_or_default());
        }
    }

    fields
}

fn extract_browser_form_file_field_names(
    form: &scraper::element_ref::ElementRef<'_>,
) -> Vec<String> {
    let selector = match Selector::parse("input[type=\"file\"][name]") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    form.select(&selector)
        .filter_map(|input| {
            input
                .value()
                .attr("name")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .collect()
}

fn normalize_browser_form_method(raw: Option<&str>) -> Method {
    match raw.unwrap_or("GET").trim().to_ascii_uppercase().as_str() {
        "POST" => Method::POST,
        _ => Method::GET,
    }
}

fn resolve_browser_form_action(current_url: &str, action: Option<&str>) -> anyhow::Result<String> {
    let raw = action.unwrap_or_default().trim();
    if raw.is_empty() {
        return Ok(current_url.to_string());
    }
    resolve_browser_href(current_url, raw)
}

fn payload_string_map(payload: &Value, field: &str) -> anyhow::Result<BTreeMap<String, String>> {
    let Some(object) = payload.get(field) else {
        return Ok(BTreeMap::new());
    };
    let Some(map) = object.as_object() else {
        anyhow::bail!("payload.{field} must be an object of string-like values");
    };
    let mut result = BTreeMap::new();
    for (key, value) in map {
        let normalized = match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
            Value::Null => String::new(),
            _ => anyhow::bail!("payload.{field}.{key} must be a scalar value"),
        };
        result.insert(key.clone(), normalized);
    }
    Ok(result)
}

fn select_browser_form<'a>(
    document: &'a Html,
    selector_text: &str,
) -> anyhow::Result<Option<(usize, scraper::element_ref::ElementRef<'a>)>> {
    if let Some(index_text) = selector_text.strip_prefix("@form-index:") {
        let form_index = index_text
            .trim()
            .parse::<usize>()
            .with_context(|| format!("invalid browser form selector `{selector_text}`"))?;
        let selector = Selector::parse("form").expect("valid form selector");
        return Ok(document.select(&selector).enumerate().nth(form_index));
    }
    let selector = Selector::parse(selector_text)
        .map_err(|_| anyhow::anyhow!("invalid CSS selector `{selector_text}`"))?;
    Ok(document
        .select(&selector)
        .next()
        .and_then(|form| find_browser_form_index(document, &form).map(|index| (index, form))))
}

fn find_browser_form_index(
    document: &Html,
    target: &scraper::element_ref::ElementRef<'_>,
) -> Option<usize> {
    let selector = Selector::parse("form").ok()?;
    document
        .select(&selector)
        .enumerate()
        .find_map(|(index, form)| {
            let target_id = target.id();
            (form.id() == target_id).then_some(index)
        })
}

fn browser_form_selector_for_index(
    form: &scraper::element_ref::ElementRef<'_>,
    form_index: usize,
) -> String {
    form.value()
        .attr("id")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("form#{value}"))
        .unwrap_or_else(|| format!("@form-index:{form_index}"))
}

fn resolve_browser_form_selector_for_field(
    document: &Html,
    field_selector_text: &str,
    field: &scraper::element_ref::ElementRef<'_>,
) -> anyhow::Result<String> {
    if let Some(form_id) = field
        .value()
        .attr("form")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(format!("form#{form_id}"));
    }
    let form_selector = Selector::parse("form").expect("valid form selector");
    let field_selector = Selector::parse(field_selector_text)
        .map_err(|_| anyhow::anyhow!("invalid CSS selector `{field_selector_text}`"))?;
    for (form_index, form) in document.select(&form_selector).enumerate() {
        if form.select(&field_selector).next().is_some() {
            return Ok(browser_form_selector_for_index(&form, form_index));
        }
    }
    anyhow::bail!("browser_type requires the target field to belong to a form");
}

fn extract_browser_field_value(field: &scraper::element_ref::ElementRef<'_>) -> String {
    match field.value().name() {
        "textarea" => normalize_browser_text(&field.text().collect::<Vec<_>>().join(" ")),
        "select" => {
            let option_selector = match Selector::parse("option") {
                Ok(selector) => selector,
                Err(_) => return String::new(),
            };
            let mut first_value = None;
            for option in field.select(&option_selector) {
                let value = option
                    .value()
                    .attr("value")
                    .map(ToString::to_string)
                    .unwrap_or_else(|| {
                        normalize_browser_text(&option.text().collect::<Vec<_>>().join(" "))
                    });
                if option.value().attr("selected").is_some() {
                    return value;
                }
                if first_value.is_none() {
                    first_value = Some(value);
                }
            }
            first_value.unwrap_or_default()
        }
        _ => field.value().attr("value").unwrap_or_default().to_string(),
    }
}

async fn submit_browser_form(
    session: &BrowserSession,
    form: &BrowserFormTarget,
    fields: BTreeMap<String, String>,
    uploads: BTreeMap<String, String>,
    history: Vec<String>,
) -> anyhow::Result<BrowserSession> {
    let method = normalize_browser_form_method(Some(form.method.as_str()));
    let request_builder = if uploads.is_empty() {
        match method {
            Method::POST => session.client.post(&form.action_url).form(&fields).header(
                reqwest::header::USER_AGENT,
                "DawnNode/0.1 browser-session-mvp",
            ),
            _ => session.client.get(&form.action_url).query(&fields).header(
                reqwest::header::USER_AGENT,
                "DawnNode/0.1 browser-session-mvp",
            ),
        }
    } else {
        if method != Method::POST {
            anyhow::bail!("browser form uploads currently require method=POST");
        }
        let mut multipart = reqwest::multipart::Form::new();
        for (key, value) in fields {
            multipart = multipart.text(key, value);
        }
        for (field_name, path_text) in uploads {
            let path = PathBuf::from(&path_text);
            if !path.is_file() {
                anyhow::bail!(
                    "browser upload path `{}` does not exist or is not a file",
                    path.display()
                );
            }
            let file_name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "upload.bin".to_string());
            let bytes = tokio::fs::read(&path)
                .await
                .with_context(|| format!("failed to read browser upload {}", path.display()))?;
            let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
            multipart = multipart.part(field_name, part);
        }
        session
            .client
            .post(&form.action_url)
            .multipart(multipart)
            .header(
                reqwest::header::USER_AGENT,
                "DawnNode/0.1 browser-session-mvp",
            )
    };
    let response = request_builder
        .send()
        .await
        .with_context(|| format!("failed to submit browser form {}", form.action_url))?;
    finalize_browser_response(
        session.client.clone(),
        response,
        "form_submit",
        history,
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    )
    .await
}

fn resolve_browser_href(current_url: &str, href: &str) -> anyhow::Result<String> {
    let base = Url::parse(current_url).context("failed to parse current browser session URL")?;
    let resolved = base.join(href).context("failed to resolve browser href")?;
    match resolved.scheme() {
        "http" | "https" => Ok(resolved.to_string()),
        other => anyhow::bail!("browser session only supports http/https links, got `{other}`"),
    }
}

fn extract_browser_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    document
        .select(&selector)
        .next()
        .map(|element| normalize_browser_text(&element.text().collect::<Vec<_>>().join(" ")))
}

fn count_browser_links(document: &Html) -> usize {
    Selector::parse("a[href]")
        .ok()
        .map(|selector| document.select(&selector).count())
        .unwrap_or(0)
}

fn extract_document_text(document: &Html, limit_chars: usize) -> String {
    let selector = Selector::parse("body").ok();
    let text = selector
        .and_then(|selector| document.select(&selector).next())
        .map(|body| body.text().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    truncate_chars(&normalize_browser_text(&text), limit_chars)
}

fn normalize_browser_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(raw: &str, limit_chars: usize) -> String {
    let mut chars = raw.chars();
    let collected = chars.by_ref().take(limit_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{collected}...")
    } else {
        collected
    }
}

async fn launch_desktop_target(target: &str, args: &[String]) -> anyhow::Result<DesktopOpenResult> {
    let normalized_target = target.trim();
    if normalized_target.is_empty() {
        anyhow::bail!("desktop target cannot be empty");
    }
    if normalized_target.contains("://") {
        let url = normalize_browser_url(normalized_target)?;
        let launcher = launch_browser_url_spawn(&url).await?;
        return Ok(DesktopOpenResult {
            launcher: launcher.0.to_string(),
            mode: "url".to_string(),
            pid: launcher.1,
        });
    }

    let path = PathBuf::from(normalized_target);
    if path.exists() {
        let launcher = launch_local_path(path).await?;
        return Ok(DesktopOpenResult {
            launcher: launcher.0.to_string(),
            mode: "path".to_string(),
            pid: launcher.1,
        });
    }

    launch_executable_target(normalized_target, args).await
}

async fn launch_local_path(path: PathBuf) -> anyhow::Result<(&'static str, Option<u32>)> {
    if cfg!(target_os = "windows") {
        let child = Command::new("explorer")
            .arg(path.as_os_str())
            .spawn()
            .context("failed to launch explorer for desktop path")?;
        Ok(("explorer", child.id()))
    } else if cfg!(target_os = "macos") {
        let child = Command::new("open")
            .arg(path.as_os_str())
            .spawn()
            .context("failed to launch open for desktop path")?;
        Ok(("open", child.id()))
    } else {
        let child = Command::new("xdg-open")
            .arg(path.as_os_str())
            .spawn()
            .context("failed to launch xdg-open for desktop path")?;
        Ok(("xdg-open", child.id()))
    }
}

async fn launch_executable_target(
    target: &str,
    args: &[String],
) -> anyhow::Result<DesktopOpenResult> {
    let mut command = Command::new(target);
    command.args(args);
    let child = command
        .spawn()
        .with_context(|| format!("failed to launch desktop target `{target}`"))?;
    Ok(DesktopOpenResult {
        launcher: target.to_string(),
        mode: "executable".to_string(),
        pid: child.id(),
    })
}

async fn launch_browser_url_spawn(url: &str) -> anyhow::Result<(&'static str, Option<u32>)> {
    let child = if cfg!(target_os = "windows") {
        Command::new("explorer")
            .arg(url)
            .spawn()
            .context("failed to launch explorer for browser URL")?
    } else if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(url)
            .spawn()
            .context("failed to launch open for browser URL")?
    } else {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("failed to launch xdg-open for browser URL")?
    };
    let launcher = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Ok((launcher, child.id()))
}

async fn set_desktop_clipboard(text: &str) -> anyhow::Result<&'static str> {
    if cfg!(target_os = "windows") {
        let mut command = Command::new("powershell");
        command
            .arg("-NoProfile")
            .arg("-Command")
            .arg("Set-Clipboard -Value $env:DAWN_CLIPBOARD_TEXT")
            .env("DAWN_CLIPBOARD_TEXT", text);
        let status = command.status().await?;
        if !status.success() {
            anyhow::bail!("Set-Clipboard exited with status {:?}", status.code());
        }
        Ok("powershell:Set-Clipboard")
    } else if cfg!(target_os = "macos") {
        let mut child = Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("failed to launch pbcopy")?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(text.as_bytes()).await?;
        }
        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("pbcopy exited with status {:?}", status.code());
        }
        Ok("pbcopy")
    } else {
        anyhow::bail!("desktop clipboard set is currently implemented for Windows and macOS only")
    }
}

async fn send_desktop_text(text: &str, delay_ms: u64) -> anyhow::Result<&'static str> {
    if cfg!(target_os = "windows") {
        let send_keys = encode_windows_send_keys_text(text);
        return run_windows_send_keys(&send_keys, delay_ms).await;
    }
    anyhow::bail!("desktop_type_text is currently implemented for Windows only")
}

async fn send_desktop_key_press(
    keys: &str,
    delay_ms: u64,
) -> anyhow::Result<(&'static str, String)> {
    if cfg!(target_os = "windows") {
        let send_keys = build_windows_send_keys_combo(keys)?;
        run_windows_send_keys(&send_keys, delay_ms).await?;
        return Ok(("powershell:WScript.Shell.SendKeys", send_keys));
    }
    anyhow::bail!("desktop_key_press is currently implemented for Windows only")
}

async fn run_windows_send_keys(send_keys: &str, delay_ms: u64) -> anyhow::Result<&'static str> {
    let script = "$wshell = New-Object -ComObject WScript.Shell; Start-Sleep -Milliseconds ([int]$env:DAWN_SEND_KEYS_DELAY_MS); $wshell.SendKeys($env:DAWN_SEND_KEYS_SEQUENCE)";
    let mut command = Command::new("powershell");
    command
        .arg("-NoProfile")
        .arg("-Command")
        .arg(script)
        .env("DAWN_SEND_KEYS_SEQUENCE", send_keys)
        .env("DAWN_SEND_KEYS_DELAY_MS", delay_ms.to_string());
    let status = command.status().await?;
    if !status.success() {
        anyhow::bail!("SendKeys exited with status {:?}", status.code());
    }
    Ok("powershell:WScript.Shell.SendKeys")
}

fn encode_windows_send_keys_text(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '+' => "{+}".to_string(),
            '^' => "{^}".to_string(),
            '%' => "{%}".to_string(),
            '~' => "{~}".to_string(),
            '(' => "{(}".to_string(),
            ')' => "{)}".to_string(),
            '[' => "{[}".to_string(),
            ']' => "{]}".to_string(),
            '{' => "{{}".to_string(),
            '}' => "{}}".to_string(),
            '\n' => "{ENTER}".to_string(),
            '\r' => String::new(),
            '\t' => "{TAB}".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
}

fn build_windows_send_keys_combo(keys: &str) -> anyhow::Result<String> {
    let tokens = keys
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        anyhow::bail!("desktop key combo cannot be empty");
    }

    let mut modifiers = String::new();
    let mut primary = None;
    for token in tokens {
        match token.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => modifiers.push('^'),
            "ALT" => modifiers.push('%'),
            "SHIFT" => modifiers.push('+'),
            other => {
                if primary.is_some() {
                    anyhow::bail!("desktop key combo supports one non-modifier key, got `{other}`");
                }
                primary = Some(map_windows_send_keys_key(other)?);
            }
        }
    }
    let primary = primary.ok_or_else(|| anyhow::anyhow!("desktop key combo is missing a key"))?;
    if modifiers.is_empty() {
        Ok(primary)
    } else {
        Ok(format!("{modifiers}({primary})"))
    }
}

fn map_windows_send_keys_key(token: &str) -> anyhow::Result<String> {
    let mapped = match token {
        "ENTER" | "RETURN" => "{ENTER}".to_string(),
        "TAB" => "{TAB}".to_string(),
        "ESC" | "ESCAPE" => "{ESC}".to_string(),
        "SPACE" => " ".to_string(),
        "BACKSPACE" => "{BACKSPACE}".to_string(),
        "DELETE" | "DEL" => "{DELETE}".to_string(),
        "UP" | "ARROWUP" => "{UP}".to_string(),
        "DOWN" | "ARROWDOWN" => "{DOWN}".to_string(),
        "LEFT" | "ARROWLEFT" => "{LEFT}".to_string(),
        "RIGHT" | "ARROWRIGHT" => "{RIGHT}".to_string(),
        "HOME" => "{HOME}".to_string(),
        "END" => "{END}".to_string(),
        "PGUP" | "PAGEUP" => "{PGUP}".to_string(),
        "PGDN" | "PAGEDOWN" => "{PGDN}".to_string(),
        "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12" => {
            format!("{{{token}}}")
        }
        other if other.len() == 1 => encode_windows_send_keys_text(other),
        other => anyhow::bail!("unsupported desktop key `{other}`"),
    };
    Ok(mapped)
}

async fn list_desktop_windows(limit: usize) -> anyhow::Result<Vec<DesktopWindowEntry>> {
    let script = r#"
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class DawnWindowInterop {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}
"@
$windows = New-Object System.Collections.Generic.List[object]
[DawnWindowInterop]::EnumWindows({
    param($handle, $lParam)
    if (-not [DawnWindowInterop]::IsWindowVisible($handle)) { return $true }
    $length = [DawnWindowInterop]::GetWindowTextLength($handle)
    if ($length -le 0) { return $true }
    $buffer = New-Object System.Text.StringBuilder ($length + 1)
    [void][DawnWindowInterop]::GetWindowText($handle, $buffer, $buffer.Capacity)
    $title = $buffer.ToString().Trim()
    if ([string]::IsNullOrWhiteSpace($title)) { return $true }
    $processId = 0
    [void][DawnWindowInterop]::GetWindowThreadProcessId($handle, [ref]$processId)
    $processName = $null
    try {
        $process = Get-Process -Id $processId -ErrorAction Stop
        $processName = $process.ProcessName
    } catch {
        $processName = $null
    }
    $windows.Add([pscustomobject]@{
        handle = ("0x{0:X}" -f $handle.ToInt64())
        title = $title
        processId = [int]$processId
        processName = $processName
    })
    return $true
}, [IntPtr]::Zero) | Out-Null
$limit = [int]$env:DAWN_WINDOW_LIMIT
@($windows | Select-Object -First $limit) | ConvertTo-Json -Compress
"#;
    let stdout =
        run_windows_powershell_capture(script, &[("DAWN_WINDOW_LIMIT", limit.max(1).to_string())])
            .await?;
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&stdout).context("failed to parse desktop window list JSON")
}

async fn focus_desktop_window(
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
) -> anyhow::Result<DesktopWindowEntry> {
    let script = r#"
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class DawnWindowInterop {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
}
"@
$titleNeedle = $env:DAWN_WINDOW_TITLE
$handleNeedle = $env:DAWN_WINDOW_HANDLE
$processNameNeedle = $env:DAWN_WINDOW_PROCESS_NAME
$match = $null
[DawnWindowInterop]::EnumWindows({
    param($handle, $lParam)
    if ($match -ne $null) { return $false }
    if (-not [DawnWindowInterop]::IsWindowVisible($handle)) { return $true }
    $length = [DawnWindowInterop]::GetWindowTextLength($handle)
    if ($length -le 0) { return $true }
    $buffer = New-Object System.Text.StringBuilder ($length + 1)
    [void][DawnWindowInterop]::GetWindowText($handle, $buffer, $buffer.Capacity)
    $title = $buffer.ToString().Trim()
    if ([string]::IsNullOrWhiteSpace($title)) { return $true }
    $formattedHandle = ("0x{0:X}" -f $handle.ToInt64())
    $processId = 0
    [void][DawnWindowInterop]::GetWindowThreadProcessId($handle, [ref]$processId)
    $processName = $null
    try {
        $process = Get-Process -Id $processId -ErrorAction Stop
        $processName = $process.ProcessName
    } catch {
        $processName = $null
    }
    $handleMatches = -not [string]::IsNullOrWhiteSpace($handleNeedle) -and $formattedHandle.Equals($handleNeedle, [System.StringComparison]::OrdinalIgnoreCase)
    $titleMatches = -not [string]::IsNullOrWhiteSpace($titleNeedle) -and $title.IndexOf($titleNeedle, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
    $processMatches = -not [string]::IsNullOrWhiteSpace($processNameNeedle) -and -not [string]::IsNullOrWhiteSpace($processName) -and $processName.Equals($processNameNeedle, [System.StringComparison]::OrdinalIgnoreCase)
    if (-not $handleMatches -and -not $titleMatches -and -not $processMatches) { return $true }
    $match = [pscustomobject]@{
        nativeHandle = $handle
        handle = $formattedHandle
        title = $title
        processId = [int]$processId
        processName = $processName
    }
    return $false
}, [IntPtr]::Zero) | Out-Null
if ($match -eq $null) {
    throw "no visible desktop window matched the requested title, handle, or process name"
}
[void][DawnWindowInterop]::ShowWindowAsync($match.nativeHandle, 9)
Start-Sleep -Milliseconds 120
if (-not [DawnWindowInterop]::SetForegroundWindow($match.nativeHandle)) {
    throw "failed to focus the matched desktop window"
}
[pscustomobject]@{
    handle = $match.handle
    title = $match.title
    processId = $match.processId
    processName = $match.processName
} | ConvertTo-Json -Compress
"#;
    let stdout = run_windows_powershell_capture(
        script,
        &[
            (
                "DAWN_WINDOW_TITLE",
                title.unwrap_or_default().trim().to_string(),
            ),
            (
                "DAWN_WINDOW_HANDLE",
                handle.unwrap_or_default().trim().to_string(),
            ),
            (
                "DAWN_WINDOW_PROCESS_NAME",
                process_name.unwrap_or_default().trim().to_string(),
            ),
        ],
    )
    .await?;
    serde_json::from_str(&stdout).context("failed to parse focused window JSON")
}

async fn wait_for_desktop_window(
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
    timeout_ms: u64,
    poll_ms: u64,
) -> anyhow::Result<DesktopWindowEntry> {
    let started = SystemTime::now();
    let normalized_process_name = process_name.map(normalize_desktop_process_name);
    loop {
        let windows = list_desktop_windows(64).await?;
        if let Some(window) = windows.into_iter().find(|window| {
            matches_desktop_window(window, title, handle, normalized_process_name.as_deref())
        }) {
            let focus_title = if title.is_some() {
                Some(window.title.clone())
            } else {
                None
            };
            let focus_handle = if handle.is_some() {
                Some(window.handle.clone())
            } else {
                None
            };
            return focus_desktop_window(
                focus_title.as_deref(),
                focus_handle.as_deref(),
                normalized_process_name.as_deref(),
            )
            .await
            .or(Ok(window));
        }
        if started.elapsed().unwrap_or_default() >= Duration::from_millis(timeout_ms.max(1)) {
            anyhow::bail!(
                "timed out after {} ms waiting for a desktop window matching the requested selector",
                timeout_ms
            );
        }
        tokio::time::sleep(Duration::from_millis(poll_ms.max(50))).await;
    }
}

async fn accessibility_snapshot_for_window(
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
    depth: usize,
    children_limit: usize,
) -> anyhow::Result<(DesktopWindowEntry, Value)> {
    let window = if let Some(handle) = handle {
        focus_desktop_window(title, Some(handle), process_name).await?
    } else {
        wait_for_desktop_window(title, handle, process_name, 5_000, 200).await?
    };
    let script = r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$handleText = $env:DAWN_ACCESSIBILITY_HANDLE
if ([string]::IsNullOrWhiteSpace($handleText)) {
    throw "desktop accessibility snapshot requires a window handle"
}
$handleNumber = [Convert]::ToInt64($handleText.Replace("0x", ""), 16)
$nativeHandle = [IntPtr]::new($handleNumber)
$root = [System.Windows.Automation.AutomationElement]::FromHandle($nativeHandle)
if ($null -eq $root) {
    throw "failed to resolve automation root for the requested desktop window"
}
$maxDepth = [int]$env:DAWN_ACCESSIBILITY_DEPTH
$childrenLimit = [int]$env:DAWN_ACCESSIBILITY_CHILDREN_LIMIT
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
function Convert-DawnAutomationElement($element, [int]$depth) {
    if ($null -eq $element) { return $null }
    $current = $element.Current
    $bounds = $current.BoundingRectangle
    $node = [ordered]@{
        name = $current.Name
        automationId = $current.AutomationId
        className = $current.ClassName
        controlType = $current.ControlType.ProgrammaticName
        nativeWindowHandle = $current.NativeWindowHandle
        isEnabled = $current.IsEnabled
        isOffscreen = $current.IsOffscreen
        boundingRect = @{
            x = [int][Math]::Round($bounds.Left)
            y = [int][Math]::Round($bounds.Top)
            width = [int][Math]::Round($bounds.Width)
            height = [int][Math]::Round($bounds.Height)
        }
    }
    if ($depth -ge $maxDepth) {
        return [pscustomobject]$node
    }
    $children = New-Object System.Collections.Generic.List[object]
    $child = $walker.GetFirstChild($element)
    $count = 0
    while ($child -ne $null -and $count -lt $childrenLimit) {
        $converted = Convert-DawnAutomationElement $child ($depth + 1)
        if ($converted -ne $null) {
            $children.Add($converted)
        }
        $count += 1
        $child = $walker.GetNextSibling($child)
    }
    $node.children = $children
    return [pscustomobject]$node
}
Convert-DawnAutomationElement $root 0 | ConvertTo-Json -Depth 12 -Compress
"#;
    let stdout = run_windows_powershell_capture(
        script,
        &[
            ("DAWN_ACCESSIBILITY_HANDLE", window.handle.clone()),
            ("DAWN_ACCESSIBILITY_DEPTH", depth.max(1).to_string()),
            (
                "DAWN_ACCESSIBILITY_CHILDREN_LIMIT",
                children_limit.max(1).to_string(),
            ),
        ],
    )
    .await?;
    let snapshot =
        serde_json::from_str(&stdout).context("failed to parse desktop accessibility JSON")?;
    Ok((window, snapshot))
}

fn matches_desktop_window(
    window: &DesktopWindowEntry,
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
) -> bool {
    let handle_matches = handle
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|needle| window.handle.eq_ignore_ascii_case(needle));
    let title_matches = title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|needle| {
            window
                .title
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        });
    let process_matches = process_name
        .map(normalize_desktop_process_name)
        .filter(|value| !value.is_empty())
        .zip(
            window
                .process_name
                .as_deref()
                .map(normalize_desktop_process_name),
        )
        .is_some_and(|(needle, current)| current == needle);
    handle_matches || title_matches || process_matches
}

fn infer_process_name_from_target(target: &str) -> Option<String> {
    if target.contains("://") {
        return None;
    }
    let candidate = PathBuf::from(target)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .or_else(|| {
            let trimmed = target.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })?;
    let normalized = normalize_desktop_process_name(&candidate);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_desktop_process_name(raw: &str) -> String {
    raw.trim()
        .trim_end_matches(".exe")
        .trim_end_matches(".EXE")
        .to_ascii_lowercase()
}

fn desktop_window_selector_from_payload(payload: &Value) -> DesktopWindowSelector {
    DesktopWindowSelector {
        title: payload
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        handle: payload
            .get("handle")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        process_name: payload
            .get("processName")
            .or_else(|| payload.get("app"))
            .and_then(Value::as_str)
            .map(normalize_desktop_process_name)
            .filter(|value| !value.is_empty()),
    }
}

async fn move_desktop_mouse(x: i32, y: i32) -> anyhow::Result<&'static str> {
    let script = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DawnMouseInterop {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int x, int y);
}
"@
if (-not [DawnMouseInterop]::SetCursorPos([int]$env:DAWN_MOUSE_X, [int]$env:DAWN_MOUSE_Y)) {
    throw "failed to move desktop mouse"
}
"#;
    run_windows_powershell_capture(
        script,
        &[
            ("DAWN_MOUSE_X", x.to_string()),
            ("DAWN_MOUSE_Y", y.to_string()),
        ],
    )
    .await?;
    Ok("powershell:user32.SetCursorPos")
}

async fn click_desktop_mouse(
    button: &str,
    double_click: bool,
    point: Option<(i32, i32)>,
) -> anyhow::Result<&'static str> {
    let script = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DawnMouseInterop {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
}
"@
function Invoke-DawnMouseClick([uint32]$down, [uint32]$up) {
    [DawnMouseInterop]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 50
    [DawnMouseInterop]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}
if (-not [string]::IsNullOrWhiteSpace($env:DAWN_MOUSE_X) -and -not [string]::IsNullOrWhiteSpace($env:DAWN_MOUSE_Y)) {
    if (-not [DawnMouseInterop]::SetCursorPos([int]$env:DAWN_MOUSE_X, [int]$env:DAWN_MOUSE_Y)) {
        throw "failed to move desktop mouse before click"
    }
}
$button = $env:DAWN_MOUSE_BUTTON
$double = $env:DAWN_MOUSE_DOUBLE -eq "true"
switch ($button) {
    "left" {
        Invoke-DawnMouseClick 0x0002 0x0004
        if ($double) { Start-Sleep -Milliseconds 80; Invoke-DawnMouseClick 0x0002 0x0004 }
    }
    "right" {
        Invoke-DawnMouseClick 0x0008 0x0010
        if ($double) { Start-Sleep -Milliseconds 80; Invoke-DawnMouseClick 0x0008 0x0010 }
    }
    "middle" {
        Invoke-DawnMouseClick 0x0020 0x0040
        if ($double) { Start-Sleep -Milliseconds 80; Invoke-DawnMouseClick 0x0020 0x0040 }
    }
    default {
        throw "unsupported desktop mouse button"
    }
}
"#;
    let point_x = point.map(|value| value.0.to_string()).unwrap_or_default();
    let point_y = point.map(|value| value.1.to_string()).unwrap_or_default();
    run_windows_powershell_capture(
        script,
        &[
            ("DAWN_MOUSE_BUTTON", button.to_string()),
            ("DAWN_MOUSE_DOUBLE", double_click.to_string()),
            ("DAWN_MOUSE_X", point_x),
            ("DAWN_MOUSE_Y", point_y),
        ],
    )
    .await?;
    Ok("powershell:user32.mouse_event")
}

async fn capture_desktop_screenshot(
    screenshot_path: &PathBuf,
    region: Option<(i32, i32, i32, i32)>,
) -> anyhow::Result<DesktopScreenshotResult> {
    if let Some(parent) = screenshot_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create screenshot directory {}", parent.display())
        })?;
    }

    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$path = $env:DAWN_SCREENSHOT_PATH
if ([string]::IsNullOrWhiteSpace($env:DAWN_SCREENSHOT_WIDTH) -or [string]::IsNullOrWhiteSpace($env:DAWN_SCREENSHOT_HEIGHT)) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
} else {
    $bounds = New-Object System.Drawing.Rectangle(
        [int]$env:DAWN_SCREENSHOT_X,
        [int]$env:DAWN_SCREENSHOT_Y,
        [int]$env:DAWN_SCREENSHOT_WIDTH,
        [int]$env:DAWN_SCREENSHOT_HEIGHT
    )
}
$bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
[pscustomobject]@{
    path = $path
    x = $bounds.X
    y = $bounds.Y
    width = $bounds.Width
    height = $bounds.Height
} | ConvertTo-Json -Compress
"#;

    let (x, y, width, height) = region.unwrap_or((0, 0, 0, 0));
    let stdout = run_windows_powershell_capture(
        script,
        &[
            (
                "DAWN_SCREENSHOT_PATH",
                screenshot_path.as_os_str().to_string_lossy().to_string(),
            ),
            ("DAWN_SCREENSHOT_X", x.to_string()),
            ("DAWN_SCREENSHOT_Y", y.to_string()),
            (
                "DAWN_SCREENSHOT_WIDTH",
                if region.is_some() {
                    width.to_string()
                } else {
                    String::new()
                },
            ),
            (
                "DAWN_SCREENSHOT_HEIGHT",
                if region.is_some() {
                    height.to_string()
                } else {
                    String::new()
                },
            ),
        ],
    )
    .await?;
    serde_json::from_str(&stdout).context("failed to parse desktop screenshot JSON")
}

async fn run_windows_powershell_capture(
    script: &str,
    envs: &[(&str, String)],
) -> anyhow::Result<String> {
    if !cfg!(target_os = "windows") {
        anyhow::bail!("this desktop automation command is currently implemented for Windows only");
    }
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-Command").arg(script);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .await
        .context("failed to launch powershell for desktop automation")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("status {:?}", output.status.code())
        };
        anyhow::bail!("desktop automation powershell command failed: {detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn normalize_desktop_mouse_button(raw: &str) -> anyhow::Result<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "left" | "primary" => Ok("left"),
        "right" | "secondary" => Ok("right"),
        "middle" | "wheel" => Ok("middle"),
        other => anyhow::bail!("unsupported desktop mouse button `{other}`"),
    }
}

fn resolve_desktop_point(payload: &Value) -> anyhow::Result<(i32, i32)> {
    let x = parse_i32_payload_field(payload, "x")?;
    let y = parse_i32_payload_field(payload, "y")?;
    Ok((x, y))
}

fn resolve_optional_desktop_point(payload: &Value) -> anyhow::Result<Option<(i32, i32)>> {
    let x = parse_optional_i32_payload_field(payload, "x")?;
    let y = parse_optional_i32_payload_field(payload, "y")?;
    match (x, y) {
        (Some(x), Some(y)) => Ok(Some((x, y))),
        (None, None) => Ok(None),
        _ => anyhow::bail!("desktop point requires both payload.x and payload.y"),
    }
}

fn resolve_desktop_capture_region(payload: &Value) -> anyhow::Result<Option<(i32, i32, i32, i32)>> {
    let x = parse_optional_i32_payload_field(payload, "x")?;
    let y = parse_optional_i32_payload_field(payload, "y")?;
    let width = parse_optional_i32_payload_field(payload, "width")?;
    let height = parse_optional_i32_payload_field(payload, "height")?;
    match (x, y, width, height) {
        (None, None, None, None) => Ok(None),
        (Some(x), Some(y), Some(width), Some(height)) if width > 0 && height > 0 => {
            Ok(Some((x, y, width, height)))
        }
        (Some(_), Some(_), Some(_), Some(_)) => {
            anyhow::bail!("desktop screenshot width and height must be positive")
        }
        _ => anyhow::bail!(
            "desktop screenshot region requires payload.x, payload.y, payload.width, and payload.height"
        ),
    }
}

fn resolve_desktop_screenshot_path(requested: Option<&str>) -> PathBuf {
    let mut path = requested
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!(
                "dawn-desktop-screenshot-{}.png",
                unix_timestamp_ms()
            ))
        });
    if path.extension().is_none() {
        path.set_extension("png");
    }
    path
}

fn parse_i32_payload_field(payload: &Value, field: &str) -> anyhow::Result<i32> {
    let raw = payload
        .get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("desktop command requires payload.{field}"))?;
    i32::try_from(raw)
        .map_err(|_| anyhow::anyhow!("payload.{field} must fit within a signed 32-bit integer"))
}

fn parse_optional_i32_payload_field(payload: &Value, field: &str) -> anyhow::Result<Option<i32>> {
    let Some(raw) = payload.get(field).and_then(Value::as_i64) else {
        return Ok(None);
    };
    Ok(Some(i32::try_from(raw).map_err(|_| {
        anyhow::anyhow!("payload.{field} must fit within a signed 32-bit integer")
    })?))
}

fn normalize_browser_url(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("browser URL cannot be empty");
    }
    if trimmed.chars().any(char::is_whitespace) {
        anyhow::bail!("browser_open expects a URL. Use browser_search for free-form search terms");
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else if looks_like_local_browser_target(trimmed) {
        format!("http://{trimmed}")
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&candidate).context("failed to parse browser URL")?;
    match url.scheme() {
        "http" | "https" => Ok(url.to_string()),
        other => anyhow::bail!("browser_open only supports http/https URLs, got scheme `{other}`"),
    }
}

fn looks_like_local_browser_target(value: &str) -> bool {
    value.starts_with("localhost")
        || value.starts_with("127.")
        || value.starts_with("10.")
        || value.starts_with("192.168.")
        || value.starts_with("[::1]")
}

fn build_browser_search_url(query: &str, engine: &str) -> anyhow::Result<String> {
    let base = match engine {
        "google" => "https://www.google.com/search",
        "bing" => "https://www.bing.com/search",
        "duckduckgo" | "ddg" => "https://duckduckgo.com/",
        "baidu" => "https://www.baidu.com/s",
        other => anyhow::bail!(
            "unsupported browser search engine `{other}`. Supported: google, bing, duckduckgo, baidu"
        ),
    };
    let key = if engine == "baidu" { "wd" } else { "q" };
    let url =
        Url::parse_with_params(base, &[(key, query)]).context("failed to build search URL")?;
    Ok(url.to_string())
}

async fn launch_browser_url(url: &str) -> anyhow::Result<&'static str> {
    let (launcher, mut command) = if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(url);
        ("explorer", command)
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        ("open", command)
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        ("xdg-open", command)
    };
    let status = command.status().await?;
    if !status.success() {
        anyhow::bail!(
            "browser launcher `{launcher}` exited with status {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
    }
    Ok(launcher)
}

fn load_config() -> NodeConfig {
    let profile = profile::load_profile_or_default();
    let node_id = env::var("DAWN_NODE_ID")
        .ok()
        .or_else(|| profile.node_id.clone())
        .unwrap_or_else(|| "node-local".to_string());
    let node_name = env::var("DAWN_NODE_NAME")
        .ok()
        .or_else(|| profile.node_name.clone())
        .unwrap_or_else(|| "Dawn Local Node".to_string());
    let gateway_ws_url = env::var("DAWN_GATEWAY_WS_URL").unwrap_or_else(|_| {
        let ws_base = profile
            .gateway_base_url
            .as_deref()
            .map(profile::http_base_to_ws_base)
            .unwrap_or_else(|| profile::http_base_to_ws_base(&profile::default_gateway_base_url()));
        format!("{ws_base}/api/gateway/control-plane/nodes/{node_id}/session")
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
    let claim_token = env::var("DAWN_NODE_CLAIM_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| profile.claim_token.clone());
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
        claim_token,
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
        "browser_navigate".to_string(),
        "browser_extract".to_string(),
        "browser_click".to_string(),
        "browser_back".to_string(),
        "browser_focus".to_string(),
        "browser_close".to_string(),
        "browser_tabs".to_string(),
        "browser_snapshot".to_string(),
        "browser_type".to_string(),
        "browser_upload".to_string(),
        "browser_download".to_string(),
        "browser_form_fill".to_string(),
        "browser_form_submit".to_string(),
        "browser_open".to_string(),
        "browser_search".to_string(),
        "desktop_open".to_string(),
        "desktop_clipboard_set".to_string(),
        "desktop_type_text".to_string(),
        "desktop_key_press".to_string(),
        "desktop_windows_list".to_string(),
        "desktop_window_focus".to_string(),
        "desktop_wait_for_window".to_string(),
        "desktop_focus_app".to_string(),
        "desktop_launch_and_focus".to_string(),
        "desktop_mouse_move".to_string(),
        "desktop_mouse_click".to_string(),
        "desktop_screenshot".to_string(),
        "desktop_accessibility_snapshot".to_string(),
        "system_info".to_string(),
        "list_directory".to_string(),
        "read_file_preview".to_string(),
        "stat_path".to_string(),
        "process_snapshot".to_string(),
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

fn verify_policy_distribution(
    config: &NodeConfig,
    bundle: &GatewayRolloutBundle,
) -> anyhow::Result<()> {
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
            let public_key_hex = config.policy_trust_roots.get(&issuer_did).ok_or_else(|| {
                anyhow::anyhow!("policy issuer '{}' is not trusted by this node", issuer_did)
            })?;
            validate_self_certifying_did(&issuer_did, public_key_hex, "did:dawn:policy:")?;
            verify_signature(
                public_key_hex,
                &serde_json::to_vec(&envelope.document)?,
                &envelope.signature_hex,
            )
            .context("policy signature verification failed on node")?;

            if profile.issuer_did.as_deref().map(str::to_ascii_lowercase)
                != Some(issuer_did.clone())
            {
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

fn verify_skill_distribution(
    config: &NodeConfig,
    bundle: &GatewayRolloutBundle,
) -> anyhow::Result<()> {
    for skill in &bundle.skills.skills {
        match skill.source_kind.as_str() {
            "signed_publisher" => {
                let issuer_did = skill
                    .issuer_did
                    .clone()
                    .ok_or_else(|| {
                        anyhow::anyhow!("signed skill '{}' is missing issuerDid", skill.skill_id)
                    })?
                    .to_ascii_lowercase();
                let signature_hex = skill.signature_hex.clone().ok_or_else(|| {
                    anyhow::anyhow!("signed skill '{}' is missing signatureHex", skill.skill_id)
                })?;
                let issued_at_unix_ms = skill.issued_at_unix_ms.ok_or_else(|| {
                    anyhow::anyhow!(
                        "signed skill '{}' is missing issuedAtUnixMs",
                        skill.skill_id
                    )
                })?;
                let public_key_hex = config
                    .skill_publisher_trust_roots
                    .get(&issuer_did)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "skill publisher '{}' is not trusted by this node",
                            issuer_did
                        )
                    })?;
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
                verify_signature(
                    public_key_hex,
                    &serde_json::to_vec(&document)?,
                    &signature_hex,
                )
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

fn verify_signature(
    public_key_hex: &str,
    payload: &[u8],
    signature_hex: &str,
) -> anyhow::Result<()> {
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
    let bytes =
        hex::decode(normalize_hex(raw)?).with_context(|| format!("{label} must be valid hex"))?;
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

fn payload_usize(payload: &Value, key: &str, default: usize, max: usize) -> usize {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .map(|value| value.clamp(1, max))
        .unwrap_or(default)
}

fn system_time_to_unix_ms(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn parse_windows_tasklist_snapshot(raw: &str, limit: usize) -> Vec<Value> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_windows_tasklist_record)
        .take(limit)
        .collect()
}

fn parse_windows_tasklist_record(line: &str) -> Option<Value> {
    let fields = parse_csv_record(line);
    if fields.len() < 5 {
        return None;
    }
    Some(json!({
        "imageName": fields[0],
        "pid": fields[1],
        "sessionName": fields[2],
        "sessionNumber": fields[3],
        "memory": fields[4]
    }))
}

fn parse_csv_record(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                values.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    values.push(current.trim().to_string());
    values
}

fn parse_unix_process_snapshot(raw: &str, limit: usize) -> Vec<Value> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.split_whitespace();
            let pid = parts.next()?;
            let command = parts.collect::<Vec<_>>().join(" ");
            Some(json!({
                "pid": pid,
                "command": command
            }))
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn base_config() -> NodeConfig {
        NodeConfig {
            gateway_ws_url: "ws://127.0.0.1:8000/api/gateway/control-plane/nodes/node-test/session"
                .to_string(),
            node_id: "node-test".to_string(),
            node_name: "Node Test".to_string(),
            capabilities: vec!["agent_ping".to_string()],
            claim_token: None,
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
        let policy_signature =
            policy_signing_key.sign(&serde_json::to_vec(&policy_document).unwrap());
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

        let error = verify_rollout_bundle(&config, &bundle)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsigned skill"));
    }

    #[tokio::test]
    async fn system_info_command_reports_node_metadata() {
        let config = base_config();
        let response = execute_system_info_command(
            &config,
            GatewayCommandEnvelope {
                command_id: "cmd-system-info".to_string(),
                command_type: "system_info".to_string(),
                payload: json!({}),
            },
        )
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["nodeId"], "node-test");
        assert_eq!(result["nodeName"], "Node Test");
        assert_eq!(result["allowShell"], false);
    }

    #[tokio::test]
    async fn read_file_preview_command_truncates_large_files() {
        let temp_path = std::env::temp_dir().join(format!(
            "dawn-node-read-preview-{}.txt",
            unix_timestamp_ms()
        ));
        fs::write(&temp_path, "abcdefghijklmnopqrstuvwxyz").unwrap();

        let response = execute_read_file_preview_command(GatewayCommandEnvelope {
            command_id: "cmd-read-preview".to_string(),
            command_type: "read_file_preview".to_string(),
            payload: json!({
                "path": temp_path.display().to_string(),
                "maxBytes": 8
            }),
        })
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["preview"], "abcdefgh");
        assert_eq!(result["truncated"], true);

        fs::remove_file(temp_path).ok();
    }

    #[tokio::test]
    async fn list_directory_command_returns_entries() {
        let temp_dir =
            std::env::temp_dir().join(format!("dawn-node-list-dir-{}", unix_timestamp_ms()));
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(temp_dir.join("alpha.txt"), "alpha").unwrap();

        let response = execute_list_directory_command(GatewayCommandEnvelope {
            command_id: "cmd-list-dir".to_string(),
            command_type: "list_directory".to_string(),
            payload: json!({
                "path": temp_dir.display().to_string()
            }),
        })
        .await;

        assert_eq!(response.status, "succeeded");
        let entries = response.result.unwrap()["entries"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            entries
                .iter()
                .any(|entry| entry["name"] == "alpha.txt" && entry["isFile"] == true)
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn extracts_browser_matches_from_html() {
        let matches = extract_browser_matches(
            r#"<html><body><main><a href="/docs">Read docs</a><a href="https://example.com/blog">Blog</a></main></body></html>"#,
            "https://example.com/start",
            "a",
            5,
            80,
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "Read docs");
        assert_eq!(matches[0].href.as_deref(), Some("https://example.com/docs"));
        assert_eq!(matches[1].href.as_deref(), Some("https://example.com/blog"));
    }

    #[test]
    fn resolves_browser_click_target_from_relative_link() {
        let target = resolve_browser_click_target(
            r#"<html><body><a class="next" href="/next">Next step</a></body></html>"#,
            "https://example.com/start",
            "a.next",
            0,
        )
        .unwrap();
        assert_eq!(target.text, "Next step");
        assert_eq!(target.href, "https://example.com/next");
    }

    #[test]
    fn resolves_browser_form_target_with_defaults() {
        let form = resolve_browser_form_target(
            r#"<html><body><form action="/search" method="get"><input name="q" value="dawn" /><input type="hidden" name="src" value="docs" /><textarea name="note">hello world</textarea></form></body></html>"#,
            "https://example.com/start",
            "form",
        )
        .unwrap();
        assert_eq!(form.method, "GET");
        assert_eq!(form.action_url, "https://example.com/search");
        assert_eq!(
            form.default_fields.get("q").map(String::as_str),
            Some("dawn")
        );
        assert_eq!(
            form.default_fields.get("src").map(String::as_str),
            Some("docs")
        );
        assert_eq!(
            form.default_fields.get("note").map(String::as_str),
            Some("hello world")
        );
        assert_eq!(form.form_index, 0);
    }

    #[test]
    fn resolves_browser_form_target_by_index() {
        let form = resolve_browser_form_target(
            r#"<html><body><form id="primary" action="/one"><input name="q" value="dawn" /></form><div><form action="/two" method="post"><input name="code" value="1234" /></form></div></body></html>"#,
            "https://example.com/start",
            "@form-index:1",
        )
        .unwrap();
        assert_eq!(form.form_index, 1);
        assert_eq!(form.method, "POST");
        assert_eq!(form.action_url, "https://example.com/two");
        assert_eq!(
            form.default_fields.get("code").map(String::as_str),
            Some("1234")
        );
    }

    #[test]
    fn resolves_browser_type_target_to_form_and_field() {
        let target = resolve_browser_type_target(
            r#"<html><body><form action="/search"><input name="q" value="dawn" /><textarea name="note">hello</textarea></form></body></html>"#,
            "https://example.com/start",
            "textarea[name=note]",
        )
        .unwrap();
        assert_eq!(target.form.form_index, 0);
        assert_eq!(target.form.selector, "@form-index:0");
        assert_eq!(target.field_name, "note");
        assert_eq!(target.field_tag, "textarea");
        assert_eq!(target.current_value, "hello");
    }

    #[test]
    fn resolves_browser_upload_target_to_file_input() {
        let target = resolve_browser_upload_target(
            r#"<html><body><form action="/upload" method="post"><input type="file" name="attachment" /><input name="note" value="hi" /></form></body></html>"#,
            "https://example.com/start",
            "input[type=file]",
        )
        .unwrap();
        assert_eq!(target.form.form_index, 0);
        assert_eq!(target.field_name, "attachment");
        assert_eq!(target.form.action_url, "https://example.com/upload");
    }

    #[test]
    fn builds_browser_snapshot_with_forms_links_and_buttons() {
        let session = BrowserSession {
            client: new_browser_client().unwrap(),
            current_url: "https://example.com/start".to_string(),
            page_html: r#"<html><head><title>Dawn</title></head><body><h1>Dawn Browser</h1><a href="/next">Next</a><form action="/search"><input name="q" value="dawn" /></form><button type="submit">Go</button></body></html>"#.to_string(),
            title: Some("Dawn".to_string()),
            status_code: 200,
            content_type: Some("text/html".to_string()),
            last_action: "navigate".to_string(),
            loaded_at_unix_ms: 42,
            history: Vec::new(),
            pending_form_selector: Some("@form-index:0".to_string()),
            pending_form_fields: BTreeMap::from([(String::from("q"), String::from("openclaw"))]),
            pending_form_uploads: BTreeMap::from([(
                String::from("attachment"),
                String::from("C:/tmp/demo.txt"),
            )]),
        };
        let snapshot = build_browser_snapshot(
            "browser-default",
            &session,
            DEFAULT_BROWSER_SNAPSHOT_LIMIT,
            120,
        )
        .unwrap();
        assert_eq!(snapshot.session_id, "browser-default");
        assert_eq!(
            snapshot.headings.first().map(|entry| entry.text.as_str()),
            Some("Dawn Browser")
        );
        assert_eq!(
            snapshot
                .links
                .first()
                .and_then(|entry| entry.href.as_deref()),
            Some("https://example.com/next")
        );
        assert_eq!(snapshot.forms.first().map(|form| form.form_index), Some(0));
        assert_eq!(
            snapshot.buttons.first().map(|entry| entry.tag.as_str()),
            Some("button")
        );
    }

    #[test]
    fn chooses_next_active_browser_session_after_close() {
        let mut browser_sessions = HashMap::new();
        browser_sessions.insert(
            "browser-default".to_string(),
            BrowserSession {
                client: new_browser_client().unwrap(),
                current_url: "https://example.com/default".to_string(),
                page_html: "<html></html>".to_string(),
                title: Some("Default".to_string()),
                status_code: 200,
                content_type: Some("text/html".to_string()),
                last_action: "navigate".to_string(),
                loaded_at_unix_ms: 10,
                history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
            },
        );
        browser_sessions.insert(
            "browser-secondary".to_string(),
            BrowserSession {
                client: new_browser_client().unwrap(),
                current_url: "https://example.com/secondary".to_string(),
                page_html: "<html></html>".to_string(),
                title: Some("Secondary".to_string()),
                status_code: 200,
                content_type: Some("text/html".to_string()),
                last_action: "navigate".to_string(),
                loaded_at_unix_ms: 20,
                history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
            },
        );
        let next = next_active_browser_session_id(
            &browser_sessions,
            Some("browser-secondary"),
            "browser-secondary",
        );
        assert_eq!(next.as_deref(), Some("browser-default"));
    }

    #[test]
    fn derives_browser_download_path_from_url() {
        let path = resolve_browser_download_path(None, "https://example.com/files/report.pdf");
        assert_eq!(
            path.file_name()
                .map(|value| value.to_string_lossy().to_string()),
            Some("report.pdf".to_string())
        );
    }

    #[test]
    fn payload_string_map_accepts_scalar_values() {
        let payload = json!({
            "fields": {
                "q": "dawn",
                "page": 2,
                "exact": true
            }
        });
        let fields = payload_string_map(&payload, "fields").unwrap();
        assert_eq!(fields.get("q").map(String::as_str), Some("dawn"));
        assert_eq!(fields.get("page").map(String::as_str), Some("2"));
        assert_eq!(fields.get("exact").map(String::as_str), Some("true"));
    }

    #[test]
    fn normalizes_browser_open_targets() {
        assert_eq!(
            normalize_browser_url("example.com/docs").unwrap(),
            "https://example.com/docs"
        );
        assert_eq!(
            normalize_browser_url("127.0.0.1:8000/console").unwrap(),
            "http://127.0.0.1:8000/console"
        );
    }

    #[test]
    fn rejects_non_http_browser_target() {
        let error = normalize_browser_url("file:///tmp/demo.txt")
            .unwrap_err()
            .to_string();
        assert!(error.contains("only supports http/https"));
    }

    #[test]
    fn builds_browser_search_urls() {
        let google = build_browser_search_url("openclaw parity", "google").unwrap();
        let duck = build_browser_search_url("desktop cli", "duckduckgo").unwrap();
        assert!(google.starts_with("https://www.google.com/search?"));
        assert!(google.contains("q=openclaw+parity"));
        assert!(duck.starts_with("https://duckduckgo.com/?"));
        assert!(duck.contains("q=desktop+cli"));
    }

    #[test]
    fn default_capabilities_include_browser_session_commands() {
        let capabilities = default_capabilities();
        assert!(capabilities.iter().any(|value| value == "browser_navigate"));
        assert!(capabilities.iter().any(|value| value == "browser_extract"));
        assert!(capabilities.iter().any(|value| value == "browser_click"));
        assert!(capabilities.iter().any(|value| value == "browser_back"));
        assert!(capabilities.iter().any(|value| value == "browser_focus"));
        assert!(capabilities.iter().any(|value| value == "browser_close"));
        assert!(capabilities.iter().any(|value| value == "browser_tabs"));
        assert!(capabilities.iter().any(|value| value == "browser_snapshot"));
        assert!(capabilities.iter().any(|value| value == "browser_type"));
        assert!(capabilities.iter().any(|value| value == "browser_upload"));
        assert!(capabilities.iter().any(|value| value == "browser_download"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_form_fill")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_form_submit")
        );
        assert!(capabilities.iter().any(|value| value == "desktop_open"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_clipboard_set")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_type_text")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_key_press")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_windows_list")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_window_focus")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_wait_for_window")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_focus_app")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_launch_and_focus")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_mouse_move")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_mouse_click")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_screenshot")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_snapshot")
        );
    }

    #[test]
    fn encodes_windows_send_keys_text_reserved_characters() {
        let encoded = encode_windows_send_keys_text("+^%~()[]{}\n\t");
        assert_eq!(encoded, "{+}{^}{%}{~}{(}{)}{[}{]}{{}{}}{ENTER}{TAB}");
    }

    #[test]
    fn builds_windows_send_keys_modifier_combo() {
        let combo = build_windows_send_keys_combo("ctrl+shift+tab").unwrap();
        assert_eq!(combo, "^+({TAB})");
    }

    #[test]
    fn normalizes_desktop_mouse_button_aliases() {
        assert_eq!(normalize_desktop_mouse_button("left").unwrap(), "left");
        assert_eq!(normalize_desktop_mouse_button("primary").unwrap(), "left");
        assert_eq!(
            normalize_desktop_mouse_button("secondary").unwrap(),
            "right"
        );
        assert_eq!(normalize_desktop_mouse_button("wheel").unwrap(), "middle");
    }

    #[test]
    fn resolves_default_desktop_screenshot_path() {
        let path = resolve_desktop_screenshot_path(None);
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("png")
        );
        assert!(path.to_string_lossy().contains("dawn-desktop-screenshot-"));
    }

    #[test]
    fn infers_desktop_process_name_from_target() {
        assert_eq!(
            infer_process_name_from_target("C:/Windows/System32/notepad.exe").as_deref(),
            Some("notepad")
        );
        assert_eq!(
            infer_process_name_from_target("calc").as_deref(),
            Some("calc")
        );
        assert_eq!(infer_process_name_from_target("https://example.com"), None);
    }

    #[test]
    fn matches_desktop_window_by_process_name() {
        let window = DesktopWindowEntry {
            handle: "0x100".to_string(),
            title: "Untitled - Notepad".to_string(),
            process_id: 42,
            process_name: Some("notepad".to_string()),
        };
        assert!(matches_desktop_window(
            &window,
            None,
            None,
            Some("notepad.exe")
        ));
        assert!(!matches_desktop_window(&window, None, None, Some("calc")));
    }

    #[test]
    fn parses_windows_tasklist_csv_records() {
        let parsed =
            parse_windows_tasklist_record("\"cmd.exe\",\"1234\",\"Console\",\"1\",\"8,192 K\"")
                .unwrap();
        assert_eq!(parsed["imageName"], "cmd.exe");
        assert_eq!(parsed["pid"], "1234");
        assert_eq!(parsed["memory"], "8,192 K");
    }
}
