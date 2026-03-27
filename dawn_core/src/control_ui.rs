use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use anyhow::Context;
use axum::http::StatusCode;
use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::{
    a2a::{self, Task},
    agent_cards::{self, InvokeAgentCardRequest},
    app_state::{AppState, ChatChannelIdentityRecord, ChatChannelIdentityStatus, NodeRecord},
    chat_ingress, control_plane, identity, skill_registry,
};

const CONTROL_UI_HTML: &str = include_str!("../../templates/frontend/control_ui.html");

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(page))
        .route("/command", post(submit_command))
        .route("/ws", get(workbench_ws))
}

async fn page() -> Html<&'static str> {
    Html(CONTROL_UI_HTML)
}

async fn workbench_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_workbench_ws(socket, state))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchCommandRequest {
    session_token: String,
    platform: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    text: String,
    route_to_task: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchTaskRequest {
    name: String,
    instruction: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchTaskInspectRequest {
    task_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchTaskStreamRequest {
    task_id: String,
    after: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchDelegateRequest {
    card_id: String,
    name: Option<String>,
    instruction: String,
    await_completion: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSkillRequest {
    session_token: String,
    skill_id: String,
    function: Option<String>,
    platform: Option<String>,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSkillInspectRequest {
    skill_id: String,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchLogsRequest {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSessionListRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSessionInspectRequest {
    session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchSessionRevokeRequest {
    session_token: String,
    session_id: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchConfigGetRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchChannelStatusRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchNodeStatusRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchNodeObserveRequest {
    session_token: String,
    node_id: String,
    command_type: String,
    payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchConfigApplyRequest {
    session_token: String,
    tenant_id: String,
    project_id: String,
    display_name: String,
    region: String,
    default_model_providers: Vec<String>,
    default_chat_platforms: Vec<String>,
    onboarding_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchRpcRequest {
    id: Option<String>,
    method: String,
    params: Option<Value>,
}

async fn submit_command(
    State(state): State<Arc<AppState>>,
    Json(request): Json<WorkbenchCommandRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    submit_command_inner(state, request)
        .await
        .map(Json)
        .map_err(service_error)
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": error.to_string(),
        })),
    )
}

async fn handle_workbench_ws(mut socket: WebSocket, state: Arc<AppState>) {
    let _ = socket
        .send(Message::Text(
            json!({
                "kind": "ready",
                "transport": "websocket",
                "detail": "gateway workbench websocket connected",
                "methods": [
                    "dashboard.refresh",
                    "command.run",
                    "skill.run",
                    "skill.inspect",
                    "config.get",
                    "channel.status",
                    "node.status",
                    "node.observe",
                    "config.apply",
                    "logs.tail",
                    "session.list",
                    "session.inspect",
                    "session.revoke",
                    "task.create",
                    "task.inspect",
                    "task.stream",
                    "delegate.invoke",
                    "ping"
                ]
            })
            .to_string()
            .into(),
        ))
        .await;

    let mut receiver = state.subscribe_console_events();

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Text(text))) => {
                        let trimmed = text.trim();
                        if trimmed.eq_ignore_ascii_case("refresh") {
                            let payload = json!({
                                "kind": "refresh_requested",
                                "detail": "client requested dashboard refresh"
                            });
                            let _ = socket.send(Message::Text(payload.to_string().into())).await;
                            continue;
                        }
                        match serde_json::from_str::<WorkbenchRpcRequest>(trimmed) {
                            Ok(request) => {
                                let id = request.id.clone();
                                let method = request.method.clone();
                                let payload = match handle_workbench_rpc(state.clone(), request).await {
                                    Ok(result) => json!({
                                        "kind": "rpc_result",
                                        "id": id,
                                        "method": method,
                                        "result": result,
                                    }),
                                    Err(error) => json!({
                                        "kind": "rpc_error",
                                        "id": id,
                                        "method": method,
                                        "error": error.to_string(),
                                    }),
                                };
                                let _ = socket.send(Message::Text(payload.to_string().into())).await;
                            }
                            Err(error) => {
                                let _ = socket.send(Message::Text(
                                    json!({
                                        "kind": "rpc_error",
                                        "method": "unknown",
                                        "error": format!("invalid websocket payload: {error}"),
                                    }).to_string().into()
                                )).await;
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            event = receiver.recv() => {
                let payload = match event {
                    Ok(event) => json!({
                        "kind": "console_update",
                        "event": event,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => json!({
                        "kind": "lagged",
                        "skipped": skipped,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if socket.send(Message::Text(payload.to_string().into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn handle_workbench_rpc(
    state: Arc<AppState>,
    request: WorkbenchRpcRequest,
) -> anyhow::Result<Value> {
    match request.method.as_str() {
        "dashboard.refresh" => Ok(json!({
            "detail": "client requested dashboard refresh"
        })),
        "ping" => Ok(json!({
            "ok": true,
            "transport": "websocket"
        })),
        "command.run" => {
            let params: WorkbenchCommandRequest = parse_rpc_params(request.params)?;
            submit_command_inner(state, params).await
        }
        "skill.run" => {
            let params: WorkbenchSkillRequest = parse_rpc_params(request.params)?;
            run_skill_inner(state, params).await
        }
        "skill.inspect" => {
            let params: WorkbenchSkillInspectRequest = parse_rpc_params(request.params)?;
            inspect_skill_inner(state, params).await
        }
        "config.get" => {
            let _params: WorkbenchConfigGetRequest = parse_rpc_params(request.params)?;
            get_config_inner(state).await
        }
        "channel.status" => {
            let _params: WorkbenchChannelStatusRequest = parse_rpc_params(request.params)?;
            channel_status_inner(state).await
        }
        "node.status" => {
            let _params: WorkbenchNodeStatusRequest = parse_rpc_params(request.params)?;
            node_status_inner(state).await
        }
        "node.observe" => {
            let params: WorkbenchNodeObserveRequest = parse_rpc_params(request.params)?;
            node_observe_inner(state, params).await
        }
        "config.apply" => {
            let params: WorkbenchConfigApplyRequest = parse_rpc_params(request.params)?;
            apply_config_inner(state, params).await
        }
        "logs.tail" => {
            let params: WorkbenchLogsRequest = parse_rpc_params(request.params)?;
            tail_logs_inner(state, params).await
        }
        "session.list" => {
            let _params: WorkbenchSessionListRequest = parse_rpc_params(request.params)?;
            list_sessions_inner(state).await
        }
        "session.inspect" => {
            let params: WorkbenchSessionInspectRequest = parse_rpc_params(request.params)?;
            inspect_session_inner(state, params).await
        }
        "session.revoke" => {
            let params: WorkbenchSessionRevokeRequest = parse_rpc_params(request.params)?;
            revoke_session_inner(state, params).await
        }
        "task.create" => {
            let params: WorkbenchTaskRequest = parse_rpc_params(request.params)?;
            create_task_inner(state, params).await
        }
        "task.inspect" => {
            let params: WorkbenchTaskInspectRequest = parse_rpc_params(request.params)?;
            inspect_task_inner(state, params).await
        }
        "task.stream" => {
            let params: WorkbenchTaskStreamRequest = parse_rpc_params(request.params)?;
            inspect_task_stream_inner(state, params).await
        }
        "delegate.invoke" => {
            let params: WorkbenchDelegateRequest = parse_rpc_params(request.params)?;
            invoke_delegate_inner(state, params).await
        }
        other => anyhow::bail!("unsupported workbench websocket method: {other}"),
    }
}

fn parse_rpc_params<T: DeserializeOwned>(params: Option<Value>) -> anyhow::Result<T> {
    serde_json::from_value(params.unwrap_or_else(|| json!({})))
        .context("invalid websocket method params")
}

async fn submit_command_inner(
    state: Arc<AppState>,
    request: WorkbenchCommandRequest,
) -> anyhow::Result<Value> {
    let session = identity::resolve_session_by_token(&state, &request.session_token).await?;
    let platform = request.platform.trim().to_ascii_lowercase();
    let text = request.text.trim().to_string();
    if platform.is_empty() {
        anyhow::bail!("platform is required");
    }
    if text.is_empty() {
        anyhow::bail!("text is required");
    }
    let record = chat_ingress::simulate_ingress_message(
        state,
        &platform,
        "control_ui.command".to_string(),
        request.chat_id,
        request.sender_id,
        request
            .sender_display
            .or_else(|| Some(session.operator_name.clone())),
        text.clone(),
        json!({
            "source": "control_ui",
            "platform": platform,
            "text": text,
            "actor": session.operator_name,
        }),
        request.route_to_task.unwrap_or(true),
    )
    .await?;
    Ok(json!({
        "record": record,
        "actor": session.operator_name,
    }))
}

async fn run_skill_inner(
    state: Arc<AppState>,
    request: WorkbenchSkillRequest,
) -> anyhow::Result<Value> {
    let skill_id = request.skill_id.trim();
    if skill_id.is_empty() {
        anyhow::bail!("skillId is required");
    }
    let selector = request
        .function
        .map(|function| format!("{skill_id}#{function}"))
        .unwrap_or_else(|| skill_id.to_string());
    submit_command_inner(
        state,
        WorkbenchCommandRequest {
            session_token: request.session_token,
            platform: request.platform.unwrap_or_else(|| "app".to_string()),
            chat_id: request.chat_id,
            sender_id: request.sender_id,
            sender_display: request.sender_display,
            text: format!("/skill {selector}"),
            route_to_task: Some(true),
        },
    )
    .await
}

async fn inspect_skill_inner(
    state: Arc<AppState>,
    request: WorkbenchSkillInspectRequest,
) -> anyhow::Result<Value> {
    let skill_id = request.skill_id.trim();
    if skill_id.is_empty() {
        anyhow::bail!("skillId is required");
    }
    let skill = skill_registry::find_skill(&state, skill_id, request.version.as_deref())
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown skill: {skill_id}"))?;
    let native_usage = skill_registry::native_builtin_skill_usage(skill_id);
    Ok(json!({
        "skill": skill,
        "nativeUsage": native_usage,
        "nativeBuiltin": skill_registry::is_native_builtin_skill(&skill),
    }))
}

async fn apply_config_inner(
    state: Arc<AppState>,
    request: WorkbenchConfigApplyRequest,
) -> anyhow::Result<Value> {
    let response = identity::apply_workspace_update(
        &state,
        identity::WorkspaceProfileUpdateRequest {
            session_token: request.session_token,
            tenant_id: request.tenant_id,
            project_id: request.project_id,
            display_name: request.display_name,
            region: request.region,
            default_model_providers: request.default_model_providers,
            default_chat_platforms: request.default_chat_platforms,
            onboarding_status: request.onboarding_status,
        },
    )
    .await?;
    Ok(json!(response))
}

async fn get_config_inner(state: Arc<AppState>) -> anyhow::Result<Value> {
    let workspace = identity::ensure_workspace_profile(&state).await?;
    Ok(json!({
        "workspace": workspace
    }))
}

async fn channel_status_inner(state: Arc<AppState>) -> anyhow::Result<Value> {
    let workspace = identity::ensure_workspace_profile(&state).await?;
    let identities = state.list_chat_channel_identities(None, None).await?;
    let mut identities_by_platform: BTreeMap<String, Vec<ChatChannelIdentityRecord>> =
        BTreeMap::new();
    for identity in identities {
        identities_by_platform
            .entry(identity.platform.clone())
            .or_default()
            .push(identity);
    }
    for platform in &workspace.default_chat_platforms {
        identities_by_platform.entry(platform.clone()).or_default();
    }

    let channels = identities_by_platform
        .into_iter()
        .map(|(platform, identities)| {
            summarize_channel_status(&workspace.default_chat_platforms, &platform, identities)
        })
        .collect::<Vec<_>>();

    let paired_channels = channels
        .iter()
        .filter(|channel| channel["pairedCount"].as_u64().unwrap_or(0) > 0)
        .count();
    let pending_pairings = channels
        .iter()
        .map(|channel| channel["pendingCount"].as_u64().unwrap_or(0) as usize)
        .sum::<usize>();
    let default_channels = channels
        .iter()
        .filter(|channel| channel["isDefault"] == json!(true))
        .count();

    Ok(json!({
        "workspace": {
            "displayName": workspace.display_name,
            "defaultChatPlatforms": workspace.default_chat_platforms,
        },
        "channels": channels,
        "summary": {
            "channelCount": channels.len(),
            "defaultChannelCount": default_channels,
            "pairedChannelCount": paired_channels,
            "pendingPairingCount": pending_pairings,
        }
    }))
}

async fn node_status_inner(state: Arc<AppState>) -> anyhow::Result<Value> {
    let nodes = state.list_nodes().await?;
    let connected_count = nodes.iter().filter(|node| node.connected).count();
    let mapped = nodes
        .into_iter()
        .map(workbench_node_status)
        .collect::<Vec<_>>();
    let total_nodes = mapped.len();
    let headless_nodes = mapped
        .iter()
        .filter(|node| node["runtimeMode"] == "headless / read-only observability")
        .count();
    let desktop_nodes = mapped
        .iter()
        .filter(|node| node["runtimeMode"] == "desktop / interactive control")
        .count();
    Ok(json!({
        "nodes": mapped,
        "summary": {
            "totalNodes": total_nodes,
            "connectedNodes": connected_count,
            "headlessNodes": headless_nodes,
            "desktopNodes": desktop_nodes,
        }
    }))
}

async fn node_observe_inner(
    state: Arc<AppState>,
    request: WorkbenchNodeObserveRequest,
) -> anyhow::Result<Value> {
    let actor = identity::resolve_session_by_token(&state, &request.session_token).await?;
    let node_id = request.node_id.trim();
    if node_id.is_empty() {
        anyhow::bail!("nodeId is required");
    }
    let command_type = request.command_type.trim();
    if !matches!(
        command_type,
        "headless_status"
            | "headless_observe"
            | "system_info"
            | "process_snapshot"
            | "list_directory"
            | "stat_path"
            | "read_file_preview"
            | "tail_file_preview"
            | "read_file_range"
            | "find_paths"
            | "grep_files"
    ) {
        anyhow::bail!("unsupported node observe command: {command_type}");
    }
    let mut payload = request.payload.unwrap_or_else(|| json!({}));
    if !payload.is_object() {
        anyhow::bail!("node observe payload must be a JSON object");
    }
    if let Some(object) = payload.as_object_mut() {
        object.insert("source".to_string(), json!("control_ui"));
        object.insert("actor".to_string(), json!(actor.operator_name.clone()));
    }
    let (command, delivery) =
        control_plane::dispatch_gateway_command(&state, node_id, command_type.to_string(), payload)
            .await?;
    let command = wait_for_node_command_result(&state, command.command_id, 40).await?;
    Ok(json!({
        "command": command,
        "delivery": delivery,
        "actor": actor.operator_name,
    }))
}

fn workbench_node_status(node: NodeRecord) -> Value {
    let is_headless = node
        .capabilities
        .iter()
        .any(|capability| capability == "headless_status" || capability == "headless_observe");
    let runtime_mode = if is_headless {
        "headless / read-only observability"
    } else {
        "desktop / interactive control"
    };
    let runtime_policy_summary = if is_headless {
        "read_only_observe; blocks desktop_interaction, managed_browser, shell_exec"
    } else {
        "interactive_control; desktop/browser actions allowed by requested capabilities"
    };
    let capability_preview = node
        .capabilities
        .iter()
        .take(6)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    json!({
        "nodeId": node.node_id,
        "displayName": node.display_name,
        "transport": node.transport,
        "capabilities": node.capabilities,
        "attestationVerified": node.attestation_verified,
        "status": node.status,
        "connected": node.connected,
        "lastSeenUnixMs": node.last_seen_unix_ms,
        "createdAtUnixMs": node.created_at_unix_ms,
        "updatedAtUnixMs": node.updated_at_unix_ms,
        "runtimeMode": runtime_mode,
        "runtimePolicySummary": runtime_policy_summary,
        "capabilityPreview": capability_preview,
    })
}

async fn wait_for_node_command_result(
    state: &Arc<AppState>,
    command_id: Uuid,
    attempts: usize,
) -> anyhow::Result<crate::app_state::NodeCommandRecord> {
    let attempts = attempts.max(1);
    for _ in 0..attempts {
        let command = state
            .get_node_command(command_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("node command not found: {command_id}"))?;
        match command.status {
            crate::app_state::NodeCommandStatus::Queued
            | crate::app_state::NodeCommandStatus::Dispatched => {
                sleep(Duration::from_millis(100)).await;
            }
            _ => return Ok(command),
        }
    }
    state
        .get_node_command(command_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("node command not found: {command_id}"))
}

fn summarize_channel_status(
    default_platforms: &[String],
    platform: &str,
    identities: Vec<ChatChannelIdentityRecord>,
) -> Value {
    let is_default = default_platforms.iter().any(|item| item == platform);
    let total_identities = identities.len();
    let mut paired_count = 0usize;
    let mut pending_count = 0usize;
    let mut rejected_count = 0usize;
    let mut blocked_count = 0usize;
    let mut dm_policies = BTreeSet::new();
    let mut latest_updated_at_unix_ms = 0u128;

    for identity in &identities {
        latest_updated_at_unix_ms = latest_updated_at_unix_ms.max(identity.updated_at_unix_ms);
        dm_policies.insert(identity.dm_policy.clone());
        match identity.status {
            ChatChannelIdentityStatus::Pending => pending_count += 1,
            ChatChannelIdentityStatus::Paired => paired_count += 1,
            ChatChannelIdentityStatus::Rejected => rejected_count += 1,
            ChatChannelIdentityStatus::Blocked => blocked_count += 1,
        }
    }

    let state_label = if paired_count > 0 {
        "paired"
    } else if pending_count > 0 {
        "pending"
    } else if blocked_count > 0 && total_identities == blocked_count {
        "blocked"
    } else if rejected_count > 0 && total_identities == rejected_count {
        "rejected"
    } else if is_default {
        "default_only"
    } else {
        "idle"
    };

    json!({
        "platform": platform,
        "isDefault": is_default,
        "stateLabel": state_label,
        "totalIdentities": total_identities,
        "pairedCount": paired_count,
        "pendingCount": pending_count,
        "rejectedCount": rejected_count,
        "blockedCount": blocked_count,
        "dmPolicies": dm_policies.into_iter().collect::<Vec<_>>(),
        "latestUpdatedAtUnixMs": if latest_updated_at_unix_ms == 0 { None::<u128> } else { Some(latest_updated_at_unix_ms) },
    })
}

async fn tail_logs_inner(
    state: Arc<AppState>,
    request: WorkbenchLogsRequest,
) -> anyhow::Result<Value> {
    let limit = request.limit.unwrap_or(24).clamp(1, 100);
    let events = state.recent_console_events(limit);
    Ok(json!({
        "events": events,
        "limit": limit,
    }))
}

async fn list_sessions_inner(state: Arc<AppState>) -> anyhow::Result<Value> {
    let sessions = identity::list_operator_session_records(&state).await?;
    Ok(json!({
        "sessions": sessions,
        "count": sessions.len(),
    }))
}

async fn inspect_session_inner(
    state: Arc<AppState>,
    request: WorkbenchSessionInspectRequest,
) -> anyhow::Result<Value> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        anyhow::bail!("sessionId is required");
    }
    let sessions = identity::list_operator_session_records(&state).await?;
    let session = sessions
        .into_iter()
        .find(|record| record.session_id.to_string() == session_id)
        .ok_or_else(|| anyhow::anyhow!("unknown operator session: {session_id}"))?;
    Ok(json!({
        "session": session
    }))
}

async fn revoke_session_inner(
    state: Arc<AppState>,
    request: WorkbenchSessionRevokeRequest,
) -> anyhow::Result<Value> {
    let actor = identity::resolve_session_by_token(&state, &request.session_token).await?;
    let session_id =
        Uuid::parse_str(request.session_id.trim()).context("sessionId must be a valid UUID")?;
    let reason = request
        .reason
        .unwrap_or_else(|| format!("revoked from workbench by {}", actor.operator_name));
    let session =
        identity::revoke_operator_session_by_id(&state, session_id, &actor.operator_name, &reason)
            .await?;
    Ok(json!({
        "session": session,
        "actor": actor.operator_name,
        "reason": reason,
    }))
}

async fn create_task_inner(
    state: Arc<AppState>,
    request: WorkbenchTaskRequest,
) -> anyhow::Result<Value> {
    let name = request.name.trim();
    let instruction = request.instruction.trim();
    if name.is_empty() {
        anyhow::bail!("task name is required");
    }
    if instruction.is_empty() {
        anyhow::bail!("instruction is required");
    }
    let response = a2a::submit_task(
        state,
        Task {
            name: name.to_string(),
            task_id: None,
            parent_task_id: None,
            instruction: instruction.to_string(),
        },
    )
    .await?;
    Ok(json!(response))
}

async fn inspect_task_inner(
    state: Arc<AppState>,
    request: WorkbenchTaskInspectRequest,
) -> anyhow::Result<Value> {
    let task_id = Uuid::parse_str(request.task_id.trim()).context("taskId must be a valid UUID")?;
    let detail = a2a::get_task_detail(state, task_id).await?;
    Ok(json!({
        "task": detail.task,
        "state": detail.state,
        "result": detail.result,
        "remote": detail.remote,
        "stream": detail.stream,
        "messages": detail.messages,
        "artifacts": detail.artifacts,
        "updates": detail.updates,
        "events": detail.events,
    }))
}

async fn inspect_task_stream_inner(
    state: Arc<AppState>,
    request: WorkbenchTaskStreamRequest,
) -> anyhow::Result<Value> {
    let task_id = Uuid::parse_str(request.task_id.trim()).context("taskId must be a valid UUID")?;
    let detail = a2a::get_task_detail(state, task_id).await?;
    let after = request.after.unwrap_or(0);
    let filtered = detail
        .stream
        .items
        .into_iter()
        .filter(|item| item.sequence > after)
        .collect::<Vec<_>>();
    let available_count = filtered.len();
    let limit = request.limit.unwrap_or(available_count).max(1);
    let has_more = available_count > limit;
    let items = filtered.into_iter().take(limit).collect::<Vec<_>>();
    let next_cursor = items.last().map(|item| item.sequence).unwrap_or(after);
    Ok(json!({
        "taskId": task_id,
        "after": after,
        "cursor": detail.stream.cursor,
        "nextCursor": next_cursor,
        "hasMore": has_more,
        "returnedCount": items.len(),
        "availableCount": available_count,
        "complete": detail.stream.complete,
        "items": items,
    }))
}

async fn invoke_delegate_inner(
    state: Arc<AppState>,
    request: WorkbenchDelegateRequest,
) -> anyhow::Result<Value> {
    let card_id = request.card_id.trim();
    let instruction = request.instruction.trim();
    if card_id.is_empty() {
        anyhow::bail!("cardId is required");
    }
    if instruction.is_empty() {
        anyhow::bail!("instruction is required");
    }
    let response = agent_cards::invoke_remote_agent_card(
        &state,
        card_id,
        InvokeAgentCardRequest {
            name: request
                .name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "delegate-from-workbench".to_string()),
            instruction: instruction.to_string(),
            parent_task_id: None,
            await_completion: request.await_completion,
            timeout_seconds: None,
            poll_interval_ms: Some(1000),
            settlement: None,
        },
        None,
    )
    .await?;
    Ok(json!(response))
}

#[cfg(test)]
mod tests {
    use super::CONTROL_UI_HTML;

    #[test]
    fn control_ui_markup_contains_expected_sections() {
        assert!(CONTROL_UI_HTML.contains("Dawn Personal Workbench"));
        assert!(CONTROL_UI_HTML.contains("id=\"bootstrap-form\""));
        assert!(CONTROL_UI_HTML.contains("id=\"task-form\""));
        assert!(CONTROL_UI_HTML.contains("id=\"command-form\""));
        assert!(CONTROL_UI_HTML.contains("id=\"native-skill-list\""));
        assert!(CONTROL_UI_HTML.contains("id=\"workbench-log-list\""));
        assert!(CONTROL_UI_HTML.contains("id=\"session-list\""));
        assert!(CONTROL_UI_HTML.contains("/api/gateway/identity/status"));
        assert!(CONTROL_UI_HTML.contains("/api/gateway/identity/sessions"));
        assert!(CONTROL_UI_HTML.contains("/api/a2a/task"));
        assert!(CONTROL_UI_HTML.contains("/app/command"));
        assert!(CONTROL_UI_HTML.contains("/app/ws"));
        assert!(CONTROL_UI_HTML.contains("skill.run"));
        assert!(CONTROL_UI_HTML.contains("skill.inspect"));
        assert!(CONTROL_UI_HTML.contains("config.get"));
        assert!(CONTROL_UI_HTML.contains("channel.status"));
        assert!(CONTROL_UI_HTML.contains("node.status"));
        assert!(CONTROL_UI_HTML.contains("node.observe"));
        assert!(CONTROL_UI_HTML.contains("config.apply"));
        assert!(CONTROL_UI_HTML.contains("logs.tail"));
        assert!(CONTROL_UI_HTML.contains("session.list"));
        assert!(CONTROL_UI_HTML.contains("session.inspect"));
        assert!(CONTROL_UI_HTML.contains("session.revoke"));
        assert!(CONTROL_UI_HTML.contains("task.inspect"));
        assert!(CONTROL_UI_HTML.contains("task.stream"));
        assert!(CONTROL_UI_HTML.contains("id=\"channel-footer\""));
    }
}
