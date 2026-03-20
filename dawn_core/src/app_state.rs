use std::{
    collections::HashMap,
    sync::Arc,
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

    pub fn emit_console_event(
        &self,
        channel: impl Into<String>,
        entity_id: Option<String>,
        status: Option<String>,
        detail: impl Into<String>,
    ) {
        let _ = self.console_events.send(ConsoleStreamEvent {
            channel: channel.into(),
            entity_id,
            status,
            detail: detail.into(),
            created_at_unix_ms: unix_timestamp_ms(),
        });
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
        .with_context(|| {
            format!("failed to fetch chat identity for {platform}:{identity_key}")
        })?;

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
        CREATE INDEX IF NOT EXISTS idx_chat_ingress_events_linked_task_id
        ON chat_ingress_events(linked_task_id, created_at_unix_ms DESC)
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
        AppState, ApprovalRequestKind, ApprovalRequestRecord, ApprovalRequestStatus, StoredTask,
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
}
