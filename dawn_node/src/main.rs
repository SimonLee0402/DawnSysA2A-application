mod cli;
mod managed_browser;
mod profile;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    env,
    future::Future,
    path::PathBuf,
    pin::Pin,
    process::Command as StdCommand,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use managed_browser::{
    ManagedBrowserPageState, ManagedBrowserSession, activate_managed_browser,
    click_managed_browser, close_managed_browser, collect_managed_browser_console_messages,
    collect_managed_browser_cookies, collect_managed_browser_errors,
    collect_managed_browser_network_requests, collect_managed_browser_trace,
    delete_managed_browser_profile, emulate_managed_browser_device,
    emulate_managed_browser_network, evaluate_managed_browser, export_managed_browser_profile,
    handle_managed_browser_dialog, import_managed_browser_profile, inspect_managed_browser,
    inspect_managed_browser_profile, inspect_managed_browser_storage, launch_managed_browser,
    launch_managed_browser_with_profile, list_managed_browser_profiles,
    mutate_managed_browser_storage, navigate_managed_browser, open_managed_browser_tab,
    open_managed_browser_window, prepare_managed_browser_download, press_key_managed_browser,
    print_to_pdf_managed_browser, refresh_managed_browser, screenshot_managed_browser,
    set_managed_browser_geolocation, set_managed_browser_headers, type_managed_browser,
    upload_managed_browser, wait_for_managed_browser,
};
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
    node_profile: String,
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
    forward_history: Vec<String>,
    pending_form_selector: Option<String>,
    pending_form_fields: BTreeMap<String, String>,
    pending_form_uploads: BTreeMap<String, String>,
    managed: Option<ManagedBrowserSession>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSessionSummary {
    session_id: String,
    runtime: String,
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
    runtime: String,
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
    runtime: String,
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

#[derive(Debug, Clone)]
struct BrowserDeviceEmulationRequest {
    preset: Option<String>,
    width: u32,
    height: u32,
    device_scale_factor: f64,
    mobile: bool,
    touch: bool,
    user_agent: Option<String>,
    platform: Option<String>,
    accept_language: Option<String>,
    reload: bool,
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
    let response = dispatch_command_future(config, runtime_state, envelope).await;

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

fn dispatch_command_future<'a>(
    config: &'a NodeConfig,
    runtime_state: &'a mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> Pin<Box<dyn Future<Output = CommandResultEnvelope> + 'a>> {
    let command_type = envelope.command_type.clone();
    if !is_command_allowed_for_runtime_profile(&config.node_profile, &command_type) {
        return Box::pin(async move {
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "runtime profile `{}` does not allow command type `{}`",
                    config.node_profile, command_type
                )),
            }
        });
    }
    match command_type.as_str() {
        "echo" => Box::pin(async move {
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(envelope.payload),
                error: None,
            }
        }),
        "list_capabilities" => Box::pin(async move {
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "nodeId": config.node_id,
                    "nodeProfile": config.node_profile,
                    "capabilities": config.capabilities,
                    "allowShell": config.allow_shell
                })),
                error: None,
            }
        }),
        "agent_ping" => Box::pin(async move {
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "nodeId": config.node_id,
                    "nodeName": config.node_name,
                    "nodeProfile": config.node_profile,
                    "observedAtUnixMs": unix_timestamp_ms()
                })),
                error: None,
            }
        }),
        "headless_status" => Box::pin(execute_headless_status_command(config, envelope)),
        "headless_observe" => Box::pin(execute_headless_observe_command(config, envelope)),
        "system_info" => Box::pin(execute_system_info_command(config, envelope)),
        "list_directory" => Box::pin(execute_list_directory_command(envelope)),
        "read_file_preview" => Box::pin(execute_read_file_preview_command(envelope)),
        "tail_file_preview" => Box::pin(execute_tail_file_preview_command(envelope)),
        "read_file_range" => Box::pin(execute_read_file_range_command(envelope)),
        "stat_path" => Box::pin(execute_stat_path_command(envelope)),
        "find_paths" => Box::pin(execute_find_paths_command(envelope)),
        "grep_files" => Box::pin(execute_grep_files_command(envelope)),
        "process_snapshot" => Box::pin(execute_process_snapshot_command(envelope)),
        "browser_start" => Box::pin(execute_browser_start_command(runtime_state, envelope)),
        "browser_profiles" => Box::pin(execute_browser_profiles_command(runtime_state, envelope)),
        "browser_profile_inspect" => Box::pin(execute_browser_profile_inspect_command(
            runtime_state,
            envelope,
        )),
        "browser_profile_import" => Box::pin(execute_browser_profile_import_command(
            runtime_state,
            envelope,
        )),
        "browser_profile_export" => Box::pin(execute_browser_profile_export_command(
            runtime_state,
            envelope,
        )),
        "browser_profile_delete" => Box::pin(execute_browser_profile_delete_command(
            runtime_state,
            envelope,
        )),
        "browser_status" => Box::pin(execute_browser_status_command(runtime_state, envelope)),
        "browser_stop" => Box::pin(execute_browser_stop_command(runtime_state, envelope)),
        "browser_navigate" => Box::pin(execute_browser_navigate_command(runtime_state, envelope)),
        "browser_new_tab" => Box::pin(execute_browser_new_tab_command(runtime_state, envelope)),
        "browser_new_window" => {
            Box::pin(execute_browser_new_window_command(runtime_state, envelope))
        }
        "browser_extract" => Box::pin(execute_browser_extract_command(runtime_state, envelope)),
        "browser_click" => Box::pin(execute_browser_click_command(runtime_state, envelope)),
        "browser_back" => Box::pin(execute_browser_back_command(runtime_state, envelope)),
        "browser_forward" => Box::pin(execute_browser_forward_command(runtime_state, envelope)),
        "browser_reload" => Box::pin(execute_browser_reload_command(runtime_state, envelope)),
        "browser_focus" => Box::pin(execute_browser_focus_command(runtime_state, envelope)),
        "browser_close" => Box::pin(execute_browser_close_command(runtime_state, envelope)),
        "browser_tabs" => Box::pin(execute_browser_tabs_command(runtime_state, envelope)),
        "browser_snapshot" => Box::pin(execute_browser_snapshot_command(runtime_state, envelope)),
        "browser_screenshot" => {
            Box::pin(execute_browser_screenshot_command(runtime_state, envelope))
        }
        "browser_pdf" => Box::pin(execute_browser_pdf_command(runtime_state, envelope)),
        "browser_console_messages" => Box::pin(execute_browser_console_messages_command(
            runtime_state,
            envelope,
        )),
        "browser_network_requests" => Box::pin(execute_browser_network_requests_command(
            runtime_state,
            envelope,
        )),
        "browser_network_export" => Box::pin(execute_browser_network_export_command(
            runtime_state,
            envelope,
        )),
        "browser_trace" => Box::pin(execute_browser_trace_command(runtime_state, envelope)),
        "browser_trace_export" => Box::pin(execute_browser_trace_export_command(
            runtime_state,
            envelope,
        )),
        "browser_errors" => Box::pin(execute_browser_errors_command(runtime_state, envelope)),
        "browser_errors_export" => Box::pin(execute_browser_errors_export_command(
            runtime_state,
            envelope,
        )),
        "browser_cookies" => Box::pin(execute_browser_cookies_command(runtime_state, envelope)),
        "browser_storage" => Box::pin(execute_browser_storage_command(runtime_state, envelope)),
        "browser_storage_set" => {
            Box::pin(execute_browser_storage_set_command(runtime_state, envelope))
        }
        "browser_set_headers" => {
            Box::pin(execute_browser_set_headers_command(runtime_state, envelope))
        }
        "browser_set_offline" => {
            Box::pin(execute_browser_set_offline_command(runtime_state, envelope))
        }
        "browser_set_geolocation" => Box::pin(execute_browser_set_geolocation_command(
            runtime_state,
            envelope,
        )),
        "browser_emulate_device" => Box::pin(execute_browser_emulate_device_command(
            runtime_state,
            envelope,
        )),
        "browser_evaluate" => Box::pin(execute_browser_evaluate_command(runtime_state, envelope)),
        "browser_wait_for" => Box::pin(execute_browser_wait_for_command(runtime_state, envelope)),
        "browser_handle_dialog" => Box::pin(execute_browser_handle_dialog_command(
            runtime_state,
            envelope,
        )),
        "browser_press_key" => Box::pin(execute_browser_press_key_command(runtime_state, envelope)),
        "browser_type" => Box::pin(execute_browser_type_command(runtime_state, envelope)),
        "browser_upload" => Box::pin(execute_browser_upload_command(runtime_state, envelope)),
        "browser_download" => Box::pin(execute_browser_download_command(runtime_state, envelope)),
        "browser_form_fill" => Box::pin(execute_browser_form_fill_command(runtime_state, envelope)),
        "browser_form_submit" => {
            Box::pin(execute_browser_form_submit_command(runtime_state, envelope))
        }
        "browser_open" => Box::pin(execute_browser_open_command(envelope)),
        "browser_search" => Box::pin(execute_browser_search_command(envelope)),
        "desktop_open" => Box::pin(execute_desktop_open_command(envelope)),
        "system_lock" => Box::pin(execute_system_lock_command(envelope)),
        "system_sleep" => Box::pin(execute_system_sleep_command(envelope)),
        "desktop_notification" => Box::pin(execute_desktop_notification_command(envelope)),
        "desktop_clipboard_set" => Box::pin(execute_desktop_clipboard_set_command(envelope)),
        "desktop_type_text" => Box::pin(execute_desktop_type_text_command(envelope)),
        "desktop_key_press" => Box::pin(execute_desktop_key_press_command(envelope)),
        "desktop_windows_list" => Box::pin(execute_desktop_windows_list_command(envelope)),
        "desktop_window_focus" => Box::pin(execute_desktop_window_focus_command(envelope)),
        "desktop_wait_for_window" => Box::pin(execute_desktop_wait_for_window_command(envelope)),
        "desktop_focus_app" => Box::pin(execute_desktop_focus_app_command(envelope)),
        "desktop_launch_and_focus" => Box::pin(execute_desktop_launch_and_focus_command(envelope)),
        "desktop_mouse_move" => Box::pin(execute_desktop_mouse_move_command(envelope)),
        "desktop_mouse_click" => Box::pin(execute_desktop_mouse_click_command(envelope)),
        "desktop_screenshot" => Box::pin(execute_desktop_screenshot_command(envelope)),
        "desktop_ocr" => Box::pin(execute_desktop_ocr_command(envelope)),
        "desktop_accessibility_query" => {
            Box::pin(execute_desktop_accessibility_query_command(envelope))
        }
        "desktop_accessibility_click" => {
            Box::pin(execute_desktop_accessibility_click_command(envelope))
        }
        "desktop_accessibility_wait_for" => {
            Box::pin(execute_desktop_accessibility_wait_for_command(envelope))
        }
        "desktop_accessibility_fill" => {
            Box::pin(execute_desktop_accessibility_fill_command(envelope))
        }
        "desktop_accessibility_workflow" => {
            Box::pin(execute_desktop_accessibility_workflow_command(envelope))
        }
        "desktop_accessibility_snapshot" => {
            Box::pin(execute_desktop_accessibility_snapshot_command(envelope))
        }
        "desktop_accessibility_focus" => {
            Box::pin(execute_desktop_accessibility_focus_command(envelope))
        }
        "desktop_accessibility_invoke" => {
            Box::pin(execute_desktop_accessibility_invoke_command(envelope))
        }
        "desktop_accessibility_set_value" => {
            Box::pin(execute_desktop_accessibility_set_value_command(envelope))
        }
        "shell_exec" => Box::pin(execute_shell_command(config, envelope)),
        other => {
            let other = other.to_string();
            Box::pin(async move {
                CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(format!("unsupported command type: {other}")),
                }
            })
        }
    }
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
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(build_system_info_result(config)),
        error: None,
    }
}

fn build_system_info_result(config: &NodeConfig) -> Value {
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

    json!({
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
    })
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

async fn execute_tail_file_preview_command(
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
            error: Some("tail_file_preview requires payload.path".to_string()),
        };
    };

    let max_bytes = payload_usize(&envelope.payload, "maxBytes", 4096, 65_536);
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let preview_start = bytes.len().saturating_sub(max_bytes);
            let preview_bytes = &bytes[preview_start..];
            let preview = String::from_utf8_lossy(preview_bytes).to_string();
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "path": path,
                    "sizeBytes": bytes.len(),
                    "preview": preview,
                    "previewBytes": preview_bytes.len(),
                    "tailStartByte": preview_start,
                    "truncated": preview_start > 0
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

async fn execute_read_file_range_command(
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
            error: Some("read_file_range requires payload.path".to_string()),
        };
    };

    let start_byte = payload_usize(&envelope.payload, "startByte", 0, usize::MAX);
    let max_bytes = payload_usize(&envelope.payload, "maxBytes", 4096, 65_536);
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let bounded_start = start_byte.min(bytes.len());
            let end_byte = bounded_start.saturating_add(max_bytes).min(bytes.len());
            let preview_bytes = &bytes[bounded_start..end_byte];
            let preview = String::from_utf8_lossy(preview_bytes).to_string();
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "path": path,
                    "sizeBytes": bytes.len(),
                    "startByte": bounded_start,
                    "endByte": end_byte,
                    "preview": preview,
                    "previewBytes": preview_bytes.len(),
                    "truncated": end_byte < bytes.len()
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

async fn execute_find_paths_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let root = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let Some(query) = envelope
        .payload
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("find_paths requires payload.query".to_string()),
        };
    };
    let limit = payload_usize(&envelope.payload, "limit", 20, 200);
    let max_depth = payload_usize(&envelope.payload, "maxDepth", 4, 12);
    let root_path = PathBuf::from(root);
    let display_root = root_path.display().to_string();
    let lowered_query = query.to_lowercase();
    let mut pending = VecDeque::from([(root_path.clone(), 0usize)]);
    let mut matches = Vec::new();
    let mut searched_directories = 0usize;
    let mut skipped_directories = 0usize;
    let mut truncated = false;

    while let Some((current_path, depth)) = pending.pop_front() {
        let mut read_dir = match tokio::fs::read_dir(&current_path).await {
            Ok(read_dir) => read_dir,
            Err(_) => {
                skipped_directories += 1;
                continue;
            }
        };
        searched_directories += 1;

        loop {
            let Some(entry) = (match read_dir.next_entry().await {
                Ok(value) => value,
                Err(_) => {
                    skipped_directories += 1;
                    break;
                }
            }) else {
                break;
            };

            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path();
            let metadata = match entry.metadata().await {
                Ok(metadata) => Some(metadata),
                Err(_) => None,
            };
            let is_dir = metadata.as_ref().is_some_and(|item| item.is_dir());
            let is_file = metadata.as_ref().is_some_and(|item| item.is_file());

            if name.to_lowercase().contains(&lowered_query) {
                matches.push(json!({
                    "name": name,
                    "path": entry_path.display().to_string(),
                    "depth": depth + 1,
                    "isDir": is_dir,
                    "isFile": is_file,
                    "len": metadata.as_ref().map(|item| item.len()),
                    "modifiedAtUnixMs": metadata
                        .as_ref()
                        .and_then(|item| item.modified().ok())
                        .map(system_time_to_unix_ms)
                }));
                if matches.len() >= limit {
                    truncated = true;
                    break;
                }
            }

            if is_dir && depth < max_depth {
                pending.push_back((entry_path, depth + 1));
            }
        }

        if truncated {
            break;
        }
    }

    let first_match = matches
        .first()
        .and_then(|item| item.get("path"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "path": display_root.clone(),
            "query": query,
            "limit": limit,
            "maxDepth": max_depth,
            "matches": matches,
            "count": matches.len(),
            "searchedDirectories": searched_directories,
            "skippedDirectories": skipped_directories,
            "truncated": truncated,
            "summary": {
                "query": query,
                "searchRoot": display_root,
                "matchCount": matches.len(),
                "searchedDirectories": searched_directories,
                "skippedDirectories": skipped_directories,
                "truncated": truncated,
                "firstMatch": first_match
            }
        })),
        error: None,
    }
}

async fn execute_grep_files_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let root = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let Some(query) = envelope
        .payload
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("grep_files requires payload.query".to_string()),
        };
    };
    let limit = payload_usize(&envelope.payload, "limit", 20, 200);
    let max_depth = payload_usize(&envelope.payload, "maxDepth", 4, 12);
    let max_bytes = payload_usize(&envelope.payload, "maxBytes", 16_384, 262_144);
    let case_sensitive = envelope
        .payload
        .get("caseSensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let root_path = PathBuf::from(root);
    let display_root = root_path.display().to_string();
    let query_cmp = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut pending = VecDeque::from([(root_path.clone(), 0usize)]);
    let mut matches = Vec::new();
    let mut searched_directories = 0usize;
    let mut searched_files = 0usize;
    let mut skipped_directories = 0usize;
    let mut skipped_files = 0usize;
    let mut truncated = false;

    while let Some((current_path, depth)) = pending.pop_front() {
        let mut read_dir = match tokio::fs::read_dir(&current_path).await {
            Ok(read_dir) => read_dir,
            Err(_) => {
                skipped_directories += 1;
                continue;
            }
        };
        searched_directories += 1;

        loop {
            let Some(entry) = (match read_dir.next_entry().await {
                Ok(value) => value,
                Err(_) => {
                    skipped_directories += 1;
                    break;
                }
            }) else {
                break;
            };

            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = match entry.metadata().await {
                Ok(metadata) => Some(metadata),
                Err(_) => None,
            };
            let is_dir = metadata.as_ref().is_some_and(|item| item.is_dir());
            let is_file = metadata.as_ref().is_some_and(|item| item.is_file());

            if is_dir && depth < max_depth {
                pending.push_back((entry_path.clone(), depth + 1));
            }

            if !is_file {
                continue;
            }

            searched_files += 1;
            let bytes = match tokio::fs::read(&entry_path).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    skipped_files += 1;
                    continue;
                }
            };

            let preview_len = bytes.len().min(max_bytes);
            let text = String::from_utf8_lossy(&bytes[..preview_len]).to_string();
            let text_cmp = if case_sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            };
            if !text_cmp.contains(&query_cmp) {
                continue;
            }

            let preview = build_match_preview(&text, query, case_sensitive, 120);
            matches.push(json!({
                "name": name,
                "path": entry_path.display().to_string(),
                "depth": depth + 1,
                "preview": preview,
                "previewBytes": preview_len,
                "truncated": bytes.len() > max_bytes,
                "len": metadata.as_ref().map(|item| item.len()),
                "modifiedAtUnixMs": metadata
                    .as_ref()
                    .and_then(|item| item.modified().ok())
                    .map(system_time_to_unix_ms)
            }));
            if matches.len() >= limit {
                truncated = true;
                break;
            }
        }

        if truncated {
            break;
        }
    }

    let first_match = matches
        .first()
        .and_then(|item| item.get("path"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "path": display_root.clone(),
            "query": query,
            "limit": limit,
            "maxDepth": max_depth,
            "maxBytes": max_bytes,
            "caseSensitive": case_sensitive,
            "matches": matches,
            "count": matches.len(),
            "searchedDirectories": searched_directories,
            "searchedFiles": searched_files,
            "skippedDirectories": skipped_directories,
            "skippedFiles": skipped_files,
            "truncated": truncated,
            "summary": {
                "query": query,
                "searchRoot": display_root,
                "matchCount": matches.len(),
                "searchedDirectories": searched_directories,
                "searchedFiles": searched_files,
                "skippedDirectories": skipped_directories,
                "skippedFiles": skipped_files,
                "truncated": truncated,
                "firstMatch": first_match
            }
        })),
        error: None,
    }
}

fn build_match_preview(text: &str, query: &str, case_sensitive: bool, max_chars: usize) -> String {
    let query_cmp = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let line = text
        .lines()
        .find(|line| {
            if case_sensitive {
                line.contains(query)
            } else {
                line.to_lowercase().contains(&query_cmp)
            }
        })
        .unwrap_or(text);
    let flattened = line.replace('\r', " ").trim().to_string();
    let preview = flattened.chars().take(max_chars).collect::<String>();
    if flattened.chars().count() > max_chars {
        format!("{preview}...")
    } else {
        preview
    }
}

