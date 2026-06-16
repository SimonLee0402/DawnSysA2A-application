use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    FromRow, Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tokio::sync::{Notify, RwLock, broadcast, mpsc};
use uuid::Uuid;
use wasmtime::Engine;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Accepted,
    AwaitingSkillBinding,
    WaitingPaymentAuthorization,
    Queued,
    Running,
    Completed,
    Failed,
}

impl TaskStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::AwaitingSkillBinding => "awaiting_skill_binding",
            Self::WaitingPaymentAuthorization => "waiting_payment_authorization",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "accepted" => Ok(Self::Accepted),
            "awaiting_skill_binding" => Ok(Self::AwaitingSkillBinding),
            "waiting_payment_authorization" => Ok(Self::WaitingPaymentAuthorization),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("unknown task status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StoredTask {
    pub task_id: Uuid,
    pub parent_task_id: Option<Uuid>,
    pub name: String,
    pub instruction: String,
    pub status: TaskStatus,
    pub linked_payment_id: Option<Uuid>,
    pub last_update_reason: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventRecord {
    pub event_type: String,
    pub detail: String,
    pub task_id: Uuid,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    PendingPhysicalAuth,
    Authorized,
    Rejected,
}

impl PaymentStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::PendingPhysicalAuth => "pending_physical_auth",
            Self::Authorized => "authorized",
            Self::Rejected => "rejected",
        }
    }

    pub(crate) fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending_physical_auth" => Ok(Self::PendingPhysicalAuth),
            "authorized" => Ok(Self::Authorized),
            "rejected" => Ok(Self::Rejected),
            _ => Err(anyhow!("unknown payment status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRecord {
    pub transaction_id: Uuid,
    pub task_id: Option<Uuid>,
    pub mandate_id: Uuid,
    pub amount: f64,
    pub description: String,
    pub status: PaymentStatus,
    pub verification_message: String,
    pub mcu_public_did: Option<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequestKind {
    NodeCommand,
    Payment,
}

impl ApprovalRequestKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::NodeCommand => "node_command",
            Self::Payment => "payment",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "node_command" => Ok(Self::NodeCommand),
            "payment" => Ok(Self::Payment),
            _ => Err(anyhow!("unknown approval request kind '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequestStatus {
    Pending,
    Approved,
    Rejected,
}

impl ApprovalRequestStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            _ => Err(anyhow!("unknown approval request status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestRecord {
    pub approval_id: Uuid,
    pub kind: ApprovalRequestKind,
    pub title: String,
    pub summary: String,
    pub task_id: Option<Uuid>,
    pub reference_id: String,
    pub status: ApprovalRequestStatus,
    pub actor: Option<String>,
    pub decision_reason: Option<String>,
    pub decision_payload: Option<Value>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndUserApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

impl EndUserApprovalStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            _ => Err(anyhow!("unknown end-user approval status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EndUserApprovalSessionRecord {
    pub session_id: Uuid,
    pub approval_id: Uuid,
    pub approval_kind: ApprovalRequestKind,
    pub task_id: Option<Uuid>,
    pub transaction_id: Option<Uuid>,
    pub platform: Option<String>,
    pub chat_id: Option<String>,
    pub sender_id: Option<String>,
    pub sender_display: Option<String>,
    pub token_hint: String,
    pub status: EndUserApprovalStatus,
    pub expires_at_unix_ms: Option<u128>,
    pub decided_at_unix_ms: Option<u128>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
    #[serde(skip_serializing, skip_deserializing)]
    pub approval_token_hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketplacePeerSyncStatus {
    Pending,
    Healthy,
    Unreachable,
    InvalidCatalog,
}

impl MarketplacePeerSyncStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Healthy => "healthy",
            Self::Unreachable => "unreachable",
            Self::InvalidCatalog => "invalid_catalog",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "healthy" => Ok(Self::Healthy),
            "unreachable" => Ok(Self::Unreachable),
            "invalid_catalog" => Ok(Self::InvalidCatalog),
            _ => Err(anyhow!("unknown marketplace peer sync status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePeerRecord {
    pub peer_id: String,
    pub display_name: String,
    pub base_url: String,
    pub catalog_url: String,
    pub enabled: bool,
    pub trust_enabled: bool,
    pub sync_status: MarketplacePeerSyncStatus,
    pub last_sync_error: Option<String>,
    pub last_synced_at_unix_ms: Option<u128>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatIngressStatus {
    Received,
    PendingApproval,
    TaskCreated,
    Replied,
    Ignored,
    Failed,
}

impl ChatIngressStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::PendingApproval => "pending_approval",
            Self::TaskCreated => "task_created",
            Self::Replied => "replied",
            Self::Ignored => "ignored",
            Self::Failed => "failed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "received" => Ok(Self::Received),
            "pending_approval" => Ok(Self::PendingApproval),
            "task_created" => Ok(Self::TaskCreated),
            "replied" => Ok(Self::Replied),
            "ignored" => Ok(Self::Ignored),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("unknown chat ingress status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatIngressEventRecord {
    pub ingress_id: Uuid,
    pub platform: String,
    pub event_type: String,
    pub chat_id: Option<String>,
    pub sender_id: Option<String>,
    pub sender_display: Option<String>,
    pub text: String,
    pub raw_payload: Value,
    pub linked_task_id: Option<Uuid>,
    pub reply_text: Option<String>,
    pub status: ChatIngressStatus,
    pub error: Option<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentExperienceRecord {
    pub experience_id: Uuid,
    pub source: String,
    pub scope: String,
    pub task_kind: String,
    pub input_summary: String,
    pub action_summary: String,
    pub outcome: String,
    pub lesson: String,
    pub reusable_hint: Option<String>,
    pub evidence: Value,
    pub tags: Vec<String>,
    pub risk_level: String,
    pub related_task_id: Option<Uuid>,
    pub related_ingress_id: Option<Uuid>,
    pub created_by: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Default, Clone)]
pub struct AgentExperienceListFilter {
    pub limit: Option<u32>,
    pub source: Option<String>,
    pub outcome: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillProposalRecord {
    pub proposal_id: Uuid,
    pub proposal_key: String,
    pub title: String,
    pub summary: String,
    pub rationale: String,
    pub suggested_skill_id: String,
    pub source: String,
    pub status: String,
    pub confidence: f64,
    pub evidence: Value,
    pub tags: Vec<String>,
    pub risk_level: String,
    pub created_by: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Default, Clone)]
pub struct SkillProposalListFilter {
    pub limit: Option<u32>,
    pub status: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillImplementationPlanRecord {
    pub plan_id: Uuid,
    pub proposal_id: Uuid,
    pub suggested_skill_id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub steps: Value,
    pub acceptance_criteria: Value,
    pub guardrails: Value,
    pub created_by: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Default, Clone)]
pub struct SkillImplementationPlanListFilter {
    pub limit: Option<u32>,
    pub status: Option<String>,
    pub proposal_id: Option<Uuid>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillImplementationRunRecord {
    pub run_id: Uuid,
    pub plan_id: Uuid,
    pub proposal_id: Uuid,
    pub suggested_skill_id: String,
    pub status: String,
    pub execution_mode: String,
    pub change_package: Value,
    pub verification: Value,
    pub rollback: Value,
    pub guardrails: Value,
    pub created_by: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Default, Clone)]
pub struct SkillImplementationRunListFilter {
    pub limit: Option<u32>,
    pub status: Option<String>,
    pub plan_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillImplementationExecutionRecord {
    pub execution_id: Uuid,
    pub run_id: Uuid,
    pub plan_id: Uuid,
    pub proposal_id: Uuid,
    pub suggested_skill_id: String,
    pub status: String,
    pub executor: String,
    pub preflight_report: Value,
    pub command_plan: Value,
    pub result: Value,
    pub guardrails: Value,
    pub created_by: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Default, Clone)]
pub struct SkillImplementationExecutionListFilter {
    pub limit: Option<u32>,
    pub status: Option<String>,
    pub run_id: Option<Uuid>,
    pub plan_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillImplementationPatchRecord {
    pub patch_id: Uuid,
    pub execution_id: Uuid,
    pub run_id: Uuid,
    pub plan_id: Uuid,
    pub proposal_id: Uuid,
    pub suggested_skill_id: String,
    pub status: String,
    pub patch_kind: String,
    pub summary: String,
    pub changed_files: Value,
    pub patch_manifest: Value,
    pub rollback_plan: Value,
    pub verification_evidence: Value,
    pub guardrails: Value,
    pub created_by: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Default, Clone)]
pub struct SkillImplementationPatchListFilter {
    pub limit: Option<u32>,
    pub status: Option<String>,
    pub execution_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub plan_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannelIdentityStatus {
    Pending,
    Paired,
    Rejected,
    Blocked,
}

impl ChatChannelIdentityStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Paired => "paired",
            Self::Rejected => "rejected",
            Self::Blocked => "blocked",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "paired" => Ok(Self::Paired),
            "rejected" => Ok(Self::Rejected),
            "blocked" => Ok(Self::Blocked),
            _ => Err(anyhow!("unknown chat channel identity status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelIdentityRecord {
    pub platform: String,
    pub identity_key: String,
    pub chat_id: Option<String>,
    pub sender_id: Option<String>,
    pub sender_display: Option<String>,
    pub pairing_code: Option<String>,
    pub dm_policy: String,
    pub decision_reason: Option<String>,
    pub last_ingress_id: Option<Uuid>,
    pub status: ChatChannelIdentityStatus,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatAutomationMode {
    Chat,
    Observe,
    Assist,
    Autopilot,
}

impl ChatAutomationMode {
    fn as_db(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Observe => "observe",
            Self::Assist => "assist",
            Self::Autopilot => "autopilot",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "chat" => Ok(Self::Chat),
            "observe" => Ok(Self::Observe),
            "assist" => Ok(Self::Assist),
            "autopilot" => Ok(Self::Autopilot),
            _ => Err(anyhow!("unknown chat automation mode '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatAutomationModeRecord {
    pub platform: String,
    pub chat_key: String,
    pub chat_id: Option<String>,
    pub sender_id: Option<String>,
    pub mode: ChatAutomationMode,
    pub updated_by: Option<String>,
    pub reason: Option<String>,
    pub last_ingress_id: Option<Uuid>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeSessionStatus {
    Registered,
    Connected,
    Disconnected,
}

impl NodeSessionStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "registered" => Ok(Self::Registered),
            "connected" => Ok(Self::Connected),
            "disconnected" => Ok(Self::Disconnected),
            _ => Err(anyhow!("unknown node session status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeRecord {
    pub node_id: String,
    pub display_name: String,
    pub transport: String,
    pub capabilities: Vec<String>,
    pub attestation_issuer_did: Option<String>,
    pub attestation_signature_hex: Option<String>,
    pub attestation_document_hash: Option<String>,
    pub attestation_issued_at_unix_ms: Option<u128>,
    pub attestation_verified: bool,
    pub attestation_verified_at_unix_ms: Option<u128>,
    pub attestation_error: Option<String>,
    pub status: NodeSessionStatus,
    pub connected: bool,
    pub last_seen_unix_ms: u128,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeAttestationState {
    pub issuer_did: String,
    pub signature_hex: String,
    pub document_hash: String,
    pub issued_at_unix_ms: u128,
    pub verified: bool,
    pub verified_at_unix_ms: Option<u128>,
    pub attestation_error: Option<String>,
    pub verified_capabilities: Option<Vec<String>>,
    pub display_name: Option<String>,
    pub transport: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeCommandStatus {
    PendingApproval,
    Queued,
    Dispatched,
    Succeeded,
    Failed,
}

impl NodeCommandStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::PendingApproval => "pending_approval",
            Self::Queued => "queued",
            Self::Dispatched => "dispatched",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending_approval" => Ok(Self::PendingApproval),
            "queued" => Ok(Self::Queued),
            "dispatched" => Ok(Self::Dispatched),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("unknown node command status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeCommandRecord {
    pub command_id: Uuid,
    pub node_id: String,
    pub command_type: String,
    pub payload: Value,
    pub status: NodeCommandStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRolloutStatus {
    Pending,
    Sent,
    Acknowledged,
    Rejected,
}

impl NodeRolloutStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Acknowledged => "acknowledged",
            Self::Rejected => "rejected",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "sent" => Ok(Self::Sent),
            "acknowledged" => Ok(Self::Acknowledged),
            "rejected" => Ok(Self::Rejected),
            _ => Err(anyhow!("unknown node rollout status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeRolloutRecord {
    pub node_id: String,
    pub bundle_hash: String,
    pub policy_version: u32,
    pub policy_document_hash: Option<String>,
    pub skill_distribution_hash: String,
    pub status: NodeRolloutStatus,
    pub last_error: Option<String>,
    pub last_sent_at_unix_ms: u128,
    pub last_ack_at_unix_ms: Option<u128>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationRunStatus {
    Queued,
    Running,
    WaitingPaymentAuthorization,
    Completed,
    Failed,
}

impl OrchestrationRunStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingPaymentAuthorization => "waiting_payment_authorization",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "waiting_payment_authorization" => Ok(Self::WaitingPaymentAuthorization),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("unknown orchestration run status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationRunRecord {
    pub task_id: Uuid,
    pub plan_json: String,
    pub next_step_index: u32,
    pub last_result: Option<Value>,
    pub waiting_transaction_id: Option<Uuid>,
    pub status: OrchestrationRunStatus,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolicyProfileRecord {
    pub policy_id: String,
    pub version: u32,
    pub issuer_did: Option<String>,
    pub allow_shell_exec: bool,
    pub allowed_model_providers: Vec<String>,
    pub allowed_chat_platforms: Vec<String>,
    pub max_payment_amount: Option<f64>,
    pub signature_hex: Option<String>,
    pub document_hash: Option<String>,
    pub issued_at_unix_ms: Option<u128>,
    pub updated_reason: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolicyAuditEventRecord {
    pub audit_id: i64,
    pub policy_id: String,
    pub version: u32,
    pub actor: String,
    pub summary: String,
    pub snapshot: Value,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PolicyTrustRootRecord {
    pub issuer_did: String,
    pub label: String,
    pub public_key_hex: String,
    pub updated_by: String,
    pub updated_reason: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeTrustRootRecord {
    pub issuer_did: String,
    pub label: String,
    pub public_key_hex: String,
    pub updated_by: String,
    pub updated_reason: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublisherTrustRootRecord {
    pub issuer_did: String,
    pub label: String,
    pub public_key_hex: String,
    pub updated_by: String,
    pub updated_reason: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

pub type NodeSessionSender = mpsc::UnboundedSender<String>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleStreamEvent {
    pub channel: String,
    pub entity_id: Option<String>,
    pub status: Option<String>,
    pub detail: String,
    pub created_at_unix_ms: u128,
}

pub struct AppState {
    pub engine: Engine,
    pool: SqlitePool,
    node_sessions: RwLock<HashMap<String, NodeSessionSender>>,
    console_events: broadcast::Sender<ConsoleStreamEvent>,
    console_event_history: Mutex<VecDeque<ConsoleStreamEvent>>,
    delivery_outbox_wakeup: Notify,
}

impl AppState {
    pub async fn new(engine: Engine) -> anyhow::Result<Arc<Self>> {
        let database_url = std::env::var("DAWN_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/dawn_core.db".to_string());
        Self::new_with_database_url(engine, &database_url).await
    }

    pub async fn new_with_database_url(
        engine: Engine,
        database_url: impl AsRef<str>,
    ) -> anyhow::Result<Arc<Self>> {
        let database_url = database_url.as_ref();
        ensure_sqlite_database_parent(database_url)?;

        let connect_options = database_url
            .parse::<SqliteConnectOptions>()
            .context("failed to parse SQLite connection string")?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connect_options)
            .await
            .context("failed to open SQLite database")?;

        migrate(&pool).await?;
        ensure_default_policy_profile(&pool).await?;
        let (console_events, _) = broadcast::channel(512);

        let state = Arc::new(Self {
            engine,
            pool,
            node_sessions: RwLock::new(HashMap::new()),
            console_events,
            console_event_history: Mutex::new(VecDeque::with_capacity(256)),
            delivery_outbox_wakeup: Notify::new(),
        });
        crate::agent_cards::spawn_delivery_outbox_worker(state.clone());
        Ok(state)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn subscribe_console_events(&self) -> broadcast::Receiver<ConsoleStreamEvent> {
        self.console_events.subscribe()
    }

    pub fn recent_console_events(&self, limit: usize) -> Vec<ConsoleStreamEvent> {
        let history = self
            .console_event_history
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        history.iter().rev().take(limit.max(1)).cloned().collect()
    }

    pub fn emit_console_event(
        &self,
        channel: impl Into<String>,
        entity_id: Option<String>,
        status: Option<String>,
        detail: impl Into<String>,
    ) {
        let event = ConsoleStreamEvent {
            channel: channel.into(),
            entity_id,
            status,
            detail: detail.into(),
            created_at_unix_ms: unix_timestamp_ms(),
        };
        let _ = self.console_events.send(event.clone());
        let mut history = self
            .console_event_history
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        history.push_back(event);
        while history.len() > 200 {
            history.pop_front();
        }
    }

    pub fn wake_delivery_outbox(&self) {
        self.delivery_outbox_wakeup.notify_one();
    }

    pub async fn wait_for_delivery_outbox(&self) {
        self.delivery_outbox_wakeup.notified().await;
    }

    pub async fn insert_task(&self, task: StoredTask) -> anyhow::Result<StoredTask> {
        save_task(&self.pool, &task).await?;
        self.emit_console_event(
            "task",
            Some(task.task_id.to_string()),
            Some(task.status.as_db().to_string()),
            format!("task '{}' inserted", task.name),
        );
        Ok(task)
    }

    pub async fn list_tasks(&self) -> anyhow::Result<Vec<StoredTask>> {
        let rows = sqlx::query_as::<_, TaskRow>(
            r#"
            SELECT
                task_id,
                parent_task_id,
                name,
                instruction,
                status,
                linked_payment_id,
                last_update_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM tasks
            ORDER BY created_at_unix_ms DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list tasks")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_task(&self, task_id: Uuid) -> anyhow::Result<Option<StoredTask>> {
        let row = sqlx::query_as::<_, TaskRow>(
            r#"
            SELECT
                task_id,
                parent_task_id,
                name,
                instruction,
                status,
                linked_payment_id,
                last_update_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM tasks
            WHERE task_id = ?1
            "#,
        )
        .bind(task_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch task")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn update_task(
        &self,
        task_id: Uuid,
        status: TaskStatus,
        reason: impl Into<String>,
        linked_payment_id: Option<Uuid>,
    ) -> anyhow::Result<Option<StoredTask>> {
        let now = unix_timestamp_ms();
        let reason = reason.into();
        let result = sqlx::query(
            r#"
            UPDATE tasks
            SET
                status = ?1,
                last_update_reason = ?2,
                linked_payment_id = COALESCE(?3, linked_payment_id),
                updated_at_unix_ms = ?4
            WHERE task_id = ?5
            "#,
        )
        .bind(status.as_db())
        .bind(reason)
        .bind(linked_payment_id.map(|value| value.to_string()))
        .bind(u128_to_i64(now)?)
        .bind(task_id.to_string())
        .execute(&self.pool)
        .await
        .context("failed to update task")?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }
        let task = self.get_task(task_id).await?;
        if let Some(task) = &task {
            self.emit_console_event(
                "task",
                Some(task.task_id.to_string()),
                Some(task.status.as_db().to_string()),
                task.last_update_reason.clone(),
            );
        }
        Ok(task)
    }

    pub async fn record_task_event(
        &self,
        task_id: Uuid,
        event_type: impl Into<String>,
        detail: impl Into<String>,
    ) -> anyhow::Result<TaskEventRecord> {
        let event = TaskEventRecord {
            event_type: event_type.into(),
            detail: detail.into(),
            task_id,
            created_at_unix_ms: unix_timestamp_ms(),
        };

        sqlx::query(
            r#"
            INSERT INTO task_events (
                task_id,
                event_type,
                detail,
                created_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(event.task_id.to_string())
        .bind(&event.event_type)
        .bind(&event.detail)
        .bind(u128_to_i64(event.created_at_unix_ms)?)
        .execute(&self.pool)
        .await
        .context("failed to insert task event")?;

        self.emit_console_event(
            "task_event",
            Some(event.task_id.to_string()),
            Some(event.event_type.clone()),
            event.detail.clone(),
        );
        Ok(event)
    }

    pub async fn task_events(&self, task_id: Uuid) -> anyhow::Result<Vec<TaskEventRecord>> {
        let rows = sqlx::query_as::<_, TaskEventRow>(
            r#"
            SELECT
                task_id,
                event_type,
                detail,
                created_at_unix_ms
            FROM task_events
            WHERE task_id = ?1
            ORDER BY created_at_unix_ms ASC, rowid ASC
            "#,
        )
        .bind(task_id.to_string())
        .fetch_all(&self.pool)
        .await
        .context("failed to list task events")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn upsert_payment(&self, payment: PaymentRecord) -> anyhow::Result<PaymentRecord> {
        save_payment(&self.pool, &payment).await?;
        self.emit_console_event(
            "payment",
            Some(payment.transaction_id.to_string()),
            Some(payment.status.as_db().to_string()),
            format!("payment {:.2} {}", payment.amount, payment.description),
        );
        Ok(payment)
    }

    pub async fn list_payments(&self) -> anyhow::Result<Vec<PaymentRecord>> {
        let rows = sqlx::query_as::<_, PaymentRow>(
            r#"
            SELECT
                transaction_id,
                task_id,
                mandate_id,
                amount,
                description,
                status,
                verification_message,
                mcu_public_did,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM payments
            ORDER BY created_at_unix_ms DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list payments")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_payment(&self, transaction_id: Uuid) -> anyhow::Result<Option<PaymentRecord>> {
        let row = sqlx::query_as::<_, PaymentRow>(
            r#"
            SELECT
                transaction_id,
                task_id,
                mandate_id,
                amount,
                description,
                status,
                verification_message,
                mcu_public_did,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM payments
            WHERE transaction_id = ?1
            "#,
        )
        .bind(transaction_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch payment")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn upsert_approval_request(
        &self,
        approval: ApprovalRequestRecord,
    ) -> anyhow::Result<ApprovalRequestRecord> {
        save_approval_request(&self.pool, &approval).await?;
        self.emit_console_event(
            "approval",
            Some(approval.approval_id.to_string()),
            Some(approval.status.as_db().to_string()),
            approval.title.clone(),
        );
        Ok(approval)
    }

    pub async fn list_approval_requests(
        &self,
        status: Option<ApprovalRequestStatus>,
    ) -> anyhow::Result<Vec<ApprovalRequestRecord>> {
        let rows = if let Some(status) = status {
            sqlx::query_as::<_, ApprovalRequestRow>(
                r#"
                SELECT
                    approval_id,
                    kind,
                    title,
                    summary,
                    task_id,
                    reference_id,
                    status,
                    actor,
                    decision_reason,
                    decision_payload,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM approval_requests
                WHERE status = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
            )
            .bind(status.as_db())
            .fetch_all(&self.pool)
            .await
            .context("failed to list approval requests by status")?
        } else {
            sqlx::query_as::<_, ApprovalRequestRow>(
                r#"
                SELECT
                    approval_id,
                    kind,
                    title,
                    summary,
                    task_id,
                    reference_id,
                    status,
                    actor,
                    decision_reason,
                    decision_payload,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM approval_requests
                ORDER BY created_at_unix_ms DESC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("failed to list approval requests")?
        };

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_approval_request(
        &self,
        approval_id: Uuid,
    ) -> anyhow::Result<Option<ApprovalRequestRecord>> {
        let row = sqlx::query_as::<_, ApprovalRequestRow>(
            r#"
            SELECT
                approval_id,
                kind,
                title,
                summary,
                task_id,
                reference_id,
                status,
                actor,
                decision_reason,
                decision_payload,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM approval_requests
            WHERE approval_id = ?1
            "#,
        )
        .bind(approval_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch approval request")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_pending_approval_by_reference(
        &self,
        kind: ApprovalRequestKind,
        reference_id: &str,
    ) -> anyhow::Result<Option<ApprovalRequestRecord>> {
        let row = sqlx::query_as::<_, ApprovalRequestRow>(
            r#"
            SELECT
                approval_id,
                kind,
                title,
                summary,
                task_id,
                reference_id,
                status,
                actor,
                decision_reason,
                decision_payload,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM approval_requests
            WHERE kind = ?1 AND reference_id = ?2 AND status = ?3
            ORDER BY created_at_unix_ms DESC
            LIMIT 1
            "#,
        )
        .bind(kind.as_db())
        .bind(reference_id)
        .bind(ApprovalRequestStatus::Pending.as_db())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to fetch pending approval request for {}:{}",
                kind.as_db(),
                reference_id
            )
        })?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn upsert_end_user_approval_session(
        &self,
        session: EndUserApprovalSessionRecord,
    ) -> anyhow::Result<EndUserApprovalSessionRecord> {
        save_end_user_approval_session(&self.pool, &session).await?;
        self.emit_console_event(
            "end_user_approval",
            Some(session.session_id.to_string()),
            Some(session.status.as_db().to_string()),
            format!(
                "end-user approval session for {}",
                session
                    .sender_display
                    .clone()
                    .or(session.sender_id.clone())
                    .unwrap_or_else(|| "unknown user".to_string())
            ),
        );
        Ok(session)
    }

    pub async fn get_end_user_approval_session(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<Option<EndUserApprovalSessionRecord>> {
        let row = sqlx::query_as::<_, EndUserApprovalSessionRow>(
            r#"
            SELECT
                session_id,
                approval_id,
                approval_kind,
                task_id,
                transaction_id,
                platform,
                chat_id,
                sender_id,
                sender_display,
                approval_token_hash,
                token_hint,
                status,
                expires_at_unix_ms,
                decided_at_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM end_user_approval_sessions
            WHERE session_id = ?1
            "#,
        )
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch end-user approval session")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_pending_end_user_approval_session_by_approval(
        &self,
        approval_id: Uuid,
    ) -> anyhow::Result<Option<EndUserApprovalSessionRecord>> {
        let row = sqlx::query_as::<_, EndUserApprovalSessionRow>(
            r#"
            SELECT
                session_id,
                approval_id,
                approval_kind,
                task_id,
                transaction_id,
                platform,
                chat_id,
                sender_id,
                sender_display,
                approval_token_hash,
                token_hint,
                status,
                expires_at_unix_ms,
                decided_at_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM end_user_approval_sessions
            WHERE approval_id = ?1 AND status = ?2
            ORDER BY created_at_unix_ms DESC
            LIMIT 1
            "#,
        )
        .bind(approval_id.to_string())
        .bind(EndUserApprovalStatus::Pending.as_db())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| {
            format!("failed to fetch pending end-user approval session for {approval_id}")
        })?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_end_user_approval_session_by_token(
        &self,
        approval_token: &str,
    ) -> anyhow::Result<Option<EndUserApprovalSessionRecord>> {
        let row = sqlx::query_as::<_, EndUserApprovalSessionRow>(
            r#"
            SELECT
                session_id,
                approval_id,
                approval_kind,
                task_id,
                transaction_id,
                platform,
                chat_id,
                sender_id,
                sender_display,
                approval_token_hash,
                token_hint,
                status,
                expires_at_unix_ms,
                decided_at_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM end_user_approval_sessions
            WHERE approval_token_hash = ?1
            ORDER BY created_at_unix_ms DESC
            LIMIT 1
            "#,
        )
        .bind(hash_approval_token(approval_token))
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch end-user approval session by token")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn upsert_marketplace_peer(
        &self,
        peer: MarketplacePeerRecord,
    ) -> anyhow::Result<MarketplacePeerRecord> {
        save_marketplace_peer(&self.pool, &peer).await?;
        self.emit_console_event(
            "marketplace_peer",
            Some(peer.peer_id.clone()),
            Some(peer.sync_status.as_db().to_string()),
            format!("marketplace peer '{}' upserted", peer.display_name),
        );
        Ok(peer)
    }

    pub async fn get_marketplace_peer(
        &self,
        peer_id: &str,
    ) -> anyhow::Result<Option<MarketplacePeerRecord>> {
        let row = sqlx::query_as::<_, MarketplacePeerRow>(
            r#"
            SELECT
                peer_id,
                display_name,
                base_url,
                catalog_url,
                enabled,
                trust_enabled,
                sync_status,
                last_sync_error,
                last_synced_at_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM marketplace_peers
            WHERE peer_id = ?1
            "#,
        )
        .bind(peer_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch marketplace peer '{peer_id}'"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_marketplace_peers(&self) -> anyhow::Result<Vec<MarketplacePeerRecord>> {
        let rows = sqlx::query_as::<_, MarketplacePeerRow>(
            r#"
            SELECT
                peer_id,
                display_name,
                base_url,
                catalog_url,
                enabled,
                trust_enabled,
                sync_status,
                last_sync_error,
                last_synced_at_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM marketplace_peers
            ORDER BY display_name ASC, peer_id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list marketplace peers")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn upsert_chat_ingress_event(
        &self,
        event: ChatIngressEventRecord,
    ) -> anyhow::Result<ChatIngressEventRecord> {
        save_chat_ingress_event(&self.pool, &event).await?;
        self.emit_console_event(
            "ingress",
            Some(event.ingress_id.to_string()),
            Some(event.status.as_db().to_string()),
            format!("{} · {}", event.platform, event.event_type),
        );
        Ok(event)
    }

    pub async fn get_chat_ingress_event(
        &self,
        ingress_id: Uuid,
    ) -> anyhow::Result<Option<ChatIngressEventRecord>> {
        let row = sqlx::query_as::<_, ChatIngressEventRow>(
            r#"
            SELECT
                ingress_id,
                platform,
                event_type,
                chat_id,
                sender_id,
                sender_display,
                text,
                raw_payload,
                linked_task_id,
                reply_text,
                status,
                error,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM chat_ingress_events
            WHERE ingress_id = ?1
            "#,
        )
        .bind(ingress_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch chat ingress event")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_chat_ingress_events(
        &self,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<ChatIngressEventRecord>> {
        let rows = match limit {
            Some(limit) => sqlx::query_as::<_, ChatIngressEventRow>(
                r#"
                    SELECT
                        ingress_id,
                        platform,
                        event_type,
                        chat_id,
                        sender_id,
                        sender_display,
                        text,
                        raw_payload,
                        linked_task_id,
                        reply_text,
                        status,
                        error,
                        created_at_unix_ms,
                        updated_at_unix_ms
                    FROM chat_ingress_events
                    ORDER BY created_at_unix_ms DESC
                    LIMIT ?1
                    "#,
            )
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .context("failed to list recent chat ingress events")?,
            None => sqlx::query_as::<_, ChatIngressEventRow>(
                r#"
                    SELECT
                        ingress_id,
                        platform,
                        event_type,
                        chat_id,
                        sender_id,
                        sender_display,
                        text,
                        raw_payload,
                        linked_task_id,
                        reply_text,
                        status,
                        error,
                        created_at_unix_ms,
                        updated_at_unix_ms
                    FROM chat_ingress_events
                    ORDER BY created_at_unix_ms DESC
                    "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("failed to list chat ingress events")?,
        };

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn latest_chat_ingress_event_for_task(
        &self,
        task_id: Uuid,
    ) -> anyhow::Result<Option<ChatIngressEventRecord>> {
        let row = sqlx::query_as::<_, ChatIngressEventRow>(
            r#"
            SELECT
                ingress_id,
                platform,
                event_type,
                chat_id,
                sender_id,
                sender_display,
                text,
                raw_payload,
                linked_task_id,
                reply_text,
                status,
                error,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM chat_ingress_events
            WHERE linked_task_id = ?1
            ORDER BY created_at_unix_ms DESC, rowid DESC
            LIMIT 1
            "#,
        )
        .bind(task_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch latest chat ingress event for task {task_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn upsert_agent_experience(
        &self,
        experience: AgentExperienceRecord,
    ) -> anyhow::Result<AgentExperienceRecord> {
        save_agent_experience(&self.pool, &experience).await?;
        self.emit_console_event(
            "evolution",
            Some(experience.experience_id.to_string()),
            Some(experience.outcome.clone()),
            format!(
                "{} · {} · {}",
                experience.source, experience.task_kind, experience.lesson
            ),
        );
        Ok(experience)
    }

    pub async fn get_agent_experience(
        &self,
        experience_id: Uuid,
    ) -> anyhow::Result<Option<AgentExperienceRecord>> {
        let row = sqlx::query_as::<_, AgentExperienceRow>(
            r#"
            SELECT
                experience_id,
                source,
                scope,
                task_kind,
                input_summary,
                action_summary,
                outcome,
                lesson,
                reusable_hint,
                evidence,
                tags,
                risk_level,
                related_task_id,
                related_ingress_id,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM agent_experiences
            WHERE experience_id = ?1
            "#,
        )
        .bind(experience_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch agent experience {experience_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_agent_experiences(
        &self,
        filter: AgentExperienceListFilter,
    ) -> anyhow::Result<Vec<AgentExperienceRecord>> {
        let search = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let limit = filter.limit.unwrap_or(50).clamp(1, 200);
        let rows = sqlx::query_as::<_, AgentExperienceRow>(
            r#"
            SELECT
                experience_id,
                source,
                scope,
                task_kind,
                input_summary,
                action_summary,
                outcome,
                lesson,
                reusable_hint,
                evidence,
                tags,
                risk_level,
                related_task_id,
                related_ingress_id,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM agent_experiences
            WHERE (?1 IS NULL OR source = ?1)
              AND (?2 IS NULL OR outcome = ?2)
              AND (
                ?3 IS NULL
                OR input_summary LIKE ?3
                OR action_summary LIKE ?3
                OR lesson LIKE ?3
                OR reusable_hint LIKE ?3
                OR tags LIKE ?3
              )
            ORDER BY updated_at_unix_ms DESC, created_at_unix_ms DESC
            LIMIT ?4
            "#,
        )
        .bind(
            filter
                .source
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(
            filter
                .outcome
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(search.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("failed to list agent experiences")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn count_agent_experiences(&self) -> anyhow::Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_experiences")
            .fetch_one(&self.pool)
            .await
            .context("failed to count agent experiences")?;
        u64::try_from(count).context("negative agent experience count")
    }

    pub async fn has_agent_experience_for_ingress(&self, ingress_id: Uuid) -> anyhow::Result<bool> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM agent_experiences
            WHERE related_ingress_id = ?1
            "#,
        )
        .bind(ingress_id.to_string())
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("failed to count experiences for ingress {ingress_id}"))?;
        Ok(count > 0)
    }

    pub async fn upsert_skill_proposal(
        &self,
        proposal: SkillProposalRecord,
    ) -> anyhow::Result<SkillProposalRecord> {
        save_skill_proposal(&self.pool, &proposal).await?;
        self.emit_console_event(
            "skill_proposal",
            Some(proposal.proposal_id.to_string()),
            Some(proposal.status.clone()),
            format!("{} · {}", proposal.suggested_skill_id, proposal.title),
        );
        Ok(proposal)
    }

    pub async fn update_skill_proposal_review(
        &self,
        proposal: SkillProposalRecord,
    ) -> anyhow::Result<SkillProposalRecord> {
        let result = sqlx::query(
            r#"
            UPDATE skill_proposals
            SET
                status = ?1,
                rationale = ?2,
                evidence = ?3,
                updated_at_unix_ms = ?4
            WHERE proposal_id = ?5
            "#,
        )
        .bind(&proposal.status)
        .bind(&proposal.rationale)
        .bind(
            serde_json::to_string(&proposal.evidence)
                .context("failed to serialize skill proposal review evidence")?,
        )
        .bind(u128_to_i64(proposal.updated_at_unix_ms)?)
        .bind(proposal.proposal_id.to_string())
        .execute(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to update skill proposal review {}",
                proposal.proposal_id
            )
        })?;
        if result.rows_affected() == 0 {
            return Err(anyhow!(
                "skill proposal {} was not found for review update",
                proposal.proposal_id
            ));
        }
        self.emit_console_event(
            "skill_proposal_review",
            Some(proposal.proposal_id.to_string()),
            Some(proposal.status.clone()),
            format!("{} · {}", proposal.suggested_skill_id, proposal.title),
        );
        Ok(proposal)
    }

    pub async fn get_skill_proposal(
        &self,
        proposal_id: Uuid,
    ) -> anyhow::Result<Option<SkillProposalRecord>> {
        let row = sqlx::query_as::<_, SkillProposalRow>(
            r#"
            SELECT
                proposal_id,
                proposal_key,
                title,
                summary,
                rationale,
                suggested_skill_id,
                source,
                status,
                confidence,
                evidence,
                tags,
                risk_level,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_proposals
            WHERE proposal_id = ?1
            "#,
        )
        .bind(proposal_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill proposal {proposal_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_skill_proposal_by_key(
        &self,
        proposal_key: &str,
    ) -> anyhow::Result<Option<SkillProposalRecord>> {
        let row = sqlx::query_as::<_, SkillProposalRow>(
            r#"
            SELECT
                proposal_id,
                proposal_key,
                title,
                summary,
                rationale,
                suggested_skill_id,
                source,
                status,
                confidence,
                evidence,
                tags,
                risk_level,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_proposals
            WHERE proposal_key = ?1
            "#,
        )
        .bind(proposal_key)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill proposal by key {proposal_key}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_skill_proposals(
        &self,
        filter: SkillProposalListFilter,
    ) -> anyhow::Result<Vec<SkillProposalRecord>> {
        let search = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let limit = filter.limit.unwrap_or(50).clamp(1, 200);
        let rows = sqlx::query_as::<_, SkillProposalRow>(
            r#"
            SELECT
                proposal_id,
                proposal_key,
                title,
                summary,
                rationale,
                suggested_skill_id,
                source,
                status,
                confidence,
                evidence,
                tags,
                risk_level,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_proposals
            WHERE (?1 IS NULL OR status = ?1)
              AND (
                ?2 IS NULL
                OR title LIKE ?2
                OR summary LIKE ?2
                OR rationale LIKE ?2
                OR suggested_skill_id LIKE ?2
                OR tags LIKE ?2
              )
            ORDER BY updated_at_unix_ms DESC, created_at_unix_ms DESC
            LIMIT ?3
            "#,
        )
        .bind(
            filter
                .status
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(search.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("failed to list skill proposals")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn count_skill_proposals(&self) -> anyhow::Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skill_proposals")
            .fetch_one(&self.pool)
            .await
            .context("failed to count skill proposals")?;
        u64::try_from(count).context("negative skill proposal count")
    }

    pub async fn upsert_skill_implementation_plan(
        &self,
        plan: SkillImplementationPlanRecord,
    ) -> anyhow::Result<SkillImplementationPlanRecord> {
        save_skill_implementation_plan(&self.pool, &plan).await?;
        self.emit_console_event(
            "skill_implementation_plan",
            Some(plan.plan_id.to_string()),
            Some(plan.status.clone()),
            format!("{} · {}", plan.suggested_skill_id, plan.title),
        );
        Ok(plan)
    }

    pub async fn update_skill_implementation_plan_review(
        &self,
        plan: SkillImplementationPlanRecord,
    ) -> anyhow::Result<SkillImplementationPlanRecord> {
        let result = sqlx::query(
            r#"
            UPDATE skill_implementation_plans
            SET
                status = ?1,
                guardrails = ?2,
                updated_at_unix_ms = ?3
            WHERE plan_id = ?4
            "#,
        )
        .bind(&plan.status)
        .bind(
            serde_json::to_string(&plan.guardrails)
                .context("failed to serialize skill implementation plan review guardrails")?,
        )
        .bind(u128_to_i64(plan.updated_at_unix_ms)?)
        .bind(plan.plan_id.to_string())
        .execute(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to update skill implementation plan review {}",
                plan.plan_id
            )
        })?;
        if result.rows_affected() == 0 {
            return Err(anyhow!(
                "skill implementation plan {} was not found for review update",
                plan.plan_id
            ));
        }
        self.emit_console_event(
            "skill_implementation_plan_review",
            Some(plan.plan_id.to_string()),
            Some(plan.status.clone()),
            format!("{} · {}", plan.suggested_skill_id, plan.title),
        );
        Ok(plan)
    }

    pub async fn get_skill_implementation_plan(
        &self,
        plan_id: Uuid,
    ) -> anyhow::Result<Option<SkillImplementationPlanRecord>> {
        let row = sqlx::query_as::<_, SkillImplementationPlanRow>(
            r#"
            SELECT
                plan_id,
                proposal_id,
                suggested_skill_id,
                title,
                summary,
                status,
                steps,
                acceptance_criteria,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_plans
            WHERE plan_id = ?1
            "#,
        )
        .bind(plan_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill implementation plan {plan_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_skill_implementation_plan_by_proposal(
        &self,
        proposal_id: Uuid,
    ) -> anyhow::Result<Option<SkillImplementationPlanRecord>> {
        let row = sqlx::query_as::<_, SkillImplementationPlanRow>(
            r#"
            SELECT
                plan_id,
                proposal_id,
                suggested_skill_id,
                title,
                summary,
                status,
                steps,
                acceptance_criteria,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_plans
            WHERE proposal_id = ?1
            "#,
        )
        .bind(proposal_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| {
            format!("failed to fetch skill implementation plan for proposal {proposal_id}")
        })?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_skill_implementation_plans(
        &self,
        filter: SkillImplementationPlanListFilter,
    ) -> anyhow::Result<Vec<SkillImplementationPlanRecord>> {
        let search = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let proposal_id = filter.proposal_id.map(|value| value.to_string());
        let limit = filter.limit.unwrap_or(50).clamp(1, 200);
        let rows = sqlx::query_as::<_, SkillImplementationPlanRow>(
            r#"
            SELECT
                plan_id,
                proposal_id,
                suggested_skill_id,
                title,
                summary,
                status,
                steps,
                acceptance_criteria,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_plans
            WHERE (?1 IS NULL OR status = ?1)
              AND (?2 IS NULL OR proposal_id = ?2)
              AND (
                ?3 IS NULL
                OR title LIKE ?3
                OR summary LIKE ?3
                OR suggested_skill_id LIKE ?3
                OR steps LIKE ?3
                OR acceptance_criteria LIKE ?3
                OR guardrails LIKE ?3
              )
            ORDER BY updated_at_unix_ms DESC, created_at_unix_ms DESC
            LIMIT ?4
            "#,
        )
        .bind(
            filter
                .status
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(proposal_id.as_deref())
        .bind(search.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("failed to list skill implementation plans")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn count_skill_implementation_plans(&self) -> anyhow::Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skill_implementation_plans")
            .fetch_one(&self.pool)
            .await
            .context("failed to count skill implementation plans")?;
        u64::try_from(count).context("negative skill implementation plan count")
    }

    pub async fn upsert_skill_implementation_run(
        &self,
        run: SkillImplementationRunRecord,
    ) -> anyhow::Result<SkillImplementationRunRecord> {
        save_skill_implementation_run(&self.pool, &run).await?;
        self.emit_console_event(
            "skill_implementation_run",
            Some(run.run_id.to_string()),
            Some(run.status.clone()),
            format!("{} · {}", run.suggested_skill_id, run.execution_mode),
        );
        Ok(run)
    }

    pub async fn update_skill_implementation_run_review(
        &self,
        run: SkillImplementationRunRecord,
    ) -> anyhow::Result<SkillImplementationRunRecord> {
        let result = sqlx::query(
            r#"
            UPDATE skill_implementation_runs
            SET
                status = ?1,
                guardrails = ?2,
                updated_at_unix_ms = ?3
            WHERE run_id = ?4
            "#,
        )
        .bind(&run.status)
        .bind(
            serde_json::to_string(&run.guardrails)
                .context("failed to serialize skill implementation run guardrails")?,
        )
        .bind(u128_to_i64(run.updated_at_unix_ms)?)
        .bind(run.run_id.to_string())
        .execute(&self.pool)
        .await
        .with_context(|| format!("failed to update skill implementation run {}", run.run_id))?;
        if result.rows_affected() == 0 {
            return Err(anyhow!(
                "skill implementation run {} was not found for review update",
                run.run_id
            ));
        }
        self.emit_console_event(
            "skill_implementation_run_review",
            Some(run.run_id.to_string()),
            Some(run.status.clone()),
            format!("{} · {}", run.suggested_skill_id, run.execution_mode),
        );
        Ok(run)
    }

    pub async fn get_skill_implementation_run(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<Option<SkillImplementationRunRecord>> {
        let row = sqlx::query_as::<_, SkillImplementationRunRow>(
            r#"
            SELECT
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                execution_mode,
                change_package,
                verification,
                rollback,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_runs
            WHERE run_id = ?1
            "#,
        )
        .bind(run_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill implementation run {run_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_skill_implementation_run_by_plan(
        &self,
        plan_id: Uuid,
    ) -> anyhow::Result<Option<SkillImplementationRunRecord>> {
        let row = sqlx::query_as::<_, SkillImplementationRunRow>(
            r#"
            SELECT
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                execution_mode,
                change_package,
                verification,
                rollback,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_runs
            WHERE plan_id = ?1
            "#,
        )
        .bind(plan_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill implementation run for plan {plan_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_skill_implementation_runs(
        &self,
        filter: SkillImplementationRunListFilter,
    ) -> anyhow::Result<Vec<SkillImplementationRunRecord>> {
        let search = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let plan_id = filter.plan_id.map(|value| value.to_string());
        let proposal_id = filter.proposal_id.map(|value| value.to_string());
        let limit = filter.limit.unwrap_or(50).clamp(1, 200);
        let rows = sqlx::query_as::<_, SkillImplementationRunRow>(
            r#"
            SELECT
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                execution_mode,
                change_package,
                verification,
                rollback,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_runs
            WHERE (?1 IS NULL OR status = ?1)
              AND (?2 IS NULL OR plan_id = ?2)
              AND (?3 IS NULL OR proposal_id = ?3)
              AND (
                ?4 IS NULL
                OR suggested_skill_id LIKE ?4
                OR execution_mode LIKE ?4
                OR change_package LIKE ?4
                OR verification LIKE ?4
                OR rollback LIKE ?4
                OR guardrails LIKE ?4
              )
            ORDER BY updated_at_unix_ms DESC, created_at_unix_ms DESC
            LIMIT ?5
            "#,
        )
        .bind(
            filter
                .status
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(plan_id.as_deref())
        .bind(proposal_id.as_deref())
        .bind(search.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("failed to list skill implementation runs")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn count_skill_implementation_runs(&self) -> anyhow::Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skill_implementation_runs")
            .fetch_one(&self.pool)
            .await
            .context("failed to count skill implementation runs")?;
        u64::try_from(count).context("negative skill implementation run count")
    }

    pub async fn upsert_skill_implementation_execution(
        &self,
        execution: SkillImplementationExecutionRecord,
    ) -> anyhow::Result<SkillImplementationExecutionRecord> {
        save_skill_implementation_execution(&self.pool, &execution).await?;
        self.emit_console_event(
            "skill_implementation_execution",
            Some(execution.execution_id.to_string()),
            Some(execution.status.clone()),
            format!("{} · {}", execution.suggested_skill_id, execution.executor),
        );
        Ok(execution)
    }

    pub async fn update_skill_implementation_execution_review(
        &self,
        execution: SkillImplementationExecutionRecord,
    ) -> anyhow::Result<SkillImplementationExecutionRecord> {
        let result = sqlx::query(
            r#"
            UPDATE skill_implementation_executions
            SET
                status = ?1,
                result = ?2,
                guardrails = ?3,
                updated_at_unix_ms = ?4
            WHERE execution_id = ?5
            "#,
        )
        .bind(&execution.status)
        .bind(
            serde_json::to_string(&execution.result)
                .context("failed to serialize skill implementation execution result")?,
        )
        .bind(
            serde_json::to_string(&execution.guardrails)
                .context("failed to serialize skill implementation execution guardrails")?,
        )
        .bind(u128_to_i64(execution.updated_at_unix_ms)?)
        .bind(execution.execution_id.to_string())
        .execute(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to update skill implementation execution {}",
                execution.execution_id
            )
        })?;
        if result.rows_affected() == 0 {
            return Err(anyhow!(
                "skill implementation execution {} was not found for review update",
                execution.execution_id
            ));
        }
        self.emit_console_event(
            "skill_implementation_execution",
            Some(execution.execution_id.to_string()),
            Some(execution.status.clone()),
            format!("{} · {}", execution.suggested_skill_id, execution.executor),
        );
        Ok(execution)
    }

    pub async fn get_skill_implementation_execution(
        &self,
        execution_id: Uuid,
    ) -> anyhow::Result<Option<SkillImplementationExecutionRecord>> {
        let row = sqlx::query_as::<_, SkillImplementationExecutionRow>(
            r#"
            SELECT
                execution_id,
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                executor,
                preflight_report,
                command_plan,
                result,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_executions
            WHERE execution_id = ?1
            "#,
        )
        .bind(execution_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| {
            format!("failed to fetch skill implementation execution {execution_id}")
        })?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_skill_implementation_executions(
        &self,
        filter: SkillImplementationExecutionListFilter,
    ) -> anyhow::Result<Vec<SkillImplementationExecutionRecord>> {
        let search = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let run_id = filter.run_id.map(|value| value.to_string());
        let plan_id = filter.plan_id.map(|value| value.to_string());
        let proposal_id = filter.proposal_id.map(|value| value.to_string());
        let limit = filter.limit.unwrap_or(50).clamp(1, 250);
        let rows = sqlx::query_as::<_, SkillImplementationExecutionRow>(
            r#"
            SELECT
                execution_id,
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                executor,
                preflight_report,
                command_plan,
                result,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_executions
            WHERE (?1 IS NULL OR status = ?1)
              AND (?2 IS NULL OR run_id = ?2)
              AND (?3 IS NULL OR plan_id = ?3)
              AND (?4 IS NULL OR proposal_id = ?4)
              AND (
                ?5 IS NULL
                OR suggested_skill_id LIKE ?5
                OR executor LIKE ?5
                OR preflight_report LIKE ?5
                OR command_plan LIKE ?5
                OR result LIKE ?5
                OR guardrails LIKE ?5
              )
            ORDER BY updated_at_unix_ms DESC, created_at_unix_ms DESC
            LIMIT ?6
            "#,
        )
        .bind(
            filter
                .status
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(run_id.as_deref())
        .bind(plan_id.as_deref())
        .bind(proposal_id.as_deref())
        .bind(search.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("failed to list skill implementation executions")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn count_skill_implementation_executions(&self) -> anyhow::Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skill_implementation_executions")
            .fetch_one(&self.pool)
            .await
            .context("failed to count skill implementation executions")?;
        u64::try_from(count).context("negative skill implementation execution count")
    }

    pub async fn upsert_skill_implementation_patch(
        &self,
        patch: SkillImplementationPatchRecord,
    ) -> anyhow::Result<SkillImplementationPatchRecord> {
        save_skill_implementation_patch(&self.pool, &patch).await?;
        self.emit_console_event(
            "skill_implementation_patch",
            Some(patch.patch_id.to_string()),
            Some(patch.status.clone()),
            format!("{} · {}", patch.suggested_skill_id, patch.patch_kind),
        );
        Ok(patch)
    }

    pub async fn update_skill_implementation_patch_review(
        &self,
        patch: SkillImplementationPatchRecord,
    ) -> anyhow::Result<SkillImplementationPatchRecord> {
        let result = sqlx::query(
            r#"
            UPDATE skill_implementation_patches
            SET
                status = ?1,
                verification_evidence = ?2,
                guardrails = ?3,
                updated_at_unix_ms = ?4
            WHERE patch_id = ?5
            "#,
        )
        .bind(&patch.status)
        .bind(
            serde_json::to_string(&patch.verification_evidence)
                .context("failed to serialize skill implementation patch verification evidence")?,
        )
        .bind(
            serde_json::to_string(&patch.guardrails)
                .context("failed to serialize skill implementation patch guardrails")?,
        )
        .bind(u128_to_i64(patch.updated_at_unix_ms)?)
        .bind(patch.patch_id.to_string())
        .execute(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to update skill implementation patch {}",
                patch.patch_id
            )
        })?;
        if result.rows_affected() == 0 {
            return Err(anyhow!(
                "skill implementation patch {} was not found for review update",
                patch.patch_id
            ));
        }
        self.emit_console_event(
            "skill_implementation_patch",
            Some(patch.patch_id.to_string()),
            Some(patch.status.clone()),
            format!("{} · {}", patch.suggested_skill_id, patch.patch_kind),
        );
        Ok(patch)
    }

    pub async fn update_skill_implementation_patch_runtime(
        &self,
        patch: SkillImplementationPatchRecord,
    ) -> anyhow::Result<SkillImplementationPatchRecord> {
        let result = sqlx::query(
            r#"
            UPDATE skill_implementation_patches
            SET
                status = ?1,
                patch_manifest = ?2,
                rollback_plan = ?3,
                verification_evidence = ?4,
                guardrails = ?5,
                updated_at_unix_ms = ?6
            WHERE patch_id = ?7
            "#,
        )
        .bind(&patch.status)
        .bind(
            serde_json::to_string(&patch.patch_manifest)
                .context("failed to serialize skill implementation patch manifest")?,
        )
        .bind(
            serde_json::to_string(&patch.rollback_plan)
                .context("failed to serialize skill implementation patch rollback plan")?,
        )
        .bind(
            serde_json::to_string(&patch.verification_evidence)
                .context("failed to serialize skill implementation patch verification evidence")?,
        )
        .bind(
            serde_json::to_string(&patch.guardrails)
                .context("failed to serialize skill implementation patch guardrails")?,
        )
        .bind(u128_to_i64(patch.updated_at_unix_ms)?)
        .bind(patch.patch_id.to_string())
        .execute(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to update skill implementation patch runtime {}",
                patch.patch_id
            )
        })?;
        if result.rows_affected() == 0 {
            return Err(anyhow!(
                "skill implementation patch {} was not found for runtime update",
                patch.patch_id
            ));
        }
        self.emit_console_event(
            "skill_implementation_patch",
            Some(patch.patch_id.to_string()),
            Some(patch.status.clone()),
            format!("{} · {}", patch.suggested_skill_id, patch.patch_kind),
        );
        Ok(patch)
    }

    pub async fn get_skill_implementation_patch(
        &self,
        patch_id: Uuid,
    ) -> anyhow::Result<Option<SkillImplementationPatchRecord>> {
        let row = sqlx::query_as::<_, SkillImplementationPatchRow>(
            r#"
            SELECT
                patch_id,
                execution_id,
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                patch_kind,
                summary,
                changed_files,
                patch_manifest,
                rollback_plan,
                verification_evidence,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_patches
            WHERE patch_id = ?1
            "#,
        )
        .bind(patch_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill implementation patch {patch_id}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_skill_implementation_patches(
        &self,
        filter: SkillImplementationPatchListFilter,
    ) -> anyhow::Result<Vec<SkillImplementationPatchRecord>> {
        let search = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let execution_id = filter.execution_id.map(|value| value.to_string());
        let run_id = filter.run_id.map(|value| value.to_string());
        let plan_id = filter.plan_id.map(|value| value.to_string());
        let proposal_id = filter.proposal_id.map(|value| value.to_string());
        let limit = filter.limit.unwrap_or(50).clamp(1, 250);
        let rows = sqlx::query_as::<_, SkillImplementationPatchRow>(
            r#"
            SELECT
                patch_id,
                execution_id,
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id,
                status,
                patch_kind,
                summary,
                changed_files,
                patch_manifest,
                rollback_plan,
                verification_evidence,
                guardrails,
                created_by,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_implementation_patches
            WHERE (?1 IS NULL OR status = ?1)
              AND (?2 IS NULL OR execution_id = ?2)
              AND (?3 IS NULL OR run_id = ?3)
              AND (?4 IS NULL OR plan_id = ?4)
              AND (?5 IS NULL OR proposal_id = ?5)
              AND (
                ?6 IS NULL
                OR suggested_skill_id LIKE ?6
                OR patch_kind LIKE ?6
                OR summary LIKE ?6
                OR changed_files LIKE ?6
                OR patch_manifest LIKE ?6
                OR rollback_plan LIKE ?6
                OR verification_evidence LIKE ?6
                OR guardrails LIKE ?6
              )
            ORDER BY updated_at_unix_ms DESC, created_at_unix_ms DESC
            LIMIT ?7
            "#,
        )
        .bind(
            filter
                .status
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(execution_id.as_deref())
        .bind(run_id.as_deref())
        .bind(plan_id.as_deref())
        .bind(proposal_id.as_deref())
        .bind(search.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("failed to list skill implementation patches")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn count_skill_implementation_patches(&self) -> anyhow::Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skill_implementation_patches")
            .fetch_one(&self.pool)
            .await
            .context("failed to count skill implementation patches")?;
        u64::try_from(count).context("negative skill implementation patch count")
    }

    pub async fn upsert_chat_channel_identity(
        &self,
        identity: ChatChannelIdentityRecord,
    ) -> anyhow::Result<ChatChannelIdentityRecord> {
        save_chat_channel_identity(&self.pool, &identity).await?;
        self.emit_console_event(
            "chat_identity",
            Some(format!("{}:{}", identity.platform, identity.identity_key)),
            Some(identity.status.as_db().to_string()),
            format!(
                "{} chat identity {}",
                identity.platform,
                identity
                    .sender_display
                    .clone()
                    .or(identity.sender_id.clone())
                    .unwrap_or_else(|| identity.identity_key.clone())
            ),
        );
        Ok(identity)
    }

    pub async fn upsert_chat_automation_mode(
        &self,
        record: ChatAutomationModeRecord,
    ) -> anyhow::Result<ChatAutomationModeRecord> {
        save_chat_automation_mode(&self.pool, &record).await?;
        self.emit_console_event(
            "chat_mode",
            Some(format!("{}:{}", record.platform, record.chat_key)),
            Some(record.mode.as_db().to_string()),
            format!("{} chat mode -> {}", record.platform, record.mode.as_db()),
        );
        Ok(record)
    }

    pub async fn get_chat_automation_mode(
        &self,
        platform: &str,
        chat_key: &str,
    ) -> anyhow::Result<Option<ChatAutomationModeRecord>> {
        let row = sqlx::query_as::<_, ChatAutomationModeRow>(
            r#"
            SELECT
                platform,
                chat_key,
                chat_id,
                sender_id,
                mode,
                updated_by,
                reason,
                last_ingress_id,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM chat_automation_modes
            WHERE platform = ?1 AND chat_key = ?2
            "#,
        )
        .bind(platform)
        .bind(chat_key)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| {
            format!("failed to fetch chat automation mode for {platform}:{chat_key}")
        })?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_chat_channel_identity(
        &self,
        platform: &str,
        identity_key: &str,
    ) -> anyhow::Result<Option<ChatChannelIdentityRecord>> {
        let row = sqlx::query_as::<_, ChatChannelIdentityRow>(
            r#"
            SELECT
                platform,
                identity_key,
                chat_id,
                sender_id,
                sender_display,
                pairing_code,
                dm_policy,
                decision_reason,
                last_ingress_id,
                status,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM chat_channel_identities
            WHERE platform = ?1 AND identity_key = ?2
            "#,
        )
        .bind(platform)
        .bind(identity_key)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch chat identity for {platform}:{identity_key}"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_chat_channel_identities(
        &self,
        platform: Option<&str>,
        status: Option<ChatChannelIdentityStatus>,
    ) -> anyhow::Result<Vec<ChatChannelIdentityRecord>> {
        let rows = match (platform, status) {
            (Some(platform), Some(status)) => sqlx::query_as::<_, ChatChannelIdentityRow>(
                r#"
                SELECT
                    platform,
                    identity_key,
                    chat_id,
                    sender_id,
                    sender_display,
                    pairing_code,
                    dm_policy,
                    decision_reason,
                    last_ingress_id,
                    status,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM chat_channel_identities
                WHERE platform = ?1 AND status = ?2
                ORDER BY updated_at_unix_ms DESC, identity_key ASC
                "#,
            )
            .bind(platform)
            .bind(status.as_db())
            .fetch_all(&self.pool)
            .await
            .context("failed to list chat identities by platform and status")?,
            (Some(platform), None) => sqlx::query_as::<_, ChatChannelIdentityRow>(
                r#"
                SELECT
                    platform,
                    identity_key,
                    chat_id,
                    sender_id,
                    sender_display,
                    pairing_code,
                    dm_policy,
                    decision_reason,
                    last_ingress_id,
                    status,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM chat_channel_identities
                WHERE platform = ?1
                ORDER BY updated_at_unix_ms DESC, identity_key ASC
                "#,
            )
            .bind(platform)
            .fetch_all(&self.pool)
            .await
            .context("failed to list chat identities by platform")?,
            (None, Some(status)) => sqlx::query_as::<_, ChatChannelIdentityRow>(
                r#"
                SELECT
                    platform,
                    identity_key,
                    chat_id,
                    sender_id,
                    sender_display,
                    pairing_code,
                    dm_policy,
                    decision_reason,
                    last_ingress_id,
                    status,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM chat_channel_identities
                WHERE status = ?1
                ORDER BY updated_at_unix_ms DESC, identity_key ASC
                "#,
            )
            .bind(status.as_db())
            .fetch_all(&self.pool)
            .await
            .context("failed to list chat identities by status")?,
            (None, None) => sqlx::query_as::<_, ChatChannelIdentityRow>(
                r#"
                SELECT
                    platform,
                    identity_key,
                    chat_id,
                    sender_id,
                    sender_display,
                    pairing_code,
                    dm_policy,
                    decision_reason,
                    last_ingress_id,
                    status,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM chat_channel_identities
                ORDER BY updated_at_unix_ms DESC, identity_key ASC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("failed to list chat identities")?,
        };

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn upsert_node(
        &self,
        node_id: impl Into<String>,
        display_name: impl Into<String>,
        transport: impl Into<String>,
        capabilities: Vec<String>,
    ) -> anyhow::Result<NodeRecord> {
        let node_id = node_id.into();
        let display_name = display_name.into();
        let transport = transport.into();
        let now = unix_timestamp_ms();

        let mut node = if let Some(existing) = self.get_node(&node_id).await? {
            existing
        } else {
            NodeRecord {
                node_id: node_id.clone(),
                display_name: display_name.clone(),
                transport: transport.clone(),
                capabilities: capabilities.clone(),
                attestation_issuer_did: None,
                attestation_signature_hex: None,
                attestation_document_hash: None,
                attestation_issued_at_unix_ms: None,
                attestation_verified: false,
                attestation_verified_at_unix_ms: None,
                attestation_error: None,
                status: NodeSessionStatus::Registered,
                connected: false,
                last_seen_unix_ms: now,
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            }
        };

        node.display_name = display_name;
        node.transport = transport;
        if !capabilities.is_empty() {
            node.capabilities = capabilities;
        }
        node.last_seen_unix_ms = now;
        node.updated_at_unix_ms = now;

        save_node(&self.pool, &node).await?;
        self.emit_console_event(
            "node",
            Some(node.node_id.clone()),
            Some(node.status.as_db().to_string()),
            format!("node '{}' upserted", node.display_name),
        );
        Ok(node)
    }

    pub async fn update_node_metadata(
        &self,
        node_id: &str,
        display_name: Option<String>,
        transport: Option<String>,
        capabilities: Option<Vec<String>>,
    ) -> anyhow::Result<Option<NodeRecord>> {
        let Some(mut node) = self.get_node(node_id).await? else {
            return Ok(None);
        };

        if let Some(display_name) = display_name {
            node.display_name = display_name;
        }
        if let Some(transport) = transport {
            node.transport = transport;
        }
        if let Some(capabilities) = capabilities {
            node.capabilities = capabilities;
        }
        node.last_seen_unix_ms = unix_timestamp_ms();
        node.updated_at_unix_ms = unix_timestamp_ms();

        save_node(&self.pool, &node).await?;
        self.emit_console_event(
            "node",
            Some(node.node_id.clone()),
            Some(node.status.as_db().to_string()),
            format!("node '{}' metadata updated", node.display_name),
        );
        Ok(Some(node))
    }

    pub async fn apply_node_attestation(
        &self,
        node_id: &str,
        attestation: NodeAttestationState,
    ) -> anyhow::Result<Option<NodeRecord>> {
        let Some(mut node) = self.get_node(node_id).await? else {
            return Ok(None);
        };

        if let Some(display_name) = attestation.display_name {
            node.display_name = display_name;
        }
        if let Some(transport) = attestation.transport {
            node.transport = transport;
        }
        if let Some(capabilities) = attestation.verified_capabilities {
            node.capabilities = capabilities;
        }
        node.attestation_issuer_did = Some(attestation.issuer_did);
        node.attestation_signature_hex = Some(attestation.signature_hex);
        node.attestation_document_hash = Some(attestation.document_hash);
        node.attestation_issued_at_unix_ms = Some(attestation.issued_at_unix_ms);
        node.attestation_verified = attestation.verified;
        node.attestation_verified_at_unix_ms = attestation.verified_at_unix_ms;
        node.attestation_error = attestation.attestation_error;
        node.last_seen_unix_ms = unix_timestamp_ms();
        node.updated_at_unix_ms = unix_timestamp_ms();

        save_node(&self.pool, &node).await?;
        self.emit_console_event(
            "attestation",
            Some(node.node_id.clone()),
            Some(if node.attestation_verified {
                "verified".to_string()
            } else {
                "unverified".to_string()
            }),
            node.attestation_error
                .clone()
                .unwrap_or_else(|| format!("node '{}' attestation updated", node.display_name)),
        );
        Ok(Some(node))
    }

    pub async fn set_node_connection(
        &self,
        node_id: &str,
        connected: bool,
        status: NodeSessionStatus,
    ) -> anyhow::Result<Option<NodeRecord>> {
        let Some(mut node) = self.get_node(node_id).await? else {
            return Ok(None);
        };

        node.connected = connected;
        node.status = status;
        node.last_seen_unix_ms = unix_timestamp_ms();
        node.updated_at_unix_ms = unix_timestamp_ms();

        save_node(&self.pool, &node).await?;
        self.emit_console_event(
            "node_connection",
            Some(node.node_id.clone()),
            Some(node.status.as_db().to_string()),
            format!(
                "node connection is now {}",
                if connected {
                    "connected"
                } else {
                    "disconnected"
                }
            ),
        );
        Ok(Some(node))
    }

    pub async fn get_node(&self, node_id: &str) -> anyhow::Result<Option<NodeRecord>> {
        let row = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                node_id,
                display_name,
                transport,
                capabilities,
                attestation_issuer_did,
                attestation_signature_hex,
                attestation_document_hash,
                attestation_issued_at_unix_ms,
                attestation_verified,
                attestation_verified_at_unix_ms,
                attestation_error,
                status,
                connected,
                last_seen_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM nodes
            WHERE node_id = ?1
            "#,
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch node")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn list_nodes(&self) -> anyhow::Result<Vec<NodeRecord>> {
        let rows = sqlx::query_as::<_, NodeRow>(
            r#"
            SELECT
                node_id,
                display_name,
                transport,
                capabilities,
                attestation_issuer_did,
                attestation_signature_hex,
                attestation_document_hash,
                attestation_issued_at_unix_ms,
                attestation_verified,
                attestation_verified_at_unix_ms,
                attestation_error,
                status,
                connected,
                last_seen_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM nodes
            ORDER BY created_at_unix_ms DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list nodes")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn list_node_trust_roots(&self) -> anyhow::Result<Vec<NodeTrustRootRecord>> {
        let rows = sqlx::query_as::<_, NodeTrustRootRow>(
            r#"
            SELECT
                issuer_did,
                label,
                public_key_hex,
                updated_by,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM node_trust_roots
            ORDER BY issuer_did ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list node trust roots")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_node_trust_root(
        &self,
        issuer_did: &str,
    ) -> anyhow::Result<Option<NodeTrustRootRecord>> {
        let row = sqlx::query_as::<_, NodeTrustRootRow>(
            r#"
            SELECT
                issuer_did,
                label,
                public_key_hex,
                updated_by,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM node_trust_roots
            WHERE issuer_did = ?1
            "#,
        )
        .bind(issuer_did)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch node trust root '{issuer_did}'"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn save_node_trust_root(
        &self,
        trust_root: &NodeTrustRootRecord,
    ) -> anyhow::Result<NodeTrustRootRecord> {
        save_node_trust_root(&self.pool, trust_root).await?;
        self.emit_console_event(
            "node_trust_root",
            Some(trust_root.issuer_did.clone()),
            Some("saved".to_string()),
            trust_root.label.clone(),
        );
        Ok(trust_root.clone())
    }

    pub async fn touch_node(&self, node_id: &str) -> anyhow::Result<Option<NodeRecord>> {
        let Some(mut node) = self.get_node(node_id).await? else {
            return Ok(None);
        };

        node.last_seen_unix_ms = unix_timestamp_ms();
        node.updated_at_unix_ms = unix_timestamp_ms();

        save_node(&self.pool, &node).await?;
        Ok(Some(node))
    }

    pub async fn attach_node_session(&self, node_id: &str, sender: NodeSessionSender) {
        self.node_sessions
            .write()
            .await
            .insert(node_id.to_string(), sender);
    }

    pub async fn detach_node_session(&self, node_id: &str) {
        self.node_sessions.write().await.remove(node_id);
    }

    pub async fn get_node_session(&self, node_id: &str) -> Option<NodeSessionSender> {
        self.node_sessions.read().await.get(node_id).cloned()
    }

    pub async fn insert_node_command(
        &self,
        command: NodeCommandRecord,
    ) -> anyhow::Result<NodeCommandRecord> {
        save_node_command(&self.pool, &command).await?;
        self.emit_console_event(
            "node_command",
            Some(command.command_id.to_string()),
            Some(command.status.as_db().to_string()),
            format!("{} on {}", command.command_type, command.node_id),
        );
        Ok(command)
    }

    pub async fn get_node_rollout(
        &self,
        node_id: &str,
    ) -> anyhow::Result<Option<NodeRolloutRecord>> {
        let row = sqlx::query_as::<_, NodeRolloutRow>(
            r#"
            SELECT
                node_id,
                bundle_hash,
                policy_version,
                policy_document_hash,
                skill_distribution_hash,
                status,
                last_error,
                last_sent_at_unix_ms,
                last_ack_at_unix_ms,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM node_rollouts
            WHERE node_id = ?1
            "#,
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch node rollout for '{node_id}'"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn save_node_rollout(
        &self,
        rollout: &NodeRolloutRecord,
    ) -> anyhow::Result<NodeRolloutRecord> {
        save_node_rollout(&self.pool, rollout).await?;
        self.emit_console_event(
            "rollout",
            Some(rollout.node_id.clone()),
            Some(rollout.status.as_db().to_string()),
            format!("bundle {}", rollout.bundle_hash),
        );
        Ok(rollout.clone())
    }

    pub async fn list_node_commands(
        &self,
        node_id: Option<&str>,
    ) -> anyhow::Result<Vec<NodeCommandRecord>> {
        let rows = if let Some(node_id) = node_id {
            sqlx::query_as::<_, NodeCommandRow>(
                r#"
                SELECT
                    command_id,
                    node_id,
                    command_type,
                    payload,
                    status,
                    result,
                    error,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM node_commands
                WHERE node_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
            )
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("failed to list node commands")?
        } else {
            sqlx::query_as::<_, NodeCommandRow>(
                r#"
                SELECT
                    command_id,
                    node_id,
                    command_type,
                    payload,
                    status,
                    result,
                    error,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM node_commands
                ORDER BY created_at_unix_ms DESC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .context("failed to list node commands")?
        };

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn pending_node_commands(
        &self,
        node_id: &str,
    ) -> anyhow::Result<Vec<NodeCommandRecord>> {
        let rows = sqlx::query_as::<_, NodeCommandRow>(
            r#"
            SELECT
                command_id,
                node_id,
                command_type,
                payload,
                status,
                result,
                error,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM node_commands
            WHERE node_id = ?1 AND status = ?2
            ORDER BY created_at_unix_ms ASC
            "#,
        )
        .bind(node_id)
        .bind(NodeCommandStatus::Queued.as_db())
        .fetch_all(&self.pool)
        .await
        .context("failed to list pending node commands")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_node_command(
        &self,
        command_id: Uuid,
    ) -> anyhow::Result<Option<NodeCommandRecord>> {
        let row = sqlx::query_as::<_, NodeCommandRow>(
            r#"
            SELECT
                command_id,
                node_id,
                command_type,
                payload,
                status,
                result,
                error,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM node_commands
            WHERE command_id = ?1
            "#,
        )
        .bind(command_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch node command")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn update_node_command(
        &self,
        command_id: Uuid,
        status: NodeCommandStatus,
        result: Option<Value>,
        error: Option<String>,
    ) -> anyhow::Result<Option<NodeCommandRecord>> {
        let Some(mut command) = self.get_node_command(command_id).await? else {
            return Ok(None);
        };

        command.status = status;
        if result.is_some() {
            command.result = result;
        }
        if error.is_some() {
            command.error = error;
        }
        command.updated_at_unix_ms = unix_timestamp_ms();

        save_node_command(&self.pool, &command).await?;
        self.emit_console_event(
            "node_command",
            Some(command.command_id.to_string()),
            Some(command.status.as_db().to_string()),
            command
                .error
                .clone()
                .unwrap_or_else(|| command.command_type.clone()),
        );
        Ok(Some(command))
    }

    pub async fn upsert_orchestration_run(
        &self,
        run: OrchestrationRunRecord,
    ) -> anyhow::Result<OrchestrationRunRecord> {
        save_orchestration_run(&self.pool, &run).await?;
        self.emit_console_event(
            "orchestration",
            Some(run.task_id.to_string()),
            Some(run.status.as_db().to_string()),
            format!("step {}", run.next_step_index),
        );
        Ok(run)
    }

    pub async fn get_orchestration_run(
        &self,
        task_id: Uuid,
    ) -> anyhow::Result<Option<OrchestrationRunRecord>> {
        let row = sqlx::query_as::<_, OrchestrationRunRow>(
            r#"
            SELECT
                task_id,
                plan_json,
                next_step_index,
                last_result,
                waiting_transaction_id,
                status,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM orchestration_runs
            WHERE task_id = ?1
            "#,
        )
        .bind(task_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch orchestration run")?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn get_policy_profile(&self) -> anyhow::Result<PolicyProfileRecord> {
        let row = sqlx::query_as::<_, PolicyProfileRow>(
            r#"
            SELECT
                policy_id,
                version,
                issuer_did,
                allow_shell_exec,
                allowed_model_providers,
                allowed_chat_platforms,
                max_payment_amount,
                signature_hex,
                document_hash,
                issued_at_unix_ms,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM policy_profiles
            WHERE policy_id = 'default'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("failed to fetch policy profile")?;

        row.try_into()
    }

    pub async fn save_policy_profile(
        &self,
        profile: &PolicyProfileRecord,
    ) -> anyhow::Result<PolicyProfileRecord> {
        save_policy_profile(&self.pool, profile).await?;
        self.emit_console_event(
            "policy",
            Some(profile.policy_id.to_string()),
            Some(format!("v{}", profile.version)),
            "policy profile updated",
        );
        Ok(profile.clone())
    }

    pub async fn list_policy_trust_roots(&self) -> anyhow::Result<Vec<PolicyTrustRootRecord>> {
        let rows = sqlx::query_as::<_, PolicyTrustRootRow>(
            r#"
            SELECT
                issuer_did,
                label,
                public_key_hex,
                updated_by,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM policy_trust_roots
            ORDER BY issuer_did ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list policy trust roots")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_policy_trust_root(
        &self,
        issuer_did: &str,
    ) -> anyhow::Result<Option<PolicyTrustRootRecord>> {
        let row = sqlx::query_as::<_, PolicyTrustRootRow>(
            r#"
            SELECT
                issuer_did,
                label,
                public_key_hex,
                updated_by,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM policy_trust_roots
            WHERE issuer_did = ?1
            "#,
        )
        .bind(issuer_did)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch policy trust root '{issuer_did}'"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn save_policy_trust_root(
        &self,
        trust_root: &PolicyTrustRootRecord,
    ) -> anyhow::Result<PolicyTrustRootRecord> {
        save_policy_trust_root(&self.pool, trust_root).await?;
        self.emit_console_event(
            "policy_trust_root",
            Some(trust_root.issuer_did.clone()),
            Some("saved".to_string()),
            trust_root.label.clone(),
        );
        Ok(trust_root.clone())
    }

    pub async fn list_skill_publisher_trust_roots(
        &self,
    ) -> anyhow::Result<Vec<SkillPublisherTrustRootRecord>> {
        let rows = sqlx::query_as::<_, SkillPublisherTrustRootRow>(
            r#"
            SELECT
                issuer_did,
                label,
                public_key_hex,
                updated_by,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_publisher_trust_roots
            ORDER BY issuer_did ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list skill publisher trust roots")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn get_skill_publisher_trust_root(
        &self,
        issuer_did: &str,
    ) -> anyhow::Result<Option<SkillPublisherTrustRootRecord>> {
        let row = sqlx::query_as::<_, SkillPublisherTrustRootRow>(
            r#"
            SELECT
                issuer_did,
                label,
                public_key_hex,
                updated_by,
                updated_reason,
                created_at_unix_ms,
                updated_at_unix_ms
            FROM skill_publisher_trust_roots
            WHERE issuer_did = ?1
            "#,
        )
        .bind(issuer_did)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch skill publisher trust root '{issuer_did}'"))?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn save_skill_publisher_trust_root(
        &self,
        trust_root: &SkillPublisherTrustRootRecord,
    ) -> anyhow::Result<SkillPublisherTrustRootRecord> {
        save_skill_publisher_trust_root(&self.pool, trust_root).await?;
        self.emit_console_event(
            "skill_trust_root",
            Some(trust_root.issuer_did.clone()),
            Some("saved".to_string()),
            trust_root.label.clone(),
        );
        Ok(trust_root.clone())
    }

    pub async fn record_policy_audit_event(
        &self,
        policy_id: &str,
        version: u32,
        actor: impl Into<String>,
        summary: impl Into<String>,
        snapshot: &Value,
    ) -> anyhow::Result<PolicyAuditEventRecord> {
        let actor = actor.into();
        let summary = summary.into();
        let created_at_unix_ms = unix_timestamp_ms();
        let result = sqlx::query(
            r#"
            INSERT INTO policy_audit_events (
                policy_id,
                version,
                actor,
                summary,
                snapshot,
                created_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(policy_id)
        .bind(i64::from(version))
        .bind(&actor)
        .bind(&summary)
        .bind(serde_json::to_string(snapshot).context("failed to serialize policy snapshot")?)
        .bind(u128_to_i64(created_at_unix_ms)?)
        .execute(&self.pool)
        .await
        .context("failed to insert policy audit event")?;

        Ok(PolicyAuditEventRecord {
            audit_id: result.last_insert_rowid(),
            policy_id: policy_id.to_string(),
            version,
            actor,
            summary,
            snapshot: snapshot.clone(),
            created_at_unix_ms,
        })
    }

    pub async fn list_policy_audit_events(&self) -> anyhow::Result<Vec<PolicyAuditEventRecord>> {
        let rows = sqlx::query_as::<_, PolicyAuditEventRow>(
            r#"
            SELECT
                audit_id,
                policy_id,
                version,
                actor,
                summary,
                snapshot,
                created_at_unix_ms
            FROM policy_audit_events
            ORDER BY audit_id DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list policy audit events")?;

        rows.into_iter().map(TryInto::try_into).collect()
    }
}

#[derive(FromRow)]
struct TaskRow {
    task_id: String,
    parent_task_id: Option<String>,
    name: String,
    instruction: String,
    status: String,
    linked_payment_id: Option<String>,
    last_update_reason: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<TaskRow> for StoredTask {
    type Error = anyhow::Error;

    fn try_from(row: TaskRow) -> Result<Self, Self::Error> {
        Ok(Self {
            task_id: parse_uuid(&row.task_id, "task_id")?,
            parent_task_id: parse_uuid_opt(row.parent_task_id, "parent_task_id")?,
            name: row.name,
            instruction: row.instruction,
            status: TaskStatus::from_db(&row.status)?,
            linked_payment_id: parse_uuid_opt(row.linked_payment_id, "linked_payment_id")?,
            last_update_reason: row.last_update_reason,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct TaskEventRow {
    task_id: String,
    event_type: String,
    detail: String,
    created_at_unix_ms: i64,
}

impl TryFrom<TaskEventRow> for TaskEventRecord {
    type Error = anyhow::Error;

    fn try_from(row: TaskEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            task_id: parse_uuid(&row.task_id, "task_id")?,
            event_type: row.event_type,
            detail: row.detail,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct PaymentRow {
    transaction_id: String,
    task_id: Option<String>,
    mandate_id: String,
    amount: f64,
    description: String,
    status: String,
    verification_message: String,
    mcu_public_did: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<PaymentRow> for PaymentRecord {
    type Error = anyhow::Error;

    fn try_from(row: PaymentRow) -> Result<Self, Self::Error> {
        Ok(Self {
            transaction_id: parse_uuid(&row.transaction_id, "transaction_id")?,
            task_id: parse_uuid_opt(row.task_id, "task_id")?,
            mandate_id: parse_uuid(&row.mandate_id, "mandate_id")?,
            amount: row.amount,
            description: row.description,
            status: PaymentStatus::from_db(&row.status)?,
            verification_message: row.verification_message,
            mcu_public_did: row.mcu_public_did,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct ApprovalRequestRow {
    approval_id: String,
    kind: String,
    title: String,
    summary: String,
    task_id: Option<String>,
    reference_id: String,
    status: String,
    actor: Option<String>,
    decision_reason: Option<String>,
    decision_payload: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<ApprovalRequestRow> for ApprovalRequestRecord {
    type Error = anyhow::Error;

    fn try_from(row: ApprovalRequestRow) -> Result<Self, Self::Error> {
        Ok(Self {
            approval_id: parse_uuid(&row.approval_id, "approval_id")?,
            kind: ApprovalRequestKind::from_db(&row.kind)?,
            title: row.title,
            summary: row.summary,
            task_id: parse_uuid_opt(row.task_id, "task_id")?,
            reference_id: row.reference_id,
            status: ApprovalRequestStatus::from_db(&row.status)?,
            actor: row.actor,
            decision_reason: row.decision_reason,
            decision_payload: row
                .decision_payload
                .map(|value| parse_json_field(&value, "decision_payload"))
                .transpose()?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct EndUserApprovalSessionRow {
    session_id: String,
    approval_id: String,
    approval_kind: String,
    task_id: Option<String>,
    transaction_id: Option<String>,
    platform: Option<String>,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    approval_token_hash: String,
    token_hint: String,
    status: String,
    expires_at_unix_ms: Option<i64>,
    decided_at_unix_ms: Option<i64>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<EndUserApprovalSessionRow> for EndUserApprovalSessionRecord {
    type Error = anyhow::Error;

    fn try_from(row: EndUserApprovalSessionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            session_id: parse_uuid(&row.session_id, "session_id")?,
            approval_id: parse_uuid(&row.approval_id, "approval_id")?,
            approval_kind: ApprovalRequestKind::from_db(&row.approval_kind)?,
            task_id: parse_uuid_opt(row.task_id, "task_id")?,
            transaction_id: parse_uuid_opt(row.transaction_id, "transaction_id")?,
            platform: row.platform,
            chat_id: row.chat_id,
            sender_id: row.sender_id,
            sender_display: row.sender_display,
            approval_token_hash: row.approval_token_hash,
            token_hint: row.token_hint,
            status: EndUserApprovalStatus::from_db(&row.status)?,
            expires_at_unix_ms: i64_to_u128_opt(row.expires_at_unix_ms)?,
            decided_at_unix_ms: i64_to_u128_opt(row.decided_at_unix_ms)?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct MarketplacePeerRow {
    peer_id: String,
    display_name: String,
    base_url: String,
    catalog_url: String,
    enabled: i64,
    trust_enabled: i64,
    sync_status: String,
    last_sync_error: Option<String>,
    last_synced_at_unix_ms: Option<i64>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<MarketplacePeerRow> for MarketplacePeerRecord {
    type Error = anyhow::Error;

    fn try_from(row: MarketplacePeerRow) -> Result<Self, Self::Error> {
        Ok(Self {
            peer_id: row.peer_id,
            display_name: row.display_name,
            base_url: row.base_url,
            catalog_url: row.catalog_url,
            enabled: row.enabled != 0,
            trust_enabled: row.trust_enabled != 0,
            sync_status: MarketplacePeerSyncStatus::from_db(&row.sync_status)?,
            last_sync_error: row.last_sync_error,
            last_synced_at_unix_ms: i64_to_u128_opt(row.last_synced_at_unix_ms)?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct ChatIngressEventRow {
    ingress_id: String,
    platform: String,
    event_type: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    text: String,
    raw_payload: String,
    linked_task_id: Option<String>,
    reply_text: Option<String>,
    status: String,
    error: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<ChatIngressEventRow> for ChatIngressEventRecord {
    type Error = anyhow::Error;

    fn try_from(row: ChatIngressEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            ingress_id: parse_uuid(&row.ingress_id, "ingress_id")?,
            platform: row.platform,
            event_type: row.event_type,
            chat_id: row.chat_id,
            sender_id: row.sender_id,
            sender_display: row.sender_display,
            text: row.text,
            raw_payload: parse_json_field(&row.raw_payload, "raw_payload")?,
            linked_task_id: parse_uuid_opt(row.linked_task_id, "linked_task_id")?,
            reply_text: row.reply_text,
            status: ChatIngressStatus::from_db(&row.status)?,
            error: row.error,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct AgentExperienceRow {
    experience_id: String,
    source: String,
    scope: String,
    task_kind: String,
    input_summary: String,
    action_summary: String,
    outcome: String,
    lesson: String,
    reusable_hint: Option<String>,
    evidence: String,
    tags: String,
    risk_level: String,
    related_task_id: Option<String>,
    related_ingress_id: Option<String>,
    created_by: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<AgentExperienceRow> for AgentExperienceRecord {
    type Error = anyhow::Error;

    fn try_from(row: AgentExperienceRow) -> Result<Self, Self::Error> {
        Ok(Self {
            experience_id: parse_uuid(&row.experience_id, "experience_id")?,
            source: row.source,
            scope: row.scope,
            task_kind: row.task_kind,
            input_summary: row.input_summary,
            action_summary: row.action_summary,
            outcome: row.outcome,
            lesson: row.lesson,
            reusable_hint: row.reusable_hint,
            evidence: parse_json_field(&row.evidence, "evidence")?,
            tags: parse_json_field(&row.tags, "tags")?,
            risk_level: row.risk_level,
            related_task_id: parse_uuid_opt(row.related_task_id, "related_task_id")?,
            related_ingress_id: parse_uuid_opt(row.related_ingress_id, "related_ingress_id")?,
            created_by: row.created_by,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct SkillProposalRow {
    proposal_id: String,
    proposal_key: String,
    title: String,
    summary: String,
    rationale: String,
    suggested_skill_id: String,
    source: String,
    status: String,
    confidence: f64,
    evidence: String,
    tags: String,
    risk_level: String,
    created_by: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<SkillProposalRow> for SkillProposalRecord {
    type Error = anyhow::Error;

    fn try_from(row: SkillProposalRow) -> Result<Self, Self::Error> {
        Ok(Self {
            proposal_id: parse_uuid(&row.proposal_id, "proposal_id")?,
            proposal_key: row.proposal_key,
            title: row.title,
            summary: row.summary,
            rationale: row.rationale,
            suggested_skill_id: row.suggested_skill_id,
            source: row.source,
            status: row.status,
            confidence: row.confidence,
            evidence: parse_json_field(&row.evidence, "evidence")?,
            tags: parse_json_field(&row.tags, "tags")?,
            risk_level: row.risk_level,
            created_by: row.created_by,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct SkillImplementationPlanRow {
    plan_id: String,
    proposal_id: String,
    suggested_skill_id: String,
    title: String,
    summary: String,
    status: String,
    steps: String,
    acceptance_criteria: String,
    guardrails: String,
    created_by: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<SkillImplementationPlanRow> for SkillImplementationPlanRecord {
    type Error = anyhow::Error;

    fn try_from(row: SkillImplementationPlanRow) -> Result<Self, Self::Error> {
        Ok(Self {
            plan_id: parse_uuid(&row.plan_id, "plan_id")?,
            proposal_id: parse_uuid(&row.proposal_id, "proposal_id")?,
            suggested_skill_id: row.suggested_skill_id,
            title: row.title,
            summary: row.summary,
            status: row.status,
            steps: parse_json_field(&row.steps, "steps")?,
            acceptance_criteria: parse_json_field(&row.acceptance_criteria, "acceptance_criteria")?,
            guardrails: parse_json_field(&row.guardrails, "guardrails")?,
            created_by: row.created_by,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct SkillImplementationRunRow {
    run_id: String,
    plan_id: String,
    proposal_id: String,
    suggested_skill_id: String,
    status: String,
    execution_mode: String,
    change_package: String,
    verification: String,
    rollback: String,
    guardrails: String,
    created_by: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<SkillImplementationRunRow> for SkillImplementationRunRecord {
    type Error = anyhow::Error;

    fn try_from(row: SkillImplementationRunRow) -> Result<Self, Self::Error> {
        Ok(Self {
            run_id: parse_uuid(&row.run_id, "run_id")?,
            plan_id: parse_uuid(&row.plan_id, "plan_id")?,
            proposal_id: parse_uuid(&row.proposal_id, "proposal_id")?,
            suggested_skill_id: row.suggested_skill_id,
            status: row.status,
            execution_mode: row.execution_mode,
            change_package: parse_json_field(&row.change_package, "change_package")?,
            verification: parse_json_field(&row.verification, "verification")?,
            rollback: parse_json_field(&row.rollback, "rollback")?,
            guardrails: parse_json_field(&row.guardrails, "guardrails")?,
            created_by: row.created_by,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct SkillImplementationExecutionRow {
    execution_id: String,
    run_id: String,
    plan_id: String,
    proposal_id: String,
    suggested_skill_id: String,
    status: String,
    executor: String,
    preflight_report: String,
    command_plan: String,
    result: String,
    guardrails: String,
    created_by: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<SkillImplementationExecutionRow> for SkillImplementationExecutionRecord {
    type Error = anyhow::Error;

    fn try_from(row: SkillImplementationExecutionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            execution_id: parse_uuid(&row.execution_id, "execution_id")?,
            run_id: parse_uuid(&row.run_id, "run_id")?,
            plan_id: parse_uuid(&row.plan_id, "plan_id")?,
            proposal_id: parse_uuid(&row.proposal_id, "proposal_id")?,
            suggested_skill_id: row.suggested_skill_id,
            status: row.status,
            executor: row.executor,
            preflight_report: parse_json_field(&row.preflight_report, "preflight_report")?,
            command_plan: parse_json_field(&row.command_plan, "command_plan")?,
            result: parse_json_field(&row.result, "result")?,
            guardrails: parse_json_field(&row.guardrails, "guardrails")?,
            created_by: row.created_by,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct SkillImplementationPatchRow {
    patch_id: String,
    execution_id: String,
    run_id: String,
    plan_id: String,
    proposal_id: String,
    suggested_skill_id: String,
    status: String,
    patch_kind: String,
    summary: String,
    changed_files: String,
    patch_manifest: String,
    rollback_plan: String,
    verification_evidence: String,
    guardrails: String,
    created_by: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<SkillImplementationPatchRow> for SkillImplementationPatchRecord {
    type Error = anyhow::Error;

    fn try_from(row: SkillImplementationPatchRow) -> Result<Self, Self::Error> {
        Ok(Self {
            patch_id: parse_uuid(&row.patch_id, "patch_id")?,
            execution_id: parse_uuid(&row.execution_id, "execution_id")?,
            run_id: parse_uuid(&row.run_id, "run_id")?,
            plan_id: parse_uuid(&row.plan_id, "plan_id")?,
            proposal_id: parse_uuid(&row.proposal_id, "proposal_id")?,
            suggested_skill_id: row.suggested_skill_id,
            status: row.status,
            patch_kind: row.patch_kind,
            summary: row.summary,
            changed_files: parse_json_field(&row.changed_files, "changed_files")?,
            patch_manifest: parse_json_field(&row.patch_manifest, "patch_manifest")?,
            rollback_plan: parse_json_field(&row.rollback_plan, "rollback_plan")?,
            verification_evidence: parse_json_field(
                &row.verification_evidence,
                "verification_evidence",
            )?,
            guardrails: parse_json_field(&row.guardrails, "guardrails")?,
            created_by: row.created_by,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct ChatChannelIdentityRow {
    platform: String,
    identity_key: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    sender_display: Option<String>,
    pairing_code: Option<String>,
    dm_policy: String,
    decision_reason: Option<String>,
    last_ingress_id: Option<String>,
    status: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<ChatChannelIdentityRow> for ChatChannelIdentityRecord {
    type Error = anyhow::Error;

    fn try_from(row: ChatChannelIdentityRow) -> Result<Self, Self::Error> {
        Ok(Self {
            platform: row.platform,
            identity_key: row.identity_key,
            chat_id: row.chat_id,
            sender_id: row.sender_id,
            sender_display: row.sender_display,
            pairing_code: row.pairing_code,
            dm_policy: row.dm_policy,
            decision_reason: row.decision_reason,
            last_ingress_id: parse_uuid_opt(row.last_ingress_id, "last_ingress_id")?,
            status: ChatChannelIdentityStatus::from_db(&row.status)?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct ChatAutomationModeRow {
    platform: String,
    chat_key: String,
    chat_id: Option<String>,
    sender_id: Option<String>,
    mode: String,
    updated_by: Option<String>,
    reason: Option<String>,
    last_ingress_id: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<ChatAutomationModeRow> for ChatAutomationModeRecord {
    type Error = anyhow::Error;

    fn try_from(row: ChatAutomationModeRow) -> Result<Self, Self::Error> {
        Ok(Self {
            platform: row.platform,
            chat_key: row.chat_key,
            chat_id: row.chat_id,
            sender_id: row.sender_id,
            mode: ChatAutomationMode::from_db(&row.mode)?,
            updated_by: row.updated_by,
            reason: row.reason,
            last_ingress_id: parse_uuid_opt(row.last_ingress_id, "last_ingress_id")?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct NodeRow {
    node_id: String,
    display_name: String,
    transport: String,
    capabilities: String,
    attestation_issuer_did: Option<String>,
    attestation_signature_hex: Option<String>,
    attestation_document_hash: Option<String>,
    attestation_issued_at_unix_ms: Option<i64>,
    attestation_verified: i64,
    attestation_verified_at_unix_ms: Option<i64>,
    attestation_error: Option<String>,
    status: String,
    connected: i64,
    last_seen_unix_ms: i64,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<NodeRow> for NodeRecord {
    type Error = anyhow::Error;

    fn try_from(row: NodeRow) -> Result<Self, Self::Error> {
        Ok(Self {
            node_id: row.node_id,
            display_name: row.display_name,
            transport: row.transport,
            capabilities: parse_json_field(&row.capabilities, "capabilities")?,
            attestation_issuer_did: row.attestation_issuer_did,
            attestation_signature_hex: row.attestation_signature_hex,
            attestation_document_hash: row.attestation_document_hash,
            attestation_issued_at_unix_ms: i64_to_u128_opt(row.attestation_issued_at_unix_ms)?,
            attestation_verified: row.attestation_verified != 0,
            attestation_verified_at_unix_ms: i64_to_u128_opt(row.attestation_verified_at_unix_ms)?,
            attestation_error: row.attestation_error,
            status: NodeSessionStatus::from_db(&row.status)?,
            connected: row.connected != 0,
            last_seen_unix_ms: i64_to_u128(row.last_seen_unix_ms)?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct NodeTrustRootRow {
    issuer_did: String,
    label: String,
    public_key_hex: String,
    updated_by: String,
    updated_reason: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<NodeTrustRootRow> for NodeTrustRootRecord {
    type Error = anyhow::Error;

    fn try_from(row: NodeTrustRootRow) -> Result<Self, Self::Error> {
        Ok(Self {
            issuer_did: row.issuer_did,
            label: row.label,
            public_key_hex: row.public_key_hex,
            updated_by: row.updated_by,
            updated_reason: row.updated_reason,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct NodeCommandRow {
    command_id: String,
    node_id: String,
    command_type: String,
    payload: String,
    status: String,
    result: Option<String>,
    error: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(FromRow)]
struct NodeRolloutRow {
    node_id: String,
    bundle_hash: String,
    policy_version: i64,
    policy_document_hash: Option<String>,
    skill_distribution_hash: String,
    status: String,
    last_error: Option<String>,
    last_sent_at_unix_ms: i64,
    last_ack_at_unix_ms: Option<i64>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<NodeCommandRow> for NodeCommandRecord {
    type Error = anyhow::Error;

    fn try_from(row: NodeCommandRow) -> Result<Self, Self::Error> {
        Ok(Self {
            command_id: parse_uuid(&row.command_id, "command_id")?,
            node_id: row.node_id,
            command_type: row.command_type,
            payload: parse_json_field(&row.payload, "payload")?,
            status: NodeCommandStatus::from_db(&row.status)?,
            result: row
                .result
                .map(|value| parse_json_field(&value, "result"))
                .transpose()?,
            error: row.error,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

impl TryFrom<NodeRolloutRow> for NodeRolloutRecord {
    type Error = anyhow::Error;

    fn try_from(row: NodeRolloutRow) -> Result<Self, Self::Error> {
        Ok(Self {
            node_id: row.node_id,
            bundle_hash: row.bundle_hash,
            policy_version: u32::try_from(row.policy_version)
                .context("negative node rollout policy version found")?,
            policy_document_hash: row.policy_document_hash,
            skill_distribution_hash: row.skill_distribution_hash,
            status: NodeRolloutStatus::from_db(&row.status)?,
            last_error: row.last_error,
            last_sent_at_unix_ms: i64_to_u128(row.last_sent_at_unix_ms)?,
            last_ack_at_unix_ms: row.last_ack_at_unix_ms.map(i64_to_u128).transpose()?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct OrchestrationRunRow {
    task_id: String,
    plan_json: String,
    next_step_index: i64,
    last_result: Option<String>,
    waiting_transaction_id: Option<String>,
    status: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<OrchestrationRunRow> for OrchestrationRunRecord {
    type Error = anyhow::Error;

    fn try_from(row: OrchestrationRunRow) -> Result<Self, Self::Error> {
        Ok(Self {
            task_id: parse_uuid(&row.task_id, "task_id")?,
            plan_json: row.plan_json,
            next_step_index: u32::try_from(row.next_step_index)
                .context("negative next_step_index found in orchestration_runs")?,
            last_result: row
                .last_result
                .map(|value| parse_json_field(&value, "last_result"))
                .transpose()?,
            waiting_transaction_id: parse_uuid_opt(
                row.waiting_transaction_id,
                "waiting_transaction_id",
            )?,
            status: OrchestrationRunStatus::from_db(&row.status)?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct PolicyProfileRow {
    policy_id: String,
    version: i64,
    issuer_did: Option<String>,
    allow_shell_exec: i64,
    allowed_model_providers: String,
    allowed_chat_platforms: String,
    max_payment_amount: Option<f64>,
    signature_hex: Option<String>,
    document_hash: Option<String>,
    issued_at_unix_ms: Option<i64>,
    updated_reason: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<PolicyProfileRow> for PolicyProfileRecord {
    type Error = anyhow::Error;

    fn try_from(row: PolicyProfileRow) -> Result<Self, Self::Error> {
        Ok(Self {
            policy_id: row.policy_id,
            version: u32::try_from(row.version).context("negative policy version found")?,
            issuer_did: row.issuer_did,
            allow_shell_exec: row.allow_shell_exec != 0,
            allowed_model_providers: parse_json_field(
                &row.allowed_model_providers,
                "allowed_model_providers",
            )?,
            allowed_chat_platforms: parse_json_field(
                &row.allowed_chat_platforms,
                "allowed_chat_platforms",
            )?,
            max_payment_amount: row.max_payment_amount,
            signature_hex: row.signature_hex,
            document_hash: row.document_hash,
            issued_at_unix_ms: i64_to_u128_opt(row.issued_at_unix_ms)?,
            updated_reason: row.updated_reason,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct PolicyAuditEventRow {
    audit_id: i64,
    policy_id: String,
    version: i64,
    actor: String,
    summary: String,
    snapshot: String,
    created_at_unix_ms: i64,
}

impl TryFrom<PolicyAuditEventRow> for PolicyAuditEventRecord {
    type Error = anyhow::Error;

    fn try_from(row: PolicyAuditEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            audit_id: row.audit_id,
            policy_id: row.policy_id,
            version: u32::try_from(row.version).context("negative policy audit version found")?,
            actor: row.actor,
            summary: row.summary,
            snapshot: parse_json_field(&row.snapshot, "snapshot")?,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct PolicyTrustRootRow {
    issuer_did: String,
    label: String,
    public_key_hex: String,
    updated_by: String,
    updated_reason: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<PolicyTrustRootRow> for PolicyTrustRootRecord {
    type Error = anyhow::Error;

    fn try_from(row: PolicyTrustRootRow) -> Result<Self, Self::Error> {
        Ok(Self {
            issuer_did: row.issuer_did,
            label: row.label,
            public_key_hex: row.public_key_hex,
            updated_by: row.updated_by,
            updated_reason: row.updated_reason,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

#[derive(FromRow)]
struct SkillPublisherTrustRootRow {
    issuer_did: String,
    label: String,
    public_key_hex: String,
    updated_by: String,
    updated_reason: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

impl TryFrom<SkillPublisherTrustRootRow> for SkillPublisherTrustRootRecord {
    type Error = anyhow::Error;

    fn try_from(row: SkillPublisherTrustRootRow) -> Result<Self, Self::Error> {
        Ok(Self {
            issuer_did: row.issuer_did,
            label: row.label,
            public_key_hex: row.public_key_hex,
            updated_by: row.updated_by,
            updated_reason: row.updated_reason,
            created_at_unix_ms: i64_to_u128(row.created_at_unix_ms)?,
            updated_at_unix_ms: i64_to_u128(row.updated_at_unix_ms)?,
        })
    }
}

async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS tasks (
            task_id TEXT PRIMARY KEY,
            parent_task_id TEXT,
            name TEXT NOT NULL,
            instruction TEXT NOT NULL,
            status TEXT NOT NULL,
            linked_payment_id TEXT,
            last_update_reason TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS task_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            detail TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_task_events_task_id_created_at
        ON task_events(task_id, created_at_unix_ms)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS qgis_projects (
            project_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            description TEXT,
            owner_actor TEXT NOT NULL,
            published_version_id TEXT,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS qgis_project_versions (
            version_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            parent_version_id TEXT,
            status TEXT NOT NULL,
            manifest_json TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_reason TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_qgis_project_versions_project_status_created_at
        ON qgis_project_versions(project_id, status, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS qgis_remote_connections (
            connection_id TEXT PRIMARY KEY,
            project_id TEXT,
            display_name TEXT NOT NULL,
            connection_kind TEXT NOT NULL,
            config_json TEXT NOT NULL,
            secret_ref TEXT,
            created_by TEXT NOT NULL,
            updated_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_qgis_remote_connections_project_kind_updated_at
        ON qgis_remote_connections(project_id, connection_kind, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS payments (
            transaction_id TEXT PRIMARY KEY,
            task_id TEXT,
            mandate_id TEXT NOT NULL,
            amount REAL NOT NULL,
            description TEXT NOT NULL,
            status TEXT NOT NULL,
            verification_message TEXT NOT NULL,
            mcu_public_did TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_payments_task_id
        ON payments(task_id)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS approval_requests (
            approval_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            task_id TEXT,
            reference_id TEXT NOT NULL,
            status TEXT NOT NULL,
            actor TEXT,
            decision_reason TEXT,
            decision_payload TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_approval_requests_status
        ON approval_requests(status, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_approval_requests_reference
        ON approval_requests(kind, reference_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS end_user_approval_sessions (
            session_id TEXT PRIMARY KEY,
            approval_id TEXT NOT NULL,
            approval_kind TEXT NOT NULL,
            task_id TEXT,
            transaction_id TEXT,
            platform TEXT,
            chat_id TEXT,
            sender_id TEXT,
            sender_display TEXT,
            approval_token_hash TEXT NOT NULL,
            token_hint TEXT NOT NULL,
            status TEXT NOT NULL,
            expires_at_unix_ms INTEGER,
            decided_at_unix_ms INTEGER,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_end_user_approval_token_hash
        ON end_user_approval_sessions(approval_token_hash)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_end_user_approval_approval
        ON end_user_approval_sessions(approval_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS marketplace_peers (
            peer_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            base_url TEXT NOT NULL,
            catalog_url TEXT NOT NULL,
            enabled INTEGER NOT NULL,
            trust_enabled INTEGER NOT NULL,
            sync_status TEXT NOT NULL,
            last_sync_error TEXT,
            last_synced_at_unix_ms INTEGER,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS workspace_profiles (
            workspace_id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            display_name TEXT NOT NULL,
            region TEXT NOT NULL,
            default_model_providers TEXT NOT NULL,
            default_chat_platforms TEXT NOT NULL,
            onboarding_status TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS operator_sessions (
            session_id TEXT PRIMARY KEY,
            operator_name TEXT NOT NULL,
            session_token_hash TEXT NOT NULL UNIQUE,
            revoked INTEGER NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            last_seen_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS node_claims (
            claim_id TEXT PRIMARY KEY,
            node_id TEXT NOT NULL,
            display_name TEXT NOT NULL,
            transport TEXT NOT NULL,
            requested_capabilities TEXT NOT NULL,
            claim_token_hash TEXT NOT NULL UNIQUE,
            issued_by_session_id TEXT,
            issued_by_operator TEXT NOT NULL,
            status TEXT NOT NULL,
            expires_at_unix_ms INTEGER NOT NULL,
            consumed_at_unix_ms INTEGER,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_node_claims_node_id
        ON node_claims(node_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_node_claims_status
        ON node_claims(status, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS node_claim_audit_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            claim_id TEXT NOT NULL,
            node_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            actor TEXT NOT NULL,
            detail TEXT NOT NULL,
            token_hint TEXT,
            session_url TEXT,
            created_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_node_claim_audit_events_claim
        ON node_claim_audit_events(claim_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_node_claim_audit_events_node
        ON node_claim_audit_events(node_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS setup_verification_receipts (
            receipt_id TEXT PRIMARY KEY,
            surface TEXT NOT NULL,
            target TEXT NOT NULL,
            label TEXT NOT NULL,
            region TEXT NOT NULL,
            integration_mode TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT NOT NULL,
            detail TEXT NOT NULL,
            action TEXT,
            endpoint TEXT NOT NULL,
            env_keys TEXT NOT NULL,
            missing_env_keys TEXT NOT NULL,
            is_default_path INTEGER NOT NULL,
            verified_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_setup_verification_receipts_surface_target
        ON setup_verification_receipts(surface, target, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS chat_ingress_events (
            ingress_id TEXT PRIMARY KEY,
            platform TEXT NOT NULL,
            event_type TEXT NOT NULL,
            chat_id TEXT,
            sender_id TEXT,
            sender_display TEXT,
            text TEXT NOT NULL,
            raw_payload TEXT NOT NULL,
            linked_task_id TEXT,
            reply_text TEXT,
            status TEXT NOT NULL,
            error TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_ingress_events_platform
        ON chat_ingress_events(platform, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_ingress_events_status
        ON chat_ingress_events(status, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS chat_automation_modes (
            platform TEXT NOT NULL,
            chat_key TEXT NOT NULL,
            chat_id TEXT,
            sender_id TEXT,
            mode TEXT NOT NULL,
            updated_by TEXT,
            reason TEXT,
            last_ingress_id TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY(platform, chat_key)
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_automation_modes_platform
        ON chat_automation_modes(platform, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_ingress_events_linked_task_id
        ON chat_ingress_events(linked_task_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS agent_experiences (
            experience_id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            scope TEXT NOT NULL,
            task_kind TEXT NOT NULL,
            input_summary TEXT NOT NULL,
            action_summary TEXT NOT NULL,
            outcome TEXT NOT NULL,
            lesson TEXT NOT NULL,
            reusable_hint TEXT,
            evidence TEXT NOT NULL,
            tags TEXT NOT NULL,
            risk_level TEXT NOT NULL,
            related_task_id TEXT,
            related_ingress_id TEXT,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_experiences_source_updated_at
        ON agent_experiences(source, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_experiences_outcome_updated_at
        ON agent_experiences(outcome, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_experiences_task_kind_updated_at
        ON agent_experiences(task_kind, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_experiences_related_task
        ON agent_experiences(related_task_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_experiences_related_ingress
        ON agent_experiences(related_ingress_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS skill_proposals (
            proposal_id TEXT PRIMARY KEY,
            proposal_key TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            rationale TEXT NOT NULL,
            suggested_skill_id TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            confidence REAL NOT NULL,
            evidence TEXT NOT NULL,
            tags TEXT NOT NULL,
            risk_level TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_proposals_status_updated_at
        ON skill_proposals(status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_proposals_skill_id_updated_at
        ON skill_proposals(suggested_skill_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS skill_implementation_plans (
            plan_id TEXT PRIMARY KEY,
            proposal_id TEXT NOT NULL UNIQUE,
            suggested_skill_id TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            status TEXT NOT NULL,
            steps TEXT NOT NULL,
            acceptance_criteria TEXT NOT NULL,
            guardrails TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_plans_status_updated_at
        ON skill_implementation_plans(status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_plans_skill_id_updated_at
        ON skill_implementation_plans(suggested_skill_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS skill_implementation_runs (
            run_id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL UNIQUE,
            proposal_id TEXT NOT NULL,
            suggested_skill_id TEXT NOT NULL,
            status TEXT NOT NULL,
            execution_mode TEXT NOT NULL,
            change_package TEXT NOT NULL,
            verification TEXT NOT NULL,
            rollback TEXT NOT NULL,
            guardrails TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_runs_status_updated_at
        ON skill_implementation_runs(status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_runs_proposal_updated_at
        ON skill_implementation_runs(proposal_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_runs_skill_id_updated_at
        ON skill_implementation_runs(suggested_skill_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS skill_implementation_executions (
            execution_id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL,
            plan_id TEXT NOT NULL,
            proposal_id TEXT NOT NULL,
            suggested_skill_id TEXT NOT NULL,
            status TEXT NOT NULL,
            executor TEXT NOT NULL,
            preflight_report TEXT NOT NULL,
            command_plan TEXT NOT NULL,
            result TEXT NOT NULL,
            guardrails TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_executions_status_updated_at
        ON skill_implementation_executions(status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_executions_run_updated_at
        ON skill_implementation_executions(run_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_executions_proposal_updated_at
        ON skill_implementation_executions(proposal_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_executions_skill_id_updated_at
        ON skill_implementation_executions(suggested_skill_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS skill_implementation_patches (
            patch_id TEXT PRIMARY KEY,
            execution_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            plan_id TEXT NOT NULL,
            proposal_id TEXT NOT NULL,
            suggested_skill_id TEXT NOT NULL,
            status TEXT NOT NULL,
            patch_kind TEXT NOT NULL,
            summary TEXT NOT NULL,
            changed_files TEXT NOT NULL,
            patch_manifest TEXT NOT NULL,
            rollback_plan TEXT NOT NULL,
            verification_evidence TEXT NOT NULL,
            guardrails TEXT NOT NULL,
            created_by TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_patches_status_updated_at
        ON skill_implementation_patches(status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_patches_execution_updated_at
        ON skill_implementation_patches(execution_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_patches_proposal_updated_at
        ON skill_implementation_patches(proposal_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_skill_implementation_patches_skill_id_updated_at
        ON skill_implementation_patches(suggested_skill_id, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS chat_channel_identities (
            platform TEXT NOT NULL,
            identity_key TEXT NOT NULL,
            chat_id TEXT,
            sender_id TEXT,
            sender_display TEXT,
            pairing_code TEXT,
            dm_policy TEXT NOT NULL,
            decision_reason TEXT,
            last_ingress_id TEXT,
            status TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY (platform, identity_key)
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_channel_identities_status
        ON chat_channel_identities(status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_channel_identities_platform_status
        ON chat_channel_identities(platform, status, updated_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS nodes (
            node_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            transport TEXT NOT NULL,
            capabilities TEXT NOT NULL,
            attestation_issuer_did TEXT,
            attestation_signature_hex TEXT,
            attestation_document_hash TEXT,
            attestation_issued_at_unix_ms INTEGER,
            attestation_verified INTEGER NOT NULL DEFAULT 0,
            attestation_verified_at_unix_ms INTEGER,
            attestation_error TEXT,
            status TEXT NOT NULL,
            connected INTEGER NOT NULL,
            last_seen_unix_ms INTEGER NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS node_commands (
            command_id TEXT PRIMARY KEY,
            node_id TEXT NOT NULL,
            command_type TEXT NOT NULL,
            payload TEXT NOT NULL,
            status TEXT NOT NULL,
            result TEXT,
            error TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_node_commands_node_id_created_at
        ON node_commands(node_id, created_at_unix_ms)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS node_rollouts (
            node_id TEXT PRIMARY KEY,
            bundle_hash TEXT NOT NULL,
            policy_version INTEGER NOT NULL,
            policy_document_hash TEXT,
            skill_distribution_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            last_error TEXT,
            last_sent_at_unix_ms INTEGER NOT NULL,
            last_ack_at_unix_ms INTEGER,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS node_trust_roots (
            issuer_did TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            public_key_hex TEXT NOT NULL,
            updated_by TEXT NOT NULL,
            updated_reason TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS orchestration_runs (
            task_id TEXT PRIMARY KEY,
            plan_json TEXT NOT NULL,
            next_step_index INTEGER NOT NULL,
            last_result TEXT,
            waiting_transaction_id TEXT,
            status TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_orchestration_runs_waiting_transaction_id
        ON orchestration_runs(waiting_transaction_id)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS policy_profiles (
            policy_id TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            issuer_did TEXT,
            allow_shell_exec INTEGER NOT NULL,
            allowed_model_providers TEXT NOT NULL,
            allowed_chat_platforms TEXT NOT NULL,
            max_payment_amount REAL,
            signature_hex TEXT,
            document_hash TEXT,
            issued_at_unix_ms INTEGER,
            updated_reason TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS policy_audit_events (
            audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
            policy_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            actor TEXT NOT NULL,
            summary TEXT NOT NULL,
            snapshot TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_policy_audit_events_policy_id_audit_id
        ON policy_audit_events(policy_id, audit_id DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS policy_trust_roots (
            issuer_did TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            public_key_hex TEXT NOT NULL,
            updated_by TEXT NOT NULL,
            updated_reason TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS wasm_skills (
            skill_id TEXT NOT NULL,
            version TEXT NOT NULL,
            display_name TEXT NOT NULL,
            description TEXT,
            entry_function TEXT NOT NULL,
            capabilities TEXT NOT NULL,
            artifact_path TEXT NOT NULL,
            artifact_sha256 TEXT NOT NULL,
            source_kind TEXT NOT NULL DEFAULT 'unsigned_local',
            issuer_did TEXT,
            signature_hex TEXT,
            document_hash TEXT,
            issued_at_unix_ms INTEGER,
            active INTEGER NOT NULL DEFAULT 0,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY (skill_id, version)
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_wasm_skills_active
        ON wasm_skills(skill_id, active)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS skill_publisher_trust_roots (
            issuer_did TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            public_key_hex TEXT NOT NULL,
            updated_by TEXT NOT NULL,
            updated_reason TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS agent_cards (
            card_id TEXT PRIMARY KEY,
            card_json TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            card_url TEXT,
            published INTEGER NOT NULL DEFAULT 1,
            locally_hosted INTEGER NOT NULL DEFAULT 0,
            issuer_did TEXT,
            signature_hex TEXT,
            regions TEXT NOT NULL,
            languages TEXT NOT NULL,
            model_providers TEXT NOT NULL,
            chat_platforms TEXT NOT NULL,
            payment_roles TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_cards_published
        ON agent_cards(published, locally_hosted, updated_at_unix_ms)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS remote_agent_invocations (
            invocation_id TEXT PRIMARY KEY,
            card_id TEXT NOT NULL,
            remote_agent_url TEXT NOT NULL,
            local_task_id TEXT,
            remote_task_id TEXT,
            request_json TEXT NOT NULL,
            response_json TEXT,
            status TEXT NOT NULL,
            error TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_remote_agent_invocations_card_id
        ON remote_agent_invocations(card_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_remote_agent_invocations_local_task_id
        ON remote_agent_invocations(local_task_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS remote_agent_settlements (
            settlement_id TEXT PRIMARY KEY,
            invocation_id TEXT NOT NULL,
            card_id TEXT NOT NULL,
            remote_agent_url TEXT NOT NULL,
            local_task_id TEXT,
            remote_task_id TEXT,
            transaction_id TEXT NOT NULL UNIQUE,
            mandate_id TEXT NOT NULL,
            quote_id TEXT,
            amount REAL NOT NULL,
            description TEXT NOT NULL,
            status TEXT NOT NULL,
            verification_message TEXT NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_remote_agent_settlements_invocation_id
        ON remote_agent_settlements(invocation_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_remote_agent_settlements_card_id
        ON remote_agent_settlements(card_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_remote_agent_settlements_local_task_id
        ON remote_agent_settlements(local_task_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_remote_agent_settlements_quote_id
        ON remote_agent_settlements(quote_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS agent_settlement_reconciliation (
            reconciliation_id TEXT PRIMARY KEY,
            direction TEXT NOT NULL,
            settlement_id TEXT NOT NULL,
            card_id TEXT NOT NULL,
            invocation_id TEXT,
            transaction_id TEXT NOT NULL,
            remote_agent_url TEXT,
            settlement_status TEXT NOT NULL,
            reconciliation_status TEXT NOT NULL,
            receipt_issuer_did TEXT NOT NULL,
            receipt_signature_hex TEXT NOT NULL,
            receipt_json TEXT NOT NULL,
            acknowledgment_issuer_did TEXT,
            acknowledgment_signature_hex TEXT,
            acknowledgment_json TEXT,
            last_error TEXT,
            last_sync_at_unix_ms INTEGER,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_settlement_reconciliation_direction_settlement
        ON agent_settlement_reconciliation(direction, settlement_id)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_settlement_reconciliation_card_id
        ON agent_settlement_reconciliation(card_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_settlement_reconciliation_transaction_id
        ON agent_settlement_reconciliation(transaction_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS agent_quote_ledger (
            quote_id TEXT PRIMARY KEY,
            card_id TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            quote_url TEXT,
            state_subscriber_url TEXT,
            previous_quote_id TEXT,
            superseded_by_quote_id TEXT,
            negotiation_round INTEGER NOT NULL,
            settlement_supported INTEGER NOT NULL,
            payment_roles TEXT NOT NULL,
            currency TEXT,
            quote_mode TEXT NOT NULL,
            requested_amount REAL,
            quoted_amount REAL,
            counter_offer_amount REAL,
            min_amount REAL,
            max_amount REAL,
            description_template TEXT,
            warning TEXT,
            expires_at_unix_ms INTEGER,
            issuer_did TEXT,
            signature_hex TEXT,
            status TEXT NOT NULL,
            consumed_by_transaction_id TEXT,
            revoked_reason TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_quote_ledger_card_id
        ON agent_quote_ledger(card_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_quote_ledger_previous_quote_id
        ON agent_quote_ledger(previous_quote_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_quote_ledger_status
        ON agent_quote_ledger(status, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_quote_ledger_consumed_by_transaction_id
        ON agent_quote_ledger(consumed_by_transaction_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS agent_delivery_outbox (
            delivery_id TEXT PRIMARY KEY,
            delivery_key TEXT NOT NULL UNIQUE,
            delivery_kind TEXT NOT NULL,
            card_id TEXT NOT NULL,
            settlement_id TEXT,
            reconciliation_id TEXT,
            quote_id TEXT,
            target_url TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL,
            max_attempts INTEGER NOT NULL,
            next_attempt_at_unix_ms INTEGER NOT NULL,
            last_attempt_at_unix_ms INTEGER,
            delivered_at_unix_ms INTEGER,
            last_http_status INTEGER,
            last_error TEXT,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_delivery_outbox_status_due
        ON agent_delivery_outbox(status, next_attempt_at_unix_ms ASC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_delivery_outbox_settlement_id
        ON agent_delivery_outbox(settlement_id, created_at_unix_ms DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_delivery_outbox_quote_id
        ON agent_delivery_outbox(quote_id, created_at_unix_ms DESC)
        "#,
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .with_context(|| format!("failed to run migration statement: {statement}"))?;
    }

    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_issuer_did",
        "ALTER TABLE nodes ADD COLUMN attestation_issuer_did TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_signature_hex",
        "ALTER TABLE nodes ADD COLUMN attestation_signature_hex TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_document_hash",
        "ALTER TABLE nodes ADD COLUMN attestation_document_hash TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_issued_at_unix_ms",
        "ALTER TABLE nodes ADD COLUMN attestation_issued_at_unix_ms INTEGER",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_verified",
        "ALTER TABLE nodes ADD COLUMN attestation_verified INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_verified_at_unix_ms",
        "ALTER TABLE nodes ADD COLUMN attestation_verified_at_unix_ms INTEGER",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "nodes",
        "attestation_error",
        "ALTER TABLE nodes ADD COLUMN attestation_error TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "policy_profiles",
        "issuer_did",
        "ALTER TABLE policy_profiles ADD COLUMN issuer_did TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "policy_profiles",
        "signature_hex",
        "ALTER TABLE policy_profiles ADD COLUMN signature_hex TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "policy_profiles",
        "document_hash",
        "ALTER TABLE policy_profiles ADD COLUMN document_hash TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "policy_profiles",
        "issued_at_unix_ms",
        "ALTER TABLE policy_profiles ADD COLUMN issued_at_unix_ms INTEGER",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "wasm_skills",
        "source_kind",
        "ALTER TABLE wasm_skills ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'unsigned_local'",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "wasm_skills",
        "issuer_did",
        "ALTER TABLE wasm_skills ADD COLUMN issuer_did TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "wasm_skills",
        "signature_hex",
        "ALTER TABLE wasm_skills ADD COLUMN signature_hex TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "wasm_skills",
        "document_hash",
        "ALTER TABLE wasm_skills ADD COLUMN document_hash TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "wasm_skills",
        "issued_at_unix_ms",
        "ALTER TABLE wasm_skills ADD COLUMN issued_at_unix_ms INTEGER",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "remote_agent_settlements",
        "quote_id",
        "ALTER TABLE remote_agent_settlements ADD COLUMN quote_id TEXT",
    )
    .await?;
    ensure_sqlite_column(
        pool,
        "agent_quote_ledger",
        "state_subscriber_url",
        "ALTER TABLE agent_quote_ledger ADD COLUMN state_subscriber_url TEXT",
    )
    .await?;

    Ok(())
}

async fn save_task(pool: &SqlitePool, task: &StoredTask) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO tasks (
            task_id,
            parent_task_id,
            name,
            instruction,
            status,
            linked_payment_id,
            last_update_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(task_id) DO UPDATE SET
            parent_task_id = excluded.parent_task_id,
            name = excluded.name,
            instruction = excluded.instruction,
            status = excluded.status,
            linked_payment_id = excluded.linked_payment_id,
            last_update_reason = excluded.last_update_reason,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(task.task_id.to_string())
    .bind(task.parent_task_id.map(|value| value.to_string()))
    .bind(&task.name)
    .bind(&task.instruction)
    .bind(task.status.as_db())
    .bind(task.linked_payment_id.map(|value| value.to_string()))
    .bind(&task.last_update_reason)
    .bind(u128_to_i64(task.created_at_unix_ms)?)
    .bind(u128_to_i64(task.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save task")?;

    Ok(())
}

async fn save_payment(pool: &SqlitePool, payment: &PaymentRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO payments (
            transaction_id,
            task_id,
            mandate_id,
            amount,
            description,
            status,
            verification_message,
            mcu_public_did,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(transaction_id) DO UPDATE SET
            task_id = excluded.task_id,
            mandate_id = excluded.mandate_id,
            amount = excluded.amount,
            description = excluded.description,
            status = excluded.status,
            verification_message = excluded.verification_message,
            mcu_public_did = excluded.mcu_public_did,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(payment.transaction_id.to_string())
    .bind(payment.task_id.map(|value| value.to_string()))
    .bind(payment.mandate_id.to_string())
    .bind(payment.amount)
    .bind(&payment.description)
    .bind(payment.status.as_db())
    .bind(&payment.verification_message)
    .bind(&payment.mcu_public_did)
    .bind(u128_to_i64(payment.created_at_unix_ms)?)
    .bind(u128_to_i64(payment.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save payment")?;

    Ok(())
}

async fn save_approval_request(
    pool: &SqlitePool,
    approval: &ApprovalRequestRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO approval_requests (
            approval_id,
            kind,
            title,
            summary,
            task_id,
            reference_id,
            status,
            actor,
            decision_reason,
            decision_payload,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(approval_id) DO UPDATE SET
            kind = excluded.kind,
            title = excluded.title,
            summary = excluded.summary,
            task_id = excluded.task_id,
            reference_id = excluded.reference_id,
            status = excluded.status,
            actor = excluded.actor,
            decision_reason = excluded.decision_reason,
            decision_payload = excluded.decision_payload,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(approval.approval_id.to_string())
    .bind(approval.kind.as_db())
    .bind(&approval.title)
    .bind(&approval.summary)
    .bind(approval.task_id.map(|value| value.to_string()))
    .bind(&approval.reference_id)
    .bind(approval.status.as_db())
    .bind(&approval.actor)
    .bind(&approval.decision_reason)
    .bind(
        approval
            .decision_payload
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(u128_to_i64(approval.created_at_unix_ms)?)
    .bind(u128_to_i64(approval.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save approval request")?;

    Ok(())
}

async fn save_end_user_approval_session(
    pool: &SqlitePool,
    session: &EndUserApprovalSessionRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO end_user_approval_sessions (
            session_id,
            approval_id,
            approval_kind,
            task_id,
            transaction_id,
            platform,
            chat_id,
            sender_id,
            sender_display,
            approval_token_hash,
            token_hint,
            status,
            expires_at_unix_ms,
            decided_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ON CONFLICT(session_id) DO UPDATE SET
            approval_id = excluded.approval_id,
            approval_kind = excluded.approval_kind,
            task_id = excluded.task_id,
            transaction_id = excluded.transaction_id,
            platform = excluded.platform,
            chat_id = excluded.chat_id,
            sender_id = excluded.sender_id,
            sender_display = excluded.sender_display,
            approval_token_hash = excluded.approval_token_hash,
            token_hint = excluded.token_hint,
            status = excluded.status,
            expires_at_unix_ms = excluded.expires_at_unix_ms,
            decided_at_unix_ms = excluded.decided_at_unix_ms,
            created_at_unix_ms = end_user_approval_sessions.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(session.session_id.to_string())
    .bind(session.approval_id.to_string())
    .bind(session.approval_kind.as_db())
    .bind(session.task_id.map(|value| value.to_string()))
    .bind(session.transaction_id.map(|value| value.to_string()))
    .bind(&session.platform)
    .bind(&session.chat_id)
    .bind(&session.sender_id)
    .bind(&session.sender_display)
    .bind(&session.approval_token_hash)
    .bind(&session.token_hint)
    .bind(session.status.as_db())
    .bind(session.expires_at_unix_ms.map(u128_to_i64).transpose()?)
    .bind(session.decided_at_unix_ms.map(u128_to_i64).transpose()?)
    .bind(u128_to_i64(session.created_at_unix_ms)?)
    .bind(u128_to_i64(session.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save end-user approval session")?;

    Ok(())
}

async fn save_marketplace_peer(
    pool: &SqlitePool,
    peer: &MarketplacePeerRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO marketplace_peers (
            peer_id,
            display_name,
            base_url,
            catalog_url,
            enabled,
            trust_enabled,
            sync_status,
            last_sync_error,
            last_synced_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(peer_id) DO UPDATE SET
            display_name = excluded.display_name,
            base_url = excluded.base_url,
            catalog_url = excluded.catalog_url,
            enabled = excluded.enabled,
            trust_enabled = excluded.trust_enabled,
            sync_status = excluded.sync_status,
            last_sync_error = excluded.last_sync_error,
            last_synced_at_unix_ms = excluded.last_synced_at_unix_ms,
            created_at_unix_ms = marketplace_peers.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&peer.peer_id)
    .bind(&peer.display_name)
    .bind(&peer.base_url)
    .bind(&peer.catalog_url)
    .bind(peer.enabled)
    .bind(peer.trust_enabled)
    .bind(peer.sync_status.as_db())
    .bind(&peer.last_sync_error)
    .bind(peer.last_synced_at_unix_ms.map(u128_to_i64).transpose()?)
    .bind(u128_to_i64(peer.created_at_unix_ms)?)
    .bind(u128_to_i64(peer.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save marketplace peer")?;

    Ok(())
}

async fn save_chat_ingress_event(
    pool: &SqlitePool,
    event: &ChatIngressEventRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO chat_ingress_events (
            ingress_id,
            platform,
            event_type,
            chat_id,
            sender_id,
            sender_display,
            text,
            raw_payload,
            linked_task_id,
            reply_text,
            status,
            error,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ON CONFLICT(ingress_id) DO UPDATE SET
            platform = excluded.platform,
            event_type = excluded.event_type,
            chat_id = excluded.chat_id,
            sender_id = excluded.sender_id,
            sender_display = excluded.sender_display,
            text = excluded.text,
            raw_payload = excluded.raw_payload,
            linked_task_id = excluded.linked_task_id,
            reply_text = excluded.reply_text,
            status = excluded.status,
            error = excluded.error,
            created_at_unix_ms = chat_ingress_events.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(event.ingress_id.to_string())
    .bind(&event.platform)
    .bind(&event.event_type)
    .bind(&event.chat_id)
    .bind(&event.sender_id)
    .bind(&event.sender_display)
    .bind(&event.text)
    .bind(serde_json::to_string(&event.raw_payload).context("failed to serialize ingress payload")?)
    .bind(event.linked_task_id.map(|value| value.to_string()))
    .bind(&event.reply_text)
    .bind(event.status.as_db())
    .bind(&event.error)
    .bind(u128_to_i64(event.created_at_unix_ms)?)
    .bind(u128_to_i64(event.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save chat ingress event")?;

    Ok(())
}

async fn save_agent_experience(
    pool: &SqlitePool,
    experience: &AgentExperienceRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO agent_experiences (
            experience_id,
            source,
            scope,
            task_kind,
            input_summary,
            action_summary,
            outcome,
            lesson,
            reusable_hint,
            evidence,
            tags,
            risk_level,
            related_task_id,
            related_ingress_id,
            created_by,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        ON CONFLICT(experience_id) DO UPDATE SET
            source = excluded.source,
            scope = excluded.scope,
            task_kind = excluded.task_kind,
            input_summary = excluded.input_summary,
            action_summary = excluded.action_summary,
            outcome = excluded.outcome,
            lesson = excluded.lesson,
            reusable_hint = excluded.reusable_hint,
            evidence = excluded.evidence,
            tags = excluded.tags,
            risk_level = excluded.risk_level,
            related_task_id = excluded.related_task_id,
            related_ingress_id = excluded.related_ingress_id,
            created_by = excluded.created_by,
            created_at_unix_ms = agent_experiences.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(experience.experience_id.to_string())
    .bind(&experience.source)
    .bind(&experience.scope)
    .bind(&experience.task_kind)
    .bind(&experience.input_summary)
    .bind(&experience.action_summary)
    .bind(&experience.outcome)
    .bind(&experience.lesson)
    .bind(&experience.reusable_hint)
    .bind(
        serde_json::to_string(&experience.evidence)
            .context("failed to serialize agent experience evidence")?,
    )
    .bind(
        serde_json::to_string(&experience.tags)
            .context("failed to serialize agent experience tags")?,
    )
    .bind(&experience.risk_level)
    .bind(experience.related_task_id.map(|value| value.to_string()))
    .bind(experience.related_ingress_id.map(|value| value.to_string()))
    .bind(&experience.created_by)
    .bind(u128_to_i64(experience.created_at_unix_ms)?)
    .bind(u128_to_i64(experience.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save agent experience")?;

    Ok(())
}

async fn save_skill_proposal(
    pool: &SqlitePool,
    proposal: &SkillProposalRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO skill_proposals (
            proposal_id,
            proposal_key,
            title,
            summary,
            rationale,
            suggested_skill_id,
            source,
            status,
            confidence,
            evidence,
            tags,
            risk_level,
            created_by,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(proposal_key) DO UPDATE SET
            title = excluded.title,
            summary = excluded.summary,
            rationale = excluded.rationale,
            suggested_skill_id = excluded.suggested_skill_id,
            source = excluded.source,
            status = skill_proposals.status,
            confidence = excluded.confidence,
            evidence = excluded.evidence,
            tags = excluded.tags,
            risk_level = excluded.risk_level,
            created_by = excluded.created_by,
            created_at_unix_ms = skill_proposals.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(proposal.proposal_id.to_string())
    .bind(&proposal.proposal_key)
    .bind(&proposal.title)
    .bind(&proposal.summary)
    .bind(&proposal.rationale)
    .bind(&proposal.suggested_skill_id)
    .bind(&proposal.source)
    .bind(&proposal.status)
    .bind(proposal.confidence)
    .bind(
        serde_json::to_string(&proposal.evidence)
            .context("failed to serialize skill proposal evidence")?,
    )
    .bind(serde_json::to_string(&proposal.tags).context("failed to serialize skill proposal tags")?)
    .bind(&proposal.risk_level)
    .bind(&proposal.created_by)
    .bind(u128_to_i64(proposal.created_at_unix_ms)?)
    .bind(u128_to_i64(proposal.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save skill proposal")?;

    Ok(())
}

async fn save_skill_implementation_plan(
    pool: &SqlitePool,
    plan: &SkillImplementationPlanRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO skill_implementation_plans (
            plan_id,
            proposal_id,
            suggested_skill_id,
            title,
            summary,
            status,
            steps,
            acceptance_criteria,
            guardrails,
            created_by,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(proposal_id) DO UPDATE SET
            suggested_skill_id = excluded.suggested_skill_id,
            title = excluded.title,
            summary = excluded.summary,
            status = skill_implementation_plans.status,
            steps = excluded.steps,
            acceptance_criteria = excluded.acceptance_criteria,
            guardrails = excluded.guardrails,
            created_by = excluded.created_by,
            created_at_unix_ms = skill_implementation_plans.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(plan.plan_id.to_string())
    .bind(plan.proposal_id.to_string())
    .bind(&plan.suggested_skill_id)
    .bind(&plan.title)
    .bind(&plan.summary)
    .bind(&plan.status)
    .bind(
        serde_json::to_string(&plan.steps)
            .context("failed to serialize skill implementation plan steps")?,
    )
    .bind(
        serde_json::to_string(&plan.acceptance_criteria)
            .context("failed to serialize skill implementation plan acceptance criteria")?,
    )
    .bind(
        serde_json::to_string(&plan.guardrails)
            .context("failed to serialize skill implementation plan guardrails")?,
    )
    .bind(&plan.created_by)
    .bind(u128_to_i64(plan.created_at_unix_ms)?)
    .bind(u128_to_i64(plan.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save skill implementation plan")?;

    Ok(())
}

async fn save_skill_implementation_run(
    pool: &SqlitePool,
    run: &SkillImplementationRunRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO skill_implementation_runs (
            run_id,
            plan_id,
            proposal_id,
            suggested_skill_id,
            status,
            execution_mode,
            change_package,
            verification,
            rollback,
            guardrails,
            created_by,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(plan_id) DO UPDATE SET
            proposal_id = excluded.proposal_id,
            suggested_skill_id = excluded.suggested_skill_id,
            status = skill_implementation_runs.status,
            execution_mode = excluded.execution_mode,
            change_package = excluded.change_package,
            verification = excluded.verification,
            rollback = excluded.rollback,
            guardrails = excluded.guardrails,
            created_by = excluded.created_by,
            created_at_unix_ms = skill_implementation_runs.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(run.run_id.to_string())
    .bind(run.plan_id.to_string())
    .bind(run.proposal_id.to_string())
    .bind(&run.suggested_skill_id)
    .bind(&run.status)
    .bind(&run.execution_mode)
    .bind(
        serde_json::to_string(&run.change_package)
            .context("failed to serialize skill implementation run change package")?,
    )
    .bind(
        serde_json::to_string(&run.verification)
            .context("failed to serialize skill implementation run verification")?,
    )
    .bind(
        serde_json::to_string(&run.rollback)
            .context("failed to serialize skill implementation run rollback")?,
    )
    .bind(
        serde_json::to_string(&run.guardrails)
            .context("failed to serialize skill implementation run guardrails")?,
    )
    .bind(&run.created_by)
    .bind(u128_to_i64(run.created_at_unix_ms)?)
    .bind(u128_to_i64(run.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save skill implementation run")?;

    Ok(())
}

async fn save_skill_implementation_execution(
    pool: &SqlitePool,
    execution: &SkillImplementationExecutionRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO skill_implementation_executions (
            execution_id,
            run_id,
            plan_id,
            proposal_id,
            suggested_skill_id,
            status,
            executor,
            preflight_report,
            command_plan,
            result,
            guardrails,
            created_by,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ON CONFLICT(execution_id) DO UPDATE SET
            run_id = excluded.run_id,
            plan_id = excluded.plan_id,
            proposal_id = excluded.proposal_id,
            suggested_skill_id = excluded.suggested_skill_id,
            status = excluded.status,
            executor = excluded.executor,
            preflight_report = excluded.preflight_report,
            command_plan = excluded.command_plan,
            result = excluded.result,
            guardrails = excluded.guardrails,
            created_by = excluded.created_by,
            created_at_unix_ms = skill_implementation_executions.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(execution.execution_id.to_string())
    .bind(execution.run_id.to_string())
    .bind(execution.plan_id.to_string())
    .bind(execution.proposal_id.to_string())
    .bind(&execution.suggested_skill_id)
    .bind(&execution.status)
    .bind(&execution.executor)
    .bind(
        serde_json::to_string(&execution.preflight_report)
            .context("failed to serialize skill implementation execution preflight report")?,
    )
    .bind(
        serde_json::to_string(&execution.command_plan)
            .context("failed to serialize skill implementation execution command plan")?,
    )
    .bind(
        serde_json::to_string(&execution.result)
            .context("failed to serialize skill implementation execution result")?,
    )
    .bind(
        serde_json::to_string(&execution.guardrails)
            .context("failed to serialize skill implementation execution guardrails")?,
    )
    .bind(&execution.created_by)
    .bind(u128_to_i64(execution.created_at_unix_ms)?)
    .bind(u128_to_i64(execution.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save skill implementation execution")?;

    Ok(())
}

async fn save_skill_implementation_patch(
    pool: &SqlitePool,
    patch: &SkillImplementationPatchRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO skill_implementation_patches (
            patch_id,
            execution_id,
            run_id,
            plan_id,
            proposal_id,
            suggested_skill_id,
            status,
            patch_kind,
            summary,
            changed_files,
            patch_manifest,
            rollback_plan,
            verification_evidence,
            guardrails,
            created_by,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        ON CONFLICT(patch_id) DO UPDATE SET
            execution_id = excluded.execution_id,
            run_id = excluded.run_id,
            plan_id = excluded.plan_id,
            proposal_id = excluded.proposal_id,
            suggested_skill_id = excluded.suggested_skill_id,
            status = excluded.status,
            patch_kind = excluded.patch_kind,
            summary = excluded.summary,
            changed_files = excluded.changed_files,
            patch_manifest = excluded.patch_manifest,
            rollback_plan = excluded.rollback_plan,
            verification_evidence = excluded.verification_evidence,
            guardrails = excluded.guardrails,
            created_by = excluded.created_by,
            created_at_unix_ms = skill_implementation_patches.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(patch.patch_id.to_string())
    .bind(patch.execution_id.to_string())
    .bind(patch.run_id.to_string())
    .bind(patch.plan_id.to_string())
    .bind(patch.proposal_id.to_string())
    .bind(&patch.suggested_skill_id)
    .bind(&patch.status)
    .bind(&patch.patch_kind)
    .bind(&patch.summary)
    .bind(
        serde_json::to_string(&patch.changed_files)
            .context("failed to serialize skill implementation patch changed files")?,
    )
    .bind(
        serde_json::to_string(&patch.patch_manifest)
            .context("failed to serialize skill implementation patch manifest")?,
    )
    .bind(
        serde_json::to_string(&patch.rollback_plan)
            .context("failed to serialize skill implementation patch rollback plan")?,
    )
    .bind(
        serde_json::to_string(&patch.verification_evidence)
            .context("failed to serialize skill implementation patch verification evidence")?,
    )
    .bind(
        serde_json::to_string(&patch.guardrails)
            .context("failed to serialize skill implementation patch guardrails")?,
    )
    .bind(&patch.created_by)
    .bind(u128_to_i64(patch.created_at_unix_ms)?)
    .bind(u128_to_i64(patch.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save skill implementation patch")?;

    Ok(())
}

async fn save_chat_channel_identity(
    pool: &SqlitePool,
    identity: &ChatChannelIdentityRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO chat_channel_identities (
            platform,
            identity_key,
            chat_id,
            sender_id,
            sender_display,
            pairing_code,
            dm_policy,
            decision_reason,
            last_ingress_id,
            status,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(platform, identity_key) DO UPDATE SET
            chat_id = excluded.chat_id,
            sender_id = excluded.sender_id,
            sender_display = excluded.sender_display,
            pairing_code = excluded.pairing_code,
            dm_policy = excluded.dm_policy,
            decision_reason = excluded.decision_reason,
            last_ingress_id = excluded.last_ingress_id,
            status = excluded.status,
            created_at_unix_ms = chat_channel_identities.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&identity.platform)
    .bind(&identity.identity_key)
    .bind(&identity.chat_id)
    .bind(&identity.sender_id)
    .bind(&identity.sender_display)
    .bind(&identity.pairing_code)
    .bind(&identity.dm_policy)
    .bind(&identity.decision_reason)
    .bind(identity.last_ingress_id.map(|value| value.to_string()))
    .bind(identity.status.as_db())
    .bind(u128_to_i64(identity.created_at_unix_ms)?)
    .bind(u128_to_i64(identity.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save chat channel identity")?;

    Ok(())
}

async fn save_chat_automation_mode(
    pool: &SqlitePool,
    record: &ChatAutomationModeRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO chat_automation_modes (
            platform,
            chat_key,
            chat_id,
            sender_id,
            mode,
            updated_by,
            reason,
            last_ingress_id,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(platform, chat_key) DO UPDATE SET
            chat_id = excluded.chat_id,
            sender_id = excluded.sender_id,
            mode = excluded.mode,
            updated_by = excluded.updated_by,
            reason = excluded.reason,
            last_ingress_id = excluded.last_ingress_id,
            created_at_unix_ms = chat_automation_modes.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&record.platform)
    .bind(&record.chat_key)
    .bind(&record.chat_id)
    .bind(&record.sender_id)
    .bind(record.mode.as_db())
    .bind(&record.updated_by)
    .bind(&record.reason)
    .bind(record.last_ingress_id.map(|value| value.to_string()))
    .bind(u128_to_i64(record.created_at_unix_ms)?)
    .bind(u128_to_i64(record.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save chat automation mode")?;

    Ok(())
}

async fn save_node(pool: &SqlitePool, node: &NodeRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO nodes (
            node_id,
            display_name,
            transport,
            capabilities,
            attestation_issuer_did,
            attestation_signature_hex,
            attestation_document_hash,
            attestation_issued_at_unix_ms,
            attestation_verified,
            attestation_verified_at_unix_ms,
            attestation_error,
            status,
            connected,
            last_seen_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ON CONFLICT(node_id) DO UPDATE SET
            display_name = excluded.display_name,
            transport = excluded.transport,
            capabilities = excluded.capabilities,
            attestation_issuer_did = excluded.attestation_issuer_did,
            attestation_signature_hex = excluded.attestation_signature_hex,
            attestation_document_hash = excluded.attestation_document_hash,
            attestation_issued_at_unix_ms = excluded.attestation_issued_at_unix_ms,
            attestation_verified = excluded.attestation_verified,
            attestation_verified_at_unix_ms = excluded.attestation_verified_at_unix_ms,
            attestation_error = excluded.attestation_error,
            status = excluded.status,
            connected = excluded.connected,
            last_seen_unix_ms = excluded.last_seen_unix_ms,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&node.node_id)
    .bind(&node.display_name)
    .bind(&node.transport)
    .bind(serde_json::to_string(&node.capabilities).context("failed to serialize capabilities")?)
    .bind(&node.attestation_issuer_did)
    .bind(&node.attestation_signature_hex)
    .bind(&node.attestation_document_hash)
    .bind(
        node.attestation_issued_at_unix_ms
            .map(u128_to_i64)
            .transpose()?,
    )
    .bind(node.attestation_verified)
    .bind(
        node.attestation_verified_at_unix_ms
            .map(u128_to_i64)
            .transpose()?,
    )
    .bind(&node.attestation_error)
    .bind(node.status.as_db())
    .bind(node.connected)
    .bind(u128_to_i64(node.last_seen_unix_ms)?)
    .bind(u128_to_i64(node.created_at_unix_ms)?)
    .bind(u128_to_i64(node.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save node")?;

    Ok(())
}

async fn save_node_trust_root(
    pool: &SqlitePool,
    trust_root: &NodeTrustRootRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_trust_roots (
            issuer_did,
            label,
            public_key_hex,
            updated_by,
            updated_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(issuer_did) DO UPDATE SET
            label = excluded.label,
            public_key_hex = excluded.public_key_hex,
            updated_by = excluded.updated_by,
            updated_reason = excluded.updated_reason,
            created_at_unix_ms = node_trust_roots.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&trust_root.issuer_did)
    .bind(&trust_root.label)
    .bind(&trust_root.public_key_hex)
    .bind(&trust_root.updated_by)
    .bind(&trust_root.updated_reason)
    .bind(u128_to_i64(trust_root.created_at_unix_ms)?)
    .bind(u128_to_i64(trust_root.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save node trust root")?;

    Ok(())
}

async fn save_node_command(pool: &SqlitePool, command: &NodeCommandRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_commands (
            command_id,
            node_id,
            command_type,
            payload,
            status,
            result,
            error,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(command_id) DO UPDATE SET
            node_id = excluded.node_id,
            command_type = excluded.command_type,
            payload = excluded.payload,
            status = excluded.status,
            result = excluded.result,
            error = excluded.error,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(command.command_id.to_string())
    .bind(&command.node_id)
    .bind(&command.command_type)
    .bind(serde_json::to_string(&command.payload).context("failed to serialize command payload")?)
    .bind(command.status.as_db())
    .bind(
        command
            .result
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("failed to serialize command result")?,
    )
    .bind(&command.error)
    .bind(u128_to_i64(command.created_at_unix_ms)?)
    .bind(u128_to_i64(command.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save node command")?;

    Ok(())
}

async fn save_node_rollout(pool: &SqlitePool, rollout: &NodeRolloutRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_rollouts (
            node_id,
            bundle_hash,
            policy_version,
            policy_document_hash,
            skill_distribution_hash,
            status,
            last_error,
            last_sent_at_unix_ms,
            last_ack_at_unix_ms,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(node_id) DO UPDATE SET
            bundle_hash = excluded.bundle_hash,
            policy_version = excluded.policy_version,
            policy_document_hash = excluded.policy_document_hash,
            skill_distribution_hash = excluded.skill_distribution_hash,
            status = excluded.status,
            last_error = excluded.last_error,
            last_sent_at_unix_ms = excluded.last_sent_at_unix_ms,
            last_ack_at_unix_ms = excluded.last_ack_at_unix_ms,
            created_at_unix_ms = node_rollouts.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&rollout.node_id)
    .bind(&rollout.bundle_hash)
    .bind(i64::from(rollout.policy_version))
    .bind(&rollout.policy_document_hash)
    .bind(&rollout.skill_distribution_hash)
    .bind(rollout.status.as_db())
    .bind(&rollout.last_error)
    .bind(u128_to_i64(rollout.last_sent_at_unix_ms)?)
    .bind(rollout.last_ack_at_unix_ms.map(u128_to_i64).transpose()?)
    .bind(u128_to_i64(rollout.created_at_unix_ms)?)
    .bind(u128_to_i64(rollout.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .with_context(|| format!("failed to save node rollout '{}'", rollout.node_id))?;

    Ok(())
}

async fn save_orchestration_run(
    pool: &SqlitePool,
    run: &OrchestrationRunRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO orchestration_runs (
            task_id,
            plan_json,
            next_step_index,
            last_result,
            waiting_transaction_id,
            status,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(task_id) DO UPDATE SET
            plan_json = excluded.plan_json,
            next_step_index = excluded.next_step_index,
            last_result = excluded.last_result,
            waiting_transaction_id = excluded.waiting_transaction_id,
            status = excluded.status,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(run.task_id.to_string())
    .bind(&run.plan_json)
    .bind(i64::from(run.next_step_index))
    .bind(
        run.last_result
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("failed to serialize orchestration last_result")?,
    )
    .bind(run.waiting_transaction_id.map(|value| value.to_string()))
    .bind(run.status.as_db())
    .bind(u128_to_i64(run.created_at_unix_ms)?)
    .bind(u128_to_i64(run.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save orchestration run")?;

    Ok(())
}

async fn ensure_default_policy_profile(pool: &SqlitePool) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM policy_profiles
        WHERE policy_id = 'default'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count policy profiles")?;

    if count > 0 {
        return Ok(());
    }

    let now = unix_timestamp_ms();
    let profile = PolicyProfileRecord {
        policy_id: "default".to_string(),
        version: 1,
        issuer_did: None,
        allow_shell_exec: false,
        allowed_model_providers: Vec::new(),
        allowed_chat_platforms: Vec::new(),
        max_payment_amount: None,
        signature_hex: None,
        document_hash: None,
        issued_at_unix_ms: None,
        updated_reason: "bootstrap default policy".to_string(),
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    save_policy_profile(pool, &profile).await?;
    let snapshot = serde_json::to_value(&profile).context("failed to serialize default policy")?;
    sqlx::query(
        r#"
        INSERT INTO policy_audit_events (
            policy_id,
            version,
            actor,
            summary,
            snapshot,
            created_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
    )
    .bind(&profile.policy_id)
    .bind(i64::from(profile.version))
    .bind("system")
    .bind("bootstrap default policy")
    .bind(serde_json::to_string(&snapshot).context("failed to serialize default policy snapshot")?)
    .bind(u128_to_i64(now)?)
    .execute(pool)
    .await
    .context("failed to insert default policy audit event")?;

    Ok(())
}

async fn save_policy_profile(
    pool: &SqlitePool,
    profile: &PolicyProfileRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO policy_profiles (
            policy_id,
            version,
            issuer_did,
            allow_shell_exec,
            allowed_model_providers,
            allowed_chat_platforms,
            max_payment_amount,
            signature_hex,
            document_hash,
            issued_at_unix_ms,
            updated_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(policy_id) DO UPDATE SET
            version = excluded.version,
            issuer_did = excluded.issuer_did,
            allow_shell_exec = excluded.allow_shell_exec,
            allowed_model_providers = excluded.allowed_model_providers,
            allowed_chat_platforms = excluded.allowed_chat_platforms,
            max_payment_amount = excluded.max_payment_amount,
            signature_hex = excluded.signature_hex,
            document_hash = excluded.document_hash,
            issued_at_unix_ms = excluded.issued_at_unix_ms,
            updated_reason = excluded.updated_reason,
            created_at_unix_ms = excluded.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&profile.policy_id)
    .bind(i64::from(profile.version))
    .bind(&profile.issuer_did)
    .bind(profile.allow_shell_exec)
    .bind(
        serde_json::to_string(&profile.allowed_model_providers)
            .context("failed to serialize allowed_model_providers")?,
    )
    .bind(
        serde_json::to_string(&profile.allowed_chat_platforms)
            .context("failed to serialize allowed_chat_platforms")?,
    )
    .bind(profile.max_payment_amount)
    .bind(&profile.signature_hex)
    .bind(&profile.document_hash)
    .bind(profile.issued_at_unix_ms.map(u128_to_i64).transpose()?)
    .bind(&profile.updated_reason)
    .bind(u128_to_i64(profile.created_at_unix_ms)?)
    .bind(u128_to_i64(profile.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save policy profile")?;

    Ok(())
}

async fn save_policy_trust_root(
    pool: &SqlitePool,
    trust_root: &PolicyTrustRootRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO policy_trust_roots (
            issuer_did,
            label,
            public_key_hex,
            updated_by,
            updated_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(issuer_did) DO UPDATE SET
            label = excluded.label,
            public_key_hex = excluded.public_key_hex,
            updated_by = excluded.updated_by,
            updated_reason = excluded.updated_reason,
            created_at_unix_ms = policy_trust_roots.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&trust_root.issuer_did)
    .bind(&trust_root.label)
    .bind(&trust_root.public_key_hex)
    .bind(&trust_root.updated_by)
    .bind(&trust_root.updated_reason)
    .bind(u128_to_i64(trust_root.created_at_unix_ms)?)
    .bind(u128_to_i64(trust_root.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .context("failed to save policy trust root")?;

    Ok(())
}

async fn save_skill_publisher_trust_root(
    pool: &SqlitePool,
    trust_root: &SkillPublisherTrustRootRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO skill_publisher_trust_roots (
            issuer_did,
            label,
            public_key_hex,
            updated_by,
            updated_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(issuer_did) DO UPDATE SET
            label = excluded.label,
            public_key_hex = excluded.public_key_hex,
            updated_by = excluded.updated_by,
            updated_reason = excluded.updated_reason,
            created_at_unix_ms = skill_publisher_trust_roots.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&trust_root.issuer_did)
    .bind(&trust_root.label)
    .bind(&trust_root.public_key_hex)
    .bind(&trust_root.updated_by)
    .bind(&trust_root.updated_reason)
    .bind(u128_to_i64(trust_root.created_at_unix_ms)?)
    .bind(u128_to_i64(trust_root.updated_at_unix_ms)?)
    .execute(pool)
    .await
    .with_context(|| {
        format!(
            "failed to save skill publisher trust root '{}'",
            trust_root.issuer_did
        )
    })?;

    Ok(())
}

async fn ensure_sqlite_column(
    pool: &SqlitePool,
    table_name: &str,
    column_name: &str,
    alter_statement: &str,
) -> anyhow::Result<()> {
    let pragma_statement = format!("PRAGMA table_info({table_name})");
    let rows = sqlx::query(&pragma_statement)
        .fetch_all(pool)
        .await
        .with_context(|| format!("failed to inspect table info for {table_name}"))?;
    let exists = rows.iter().any(|row| {
        row.try_get::<String, _>("name")
            .map(|value| value == column_name)
            .unwrap_or(false)
    });
    if exists {
        return Ok(());
    }

    sqlx::query(alter_statement)
        .execute(pool)
        .await
        .with_context(|| format!("failed to add column {column_name} to {table_name}"))?;
    Ok(())
}

fn ensure_sqlite_database_parent(database_url: &str) -> anyhow::Result<()> {
    let Some(path) = database_url.strip_prefix("sqlite://") else {
        return Ok(());
    };
    if path == ":memory:" || path.starts_with("?") {
        return Ok(());
    }

    let database_path = std::path::Path::new(path);
    if let Some(parent) = database_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create SQLite data directory for {database_url}")
            })?;
        }
    }
    Ok(())
}

fn hash_approval_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn parse_uuid(raw: &str, field: &str) -> anyhow::Result<Uuid> {
    Uuid::parse_str(raw).with_context(|| format!("invalid uuid in field {field}: {raw}"))
}

fn parse_uuid_opt(raw: Option<String>, field: &str) -> anyhow::Result<Option<Uuid>> {
    raw.map(|value| parse_uuid(&value, field)).transpose()
}

fn parse_json_field<T>(raw: &str, field: &str) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(raw).with_context(|| format!("invalid json in field {field}"))
}

fn u128_to_i64(value: u128) -> anyhow::Result<i64> {
    i64::try_from(value).context("timestamp overflow while writing to SQLite")
}

fn i64_to_u128(value: i64) -> anyhow::Result<u128> {
    u128::try_from(value).context("negative timestamp found in SQLite")
}

fn i64_to_u128_opt(value: Option<i64>) -> anyhow::Result<Option<u128>> {
    value.map(i64_to_u128).transpose()
}

pub fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::{
        AgentExperienceListFilter, AgentExperienceRecord, AppState, ApprovalRequestKind,
        ApprovalRequestRecord, ApprovalRequestStatus, SkillImplementationExecutionListFilter,
        SkillImplementationExecutionRecord, SkillImplementationPatchListFilter,
        SkillImplementationPatchRecord, SkillImplementationPlanListFilter,
        SkillImplementationPlanRecord, SkillImplementationRunListFilter,
        SkillImplementationRunRecord, SkillProposalListFilter, SkillProposalRecord, StoredTask,
        TaskStatus, unix_timestamp_ms,
    };
    use crate::sandbox;

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-app-state-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    #[tokio::test]
    async fn emits_console_events_for_task_and_approval_updates() {
        let (database_url, db_path) = temp_database_url();
        let engine = sandbox::init_engine().unwrap();
        let state = AppState::new_with_database_url(engine, &database_url)
            .await
            .unwrap();
        let mut receiver = state.subscribe_console_events();
        let now = unix_timestamp_ms();
        let task_id = Uuid::new_v4();

        state
            .insert_task(StoredTask {
                task_id,
                parent_task_id: None,
                name: "console event task".to_string(),
                instruction: "echo".to_string(),
                status: TaskStatus::Accepted,
                linked_payment_id: None,
                last_update_reason: "created".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();

        state
            .upsert_approval_request(ApprovalRequestRecord {
                approval_id: Uuid::new_v4(),
                kind: ApprovalRequestKind::NodeCommand,
                title: "Approve command".to_string(),
                summary: "Pending command approval".to_string(),
                task_id: Some(task_id),
                reference_id: Uuid::new_v4().to_string(),
                status: ApprovalRequestStatus::Pending,
                actor: None,
                decision_reason: None,
                decision_payload: None,
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();

        let first = receiver.recv().await.unwrap();
        let second = receiver.recv().await.unwrap();
        assert!(matches!(first.channel.as_str(), "task" | "approval"));
        assert!(matches!(second.channel.as_str(), "task" | "approval"));
        assert_ne!(first.channel, second.channel);

        drop(state);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn recent_console_events_returns_newest_events_first() {
        let (database_url, db_path) = temp_database_url();
        let engine = sandbox::init_engine().unwrap();
        let state = AppState::new_with_database_url(engine, &database_url)
            .await
            .unwrap();

        state.emit_console_event(
            "task",
            Some("task-1".to_string()),
            Some("accepted".to_string()),
            "first event",
        );
        state.emit_console_event(
            "approval",
            Some("approval-1".to_string()),
            Some("pending".to_string()),
            "second event",
        );
        state.emit_console_event(
            "task",
            Some("task-2".to_string()),
            Some("completed".to_string()),
            "third event",
        );

        let recent = state.recent_console_events(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].entity_id.as_deref(), Some("task-2"));
        assert_eq!(recent[0].detail, "third event");
        assert_eq!(recent[1].entity_id.as_deref(), Some("approval-1"));
        assert_eq!(recent[1].detail, "second event");

        drop(state);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn stores_and_filters_agent_experiences() {
        let (database_url, db_path) = temp_database_url();
        let engine = sandbox::init_engine().unwrap();
        let state = AppState::new_with_database_url(engine, &database_url)
            .await
            .unwrap();
        let now = unix_timestamp_ms();
        let experience_id = Uuid::new_v4();
        let ingress_id = Uuid::new_v4();

        state
            .upsert_agent_experience(AgentExperienceRecord {
                experience_id,
                source: "chat_ingress:telegram".to_string(),
                scope: "chat".to_string(),
                task_kind: "desktop_control".to_string(),
                input_summary: "用户要求打开微信开发者工具".to_string(),
                action_summary: "识别为桌面控制请求，等待审批后执行".to_string(),
                outcome: "success".to_string(),
                lesson: "类似请求应先确认目标窗口，再下发点击或快捷键".to_string(),
                reusable_hint: Some("先检查窗口标题和进程列表".to_string()),
                evidence: serde_json::json!({"verified": true}),
                tags: vec!["telegram".to_string(), "desktop".to_string()],
                risk_level: "guarded".to_string(),
                related_task_id: None,
                related_ingress_id: Some(ingress_id),
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();

        let stored = state
            .get_agent_experience(experience_id)
            .await
            .unwrap()
            .expect("experience should exist");
        assert_eq!(stored.task_kind, "desktop_control");
        assert_eq!(stored.tags, vec!["telegram", "desktop"]);

        let filtered = state
            .list_agent_experiences(AgentExperienceListFilter {
                query: Some("窗口标题".to_string()),
                outcome: Some("success".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(state.count_agent_experiences().await.unwrap(), 1);
        assert!(
            state
                .has_agent_experience_for_ingress(ingress_id)
                .await
                .unwrap()
        );
        assert!(
            !state
                .has_agent_experience_for_ingress(Uuid::new_v4())
                .await
                .unwrap()
        );

        drop(state);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn stores_and_filters_skill_proposals() {
        let (database_url, db_path) = temp_database_url();
        let engine = sandbox::init_engine().unwrap();
        let state = AppState::new_with_database_url(engine, &database_url)
            .await
            .unwrap();
        let now = unix_timestamp_ms();
        let proposal_id = Uuid::new_v4();

        state
            .upsert_skill_proposal(SkillProposalRecord {
                proposal_id,
                proposal_key: "task-kind:desktop_control".to_string(),
                title: "Create a desktop control helper skill".to_string(),
                summary: "Repeated desktop control requests should become a reviewed skill."
                    .to_string(),
                rationale: "Two or more guarded experiences mention desktop control.".to_string(),
                suggested_skill_id: "dawn.desktop-control-helper".to_string(),
                source: "experience-pattern".to_string(),
                status: "proposed".to_string(),
                confidence: 0.75,
                evidence: serde_json::json!({"experienceCount": 2}),
                tags: vec!["desktop".to_string(), "proposal".to_string()],
                risk_level: "guarded".to_string(),
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();

        let stored = state
            .get_skill_proposal(proposal_id)
            .await
            .unwrap()
            .expect("proposal should exist");
        assert_eq!(stored.suggested_skill_id, "dawn.desktop-control-helper");

        let by_key = state
            .get_skill_proposal_by_key("task-kind:desktop_control")
            .await
            .unwrap()
            .expect("proposal should exist by key");
        assert_eq!(by_key.proposal_id, proposal_id);

        let filtered = state
            .list_skill_proposals(SkillProposalListFilter {
                status: Some("proposed".to_string()),
                query: Some("desktop".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(state.count_skill_proposals().await.unwrap(), 1);

        let mut reviewed = stored;
        reviewed.status = "approved".to_string();
        reviewed.evidence = serde_json::json!({
            "experienceCount": 2,
            "reviewTrail": [
                {
                    "status": "approved",
                    "reviewer": "operator",
                    "activation": "not_activated"
                }
            ]
        });
        reviewed.updated_at_unix_ms = now + 1;
        state.update_skill_proposal_review(reviewed).await.unwrap();
        let reviewed = state
            .get_skill_proposal(proposal_id)
            .await
            .unwrap()
            .expect("reviewed proposal should exist");
        assert_eq!(reviewed.status, "approved");
        assert_eq!(
            reviewed.evidence["reviewTrail"][0]["activation"],
            "not_activated"
        );

        let plan_id = Uuid::new_v4();
        state
            .upsert_skill_implementation_plan(SkillImplementationPlanRecord {
                plan_id,
                proposal_id,
                suggested_skill_id: "dawn.desktop-control-helper".to_string(),
                title: "Draft implementation plan".to_string(),
                summary: "Plan only; no activation.".to_string(),
                status: "draft".to_string(),
                steps: serde_json::json!([
                    {
                        "order": 1,
                        "name": "Inspect existing desktop control skill"
                    }
                ]),
                acceptance_criteria: serde_json::json!([
                    "Existing chat ingress behavior is preserved."
                ]),
                guardrails: serde_json::json!({
                    "autonomousCodeMutation": false,
                    "requiresHumanReviewBeforeActivation": true
                }),
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();
        let stored_plan = state
            .get_skill_implementation_plan(plan_id)
            .await
            .unwrap()
            .expect("implementation plan should exist");
        assert_eq!(stored_plan.proposal_id, proposal_id);
        assert_eq!(stored_plan.status, "draft");
        assert_eq!(
            stored_plan.guardrails["requiresHumanReviewBeforeActivation"],
            true
        );
        let by_proposal = state
            .get_skill_implementation_plan_by_proposal(proposal_id)
            .await
            .unwrap()
            .expect("implementation plan should exist by proposal");
        assert_eq!(by_proposal.plan_id, plan_id);
        let filtered_plans = state
            .list_skill_implementation_plans(SkillImplementationPlanListFilter {
                status: Some("draft".to_string()),
                proposal_id: Some(proposal_id),
                query: Some("desktop".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered_plans.len(), 1);
        assert_eq!(state.count_skill_implementation_plans().await.unwrap(), 1);

        let mut reviewed_plan = stored_plan;
        reviewed_plan.status = "approved".to_string();
        reviewed_plan.guardrails = serde_json::json!({
            "autonomousCodeMutation": false,
            "requiresHumanReviewBeforeActivation": true,
            "reviewTrail": [
                {
                    "status": "approved",
                    "execution": "not_started",
                    "activation": "not_activated"
                }
            ]
        });
        reviewed_plan.updated_at_unix_ms = now + 2;
        state
            .update_skill_implementation_plan_review(reviewed_plan)
            .await
            .unwrap();
        let reviewed_plan = state
            .get_skill_implementation_plan(plan_id)
            .await
            .unwrap()
            .expect("reviewed implementation plan should exist");
        assert_eq!(reviewed_plan.status, "approved");
        assert_eq!(
            reviewed_plan.guardrails["reviewTrail"][0]["execution"],
            "not_started"
        );

        let run_id = Uuid::new_v4();
        state
            .upsert_skill_implementation_run(SkillImplementationRunRecord {
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id: "dawn.desktop-control-helper".to_string(),
                status: "prepared".to_string(),
                execution_mode: "guarded_manual_or_future_agent".to_string(),
                change_package: serde_json::json!({
                    "allowedTargetAreas": ["dawn_core/src"],
                    "workspacePolicy": "do not revert unrelated user changes"
                }),
                verification: serde_json::json!({
                    "requiredCommands": ["cargo check --manifest-path dawn_core/Cargo.toml"]
                }),
                rollback: serde_json::json!({
                    "manualRollbackOnly": true
                }),
                guardrails: serde_json::json!({
                    "autonomousCodeMutation": false,
                    "execution": "not_executed",
                    "activation": "not_activated"
                }),
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();
        let stored_run = state
            .get_skill_implementation_run(run_id)
            .await
            .unwrap()
            .expect("implementation run should exist");
        assert_eq!(stored_run.status, "prepared");
        assert_eq!(stored_run.plan_id, plan_id);
        let run_by_plan = state
            .get_skill_implementation_run_by_plan(plan_id)
            .await
            .unwrap()
            .expect("implementation run should exist by plan");
        assert_eq!(run_by_plan.run_id, run_id);
        let filtered_runs = state
            .list_skill_implementation_runs(SkillImplementationRunListFilter {
                status: Some("prepared".to_string()),
                plan_id: Some(plan_id),
                proposal_id: Some(proposal_id),
                query: Some("desktop".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered_runs.len(), 1);
        assert_eq!(state.count_skill_implementation_runs().await.unwrap(), 1);

        let mut reviewed_run = stored_run;
        reviewed_run.status = "approved_for_execution".to_string();
        reviewed_run.guardrails = serde_json::json!({
            "autonomousCodeMutation": false,
            "reviewTrail": [
                {
                    "status": "approved_for_execution",
                    "execution": "not_executed",
                    "activation": "not_activated"
                }
            ]
        });
        reviewed_run.updated_at_unix_ms = now + 3;
        state
            .update_skill_implementation_run_review(reviewed_run)
            .await
            .unwrap();
        let reviewed_run = state
            .get_skill_implementation_run(run_id)
            .await
            .unwrap()
            .expect("reviewed implementation run should exist");
        assert_eq!(reviewed_run.status, "approved_for_execution");
        assert_eq!(
            reviewed_run.guardrails["reviewTrail"][0]["execution"],
            "not_executed"
        );

        let execution_id = Uuid::new_v4();
        state
            .upsert_skill_implementation_execution(SkillImplementationExecutionRecord {
                execution_id,
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id: "dawn.desktop-control-helper".to_string(),
                status: "ready_for_execution".to_string(),
                executor: "operator_or_guarded_agent".to_string(),
                preflight_report: serde_json::json!({
                    "status": "passed",
                    "apiExecutedCommands": false
                }),
                command_plan: serde_json::json!({
                    "requiredCommands": ["cargo check --manifest-path dawn_core/Cargo.toml"],
                    "executionPolicy": "record only"
                }),
                result: serde_json::json!({
                    "execution": "not_started",
                    "activation": "not_activated"
                }),
                guardrails: serde_json::json!({
                    "autonomousCodeMutation": false,
                    "apiExecutedCommands": false,
                    "activation": "not_activated"
                }),
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();
        let stored_execution = state
            .get_skill_implementation_execution(execution_id)
            .await
            .unwrap()
            .expect("implementation execution should exist");
        assert_eq!(stored_execution.status, "ready_for_execution");
        assert_eq!(stored_execution.run_id, run_id);
        let filtered_executions = state
            .list_skill_implementation_executions(SkillImplementationExecutionListFilter {
                status: Some("ready_for_execution".to_string()),
                run_id: Some(run_id),
                plan_id: Some(plan_id),
                proposal_id: Some(proposal_id),
                query: Some("guarded".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered_executions.len(), 1);
        assert_eq!(
            state.count_skill_implementation_executions().await.unwrap(),
            1
        );

        let mut reviewed_execution = stored_execution;
        reviewed_execution.status = "approved_for_manual_execution".to_string();
        reviewed_execution.guardrails = serde_json::json!({
            "autonomousCodeMutation": false,
            "reviewTrail": [
                {
                    "status": "approved_for_manual_execution",
                    "apiExecutedCommands": false,
                    "activation": "not_activated"
                }
            ]
        });
        reviewed_execution.result = serde_json::json!({
            "execution": "not_started",
            "activation": "not_activated",
            "apiExecutedCommands": false
        });
        reviewed_execution.updated_at_unix_ms = now + 4;
        state
            .update_skill_implementation_execution_review(reviewed_execution)
            .await
            .unwrap();
        let reviewed_execution = state
            .get_skill_implementation_execution(execution_id)
            .await
            .unwrap()
            .expect("reviewed implementation execution should exist");
        assert_eq!(reviewed_execution.status, "approved_for_manual_execution");
        assert_eq!(
            reviewed_execution.guardrails["reviewTrail"][0]["apiExecutedCommands"],
            false
        );

        let patch_id = Uuid::new_v4();
        state
            .upsert_skill_implementation_patch(SkillImplementationPatchRecord {
                patch_id,
                execution_id,
                run_id,
                plan_id,
                proposal_id,
                suggested_skill_id: "dawn.desktop-control-helper".to_string(),
                status: "draft".to_string(),
                patch_kind: "review_only_candidate".to_string(),
                summary: "Candidate patch package for a guarded desktop control helper."
                    .to_string(),
                changed_files: serde_json::json!(["dawn_core/src/evolution.rs"]),
                patch_manifest: serde_json::json!({
                    "apiAppliedPatch": false,
                    "patchContentRequiredBeforeApplyApproval": true
                }),
                rollback_plan: serde_json::json!({
                    "manualRollbackOnly": true,
                    "apiRollbackExecuted": false
                }),
                verification_evidence: serde_json::json!({
                    "sourceExecutionStatus": "verification_succeeded"
                }),
                guardrails: serde_json::json!({
                    "autonomousCodeMutation": false,
                    "apiAppliedPatch": false,
                    "activation": "not_activated"
                }),
                created_by: "test".to_string(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();
        let stored_patch = state
            .get_skill_implementation_patch(patch_id)
            .await
            .unwrap()
            .expect("implementation patch should exist");
        assert_eq!(stored_patch.status, "draft");
        assert_eq!(stored_patch.execution_id, execution_id);
        let filtered_patches = state
            .list_skill_implementation_patches(SkillImplementationPatchListFilter {
                status: Some("draft".to_string()),
                execution_id: Some(execution_id),
                run_id: Some(run_id),
                plan_id: Some(plan_id),
                proposal_id: Some(proposal_id),
                query: Some("desktop".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(filtered_patches.len(), 1);
        assert_eq!(state.count_skill_implementation_patches().await.unwrap(), 1);

        let mut reviewed_patch = stored_patch;
        reviewed_patch.status = "approved_for_apply".to_string();
        reviewed_patch.guardrails = serde_json::json!({
            "autonomousCodeMutation": false,
            "reviewTrail": [
                {
                    "status": "approved_for_apply",
                    "apiAppliedPatch": false,
                    "activation": "not_activated"
                }
            ]
        });
        reviewed_patch.updated_at_unix_ms = now + 5;
        state
            .update_skill_implementation_patch_review(reviewed_patch)
            .await
            .unwrap();
        let reviewed_patch = state
            .get_skill_implementation_patch(patch_id)
            .await
            .unwrap()
            .expect("reviewed implementation patch should exist");
        assert_eq!(reviewed_patch.status, "approved_for_apply");
        assert_eq!(
            reviewed_patch.guardrails["reviewTrail"][0]["apiAppliedPatch"],
            false
        );

        let mut runtime_patch = reviewed_patch;
        runtime_patch.status = "applied_pending_verification".to_string();
        runtime_patch.patch_manifest = serde_json::json!({
            "apiAppliedPatch": true,
            "lastApplyDryRun": false
        });
        runtime_patch.rollback_plan = serde_json::json!({
            "apiRollbackAvailable": true,
            "apiRollbackExecuted": false,
            "snapshots": [
                {
                    "path": "dawn_core/src/evolution.rs",
                    "existed": true,
                    "oldSha256": "old",
                    "oldContent": "previous",
                    "newSha256": "new"
                }
            ]
        });
        runtime_patch.verification_evidence = serde_json::json!({
            "runtimeTrail": [
                {
                    "operation": "apply",
                    "status": "applied_pending_verification"
                }
            ]
        });
        runtime_patch.guardrails = serde_json::json!({
            "autonomousCodeMutation": false,
            "apiAppliedPatch": true,
            "apiRollbackExecuted": false,
            "activation": "not_activated"
        });
        runtime_patch.updated_at_unix_ms = now + 6;
        state
            .update_skill_implementation_patch_runtime(runtime_patch)
            .await
            .unwrap();
        let runtime_patch = state
            .get_skill_implementation_patch(patch_id)
            .await
            .unwrap()
            .expect("runtime-updated implementation patch should exist");
        assert_eq!(runtime_patch.status, "applied_pending_verification");
        assert_eq!(runtime_patch.patch_manifest["apiAppliedPatch"], true);
        assert_eq!(runtime_patch.rollback_plan["apiRollbackAvailable"], true);
        assert_eq!(runtime_patch.guardrails["activation"], "not_activated");

        drop(state);
        let _ = fs::remove_file(db_path);
    }
}
