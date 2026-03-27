use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    agent_cards::{
        self, InvokeAgentCardRequest, RemoteAgentInvocationRecord, RemoteInvocationStatus,
        RemoteSettlementRequest,
    },
    ap2::{self, PaymentRequest},
    app_state::{
        AppState, NodeCommandRecord, NodeCommandStatus, OrchestrationRunRecord,
        OrchestrationRunStatus, StoredTask, TaskEventRecord, TaskStatus, unix_timestamp_ms,
    },
    connectors::{self, ChatDispatchRequest, OpenAIResponseRequest},
    control_plane,
    policy::{self, PolicyEffect},
    sandbox, skill_registry,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub name: String,
    pub task_id: Option<Uuid>,
    pub parent_task_id: Option<Uuid>,
    pub instruction: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResponse {
    pub task: StoredTask,
    pub sandbox_status: String,
    pub state: A2aTaskState,
    pub result: A2aTaskResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<A2aRemoteStatus>,
    pub stream: A2aTaskStream,
    pub messages: Vec<A2aMessage>,
    pub artifacts: Vec<A2aArtifact>,
    pub updates: Vec<A2aTaskUpdate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetailResponse {
    pub task: StoredTask,
    pub events: Vec<TaskEventRecord>,
    pub state: A2aTaskState,
    pub result: A2aTaskResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<A2aRemoteStatus>,
    pub stream: A2aTaskStream,
    pub messages: Vec<A2aMessage>,
    pub artifacts: Vec<A2aArtifact>,
    pub updates: Vec<A2aTaskUpdate>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskState {
    pub status: TaskStatus,
    pub phase: String,
    pub terminal: bool,
    pub awaiting_binding: bool,
    pub awaiting_payment: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskResult {
    pub summary: String,
    pub status: TaskStatus,
    pub complete: bool,
    pub updated_at_unix_ms: u128,
    pub display_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_label: Option<String>,
    pub display_text: String,
    pub last_event_type: Option<String>,
    pub latest_update_type: Option<String>,
    pub latest_update_detail: Option<String>,
    pub latest_message_label: Option<String>,
    pub latest_message_text: Option<String>,
    pub latest_message: Option<A2aMessage>,
    pub primary_artifact_name: Option<String>,
    pub primary_artifact_mime_type: Option<String>,
    pub primary_artifact_preview: Option<String>,
    pub artifact_names: Vec<String>,
    pub message_labels: Vec<String>,
    pub artifact_mime_types: Vec<String>,
    pub message_count: usize,
    pub artifact_count: usize,
    pub update_count: usize,
    pub stream_cursor: usize,
    pub stream_next_cursor: usize,
    pub stream_has_more: bool,
    pub stream_returned_count: usize,
    pub stream_available_count: usize,
    pub stream_summary: A2aTaskStreamSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_stream_item: Option<A2aTaskStreamItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_state_label: Option<String>,
    pub remote_total_invocations: usize,
    pub remote_dispatched_count: usize,
    pub remote_running_count: usize,
    pub remote_completed_count: usize,
    pub remote_failed_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_latest_card_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_latest_agent_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_latest_task_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskUpdate {
    pub event_type: String,
    pub detail: String,
    pub created_at_unix_ms: u128,
    pub terminal: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskStream {
    pub after: usize,
    pub cursor: usize,
    pub next_cursor: usize,
    pub has_more: bool,
    pub returned_count: usize,
    pub available_count: usize,
    pub complete: bool,
    pub summary: A2aTaskStreamSummary,
    pub items: Vec<A2aTaskStreamItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskStreamSummary {
    pub total_items: usize,
    pub terminal_items: usize,
    pub latest_kind: Option<String>,
    pub latest_phase: Option<String>,
    pub latest_event_type: Option<String>,
    pub kind_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskStreamItem {
    pub sequence: usize,
    pub kind: String,
    pub phase: String,
    pub event_type: String,
    pub detail: String,
    pub summary: String,
    pub created_at_unix_ms: u128,
    pub terminal: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2aRemoteStatus {
    pub state_label: String,
    pub total_invocations: usize,
    pub dispatched_count: usize,
    pub running_count: usize,
    pub completed_count: usize,
    pub failed_count: usize,
    pub latest_updated_at_unix_ms: u128,
    pub latest_invocation: Option<A2aRemoteInvocation>,
    pub invocations: Vec<A2aRemoteInvocation>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2aRemoteInvocation {
    pub invocation_id: Uuid,
    pub card_id: String,
    pub remote_agent_url: String,
    pub remote_task_id: Option<String>,
    pub status: RemoteInvocationStatus,
    pub error: Option<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum A2aMessageRole {
    User,
    Agent,
    System,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum A2aPart {
    Text { text: String },
    Data { data: Value },
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2aMessage {
    pub role: A2aMessageRole,
    pub parts: Vec<A2aPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2aArtifact {
    pub name: String,
    pub mime_type: String,
    pub parts: Vec<A2aPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WasmInstructionBinding {
    skill_id: String,
    version: Option<String>,
    function_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OrchestrationPlan {
    steps: Vec<OrchestrationStep>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum OrchestrationStep {
    NodeCommand {
        node_id: String,
        command_type: String,
        #[serde(default = "default_value_payload")]
        payload: Value,
        timeout_seconds: Option<u64>,
    },
    ModelConnector {
        provider: String,
        input: String,
        model: Option<String>,
        instructions: Option<String>,
    },
    ChatConnector {
        platform: String,
        text: String,
        chat_id: Option<String>,
        parse_mode: Option<String>,
        disable_notification: Option<bool>,
    },
    RemoteA2aAgent {
        card_id: String,
        name: String,
        instruction: String,
        await_completion: Option<bool>,
        timeout_seconds: Option<u64>,
        settlement: Option<RemoteSettlementRequest>,
    },
    Ap2Authorize {
        mandate_id: Uuid,
        amount: f64,
        description: String,
    },
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TaskStreamQuery {
    after: Option<usize>,
    limit: Option<usize>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/task", post(create_task))
        .route("/tasks", get(list_tasks))
        .route("/task/:task_id", get(get_task))
        .route("/task/:task_id/events", get(get_task_events))
        .route("/task/:task_id/stream", get(get_task_stream))
}

async fn status() -> &'static str {
    "A2A Endpoints Operational."
}

async fn list_tasks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<StoredTask>>, (StatusCode, Json<Value>)> {
    state.list_tasks().await.map(Json).map_err(internal_error)
}

async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskDetailResponse>, (StatusCode, Json<Value>)> {
    get_task_detail(state, task_id)
        .await
        .map(Json)
        .map_err(service_error)
}

async fn get_task_events(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Vec<TaskEventRecord>>, (StatusCode, Json<Value>)> {
    if state
        .get_task(task_id)
        .await
        .map_err(internal_error)?
        .is_none()
    {
        return Err(not_found("task not found"));
    }
    Ok(Json(
        state.task_events(task_id).await.map_err(internal_error)?,
    ))
}

async fn get_task_stream(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<Uuid>,
    Query(query): Query<TaskStreamQuery>,
) -> Result<Json<A2aTaskStream>, (StatusCode, Json<Value>)> {
    let task = state
        .get_task(task_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("task not found"))?;
    let events = state.task_events(task_id).await.map_err(internal_error)?;
    Ok(Json(build_task_stream(
        &task,
        &events,
        query.after,
        query.limit,
    )))
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(task): Json<Task>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<Value>)> {
    submit_task(state, task)
        .await
        .map(Json)
        .map_err(service_error)
}

pub async fn submit_task(state: Arc<AppState>, task: Task) -> anyhow::Result<TaskResponse> {
    info!("Creating new A2A task: {}", task.name);
    let task_id = task.task_id.unwrap_or_else(Uuid::new_v4);
    let stored_task = StoredTask {
        task_id,
        parent_task_id: task.parent_task_id,
        name: task.name,
        instruction: task.instruction.clone(),
        status: TaskStatus::Accepted,
        linked_payment_id: None,
        last_update_reason: "task accepted by gateway".to_string(),
        created_at_unix_ms: unix_timestamp_ms(),
        updated_at_unix_ms: unix_timestamp_ms(),
    };
    let stored_task = state.insert_task(stored_task).await?;
    state
        .record_task_event(
            task_id,
            "task_accepted",
            "gateway accepted the task for orchestration",
        )
        .await?;

    let orchestration_plan = parse_orchestration_plan(&task.instruction)?;
    let wasm_binding = parse_wasm_instruction(&task.instruction)?;

    let sandbox_status = if let Some(plan) = orchestration_plan {
        state
            .update_task(
                task_id,
                TaskStatus::Queued,
                "task queued for orchestration execution",
                None,
            )
            .await?;
        state
            .record_task_event(
                task_id,
                "orchestration_queued",
                format!("gateway queued {} orchestration steps", plan.steps.len()),
            )
            .await?;
        initialize_orchestration_run(&state, task_id, &plan).await?;
        spawn_orchestration_resume(state.clone(), task_id);

        "task accepted; orchestration execution was queued".to_string()
    } else if let Some(binding) = wasm_binding {
        match skill_registry::find_skill(&state, &binding.skill_id, binding.version.as_deref())
            .await?
        {
            Some(skill) => {
                let execution_function = binding
                    .function_name
                    .clone()
                    .unwrap_or_else(|| skill.entry_function.clone());
                state
                    .update_task(
                        task_id,
                        TaskStatus::Running,
                        format!("executing bound wasm skill {}", skill.skill_id),
                        None,
                    )
                    .await?;
                state
                    .record_task_event(
                        task_id,
                        "skill_binding_resolved",
                        format!(
                            "resolved {}@{} using function {}",
                            skill.skill_id, skill.version, execution_function
                        ),
                    )
                    .await?;

                let wasm_bytes = tokio::fs::read(&skill.artifact_path)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "failed to read skill artifact {}: {error}",
                            skill.artifact_path
                        )
                    })?;
                match sandbox::execute_skill(&state.engine, &wasm_bytes, &execution_function) {
                    Ok(msg) => {
                        state
                            .update_task(
                                task_id,
                                TaskStatus::Completed,
                                format!("wasm skill {} completed", skill.skill_id),
                                None,
                            )
                            .await?;
                        state
                            .record_task_event(task_id, "skill_executed", &msg)
                            .await?;
                        msg
                    }
                    Err(error) => {
                        let detail =
                            format!("sandbox blocked or failed the requested skill: {error}");
                        warn!("{detail}");
                        state
                            .update_task(
                                task_id,
                                TaskStatus::Failed,
                                "bound wasm skill execution failed",
                                None,
                            )
                            .await?;
                        state
                            .record_task_event(task_id, "skill_execution_failed", &detail)
                            .await?;
                        detail
                    }
                }
            }
            None => {
                let detail = format!(
                    "task accepted; awaiting registered Wasm skill binding for {}",
                    binding.skill_id
                );
                state
                    .update_task(
                        task_id,
                        TaskStatus::AwaitingSkillBinding,
                        "task is waiting for a registered skill artifact binding",
                        None,
                    )
                    .await?;
                state
                    .record_task_event(task_id, "awaiting_skill_binding", &detail)
                    .await?;
                detail
            }
        }
    } else {
        let detail =
            "task accepted; execution deferred until a Wasm skill artifact is attached".to_string();
        state
            .update_task(
                task_id,
                TaskStatus::AwaitingSkillBinding,
                "task is waiting for a skill artifact binding",
                None,
            )
            .await?;
        state
            .record_task_event(task_id, "awaiting_skill_binding", &detail)
            .await?;
        detail
    };

    let task = state.get_task(task_id).await?.unwrap_or(stored_task);

    let events = state.task_events(task_id).await?;
    let state_envelope = build_task_state(&task);
    let messages = build_task_messages(&task, &events);
    let artifacts = build_task_artifacts(&task, &events);
    let updates = build_task_updates(&events);
    let remote = build_remote_status(&state, task_id).await?;
    let stream = build_task_stream(&task, &events, None, None);
    let result = build_task_result(
        &task,
        &messages,
        &artifacts,
        &updates,
        remote.as_ref(),
        &stream,
    );

    Ok(TaskResponse {
        task,
        sandbox_status,
        state: state_envelope,
        result,
        remote,
        stream,
        messages,
        artifacts,
        updates,
    })
}

pub async fn get_task_detail(
    state: Arc<AppState>,
    task_id: Uuid,
) -> anyhow::Result<TaskDetailResponse> {
    let task = state
        .get_task(task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task not found"))?;
    let events = state.task_events(task_id).await?;
    let state_envelope = build_task_state(&task);
    let messages = build_task_messages(&task, &events);
    let artifacts = build_task_artifacts(&task, &events);
    let updates = build_task_updates(&events);
    let remote = build_remote_status(&state, task_id).await?;
    let stream = build_task_stream(&task, &events, None, None);
    let result = build_task_result(
        &task,
        &messages,
        &artifacts,
        &updates,
        remote.as_ref(),
        &stream,
    );
    Ok(TaskDetailResponse {
        task,
        events,
        state: state_envelope,
        result,
        remote,
        stream,
        messages,
        artifacts,
        updates,
    })
}

fn parse_wasm_instruction(instruction: &str) -> anyhow::Result<Option<WasmInstructionBinding>> {
    let Some(raw_binding) = instruction.strip_prefix("wasm:") else {
        return Ok(None);
    };
    if raw_binding.is_empty() {
        anyhow::bail!("wasm instruction requires a skill id");
    }

    let (skill_selector, function_name) = match raw_binding.split_once('#') {
        Some((selector, function_name)) if !function_name.is_empty() => {
            (selector, Some(function_name.to_string()))
        }
        Some((_selector, _)) => anyhow::bail!("wasm instruction function name cannot be empty"),
        None => (raw_binding, None),
    };
    let (skill_id, version) = match skill_selector.split_once('@') {
        Some((skill_id, version)) if !skill_id.is_empty() && !version.is_empty() => {
            (skill_id.to_string(), Some(version.to_string()))
        }
        Some((_skill_id, _version)) => {
            anyhow::bail!("wasm instruction version selector is invalid")
        }
        None => (skill_selector.to_string(), None),
    };

    Ok(Some(WasmInstructionBinding {
        skill_id,
        version,
        function_name,
    }))
}

#[derive(Clone, Copy)]
struct TemplateContext<'a> {
    task_id: Uuid,
    task_name: &'a str,
    task_instruction: &'a str,
    last_result: Option<&'a Value>,
    step_index: usize,
}

enum StepExecution {
    Completed(Value),
    PausedForPayment {
        transaction_id: Uuid,
        payment_result: Value,
    },
}

impl OrchestrationStep {
    fn summary(&self, context: &TemplateContext<'_>) -> anyhow::Result<String> {
        match self {
            Self::NodeCommand {
                node_id,
                command_type,
                ..
            } => Ok(format!(
                "node_command to {} using {}",
                resolve_template_string(node_id, context),
                resolve_template_string(command_type, context)
            )),
            Self::ModelConnector {
                provider, model, ..
            } => Ok(format!(
                "model_connector via {} ({})",
                resolve_template_string(provider, context),
                model
                    .as_ref()
                    .map(|value| resolve_template_string(value, context))
                    .unwrap_or_else(|| "default-model".to_string())
            )),
            Self::ChatConnector { platform, .. } => Ok(format!(
                "chat_connector via {}",
                resolve_template_string(platform, context)
            )),
            Self::RemoteA2aAgent {
                card_id,
                name,
                settlement,
                ..
            } => Ok(format!(
                "remote_a2a_agent via {} ({}){}",
                resolve_template_string(card_id, context),
                resolve_template_string(name, context),
                settlement
                    .as_ref()
                    .map(|value| format!(
                        " with AP2 settlement {:.2} ({})",
                        value.amount,
                        resolve_template_string(&value.description, context)
                    ))
                    .unwrap_or_default()
            )),
            Self::Ap2Authorize {
                mandate_id,
                amount,
                description,
            } => Ok(format!(
                "ap2_authorize for mandate {} amount {:.2} ({})",
                mandate_id,
                amount,
                resolve_template_string(description, context)
            )),
        }
    }
}

async fn execute_step(
    state: &Arc<AppState>,
    step: OrchestrationStep,
    context: &TemplateContext<'_>,
) -> anyhow::Result<StepExecution> {
    match step {
        OrchestrationStep::NodeCommand {
            node_id,
            command_type,
            payload,
            timeout_seconds,
        } => {
            let resolved_command_type = resolve_template_string(&command_type, context);
            let policy_profile = policy::current_profile(state).await?;
            log_policy_decision(
                state,
                context.task_id,
                context.step_index,
                policy::evaluate_node_command(&policy_profile, &resolved_command_type),
            )
            .await?;
            Ok(StepExecution::Completed(
                execute_node_command_step(
                    state,
                    resolve_template_string(&node_id, context),
                    resolved_command_type,
                    resolve_json_templates(&payload, context),
                    timeout_seconds.unwrap_or(30),
                )
                .await?,
            ))
        }
        OrchestrationStep::ModelConnector {
            provider,
            input,
            model,
            instructions,
        } => {
            let resolved_provider = resolve_template_string(&provider, context);
            let policy_profile = policy::current_profile(state).await?;
            log_policy_decision(
                state,
                context.task_id,
                context.step_index,
                policy::evaluate_model_provider(&policy_profile, &resolved_provider),
            )
            .await?;
            let result = connectors::execute_model_connector(
                &resolved_provider,
                OpenAIResponseRequest {
                    input: resolve_template_string(&input, context),
                    model: model.map(|value| resolve_template_string(&value, context)),
                    instructions: instructions
                        .map(|value| resolve_template_string(&value, context)),
                },
            )
            .await?;
            Ok(StepExecution::Completed(serde_json::to_value(result)?))
        }
        OrchestrationStep::ChatConnector {
            platform,
            text,
            chat_id,
            parse_mode,
            disable_notification,
        } => {
            let resolved_platform = resolve_template_string(&platform, context);
            let policy_profile = policy::current_profile(state).await?;
            log_policy_decision(
                state,
                context.task_id,
                context.step_index,
                policy::evaluate_chat_platform(&policy_profile, &resolved_platform),
            )
            .await?;
            let result = connectors::execute_chat_connector(ChatDispatchRequest {
                platform: resolved_platform,
                text: Some(resolve_template_string(&text, context)),
                chat_id: chat_id.map(|value| resolve_template_string(&value, context)),
                account_key: None,
                attachment_name: None,
                attachment_base64: None,
                attachment_content_type: None,
                reaction: None,
                target_message_id: None,
                target_author: None,
                remove_reaction: None,
                receipt_type: None,
                typing: None,
                mark_read: None,
                mark_unread: None,
                part_index: None,
                effect_id: None,
                edit_message_id: None,
                edited_text: None,
                unsend_message_id: None,
                participant_action: None,
                participant_address: None,
                group_action: None,
                group_id: None,
                group_name: None,
                group_description: None,
                group_link_mode: None,
                group_members: None,
                group_admins: None,
                parse_mode: parse_mode.map(|value| resolve_template_string(&value, context)),
                disable_notification,
                target_type: None,
                event_id: None,
                msg_id: None,
                msg_seq: None,
                is_wakeup: None,
            })
            .await?;
            Ok(StepExecution::Completed(serde_json::to_value(result)?))
        }
        OrchestrationStep::RemoteA2aAgent {
            card_id,
            name,
            instruction,
            await_completion,
            timeout_seconds,
            settlement,
        } => {
            let resolved_settlement = settlement.map(|value| RemoteSettlementRequest {
                mandate_id: value.mandate_id,
                amount: value.amount,
                description: resolve_template_string(&value.description, context),
                quote_id: None,
                counter_offer_amount: None,
            });
            let result = agent_cards::invoke_remote_agent_card(
                state,
                &resolve_template_string(&card_id, context),
                InvokeAgentCardRequest {
                    name: resolve_template_string(&name, context),
                    instruction: resolve_template_string(&instruction, context),
                    parent_task_id: Some(context.task_id),
                    await_completion,
                    timeout_seconds,
                    poll_interval_ms: Some(1000),
                    settlement: resolved_settlement,
                },
                Some(context.task_id),
            )
            .await?;
            if let Some(settlement) = result.settlement.clone() {
                Ok(StepExecution::PausedForPayment {
                    transaction_id: settlement.transaction_id,
                    payment_result: serde_json::to_value(result)?,
                })
            } else {
                Ok(StepExecution::Completed(serde_json::to_value(result)?))
            }
        }
        OrchestrationStep::Ap2Authorize {
            mandate_id,
            amount,
            description,
        } => {
            let resolved_description = resolve_template_string(&description, context);
            let policy_profile = policy::current_profile(state).await?;
            log_policy_decision(
                state,
                context.task_id,
                context.step_index,
                policy::evaluate_payment(
                    &policy_profile,
                    mandate_id,
                    amount,
                    &resolved_description,
                ),
            )
            .await?;
            let result = ap2::request_payment_authorization(
                state,
                PaymentRequest {
                    transaction_id: None,
                    task_id: Some(context.task_id),
                    mandate_id,
                    amount,
                    description: resolved_description,
                    mcu_public_did: None,
                    mcu_signature: None,
                },
            )
            .await?;
            Ok(StepExecution::PausedForPayment {
                transaction_id: result.transaction_id,
                payment_result: serde_json::to_value(result)?,
            })
        }
    }
}

async fn execute_node_command_step(
    state: &Arc<AppState>,
    node_id: String,
    command_type: String,
    payload: Value,
    timeout_seconds: u64,
) -> anyhow::Result<Value> {
    let (command, delivery) =
        control_plane::dispatch_gateway_command(state, &node_id, command_type.clone(), payload)
            .await?;
    let completed = wait_for_node_command(state, command.command_id, timeout_seconds).await?;
    let status = completed.status;
    let result = completed.result.clone();
    let error = completed.error.clone();

    if status != NodeCommandStatus::Succeeded {
        anyhow::bail!(
            "node command {} failed with status {:?}: {}",
            command.command_id,
            status,
            error.unwrap_or_else(|| "no error detail returned".to_string())
        );
    }

    Ok(json!({
        "type": "node_command",
        "nodeId": node_id,
        "commandType": command_type,
        "commandId": command.command_id,
        "delivery": delivery,
        "status": status,
        "result": result
    }))
}

async fn wait_for_node_command(
    state: &Arc<AppState>,
    command_id: Uuid,
    timeout_seconds: u64,
) -> anyhow::Result<NodeCommandRecord> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_seconds.max(1));
    loop {
        let command = state
            .get_node_command(command_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("node command disappeared: {command_id}"))?;
        match command.status {
            NodeCommandStatus::PendingApproval
            | NodeCommandStatus::Queued
            | NodeCommandStatus::Dispatched => {
                if std::time::Instant::now() >= deadline {
                    anyhow::bail!("timed out waiting for node command {command_id}");
                }
                sleep(Duration::from_millis(250)).await;
            }
            NodeCommandStatus::Succeeded | NodeCommandStatus::Failed => return Ok(command),
        }
    }
}

fn parse_orchestration_plan(raw: &str) -> anyhow::Result<Option<OrchestrationPlan>> {
    let trimmed = raw.trim();
    let candidate = if let Some(json_body) = trimmed.strip_prefix("orchestrate:") {
        json_body.trim()
    } else if trimmed.starts_with('{') {
        trimmed
    } else {
        return Ok(None);
    };

    match serde_json::from_str::<OrchestrationPlan>(candidate) {
        Ok(plan) => {
            if plan.steps.is_empty() {
                return Err(anyhow::anyhow!(
                    "orchestration plan requires at least one step"
                ));
            }
            Ok(Some(plan))
        }
        Err(error) if trimmed.starts_with('{') && !trimmed.starts_with("orchestrate:") => {
            info!("Ignoring non-orchestration JSON instruction payload: {error}");
            Ok(None)
        }
        Err(error) => Err(anyhow::anyhow!("invalid orchestration plan: {error}")),
    }
}

fn resolve_json_templates(value: &Value, context: &TemplateContext<'_>) -> Value {
    match value {
        Value::String(raw) => Value::String(resolve_template_string(raw, context)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| resolve_json_templates(item, context))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), resolve_json_templates(value, context)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn resolve_template_string(input: &str, context: &TemplateContext<'_>) -> String {
    let mut remaining = input;
    let mut output = String::new();

    while let Some(start) = remaining.find("{{") {
        output.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let expression = after_start[..end].trim();
        output.push_str(&resolve_template_expression(expression, context));
        remaining = &after_start[end + 2..];
    }

    output.push_str(remaining);
    output
}

fn resolve_template_expression(expression: &str, context: &TemplateContext<'_>) -> String {
    match expression {
        "task.id" => context.task_id.to_string(),
        "task.name" => context.task_name.to_string(),
        "task.instruction" => context.task_instruction.to_string(),
        "step.index" => (context.step_index + 1).to_string(),
        "last" | "last.json" => context
            .last_result
            .map(|value| value.to_string())
            .unwrap_or_default(),
        "last.text" => context
            .last_result
            .map(extract_text_from_value)
            .unwrap_or_default(),
        _ if expression.starts_with("last.") => context
            .last_result
            .and_then(|value| lookup_json_path(value, &expression[5..]))
            .map(stringify_template_value)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_text_from_value(value: &Value) -> String {
    if let Some(text) = value.get("outputText").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(result) = value.get("result") {
        if let Some(stdout) = result.get("stdout").and_then(Value::as_str) {
            return stdout.to_string();
        }
        if let Some(text) = result.get("text").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    stringify_template_value(value)
}

fn lookup_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn stringify_template_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        _ => value.to_string(),
    }
}

fn summarize_value(value: &Value) -> String {
    let rendered = extract_text_from_value(value);
    if rendered.chars().count() <= 240 {
        rendered
    } else {
        let mut truncated = rendered.chars().take(240).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

fn build_task_messages(task: &StoredTask, events: &[TaskEventRecord]) -> Vec<A2aMessage> {
    let mut messages = vec![A2aMessage {
        role: A2aMessageRole::User,
        parts: vec![A2aPart::Text {
            text: task.instruction.clone(),
        }],
        label: Some("instruction".to_string()),
    }];

    messages.extend(events.iter().map(task_event_to_message));
    messages
}

fn build_task_state(task: &StoredTask) -> A2aTaskState {
    let (phase, terminal, awaiting_binding, awaiting_payment) = match task.status {
        TaskStatus::Accepted => ("accepted", false, false, false),
        TaskStatus::AwaitingSkillBinding => ("awaiting_binding", false, true, false),
        TaskStatus::WaitingPaymentAuthorization => ("awaiting_payment", false, false, true),
        TaskStatus::Queued => ("queued", false, false, false),
        TaskStatus::Running => ("running", false, false, false),
        TaskStatus::Completed => ("completed", true, false, false),
        TaskStatus::Failed => ("failed", true, false, false),
    };
    A2aTaskState {
        status: task.status,
        phase: phase.to_string(),
        terminal,
        awaiting_binding,
        awaiting_payment,
    }
}

fn task_event_to_message(event: &TaskEventRecord) -> A2aMessage {
    A2aMessage {
        role: if event.event_type.contains("failed") {
            A2aMessageRole::System
        } else {
            A2aMessageRole::Agent
        },
        parts: vec![A2aPart::Text {
            text: event.detail.clone(),
        }],
        label: Some(event.event_type.clone()),
    }
}

fn build_task_artifacts(task: &StoredTask, events: &[TaskEventRecord]) -> Vec<A2aArtifact> {
    vec![A2aArtifact {
        name: "task-summary".to_string(),
        mime_type: "application/json".to_string(),
        parts: vec![A2aPart::Data {
            data: json!({
                "taskId": task.task_id,
                "status": task.status,
                "lastUpdateReason": task.last_update_reason,
                "eventCount": events.len(),
                "updatedAtUnixMs": task.updated_at_unix_ms,
            }),
        }],
    }]
}

fn build_task_updates(events: &[TaskEventRecord]) -> Vec<A2aTaskUpdate> {
    events
        .iter()
        .map(|event| {
            let normalized = event.event_type.to_ascii_lowercase();
            let terminal = normalized.contains("completed")
                || normalized.contains("failed")
                || normalized.contains("rejected")
                || normalized.contains("revoked");
            A2aTaskUpdate {
                event_type: event.event_type.clone(),
                detail: event.detail.clone(),
                created_at_unix_ms: event.created_at_unix_ms,
                terminal,
            }
        })
        .collect()
}

fn build_task_stream(
    task: &StoredTask,
    events: &[TaskEventRecord],
    after: Option<usize>,
    limit: Option<usize>,
) -> A2aTaskStream {
    let after = after.unwrap_or(0);
    let filtered = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let sequence = index + 1;
            if sequence <= after {
                return None;
            }
            let (kind, phase, terminal) = classify_task_stream_event(&event.event_type);
            Some(A2aTaskStreamItem {
                sequence,
                kind: kind.to_string(),
                phase: phase.to_string(),
                event_type: event.event_type.clone(),
                detail: event.detail.clone(),
                summary: event.detail.clone(),
                created_at_unix_ms: event.created_at_unix_ms,
                terminal,
            })
        })
        .collect::<Vec<_>>();
    let available_count = filtered.len();
    let limit = limit.unwrap_or(available_count).max(1);
    let has_more = available_count > limit;
    let items = filtered.into_iter().take(limit).collect::<Vec<_>>();
    let next_cursor = items.last().map(|item| item.sequence).unwrap_or(after);
    A2aTaskStream {
        after,
        cursor: events.len(),
        next_cursor,
        has_more,
        returned_count: items.len(),
        available_count,
        complete: matches!(task.status, TaskStatus::Completed | TaskStatus::Failed),
        summary: summarize_task_stream(&items),
        items,
    }
}

fn summarize_task_stream(items: &[A2aTaskStreamItem]) -> A2aTaskStreamSummary {
    let mut kind_counts = BTreeMap::new();
    let mut terminal_items = 0usize;
    for item in items {
        *kind_counts.entry(item.kind.clone()).or_insert(0) += 1;
        if item.terminal {
            terminal_items += 1;
        }
    }
    let latest = items.last();
    A2aTaskStreamSummary {
        total_items: items.len(),
        terminal_items,
        latest_kind: latest.map(|item| item.kind.clone()),
        latest_phase: latest.map(|item| item.phase.clone()),
        latest_event_type: latest.map(|item| item.event_type.clone()),
        kind_counts,
    }
}

fn classify_task_stream_event(event_type: &str) -> (&'static str, &'static str, bool) {
    let normalized = event_type.to_ascii_lowercase();
    if normalized.contains("awaiting_skill_binding") {
        ("binding", "awaiting_binding", false)
    } else if normalized.contains("payment") {
        (
            "payment",
            if normalized.contains("authorized") {
                "completed"
            } else {
                "awaiting_payment"
            },
            normalized.contains("authorized"),
        )
    } else if normalized.contains("policy") {
        ("policy", "running", false)
    } else if normalized.contains("remote")
        || normalized.contains("invocation")
        || normalized.contains("delegate")
    {
        (
            "remote",
            if normalized.contains("failed") {
                "failed"
            } else if normalized.contains("completed") {
                "completed"
            } else {
                "running"
            },
            normalized.contains("failed") || normalized.contains("completed"),
        )
    } else if normalized.contains("failed")
        || normalized.contains("rejected")
        || normalized.contains("revoked")
    {
        ("failure", "failed", true)
    } else if normalized.contains("completed") || normalized.contains("executed") {
        ("result", "completed", true)
    } else if normalized.contains("progress") || normalized.contains("resumed") {
        ("progress", "running", false)
    } else if normalized.contains("started") {
        ("lifecycle", "running", false)
    } else if normalized.contains("queued") {
        ("lifecycle", "queued", false)
    } else if normalized.contains("accepted") {
        ("lifecycle", "accepted", false)
    } else {
        ("update", "running", false)
    }
}

async fn build_remote_status(
    state: &Arc<AppState>,
    task_id: Uuid,
) -> anyhow::Result<Option<A2aRemoteStatus>> {
    let invocations = agent_cards::list_remote_invocations(state, None, Some(task_id)).await?;
    Ok(summarize_remote_status(invocations))
}

fn summarize_remote_status(
    mut invocations: Vec<RemoteAgentInvocationRecord>,
) -> Option<A2aRemoteStatus> {
    if invocations.is_empty() {
        return None;
    }
    invocations.sort_by(|left, right| {
        right
            .updated_at_unix_ms
            .cmp(&left.updated_at_unix_ms)
            .then_with(|| right.created_at_unix_ms.cmp(&left.created_at_unix_ms))
    });
    let latest_updated_at_unix_ms = invocations
        .iter()
        .map(|invocation| invocation.updated_at_unix_ms)
        .max()
        .unwrap_or(0);
    let dispatched_count = invocations
        .iter()
        .filter(|invocation| invocation.status == RemoteInvocationStatus::Dispatched)
        .count();
    let running_count = invocations
        .iter()
        .filter(|invocation| invocation.status == RemoteInvocationStatus::Running)
        .count();
    let completed_count = invocations
        .iter()
        .filter(|invocation| invocation.status == RemoteInvocationStatus::Completed)
        .count();
    let failed_count = invocations
        .iter()
        .filter(|invocation| invocation.status == RemoteInvocationStatus::Failed)
        .count();
    let state_label = if failed_count > 0 {
        "failed"
    } else if running_count > 0 {
        "running"
    } else if dispatched_count > 0 {
        "dispatched"
    } else {
        "completed"
    };
    let latest_invocation = invocations.first().cloned().map(remote_invocation_to_a2a);
    let invocations = invocations
        .into_iter()
        .map(remote_invocation_to_a2a)
        .collect::<Vec<_>>();
    Some(A2aRemoteStatus {
        state_label: state_label.to_string(),
        total_invocations: invocations.len(),
        dispatched_count,
        running_count,
        completed_count,
        failed_count,
        latest_updated_at_unix_ms,
        latest_invocation,
        invocations,
    })
}

fn remote_invocation_to_a2a(invocation: RemoteAgentInvocationRecord) -> A2aRemoteInvocation {
    A2aRemoteInvocation {
        invocation_id: invocation.invocation_id,
        card_id: invocation.card_id,
        remote_agent_url: invocation.remote_agent_url,
        remote_task_id: invocation.remote_task_id,
        status: invocation.status,
        error: invocation.error,
        created_at_unix_ms: invocation.created_at_unix_ms,
        updated_at_unix_ms: invocation.updated_at_unix_ms,
    }
}

fn build_task_result(
    task: &StoredTask,
    messages: &[A2aMessage],
    artifacts: &[A2aArtifact],
    updates: &[A2aTaskUpdate],
    remote: Option<&A2aRemoteStatus>,
    stream: &A2aTaskStream,
) -> A2aTaskResult {
    let latest_display_message = messages
        .iter()
        .rev()
        .find(|message| message.label.as_deref() != Some("instruction"))
        .cloned();
    let latest_message = latest_display_message
        .clone()
        .or_else(|| messages.last().cloned());
    let complete = matches!(task.status, TaskStatus::Completed | TaskStatus::Failed);
    let latest_update = updates.last();
    let latest_message_label = latest_display_message
        .as_ref()
        .and_then(|message| message.label.clone());
    let latest_message_text = latest_display_message.as_ref().and_then(|message| {
        message.parts.iter().find_map(|part| match part {
            A2aPart::Text { text } => Some(text.clone()),
            A2aPart::Data { .. } => None,
        })
    });
    let last_event_type = messages.iter().rev().find_map(|message| {
        message.label.as_ref().and_then(|label| {
            if label == "instruction" {
                None
            } else {
                Some(label.clone())
            }
        })
    });
    let primary_artifact_name = artifacts.first().map(|artifact| artifact.name.clone());
    let primary_artifact_mime_type = artifacts.first().map(|artifact| artifact.mime_type.clone());
    let primary_artifact_preview = artifacts
        .first()
        .and_then(summarize_primary_artifact_preview);
    let summary = latest_message_text
        .clone()
        .or_else(|| primary_artifact_preview.clone())
        .or_else(|| latest_update.map(|update| update.detail.clone()))
        .unwrap_or_else(|| task.last_update_reason.clone());
    let (display_source, display_label, display_text) = build_task_result_display_payload(
        &summary,
        latest_message_label.clone(),
        latest_message_text.clone(),
        latest_update,
        primary_artifact_name.clone(),
        primary_artifact_preview.clone(),
        last_event_type.clone(),
    );
    A2aTaskResult {
        summary,
        status: task.status,
        complete,
        updated_at_unix_ms: task.updated_at_unix_ms,
        display_source,
        display_label,
        display_text,
        last_event_type,
        latest_update_type: latest_update.map(|update| update.event_type.clone()),
        latest_update_detail: latest_update.map(|update| update.detail.clone()),
        latest_message_label,
        latest_message_text,
        latest_message,
        primary_artifact_name,
        primary_artifact_mime_type,
        primary_artifact_preview,
        artifact_names: artifacts
            .iter()
            .map(|artifact| artifact.name.clone())
            .collect(),
        message_labels: messages
            .iter()
            .filter_map(|message| message.label.clone())
            .collect(),
        artifact_mime_types: artifacts
            .iter()
            .map(|artifact| artifact.mime_type.clone())
            .collect(),
        message_count: messages.len(),
        artifact_count: artifacts.len(),
        update_count: updates.len(),
        stream_cursor: stream.cursor,
        stream_next_cursor: stream.next_cursor,
        stream_has_more: stream.has_more,
        stream_returned_count: stream.returned_count,
        stream_available_count: stream.available_count,
        stream_summary: stream.summary.clone(),
        latest_stream_item: stream.items.last().cloned(),
        remote_state_label: remote.map(|value| value.state_label.clone()),
        remote_total_invocations: remote.map(|value| value.total_invocations).unwrap_or(0),
        remote_dispatched_count: remote.map(|value| value.dispatched_count).unwrap_or(0),
        remote_running_count: remote.map(|value| value.running_count).unwrap_or(0),
        remote_completed_count: remote.map(|value| value.completed_count).unwrap_or(0),
        remote_failed_count: remote.map(|value| value.failed_count).unwrap_or(0),
        remote_latest_card_id: remote
            .and_then(|value| value.latest_invocation.as_ref())
            .map(|item| item.card_id.clone()),
        remote_latest_agent_url: remote
            .and_then(|value| value.latest_invocation.as_ref())
            .map(|item| item.remote_agent_url.clone()),
        remote_latest_task_id: remote
            .and_then(|value| value.latest_invocation.as_ref())
            .and_then(|item| item.remote_task_id.clone()),
    }
}

fn build_task_result_display_payload(
    summary: &str,
    latest_message_label: Option<String>,
    latest_message_text: Option<String>,
    latest_update: Option<&A2aTaskUpdate>,
    primary_artifact_name: Option<String>,
    primary_artifact_preview: Option<String>,
    last_event_type: Option<String>,
) -> (String, Option<String>, String) {
    if let Some(text) = latest_message_text.filter(|value| !value.trim().is_empty()) {
        return ("message".to_string(), latest_message_label, text);
    }

    if let Some(preview) = primary_artifact_preview.filter(|value| !value.trim().is_empty()) {
        return ("artifact".to_string(), primary_artifact_name, preview);
    }

    if let Some(update) = latest_update.filter(|value| !value.detail.trim().is_empty()) {
        return (
            "update".to_string(),
            Some(update.event_type.clone()),
            update.detail.clone(),
        );
    }

    ("summary".to_string(), last_event_type, summary.to_string())
}

fn summarize_primary_artifact_preview(artifact: &A2aArtifact) -> Option<String> {
    artifact.parts.iter().find_map(|part| match part {
        A2aPart::Text { text } => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(120).collect::<String>())
            }
        }
        A2aPart::Data { data } => {
            let compact = serde_json::to_string(data).ok()?;
            if compact.is_empty() {
                None
            } else {
                Some(compact.chars().take(120).collect::<String>())
            }
        }
    })
}

fn default_value_payload() -> Value {
    json!({})
}

async fn log_policy_decision(
    state: &Arc<AppState>,
    task_id: Uuid,
    step_index: usize,
    decision: crate::policy::PolicyDecision,
) -> anyhow::Result<()> {
    let effect = match decision.effect {
        PolicyEffect::Allow => "allowed",
        PolicyEffect::Deny => "denied",
    };
    state
        .record_task_event(
            task_id,
            "policy_decision",
            format!(
                "policy {} step {}: {}",
                effect,
                step_index + 1,
                decision.reason
            ),
        )
        .await?;
    decision.ensure_allowed()
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
}

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    if message.contains("requires")
        || message.contains("cannot be empty")
        || message.contains("invalid")
        || message.contains("unsupported")
    {
        return bad_request(error);
    }
    internal_error(error)
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "A2A persistence failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal persistence error"
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::agent_cards::{RemoteAgentInvocationRecord, RemoteInvocationStatus};

    use super::{
        A2aArtifact, A2aMessage, A2aMessageRole, A2aPart, A2aRemoteInvocation, A2aRemoteStatus,
        A2aTaskStream, A2aTaskStreamItem, A2aTaskStreamSummary, A2aTaskUpdate, OrchestrationStep,
        StoredTask, TaskEventRecord, TaskStatus, TemplateContext, WasmInstructionBinding,
        build_task_artifacts, build_task_messages, build_task_result, build_task_state,
        build_task_stream, build_task_updates, classify_task_stream_event, extract_text_from_value,
        parse_orchestration_plan, parse_wasm_instruction, resolve_json_templates,
        resolve_template_string, summarize_remote_status,
    };
    use uuid::Uuid;

    #[test]
    fn parses_prefixed_orchestration_plan() {
        let raw = r#"orchestrate:{"steps":[{"kind":"node_command","nodeId":"node-local","commandType":"agent_ping"}]}"#;
        let plan = parse_orchestration_plan(raw)
            .expect("plan should parse")
            .expect("instruction should be treated as orchestration");

        assert_eq!(plan.steps.len(), 1);
        assert!(matches!(
            plan.steps.first(),
            Some(OrchestrationStep::NodeCommand { .. })
        ));
    }

    #[test]
    fn parses_ap2_authorize_step() {
        let raw = r#"orchestrate:{"steps":[{"kind":"ap2_authorize","mandateId":"00000000-0000-0000-0000-000000000111","amount":18.5,"description":"Approve {{task.name}}"}]}"#;
        let plan = parse_orchestration_plan(raw)
            .expect("plan should parse")
            .expect("instruction should be treated as orchestration");

        assert!(matches!(
            plan.steps.first(),
            Some(OrchestrationStep::Ap2Authorize { amount, .. }) if (*amount - 18.5).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn parses_remote_a2a_agent_step() {
        let raw = r#"orchestrate:{"steps":[{"kind":"remote_a2a_agent","cardId":"travel-agent","name":"delegate-booking","instruction":"Book train to Shanghai","awaitCompletion":true,"timeoutSeconds":15,"settlement":{"mandateId":"11111111-1111-1111-1111-111111111111","amount":18.5,"description":"Settle {{task.name}}"}}]}"#;
        let plan = parse_orchestration_plan(raw)
            .expect("plan should parse")
            .expect("instruction should be treated as orchestration");

        assert!(matches!(
            plan.steps.first(),
            Some(OrchestrationStep::RemoteA2aAgent {
                card_id,
                await_completion,
                timeout_seconds,
                settlement,
                ..
            }) if card_id == "travel-agent"
                && *await_completion == Some(true)
                && *timeout_seconds == Some(15)
                && settlement.as_ref().map(|value| value.amount) == Some(18.5)
        ));
    }

    #[test]
    fn resolves_templates_from_previous_result() {
        let task_id = Uuid::new_v4();
        let last_result = json!({
            "outputText": "model says hello",
            "rawResponse": {
                "messageId": "abc123"
            }
        });
        let context = TemplateContext {
            task_id,
            task_name: "demo-task",
            task_instruction: "orchestrate:demo",
            last_result: Some(&last_result),
            step_index: 1,
        };

        assert_eq!(
            resolve_template_string("{{task.name}}/{{step.index}}", &context),
            "demo-task/2"
        );
        assert_eq!(
            resolve_template_string(
                "{{last.outputText}} -> {{last.rawResponse.messageId}}",
                &context
            ),
            "model says hello -> abc123"
        );
        assert_eq!(extract_text_from_value(&last_result), "model says hello");
    }

    #[test]
    fn resolves_templates_inside_json_payloads() {
        let last_result = json!({
            "result": {
                "stdout": "dir listing"
            }
        });
        let context = TemplateContext {
            task_id: Uuid::new_v4(),
            task_name: "demo-task",
            task_instruction: "orchestrate:demo",
            last_result: Some(&last_result),
            step_index: 0,
        };
        let payload = json!({
            "prompt": "Use {{last.text}}",
            "meta": ["{{task.name}}", "{{step.index}}"]
        });

        let resolved = resolve_json_templates(&payload, &context);
        assert_eq!(resolved["prompt"], "Use dir listing");
        assert_eq!(resolved["meta"][0], "demo-task");
        assert_eq!(resolved["meta"][1], "1");
    }

    #[test]
    fn parses_wasm_instruction_with_version_and_function() {
        let binding = parse_wasm_instruction("wasm:echo-skill@1.0.0#run_skill")
            .expect("instruction should parse")
            .expect("instruction should be treated as wasm");

        assert_eq!(
            binding,
            WasmInstructionBinding {
                skill_id: "echo-skill".to_string(),
                version: Some("1.0.0".to_string()),
                function_name: Some("run_skill".to_string()),
            }
        );
    }

    #[test]
    fn parses_wasm_instruction_without_version() {
        let binding = parse_wasm_instruction("wasm:echo-skill")
            .expect("instruction should parse")
            .expect("instruction should be treated as wasm");

        assert_eq!(
            binding,
            WasmInstructionBinding {
                skill_id: "echo-skill".to_string(),
                version: None,
                function_name: None,
            }
        );
    }

    #[test]
    fn builds_a2a_messages_from_task_and_events() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "demo".to_string(),
            instruction: "Summarize the system status".to_string(),
            status: TaskStatus::Completed,
            linked_payment_id: None,
            last_update_reason: "completed".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        let events = vec![
            TaskEventRecord {
                event_type: "task_accepted".to_string(),
                detail: "gateway accepted the task".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 1,
            },
            TaskEventRecord {
                event_type: "skill_execution_failed".to_string(),
                detail: "sandbox denied shell_exec".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 2,
            },
        ];

        let messages = build_task_messages(&task, &events);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, A2aMessageRole::User);
        assert_eq!(
            messages[0].parts,
            vec![A2aPart::Text {
                text: "Summarize the system status".to_string()
            }]
        );
        assert_eq!(messages[1].role, A2aMessageRole::Agent);
        assert_eq!(messages[2].role, A2aMessageRole::System);
    }

    #[test]
    fn builds_task_summary_artifact() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "demo".to_string(),
            instruction: "Summarize the system status".to_string(),
            status: TaskStatus::Completed,
            linked_payment_id: None,
            last_update_reason: "completed".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        let events = vec![TaskEventRecord {
            event_type: "task_accepted".to_string(),
            detail: "gateway accepted the task".to_string(),
            task_id: task.task_id,
            created_at_unix_ms: 1,
        }];

        let artifacts = build_task_artifacts(&task, &events);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "task-summary");
        assert_eq!(artifacts[0].mime_type, "application/json");
        match &artifacts[0].parts[0] {
            A2aPart::Data { data } => {
                assert_eq!(data["status"], json!("completed"));
                assert_eq!(data["eventCount"], json!(1));
            }
            other => panic!("expected data artifact part, got {other:?}"),
        }
    }

    #[test]
    fn maps_task_status_into_a2a_state_flags() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "demo".to_string(),
            instruction: "Summarize the system status".to_string(),
            status: TaskStatus::WaitingPaymentAuthorization,
            linked_payment_id: None,
            last_update_reason: "awaiting payment".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };

        let state = build_task_state(&task);
        assert_eq!(state.phase, "awaiting_payment");
        assert!(!state.terminal);
        assert!(!state.awaiting_binding);
        assert!(state.awaiting_payment);
    }

    #[test]
    fn builds_task_result_from_latest_message_and_artifacts() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "demo".to_string(),
            instruction: "Summarize the system status".to_string(),
            status: TaskStatus::Completed,
            linked_payment_id: None,
            last_update_reason: "completed".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        let events = vec![TaskEventRecord {
            event_type: "task_completed".to_string(),
            detail: "All done".to_string(),
            task_id: task.task_id,
            created_at_unix_ms: 2,
        }];

        let messages = build_task_messages(&task, &events);
        let artifacts = build_task_artifacts(&task, &events);
        let updates = build_task_updates(&events);
        let stream = build_task_stream(&task, &events, None, None);
        let result = build_task_result(&task, &messages, &artifacts, &updates, None, &stream);

        assert_eq!(result.summary, "All done");
        assert_eq!(result.status, TaskStatus::Completed);
        assert!(result.complete);
        assert_eq!(result.updated_at_unix_ms, 2);
        assert_eq!(result.display_source, "message");
        assert_eq!(result.display_label.as_deref(), Some("task_completed"));
        assert_eq!(result.display_text, "All done");
        assert_eq!(result.latest_update_type.as_deref(), Some("task_completed"));
        assert_eq!(result.latest_update_detail.as_deref(), Some("All done"));
        assert_eq!(
            result.latest_message_label.as_deref(),
            Some("task_completed")
        );
        assert_eq!(result.latest_message_text.as_deref(), Some("All done"));
        assert_eq!(result.last_event_type.as_deref(), Some("task_completed"));
        assert_eq!(result.artifact_names, vec!["task-summary".to_string()]);
        assert_eq!(
            result.primary_artifact_name.as_deref(),
            Some("task-summary")
        );
        assert_eq!(
            result.primary_artifact_mime_type.as_deref(),
            Some("application/json")
        );
        assert!(
            result
                .primary_artifact_preview
                .as_deref()
                .unwrap_or_default()
                .contains("\"taskId\":\"00000000-0000-0000-0000-000000000000\"")
        );
        assert!(
            result
                .primary_artifact_preview
                .as_deref()
                .unwrap_or_default()
                .contains("\"status\":\"completed\"")
        );
        assert_eq!(
            result.message_labels,
            vec!["instruction".to_string(), "task_completed".to_string()]
        );
        assert_eq!(
            result.artifact_mime_types,
            vec!["application/json".to_string()]
        );
        assert!(result.latest_message.is_some());
        assert_eq!(result.message_count, 2);
        assert_eq!(result.artifact_count, 1);
        assert_eq!(result.update_count, 1);
        assert_eq!(result.stream_cursor, 1);
        assert_eq!(result.stream_next_cursor, 1);
        assert!(!result.stream_has_more);
        assert_eq!(result.stream_returned_count, 1);
        assert_eq!(result.stream_available_count, 1);
        assert_eq!(result.stream_summary.total_items, 1);
        assert_eq!(result.stream_summary.latest_kind.as_deref(), Some("result"));
        assert_eq!(
            result
                .latest_stream_item
                .as_ref()
                .map(|item| item.event_type.as_str()),
            Some("task_completed")
        );
    }

    #[test]
    fn task_result_display_payload_falls_back_to_artifact_preview() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "artifact-demo".to_string(),
            instruction: "Inspect generated artifact".to_string(),
            status: TaskStatus::Completed,
            linked_payment_id: None,
            last_update_reason: "artifact ready".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 4,
        };
        let messages = vec![A2aMessage {
            role: A2aMessageRole::User,
            parts: vec![A2aPart::Text {
                text: task.instruction.clone(),
            }],
            label: Some("instruction".to_string()),
        }];
        let artifacts = vec![A2aArtifact {
            name: "report.txt".to_string(),
            mime_type: "text/plain".to_string(),
            parts: vec![A2aPart::Text {
                text: "artifact preview body".to_string(),
            }],
        }];
        let updates = vec![A2aTaskUpdate {
            event_type: "task_completed".to_string(),
            detail: "artifact ready".to_string(),
            created_at_unix_ms: 4,
            terminal: true,
        }];
        let stream = A2aTaskStream {
            after: 0,
            cursor: 1,
            next_cursor: 1,
            has_more: false,
            returned_count: 1,
            available_count: 1,
            complete: true,
            summary: A2aTaskStreamSummary {
                total_items: 1,
                terminal_items: 1,
                latest_kind: Some("result".to_string()),
                latest_phase: Some("completed".to_string()),
                latest_event_type: Some("task_completed".to_string()),
                kind_counts: BTreeMap::from([(String::from("result"), 1usize)]),
            },
            items: vec![A2aTaskStreamItem {
                sequence: 1,
                kind: "result".to_string(),
                phase: "completed".to_string(),
                event_type: "task_completed".to_string(),
                detail: "artifact ready".to_string(),
                summary: "artifact ready".to_string(),
                created_at_unix_ms: 4,
                terminal: true,
            }],
        };

        let result = build_task_result(&task, &messages, &artifacts, &updates, None, &stream);

        assert_eq!(result.display_source, "artifact");
        assert_eq!(result.display_label.as_deref(), Some("report.txt"));
        assert_eq!(result.display_text, "artifact preview body");
    }

    #[test]
    fn task_result_includes_remote_status_summary_fields() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "remote-demo".to_string(),
            instruction: "Delegate to remote agent".to_string(),
            status: TaskStatus::Completed,
            linked_payment_id: None,
            last_update_reason: "remote completed".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        let messages = vec![A2aMessage {
            role: A2aMessageRole::Agent,
            label: Some("task_completed".to_string()),
            parts: vec![A2aPart::Text {
                text: "remote done".to_string(),
            }],
        }];
        let artifacts = Vec::new();
        let updates = vec![A2aTaskUpdate {
            event_type: "remote_invocation_completed".to_string(),
            detail: "remote run completed".to_string(),
            created_at_unix_ms: 2,
            terminal: true,
        }];
        let stream = A2aTaskStream {
            after: 0,
            cursor: 1,
            next_cursor: 1,
            has_more: false,
            returned_count: 1,
            available_count: 1,
            complete: true,
            summary: A2aTaskStreamSummary {
                total_items: 1,
                terminal_items: 1,
                latest_kind: Some("remote".to_string()),
                latest_phase: Some("completed".to_string()),
                latest_event_type: Some("remote_invocation_completed".to_string()),
                kind_counts: BTreeMap::from([("remote".to_string(), 1usize)]),
            },
            items: vec![A2aTaskStreamItem {
                sequence: 1,
                kind: "remote".to_string(),
                phase: "completed".to_string(),
                event_type: "remote_invocation_completed".to_string(),
                detail: "remote run completed".to_string(),
                summary: "remote completed".to_string(),
                created_at_unix_ms: 2,
                terminal: true,
            }],
        };
        let remote = A2aRemoteStatus {
            state_label: "completed".to_string(),
            total_invocations: 2,
            dispatched_count: 0,
            running_count: 0,
            completed_count: 2,
            failed_count: 0,
            latest_updated_at_unix_ms: 2,
            latest_invocation: Some(A2aRemoteInvocation {
                invocation_id: Uuid::new_v4(),
                card_id: "travel-agent".to_string(),
                remote_agent_url: "https://agent.example.com/a2a".to_string(),
                remote_task_id: Some("remote-123".to_string()),
                status: RemoteInvocationStatus::Completed,
                error: None,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 2,
            }),
            invocations: Vec::new(),
        };

        let result = build_task_result(
            &task,
            &messages,
            &artifacts,
            &updates,
            Some(&remote),
            &stream,
        );

        assert_eq!(result.remote_state_label.as_deref(), Some("completed"));
        assert_eq!(result.remote_total_invocations, 2);
        assert_eq!(result.remote_completed_count, 2);
        assert_eq!(
            result.remote_latest_card_id.as_deref(),
            Some("travel-agent")
        );
        assert_eq!(
            result.remote_latest_agent_url.as_deref(),
            Some("https://agent.example.com/a2a")
        );
        assert_eq!(result.remote_latest_task_id.as_deref(), Some("remote-123"));
    }

    #[test]
    fn summarizes_remote_invocations_into_a2a_remote_status() {
        let latest = 1_700_000_000_000u128;
        let remote = summarize_remote_status(vec![
            RemoteAgentInvocationRecord {
                invocation_id: Uuid::new_v4(),
                card_id: "travel-agent".to_string(),
                remote_agent_url: "https://agent.example.com/a2a".to_string(),
                local_task_id: Some(Uuid::new_v4()),
                remote_task_id: Some("remote-002".to_string()),
                request: json!({"instruction":"book hotel"}),
                response: Some(json!({"status":"running"})),
                status: RemoteInvocationStatus::Running,
                error: None,
                created_at_unix_ms: latest.saturating_sub(10),
                updated_at_unix_ms: latest,
            },
            RemoteAgentInvocationRecord {
                invocation_id: Uuid::new_v4(),
                card_id: "travel-agent".to_string(),
                remote_agent_url: "https://agent.example.com/a2a".to_string(),
                local_task_id: Some(Uuid::new_v4()),
                remote_task_id: Some("remote-001".to_string()),
                request: json!({"instruction":"quote"}),
                response: Some(json!({"status":"completed"})),
                status: RemoteInvocationStatus::Completed,
                error: None,
                created_at_unix_ms: latest.saturating_sub(100),
                updated_at_unix_ms: latest.saturating_sub(50),
            },
        ])
        .expect("remote summary should be present");

        assert_eq!(remote.state_label, "running");
        assert_eq!(remote.total_invocations, 2);
        assert_eq!(remote.running_count, 1);
        assert_eq!(remote.completed_count, 1);
        assert_eq!(
            remote
                .latest_invocation
                .as_ref()
                .and_then(|item| item.remote_task_id.as_deref()),
            Some("remote-002")
        );
    }

    #[test]
    fn builds_task_updates_from_events() {
        let task_id = Uuid::nil();
        let events = vec![
            TaskEventRecord {
                event_type: "task_started".to_string(),
                detail: "Running".to_string(),
                task_id,
                created_at_unix_ms: 5,
            },
            TaskEventRecord {
                event_type: "task_completed".to_string(),
                detail: "Done".to_string(),
                task_id,
                created_at_unix_ms: 9,
            },
        ];

        let updates = build_task_updates(&events);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].event_type, "task_started");
        assert!(!updates[0].terminal);
        assert_eq!(updates[1].event_type, "task_completed");
        assert!(updates[1].terminal);
        assert_eq!(updates[1].created_at_unix_ms, 9);
    }

    #[test]
    fn builds_incremental_task_stream_after_cursor() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "demo".to_string(),
            instruction: "Summarize the system status".to_string(),
            status: TaskStatus::Running,
            linked_payment_id: None,
            last_update_reason: "running".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 9,
        };
        let events = vec![
            TaskEventRecord {
                event_type: "task_accepted".to_string(),
                detail: "Accepted".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 1,
            },
            TaskEventRecord {
                event_type: "task_started".to_string(),
                detail: "Started".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 5,
            },
            TaskEventRecord {
                event_type: "task_progress".to_string(),
                detail: "Halfway there".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 9,
            },
        ];

        let stream = build_task_stream(&task, &events, Some(1), None);
        assert_eq!(stream.after, 1);
        assert_eq!(stream.cursor, 3);
        assert_eq!(stream.next_cursor, 3);
        assert_eq!(stream.returned_count, 2);
        assert_eq!(stream.available_count, 2);
        assert!(!stream.has_more);
        assert!(!stream.complete);
        assert_eq!(stream.summary.total_items, 2);
        assert_eq!(stream.summary.terminal_items, 0);
        assert_eq!(stream.summary.latest_kind.as_deref(), Some("progress"));
        assert_eq!(stream.summary.latest_phase.as_deref(), Some("running"));
        assert_eq!(
            stream.summary.latest_event_type.as_deref(),
            Some("task_progress")
        );
        assert_eq!(stream.summary.kind_counts.get("lifecycle"), Some(&1));
        assert_eq!(stream.summary.kind_counts.get("progress"), Some(&1));
        assert_eq!(stream.items.len(), 2);
        assert_eq!(stream.items[0].sequence, 2);
        assert_eq!(stream.items[0].kind, "lifecycle");
        assert_eq!(stream.items[0].phase, "running");
        assert_eq!(stream.items[0].event_type, "task_started");
        assert_eq!(stream.items[1].sequence, 3);
        assert_eq!(stream.items[1].kind, "progress");
        assert_eq!(stream.items[1].phase, "running");
        assert_eq!(stream.items[1].detail, "Halfway there");
        assert_eq!(stream.items[1].summary, "Halfway there");
    }

    #[test]
    fn builds_paginated_task_stream_with_next_cursor() {
        let task = StoredTask {
            task_id: Uuid::nil(),
            parent_task_id: None,
            name: "demo".to_string(),
            instruction: "Observe stream pagination".to_string(),
            status: TaskStatus::Running,
            linked_payment_id: None,
            last_update_reason: "running".to_string(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 12,
        };
        let events = vec![
            TaskEventRecord {
                event_type: "task_accepted".to_string(),
                detail: "Accepted".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 1,
            },
            TaskEventRecord {
                event_type: "task_started".to_string(),
                detail: "Started".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 3,
            },
            TaskEventRecord {
                event_type: "task_progress".to_string(),
                detail: "Progress".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 6,
            },
            TaskEventRecord {
                event_type: "task_completed".to_string(),
                detail: "Done".to_string(),
                task_id: task.task_id,
                created_at_unix_ms: 12,
            },
        ];

        let stream = build_task_stream(&task, &events, Some(1), Some(2));
        assert_eq!(stream.after, 1);
        assert_eq!(stream.cursor, 4);
        assert_eq!(stream.next_cursor, 3);
        assert_eq!(stream.returned_count, 2);
        assert_eq!(stream.available_count, 3);
        assert!(stream.has_more);
        assert_eq!(stream.items.len(), 2);
        assert_eq!(stream.items[0].sequence, 2);
        assert_eq!(stream.items[1].sequence, 3);
        assert_eq!(
            stream.summary.latest_event_type.as_deref(),
            Some("task_progress")
        );
    }

    #[test]
    fn classifies_binding_and_failure_stream_events() {
        assert_eq!(
            classify_task_stream_event("awaiting_skill_binding"),
            ("binding", "awaiting_binding", false)
        );
        assert_eq!(
            classify_task_stream_event("orchestration_step_failed"),
            ("failure", "failed", true)
        );
    }
}