async fn execute_process_snapshot_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let limit = payload_usize(&envelope.payload, "limit", 50, 500);
    match build_process_snapshot_result(limit) {
        Ok(result) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(result),
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

fn build_process_snapshot_result(limit: usize) -> anyhow::Result<Value> {
    let output = if cfg!(target_os = "windows") {
        StdCommand::new("tasklist")
            .arg("/FO")
            .arg("CSV")
            .arg("/NH")
            .output()
    } else {
        StdCommand::new("ps").arg("-eo").arg("pid=,comm=").output()
    };

    match output {
        Ok(output) => {
            if !output.status.success() {
                anyhow::bail!(
                    "process snapshot command failed: exitCode={:?} stderr={}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let processes = if cfg!(target_os = "windows") {
                parse_windows_tasklist_snapshot(&stdout, limit)
            } else {
                parse_unix_process_snapshot(&stdout, limit)
            };

            Ok(json!({
                "count": processes.len(),
                "limit": limit,
                "processes": processes
            }))
        }
        Err(error) => anyhow::bail!("failed to gather process snapshot: {error}"),
    }
}

async fn execute_headless_status_command(
    config: &NodeConfig,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let mut result = build_system_info_result(config);
    if let Some(object) = result.as_object_mut() {
        object.insert("runtimeProfile".to_string(), json!("headless"));
        object.insert("interactiveDesktop".to_string(), json!(false));
        object.insert("managedBrowserPreferred".to_string(), json!(false));
        object.insert("runtimePolicy".to_string(), headless_runtime_policy());
        object.insert(
            "recommendedCapabilities".to_string(),
            json!([
                "headless_status",
                "headless_observe",
                "system_info",
                "process_snapshot",
                "list_directory",
                "read_file_preview",
                "tail_file_preview",
                "read_file_range",
                "stat_path",
                "find_paths",
                "grep_files"
            ]),
        );
        object.insert(
            "summary".to_string(),
            json!({
                "mode": "read_only_observe",
                "requestedCapabilities": config.capabilities,
                "interactiveCommandsBlocked": true,
                "recommendedCommand": "headless_observe"
            }),
        );
    }
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(result),
        error: None,
    }
}

async fn execute_headless_observe_command(
    config: &NodeConfig,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let process_limit = payload_usize(&envelope.payload, "processLimit", 10, 100);
    let directory_limit = payload_usize(&envelope.payload, "directoryLimit", 10, 100);
    let directory_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");

    let process_snapshot = match build_process_snapshot_result(process_limit) {
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

    let directory_envelope = GatewayCommandEnvelope {
        command_id: format!("{}:list_directory", envelope.command_id),
        command_type: "list_directory".to_string(),
        payload: json!({
            "path": directory_path,
            "limit": directory_limit
        }),
    };
    let directory_result = execute_list_directory_command(directory_envelope).await;
    if directory_result.status != "succeeded" {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: directory_result.error,
        };
    }

    let directory_payload = directory_result.result.unwrap_or(Value::Null);
    let summary =
        summarize_headless_observation(&process_snapshot, &directory_payload, directory_path);

    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "runtimeProfile": "headless",
            "runtimePolicy": headless_runtime_policy(),
            "summary": summary,
            "system": build_system_info_result(config),
            "processSnapshot": process_snapshot,
            "directory": directory_payload,
            "observedAtUnixMs": unix_timestamp_ms()
        })),
        error: None,
    }
}

fn headless_runtime_policy() -> Value {
    json!({
        "mode": "read_only_observe",
        "interactiveDesktop": false,
        "managedBrowserAllowed": false,
        "shellExecAllowed": false,
        "allowedCommandTypes": headless_default_capabilities(),
        "blockedCommandClasses": [
            "desktop_interaction",
            "managed_browser",
            "shell_exec"
        ],
        "recommendedCommands": [
            "headless_status",
            "headless_observe",
            "system_info",
            "process_snapshot",
            "list_directory",
            "read_file_preview",
            "tail_file_preview",
            "read_file_range",
            "stat_path",
            "find_paths",
            "grep_files"
        ]
    })
}

fn summarize_headless_observation(
    process_snapshot: &Value,
    directory: &Value,
    directory_path: &str,
) -> Value {
    let top_process = process_snapshot
        .get("processes")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .map(headless_process_label)
        .unwrap_or_else(|| "unknown".to_string());
    let first_entry = directory
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    json!({
        "mode": "read_only_observe",
        "processCount": process_snapshot.get("count").and_then(Value::as_u64).unwrap_or(0),
        "topProcess": top_process,
        "directoryPath": directory_path,
        "directoryEntryCount": directory.get("count").and_then(Value::as_u64).unwrap_or(0),
        "directoryTruncated": directory.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        "firstDirectoryEntry": if first_entry.is_empty() { Value::Null } else { json!(first_entry) },
        "recommendedNextCommands": [
            "read_file_preview",
            "tail_file_preview",
            "read_file_range",
            "stat_path",
            "list_directory",
            "find_paths",
            "grep_files"
        ]
    })
}

fn headless_process_label(process: &Value) -> String {
    if let Some(name) = process.get("imageName").and_then(Value::as_str) {
        let pid = process.get("pid").and_then(Value::as_str).unwrap_or("?");
        return format!("{name} (pid {pid})");
    }
    if let Some(command) = process.get("command").and_then(Value::as_str) {
        let pid = process.get("pid").and_then(Value::as_str).unwrap_or("?");
        return format!("{command} (pid {pid})");
    }
    "unknown".to_string()
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

async fn execute_system_lock_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    match lock_host_system().await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "system_lock",
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

async fn execute_system_sleep_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    match sleep_host_system().await {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "system_sleep",
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

async fn execute_desktop_notification_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let title = envelope
        .payload
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Dawn Node")
        .to_string();
    let message = envelope
        .payload
        .get("message")
        .or_else(|| envelope.payload.get("text"))
        .or_else(|| envelope.payload.get("body"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if message.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_notification requires payload.message, payload.text, or payload.body"
                    .to_string(),
            ),
        };
    }
    let subtitle = envelope
        .payload
        .get("subtitle")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let app_name = envelope
        .payload
        .get("appName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Dawn Node")
        .to_string();
    let urgency = normalize_desktop_notification_urgency(
        envelope
            .payload
            .get("urgency")
            .and_then(Value::as_str)
            .unwrap_or("info"),
    );
    let duration_ms = envelope
        .payload
        .get("durationMs")
        .and_then(Value::as_u64)
        .unwrap_or(4_000)
        .max(1_000);

    match show_desktop_notification(
        &title,
        &subtitle,
        &message,
        &app_name,
        &urgency,
        duration_ms,
    )
    .await
    {
        Ok(launcher) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_notification",
                "title": title,
                "subtitle": subtitle,
                "appName": app_name,
                "urgency": urgency,
                "durationMs": duration_ms,
                "launcher": launcher,
                "messageLength": message.chars().count(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopBoundingRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopAccessibilityMatch {
    name: Option<String>,
    automation_id: Option<String>,
    class_name: Option<String>,
    control_type: Option<String>,
    native_window_handle: Option<i64>,
    is_enabled: Option<bool>,
    is_offscreen: Option<bool>,
    bounding_rect: Option<DesktopBoundingRect>,
    center_x: Option<i32>,
    center_y: Option<i32>,
    match_score: Option<i32>,
    depth: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopAccessibilityQueryResult {
    visited_nodes: usize,
    matches: Vec<DesktopAccessibilityMatch>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopOcrResult {
    backend: String,
    text: String,
    lines: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct DesktopWindowSelector {
    title: Option<String>,
    handle: Option<String>,
    process_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopAccessibilityNodeSelector {
    name: Option<String>,
    automation_id: Option<String>,
    class_name: Option<String>,
    control_type: Option<String>,
    match_mode: String,
    prefer_visible: bool,
    prefer_enabled: bool,
}

impl Default for DesktopAccessibilityNodeSelector {
    fn default() -> Self {
        Self {
            name: None,
            automation_id: None,
            class_name: None,
            control_type: None,
            match_mode: "contains".to_string(),
            prefer_visible: true,
            prefer_enabled: true,
        }
    }
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

async fn execute_desktop_accessibility_query_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let window_selector = desktop_window_selector_from_payload(&envelope.payload);
    if window_selector.title.is_none()
        && window_selector.handle.is_none()
        && window_selector.process_name.is_none()
    {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_query requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }
    let node_selector = desktop_accessibility_node_selector_from_payload(&envelope.payload);
    if !desktop_accessibility_selector_has_predicate(&node_selector) {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_query requires payload.name, payload.automationId, payload.className, or payload.controlType"
                    .to_string(),
            ),
        };
    }
    let search_depth = envelope
        .payload
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(5);
    let node_limit = envelope
        .payload
        .get("nodeLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(400);
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(10);

    match query_desktop_accessibility_nodes(
        window_selector.title.as_deref(),
        window_selector.handle.as_deref(),
        window_selector.process_name.as_deref(),
        &node_selector,
        search_depth,
        node_limit,
        limit,
    )
    .await
    {
        Ok((window, query)) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_accessibility_query",
                "window": window,
                "selector": node_selector,
                "searchDepth": search_depth,
                "nodeLimit": node_limit,
                "limit": limit,
                "query": query,
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

async fn execute_desktop_accessibility_click_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let window_selector = desktop_window_selector_from_payload(&envelope.payload);
    if window_selector.title.is_none()
        && window_selector.handle.is_none()
        && window_selector.process_name.is_none()
    {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_click requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }
    let node_selector = desktop_accessibility_node_selector_from_payload(&envelope.payload);
    if !desktop_accessibility_selector_has_predicate(&node_selector) {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_click requires payload.name, payload.automationId, payload.className, or payload.controlType"
                    .to_string(),
            ),
        };
    }
    let search_depth = envelope
        .payload
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(5);
    let node_limit = envelope
        .payload
        .get("nodeLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(400);
    let element_index = envelope
        .payload
        .get("elementIndex")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(0);
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
    let limit = element_index.saturating_add(1).max(1);

    match query_desktop_accessibility_nodes(
        window_selector.title.as_deref(),
        window_selector.handle.as_deref(),
        window_selector.process_name.as_deref(),
        &node_selector,
        search_depth,
        node_limit,
        limit,
    )
    .await
    {
        Ok((window, query)) => {
            let Some(target) = query.matches.get(element_index).cloned() else {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: Some(json!({
                        "action": "desktop_accessibility_click",
                        "window": window,
                        "selector": node_selector,
                        "elementIndex": element_index,
                        "query": query,
                    })),
                    error: Some(format!(
                        "desktop_accessibility_click did not find match at elementIndex {}",
                        element_index
                    )),
                };
            };
            let Some(center_x) = target.center_x else {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: Some(json!({
                        "action": "desktop_accessibility_click",
                        "window": window,
                        "selector": node_selector,
                        "elementIndex": element_index,
                        "match": target,
                    })),
                    error: Some(
                        "desktop_accessibility_click target did not expose a usable bounding rectangle"
                            .to_string(),
                    ),
                };
            };
            let Some(center_y) = target.center_y else {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: Some(json!({
                        "action": "desktop_accessibility_click",
                        "window": window,
                        "selector": node_selector,
                        "elementIndex": element_index,
                        "match": target,
                    })),
                    error: Some(
                        "desktop_accessibility_click target did not expose a usable bounding rectangle"
                            .to_string(),
                    ),
                };
            };
            match click_desktop_mouse(button, double_click, Some((center_x, center_y))).await {
                Ok(launcher) => CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "succeeded",
                    result: Some(json!({
                        "action": "desktop_accessibility_click",
                        "window": window,
                        "selector": node_selector,
                        "elementIndex": element_index,
                        "button": button,
                        "doubleClick": double_click,
                        "launcher": launcher,
                        "match": target,
                        "query": query,
                    })),
                    error: None,
                },
                Err(error) => CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: Some(json!({
                        "action": "desktop_accessibility_click",
                        "window": window,
                        "selector": node_selector,
                        "elementIndex": element_index,
                        "button": button,
                        "doubleClick": double_click,
                        "match": target,
                    })),
                    error: Some(error.to_string()),
                },
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

async fn execute_desktop_accessibility_wait_for_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let window_selector = desktop_window_selector_from_payload(&envelope.payload);
    if window_selector.title.is_none()
        && window_selector.handle.is_none()
        && window_selector.process_name.is_none()
    {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_wait_for requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }
    let node_selector = desktop_accessibility_node_selector_from_payload(&envelope.payload);
    if !desktop_accessibility_selector_has_predicate(&node_selector) {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_wait_for requires payload.name, payload.automationId, payload.className, or payload.controlType"
                    .to_string(),
            ),
        };
    }
    let search_depth = envelope
        .payload
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(5);
    let node_limit = envelope
        .payload
        .get("nodeLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(400);
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

    match wait_for_desktop_accessibility_node(
        window_selector.title.as_deref(),
        window_selector.handle.as_deref(),
        window_selector.process_name.as_deref(),
        &node_selector,
        search_depth,
        node_limit,
        timeout_ms,
        poll_ms,
    )
    .await
    {
        Ok((window, query)) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_accessibility_wait_for",
                "window": window,
                "selector": node_selector,
                "searchDepth": search_depth,
                "nodeLimit": node_limit,
                "timeoutMs": timeout_ms,
                "pollMs": poll_ms,
                "query": query,
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

async fn execute_desktop_accessibility_fill_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let window_selector = desktop_window_selector_from_payload(&envelope.payload);
    if window_selector.title.is_none()
        && window_selector.handle.is_none()
        && window_selector.process_name.is_none()
    {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_fill requires payload.title, payload.handle, or payload.processName"
                    .to_string(),
            ),
        };
    }
    let node_selector = desktop_accessibility_node_selector_from_payload(&envelope.payload);
    if !desktop_accessibility_selector_has_predicate(&node_selector) {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_fill requires payload.name, payload.automationId, payload.className, or payload.controlType"
                    .to_string(),
            ),
        };
    }
    let value = envelope
        .payload
        .get("value")
        .or_else(|| envelope.payload.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if value.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("desktop_accessibility_fill requires payload.value".to_string()),
        };
    }
    let search_depth = envelope
        .payload
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(5);
    let node_limit = envelope
        .payload
        .get("nodeLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(400);
    let element_index = envelope
        .payload
        .get("elementIndex")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(0);
    let delay_ms = envelope
        .payload
        .get("delayMs")
        .and_then(Value::as_u64)
        .unwrap_or(250);
    let clear_existing = envelope
        .payload
        .get("clearExisting")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let submit = envelope
        .payload
        .get("submit")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let fallback_to_type = envelope
        .payload
        .get("fallbackToType")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let set_value_attempt = perform_desktop_accessibility_action(
        window_selector.title.as_deref(),
        window_selector.handle.as_deref(),
        window_selector.process_name.as_deref(),
        &node_selector,
        "set_value",
        Some(&value),
        search_depth,
        node_limit,
        element_index,
    )
    .await;
    match set_value_attempt {
        Ok((window, action_result)) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": "desktop_accessibility_fill",
                "mode": "value_pattern",
                "window": window,
                "selector": node_selector,
                "searchDepth": search_depth,
                "nodeLimit": node_limit,
                "valueLength": value.chars().count(),
                "result": action_result,
            })),
            error: None,
        },
        Err(error) if fallback_to_type => {
            let focus_result = match perform_desktop_accessibility_action(
                window_selector.title.as_deref(),
                window_selector.handle.as_deref(),
                window_selector.process_name.as_deref(),
                &node_selector,
                "focus",
                None,
                search_depth,
                node_limit,
                element_index,
            )
            .await
            {
                Ok(result) => result,
                Err(focus_error) => {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: Some(json!({
                            "action": "desktop_accessibility_fill",
                            "selector": node_selector,
                            "fallbackToType": fallback_to_type,
                            "setValueError": error.to_string(),
                        })),
                        error: Some(focus_error.to_string()),
                    };
                }
            };
            if clear_existing {
                if let Err(clear_error) = send_desktop_key_press("CTRL+A", delay_ms).await {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: Some(json!({
                            "action": "desktop_accessibility_fill",
                            "mode": "send_keys",
                            "window": focus_result.0,
                            "selector": node_selector,
                            "setValueError": error.to_string(),
                            "focusResult": focus_result.1,
                        })),
                        error: Some(clear_error.to_string()),
                    };
                }
            }
            let text_launcher = match send_desktop_text(&value, delay_ms).await {
                Ok(launcher) => launcher,
                Err(type_error) => {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: Some(json!({
                            "action": "desktop_accessibility_fill",
                            "mode": "send_keys",
                            "window": focus_result.0,
                            "selector": node_selector,
                            "setValueError": error.to_string(),
                            "focusResult": focus_result.1,
                            "clearExisting": clear_existing,
                        })),
                        error: Some(type_error.to_string()),
                    };
                }
            };
            let submit_result = if submit {
                match send_desktop_key_press("ENTER", delay_ms).await {
                    Ok((launcher, send_keys)) => Some(json!({
                        "launcher": launcher,
                        "sendKeys": send_keys,
                    })),
                    Err(submit_error) => {
                        return CommandResultEnvelope {
                            message_type: "command_result",
                            command_id: envelope.command_id,
                            status: "failed",
                            result: Some(json!({
                                "action": "desktop_accessibility_fill",
                                "mode": "send_keys",
                                "window": focus_result.0,
                                "selector": node_selector,
                                "setValueError": error.to_string(),
                                "focusResult": focus_result.1,
                                "clearExisting": clear_existing,
                                "textLauncher": text_launcher,
                            })),
                            error: Some(submit_error.to_string()),
                        };
                    }
                }
            } else {
                None
            };
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "action": "desktop_accessibility_fill",
                    "mode": "send_keys",
                    "window": focus_result.0,
                    "selector": node_selector,
                    "searchDepth": search_depth,
                    "nodeLimit": node_limit,
                    "valueLength": value.chars().count(),
                    "clearExisting": clear_existing,
                    "submit": submit,
                    "setValueError": error.to_string(),
                    "focusResult": focus_result.1,
                    "textLauncher": text_launcher,
                    "submitResult": submit_result,
                })),
                error: None,
            }
        }
        Err(error) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: Some(json!({
                "action": "desktop_accessibility_fill",
                "selector": node_selector,
                "searchDepth": search_depth,
                "nodeLimit": node_limit,
                "fallbackToType": fallback_to_type,
            })),
            error: Some(error.to_string()),
        },
    }
}

async fn execute_desktop_accessibility_workflow_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(steps) = envelope.payload.get("steps").and_then(Value::as_array) else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_workflow requires payload.steps to be an array".to_string(),
            ),
        };
    };
    if steps.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "desktop_accessibility_workflow requires at least one workflow step".to_string(),
            ),
        };
    }

    let mut results = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        let step_payload =
            match merge_desktop_accessibility_workflow_step_payload(&envelope.payload, step) {
                Ok(payload) => payload,
                Err(error) => {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: Some(json!({
                            "action": "desktop_accessibility_workflow",
                            "stepIndex": index,
                            "results": results,
                        })),
                        error: Some(error.to_string()),
                    };
                }
            };
        let kind = match desktop_accessibility_workflow_step_kind(&step_payload) {
            Ok(kind) => kind,
            Err(error) => {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: Some(json!({
                        "action": "desktop_accessibility_workflow",
                        "stepIndex": index,
                        "results": results,
                    })),
                    error: Some(error.to_string()),
                };
            }
        };
        let step_command_id = format!("{}:workflow:{index}", envelope.command_id);
        let step_result = match kind.as_str() {
            "sleep" => {
                let duration_ms = step_payload
                    .get("durationMs")
                    .or_else(|| step_payload.get("timeoutMs"))
                    .and_then(Value::as_u64)
                    .unwrap_or(250);
                tokio::time::sleep(Duration::from_millis(duration_ms.max(1))).await;
                CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: step_command_id.clone(),
                    status: "succeeded",
                    result: Some(json!({
                        "action": "desktop_accessibility_workflow_sleep",
                        "durationMs": duration_ms,
                    })),
                    error: None,
                }
            }
            "wait_for_window" => {
                execute_desktop_wait_for_window_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_wait_for_window".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "window_focus" => {
                execute_desktop_window_focus_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_window_focus".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "launch_and_focus" => {
                execute_desktop_launch_and_focus_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_launch_and_focus".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "query" => {
                execute_desktop_accessibility_query_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_query".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "wait_for" => {
                execute_desktop_accessibility_wait_for_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_wait_for".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "click" => {
                execute_desktop_accessibility_click_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_click".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "focus" => {
                execute_desktop_accessibility_focus_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_focus".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "invoke" => {
                execute_desktop_accessibility_invoke_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_invoke".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "set_value" => {
                execute_desktop_accessibility_set_value_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_set_value".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "fill" => {
                execute_desktop_accessibility_fill_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_accessibility_fill".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "ocr" => {
                execute_desktop_ocr_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_ocr".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "type_text" => {
                execute_desktop_type_text_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_type_text".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            "key_press" => {
                execute_desktop_key_press_command(GatewayCommandEnvelope {
                    command_id: step_command_id.clone(),
                    command_type: "desktop_key_press".to_string(),
                    payload: step_payload.clone(),
                })
                .await
            }
            other => CommandResultEnvelope {
                message_type: "command_result",
                command_id: step_command_id.clone(),
                status: "failed",
                result: None,
                error: Some(format!(
                    "unsupported desktop_accessibility_workflow step kind `{other}`"
                )),
            },
        };
        let step_summary = json!({
            "index": index,
            "kind": kind,
            "status": step_result.status,
            "result": step_result.result,
            "error": step_result.error,
        });
        let failed = step_result.status != "succeeded";
        results.push(step_summary);
        if failed {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: Some(json!({
                    "action": "desktop_accessibility_workflow",
                    "stepIndex": index,
                    "results": results,
                })),
                error: Some(format!(
                    "desktop_accessibility_workflow stopped at step {} ({})",
                    index, kind
                )),
            };
        }
    }

    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "action": "desktop_accessibility_workflow",
            "stepCount": results.len(),
            "results": results,
        })),
        error: None,
    }
}

