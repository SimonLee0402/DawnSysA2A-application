use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    a2a,
    app_state::{AppState, PaymentRecord, PaymentStatus, TaskStatus, unix_timestamp_ms},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AP2Mandate {
    pub mandate_id: Uuid,
    pub payer_did: String,
    pub merchant_did: String,
    pub max_amount: f64,
    pub currency: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VerifiableCredential<T> {
    pub credential_type: String,
    pub issuer_did: String,
    pub payload: T,
    pub signature: String,
}

pub type AP2MandateVC = VerifiableCredential<AP2Mandate>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequest {
    pub transaction_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub mandate_id: Uuid,
    pub amount: f64,
    pub description: String,
    pub mcu_public_did: Option<String>,
    pub mcu_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentResponse {
    pub status: PaymentStatus,
    pub transaction_id: Uuid,
    pub verification_message: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/authorize", post(authorize_payment))
        .route("/transactions", get(list_transactions))
        .route("/transactions/:transaction_id", get(get_transaction))
}

async fn status() -> &'static str {
    "AP2 Endpoints Operational."
}

async fn list_transactions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PaymentRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_payments()
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(transaction_id): Path<Uuid>,
) -> Result<Json<PaymentRecord>, (StatusCode, Json<Value>)> {
    state
        .get_payment(transaction_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("payment transaction not found"))
}

async fn authorize_payment(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PaymentRequest>,
) -> Result<Json<PaymentResponse>, (StatusCode, Json<serde_json::Value>)> {
    info!(
        "Received AP2 payment request. transaction_id={:?} mandate_id={}",
        req.transaction_id, req.mandate_id
    );

    if req.transaction_id.is_none() && req.mcu_signature.is_none() {
        return request_payment_authorization(&state, req)
            .await
            .map(Json)
            .map_err(service_error);
    }

    process_signed_payment_authorization(&state, req)
        .await
        .map(Json)
        .map_err(service_error)
}

pub async fn request_payment_authorization(
    state: &AppState,
    req: PaymentRequest,
) -> anyhow::Result<PaymentResponse> {
    let transaction_id = Uuid::new_v4();
    let payment = PaymentRecord {
        transaction_id,
        task_id: req.task_id,
        mandate_id: req.mandate_id,
        amount: req.amount,
        description: req.description.clone(),
        status: PaymentStatus::PendingPhysicalAuth,
        verification_message: "waiting for hardware approval".to_string(),
        mcu_public_did: None,
        created_at_unix_ms: unix_timestamp_ms(),
        updated_at_unix_ms: unix_timestamp_ms(),
    };
    state.upsert_payment(payment).await?;
    if let Some(task_id) = req.task_id {
        state
            .update_task(
                task_id,
                TaskStatus::WaitingPaymentAuthorization,
                "task paused until AP2 physical authorization completes",
                Some(transaction_id),
            )
            .await?;
        state
            .record_task_event(
                task_id,
                "payment_authorization_requested",
                format!("payment transaction {transaction_id} is pending hardware approval"),
            )
            .await?;
    }

    Ok(PaymentResponse {
        status: PaymentStatus::PendingPhysicalAuth,
        transaction_id,
        verification_message: "hardware approval required".to_string(),
    })
}

