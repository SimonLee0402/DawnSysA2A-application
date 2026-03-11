use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    FromRow, Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tokio::sync::{RwLock, mpsc};
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
    Queued,
    Dispatched,
    Succeeded,
    Failed,
}

impl NodeCommandStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Dispatched => "dispatched",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
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

pub struct AppState {
    pub engine: Engine,
    pool: SqlitePool,
    node_sessions: RwLock<HashMap<String, NodeSessionSender>>,
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

        Ok(Arc::new(Self {
            engine,
            pool,
            node_sessions: RwLock::new(HashMap::new()),
        }))
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn insert_task(&self, task: StoredTask) -> anyhow::Result<StoredTask> {
        save_task(&self.pool, &task).await?;
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

        self.get_task(task_id).await
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
        Ok(Some(command))
    }

    pub async fn upsert_orchestration_run(
        &self,
        run: OrchestrationRunRecord,
    ) -> anyhow::Result<OrchestrationRunRecord> {
        save_orchestration_run(&self.pool, &run).await?;
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
        .with_context(|| {
            format!("failed to fetch skill publisher trust root '{issuer_did}'")
        })?;

        row.map(TryInto::try_into).transpose()
    }

    pub async fn save_skill_publisher_trust_root(
        &self,
        trust_root: &SkillPublisherTrustRootRecord,
    ) -> anyhow::Result<SkillPublisherTrustRootRecord> {
        save_skill_publisher_trust_root(&self.pool, trust_root).await?;
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
        CREATE TABLE IF NOT EXISTS agent_quote_ledger (
            quote_id TEXT PRIMARY KEY,
            card_id TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            quote_url TEXT,
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