async fn execute_desktop_accessibility_focus_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    execute_desktop_accessibility_action_command(envelope, "focus").await
}

async fn execute_desktop_accessibility_invoke_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    execute_desktop_accessibility_action_command(envelope, "invoke").await
}

async fn execute_desktop_accessibility_set_value_command(
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    execute_desktop_accessibility_action_command(envelope, "set_value").await
}

async fn execute_desktop_accessibility_action_command(
    envelope: GatewayCommandEnvelope,
    action: &str,
) -> CommandResultEnvelope {
    let window_selector = desktop_window_selector_from_payload(&envelope.payload);
    if window_selector.title.is_none()
        && window_selector.handle.is_none()
        && window_selector.process_name.is_none()
    {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "desktop_accessibility_{action} requires payload.title, payload.handle, or payload.processName"
            )),
        };
    }
    let node_selector = desktop_accessibility_node_selector_from_payload(&envelope.payload);
    if !desktop_accessibility_selector_has_predicate(&node_selector) {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "desktop_accessibility_{action} requires payload.name, payload.automationId, payload.className, or payload.controlType"
            )),
        };
    }
    let value = if action == "set_value" {
        let value = envelope
            .payload
            .get("value")
            .or_else(|| envelope.payload.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if value.is_empty() {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some("desktop_accessibility_set_value requires payload.value".to_string()),
            };
        }
        Some(value)
    } else {
        None
    };
    let search_depth = envelope
        .payload
        .get("depth")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(5);
    let node_limit = envelope
        .payload
        .get("nodeLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(400);
    let element_index = envelope
        .payload
        .get("elementIndex")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(0);

    match perform_desktop_accessibility_action(
        window_selector.title.as_deref(),
        window_selector.handle.as_deref(),
        window_selector.process_name.as_deref(),
        &node_selector,
        action,
        value.as_deref(),
        search_depth,
        node_limit,
        element_index,
    )
    .await
    {
        Ok((window, action_result)) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "action": format!("desktop_accessibility_{action}"),
                "window": window,
                "selector": node_selector,
                "searchDepth": search_depth,
                "nodeLimit": node_limit,
                "value": value,
                "result": action_result,
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

async fn execute_desktop_ocr_command(envelope: GatewayCommandEnvelope) -> CommandResultEnvelope {
    let requested_backend = envelope
        .payload
        .get("backend")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let language = envelope
        .payload
        .get("language")
        .or_else(|| envelope.payload.get("lang"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let image_path = envelope
        .payload
        .get("imagePath")
        .or_else(|| envelope.payload.get("path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
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
    let keep_image = envelope
        .payload
        .get("keepImage")
        .and_then(Value::as_bool)
        .unwrap_or(image_path.is_some());

    let (image_path, captured) = if let Some(path) = image_path {
        if !path.is_file() {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "desktop_ocr image path `{}` does not exist or is not a file",
                    path.display()
                )),
            };
        }
        (path, false)
    } else {
        let screenshot_path = resolve_desktop_screenshot_path(None);
        if let Err(error) = capture_desktop_screenshot(&screenshot_path, region).await {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
        (screenshot_path, true)
    };

    let backend = match resolve_desktop_ocr_backend(requested_backend.as_deref()).await {
        Ok(backend) => backend,
        Err(error) => {
            if captured && !keep_image {
                let _ = std::fs::remove_file(&image_path);
            }
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(error.to_string()),
            };
        }
    };

    let outcome = match backend {
        "tesseract" => run_tesseract_ocr(&image_path, language.as_deref()).await,
        other => Err(anyhow::anyhow!("unsupported desktop OCR backend `{other}`")),
    };

    match outcome {
        Ok(ocr) => {
            if captured && !keep_image {
                let _ = std::fs::remove_file(&image_path);
            }
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "action": "desktop_ocr",
                    "backend": backend,
                    "language": language,
                    "image": {
                        "path": image_path.display().to_string(),
                        "captured": captured,
                        "retained": !captured || keep_image,
                    },
                    "region": region.map(|(x, y, width, height)| json!({
                        "x": x,
                        "y": y,
                        "width": width,
                        "height": height,
                    })),
                    "ocr": ocr,
                })),
                error: None,
            }
        }
        Err(error) => {
            if captured && !keep_image {
                let _ = std::fs::remove_file(&image_path);
            }
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: Some(json!({
                    "action": "desktop_ocr",
                    "backend": backend,
                    "language": language,
                    "image": {
                        "path": image_path.display().to_string(),
                        "captured": captured,
                        "retained": !captured || keep_image,
                    },
                })),
                error: Some(error.to_string()),
            }
        }
    }
}

const DEFAULT_BROWSER_SESSION_ID: &str = "browser-default";
const MAX_BROWSER_HTML_BYTES: usize = 1_500_000;
const DEFAULT_BROWSER_EXTRACT_LIMIT: usize = 5;
const DEFAULT_BROWSER_TEXT_LIMIT_CHARS: usize = 1_200;
const DEFAULT_BROWSER_SNAPSHOT_LIMIT: usize = 6;
const DEFAULT_DESKTOP_WINDOW_LIMIT: usize = 10;

