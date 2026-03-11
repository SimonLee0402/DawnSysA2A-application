use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::error;
use uuid::Uuid;

use crate::{
    ap2::{self, PaymentRequest, PaymentResponse},
    app_state::{
        AppState, ApprovalRequestKind, ApprovalRequestRecord, ApprovalRequestStatus,
        NodeCommandRecord, PaymentRecord,
    },
    control_plane,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalListQuery {
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalDecisionRequest {
    actor: String,
    decision: String,
    reason: Option<String>,
    mcu_public_did: Option<String>,
    mcu_signature: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalDetailResponse {
    approval: ApprovalRequestRecord,
    node_command: Option<NodeCommandRecord>,
    payment: Option<PaymentRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalDecisionResponse {
    approval: ApprovalRequestRecord,
    node_command: Option<NodeCommandRecord>,
    payment: Option<PaymentRecord>,
    payment_response: Option<PaymentResponse>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_approvals))
        .route("/:approval_id", get(get_approval))
        .route("/:approval_id/decision", post(decide_approval))
}

async fn list_approvals(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ApprovalListQuery>,
) -> Result<Json<Vec<ApprovalRequestRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_approval_requests(parse_status(query.status.as_deref())?)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_approval(
    State(state): State<Arc<AppState>>,
    Path(approval_id): Path<Uuid>,
) -> Result<Json<ApprovalDetailResponse>, (StatusCode, Json<Value>)> {
    let approval = state
        .get_approval_request(approval_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("approval request not found"))?;
    let (node_command, payment) = load_reference_details(&state, &approval).await?;
    Ok(Json(ApprovalDetailResponse {
        approval,
        node_command,
        payment,
    }))
}

async fn decide_approval(
    State(state): State<Arc<AppState>>,
    Path(approval_id): Path<Uuid>,
    Json(request): Json<ApprovalDecisionRequest>,
) -> Result<Json<ApprovalDecisionResponse>, (StatusCode, Json<Value>)> {
    let approval = state
        .get_approval_request(approval_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("approval request not found"))?;
    if approval.status != ApprovalRequestStatus::Pending {
        return Err(bad_request(anyhow::anyhow!(
            "approval request is already resolved"
        )));
    }

    let normalized_decision = request.decision.to_ascii_lowercase();
    let response = match approval.kind {
        ApprovalRequestKind::NodeCommand => {
            let command_id = Uuid::parse_str(&approval.reference_id)
                .map_err(|_| bad_request(anyhow::anyhow!("invalid node command id")))?;
            let node_command = match normalized_decision.as_str() {
                "approve" => control_plane::approve_pending_node_command(
                    &state,
                    command_id,
                    &request.actor,
                    request.reason.as_deref(),
                )
                .await
                .map_err(service_error)?,
                "reject" => control_plane::reject_pending_node_command(
                    &state,
                    command_id,
                    &request.actor,
                    request
                        .reason
                        .as_deref()
                        .unwrap_or("rejected by operator"),
                )
                .await
                .map_err(service_error)?,
                _ => return Err(bad_request(anyhow::anyhow!("decision must be approve or reject"))),
            };
            let approval = state
                .get_approval_request(approval_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| not_found("approval request not found after update"))?;
            ApprovalDecisionResponse {
                approval,
                node_command: Some(node_command),
                payment: None,
                payment_response: None,
            }
        }
        ApprovalRequestKind::Payment => {
            let transaction_id = Uuid::parse_str(&approval.reference_id)
                .map_err(|_| bad_request(anyhow::anyhow!("invalid payment transaction id")))?;
            let payment_response = match normalized_decision.as_str() {
                "approve" => ap2::submit_signed_payment_authorization(
                    &state,
                    PaymentRequest {
                        transaction_id: Some(transaction_id),
                        task_id: approval.task_id,
                        mandate_id: Uuid::nil(),
                        amount: 0.0,
                        description: approval.summary.clone(),
                        mcu_public_did: request.mcu_public_did.clone(),
                        mcu_signature: request.mcu_signature.clone(),
                    },
                )
                .await
                .map_err(service_error)?,
                "reject" => ap2::reject_payment_authorization(
                    &state,
                    transaction_id,
                    &request.actor,
                    request
                        .reason
                        .as_deref()
                        .unwrap_or("rejected by operator"),
                )
                .await
                .map_err(service_error)?,
                _ => return Err(bad_request(anyhow::anyhow!("decision must be approve or reject"))),
            };
            let approval = state
                .get_approval_request(approval_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| not_found("approval request not found after update"))?;
            let payment = state
                .get_payment(transaction_id)
                .await
                .map_err(internal_error)?;
            ApprovalDecisionResponse {
                approval,
                node_command: None,
                payment,
                payment_response: Some(payment_response),
            }
        }
    };

    Ok(Json(response))
}

async fn load_reference_details(
    state: &Arc<AppState>,
    approval: &ApprovalRequestRecord,
) -> Result<(Option<NodeCommandRecord>, Option<PaymentRecord>), (StatusCode, Json<Value>)> {
    match approval.kind {
        ApprovalRequestKind::NodeCommand => {
            let command_id = Uuid::parse_str(&approval.reference_id)
                .map_err(|_| bad_request(anyhow::anyhow!("invalid node command id")))?;
            let command = state.get_node_command(command_id).await.map_err(internal_error)?;
            Ok((command, None))
        }
        ApprovalRequestKind::Payment => {
            let transaction_id = Uuid::parse_str(&approval.reference_id)
                .map_err(|_| bad_request(anyhow::anyhow!("invalid payment transaction id")))?;
            let payment = state.get_payment(transaction_id).await.map_err(internal_error)?;
            Ok((None, payment))
        }
    }
}

fn parse_status(raw: Option<&str>) -> Result<Option<ApprovalRequestStatus>, (StatusCode, Json<Value>)> {
    match raw {
        None => Ok(None),
        Some("pending") => Ok(Some(ApprovalRequestStatus::Pending)),
        Some("approved") => Ok(Some(ApprovalRequestStatus::Approved)),
        Some("rejected") => Ok(Some(ApprovalRequestStatus::Rejected)),
        Some(_) => Err(bad_request(anyhow::anyhow!(
            "approval status must be pending, approved, or rejected"
        ))),
    }
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
}

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error.to_string() })))
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    let status = if message.contains("not found")
        || message.contains("unknown transactionId")
        || message.contains("unknown node command")
    {
        StatusCode::NOT_FOUND
    } else if message.contains("required")
        || message.contains("invalid")
        || message.contains("must be")
        || message.contains("already resolved")
        || message.contains("not pending approval")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        error!(?error, "Approval-center service failure");
    }
    (status, Json(json!({ "error": message })))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "Approval-center persistence failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal persistence error"
        })),
    )
}

