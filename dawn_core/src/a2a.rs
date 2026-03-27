use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    agent_cards::{self, InvokeAgentCardRequest, RemoteSettlementRequest},
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
    pub messages: Vec<A2aMessage>,
    pub artifacts: Vec<A2aArtifact>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetailResponse {
    pub task: StoredTask,
    pub events: Vec<TaskEventRecord>,
    pub messages: Vec<A2aMessage>,
    pub artifacts: Vec<A2aArtifact>,
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

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/task", post(create_task))
        .route("/tasks", get(list_tasks))
        .route("/task/:task_id", get(get_task))
        .route("/task/:task_id/events", get(get_task_events))
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
    let task = state
        .get_task(task_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("task not found"))?;
    let events = state.task_events(task_id).await.map_err(internal_error)?;
    let messages = build_task_messages(&task, &events);
    let artifacts = build_task_artifacts(&task, &events);
    Ok(Json(TaskDetailResponse {
        task,
        events,
        messages,
        artifacts,
    }))
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
    let messages = build_task_messages(&task, &events);
    let artifacts = build_task_artifacts(&task, &events);

    Ok(TaskResponse {
        task,
        sandbox_status,
        messages,
        artifacts,
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
    use serde_json::json;

    use super::{
        A2aMessageRole, A2aPart, OrchestrationStep, StoredTask, TaskEventRecord, TaskStatus,
        TemplateContext, WasmInstructionBinding, build_task_artifacts, build_task_messages,
        extract_text_from_value, parse_orchestration_plan, parse_wasm_instruction,
        resolve_json_templates, resolve_template_string,
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