async fn initialize_orchestration_run(
    state: &Arc<AppState>,
    task_id: Uuid,
    plan: &OrchestrationPlan,
) -> anyhow::Result<()> {
    let now = unix_timestamp_ms();
    state
        .upsert_orchestration_run(OrchestrationRunRecord {
            task_id,
            plan_json: serde_json::to_string(plan)?,
            next_step_index: 0,
            last_result: None,
            waiting_transaction_id: None,
            status: OrchestrationRunStatus::Queued,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        })
        .await?;
    Ok(())
}

pub fn spawn_orchestration_resume(state: Arc<AppState>, task_id: Uuid) {
    tokio::spawn(async move {
        if let Err(error) = resume_orchestration(state.clone(), task_id).await {
            error!(?error, "background orchestration failed for task {task_id}");
        }
    });
}

async fn resume_orchestration(state: Arc<AppState>, task_id: Uuid) -> anyhow::Result<()> {
    let task = state
        .get_task(task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task not found while resuming orchestration: {task_id}"))?;
    let mut run = state
        .get_orchestration_run(task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("orchestration run not found for task {task_id}"))?;
    let plan: OrchestrationPlan = serde_json::from_str(&run.plan_json)
        .map_err(|error| anyhow::anyhow!("invalid persisted orchestration plan: {error}"))?;

    if run.status == OrchestrationRunStatus::Completed {
        return Ok(());
    }

    run.status = OrchestrationRunStatus::Running;
    run.waiting_transaction_id = None;
    run.updated_at_unix_ms = unix_timestamp_ms();
    state.upsert_orchestration_run(run.clone()).await?;

    state
        .update_task(
            task_id,
            TaskStatus::Running,
            "orchestration execution started",
            None,
        )
        .await?;
    state
        .record_task_event(
            task_id,
            "orchestration_started",
            format!(
                "executing orchestration from step {}",
                run.next_step_index + 1
            ),
        )
        .await?;

    for (index, step) in plan
        .steps
        .into_iter()
        .enumerate()
        .skip(run.next_step_index as usize)
    {
        let context = TemplateContext {
            task_id,
            task_name: &task.name,
            task_instruction: &task.instruction,
            last_result: run.last_result.as_ref(),
            step_index: index,
        };
        let summary = step.summary(&context)?;
        state
            .record_task_event(
                task_id,
                "orchestration_step_started",
                format!("step {}: {summary}", index + 1),
            )
            .await?;

        match execute_step(&state, step, &context).await {
            Ok(StepExecution::Completed(result)) => {
                let detail = summarize_value(&result);
                run.next_step_index =
                    u32::try_from(index + 1).map_err(|_| anyhow::anyhow!("step index overflow"))?;
                run.last_result = Some(result.clone());
                run.status = OrchestrationRunStatus::Running;
                run.waiting_transaction_id = None;
                run.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_orchestration_run(run.clone()).await?;
                state
                    .record_task_event(
                        task_id,
                        "orchestration_step_completed",
                        format!("step {} completed: {detail}", index + 1),
                    )
                    .await?;
            }
            Ok(StepExecution::PausedForPayment {
                transaction_id,
                payment_result,
            }) => {
                run.next_step_index =
                    u32::try_from(index + 1).map_err(|_| anyhow::anyhow!("step index overflow"))?;
                run.last_result = Some(payment_result);
                run.status = OrchestrationRunStatus::WaitingPaymentAuthorization;
                run.waiting_transaction_id = Some(transaction_id);
                run.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_orchestration_run(run).await?;
                state
                    .record_task_event(
                        task_id,
                        "orchestration_paused_for_payment",
                        format!(
                            "step {} paused until AP2 transaction {} is authorized",
                            index + 1,
                            transaction_id
                        ),
                    )
                    .await?;
                return Ok(());
            }
            Err(error) => {
                let detail = format!("step {} failed: {error:#}", index + 1);
                run.status = OrchestrationRunStatus::Failed;
                run.updated_at_unix_ms = unix_timestamp_ms();
                state.upsert_orchestration_run(run).await?;
                state
                    .update_task(task_id, TaskStatus::Failed, &detail, None)
                    .await?;
                state
                    .record_task_event(task_id, "orchestration_step_failed", &detail)
                    .await?;
                return Err(error);
            }
        }
    }

    run.status = OrchestrationRunStatus::Completed;
    run.waiting_transaction_id = None;
    run.updated_at_unix_ms = unix_timestamp_ms();
    state.upsert_orchestration_run(run).await?;
    state
        .update_task(
            task_id,
            TaskStatus::Completed,
            "orchestration completed successfully",
            None,
        )
        .await?;
    state
        .record_task_event(
            task_id,
            "orchestration_completed",
            "all orchestration steps completed successfully",
        )
        .await?;
    Ok(())
}
