use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::app_state::{
    AppState, NodeCommandRecord, NodeCommandStatus, NodeRecord, NodeRolloutRecord,
    NodeRolloutStatus, NodeSessionStatus, unix_timestamp_ms,
};
use crate::{node_attestation, policy, skill_registry};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRegistrationRequest {
    pub node_id: Option<String>,
    pub display_name: Option<String>,
    pub transport: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub capability_attestation: Option<node_attestation::SignedNodeCapabilityAttestation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeHeartbeatRequest {
    pub display_name: Option<String>,
    pub transport: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub capability_attestation: Option<node_attestation::SignedNodeCapabilityAttestation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCommandRequest {
    pub command_type: String,
    #[serde(default = "default_payload")]
    pub payload: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCommandDispatchResponse {
    pub command: NodeCommandRecord,
    pub delivery: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRolloutDispatchResponse {
    pub rollout: NodeRolloutRecord,
    pub delivery: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayNodeEnvelope {
    message_type: &'static str,
    node_id: String,
    command_id: Uuid,
    command_type: String,
    payload: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GatewayRolloutBundle {
    generated_at_unix_ms: u128,
    bundle_hash: String,
    policy_version: u32,
    policy_document_hash: Option<String>,
    skill_distribution_hash: String,
    policy: policy::PolicyDistributionResponse,
    skills: skill_registry::SkillDistributionResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayRolloutEnvelope {
    message_type: &'static str,
    node_id: String,
    bundle: GatewayRolloutBundle,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewaySessionEnvelope {
    message_type: &'static str,
    node_id: String,
    detail: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeInboundEnvelope {
    message_type: String,
    command_id: Option<Uuid>,
    status: Option<NodeCommandStatus>,
    result: Option<Value>,
    error: Option<String>,
    display_name: Option<String>,
    capabilities: Option<Vec<String>>,
    capability_attestation: Option<node_attestation::SignedNodeCapabilityAttestation>,
    bundle_hash: Option<String>,
    accepted: Option<bool>,
    policy_version: Option<u32>,
    skill_distribution_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionQuery {
    display_name: Option<String>,
    transport: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/nodes", get(list_nodes))
        .route("/nodes/register", post(register_node))
        .route(
            "/nodes/trust-roots",
            get(list_node_trust_roots).post(upsert_node_trust_root),
        )
        .route("/nodes/:node_id", get(get_node))
        .route("/nodes/:node_id/heartbeat", post(heartbeat))
        .route("/nodes/:node_id/rollout", get(get_node_rollout).post(dispatch_rollout))
        .route(
            "/nodes/:node_id/commands",
            get(list_node_commands).post(dispatch_command),
        )
        .route("/commands/:command_id", get(get_command))
        .route("/nodes/:node_id/session", get(open_node_session))
}

pub async fn dispatch_gateway_command(
    state: &Arc<AppState>,
    node_id: &str,
    command_type: impl Into<String>,
    payload: Value,
) -> anyhow::Result<(NodeCommandRecord, &'static str)> {
    let command_type = command_type.into();
    let node = state
        .get_node(node_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("node not found: {node_id}"))?;
    node_attestation::authorize_node_command(&node, &command_type)?;

    let command = NodeCommandRecord {
        command_id: Uuid::new_v4(),
        node_id: node_id.to_string(),
        command_type,
        payload,
        status: NodeCommandStatus::Queued,
        result: None,
        error: None,
        created_at_unix_ms: unix_timestamp_ms(),
        updated_at_unix_ms: unix_timestamp_ms(),
    };
    let command = state.insert_node_command(command).await?;
    let delivery = if enqueue_command_for_dispatch(state, &command).await {
        "dispatched"
    } else {
        "queued"
    };
    let command = state
        .get_node_command(command.command_id)
        .await?
        .unwrap_or(command);
    Ok((command, delivery))
}

async fn list_nodes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<NodeRecord>>, (StatusCode, Json<Value>)> {
    state.list_nodes().await.map(Json).map_err(internal_error)
}

async fn register_node(
    State(state): State<Arc<AppState>>,
    Json(request): Json<NodeRegistrationRequest>,
) -> Result<Json<NodeRecord>, (StatusCode, Json<Value>)> {
    let node_id = request
        .node_id
        .unwrap_or_else(|| format!("node-{}", Uuid::new_v4()));
    let node = state
        .upsert_node(
            node_id,
            request
                .display_name
                .unwrap_or_else(|| "Unnamed Dawn Node".to_string()),
            request.transport.unwrap_or_else(|| "http".to_string()),
            request.capabilities,
        )
        .await
        .map_err(internal_error)?;
    let node = match request.capability_attestation {
        Some(attestation) => apply_node_attestation(&state, &node.node_id, attestation)
            .await
            .map_err(internal_error)?,
        None => node,
    };
    Ok(Json(node))
}

async fn get_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeRecord>, (StatusCode, Json<Value>)> {
    state
        .get_node(&node_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("node not found"))
}

async fn heartbeat(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Json(request): Json<NodeHeartbeatRequest>,
) -> Result<Json<NodeRecord>, (StatusCode, Json<Value>)> {
    let updated = if state
        .get_node(&node_id)
        .await
        .map_err(internal_error)?
        .is_some()
    {
        state
            .update_node_metadata(
                &node_id,
                request.display_name,
                request.transport,
                request.capabilities,
            )
            .await
            .map_err(internal_error)?
    } else {
        Some(
            state
                .upsert_node(
                    node_id.clone(),
                    request
                        .display_name
                        .unwrap_or_else(|| format!("Dawn Node {node_id}")),
                    request.transport.unwrap_or_else(|| "http".to_string()),
                    request.capabilities.unwrap_or_default(),
                )
                .await
                .map_err(internal_error)?,
        )
    };
    let Some(node) = updated else {
        return Err(not_found("node not found"));
    };
    let node = match request.capability_attestation {
        Some(attestation) => {
            let node = apply_node_attestation(&state, &node_id, attestation)
                .await
                .map_err(internal_error)?;
            dispatch_pending_commands_for_node(&state, &node_id).await;
            let _ = dispatch_current_rollout_if_needed(&state, &node_id, false).await;
            node
        }
        None => node,
    };
    Ok(Json(node))
}

async fn get_node_rollout(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeRolloutRecord>, (StatusCode, Json<Value>)> {
    state
        .get_node_rollout(&node_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("node rollout not found"))
}

async fn list_node_trust_roots(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::app_state::NodeTrustRootRecord>>, (StatusCode, Json<Value>)> {
    node_attestation::list_trust_roots(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn upsert_node_trust_root(
    State(state): State<Arc<AppState>>,
    Json(request): Json<node_attestation::NodeTrustRootUpsertRequest>,
) -> Result<Json<node_attestation::NodeTrustRootUpsertResponse>, (StatusCode, Json<Value>)> {
    node_attestation::upsert_trust_root(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn list_node_commands(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<Vec<NodeCommandRecord>>, (StatusCode, Json<Value>)> {
    if state
        .get_node(&node_id)
        .await
        .map_err(internal_error)?
        .is_none()
    {
        return Err(not_found("node not found"));
    }
    Ok(Json(
        state
            .list_node_commands(Some(&node_id))
            .await
            .map_err(internal_error)?,
    ))
}

async fn dispatch_command(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Json(request): Json<NodeCommandRequest>,
) -> Result<Json<NodeCommandDispatchResponse>, (StatusCode, Json<Value>)> {
    if state
        .get_node(&node_id)
        .await
        .map_err(internal_error)?
        .is_none()
    {
        return Err(not_found("node not found"));
    }

    let (command, delivery) =
        dispatch_gateway_command(&state, &node_id, request.command_type, request.payload)
            .await
            .map_err(internal_error)?;

    Ok(Json(NodeCommandDispatchResponse { command, delivery }))
}

async fn dispatch_rollout(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeRolloutDispatchResponse>, (StatusCode, Json<Value>)> {
    let (rollout, delivery) = match dispatch_current_rollout_if_needed(&state, &node_id, true)
        .await
        .map_err(internal_error)?
    {
        Some(result) => result,
        None => {
            let rollout = state
                .get_node_rollout(&node_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| not_found("node rollout not found"))?;
            (rollout, "up_to_date")
        }
    };

    Ok(Json(NodeRolloutDispatchResponse { rollout, delivery }))
}

async fn get_command(
    State(state): State<Arc<AppState>>,
    Path(command_id): Path<Uuid>,
) -> Result<Json<NodeCommandRecord>, (StatusCode, Json<Value>)> {
    state
        .get_node_command(command_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("node command not found"))
}

async fn open_node_session(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Query(query): Query<SessionQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| node_session(socket, state, node_id, query))
}

async fn node_session(
    socket: WebSocket,
    state: Arc<AppState>,
    node_id: String,
    query: SessionQuery,
) {
    let node = match state
        .upsert_node(
            node_id.clone(),
            query
                .display_name
                .unwrap_or_else(|| format!("Dawn Node {node_id}")),
            query.transport.unwrap_or_else(|| "websocket".to_string()),
            Vec::new(),
        )
        .await
    {
        Ok(node) => node,
        Err(error) => {
            error!(?error, "Failed to upsert node session record for {node_id}");
            return;
        }
    };
    info!("Node session connected: {}", node.node_id);

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    state.attach_node_session(&node_id, tx.clone()).await;
    if let Err(error) = state
        .set_node_connection(&node_id, true, NodeSessionStatus::Connected)
        .await
    {
        error!(
            ?error,
            "Failed to persist connected node state for {node_id}"
        );
    }

    let (mut sender, mut receiver) = socket.split();
    let send_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sender.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    let greeting = GatewaySessionEnvelope {
        message_type: "session_ready",
        node_id: node_id.clone(),
        detail: "gateway websocket session established",
    };
    let _ = tx.send(serde_json::to_string(&greeting).unwrap_or_else(|_| "{}".to_string()));

    let pending = match state.pending_node_commands(&node_id).await {
        Ok(commands) => commands,
        Err(error) => {
            error!(?error, "Failed to load pending node commands for {node_id}");
            Vec::new()
        }
    };
    for command in pending {
        let _ = dispatch_command_to_live_session(&state, &command).await;
    }
    if node.attestation_verified {
        let _ = dispatch_current_rollout_if_needed(&state, &node_id, false).await;
    }

    while let Some(inbound) = receiver.next().await {
        match inbound {
            Ok(Message::Text(text)) => {
                handle_node_message(&state, &node_id, &text).await;
            }
            Ok(Message::Pong(_)) | Ok(Message::Ping(_)) => {
                if let Err(error) = state.touch_node(&node_id).await {
                    error!(
                        ?error,
                        "Failed to touch node heartbeat timestamp for {node_id}"
                    );
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                warn!("Node websocket error for {node_id}: {error}");
                break;
            }
        }
    }

    send_task.abort();
    state.detach_node_session(&node_id).await;
    if let Err(error) = state
        .set_node_connection(&node_id, false, NodeSessionStatus::Disconnected)
        .await
    {
        error!(
            ?error,
            "Failed to persist disconnected node state for {node_id}"
        );
    }
    info!("Node session disconnected: {}", node_id);
}

async fn handle_node_message(state: &Arc<AppState>, node_id: &str, raw: &str) {
    let Ok(message) = serde_json::from_str::<NodeInboundEnvelope>(raw) else {
        warn!("Ignoring malformed node payload from {node_id}");
        return;
    };

    match message.message_type.as_str() {
        "heartbeat" => {
            let attestation_verified = if let Some(attestation) = message.capability_attestation {
                match apply_node_attestation(state, node_id, attestation).await {
                    Ok(node) => node.attestation_verified,
                    Err(error) => {
                        error!(?error, "Failed to verify node attestation for {node_id}");
                        false
                    }
                }
            } else {
                if let Err(error) = state
                    .update_node_metadata(node_id, message.display_name, None, message.capabilities)
                    .await
                {
                    error!(?error, "Failed to update node metadata for {node_id}");
                }
                false
            };
            if let Err(error) = state
                .set_node_connection(node_id, true, NodeSessionStatus::Connected)
                .await
            {
                error!(
                    ?error,
                    "Failed to update node connection state for {node_id}"
                );
            }
            if attestation_verified {
                dispatch_pending_commands_for_node(state, node_id).await;
                let _ = dispatch_current_rollout_if_needed(state, node_id, false).await;
            }
        }
        "command_result" => {
            let Some(command_id) = message.command_id else {
                warn!("Node {node_id} sent command_result without command_id");
                return;
            };
            let status = message.status.unwrap_or(NodeCommandStatus::Succeeded);
            if let Err(error) = state
                .update_node_command(command_id, status, message.result, message.error)
                .await
            {
                error!(?error, "Failed to persist command result for {node_id}");
            }
        }
        "rollout_ack" => {
            let Some(bundle_hash) = message.bundle_hash else {
                warn!("Node {node_id} sent rollout_ack without bundle_hash");
                return;
            };
            if let Err(error) = process_rollout_ack(
                state,
                node_id,
                bundle_hash,
                message.accepted.unwrap_or(false),
                message.error,
                message.policy_version,
                message.skill_distribution_hash,
            )
            .await
            {
                error!(?error, "Failed to persist rollout ack for {node_id}");
            }
        }
        other => {
            warn!("Ignoring unsupported node message type '{other}' from {node_id}");
        }
    }
}

async fn enqueue_command_for_dispatch(state: &Arc<AppState>, command: &NodeCommandRecord) -> bool {
    if state.get_node_session(&command.node_id).await.is_none() {
        return false;
    }
    dispatch_command_to_live_session(state, command).await
}

async fn dispatch_command_to_live_session(
    state: &Arc<AppState>,
    command: &NodeCommandRecord,
) -> bool {
    let Ok(Some(node)) = state.get_node(&command.node_id).await else {
        return false;
    };
    if node_attestation::authorize_node_command(&node, &command.command_type).is_err() {
        return false;
    }
    let Some(sender) = state.get_node_session(&command.node_id).await else {
        return false;
    };

    let envelope = GatewayNodeEnvelope {
        message_type: "command_dispatch",
        node_id: command.node_id.clone(),
        command_id: command.command_id,
        command_type: command.command_type.clone(),
        payload: command.payload.clone(),
    };
    let Ok(serialized) = serde_json::to_string(&envelope) else {
        if let Err(error) = state
            .update_node_command(
                command.command_id,
                NodeCommandStatus::Failed,
                None,
                Some("failed to serialize gateway command envelope".to_string()),
            )
            .await
        {
            error!(
                ?error,
                "Failed to persist serialization error for command {}", command.command_id
            );
        }
        return false;
    };

    if sender.send(serialized).is_err() {
        if let Err(error) = state
            .update_node_command(
                command.command_id,
                NodeCommandStatus::Queued,
                None,
                Some("node session disappeared before command dispatch".to_string()),
            )
            .await
        {
            error!(
                ?error,
                "Failed to persist dispatch queue reset for command {}", command.command_id
            );
        }
        return false;
    }

    if let Err(error) = state
        .update_node_command(
            command.command_id,
            NodeCommandStatus::Dispatched,
            None,
            None,
        )
        .await
    {
        error!(
            ?error,
            "Failed to persist dispatch state for command {}", command.command_id
        );
        return false;
    }
    true
}

async fn dispatch_current_rollout_if_needed(
    state: &Arc<AppState>,
    node_id: &str,
    force: bool,
) -> anyhow::Result<Option<(NodeRolloutRecord, &'static str)>> {
    let node = state
        .get_node(node_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("node not found: {node_id}"))?;
    if !node.attestation_verified {
        anyhow::bail!("node '{}' is not attested; rollout dispatch is blocked", node.node_id);
    }

    let bundle = build_current_rollout_bundle(state).await?;
    let existing = state.get_node_rollout(node_id).await?;
    if !force
        && existing
            .as_ref()
            .map(|record| {
                record.bundle_hash == bundle.bundle_hash
                    && record.status == NodeRolloutStatus::Acknowledged
            })
            .unwrap_or(false)
    {
        return Ok(None);
    }

    let now = unix_timestamp_ms();
    let mut rollout = NodeRolloutRecord {
        node_id: node_id.to_string(),
        bundle_hash: bundle.bundle_hash.clone(),
        policy_version: bundle.policy_version,
        policy_document_hash: bundle.policy_document_hash.clone(),
        skill_distribution_hash: bundle.skill_distribution_hash.clone(),
        status: NodeRolloutStatus::Pending,
        last_error: None,
        last_sent_at_unix_ms: now,
        last_ack_at_unix_ms: None,
        created_at_unix_ms: existing
            .as_ref()
            .map(|record| record.created_at_unix_ms)
            .unwrap_or(now),
        updated_at_unix_ms: now,
    };

    let delivery = if send_rollout_to_live_session(state, &node, &bundle).await {
        rollout.status = NodeRolloutStatus::Sent;
        "sent"
    } else {
        rollout.last_error =
            Some("node session not connected; rollout will retry on next attested session".into());
        "pending"
    };
    let rollout = state.save_node_rollout(&rollout).await?;
    Ok(Some((rollout, delivery)))
}

async fn send_rollout_to_live_session(
    state: &Arc<AppState>,
    node: &NodeRecord,
    bundle: &GatewayRolloutBundle,
) -> bool {
    let Some(sender) = state.get_node_session(&node.node_id).await else {
        return false;
    };
    let envelope = GatewayRolloutEnvelope {
        message_type: "rollout_bundle",
        node_id: node.node_id.clone(),
        bundle: bundle.clone(),
    };
    let Ok(serialized) = serde_json::to_string(&envelope) else {
        return false;
    };
    sender.send(serialized).is_ok()
}

async fn process_rollout_ack(
    state: &Arc<AppState>,
    node_id: &str,
    bundle_hash: String,
    accepted: bool,
    error_message: Option<String>,
    policy_version: Option<u32>,
    skill_distribution_hash: Option<String>,
) -> anyhow::Result<()> {
    let Some(mut rollout) = state.get_node_rollout(node_id).await? else {
        anyhow::bail!("node '{node_id}' acknowledged an unknown rollout");
    };
    if rollout.bundle_hash != bundle_hash {
        anyhow::bail!(
            "node '{}' acknowledged stale rollout '{}', current bundle is '{}'",
            node_id,
            bundle_hash,
            rollout.bundle_hash
        );
    }
    if let Some(policy_version) = policy_version {
        if policy_version != rollout.policy_version {
            anyhow::bail!(
                "node '{}' acknowledged rollout policy version {}, expected {}",
                node_id,
                policy_version,
                rollout.policy_version
            );
        }
    }
    if let Some(skill_distribution_hash) = skill_distribution_hash {
        if skill_distribution_hash != rollout.skill_distribution_hash {
            anyhow::bail!(
                "node '{}' acknowledged rollout skill hash '{}', expected '{}'",
                node_id,
                skill_distribution_hash,
                rollout.skill_distribution_hash
            );
        }
    }

    rollout.status = if accepted {
        NodeRolloutStatus::Acknowledged
    } else {
        NodeRolloutStatus::Rejected
    };
    rollout.last_error = if accepted {
        None
    } else {
        Some(error_message.unwrap_or_else(|| "node rejected rollout".to_string()))
    };
    rollout.last_ack_at_unix_ms = Some(unix_timestamp_ms());
    rollout.updated_at_unix_ms = unix_timestamp_ms();
    state.save_node_rollout(&rollout).await?;
    Ok(())
}

async fn build_current_rollout_bundle(state: &Arc<AppState>) -> anyhow::Result<GatewayRolloutBundle> {
    let policy_distribution = policy::current_distribution(state).await?;
    let skill_distribution = skill_registry::current_distribution(state).await?;
    let policy_document_hash = match &policy_distribution.profile.document_hash {
        Some(hash) => Some(hash.clone()),
        None => Some(hash_json_value(&policy_distribution.profile)?),
    };
    let skill_distribution_hash = hash_json_value(&skill_distribution)?;
    let generated_at_unix_ms = unix_timestamp_ms();
    let bundle_hash = hash_json_value(&json!({
        "policyVersion": policy_distribution.profile.version,
        "policyDocumentHash": &policy_document_hash,
        "skillDistributionHash": &skill_distribution_hash
    }))?;

    Ok(GatewayRolloutBundle {
        generated_at_unix_ms,
        bundle_hash,
        policy_version: policy_distribution.profile.version,
        policy_document_hash,
        skill_distribution_hash,
        policy: policy_distribution,
        skills: skill_distribution,
    })
}

fn hash_json_value(value: &impl Serialize) -> anyhow::Result<String> {
    let payload = serde_json::to_vec(value)?;
    Ok(hex::encode(Sha256::digest(payload)))
}

fn default_payload() -> Value {
    json!({})
}

async fn apply_node_attestation(
    state: &Arc<AppState>,
    node_id: &str,
    attestation: node_attestation::SignedNodeCapabilityAttestation,
) -> anyhow::Result<NodeRecord> {
    let update = node_attestation::resolve_attestation_update(state, node_id, attestation).await?;
    state
        .apply_node_attestation(node_id, update)
        .await?
        .ok_or_else(|| anyhow::anyhow!("node not found: {node_id}"))
}

async fn dispatch_pending_commands_for_node(state: &Arc<AppState>, node_id: &str) {
    let pending = match state.pending_node_commands(node_id).await {
        Ok(commands) => commands,
        Err(error) => {
            error!(?error, "Failed to load pending node commands for {node_id}");
            return;
        }
    };
    for command in pending {
        let _ = dispatch_command_to_live_session(state, &command).await;
    }
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "Control-plane persistence failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal persistence error"
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use uuid::Uuid;
    use wasmtime::Engine;

    use super::{dispatch_current_rollout_if_needed, process_rollout_ack};
    use crate::{
        app_state::{AppState, NodeAttestationState, NodeRolloutStatus},
        sandbox,
    };

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-control-plane-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    async fn attested_node(state: &Arc<AppState>, node_id: &str) {
        state
            .upsert_node(
                node_id.to_string(),
                "Test Node".to_string(),
                "websocket".to_string(),
                vec!["agent_ping".to_string()],
            )
            .await
            .unwrap();
        state
            .apply_node_attestation(
                node_id,
                NodeAttestationState {
                    issuer_did: "did:dawn:node:test".to_string(),
                    signature_hex: "aa".repeat(64),
                    document_hash: "bb".repeat(32),
                    issued_at_unix_ms: 1_700_000_000_000,
                    verified: true,
                    verified_at_unix_ms: Some(1_700_000_000_010),
                    attestation_error: None,
                    verified_capabilities: Some(vec!["agent_ping".to_string()]),
                    display_name: Some("Test Node".to_string()),
                    transport: Some("websocket".to_string()),
                },
            )
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn creates_pending_rollout_for_attested_offline_node() {
        let (state, db_path) = test_state().await.unwrap();
        attested_node(&state, "node-alpha").await;

        let (rollout, delivery) = dispatch_current_rollout_if_needed(&state, "node-alpha", true)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(delivery, "pending");
        assert_eq!(rollout.status, NodeRolloutStatus::Pending);
        assert!(rollout.policy_version >= 1);
        assert!(!rollout.bundle_hash.is_empty());

        drop(state);
        fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn acknowledges_matching_rollout_bundle() {
        let (state, db_path) = test_state().await.unwrap();
        attested_node(&state, "node-beta").await;

        let (rollout, _) = dispatch_current_rollout_if_needed(&state, "node-beta", true)
            .await
            .unwrap()
            .unwrap();
        process_rollout_ack(
            &state,
            "node-beta",
            rollout.bundle_hash.clone(),
            true,
            None,
            Some(rollout.policy_version),
            Some(rollout.skill_distribution_hash.clone()),
        )
        .await
        .unwrap();

        let rollout = state.get_node_rollout("node-beta").await.unwrap().unwrap();
        assert_eq!(rollout.status, NodeRolloutStatus::Acknowledged);
        assert!(rollout.last_ack_at_unix_ms.is_some());

        drop(state);
        fs::remove_file(db_path).ok();
    }
}