async fn process_signed_payment_authorization(
    state: &Arc<AppState>,
    req: PaymentRequest,
) -> anyhow::Result<PaymentResponse> {
    let transaction_id = req.transaction_id.ok_or_else(|| {
        anyhow::anyhow!("transactionId is required when submitting a signed AP2 authorization")
    })?;

    let mut payment = state
        .get_payment(transaction_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown transactionId"))?;

    if payment.status != PaymentStatus::PendingPhysicalAuth {
        return Ok(PaymentResponse {
            status: payment.status,
            transaction_id,
            verification_message: payment.verification_message,
        });
    }

    let public_did = req
        .mcu_public_did
        .clone()
        .ok_or_else(|| anyhow::anyhow!("mcuPublicDid is required for signed AP2 authorization"))?;
    let signature = req
        .mcu_signature
        .clone()
        .ok_or_else(|| anyhow::anyhow!("mcuSignature is required for signed AP2 authorization"))?;

    let payload = signature_payload(
        transaction_id,
        payment.mandate_id,
        payment.amount,
        &payment.description,
    );
    let resume_task_id = payment.task_id;
    let verification_message = match verify_signature(&public_did, &signature, &payload) {
        Ok(()) => {
            payment.status = PaymentStatus::Authorized;
            payment.verification_message =
                "hardware signature verified; payment authorized".to_string();
            payment.mcu_public_did = Some(public_did);
            payment.updated_at_unix_ms = unix_timestamp_ms();
            if let Some(task_id) = payment.task_id {
                state
                    .update_task(
                        task_id,
                        TaskStatus::Queued,
                        "AP2 authorization completed; task can proceed to execution",
                        Some(transaction_id),
                    )
                    .await?;
                state
                    .record_task_event(
                        task_id,
                        "payment_authorized",
                        format!(
                            "payment transaction {transaction_id} was authorized by hardware signature"
                        ),
                    )
                    .await?;
            }
            payment.verification_message.clone()
        }
        Err(error) => {
            payment.status = PaymentStatus::Rejected;
            payment.verification_message = format!("hardware signature rejected: {error}");
            payment.mcu_public_did = Some(public_did);
            payment.updated_at_unix_ms = unix_timestamp_ms();
            if let Some(task_id) = payment.task_id {
                state
                    .update_task(
                        task_id,
                        TaskStatus::Failed,
                        "AP2 authorization failed during hardware signature verification",
                        Some(transaction_id),
                    )
                    .await?;
                state
                    .record_task_event(
                        task_id,
                        "payment_rejected",
                        format!(
                            "payment transaction {transaction_id} was rejected by hardware signature verification"
                        ),
                    )
                    .await?;
            }
            payment.verification_message.clone()
        }
    };

    let status = payment.status;
    let payment = state.upsert_payment(payment).await?;
    crate::agent_cards::sync_remote_settlement_from_payment(state, &payment).await?;
    if status == PaymentStatus::Authorized {
        if let Some(task_id) = resume_task_id {
            a2a::spawn_orchestration_resume(state.clone(), task_id);
        }
    }

    Ok(PaymentResponse {
        status,
        transaction_id,
        verification_message,
    })
}

pub fn signature_payload(
    transaction_id: Uuid,
    mandate_id: Uuid,
    amount: f64,
    description: &str,
) -> String {
    format!("{transaction_id}:{mandate_id}:{amount:.4}:{description}")
}

fn verify_signature(public_did: &str, signature_hex: &str, payload: &str) -> anyhow::Result<()> {
    let public_key_hex = public_did
        .strip_prefix("did:dawn:mcu:")
        .ok_or_else(|| anyhow::anyhow!("unexpected DID format"))?;
    let public_key_bytes = decode_hex(public_key_hex)?;
    let public_key: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)?;

    let signature_bytes = decode_hex(signature_hex)?;
    let signature: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let signature = Signature::from_bytes(&signature);

    verifying_key.verify(payload.as_bytes(), &signature)?;
    Ok(())
}

fn decode_hex(raw: &str) -> anyhow::Result<Vec<u8>> {
    Ok(hex::decode(raw)?)
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "AP2 persistence failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal persistence error"
        })),
    )
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    let status = if message.contains("unknown transactionId") {
        StatusCode::NOT_FOUND
    } else if message.contains("required") || message.contains("unexpected DID format") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        error!(?error, "AP2 service failure");
    }
    (status, Json(json!({ "error": message })))
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    #[test]
    fn verifies_valid_signature() {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let transaction_id = Uuid::new_v4();
        let mandate_id = Uuid::new_v4();
        let payload = signature_payload(transaction_id, mandate_id, 42.5, "deploy agent");
        let signature = signing_key.sign(payload.as_bytes());
        let did = format!(
            "did:dawn:mcu:{}",
            hex::encode(signing_key.verifying_key().as_bytes())
        );

        assert!(verify_signature(&did, &hex::encode(signature.to_bytes()), &payload).is_ok());
    }
}