async fn execute_browser_start_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let raw_url = envelope
        .payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("about:blank")
        .trim()
        .to_string();
    let launch_url = if raw_url.is_empty() || raw_url.eq_ignore_ascii_case("about:blank") {
        "about:blank".to_string()
    } else {
        match normalize_browser_url(&raw_url) {
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
    };
    let session_id = match resolve_browser_start_session_id(
        &runtime_state.browser_sessions,
        &envelope.payload,
    ) {
        Ok(session_id) => session_id,
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
    let profile_name = envelope
        .payload
        .get("profileName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let persistent_profile = envelope
        .payload
        .get("persistProfile")
        .and_then(Value::as_bool)
        .unwrap_or(profile_name.is_some());
    match launch_managed_browser_with_profile(
        &launch_url,
        profile_name.as_deref(),
        persistent_profile,
    )
    .await
    {
        Ok((managed, page)) => {
            let session = match new_browser_client() {
                Ok(client) => build_managed_browser_session(
                    client,
                    managed.clone(),
                    page,
                    "start",
                    Vec::new(),
                ),
                Err(error) => {
                    let cleanup_error = close_managed_browser(&managed, true)
                        .await
                        .err()
                        .map(|value| value.to_string());
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(match cleanup_error {
                            Some(cleanup_error) => {
                                format!("{error}; cleanup also failed: {cleanup_error}")
                            }
                            None => error.to_string(),
                        }),
                    };
                }
            };
            let summary = browser_session_summary(&session_id, &session);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), session);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_start",
                    "profileName": managed.profile_name,
                    "persistentProfile": managed.persistent_profile,
                    "page": summary,
                    "tabs": browser_tab_summaries(
                        &runtime_state.browser_sessions,
                        runtime_state.active_browser_session_id.as_deref(),
                    ),
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

async fn execute_browser_profiles_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    match list_managed_browser_profiles() {
        Ok(profiles) => {
            let active_profiles = runtime_state
                .browser_sessions
                .iter()
                .filter_map(|(session_id, session)| {
                    let managed = session.managed.as_ref()?;
                    let profile_name = managed.profile_name.as_ref()?;
                    Some(json!({
                        "sessionId": session_id,
                        "profileName": profile_name,
                        "persistentProfile": managed.persistent_profile,
                        "debugPort": managed.debug_port,
                        "currentUrl": session.current_url,
                    }))
                })
                .collect::<Vec<_>>();
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "runtime": "managed",
                    "action": "browser_profiles",
                    "profileCount": profiles.len(),
                    "profiles": profiles,
                    "activeProfiles": active_profiles,
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

async fn execute_browser_profile_inspect_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(profile_name) = envelope
        .payload
        .get("profileName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_profile_inspect requires payload.profileName".to_string()),
        };
    };
    let in_use_by_sessions = runtime_state
        .browser_sessions
        .iter()
        .filter_map(|(session_id, session)| {
            let managed = session.managed.as_ref()?;
            let candidate = managed.profile_name.as_deref()?;
            if candidate == profile_name && managed.persistent_profile {
                return Some(json!({
                    "sessionId": session_id,
                    "currentUrl": session.current_url,
                    "debugPort": managed.debug_port,
                    "active": runtime_state
                        .active_browser_session_id
                        .as_deref()
                        .map(|active_id| active_id == session_id)
                        .unwrap_or(false),
                }));
            }
            None
        })
        .collect::<Vec<_>>();
    match inspect_managed_browser_profile(profile_name) {
        Ok(profile) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "runtime": "managed",
                "action": "browser_profile_inspect",
                "profile": profile,
                "inUseBySessions": in_use_by_sessions,
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

async fn execute_browser_profile_import_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(source_path_raw) = envelope
        .payload
        .get("sourcePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_profile_import requires payload.sourcePath".to_string()),
        };
    };
    let source_path = PathBuf::from(source_path_raw);
    let inferred_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty());
    let Some(profile_name) = envelope
        .payload
        .get("profileName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or(inferred_name)
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_profile_import requires payload.profileName or a named source directory"
                    .to_string(),
            ),
        };
    };
    let overwrite = envelope
        .payload
        .get("overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let in_use_by_sessions = runtime_state
        .browser_sessions
        .iter()
        .filter_map(|(session_id, session)| {
            let managed = session.managed.as_ref()?;
            let candidate = managed.profile_name.as_deref()?;
            if candidate == profile_name && managed.persistent_profile {
                return Some(json!({
                    "sessionId": session_id,
                    "currentUrl": session.current_url,
                    "debugPort": managed.debug_port,
                    "active": runtime_state
                        .active_browser_session_id
                        .as_deref()
                        .map(|active_id| active_id == session_id)
                        .unwrap_or(false),
                }));
            }
            None
        })
        .collect::<Vec<_>>();
    if overwrite && !in_use_by_sessions.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: Some(json!({
                "profileName": profile_name,
                "inUseBySessions": in_use_by_sessions,
            })),
            error: Some(format!(
                "managed browser profile `{profile_name}` is still in use by tracked sessions"
            )),
        };
    }
    match import_managed_browser_profile(&source_path, &profile_name, overwrite).await {
        Ok(imported) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "runtime": "managed",
                "action": "browser_profile_import",
                "profile": imported,
                "inUseBySessions": in_use_by_sessions,
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

async fn execute_browser_profile_export_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(profile_name) = envelope
        .payload
        .get("profileName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_profile_export requires payload.profileName".to_string()),
        };
    };
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let export_path = resolve_browser_profile_export_path(requested_path, profile_name);
    let in_use_by_sessions = runtime_state
        .browser_sessions
        .iter()
        .filter_map(|(session_id, session)| {
            let managed = session.managed.as_ref()?;
            let candidate = managed.profile_name.as_deref()?;
            if candidate == profile_name && managed.persistent_profile {
                return Some(json!({
                    "sessionId": session_id,
                    "currentUrl": session.current_url,
                    "debugPort": managed.debug_port,
                    "active": runtime_state
                        .active_browser_session_id
                        .as_deref()
                        .map(|active_id| active_id == session_id)
                        .unwrap_or(false),
                }));
            }
            None
        })
        .collect::<Vec<_>>();
    match export_managed_browser_profile(profile_name, &export_path) {
        Ok(exported) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "runtime": "managed",
                "action": "browser_profile_export",
                "profile": exported,
                "inUseBySessions": in_use_by_sessions,
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

async fn execute_browser_profile_delete_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let Some(profile_name) = envelope
        .payload
        .get("profileName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_profile_delete requires payload.profileName".to_string()),
        };
    };
    let in_use = runtime_state
        .browser_sessions
        .iter()
        .filter_map(|(session_id, session)| {
            let managed = session.managed.as_ref()?;
            let candidate = managed.profile_name.as_deref()?;
            if candidate == profile_name && managed.persistent_profile {
                return Some(json!({
                    "sessionId": session_id,
                    "currentUrl": session.current_url,
                    "debugPort": managed.debug_port,
                }));
            }
            None
        })
        .collect::<Vec<_>>();
    if !in_use.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: Some(json!({
                "profileName": profile_name,
                "inUseBySessions": in_use,
            })),
            error: Some(format!(
                "managed browser profile `{profile_name}` is still in use by tracked sessions"
            )),
        };
    }
    match delete_managed_browser_profile(profile_name).await {
        Ok(deleted) => CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(json!({
                "runtime": "managed",
                "action": "browser_profile_delete",
                "profile": deleted,
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

async fn execute_browser_status_command(
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
                "browser session `{session_id}` not found. Run browser_start or browser_navigate first."
            )),
        };
    };
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_status requires a managed browser session. Run browser_start or browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    match inspect_managed_browser(&managed).await {
        Ok(status) => {
            let tracked_sessions = tracked_managed_browser_group_summaries(runtime_state, &managed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_status",
                    "browser": status,
                    "trackedSessions": tracked_sessions,
                    "activeSessionId": runtime_state.active_browser_session_id,
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

async fn execute_browser_stop_command(
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
                "browser session `{session_id}` not found. Run browser_start or browser_navigate first."
            )),
        };
    };
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_stop requires a managed browser session. Run browser_start or browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let removed_session_ids = tracked_managed_browser_group_session_ids(runtime_state, &managed);
    let removed_tabs = removed_session_ids
        .iter()
        .filter_map(|candidate_id| {
            runtime_state
                .browser_sessions
                .get(candidate_id)
                .map(|candidate| browser_session_summary(candidate_id, candidate))
        })
        .collect::<Vec<_>>();
    for removed_session_id in &removed_session_ids {
        runtime_state.browser_sessions.remove(removed_session_id);
    }
    let cleanup_warning = close_managed_browser(&managed, true)
        .await
        .err()
        .map(|error| error.to_string());
    runtime_state.active_browser_session_id = preferred_active_browser_session_id(
        &runtime_state.browser_sessions,
        runtime_state.active_browser_session_id.as_deref(),
    );
    CommandResultEnvelope {
        message_type: "command_result",
        command_id: envelope.command_id,
        status: "succeeded",
        result: Some(json!({
            "sessionId": session_id,
            "runtime": "managed",
            "action": "browser_stop",
            "stoppedSessionIds": removed_session_ids,
            "stoppedTabs": removed_tabs,
            "cleanupWarning": cleanup_warning,
            "activeSessionId": runtime_state.active_browser_session_id,
            "remainingTabs": browser_tab_summaries(
                &runtime_state.browser_sessions,
                runtime_state.active_browser_session_id.as_deref(),
            ),
        })),
        error: None,
    }
}

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
    let existing_session = runtime_state.browser_sessions.get(&session_id).cloned();
    let use_managed_browser = existing_session
        .as_ref()
        .and_then(|session| session.managed.as_ref())
        .is_some()
        || payload_requests_managed_browser(&envelope.payload);
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

    if use_managed_browser {
        let managed_result = if let Some(session) = existing_session {
            if let Some(managed) = session.managed.clone() {
                let history = browser_navigation_history_with_current(&session);
                match navigate_managed_browser(&managed, &normalized_url).await {
                    Ok(page) => Ok(build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "navigate",
                        history,
                    )),
                    Err(error) => Err(error),
                }
            } else {
                match launch_managed_browser(&normalized_url).await {
                    Ok((managed, page)) => Ok(build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "navigate",
                        Vec::new(),
                    )),
                    Err(error) => Err(error),
                }
            }
        } else {
            match launch_managed_browser(&normalized_url).await {
                Ok((managed, page)) => match new_browser_client() {
                    Ok(client) => Ok(build_managed_browser_session(
                        client,
                        managed,
                        page,
                        "navigate",
                        Vec::new(),
                    )),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };

        return match managed_result {
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
                    result: Some(json!({
                        "sessionId": session_id,
                        "runtime": "managed",
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
        };
    }

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

async fn execute_browser_new_tab_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    execute_browser_new_managed_target_command(runtime_state, envelope, "tab").await
}

async fn execute_browser_new_window_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    execute_browser_new_managed_target_command(runtime_state, envelope, "window").await
}

async fn execute_browser_new_managed_target_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
    target_kind: &str,
) -> CommandResultEnvelope {
    let source_session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let Some(source_session) = runtime_state
        .browser_sessions
        .get(&source_session_id)
        .cloned()
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{source_session_id}` not found. Run browser_navigate first."
            )),
        };
    };
    let Some(managed) = source_session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser_new_{target_kind} requires a managed browser session. Run browser_navigate with payload.managed=true first."
            )),
        };
    };
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
            error: Some(format!("browser_new_{target_kind} requires payload.url")),
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
    let next_session_id = match resolve_new_browser_session_id(
        &runtime_state.browser_sessions,
        &source_session_id,
        target_kind,
        &envelope.payload,
    ) {
        Ok(session_id) => session_id,
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
    let open_result = match target_kind {
        "window" => open_managed_browser_window(&managed, &normalized_url).await,
        _ => open_managed_browser_tab(&managed, &normalized_url).await,
    };
    match open_result {
        Ok((next_managed, page)) => {
            let next_session = build_managed_browser_session(
                source_session.client.clone(),
                next_managed,
                page,
                &format!("new_{target_kind}"),
                Vec::new(),
            );
            let summary = browser_session_summary(&next_session_id, &next_session);
            runtime_state
                .browser_sessions
                .insert(next_session_id.clone(), next_session);
            set_active_browser_session(runtime_state, &next_session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": next_session_id,
                    "sourceSessionId": source_session_id,
                    "runtime": "managed",
                    "action": format!("browser_new_{target_kind}"),
                    "page": summary,
                    "tabs": browser_tab_summaries(
                        &runtime_state.browser_sessions,
                        runtime_state.active_browser_session_id.as_deref(),
                    ),
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
    let (previous_url, history, forward_history) = match browser_back_transition(&session) {
        Ok(result) => result,
        Err(_) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "browser session `{session_id}` has no previous page in history"
                )),
            };
        }
    };
    if let Some(managed) = session.managed.clone() {
        return match navigate_managed_browser(&managed, &previous_url).await {
            Ok(page) => {
                let mut next_session = build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "back",
                    history,
                );
                next_session.forward_history = forward_history;
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
                        "runtime": "managed",
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
        };
    }

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
        Ok(mut next_session) => {
            next_session.forward_history = forward_history;
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

async fn execute_browser_forward_command(
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
    let (next_url, history, forward_history) = match browser_forward_transition(&session) {
        Ok(result) => result,
        Err(_) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "browser session `{session_id}` has no forward page in history"
                )),
            };
        }
    };
    if let Some(managed) = session.managed.clone() {
        return match navigate_managed_browser(&managed, &next_url).await {
            Ok(page) => {
                let mut next_session = build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "forward",
                    history,
                );
                next_session.forward_history = forward_history;
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
                        "action": "browser_forward",
                        "runtime": "managed",
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
        };
    }

    match fetch_browser_session_with_client(
        session.client.clone(),
        &next_url,
        "forward",
        history,
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    )
    .await
    {
        Ok(mut next_session) => {
            next_session.forward_history = forward_history;
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
                    "action": "browser_forward",
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

async fn execute_browser_reload_command(
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
    if session.current_url.trim().is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(format!(
                "browser session `{session_id}` has no current URL to reload"
            )),
        };
    }
    if let Some(managed) = session.managed.clone() {
        return match navigate_managed_browser(&managed, &session.current_url).await {
            Ok(page) => {
                let next_session = preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "reload",
                        session.history.clone(),
                    ),
                    &session,
                );
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
                        "action": "browser_reload",
                        "runtime": "managed",
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
        };
    }

    match fetch_browser_session_with_client(
        session.client.clone(),
        &session.current_url,
        "reload",
        session.history.clone(),
        session.pending_form_fields.clone(),
        session.pending_form_uploads.clone(),
        session.pending_form_selector.clone(),
    )
    .await
    {
        Ok(mut next_session) => {
            next_session.forward_history = session.forward_history.clone();
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
                    "action": "browser_reload",
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
    let activation_warning = if let Some(managed) = session.managed.as_ref() {
        activate_managed_browser(managed)
            .await
            .err()
            .map(|error| error.to_string())
    } else {
        None
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
            "activationWarning": activation_warning,
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
    let cleanup_warning = if let Some(managed) = session.managed.as_ref() {
        let terminate_browser = !runtime_state.browser_sessions.values().any(|candidate| {
            candidate
                .managed
                .as_ref()
                .map(|candidate_managed| {
                    managed_browser_sessions_share_process(candidate_managed, managed)
                })
                .unwrap_or(false)
        });
        close_managed_browser(managed, terminate_browser)
            .await
            .err()
            .map(|error| error.to_string())
    } else {
        None
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
            "cleanupWarning": cleanup_warning,
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
    let session = if let Some(managed) = session.managed.clone() {
        match refresh_managed_browser(&managed).await {
            Ok(page) => {
                let refreshed = preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "snapshot",
                        session.history.clone(),
                    ),
                    &session,
                );
                runtime_state
                    .browser_sessions
                    .insert(session_id.clone(), refreshed.clone());
                refreshed
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
        }
    } else {
        session
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

async fn execute_browser_screenshot_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_screenshot requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let screenshot_path = resolve_browser_screenshot_path(requested_path, &session_id);
    let full_page = envelope
        .payload
        .get("fullPage")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    match screenshot_managed_browser(&managed, &screenshot_path, full_page).await {
        Ok(result) => {
            let page = match refresh_managed_browser(&managed).await {
                Ok(page) => {
                    let refreshed = preserve_browser_forward_history(
                        build_managed_browser_session(
                            session.client.clone(),
                            managed.clone(),
                            page,
                            "screenshot",
                            session.history.clone(),
                        ),
                        &session,
                    );
                    let summary = browser_session_summary(&session_id, &refreshed);
                    runtime_state
                        .browser_sessions
                        .insert(session_id.clone(), refreshed);
                    summary
                }
                Err(_) => browser_session_summary(&session_id, &session),
            };
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_screenshot",
                    "screenshot": result,
                    "page": page,
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

async fn execute_browser_pdf_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_pdf requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let pdf_path = resolve_browser_pdf_path(requested_path, &session_id);
    let landscape = envelope
        .payload
        .get("landscape")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let print_background = envelope
        .payload
        .get("printBackground")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    match print_to_pdf_managed_browser(&managed, &pdf_path, landscape, print_background).await {
        Ok(result) => {
            let page = match refresh_managed_browser(&managed).await {
                Ok(page) => {
                    let refreshed = preserve_browser_forward_history(
                        build_managed_browser_session(
                            session.client.clone(),
                            managed.clone(),
                            page,
                            "pdf",
                            session.history.clone(),
                        ),
                        &session,
                    );
                    let summary = browser_session_summary(&session_id, &refreshed);
                    runtime_state
                        .browser_sessions
                        .insert(session_id.clone(), refreshed);
                    summary
                }
                Err(_) => browser_session_summary(&session_id, &session),
            };
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_pdf",
                    "pdf": result,
                    "page": page,
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

async fn execute_browser_console_messages_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_console_messages requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(50);
    match collect_managed_browser_console_messages(&managed, limit).await {
        Ok(console) => {
            let refreshed = match refresh_managed_browser(&managed).await {
                Ok(page) => preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "console_messages",
                        session.history.clone(),
                    ),
                    &session,
                ),
                Err(_) => session.clone(),
            };
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_console_messages",
                    "console": console,
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

async fn execute_browser_network_requests_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_network_requests requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(25);
    match collect_managed_browser_network_requests(&managed, limit).await {
        Ok(network) => {
            let refreshed = match refresh_managed_browser(&managed).await {
                Ok(page) => preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "network_requests",
                        session.history.clone(),
                    ),
                    &session,
                ),
                Err(_) => session.clone(),
            };
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_network_requests",
                    "network": network,
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

async fn execute_browser_network_export_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_network_export requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(100);
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let export_path = resolve_browser_network_export_path(requested_path, &session_id);

    match collect_managed_browser_network_requests(&managed, limit).await {
        Ok(network) => {
            let encoded = match serde_json::to_vec_pretty(&network) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(format!(
                            "failed to serialize managed browser network log: {error}"
                        )),
                    };
                }
            };
            if let Some(parent) = export_path.parent() {
                if let Err(error) = tokio::fs::create_dir_all(parent).await {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(format!(
                            "failed to create network export directory {}: {error}",
                            parent.display()
                        )),
                    };
                }
            }
            if let Err(error) = tokio::fs::write(&export_path, &encoded).await {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "failed to save managed browser network log to {}: {error}",
                        export_path.display()
                    )),
                };
            }
            let page = match refresh_managed_browser(&managed).await {
                Ok(page) => {
                    let refreshed = preserve_browser_forward_history(
                        build_managed_browser_session(
                            session.client.clone(),
                            managed.clone(),
                            page,
                            "network_export",
                            session.history.clone(),
                        ),
                        &session,
                    );
                    let summary = browser_session_summary(&session_id, &refreshed);
                    runtime_state
                        .browser_sessions
                        .insert(session_id.clone(), refreshed);
                    summary
                }
                Err(_) => browser_session_summary(&session_id, &session),
            };
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_network_export",
                    "networkPath": export_path.display().to_string(),
                    "bytesWritten": encoded.len(),
                    "network": {
                        "currentUrl": network.current_url,
                        "navigationCount": network.navigation_count,
                        "resourceCount": network.resource_count,
                        "entryCount": network.entries.len(),
                    },
                    "page": page,
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

async fn execute_browser_trace_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_trace requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(100);
    match collect_managed_browser_trace(&managed, limit).await {
        Ok(trace) => {
            let refreshed = match refresh_managed_browser(&managed).await {
                Ok(page) => preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "trace",
                        session.history.clone(),
                    ),
                    &session,
                ),
                Err(_) => session.clone(),
            };
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_trace",
                    "trace": trace,
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

async fn execute_browser_trace_export_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_trace_export requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(200);
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let trace_path = resolve_browser_trace_path(requested_path, &session_id);

    match collect_managed_browser_trace(&managed, limit).await {
        Ok(trace) => {
            let encoded = match serde_json::to_vec_pretty(&trace) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(format!(
                            "failed to serialize managed browser trace: {error}"
                        )),
                    };
                }
            };
            if let Some(parent) = trace_path.parent() {
                if let Err(error) = tokio::fs::create_dir_all(parent).await {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(format!(
                            "failed to create trace export directory {}: {error}",
                            parent.display()
                        )),
                    };
                }
            }
            if let Err(error) = tokio::fs::write(&trace_path, &encoded).await {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "failed to save managed browser trace to {}: {error}",
                        trace_path.display()
                    )),
                };
            }
            let page = match refresh_managed_browser(&managed).await {
                Ok(page) => {
                    let refreshed = preserve_browser_forward_history(
                        build_managed_browser_session(
                            session.client.clone(),
                            managed.clone(),
                            page,
                            "trace_export",
                            session.history.clone(),
                        ),
                        &session,
                    );
                    let summary = browser_session_summary(&session_id, &refreshed);
                    runtime_state
                        .browser_sessions
                        .insert(session_id.clone(), refreshed);
                    summary
                }
                Err(_) => browser_session_summary(&session_id, &session),
            };
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_trace_export",
                    "tracePath": trace_path.display().to_string(),
                    "bytesWritten": encoded.len(),
                    "trace": {
                        "currentUrl": trace.current_url,
                        "count": trace.count,
                    },
                    "page": page,
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

async fn execute_browser_errors_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_errors requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let console_limit = envelope
        .payload
        .get("consoleLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(50);
    let network_limit = envelope
        .payload
        .get("networkLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(50);
    match collect_managed_browser_errors(&managed, console_limit, network_limit).await {
        Ok(errors) => {
            let refreshed = match refresh_managed_browser(&managed).await {
                Ok(page) => preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "errors",
                        session.history.clone(),
                    ),
                    &session,
                ),
                Err(_) => session.clone(),
            };
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_errors",
                    "errors": errors,
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

async fn execute_browser_errors_export_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_errors_export requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let console_limit = envelope
        .payload
        .get("consoleLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(100);
    let network_limit = envelope
        .payload
        .get("networkLimit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(100);
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let export_path = resolve_browser_errors_export_path(requested_path, &session_id);

    match collect_managed_browser_errors(&managed, console_limit, network_limit).await {
        Ok(errors) => {
            let encoded = match serde_json::to_vec_pretty(&errors) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(format!(
                            "failed to serialize managed browser errors log: {error}"
                        )),
                    };
                }
            };
            if let Some(parent) = export_path.parent() {
                if let Err(error) = tokio::fs::create_dir_all(parent).await {
                    return CommandResultEnvelope {
                        message_type: "command_result",
                        command_id: envelope.command_id,
                        status: "failed",
                        result: None,
                        error: Some(format!(
                            "failed to create errors export directory {}: {error}",
                            parent.display()
                        )),
                    };
                }
            }
            if let Err(error) = tokio::fs::write(&export_path, &encoded).await {
                return CommandResultEnvelope {
                    message_type: "command_result",
                    command_id: envelope.command_id,
                    status: "failed",
                    result: None,
                    error: Some(format!(
                        "failed to save managed browser errors log to {}: {error}",
                        export_path.display()
                    )),
                };
            }
            let page = match refresh_managed_browser(&managed).await {
                Ok(page) => {
                    let refreshed = preserve_browser_forward_history(
                        build_managed_browser_session(
                            session.client.clone(),
                            managed.clone(),
                            page,
                            "errors_export",
                            session.history.clone(),
                        ),
                        &session,
                    );
                    let summary = browser_session_summary(&session_id, &refreshed);
                    runtime_state
                        .browser_sessions
                        .insert(session_id.clone(), refreshed);
                    summary
                }
                Err(_) => browser_session_summary(&session_id, &session),
            };
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_errors_export",
                    "errorsPath": export_path.display().to_string(),
                    "bytesWritten": encoded.len(),
                    "errors": {
                        "currentUrl": errors.current_url,
                        "consoleCount": errors.console_count,
                        "networkCount": errors.network_count,
                    },
                    "page": page,
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

async fn execute_browser_cookies_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_cookies requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(50);
    match collect_managed_browser_cookies(&managed, limit).await {
        Ok(cookies) => {
            let refreshed = match refresh_managed_browser(&managed).await {
                Ok(page) => preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "cookies",
                        session.history.clone(),
                    ),
                    &session,
                ),
                Err(_) => session.clone(),
            };
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_cookies",
                    "cookies": cookies,
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

async fn execute_browser_storage_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_storage requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let limit = envelope
        .payload
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.max(1) as usize)
        .unwrap_or(100);
    match inspect_managed_browser_storage(&managed, limit).await {
        Ok(storage) => {
            let refreshed = match refresh_managed_browser(&managed).await {
                Ok(page) => preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "storage",
                        session.history.clone(),
                    ),
                    &session,
                ),
                Err(_) => session.clone(),
            };
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_storage",
                    "storage": storage,
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

async fn execute_browser_storage_set_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_storage_set requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let Some(storage_area) = envelope
        .payload
        .get("storageArea")
        .or_else(|| envelope.payload.get("scope"))
        .and_then(Value::as_str)
        .and_then(normalize_browser_storage_area)
    else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_storage_set requires payload.storageArea with one of localStorage/local/sessionStorage/session"
                    .to_string(),
            ),
        };
    };
    let key = envelope
        .payload
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if key.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_storage_set requires payload.key".to_string()),
        };
    }
    let remove = envelope
        .payload
        .get("remove")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let value = envelope
        .payload
        .get("value")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    if !remove && value.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_storage_set requires payload.value unless payload.remove=true".to_string(),
            ),
        };
    }
    match mutate_managed_browser_storage(&managed, storage_area, &key, value.as_deref(), remove)
        .await
    {
        Ok((mutation, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "storage_set",
                    session.history.clone(),
                ),
                &session,
            );
            let summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_storage_set",
                    "mutation": mutation,
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

async fn execute_browser_set_headers_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_set_headers requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let headers = match payload_string_map(&envelope.payload, "headers") {
        Ok(headers) => headers,
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
    let reload = envelope
        .payload
        .get("reload")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match set_managed_browser_headers(&managed, &headers, reload).await {
        Ok((headers_result, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "set_headers",
                    session.history.clone(),
                ),
                &session,
            );
            let summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_set_headers",
                    "headers": headers_result,
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

async fn execute_browser_set_offline_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_set_offline requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let offline = envelope
        .payload
        .get("offline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let latency_ms = envelope
        .payload
        .get("latencyMs")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let download_throughput_kbps = envelope
        .payload
        .get("downloadThroughputKbps")
        .and_then(Value::as_u64);
    let upload_throughput_kbps = envelope
        .payload
        .get("uploadThroughputKbps")
        .and_then(Value::as_u64);
    let reload = envelope
        .payload
        .get("reload")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match emulate_managed_browser_network(
        &managed,
        offline,
        latency_ms,
        download_throughput_kbps,
        upload_throughput_kbps,
        reload,
    )
    .await
    {
        Ok((network_result, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "set_offline",
                    session.history.clone(),
                ),
                &session,
            );
            let summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_set_offline",
                    "network": network_result,
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

async fn execute_browser_set_geolocation_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_set_geolocation requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let Some(latitude) = envelope.payload.get("latitude").and_then(Value::as_f64) else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_set_geolocation requires payload.latitude".to_string()),
        };
    };
    let Some(longitude) = envelope.payload.get("longitude").and_then(Value::as_f64) else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_set_geolocation requires payload.longitude".to_string()),
        };
    };
    let accuracy = envelope
        .payload
        .get("accuracy")
        .and_then(Value::as_f64)
        .unwrap_or(10.0);
    let reload = envelope
        .payload
        .get("reload")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match set_managed_browser_geolocation(&managed, latitude, longitude, accuracy, reload).await {
        Ok((geolocation_result, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "set_geolocation",
                    session.history.clone(),
                ),
                &session,
            );
            let summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_set_geolocation",
                    "geolocation": geolocation_result,
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

async fn execute_browser_emulate_device_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_emulate_device requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let request = match resolve_browser_device_emulation_request(&envelope.payload) {
        Ok(request) => request,
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
    match emulate_managed_browser_device(
        &managed,
        request.preset.as_deref(),
        request.width,
        request.height,
        request.device_scale_factor,
        request.mobile,
        request.touch,
        request.user_agent.as_deref(),
        request.platform.as_deref(),
        request.accept_language.as_deref(),
        request.reload,
    )
    .await
    {
        Ok((device_result, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "emulate_device",
                    session.history.clone(),
                ),
                &session,
            );
            let summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_emulate_device",
                    "device": device_result,
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

async fn execute_browser_evaluate_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let expression = envelope
        .payload
        .get("expression")
        .and_then(Value::as_str)
        .or_else(|| envelope.payload.get("script").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if expression.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_evaluate requires payload.expression".to_string()),
        };
    }
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_evaluate requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let return_by_value = envelope
        .payload
        .get("returnByValue")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let refresh = envelope
        .payload
        .get("refresh")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    match evaluate_managed_browser(&managed, &expression, return_by_value).await {
        Ok(evaluation) => {
            let page_summary = if refresh {
                match refresh_managed_browser(&managed).await {
                    Ok(page) => {
                        let refreshed = preserve_browser_forward_history(
                            build_managed_browser_session(
                                session.client.clone(),
                                managed.clone(),
                                page,
                                "evaluate",
                                session.history.clone(),
                            ),
                            &session,
                        );
                        let summary = browser_session_summary(&session_id, &refreshed);
                        runtime_state
                            .browser_sessions
                            .insert(session_id.clone(), refreshed);
                        summary
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
                }
            } else {
                browser_session_summary(&session_id, &session)
            };
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_evaluate",
                    "evaluation": evaluation,
                    "page": page_summary,
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

async fn execute_browser_wait_for_command(
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
        .map(ToString::to_string);
    let text = envelope
        .payload
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let text_gone = envelope
        .payload
        .get("textGone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if selector.is_none() && text.is_none() && text_gone.is_none() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_wait_for requires at least one of payload.selector, payload.text, or payload.textGone"
                    .to_string(),
            ),
        };
    }
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_wait_for requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let timeout = Duration::from_millis(
        envelope
            .payload
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(10_000)
            .max(100),
    );
    let poll_interval = Duration::from_millis(
        envelope
            .payload
            .get("pollMs")
            .and_then(Value::as_u64)
            .unwrap_or(250)
            .max(50),
    );
    match wait_for_managed_browser(
        &managed,
        selector.as_deref(),
        text.as_deref(),
        text_gone.as_deref(),
        timeout,
        poll_interval,
    )
    .await
    {
        Ok((wait, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "wait_for",
                    session.history.clone(),
                ),
                &session,
            );
            let page_summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_wait_for",
                    "wait": wait,
                    "page": page_summary,
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

async fn execute_browser_handle_dialog_command(
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_handle_dialog requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    let accept = envelope
        .payload
        .get("accept")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let prompt_text = envelope
        .payload
        .get("promptText")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match handle_managed_browser_dialog(&managed, accept, prompt_text).await {
        Ok((handled, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "dialog",
                    session.history.clone(),
                ),
                &session,
            );
            let summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_handle_dialog",
                    "dialog": handled,
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

async fn execute_browser_press_key_command(
    runtime_state: &mut NodeRuntimeState,
    envelope: GatewayCommandEnvelope,
) -> CommandResultEnvelope {
    let session_id = resolve_browser_session_id(runtime_state, &envelope.payload);
    let key = envelope
        .payload
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if key.is_empty() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some("browser_press_key requires payload.key".to_string()),
        };
    }
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
    let Some(managed) = session.managed.clone() else {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(
                "browser_press_key requires a managed browser session. Run browser_navigate with payload.managed=true first."
                    .to_string(),
            ),
        };
    };
    match press_key_managed_browser(&managed, &key).await {
        Ok((pressed, page)) => {
            let refreshed = preserve_browser_forward_history(
                build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "press_key",
                    session.history.clone(),
                ),
                &session,
            );
            let page_summary = browser_session_summary(&session_id, &refreshed);
            runtime_state
                .browser_sessions
                .insert(session_id.clone(), refreshed);
            set_active_browser_session(runtime_state, &session_id);
            CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "succeeded",
                result: Some(json!({
                    "sessionId": session_id,
                    "runtime": "managed",
                    "action": "browser_press_key",
                    "keyPress": pressed,
                    "page": page_summary,
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
    let managed_session = runtime_state
        .browser_sessions
        .get(&session_id)
        .and_then(|session| session.managed.clone());
    if let Some(managed) = managed_session {
        let current_session = runtime_state
            .browser_sessions
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| BrowserSession {
                client: new_browser_client().unwrap_or_else(|_| Client::new()),
                current_url: String::new(),
                page_html: String::new(),
                title: None,
                status_code: 200,
                content_type: Some("text/html".to_string()),
                last_action: "type".to_string(),
                loaded_at_unix_ms: unix_timestamp_ms(),
                history: Vec::new(),
                forward_history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
                managed: Some(managed.clone()),
            });
        return match type_managed_browser(&managed, &selector, &text, append, submit).await {
            Ok((typed, page)) => {
                let mut history = current_session.history.clone();
                if submit && !current_session.current_url.is_empty() {
                    history.push(current_session.current_url.clone());
                }
                let mut next_session = build_managed_browser_session(
                    current_session.client.clone(),
                    managed,
                    page,
                    "type",
                    history,
                );
                if !submit {
                    next_session.forward_history = current_session.forward_history.clone();
                }
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
                        "runtime": "managed",
                        "action": "browser_type",
                        "typed": {
                            "selector": selector,
                            "fieldTag": typed.tag,
                            "fieldType": typed.field_type,
                            "value": typed.value,
                            "append": append,
                            "submitted": typed.submitted,
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
        };
    }
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
    let upload_path = match upload_path.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(format!(
                    "failed to resolve browser_upload path `{}`: {error}",
                    upload_path.display()
                )),
            };
        }
    };
    let staged_path = upload_path.display().to_string();
    let managed_session = runtime_state
        .browser_sessions
        .get(&session_id)
        .and_then(|session| session.managed.clone());
    if let Some(managed) = managed_session {
        let current_session = runtime_state
            .browser_sessions
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| BrowserSession {
                client: new_browser_client().unwrap_or_else(|_| Client::new()),
                current_url: String::new(),
                page_html: String::new(),
                title: None,
                status_code: 200,
                content_type: Some("text/html".to_string()),
                last_action: "upload".to_string(),
                loaded_at_unix_ms: unix_timestamp_ms(),
                history: Vec::new(),
                forward_history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
                managed: Some(managed.clone()),
            });
        return match upload_managed_browser(&managed, &selector, &upload_path, submit).await {
            Ok((uploaded, page)) => {
                let mut history = current_session.history.clone();
                if uploaded.submitted && !current_session.current_url.is_empty() {
                    history.push(current_session.current_url.clone());
                }
                let mut next_session = build_managed_browser_session(
                    current_session.client.clone(),
                    managed,
                    page,
                    "upload",
                    history,
                );
                if !uploaded.submitted {
                    next_session.forward_history = current_session.forward_history.clone();
                }
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
                        "runtime": "managed",
                        "action": "browser_upload",
                        "uploaded": {
                            "selector": selector,
                            "fieldName": uploaded.field_name,
                            "fieldType": uploaded.field_type,
                            "fileName": uploaded.file_name,
                            "fileCount": uploaded.file_count,
                            "path": staged_path,
                            "submitted": uploaded.submitted,
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
        };
    }
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
    let raw_url = envelope
        .payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let selector = envelope
        .payload
        .get("selector")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let element_index = envelope
        .payload
        .get("elementIndex")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let requested_path = envelope
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(managed) = session.managed.clone() {
        let current_session = runtime_state
            .browser_sessions
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| session.clone());
        let (download_request, page) = match prepare_managed_browser_download(
            &managed,
            raw_url,
            selector,
            element_index,
        )
        .await
        {
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
        let output_path =
            resolve_browser_download_path(requested_path, &download_request.source_url);
        let result = match download_managed_browser_resource(
            &current_session.client,
            &session_id,
            &download_request,
            &output_path,
        )
        .await
        {
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
        let next_session = preserve_browser_forward_history(
            build_managed_browser_session(
                current_session.client.clone(),
                managed,
                page,
                "download",
                current_session.history.clone(),
            ),
            &current_session,
        );
        runtime_state
            .browser_sessions
            .insert(session_id.clone(), next_session);
        set_active_browser_session(runtime_state, &session_id);
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "succeeded",
            result: Some(serde_json::to_value(result).unwrap_or_else(|_| json!({}))),
            error: None,
        };
    }
    let source_url = if let Some(raw_url) = raw_url {
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
        let Some(selector) = selector else {
            return CommandResultEnvelope {
                message_type: "command_result",
                command_id: envelope.command_id,
                status: "failed",
                result: None,
                error: Some(
                    "browser_download requires payload.url or payload.selector".to_string(),
                ),
            };
        };
        match resolve_browser_click_target(
            &session.page_html,
            &session.current_url,
            selector,
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
    let session = if let Some(managed) = session.managed.clone() {
        match refresh_managed_browser(&managed).await {
            Ok(page) => {
                let refreshed = preserve_browser_forward_history(
                    build_managed_browser_session(
                        session.client.clone(),
                        managed,
                        page,
                        "extract",
                        session.history.clone(),
                    ),
                    &session,
                );
                runtime_state
                    .browser_sessions
                    .insert(session_id.clone(), refreshed.clone());
                refreshed
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
        }
    } else {
        session
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
    if runtime_state
        .browser_sessions
        .get(&session_id)
        .and_then(|session| session.managed.as_ref())
        .is_some()
    {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(managed_browser_command_not_supported("browser_form_fill")),
        };
    }
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
    if session.managed.is_some() {
        return CommandResultEnvelope {
            message_type: "command_result",
            command_id: envelope.command_id,
            status: "failed",
            result: None,
            error: Some(managed_browser_command_not_supported("browser_form_submit")),
        };
    }
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

    if let Some(managed) = session.managed.clone() {
        let mut history = session.history.clone();
        history.push(session.current_url.clone());
        return match click_managed_browser(&managed, &selector, element_index).await {
            Ok((clicked, page)) => {
                let next_session = build_managed_browser_session(
                    session.client.clone(),
                    managed,
                    page,
                    "click",
                    history,
                );
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
                        "runtime": "managed",
                        "clicked": {
                            "selector": selector,
                            "elementIndex": element_index,
                            "tag": clicked.tag,
                            "text": clicked.text,
                            "href": clicked.href,
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
        };
    }

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

fn normalize_browser_storage_area(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local" | "localstorage" | "local_storage" => Some("localStorage"),
        "session" | "sessionstorage" | "session_storage" => Some("sessionStorage"),
        _ => None,
    }
}

fn resolve_browser_device_emulation_request(
    payload: &Value,
) -> anyhow::Result<BrowserDeviceEmulationRequest> {
    let preset = payload
        .get("preset")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let mut request = match preset.as_deref() {
        Some(value) => match browser_device_emulation_preset(value) {
            Some(preset_request) => preset_request,
            None => anyhow::bail!("unsupported browser_emulate_device preset `{value}`"),
        },
        None => BrowserDeviceEmulationRequest {
            preset: None,
            width: 1440,
            height: 900,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            user_agent: None,
            platform: Some("Windows".to_string()),
            accept_language: None,
            reload: false,
        },
    };
    request.preset = preset;
    if let Some(value) = payload.get("width").and_then(Value::as_u64) {
        request.width = value.max(1) as u32;
    }
    if let Some(value) = payload.get("height").and_then(Value::as_u64) {
        request.height = value.max(1) as u32;
    }
    if let Some(value) = payload.get("deviceScaleFactor").and_then(Value::as_f64) {
        request.device_scale_factor = value.max(0.1);
    }
    if let Some(value) = payload.get("mobile").and_then(Value::as_bool) {
        request.mobile = value;
    }
    if let Some(value) = payload.get("touch").and_then(Value::as_bool) {
        request.touch = value;
    }
    if let Some(value) = payload
        .get("userAgent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.user_agent = Some(value.to_string());
    }
    if let Some(value) = payload
        .get("platform")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.platform = Some(value.to_string());
    }
    if let Some(value) = payload
        .get("acceptLanguage")
        .or_else(|| payload.get("locale"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request.accept_language = Some(value.to_string());
    }
    request.reload = payload
        .get("reload")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(request)
}

fn browser_device_emulation_preset(value: &str) -> Option<BrowserDeviceEmulationRequest> {
    match value.trim().to_ascii_lowercase().as_str() {
        "desktop" | "desktop-default" => Some(BrowserDeviceEmulationRequest {
            preset: Some("desktop".to_string()),
            width: 1440,
            height: 900,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            user_agent: Some(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36".to_string(),
            ),
            platform: Some("Windows".to_string()),
            accept_language: Some("en-US,en;q=0.9".to_string()),
            reload: false,
        }),
        "mobile" | "iphone-13" | "iphone13" => Some(BrowserDeviceEmulationRequest {
            preset: Some("iphone-13".to_string()),
            width: 390,
            height: 844,
            device_scale_factor: 3.0,
            mobile: true,
            touch: true,
            user_agent: Some(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1".to_string(),
            ),
            platform: Some("iPhone".to_string()),
            accept_language: Some("en-US,en;q=0.9".to_string()),
            reload: false,
        }),
        "pixel-7" | "pixel7" | "android" => Some(BrowserDeviceEmulationRequest {
            preset: Some("pixel-7".to_string()),
            width: 412,
            height: 915,
            device_scale_factor: 2.625,
            mobile: true,
            touch: true,
            user_agent: Some(
                "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Mobile Safari/537.36".to_string(),
            ),
            platform: Some("Android".to_string()),
            accept_language: Some("en-US,en;q=0.9".to_string()),
            reload: false,
        }),
        _ => None,
    }
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

fn preferred_active_browser_session_id(
    browser_sessions: &HashMap<String, BrowserSession>,
    preferred_session_id: Option<&str>,
) -> Option<String> {
    if let Some(preferred_session_id) = preferred_session_id
        .filter(|preferred_session_id| browser_sessions.contains_key(*preferred_session_id))
    {
        return Some(preferred_session_id.to_string());
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

fn tracked_managed_browser_group_session_ids(
    runtime_state: &NodeRuntimeState,
    managed: &ManagedBrowserSession,
) -> Vec<String> {
    let mut session_ids = runtime_state
        .browser_sessions
        .iter()
        .filter_map(|(session_id, session)| {
            session
                .managed
                .as_ref()
                .filter(|candidate| managed_browser_sessions_share_process(candidate, managed))
                .map(|_| session_id.clone())
        })
        .collect::<Vec<_>>();
    session_ids.sort();
    session_ids
}

fn tracked_managed_browser_group_summaries(
    runtime_state: &NodeRuntimeState,
    managed: &ManagedBrowserSession,
) -> Vec<BrowserTabSummary> {
    let session_ids = tracked_managed_browser_group_session_ids(runtime_state, managed);
    let mut tabs = session_ids
        .iter()
        .filter_map(|session_id| {
            runtime_state
                .browser_sessions
                .get(session_id)
                .map(|session| BrowserTabSummary {
                    session_id: session_id.clone(),
                    runtime: browser_session_runtime_label(session).to_string(),
                    current_url: session.current_url.clone(),
                    title: session.title.clone(),
                    last_action: session.last_action.clone(),
                    loaded_at_unix_ms: session.loaded_at_unix_ms,
                    history_depth: session.history.len(),
                    pending_form_field_count: session.pending_form_fields.len(),
                    pending_form_upload_count: session.pending_form_uploads.len(),
                    active: runtime_state
                        .active_browser_session_id
                        .as_deref()
                        .map(|active_id| active_id == session_id)
                        .unwrap_or(false),
                })
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

fn resolve_browser_start_session_id(
    sessions: &HashMap<String, BrowserSession>,
    payload: &Value,
) -> anyhow::Result<String> {
    if let Some(explicit) = requested_browser_session_id(payload).or_else(|| {
        payload
            .get("newSessionId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }) {
        if sessions.contains_key(&explicit) {
            anyhow::bail!("browser session `{explicit}` already exists");
        }
        return Ok(explicit);
    }
    if !sessions.contains_key(DEFAULT_BROWSER_SESSION_ID) {
        return Ok(DEFAULT_BROWSER_SESSION_ID.to_string());
    }
    let mut counter = 1usize;
    loop {
        let candidate = format!("browser-managed-{counter}");
        if !sessions.contains_key(&candidate) {
            return Ok(candidate);
        }
        counter += 1;
    }
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

fn resolve_browser_screenshot_path(requested: Option<&str>, session_id: &str) -> PathBuf {
    let mut path = requested
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!(
                "dawn-browser-screenshot-{}-{}.png",
                session_id,
                unix_timestamp_ms()
            ))
        });
    if path.extension().is_none() {
        path.set_extension("png");
    }
    path
}

fn resolve_browser_pdf_path(requested: Option<&str>, session_id: &str) -> PathBuf {
    let mut path = requested
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!(
                "dawn-browser-pdf-{}-{}.pdf",
                session_id,
                unix_timestamp_ms()
            ))
        });
    if path.extension().is_none() {
        path.set_extension("pdf");
    }
    path
}

fn resolve_browser_network_export_path(requested: Option<&str>, session_id: &str) -> PathBuf {
    let mut path = requested
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!(
                "dawn-browser-network-{}-{}.json",
                session_id,
                unix_timestamp_ms()
            ))
        });
    if path.extension().is_none() {
        path.set_extension("json");
    }
    path
}

fn resolve_browser_errors_export_path(requested: Option<&str>, session_id: &str) -> PathBuf {
    let mut path = requested
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!(
                "dawn-browser-errors-{}-{}.json",
                session_id,
                unix_timestamp_ms()
            ))
        });
    if path.extension().is_none() {
        path.set_extension("json");
    }
    path
}

fn resolve_browser_trace_path(requested: Option<&str>, session_id: &str) -> PathBuf {
    let mut path = requested
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!(
                "dawn-browser-trace-{}-{}.json",
                session_id,
                unix_timestamp_ms()
            ))
        });
    if path.extension().is_none() {
        path.set_extension("json");
    }
    path
}

fn resolve_browser_profile_export_path(requested: Option<&str>, profile_name: &str) -> PathBuf {
    match requested.map(PathBuf::from) {
        Some(path) if path.is_dir() => path.join(profile_name),
        Some(path) if path.to_string_lossy().ends_with(['\\', '/']) => path.join(profile_name),
        Some(path) => path,
        None => env::temp_dir().join(format!(
            "dawn-browser-profile-export-{}-{}",
            profile_name,
            unix_timestamp_ms()
        )),
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

async fn download_managed_browser_resource(
    client: &Client,
    session_id: &str,
    request: &managed_browser::ManagedBrowserDownloadRequest,
    output_path: &PathBuf,
) -> anyhow::Result<BrowserDownloadResult> {
    let mut builder = client.get(&request.source_url).header(
        reqwest::header::USER_AGENT,
        request
            .user_agent
            .as_deref()
            .unwrap_or("DawnNode/0.1 managed-browser"),
    );
    if let Some(cookie_header) = request.cookie_header.as_deref() {
        builder = builder.header(reqwest::header::COOKIE, cookie_header);
    }
    if !request.current_url.trim().is_empty() {
        builder = builder.header(reqwest::header::REFERER, request.current_url.as_str());
    }
    let response = builder.send().await.with_context(|| {
        format!(
            "failed to download managed browser resource {}",
            request.source_url
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("browser_download received HTTP {}", status.as_u16());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let bytes = response.bytes().await.with_context(|| {
        format!(
            "failed to read managed browser download body from {}",
            request.source_url
        )
    })?;
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
        source_url: request.source_url.to_string(),
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

fn browser_session_runtime_label(session: &BrowserSession) -> &'static str {
    if session.managed.is_some() {
        "managed"
    } else {
        "http-dom"
    }
}

fn payload_requests_managed_browser(payload: &Value) -> bool {
    payload
        .get("managed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || payload
            .get("runtime")
            .and_then(Value::as_str)
            .map(|value| value.eq_ignore_ascii_case("managed"))
            .unwrap_or(false)
}

fn browser_navigation_history_with_current(session: &BrowserSession) -> Vec<String> {
    let mut history = session.history.clone();
    if !session.current_url.is_empty() {
        history.push(session.current_url.clone());
    }
    history
}

fn browser_back_transition(
    session: &BrowserSession,
) -> anyhow::Result<(String, Vec<String>, Vec<String>)> {
    let Some(previous_url) = session.history.last().cloned() else {
        anyhow::bail!("browser session has no previous page in history");
    };
    let mut history = session.history.clone();
    history.pop();
    let mut forward_history = session.forward_history.clone();
    if !session.current_url.is_empty() {
        forward_history.push(session.current_url.clone());
    }
    Ok((previous_url, history, forward_history))
}

fn browser_forward_transition(
    session: &BrowserSession,
) -> anyhow::Result<(String, Vec<String>, Vec<String>)> {
    let Some(next_url) = session.forward_history.last().cloned() else {
        anyhow::bail!("browser session has no forward page in history");
    };
    let mut forward_history = session.forward_history.clone();
    forward_history.pop();
    let mut history = session.history.clone();
    if !session.current_url.is_empty() {
        history.push(session.current_url.clone());
    }
    Ok((next_url, history, forward_history))
}

fn preserve_browser_forward_history(
    mut next_session: BrowserSession,
    current_session: &BrowserSession,
) -> BrowserSession {
    next_session.forward_history = current_session.forward_history.clone();
    next_session
}

fn resolve_new_browser_session_id(
    sessions: &HashMap<String, BrowserSession>,
    base_session_id: &str,
    session_kind: &str,
    payload: &Value,
) -> anyhow::Result<String> {
    if let Some(explicit) = payload
        .get("newSessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if sessions.contains_key(explicit) {
            anyhow::bail!("browser session `{explicit}` already exists");
        }
        return Ok(explicit.to_string());
    }
    let mut counter = 1usize;
    loop {
        let candidate = format!("{base_session_id}-{session_kind}-{counter}");
        if !sessions.contains_key(&candidate) {
            return Ok(candidate);
        }
        counter += 1;
    }
}

fn managed_browser_sessions_share_process(
    left: &ManagedBrowserSession,
    right: &ManagedBrowserSession,
) -> bool {
    if let (Some(left_pid), Some(right_pid)) = (left.browser_pid, right.browser_pid) {
        return left_pid == right_pid;
    }
    left.debug_port == right.debug_port
        && left.user_data_dir == right.user_data_dir
        && left.executable == right.executable
}

fn managed_browser_command_not_supported(command_type: &str) -> String {
    format!(
        "{command_type} is not yet supported for managed browser sessions; managed mode currently supports navigate, click, snapshot, screenshot, pdf, evaluate, wait_for, handle_dialog, press_key, type, upload, and download"
    )
}

fn build_managed_browser_session(
    client: Client,
    managed: ManagedBrowserSession,
    page: ManagedBrowserPageState,
    action: &str,
    history: Vec<String>,
) -> BrowserSession {
    let _ready_state = page.ready_state.clone();
    BrowserSession {
        client,
        current_url: page.current_url,
        page_html: page.html,
        title: page.title,
        status_code: 200,
        content_type: Some("text/html".to_string()),
        last_action: action.to_string(),
        loaded_at_unix_ms: unix_timestamp_ms(),
        history,
        forward_history: Vec::new(),
        pending_form_selector: None,
        pending_form_fields: BTreeMap::new(),
        pending_form_uploads: BTreeMap::new(),
        managed: Some(managed),
    }
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
        forward_history: Vec::new(),
        pending_form_selector,
        pending_form_fields,
        pending_form_uploads,
        managed: None,
    })
}

fn browser_session_summary(session_id: &str, session: &BrowserSession) -> BrowserSessionSummary {
    let document = Html::parse_document(&session.page_html);
    BrowserSessionSummary {
        session_id: session_id.to_string(),
        runtime: browser_session_runtime_label(session).to_string(),
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
            runtime: browser_session_runtime_label(session).to_string(),
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
        runtime: browser_session_runtime_label(session).to_string(),
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

async fn show_desktop_notification(
    title: &str,
    subtitle: &str,
    message: &str,
    app_name: &str,
    urgency: &str,
    duration_ms: u64,
) -> anyhow::Result<&'static str> {
    if cfg!(target_os = "windows") {
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$notify = New-Object System.Windows.Forms.NotifyIcon
$notify.Icon = switch ($env:DAWN_NOTIFY_URGENCY) {
    'warning' { [System.Drawing.SystemIcons]::Warning }
    'error' { [System.Drawing.SystemIcons]::Error }
    default { [System.Drawing.SystemIcons]::Information }
}
$notify.Text = if ([string]::IsNullOrWhiteSpace($env:DAWN_NOTIFY_APP_NAME)) { 'Dawn Node' } else { $env:DAWN_NOTIFY_APP_NAME.Substring(0, [Math]::Min($env:DAWN_NOTIFY_APP_NAME.Length, 63)) }
$notify.BalloonTipTitle = if ([string]::IsNullOrWhiteSpace($env:DAWN_NOTIFY_SUBTITLE)) { $env:DAWN_NOTIFY_TITLE } else { '{0} [{1}]' -f $env:DAWN_NOTIFY_TITLE, $env:DAWN_NOTIFY_SUBTITLE }
$notify.BalloonTipText = $env:DAWN_NOTIFY_MESSAGE
$notify.Visible = $true
$notify.ShowBalloonTip([Math]::Max([int]$env:DAWN_NOTIFY_DURATION_MS, 1000))
Start-Sleep -Milliseconds ([Math]::Max([int]$env:DAWN_NOTIFY_DURATION_MS, 1500))
$notify.Dispose()
"#;
        let mut command = Command::new("powershell");
        command
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .env("DAWN_NOTIFY_TITLE", title)
            .env("DAWN_NOTIFY_SUBTITLE", subtitle)
            .env("DAWN_NOTIFY_MESSAGE", message)
            .env("DAWN_NOTIFY_APP_NAME", app_name)
            .env("DAWN_NOTIFY_URGENCY", urgency)
            .env("DAWN_NOTIFY_DURATION_MS", duration_ms.to_string());
        let status = command.status().await?;
        if !status.success() {
            anyhow::bail!(
                "desktop notification exited with status {:?}",
                status.code()
            );
        }
        return Ok("powershell:System.Windows.Forms.NotifyIcon");
    }
    if cfg!(target_os = "macos") {
        let script = if subtitle.is_empty() {
            "display notification (system attribute \"DAWN_NOTIFY_MESSAGE\") with title (system attribute \"DAWN_NOTIFY_TITLE\")"
        } else {
            "display notification (system attribute \"DAWN_NOTIFY_MESSAGE\") with title (system attribute \"DAWN_NOTIFY_TITLE\") subtitle (system attribute \"DAWN_NOTIFY_SUBTITLE\")"
        };
        let mut command = Command::new("osascript");
        command
            .arg("-e")
            .arg(script)
            .env("DAWN_NOTIFY_TITLE", title)
            .env("DAWN_NOTIFY_SUBTITLE", subtitle)
            .env("DAWN_NOTIFY_MESSAGE", message);
        let status = command.status().await?;
        if !status.success() {
            anyhow::bail!(
                "desktop notification exited with status {:?}",
                status.code()
            );
        }
        return Ok("osascript:display notification");
    }
    if cfg!(target_os = "linux") {
        let urgency_flag = match urgency {
            "error" => "critical",
            "warning" => "normal",
            _ => "low",
        };
        let body = if subtitle.is_empty() {
            message.to_string()
        } else {
            format!("{subtitle}\n{message}")
        };
        let status = Command::new("notify-send")
            .arg("-u")
            .arg(urgency_flag)
            .arg("-t")
            .arg(duration_ms.to_string())
            .arg("-a")
            .arg(app_name)
            .arg(title)
            .arg(body)
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!(
                "desktop notification exited with status {:?}",
                status.code()
            );
        }
        return Ok("notify-send");
    }
    anyhow::bail!("desktop_notification is not implemented for this operating system")
}

async fn lock_host_system() -> anyhow::Result<&'static str> {
    if cfg!(target_os = "windows") {
        let status = Command::new("rundll32.exe")
            .arg("user32.dll,LockWorkStation")
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("system_lock exited with status {:?}", status.code());
        }
        return Ok("rundll32:user32.dll,LockWorkStation");
    }
    if cfg!(target_os = "macos") {
        let status = Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"q\" using {control down, command down}")
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("system_lock exited with status {:?}", status.code());
        }
        return Ok("osascript:lock-screen-shortcut");
    }
    if cfg!(target_os = "linux") {
        let status = Command::new("loginctl")
            .arg("lock-session")
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("system_lock exited with status {:?}", status.code());
        }
        return Ok("loginctl lock-session");
    }
    anyhow::bail!("system_lock is not implemented for this operating system")
}

async fn sleep_host_system() -> anyhow::Result<&'static str> {
    if cfg!(target_os = "windows") {
        let status = Command::new("rundll32.exe")
            .arg("powrprof.dll,SetSuspendState")
            .arg("0,1,0")
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("system_sleep exited with status {:?}", status.code());
        }
        return Ok("rundll32:powrprof.dll,SetSuspendState");
    }
    if cfg!(target_os = "macos") {
        let status = Command::new("pmset").arg("sleepnow").status().await?;
        if !status.success() {
            anyhow::bail!("system_sleep exited with status {:?}", status.code());
        }
        return Ok("pmset sleepnow");
    }
    if cfg!(target_os = "linux") {
        let status = Command::new("systemctl").arg("suspend").status().await?;
        if !status.success() {
            anyhow::bail!("system_sleep exited with status {:?}", status.code());
        }
        return Ok("systemctl suspend");
    }
    anyhow::bail!("system_sleep is not implemented for this operating system")
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

fn normalize_desktop_notification_urgency(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "warn" | "warning" => "warning".to_string(),
        "err" | "error" | "critical" => "error".to_string(),
        _ => "info".to_string(),
    }
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

async fn perform_desktop_accessibility_action(
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
    node_selector: &DesktopAccessibilityNodeSelector,
    action: &str,
    value: Option<&str>,
    search_depth: usize,
    node_limit: usize,
    element_index: usize,
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
    throw "desktop accessibility action requires a window handle"
}
$handleNumber = [Convert]::ToInt64($handleText.Replace("0x", ""), 16)
$nativeHandle = [IntPtr]::new($handleNumber)
$root = [System.Windows.Automation.AutomationElement]::FromHandle($nativeHandle)
if ($null -eq $root) {
    throw "failed to resolve automation root for the requested desktop window"
}
$action = ([string]$env:DAWN_ACCESSIBILITY_ACTION).Trim().ToLowerInvariant()
$selectorName = ([string]$env:DAWN_ACCESSIBILITY_NAME).Trim().ToLowerInvariant()
$selectorAutomationId = ([string]$env:DAWN_ACCESSIBILITY_AUTOMATION_ID).Trim().ToLowerInvariant()
$selectorClassName = ([string]$env:DAWN_ACCESSIBILITY_CLASS_NAME).Trim().ToLowerInvariant()
$selectorControlType = ([string]$env:DAWN_ACCESSIBILITY_CONTROL_TYPE).Trim().ToLowerInvariant()
if ($selectorControlType.StartsWith("controltype.")) {
    $selectorControlType = $selectorControlType.Substring(12)
}
$matchMode = ([string]$env:DAWN_ACCESSIBILITY_MATCH_MODE).Trim().ToLowerInvariant()
if ([string]::IsNullOrWhiteSpace($matchMode)) {
    $matchMode = "contains"
}
$preferVisible = ([string]$env:DAWN_ACCESSIBILITY_PREFER_VISIBLE).Trim().ToLowerInvariant() -ne "false"
$preferEnabled = ([string]$env:DAWN_ACCESSIBILITY_PREFER_ENABLED).Trim().ToLowerInvariant() -ne "false"
$elementIndex = [Math]::Max(0, [int]$env:DAWN_ACCESSIBILITY_ELEMENT_INDEX)
$actionValue = $env:DAWN_ACCESSIBILITY_VALUE
$maxDepth = [int]$env:DAWN_ACCESSIBILITY_SEARCH_DEPTH
$nodeLimit = [int]$env:DAWN_ACCESSIBILITY_NODE_LIMIT
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker

function Test-DawnTextMatch([string]$actual, [string]$needle) {
    if ([string]::IsNullOrWhiteSpace($needle)) { return $true }
    if ([string]::IsNullOrWhiteSpace($actual)) { return $false }
    $normalizedActual = $actual.Trim().ToLowerInvariant()
    switch ($matchMode) {
        "exact" { return $normalizedActual -eq $needle }
        "starts_with" { return $normalizedActual.StartsWith($needle) }
        default { return $normalizedActual.Contains($needle) }
    }
}

function Convert-DawnAutomationSummary($element, [int]$depth, [int]$matchScore) {
    $current = $element.Current
    $bounds = $current.BoundingRectangle
    $width = [int][Math]::Round($bounds.Width)
    $height = [int][Math]::Round($bounds.Height)
    $x = [int][Math]::Round($bounds.Left)
    $y = [int][Math]::Round($bounds.Top)
    $centerX = if ($width -gt 0) { $x + [int][Math]::Floor($width / 2) } else { $null }
    $centerY = if ($height -gt 0) { $y + [int][Math]::Floor($height / 2) } else { $null }
    return [pscustomobject]@{
        name = if ([string]::IsNullOrWhiteSpace($current.Name)) { $null } else { $current.Name }
        automationId = if ([string]::IsNullOrWhiteSpace($current.AutomationId)) { $null } else { $current.AutomationId }
        className = if ([string]::IsNullOrWhiteSpace($current.ClassName)) { $null } else { $current.ClassName }
        controlType = if ([string]::IsNullOrWhiteSpace($current.ControlType.ProgrammaticName)) { $null } else { $current.ControlType.ProgrammaticName }
        nativeWindowHandle = if ($current.NativeWindowHandle -eq 0) { $null } else { [int64]$current.NativeWindowHandle }
        isEnabled = $current.IsEnabled
        isOffscreen = $current.IsOffscreen
        boundingRect = if ($width -gt 0 -and $height -gt 0) {
            [pscustomobject]@{
                x = $x
                y = $y
                width = $width
                height = $height
            }
        } else {
            $null
        }
        centerX = $centerX
        centerY = $centerY
        matchScore = $matchScore
        depth = $depth
    }
}

function Test-DawnAutomationMatch($element) {
    if ($null -eq $element) { return $false }
    $current = $element.Current
    if (-not [string]::IsNullOrWhiteSpace($selectorName)) {
        if (-not (Test-DawnTextMatch ([string]$current.Name) $selectorName)) { return $false }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorAutomationId)) {
        $currentAutomationId = ([string]$current.AutomationId).Trim().ToLowerInvariant()
        if ($currentAutomationId -ne $selectorAutomationId) { return $false }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorClassName)) {
        if (-not (Test-DawnTextMatch ([string]$current.ClassName) $selectorClassName)) { return $false }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorControlType)) {
        $controlType = ([string]$current.ControlType.ProgrammaticName).Trim().ToLowerInvariant()
        if ($controlType.StartsWith("controltype.")) {
            $controlType = $controlType.Substring(12)
        }
        if ($controlType -ne $selectorControlType) { return $false }
    }
    return $true
}

function Get-DawnMatchScore($element, [int]$depth) {
    $current = $element.Current
    $score = 0
    if (-not [string]::IsNullOrWhiteSpace($selectorAutomationId)) {
        $score += 120
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorName)) {
        $normalizedName = ([string]$current.Name).Trim().ToLowerInvariant()
        switch ($matchMode) {
            "exact" { $score += 90 }
            "starts_with" { $score += 75 }
            default {
                if ($normalizedName -eq $selectorName) {
                    $score += 80
                } elseif ($normalizedName.StartsWith($selectorName)) {
                    $score += 70
                } else {
                    $score += 55
                }
            }
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorClassName)) {
        $score += 35
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorControlType)) {
        $score += 35
    }
    if ($preferVisible) {
        if ($current.IsOffscreen) { $score -= 40 } else { $score += 18 }
    }
    if ($preferEnabled) {
        if ($current.IsEnabled) { $score += 18 } else { $score -= 30 }
    }
    $bounds = $current.BoundingRectangle
    if ($bounds.Width -gt 0 -and $bounds.Height -gt 0) {
        $score += 10
    } else {
        $score -= 10
    }
    $score += [Math]::Max(0, 12 - $depth)
    return [int]$score
}

$stack = New-Object System.Collections.Generic.Stack[object]
$stack.Push([pscustomobject]@{ element = $root; depth = 0 })
$visitedNodes = 0
$candidates = New-Object System.Collections.Generic.List[object]
while ($stack.Count -gt 0 -and $visitedNodes -lt $nodeLimit) {
    $frame = $stack.Pop()
    $element = $frame.element
    $depth = [int]$frame.depth
    $visitedNodes += 1
    if (Test-DawnAutomationMatch $element) {
        $matchScore = Get-DawnMatchScore $element $depth
        $candidates.Add([pscustomobject]@{
            element = $element
            depth = $depth
            matchScore = $matchScore
            node = (Convert-DawnAutomationSummary $element $depth $matchScore)
        })
    }
    if ($depth -ge $maxDepth) {
        continue
    }
    $children = New-Object System.Collections.Generic.List[object]
    $child = $walker.GetFirstChild($element)
    while ($child -ne $null) {
        $children.Add($child)
        $child = $walker.GetNextSibling($child)
    }
    for ($index = $children.Count - 1; $index -ge 0; $index -= 1) {
        $stack.Push([pscustomobject]@{ element = $children[$index]; depth = ($depth + 1) })
    }
}

$sortedCandidates = @(
    $candidates | Sort-Object -Property `
        @{ Expression = { $_.matchScore }; Descending = $true }, `
        @{ Expression = { if ($_.node.isOffscreen -eq $true) { 1 } else { 0 } }; Descending = $false }, `
        @{ Expression = { if ($_.node.isEnabled -eq $false) { 1 } else { 0 } }; Descending = $false }, `
        @{ Expression = { $_.depth }; Descending = $false }
)

if ($sortedCandidates.Count -eq 0) {
    throw "no accessibility node matched the requested selector"
}

if ($elementIndex -ge $sortedCandidates.Count) {
    throw ("no accessibility node matched the requested selector at elementIndex {0}" -f $elementIndex)
}

$selectedCandidate = $sortedCandidates[$elementIndex]
$match = $selectedCandidate.element

$patternUsed = $null
switch ($action) {
    "focus" {
        $match.SetFocus()
        $patternUsed = "SetFocus"
    }
    "invoke" {
        $pattern = $null
        if ($match.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
            ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
            $patternUsed = "InvokePattern"
        } elseif ($match.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
            ([System.Windows.Automation.SelectionItemPattern]$pattern).Select()
            $patternUsed = "SelectionItemPattern"
        } elseif ($match.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
            ([System.Windows.Automation.TogglePattern]$pattern).Toggle()
            $patternUsed = "TogglePattern"
        } else {
            throw "desktop accessibility invoke did not find an invoke-capable pattern"
        }
    }
    "set_value" {
        if ([string]::IsNullOrWhiteSpace($actionValue)) {
            throw "desktop accessibility set_value requires a non-empty value"
        }
        $pattern = $null
        if ($match.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            ([System.Windows.Automation.ValuePattern]$pattern).SetValue($actionValue)
            $patternUsed = "ValuePattern"
        } else {
            throw "desktop accessibility set_value requires a node with ValuePattern"
        }
    }
    default {
        throw "unsupported accessibility action"
    }
}

[pscustomobject]@{
    action = $action
    patternUsed = $patternUsed
    node = (Convert-DawnAutomationSummary $match $selectedCandidate.depth $selectedCandidate.matchScore)
    selectedIndex = $elementIndex
    candidateCount = $sortedCandidates.Count
    visitedNodes = $visitedNodes
} | ConvertTo-Json -Depth 8 -Compress
"#;
    let stdout = run_windows_powershell_capture(
        script,
        &[
            ("DAWN_ACCESSIBILITY_HANDLE", window.handle.clone()),
            ("DAWN_ACCESSIBILITY_ACTION", action.to_string()),
            (
                "DAWN_ACCESSIBILITY_NAME",
                node_selector.name.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_AUTOMATION_ID",
                node_selector.automation_id.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_CLASS_NAME",
                node_selector.class_name.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_CONTROL_TYPE",
                node_selector.control_type.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_MATCH_MODE",
                node_selector.match_mode.clone(),
            ),
            (
                "DAWN_ACCESSIBILITY_PREFER_VISIBLE",
                node_selector.prefer_visible.to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_PREFER_ENABLED",
                node_selector.prefer_enabled.to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_VALUE",
                value.unwrap_or_default().to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_SEARCH_DEPTH",
                search_depth.max(1).to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_NODE_LIMIT",
                node_limit.max(1).to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_ELEMENT_INDEX",
                element_index.to_string(),
            ),
        ],
    )
    .await?;
    let result = serde_json::from_str(&stdout)
        .context("failed to parse desktop accessibility action JSON")?;
    Ok((window, result))
}

async fn query_desktop_accessibility_nodes(
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
    node_selector: &DesktopAccessibilityNodeSelector,
    search_depth: usize,
    node_limit: usize,
    limit: usize,
) -> anyhow::Result<(DesktopWindowEntry, DesktopAccessibilityQueryResult)> {
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
    throw "desktop accessibility query requires a window handle"
}
$handleNumber = [Convert]::ToInt64($handleText.Replace("0x", ""), 16)
$nativeHandle = [IntPtr]::new($handleNumber)
$root = [System.Windows.Automation.AutomationElement]::FromHandle($nativeHandle)
if ($null -eq $root) {
    throw "failed to resolve automation root for the requested desktop window"
}
$selectorName = ([string]$env:DAWN_ACCESSIBILITY_NAME).Trim().ToLowerInvariant()
$selectorAutomationId = ([string]$env:DAWN_ACCESSIBILITY_AUTOMATION_ID).Trim().ToLowerInvariant()
$selectorClassName = ([string]$env:DAWN_ACCESSIBILITY_CLASS_NAME).Trim().ToLowerInvariant()
$selectorControlType = ([string]$env:DAWN_ACCESSIBILITY_CONTROL_TYPE).Trim().ToLowerInvariant()
if ($selectorControlType.StartsWith("controltype.")) {
    $selectorControlType = $selectorControlType.Substring(12)
}
$matchMode = ([string]$env:DAWN_ACCESSIBILITY_MATCH_MODE).Trim().ToLowerInvariant()
if ([string]::IsNullOrWhiteSpace($matchMode)) {
    $matchMode = "contains"
}
$preferVisible = ([string]$env:DAWN_ACCESSIBILITY_PREFER_VISIBLE).Trim().ToLowerInvariant() -ne "false"
$preferEnabled = ([string]$env:DAWN_ACCESSIBILITY_PREFER_ENABLED).Trim().ToLowerInvariant() -ne "false"
$maxDepth = [int]$env:DAWN_ACCESSIBILITY_SEARCH_DEPTH
$nodeLimit = [int]$env:DAWN_ACCESSIBILITY_NODE_LIMIT
$matchLimit = [int]$env:DAWN_ACCESSIBILITY_MATCH_LIMIT
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker

function Test-DawnTextMatch([string]$actual, [string]$needle) {
    if ([string]::IsNullOrWhiteSpace($needle)) { return $true }
    if ([string]::IsNullOrWhiteSpace($actual)) { return $false }
    $normalizedActual = $actual.Trim().ToLowerInvariant()
    switch ($matchMode) {
        "exact" { return $normalizedActual -eq $needle }
        "starts_with" { return $normalizedActual.StartsWith($needle) }
        default { return $normalizedActual.Contains($needle) }
    }
}

function Convert-DawnAutomationSummary($element, [int]$depth, [int]$matchScore) {
    $current = $element.Current
    $bounds = $current.BoundingRectangle
    $width = [int][Math]::Round($bounds.Width)
    $height = [int][Math]::Round($bounds.Height)
    $x = [int][Math]::Round($bounds.Left)
    $y = [int][Math]::Round($bounds.Top)
    $centerX = if ($width -gt 0) { $x + [int][Math]::Floor($width / 2) } else { $null }
    $centerY = if ($height -gt 0) { $y + [int][Math]::Floor($height / 2) } else { $null }
    return [pscustomobject]@{
        name = if ([string]::IsNullOrWhiteSpace($current.Name)) { $null } else { $current.Name }
        automationId = if ([string]::IsNullOrWhiteSpace($current.AutomationId)) { $null } else { $current.AutomationId }
        className = if ([string]::IsNullOrWhiteSpace($current.ClassName)) { $null } else { $current.ClassName }
        controlType = if ([string]::IsNullOrWhiteSpace($current.ControlType.ProgrammaticName)) { $null } else { $current.ControlType.ProgrammaticName }
        nativeWindowHandle = if ($current.NativeWindowHandle -eq 0) { $null } else { [int64]$current.NativeWindowHandle }
        isEnabled = $current.IsEnabled
        isOffscreen = $current.IsOffscreen
        boundingRect = if ($width -gt 0 -and $height -gt 0) {
            [pscustomobject]@{
                x = $x
                y = $y
                width = $width
                height = $height
            }
        } else {
            $null
        }
        centerX = $centerX
        centerY = $centerY
        matchScore = $matchScore
        depth = $depth
    }
}

function Test-DawnAutomationMatch($element) {
    if ($null -eq $element) { return $false }
    $current = $element.Current
    if (-not [string]::IsNullOrWhiteSpace($selectorName)) {
        if (-not (Test-DawnTextMatch ([string]$current.Name) $selectorName)) { return $false }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorAutomationId)) {
        $currentAutomationId = ([string]$current.AutomationId).Trim().ToLowerInvariant()
        if ($currentAutomationId -ne $selectorAutomationId) { return $false }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorClassName)) {
        if (-not (Test-DawnTextMatch ([string]$current.ClassName) $selectorClassName)) { return $false }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorControlType)) {
        $controlType = ([string]$current.ControlType.ProgrammaticName).Trim().ToLowerInvariant()
        if ($controlType.StartsWith("controltype.")) {
            $controlType = $controlType.Substring(12)
        }
        if ($controlType -ne $selectorControlType) { return $false }
    }
    return $true
}

function Get-DawnMatchScore($element, [int]$depth) {
    $current = $element.Current
    $score = 0
    if (-not [string]::IsNullOrWhiteSpace($selectorAutomationId)) {
        $score += 120
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorName)) {
        $normalizedName = ([string]$current.Name).Trim().ToLowerInvariant()
        switch ($matchMode) {
            "exact" { $score += 90 }
            "starts_with" { $score += 75 }
            default {
                if ($normalizedName -eq $selectorName) {
                    $score += 80
                } elseif ($normalizedName.StartsWith($selectorName)) {
                    $score += 70
                } else {
                    $score += 55
                }
            }
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorClassName)) {
        $score += 35
    }
    if (-not [string]::IsNullOrWhiteSpace($selectorControlType)) {
        $score += 35
    }
    if ($preferVisible) {
        if ($current.IsOffscreen) { $score -= 40 } else { $score += 18 }
    }
    if ($preferEnabled) {
        if ($current.IsEnabled) { $score += 18 } else { $score -= 30 }
    }
    $bounds = $current.BoundingRectangle
    if ($bounds.Width -gt 0 -and $bounds.Height -gt 0) {
        $score += 10
    } else {
        $score -= 10
    }
    $score += [Math]::Max(0, 12 - $depth)
    return [int]$score
}

$stack = New-Object System.Collections.Generic.Stack[object]
$stack.Push([pscustomobject]@{ element = $root; depth = 0 })
$visitedNodes = 0
$matches = New-Object System.Collections.Generic.List[object]
while ($stack.Count -gt 0 -and $visitedNodes -lt $nodeLimit) {
    $frame = $stack.Pop()
    $element = $frame.element
    $depth = [int]$frame.depth
    $visitedNodes += 1
    if (Test-DawnAutomationMatch $element) {
        $matchScore = Get-DawnMatchScore $element $depth
        $matches.Add((Convert-DawnAutomationSummary $element $depth $matchScore))
    }
    if ($depth -ge $maxDepth) {
        continue
    }
    $children = New-Object System.Collections.Generic.List[object]
    $child = $walker.GetFirstChild($element)
    while ($child -ne $null) {
        $children.Add($child)
        $child = $walker.GetNextSibling($child)
    }
    for ($index = $children.Count - 1; $index -ge 0; $index -= 1) {
        $stack.Push([pscustomobject]@{ element = $children[$index]; depth = ($depth + 1) })
    }
}

$sortedMatches = @(
    $matches | Sort-Object -Property `
        @{ Expression = { $_.matchScore }; Descending = $true }, `
        @{ Expression = { if ($_.isOffscreen -eq $true) { 1 } else { 0 } }; Descending = $false }, `
        @{ Expression = { if ($_.isEnabled -eq $false) { 1 } else { 0 } }; Descending = $false }, `
        @{ Expression = { $_.depth }; Descending = $false }
) | Select-Object -First $matchLimit

[pscustomobject]@{
    visitedNodes = $visitedNodes
    matches = @($sortedMatches)
} | ConvertTo-Json -Depth 8 -Compress
"#;
    let stdout = run_windows_powershell_capture(
        script,
        &[
            ("DAWN_ACCESSIBILITY_HANDLE", window.handle.clone()),
            (
                "DAWN_ACCESSIBILITY_NAME",
                node_selector.name.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_AUTOMATION_ID",
                node_selector.automation_id.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_CLASS_NAME",
                node_selector.class_name.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_CONTROL_TYPE",
                node_selector.control_type.clone().unwrap_or_default(),
            ),
            (
                "DAWN_ACCESSIBILITY_MATCH_MODE",
                node_selector.match_mode.clone(),
            ),
            (
                "DAWN_ACCESSIBILITY_PREFER_VISIBLE",
                node_selector.prefer_visible.to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_PREFER_ENABLED",
                node_selector.prefer_enabled.to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_SEARCH_DEPTH",
                search_depth.max(1).to_string(),
            ),
            (
                "DAWN_ACCESSIBILITY_NODE_LIMIT",
                node_limit.max(1).to_string(),
            ),
            ("DAWN_ACCESSIBILITY_MATCH_LIMIT", limit.max(1).to_string()),
        ],
    )
    .await?;
    let result = serde_json::from_str(&stdout)
        .context("failed to parse desktop accessibility query JSON")?;
    Ok((window, result))
}

async fn wait_for_desktop_accessibility_node(
    title: Option<&str>,
    handle: Option<&str>,
    process_name: Option<&str>,
    node_selector: &DesktopAccessibilityNodeSelector,
    search_depth: usize,
    node_limit: usize,
    timeout_ms: u64,
    poll_ms: u64,
) -> anyhow::Result<(DesktopWindowEntry, DesktopAccessibilityQueryResult)> {
    let started = SystemTime::now();
    loop {
        let (window, query) = query_desktop_accessibility_nodes(
            title,
            handle,
            process_name,
            node_selector,
            search_depth,
            node_limit,
            1,
        )
        .await?;
        if !query.matches.is_empty() {
            return Ok((window, query));
        }
        if started.elapsed().unwrap_or_default() >= Duration::from_millis(timeout_ms.max(1)) {
            anyhow::bail!(
                "timed out after {} ms waiting for a desktop accessibility node matching the requested selector",
                timeout_ms
            );
        }
        tokio::time::sleep(Duration::from_millis(poll_ms.max(50))).await;
    }
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

fn desktop_accessibility_node_selector_from_payload(
    payload: &Value,
) -> DesktopAccessibilityNodeSelector {
    let mut selector = DesktopAccessibilityNodeSelector::default();
    selector.name = payload
        .get("name")
        .or_else(|| payload.get("label"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    selector.automation_id = payload
        .get("automationId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    selector.class_name = payload
        .get("className")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    selector.control_type = payload
        .get("controlType")
        .and_then(Value::as_str)
        .map(normalize_desktop_accessibility_control_type)
        .filter(|value| !value.is_empty());
    if let Some(match_mode) = payload
        .get("matchMode")
        .or_else(|| payload.get("nameMatchMode"))
        .and_then(Value::as_str)
    {
        selector.match_mode = normalize_desktop_accessibility_match_mode(match_mode);
    }
    if let Some(prefer_visible) = payload.get("preferVisible").and_then(Value::as_bool) {
        selector.prefer_visible = prefer_visible;
    }
    if let Some(prefer_enabled) = payload.get("preferEnabled").and_then(Value::as_bool) {
        selector.prefer_enabled = prefer_enabled;
    }
    selector
}

fn merge_desktop_accessibility_workflow_step_payload(
    workflow_payload: &Value,
    step: &Value,
) -> anyhow::Result<Value> {
    let Some(step_object) = step.as_object() else {
        anyhow::bail!("desktop_accessibility_workflow steps must be JSON objects");
    };
    let mut merged = step_object.clone();
    for key in [
        "title",
        "handle",
        "processName",
        "app",
        "target",
        "args",
        "name",
        "automationId",
        "className",
        "controlType",
        "matchMode",
        "nameMatchMode",
        "preferVisible",
        "preferEnabled",
        "depth",
        "nodeLimit",
        "timeoutMs",
        "pollMs",
        "delayMs",
        "clearExisting",
        "submit",
        "fallbackToType",
        "backend",
        "language",
        "lang",
        "x",
        "y",
        "width",
        "height",
        "keepImage",
        "button",
        "doubleClick",
        "elementIndex",
        "limit",
        "value",
        "text",
    ] {
        if !merged.contains_key(key) {
            if let Some(value) = workflow_payload.get(key) {
                merged.insert(key.to_string(), value.clone());
            }
        }
    }
    Ok(Value::Object(merged))
}

fn desktop_accessibility_workflow_step_kind(payload: &Value) -> anyhow::Result<String> {
    let kind = payload
        .get("kind")
        .or_else(|| payload.get("action"))
        .or_else(|| payload.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "desktop_accessibility_workflow steps require payload.kind, payload.action, or payload.type"
            )
        })?;
    Ok(kind.to_ascii_lowercase())
}

fn normalize_desktop_accessibility_control_type(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("ControlType.")
        .trim_start_matches("controltype.")
        .to_ascii_lowercase()
}

fn normalize_desktop_accessibility_match_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "exact" => "exact".to_string(),
        "starts_with" | "startswith" | "starts-with" | "prefix" => "starts_with".to_string(),
        _ => "contains".to_string(),
    }
}

fn desktop_accessibility_selector_has_predicate(
    selector: &DesktopAccessibilityNodeSelector,
) -> bool {
    selector.name.is_some()
        || selector.automation_id.is_some()
        || selector.class_name.is_some()
        || selector.control_type.is_some()
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

async fn resolve_desktop_ocr_backend(requested: Option<&str>) -> anyhow::Result<&'static str> {
    let backend = requested
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "auto".to_string());
    match backend.as_str() {
        "" | "auto" | "tesseract" => {
            if desktop_ocr_tesseract_available().await {
                Ok("tesseract")
            } else if backend == "auto" {
                anyhow::bail!(
                    "desktop_ocr could not find a local OCR backend. Install Tesseract and make `tesseract` available on PATH, or pass payload.imagePath to an external OCR pipeline."
                )
            } else {
                anyhow::bail!(
                    "desktop_ocr backend `tesseract` is not available on PATH. Install Tesseract or choose a different backend."
                )
            }
        }
        "windows_ocr" | "winrt" => anyhow::bail!(
            "desktop_ocr backend `{backend}` is not currently enabled for this unpackaged node runtime; use the Tesseract backend instead"
        ),
        other => {
            anyhow::bail!("unsupported desktop OCR backend `{other}`. Supported: auto, tesseract")
        }
    }
}

async fn desktop_ocr_tesseract_available() -> bool {
    Command::new("tesseract")
        .arg("--version")
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn run_tesseract_ocr(
    image_path: &PathBuf,
    language: Option<&str>,
) -> anyhow::Result<DesktopOcrResult> {
    let mut command = Command::new("tesseract");
    command
        .arg(image_path)
        .arg("stdout")
        .arg("--dpi")
        .arg("150");
    if let Some(language) = language {
        command.arg("-l").arg(language);
    }
    let output = command.output().await.with_context(|| {
        format!(
            "failed to launch Tesseract OCR for image {}",
            image_path.display()
        )
    })?;
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
        anyhow::bail!("desktop_ocr tesseract command failed: {detail}");
    }
    let text = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Ok(DesktopOcrResult {
        backend: "tesseract".to_string(),
        text: text.trim().to_string(),
        lines,
    })
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
    let node_profile = env::var("DAWN_NODE_PROFILE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .or_else(|| profile.node_profile.clone())
        .unwrap_or_else(|| "desktop".to_string());
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
    let claim_token = env::var("DAWN_NODE_CLAIM_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| profile.claim_token.clone());
    let allow_shell = resolve_allow_shell(&profile);
    let capabilities = resolve_node_capabilities(&profile, allow_shell, &node_profile);
    let enforce_trusted_rollout = env::var("DAWN_NODE_ENFORCE_TRUSTED_ROLLOUT")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    let require_signed_skills = env::var("DAWN_NODE_REQUIRE_SIGNED_SKILLS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
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
        node_profile,
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
        "headless_status".to_string(),
        "headless_observe".to_string(),
        "browser_start".to_string(),
        "browser_profiles".to_string(),
        "browser_profile_inspect".to_string(),
        "browser_profile_import".to_string(),
        "browser_profile_export".to_string(),
        "browser_profile_delete".to_string(),
        "browser_status".to_string(),
        "browser_stop".to_string(),
        "browser_navigate".to_string(),
        "browser_new_tab".to_string(),
        "browser_new_window".to_string(),
        "browser_extract".to_string(),
        "browser_click".to_string(),
        "browser_back".to_string(),
        "browser_forward".to_string(),
        "browser_reload".to_string(),
        "browser_focus".to_string(),
        "browser_close".to_string(),
        "browser_tabs".to_string(),
        "browser_snapshot".to_string(),
        "browser_screenshot".to_string(),
        "browser_pdf".to_string(),
        "browser_console_messages".to_string(),
        "browser_network_requests".to_string(),
        "browser_network_export".to_string(),
        "browser_trace".to_string(),
        "browser_trace_export".to_string(),
        "browser_errors".to_string(),
        "browser_errors_export".to_string(),
        "browser_cookies".to_string(),
        "browser_storage".to_string(),
        "browser_storage_set".to_string(),
        "browser_set_headers".to_string(),
        "browser_set_offline".to_string(),
        "browser_set_geolocation".to_string(),
        "browser_emulate_device".to_string(),
        "browser_evaluate".to_string(),
        "browser_wait_for".to_string(),
        "browser_handle_dialog".to_string(),
        "browser_press_key".to_string(),
        "browser_type".to_string(),
        "browser_upload".to_string(),
        "browser_download".to_string(),
        "browser_form_fill".to_string(),
        "browser_form_submit".to_string(),
        "browser_open".to_string(),
        "browser_search".to_string(),
        "desktop_open".to_string(),
        "system_lock".to_string(),
        "system_sleep".to_string(),
        "desktop_notification".to_string(),
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
        "desktop_ocr".to_string(),
        "desktop_accessibility_query".to_string(),
        "desktop_accessibility_click".to_string(),
        "desktop_accessibility_wait_for".to_string(),
        "desktop_accessibility_fill".to_string(),
        "desktop_accessibility_workflow".to_string(),
        "desktop_accessibility_snapshot".to_string(),
        "desktop_accessibility_focus".to_string(),
        "desktop_accessibility_invoke".to_string(),
        "desktop_accessibility_set_value".to_string(),
        "system_info".to_string(),
        "list_directory".to_string(),
        "read_file_preview".to_string(),
        "tail_file_preview".to_string(),
        "read_file_range".to_string(),
        "stat_path".to_string(),
        "find_paths".to_string(),
        "grep_files".to_string(),
        "process_snapshot".to_string(),
    ]
}

fn parse_capability_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn headless_default_capabilities() -> Vec<String> {
    vec![
        "echo".to_string(),
        "list_capabilities".to_string(),
        "agent_ping".to_string(),
        "headless_status".to_string(),
        "headless_observe".to_string(),
        "system_info".to_string(),
        "list_directory".to_string(),
        "read_file_preview".to_string(),
        "tail_file_preview".to_string(),
        "read_file_range".to_string(),
        "stat_path".to_string(),
        "find_paths".to_string(),
        "grep_files".to_string(),
        "process_snapshot".to_string(),
    ]
}

fn resolve_allow_shell(profile: &profile::DawnCliProfile) -> bool {
    env::var("DAWN_NODE_ALLOW_SHELL")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or_else(|_| {
            profile
                .requested_capabilities
                .iter()
                .any(|value| value == "shell_exec")
        })
}

fn default_capabilities_for_profile(profile_name: &str) -> Vec<String> {
    match profile_name {
        "headless" => headless_default_capabilities(),
        _ => default_capabilities(),
    }
}

fn resolve_node_capabilities(
    profile: &profile::DawnCliProfile,
    allow_shell: bool,
    node_profile: &str,
) -> Vec<String> {
    let capabilities = env::var("DAWN_NODE_CAPABILITIES")
        .map(|raw| parse_capability_list(&raw))
        .ok()
        .or_else(|| {
            if profile.requested_capabilities.is_empty() {
                None
            } else {
                Some(profile.requested_capabilities.clone())
            }
        })
        .unwrap_or_else(|| default_capabilities_for_profile(node_profile));
    normalize_capabilities(capabilities, allow_shell, node_profile)
}

fn normalize_capabilities(
    mut capabilities: Vec<String>,
    allow_shell: bool,
    node_profile: &str,
) -> Vec<String> {
    if allow_shell {
        if !capabilities.iter().any(|value| value == "shell_exec") {
            capabilities.push("shell_exec".to_string());
        }
    } else {
        capabilities.retain(|value| value != "shell_exec");
    }
    if node_profile == "headless" {
        let allowed = headless_default_capabilities();
        capabilities.retain(|value| allowed.iter().any(|allowed_value| allowed_value == value));
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn is_command_allowed_for_runtime_profile(node_profile: &str, command_type: &str) -> bool {
    if node_profile != "headless" {
        return true;
    }
    matches!(
        command_type,
        "echo"
            | "list_capabilities"
            | "agent_ping"
            | "headless_status"
            | "headless_observe"
            | "system_info"
            | "list_directory"
            | "read_file_preview"
            | "tail_file_preview"
            | "read_file_range"
            | "stat_path"
            | "find_paths"
            | "grep_files"
            | "process_snapshot"
    )
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
            node_profile: "desktop".to_string(),
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

    #[test]
    fn execute_command_future_stays_small_enough_for_background_hosts() {
        let config = base_config();
        let mut runtime_state = NodeRuntimeState::default();
        let future = execute_command(
            &config,
            &mut runtime_state,
            GatewayCommandEnvelope {
                command_id: "cmd-test".to_string(),
                command_type: "system_info".to_string(),
                payload: json!({}),
            },
        );
        let future_size = std::mem::size_of_val(&future);
        assert!(
            future_size <= 2048,
            "execute_command future grew too large for background-host safety: {future_size} bytes"
        );
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
    async fn headless_status_command_reports_headless_profile() {
        let config = base_config();
        let response = execute_headless_status_command(
            &config,
            GatewayCommandEnvelope {
                command_id: "cmd-headless-status".to_string(),
                command_type: "headless_status".to_string(),
                payload: json!({}),
            },
        )
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["runtimeProfile"], "headless");
        assert_eq!(result["interactiveDesktop"], false);
        assert_eq!(result["runtimePolicy"]["mode"], "read_only_observe");
        assert_eq!(result["summary"]["recommendedCommand"], "headless_observe");
    }

    #[tokio::test]
    async fn headless_observe_command_collects_process_and_directory_views() {
        let config = base_config();
        let response = execute_headless_observe_command(
            &config,
            GatewayCommandEnvelope {
                command_id: "cmd-headless-observe".to_string(),
                command_type: "headless_observe".to_string(),
                payload: json!({
                    "processLimit": 5,
                    "directoryLimit": 5,
                    "path": "."
                }),
            },
        )
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["runtimeProfile"], "headless");
        assert!(result.get("system").is_some());
        assert!(result.get("processSnapshot").is_some());
        assert!(result.get("directory").is_some());
        assert_eq!(result["summary"]["mode"], "read_only_observe");
        assert_eq!(result["summary"]["directoryPath"], ".");
        assert!(result["summary"].get("topProcess").is_some());
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
    async fn tail_file_preview_command_reads_tail_bytes() {
        let temp_path = std::env::temp_dir().join(format!(
            "dawn-node-tail-preview-{}.txt",
            unix_timestamp_ms()
        ));
        fs::write(&temp_path, "alpha\nbeta\ngamma\ndelta").unwrap();

        let response = execute_tail_file_preview_command(GatewayCommandEnvelope {
            command_id: "cmd-tail-preview".to_string(),
            command_type: "tail_file_preview".to_string(),
            payload: json!({
                "path": temp_path.display().to_string(),
                "maxBytes": 8
            }),
        })
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["truncated"], true);
        assert_eq!(result["previewBytes"], 8);
        assert!(
            result["preview"]
                .as_str()
                .unwrap_or_default()
                .contains("delta")
        );

        fs::remove_file(temp_path).ok();
    }

    #[tokio::test]
    async fn read_file_range_command_reads_requested_slice() {
        let temp_path = std::env::temp_dir().join(format!(
            "dawn-node-range-preview-{}.txt",
            unix_timestamp_ms()
        ));
        fs::write(&temp_path, "alpha\nbeta\ngamma\ndelta").unwrap();

        let response = execute_read_file_range_command(GatewayCommandEnvelope {
            command_id: "cmd-range-preview".to_string(),
            command_type: "read_file_range".to_string(),
            payload: json!({
                "path": temp_path.display().to_string(),
                "startByte": 6,
                "maxBytes": 5
            }),
        })
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["startByte"], 6);
        assert_eq!(result["previewBytes"], 5);
        assert_eq!(result["preview"], "beta\n");
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

    #[tokio::test]
    async fn find_paths_command_returns_matches_with_summary() {
        let temp_dir =
            std::env::temp_dir().join(format!("dawn-node-find-paths-{}", unix_timestamp_ms()));
        let nested_dir = temp_dir.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(temp_dir.join("alpha-demo.txt"), "alpha").unwrap();
        fs::write(nested_dir.join("report-demo.log"), "report").unwrap();
        fs::write(nested_dir.join("ignore.bin"), "bin").unwrap();

        let response = execute_find_paths_command(GatewayCommandEnvelope {
            command_id: "cmd-find-paths".to_string(),
            command_type: "find_paths".to_string(),
            payload: json!({
                "path": temp_dir.display().to_string(),
                "query": "demo",
                "limit": 10,
                "maxDepth": 4
            }),
        })
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["query"], "demo");
        assert_eq!(result["count"], 2);
        assert_eq!(result["summary"]["matchCount"], 2);
        assert!(result["summary"]["firstMatch"].as_str().is_some());

        fs::remove_dir_all(temp_dir).ok();
    }

    #[tokio::test]
    async fn grep_files_command_returns_matches_with_preview() {
        let temp_dir =
            std::env::temp_dir().join(format!("dawn-node-grep-files-{}", unix_timestamp_ms()));
        let nested_dir = temp_dir.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(temp_dir.join("alpha.txt"), "hello world\nsecond line").unwrap();
        fs::write(nested_dir.join("report.txt"), "TODO: investigate issue").unwrap();
        fs::write(nested_dir.join("ignore.bin"), "binary").unwrap();

        let response = execute_grep_files_command(GatewayCommandEnvelope {
            command_id: "cmd-grep-files".to_string(),
            command_type: "grep_files".to_string(),
            payload: json!({
                "path": temp_dir.display().to_string(),
                "query": "todo",
                "limit": 10,
                "maxDepth": 4,
                "caseSensitive": false
            }),
        })
        .await;

        assert_eq!(response.status, "succeeded");
        let result = response.result.unwrap();
        assert_eq!(result["query"], "todo");
        assert_eq!(result["count"], 1);
        assert_eq!(result["summary"]["matchCount"], 1);
        assert!(
            result["matches"][0]["preview"]
                .as_str()
                .unwrap_or_default()
                .contains("TODO")
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
            forward_history: Vec::new(),
            pending_form_selector: Some("@form-index:0".to_string()),
            pending_form_fields: BTreeMap::from([(String::from("q"), String::from("openclaw"))]),
            pending_form_uploads: BTreeMap::from([(
                String::from("attachment"),
                String::from("C:/tmp/demo.txt"),
            )]),
            managed: None,
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
                forward_history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
                managed: None,
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
                forward_history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
                managed: None,
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
    fn derives_browser_back_and_forward_transitions() {
        let session = BrowserSession {
            client: new_browser_client().unwrap(),
            current_url: "https://example.com/c".to_string(),
            page_html: "<html></html>".to_string(),
            title: Some("C".to_string()),
            status_code: 200,
            content_type: Some("text/html".to_string()),
            last_action: "navigate".to_string(),
            loaded_at_unix_ms: 30,
            history: vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ],
            forward_history: vec!["https://example.com/d".to_string()],
            pending_form_selector: None,
            pending_form_fields: BTreeMap::new(),
            pending_form_uploads: BTreeMap::new(),
            managed: None,
        };
        let (back_url, back_history, back_forward) = browser_back_transition(&session).unwrap();
        assert_eq!(back_url, "https://example.com/b");
        assert_eq!(back_history, vec!["https://example.com/a".to_string()]);
        assert_eq!(
            back_forward,
            vec![
                "https://example.com/d".to_string(),
                "https://example.com/c".to_string()
            ]
        );

        let (forward_url, forward_history, forward_stack) =
            browser_forward_transition(&session).unwrap();
        assert_eq!(forward_url, "https://example.com/d");
        assert_eq!(
            forward_history,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
                "https://example.com/c".to_string()
            ]
        );
        assert!(forward_stack.is_empty());
    }

    #[test]
    fn allocates_unique_browser_tab_session_ids() {
        let sessions = HashMap::from([
            (
                "browser-default".to_string(),
                BrowserSession {
                    client: new_browser_client().unwrap(),
                    current_url: "https://example.com".to_string(),
                    page_html: "<html></html>".to_string(),
                    title: None,
                    status_code: 200,
                    content_type: Some("text/html".to_string()),
                    last_action: "navigate".to_string(),
                    loaded_at_unix_ms: 1,
                    history: Vec::new(),
                    forward_history: Vec::new(),
                    pending_form_selector: None,
                    pending_form_fields: BTreeMap::new(),
                    pending_form_uploads: BTreeMap::new(),
                    managed: None,
                },
            ),
            (
                "browser-default-tab-1".to_string(),
                BrowserSession {
                    client: new_browser_client().unwrap(),
                    current_url: "https://example.com/one".to_string(),
                    page_html: "<html></html>".to_string(),
                    title: None,
                    status_code: 200,
                    content_type: Some("text/html".to_string()),
                    last_action: "navigate".to_string(),
                    loaded_at_unix_ms: 2,
                    history: Vec::new(),
                    forward_history: Vec::new(),
                    pending_form_selector: None,
                    pending_form_fields: BTreeMap::new(),
                    pending_form_uploads: BTreeMap::new(),
                    managed: None,
                },
            ),
        ]);
        let next = resolve_new_browser_session_id(&sessions, "browser-default", "tab", &json!({}))
            .unwrap();
        assert_eq!(next, "browser-default-tab-2");
    }

    #[test]
    fn allocates_unique_browser_window_session_ids() {
        let sessions = HashMap::from([(
            "browser-default".to_string(),
            BrowserSession {
                client: new_browser_client().unwrap(),
                current_url: "https://example.com".to_string(),
                page_html: "<html></html>".to_string(),
                title: None,
                status_code: 200,
                content_type: Some("text/html".to_string()),
                last_action: "navigate".to_string(),
                loaded_at_unix_ms: 1,
                history: Vec::new(),
                forward_history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
                managed: None,
            },
        )]);
        let next =
            resolve_new_browser_session_id(&sessions, "browser-default", "window", &json!({}))
                .unwrap();
        assert_eq!(next, "browser-default-window-1");
    }

    #[test]
    fn browser_start_session_id_prefers_default_when_available() {
        let sessions = HashMap::new();
        let next = resolve_browser_start_session_id(&sessions, &json!({})).unwrap();
        assert_eq!(next, "browser-default");
    }

    #[test]
    fn browser_start_session_id_allocates_managed_suffix_when_default_is_taken() {
        let sessions = HashMap::from([(
            "browser-default".to_string(),
            BrowserSession {
                client: new_browser_client().unwrap(),
                current_url: "https://example.com".to_string(),
                page_html: "<html></html>".to_string(),
                title: None,
                status_code: 200,
                content_type: Some("text/html".to_string()),
                last_action: "navigate".to_string(),
                loaded_at_unix_ms: 1,
                history: Vec::new(),
                forward_history: Vec::new(),
                pending_form_selector: None,
                pending_form_fields: BTreeMap::new(),
                pending_form_uploads: BTreeMap::new(),
                managed: None,
            },
        )]);
        let next = resolve_browser_start_session_id(&sessions, &json!({})).unwrap();
        assert_eq!(next, "browser-managed-1");
    }

    #[test]
    fn detects_shared_managed_browser_processes() {
        let left = ManagedBrowserSession {
            backend: "edge".to_string(),
            executable: "msedge.exe".to_string(),
            debug_port: 9222,
            browser_pid: Some(111),
            profile_name: Some("ops".to_string()),
            persistent_profile: true,
            target_id: "page-a".to_string(),
            websocket_url: "ws://127.0.0.1/devtools/page/a".to_string(),
            user_data_dir: PathBuf::from("C:/tmp/dawn-browser"),
        };
        let right = ManagedBrowserSession {
            backend: "edge".to_string(),
            executable: "msedge.exe".to_string(),
            debug_port: 9222,
            browser_pid: Some(111),
            profile_name: Some("ops".to_string()),
            persistent_profile: true,
            target_id: "page-b".to_string(),
            websocket_url: "ws://127.0.0.1/devtools/page/b".to_string(),
            user_data_dir: PathBuf::from("C:/tmp/dawn-browser"),
        };
        assert!(managed_browser_sessions_share_process(&left, &right));
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
    fn detects_managed_browser_payload_flags() {
        assert!(payload_requests_managed_browser(
            &json!({ "managed": true })
        ));
        assert!(payload_requests_managed_browser(
            &json!({ "runtime": "managed" })
        ));
        assert!(!payload_requests_managed_browser(
            &json!({ "managed": false })
        ));
    }

    #[test]
    fn resolves_browser_screenshot_path_with_png_extension() {
        let path =
            resolve_browser_screenshot_path(Some("artifacts/browser/demo"), "browser-default");
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("png")
        );
    }

    #[test]
    fn resolves_browser_pdf_path_with_pdf_extension() {
        let path = resolve_browser_pdf_path(Some("artifacts/browser/demo"), "browser-default");
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("pdf")
        );
    }

    #[test]
    fn resolves_browser_network_export_path_with_json_extension() {
        let path = resolve_browser_network_export_path(
            Some("artifacts/browser/network-log"),
            "browser-default",
        );
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("json")
        );
    }

    #[test]
    fn resolves_browser_errors_export_path_with_json_extension() {
        let path = resolve_browser_errors_export_path(
            Some("artifacts/browser/errors-log"),
            "browser-default",
        );
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("json")
        );
    }

    #[test]
    fn resolves_browser_trace_path_with_json_extension() {
        let path =
            resolve_browser_trace_path(Some("artifacts/browser/demo-trace"), "browser-default");
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("json")
        );
    }

    #[test]
    fn resolves_browser_profile_export_path_to_named_directory() {
        let path = resolve_browser_profile_export_path(Some("artifacts/browser/exports/"), "ops");
        assert_eq!(
            path.file_name().and_then(|value| value.to_str()),
            Some("ops")
        );
    }

    #[test]
    fn normalizes_browser_storage_area_aliases() {
        assert_eq!(
            normalize_browser_storage_area("local"),
            Some("localStorage")
        );
        assert_eq!(
            normalize_browser_storage_area("local_storage"),
            Some("localStorage")
        );
        assert_eq!(
            normalize_browser_storage_area("session"),
            Some("sessionStorage")
        );
        assert_eq!(
            normalize_browser_storage_area("sessionStorage"),
            Some("sessionStorage")
        );
        assert_eq!(normalize_browser_storage_area("indexedDb"), None);
    }

    #[test]
    fn resolves_browser_device_emulation_presets() {
        let mobile = resolve_browser_device_emulation_request(&json!({
            "preset": "iphone-13",
            "reload": true
        }))
        .unwrap();
        assert_eq!(mobile.width, 390);
        assert!(mobile.mobile);
        assert!(mobile.touch);
        assert!(mobile.reload);

        let desktop = resolve_browser_device_emulation_request(&json!({
            "preset": "desktop",
            "width": 1600
        }))
        .unwrap();
        assert_eq!(desktop.width, 1600);
        assert!(!desktop.mobile);

        let error = resolve_browser_device_emulation_request(&json!({
            "preset": "gameboy"
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("unsupported browser_emulate_device preset"));
    }

    #[test]
    fn default_capabilities_include_browser_session_commands() {
        let capabilities = default_capabilities();
        assert!(capabilities.iter().any(|value| value == "headless_status"));
        assert!(capabilities.iter().any(|value| value == "headless_observe"));
        assert!(capabilities.iter().any(|value| value == "browser_start"));
        assert!(capabilities.iter().any(|value| value == "browser_profiles"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_profile_inspect")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_profile_import")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_profile_export")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_profile_delete")
        );
        assert!(capabilities.iter().any(|value| value == "browser_status"));
        assert!(capabilities.iter().any(|value| value == "browser_stop"));
        assert!(capabilities.iter().any(|value| value == "browser_navigate"));
        assert!(capabilities.iter().any(|value| value == "browser_new_tab"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_new_window")
        );
        assert!(capabilities.iter().any(|value| value == "browser_extract"));
        assert!(capabilities.iter().any(|value| value == "browser_click"));
        assert!(capabilities.iter().any(|value| value == "browser_back"));
        assert!(capabilities.iter().any(|value| value == "browser_forward"));
        assert!(capabilities.iter().any(|value| value == "browser_reload"));
        assert!(capabilities.iter().any(|value| value == "browser_focus"));
        assert!(capabilities.iter().any(|value| value == "browser_close"));
        assert!(capabilities.iter().any(|value| value == "browser_tabs"));
        assert!(capabilities.iter().any(|value| value == "browser_snapshot"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_screenshot")
        );
        assert!(capabilities.iter().any(|value| value == "browser_pdf"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_console_messages")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_network_requests")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_network_export")
        );
        assert!(capabilities.iter().any(|value| value == "browser_trace"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_trace_export")
        );
        assert!(capabilities.iter().any(|value| value == "browser_errors"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_errors_export")
        );
        assert!(capabilities.iter().any(|value| value == "browser_cookies"));
        assert!(capabilities.iter().any(|value| value == "browser_storage"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_storage_set")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_set_headers")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_set_offline")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_set_geolocation")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_emulate_device")
        );
        assert!(capabilities.iter().any(|value| value == "browser_evaluate"));
        assert!(capabilities.iter().any(|value| value == "browser_wait_for"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_handle_dialog")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "browser_press_key")
        );
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
                .any(|value| value == "desktop_notification")
        );
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
        assert!(capabilities.iter().any(|value| value == "desktop_ocr"));
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_query")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_click")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_wait_for")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_fill")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_workflow")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_snapshot")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_focus")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_invoke")
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_accessibility_set_value")
        );
    }

    #[test]
    fn normalizes_desktop_notification_urgency_variants() {
        assert_eq!(normalize_desktop_notification_urgency("warning"), "warning");
        assert_eq!(normalize_desktop_notification_urgency("critical"), "error");
        assert_eq!(
            normalize_desktop_notification_urgency("anything-else"),
            "info"
        );
    }

    #[test]
    fn encodes_windows_send_keys_text_reserved_characters() {
        let encoded = encode_windows_send_keys_text("+^%~()[]{}\n\t");
        assert_eq!(encoded, "{+}{^}{%}{~}{(}{)}{[}{]}{{}{}}{ENTER}{TAB}");
    }

    #[test]
    fn normalizes_desktop_accessibility_control_type_aliases() {
        assert_eq!(
            normalize_desktop_accessibility_control_type("ControlType.Edit"),
            "edit"
        );
        assert_eq!(
            normalize_desktop_accessibility_control_type("button"),
            "button"
        );
    }

    #[test]
    fn normalizes_desktop_accessibility_match_mode_aliases() {
        assert_eq!(
            normalize_desktop_accessibility_match_mode("starts-with"),
            "starts_with"
        );
        assert_eq!(normalize_desktop_accessibility_match_mode("exact"), "exact");
        assert_eq!(
            normalize_desktop_accessibility_match_mode("whatever"),
            "contains"
        );
    }

    #[test]
    fn parses_desktop_accessibility_selector_ranking_preferences() {
        let selector = desktop_accessibility_node_selector_from_payload(&json!({
            "name": "Save",
            "className": "Button",
            "controlType": "ControlType.Button",
            "matchMode": "starts-with",
            "preferVisible": false,
            "preferEnabled": true
        }));
        assert_eq!(selector.name.as_deref(), Some("Save"));
        assert_eq!(selector.class_name.as_deref(), Some("Button"));
        assert_eq!(selector.control_type.as_deref(), Some("button"));
        assert_eq!(selector.match_mode, "starts_with");
        assert!(!selector.prefer_visible);
        assert!(selector.prefer_enabled);
        assert!(desktop_accessibility_selector_has_predicate(&selector));
    }

    #[test]
    fn merges_desktop_accessibility_workflow_step_payload_with_defaults() {
        let workflow_payload = json!({
            "processName": "notepad",
            "depth": 4,
            "className": "RichEditD2DPT",
            "matchMode": "exact",
            "preferVisible": true,
            "clearExisting": true
        });
        let step = json!({
            "kind": "fill",
            "controlType": "edit",
            "value": "hello"
        });
        let merged = merge_desktop_accessibility_workflow_step_payload(&workflow_payload, &step)
            .expect("expected workflow step payload to merge");
        assert_eq!(
            merged.get("processName").and_then(Value::as_str),
            Some("notepad")
        );
        assert_eq!(merged.get("depth").and_then(Value::as_u64), Some(4));
        assert_eq!(
            merged.get("className").and_then(Value::as_str),
            Some("RichEditD2DPT")
        );
        assert_eq!(
            merged.get("matchMode").and_then(Value::as_str),
            Some("exact")
        );
        assert_eq!(
            merged.get("preferVisible").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            merged.get("clearExisting").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(merged.get("kind").and_then(Value::as_str), Some("fill"));
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

    #[test]
    fn resolves_allow_shell_from_profile_requested_capabilities() {
        let profile = profile::DawnCliProfile {
            requested_capabilities: vec!["shell_exec".to_string(), "system_info".to_string()],
            ..Default::default()
        };

        assert!(resolve_allow_shell(&profile));
    }

    #[test]
    fn resolves_node_capabilities_from_profile_when_env_is_absent() {
        let profile = profile::DawnCliProfile {
            requested_capabilities: vec![
                "desktop_notification".to_string(),
                "system_info".to_string(),
            ],
            ..Default::default()
        };

        let capabilities = resolve_node_capabilities(&profile, false, "desktop");

        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_notification")
        );
        assert!(capabilities.iter().any(|value| value == "system_info"));
        assert!(!capabilities.iter().any(|value| value == "shell_exec"));
    }

    #[test]
    fn resolves_headless_defaults_when_profile_requests_none() {
        let profile = profile::DawnCliProfile {
            node_profile: Some("headless".to_string()),
            ..Default::default()
        };

        let capabilities = resolve_node_capabilities(&profile, false, "headless");

        assert!(capabilities.iter().any(|value| value == "headless_status"));
        assert!(capabilities.iter().any(|value| value == "headless_observe"));
        assert!(capabilities.iter().any(|value| value == "process_snapshot"));
        assert!(!capabilities.iter().any(|value| value == "browser_start"));
        assert!(
            !capabilities
                .iter()
                .any(|value| value == "desktop_notification")
        );
    }

    #[tokio::test]
    async fn headless_runtime_rejects_interactive_commands() {
        let mut config = base_config();
        config.node_profile = "headless".to_string();
        config.capabilities = headless_default_capabilities();
        let mut runtime_state = NodeRuntimeState::default();

        let response = dispatch_command_future(
            &config,
            &mut runtime_state,
            GatewayCommandEnvelope {
                command_id: "cmd-headless-browser-open".to_string(),
                command_type: "browser_open".to_string(),
                payload: json!({"url": "https://example.com"}),
            },
        )
        .await;

        assert_eq!(response.status, "failed");
        assert!(
            response
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("runtime profile `headless` does not allow command type `browser_open`")
        );
    }
}
