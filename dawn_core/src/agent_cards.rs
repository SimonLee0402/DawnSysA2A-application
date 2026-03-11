use std::{collections::HashSet, sync::Arc};

use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::time::{Duration, Instant, sleep};
use uuid::Uuid;

use crate::{
    ap2::{self, PaymentRequest},
    app_state::{AppState, PaymentRecord, PaymentStatus, unix_timestamp_ms},
};

const AP2_EXTENSION_URI: &str = "https://github.com/google-agentic-commerce/ap2/tree/v0.1";
const QUOTE_ISSUER_DID_PREFIX: &str = "did:dawn:quote:";
const DEFAULT_QUOTE_TTL_SECONDS: u64 = 300;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    pub organization: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthentication {
    #[serde(default)]
    pub schemes: Vec<String>,
    pub credentials: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentExtension {
    pub uri: String,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub streaming: Option<bool>,
    pub push_notifications: Option<bool>,
    pub state_transition_history: Option<bool>,
    #[serde(default)]
    pub extensions: Vec<AgentExtension>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub input_modes: Vec<String>,
    #[serde(default)]
    pub output_modes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    pub provider: Option<AgentProvider>,
    pub version: String,
    pub documentation_url: Option<String>,
    #[serde(default)]
    pub capabilities: AgentCapabilities,
    #[serde(default)]
    pub authentication: AgentAuthentication,
    #[serde(default)]
    pub default_input_modes: Vec<String>,
    #[serde(default)]
    pub default_output_modes: Vec<String>,
    #[serde(default)]
    pub skills: Vec<AgentSkill>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PublishedAgentCard {
    pub card_id: String,
    pub source_kind: String,
    pub card_url: Option<String>,
    pub published: bool,
    pub locally_hosted: bool,
    pub issuer_did: Option<String>,
    pub signature_hex: Option<String>,
    pub regions: Vec<String>,
    pub languages: Vec<String>,
    pub model_providers: Vec<String>,
    pub chat_platforms: Vec<String>,
    pub payment_roles: Vec<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
    pub card: AgentCard,
}

#[derive(Debug, FromRow)]
struct AgentCardRow {
    card_id: String,
    card_json: String,
    source_kind: String,
    card_url: Option<String>,
    published: i64,
    locally_hosted: i64,
    issuer_did: Option<String>,
    signature_hex: Option<String>,
    regions: String,
    languages: String,
    model_providers: String,
    chat_platforms: String,
    payment_roles: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteInvocationStatus {
    Dispatched,
    Running,
    Completed,
    Failed,
}

impl RemoteInvocationStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Dispatched => "dispatched",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "dispatched" => Ok(Self::Dispatched),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("unknown remote invocation status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentInvocationRecord {
    pub invocation_id: Uuid,
    pub card_id: String,
    pub remote_agent_url: String,
    pub local_task_id: Option<Uuid>,
    pub remote_task_id: Option<String>,
    pub request: Value,
    pub response: Option<Value>,
    pub status: RemoteInvocationStatus,
    pub error: Option<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, FromRow)]
struct RemoteAgentInvocationRow {
    invocation_id: String,
    card_id: String,
    remote_agent_url: String,
    local_task_id: Option<String>,
    remote_task_id: Option<String>,
    request_json: String,
    response_json: Option<String>,
    status: String,
    error: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSettlementRequest {
    pub mandate_id: Uuid,
    pub amount: f64,
    pub description: String,
    pub quote_id: Option<String>,
    pub counter_offer_amount: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentSettlementRecord {
    pub settlement_id: Uuid,
    pub invocation_id: Uuid,
    pub card_id: String,
    pub remote_agent_url: String,
    pub local_task_id: Option<Uuid>,
    pub remote_task_id: Option<String>,
    pub transaction_id: Uuid,
    pub mandate_id: Uuid,
    pub quote_id: Option<String>,
    pub amount: f64,
    pub description: String,
    pub status: PaymentStatus,
    pub verification_message: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, FromRow)]
struct RemoteAgentSettlementRow {
    settlement_id: String,
    invocation_id: String,
    card_id: String,
    remote_agent_url: String,
    local_task_id: Option<String>,
    remote_task_id: Option<String>,
    transaction_id: String,
    mandate_id: String,
    quote_id: Option<String>,
    amount: f64,
    description: String,
    status: String,
    verification_message: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardRegistryStatus {
    total_cards: usize,
    published_cards: usize,
    local_cards: usize,
    remote_cards: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishAgentCardRequest {
    pub card_id: Option<String>,
    pub card: AgentCard,
    pub regions: Option<Vec<String>>,
    pub languages: Option<Vec<String>>,
    pub model_providers: Option<Vec<String>>,
    pub chat_platforms: Option<Vec<String>>,
    pub payment_roles: Option<Vec<String>>,
    pub locally_hosted: Option<bool>,
    pub published: Option<bool>,
    pub issuer_did: Option<String>,
    pub signature_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAgentCardRequest {
    pub card_id: Option<String>,
    pub card_url: String,
    pub regions: Option<Vec<String>>,
    pub languages: Option<Vec<String>>,
    pub model_providers: Option<Vec<String>>,
    pub chat_platforms: Option<Vec<String>>,
    pub payment_roles: Option<Vec<String>>,
    pub published: Option<bool>,
    pub issuer_did: Option<String>,
    pub signature_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchAgentCardsRequest {
    q: Option<String>,
    skill_id: Option<String>,
    skill_tag: Option<String>,
    region: Option<String>,
    language: Option<String>,
    model_provider: Option<String>,
    chat_platform: Option<String>,
    payment_role: Option<String>,
    streaming: Option<bool>,
    push_notifications: Option<bool>,
    published_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementQuoteQuery {
    requested_amount: Option<f64>,
    description: Option<String>,
    remote: Option<bool>,
    timeout_seconds: Option<u64>,
    allow_metadata_fallback: Option<bool>,
    quote_id: Option<String>,
    counter_offer_amount: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvocationListQuery {
    card_id: Option<String>,
    local_task_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementListQuery {
    card_id: Option<String>,
    invocation_id: Option<Uuid>,
    local_task_id: Option<Uuid>,
    transaction_id: Option<Uuid>,
    status: Option<PaymentStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteListQuery {
    card_id: Option<String>,
    status: Option<QuoteLedgerStatus>,
    source_kind: Option<String>,
    transaction_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevokeQuoteRequest {
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardPublishResponse {
    record: PublishedAgentCard,
    well_known_card_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvokeAgentCardRequest {
    pub name: String,
    pub instruction: String,
    pub parent_task_id: Option<Uuid>,
    pub await_completion: Option<bool>,
    pub timeout_seconds: Option<u64>,
    pub poll_interval_ms: Option<u64>,
    pub settlement: Option<RemoteSettlementRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvokeAgentCardResponse {
    pub invocation: RemoteAgentInvocationRecord,
    pub remote_status: Option<String>,
    pub settlement: Option<RemoteAgentSettlementRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettlementQuote {
    pub card_id: String,
    pub settlement_supported: bool,
    pub payment_roles: Vec<String>,
    pub currency: Option<String>,
    pub quote_mode: String,
    pub quote_source: String,
    pub quote_url: Option<String>,
    pub quote_id: Option<String>,
    pub previous_quote_id: Option<String>,
    pub counter_offer_amount: Option<f64>,
    pub requested_amount: Option<f64>,
    pub quoted_amount: Option<f64>,
    pub min_amount: Option<f64>,
    pub max_amount: Option<f64>,
    pub description_template: Option<String>,
    pub warning: Option<String>,
    pub expires_at_unix_ms: Option<u128>,
    pub issuer_did: Option<String>,
    pub signature_hex: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignedSettlementQuoteDocument {
    card_id: String,
    settlement_supported: bool,
    payment_roles: Vec<String>,
    currency: Option<String>,
    quote_mode: String,
    quote_source: String,
    quote_url: Option<String>,
    quote_id: String,
    previous_quote_id: Option<String>,
    counter_offer_amount: Option<f64>,
    requested_amount: Option<f64>,
    quoted_amount: Option<f64>,
    min_amount: Option<f64>,
    max_amount: Option<f64>,
    description_template: Option<String>,
    warning: Option<String>,
    expires_at_unix_ms: u128,
    issuer_did: String,
}

#[derive(Debug, Clone, Default)]
struct AgentPaymentTerms {
    roles: Vec<String>,
    currency: Option<String>,
    quote_mode: Option<String>,
    quote_method: Option<String>,
    quote_url: Option<String>,
    quote_path: Option<String>,
    quote_state_url_template: Option<String>,
    quote_issuer_did: Option<String>,
    flat_amount: Option<f64>,
    min_amount: Option<f64>,
    max_amount: Option<f64>,
    description_template: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct QuoteStateSnapshot {
    quote_id: String,
    card_id: String,
    status: QuoteLedgerStatus,
    previous_quote_id: Option<String>,
    superseded_by_quote_id: Option<String>,
    negotiation_round: u32,
    consumed_by_transaction_id: Option<Uuid>,
    revoked_reason: Option<String>,
    expires_at_unix_ms: Option<u128>,
    updated_at_unix_ms: u128,
    issuer_did: String,
    signature_hex: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignedQuoteStateDocument {
    quote_id: String,
    card_id: String,
    status: QuoteLedgerStatus,
    previous_quote_id: Option<String>,
    superseded_by_quote_id: Option<String>,
    negotiation_round: u32,
    consumed_by_transaction_id: Option<Uuid>,
    revoked_reason: Option<String>,
    expires_at_unix_ms: Option<u128>,
    updated_at_unix_ms: u128,
    issuer_did: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum QuoteLedgerStatus {
    Offered,
    Superseded,
    Revoked,
    Consumed,
}

impl QuoteLedgerStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::Superseded => "superseded",
            Self::Revoked => "revoked",
            Self::Consumed => "consumed",
        }
    }

    fn from_db(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "offered" => Ok(Self::Offered),
            "superseded" => Ok(Self::Superseded),
            "revoked" => Ok(Self::Revoked),
            "consumed" => Ok(Self::Consumed),
            _ => Err(anyhow!("unknown quote ledger status '{raw}'")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct QuoteLedgerRecord {
    quote_id: String,
    card_id: String,
    source_kind: String,
    quote_url: Option<String>,
    previous_quote_id: Option<String>,
    superseded_by_quote_id: Option<String>,
    negotiation_round: u32,
    settlement_supported: bool,
    payment_roles: Vec<String>,
    currency: Option<String>,
    quote_mode: String,
    requested_amount: Option<f64>,
    quoted_amount: Option<f64>,
    counter_offer_amount: Option<f64>,
    min_amount: Option<f64>,
    max_amount: Option<f64>,
    description_template: Option<String>,
    warning: Option<String>,
    expires_at_unix_ms: Option<u128>,
    issuer_did: Option<String>,
    signature_hex: Option<String>,
    status: QuoteLedgerStatus,
    consumed_by_transaction_id: Option<Uuid>,
    revoked_reason: Option<String>,
    created_at_unix_ms: u128,
    updated_at_unix_ms: u128,
}

#[derive(Debug, FromRow)]
struct QuoteLedgerRow {
    quote_id: String,
    card_id: String,
    source_kind: String,
    quote_url: Option<String>,
    previous_quote_id: Option<String>,
    superseded_by_quote_id: Option<String>,
    negotiation_round: i64,
    settlement_supported: i64,
    payment_roles: String,
    currency: Option<String>,
    quote_mode: String,
    requested_amount: Option<f64>,
    quoted_amount: Option<f64>,
    counter_offer_amount: Option<f64>,
    min_amount: Option<f64>,
    max_amount: Option<f64>,
    description_template: Option<String>,
    warning: Option<String>,
    expires_at_unix_ms: Option<i64>,
    issuer_did: Option<String>,
    signature_hex: Option<String>,
    status: String,
    consumed_by_transaction_id: Option<String>,
    revoked_reason: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/", get(list_cards))
        .route("/search", get(search_cards))
        .route("/:card_id/quote", get(get_settlement_quote))
        .route("/quotes", get(list_quotes))
        .route("/quotes/:quote_id/state", get(get_quote_state))
        .route("/quotes/:quote_id", get(get_quote))
        .route("/quotes/:quote_id/revoke", post(revoke_quote))
        .route("/invocations", get(list_invocations))
        .route("/settlements", get(list_settlements))
        .route("/invocations/:invocation_id", get(get_invocation))
        .route(
            "/invocations/:invocation_id/settlement",
            get(get_invocation_settlement),
        )
        .route("/settlements/:settlement_id", get(get_settlement))
        .route("/publish", post(publish_card))
        .route("/import", post(import_card))
        .route("/:card_id/quotes/:quote_id/sync", post(sync_quote))
        .route("/:card_id", get(get_card))
        .route("/:card_id/invoke", post(invoke_card))
}

pub async fn well_known_agent_card_json(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AgentCard>, (StatusCode, Json<Value>)> {
    let card = load_active_local_card(&state)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("no locally hosted published agent card"))?;
    Ok(Json(card.card))
}

async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AgentCardRegistryStatus>, (StatusCode, Json<Value>)> {
    let records = list_card_records(&state).await.map_err(internal_error)?;
    let published_cards = records.iter().filter(|card| card.published).count();
    let local_cards = records.iter().filter(|card| card.locally_hosted).count();
    Ok(Json(AgentCardRegistryStatus {
        total_cards: records.len(),
        published_cards,
        local_cards,
        remote_cards: records.len().saturating_sub(local_cards),
    }))
}

async fn list_cards(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PublishedAgentCard>>, (StatusCode, Json<Value>)> {
    list_card_records(&state)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn search_cards(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchAgentCardsRequest>,
) -> Result<Json<Vec<PublishedAgentCard>>, (StatusCode, Json<Value>)> {
    let records = list_card_records(&state).await.map_err(internal_error)?;
    let published_only = query.published_only.unwrap_or(true);
    let filtered = records
        .into_iter()
        .filter(|card| matches_search(card, &query, published_only))
        .collect();
    Ok(Json(filtered))
}

async fn get_card(
    State(state): State<Arc<AppState>>,
    AxumPath(card_id): AxumPath<String>,
) -> Result<Json<PublishedAgentCard>, (StatusCode, Json<Value>)> {
    find_card_record(&state, &card_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("agent card not found"))
}

async fn get_settlement_quote(
    State(state): State<Arc<AppState>>,
    AxumPath(card_id): AxumPath<String>,
    Query(query): Query<SettlementQuoteQuery>,
) -> Result<Json<AgentSettlementQuote>, (StatusCode, Json<Value>)> {
    let card = find_card_record(&state, &card_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("agent card not found"))?;
    let base_quote = if query.remote.unwrap_or(false) {
        fetch_remote_settlement_quote(
            &card,
            query.requested_amount,
            query.description.as_deref(),
            query.timeout_seconds.unwrap_or(10),
            query.allow_metadata_fallback.unwrap_or(true),
            query.quote_id.as_deref(),
            query.counter_offer_amount,
        )
        .await
        .map_err(service_error)?
    } else {
        build_settlement_quote(&card, query.requested_amount, query.description.as_deref())
    };
    let quote = if query.remote.unwrap_or(false) {
        base_quote
    } else {
        apply_counter_offer_to_quote(
            base_quote,
            query.counter_offer_amount,
            query.quote_id.as_deref(),
        )
    };
    let quote = if card.locally_hosted && !query.remote.unwrap_or(false) {
        sign_local_settlement_quote(&card, quote).map_err(service_error)?
    } else {
        quote
    };
    if quote.quote_id.is_some() {
        let source_kind = if card.locally_hosted && !query.remote.unwrap_or(false) {
            "local"
        } else if query.remote.unwrap_or(false) {
            "remote"
        } else {
            "metadata"
        };
        record_quote_offer(&state, &card, &quote, source_kind)
            .await
            .map_err(service_error)?;
    }
    Ok(Json(quote))
}

async fn publish_card(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PublishAgentCardRequest>,
) -> Result<Json<AgentCardPublishResponse>, (StatusCode, Json<Value>)> {
    publish_card_inner(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

pub async fn publish_agent_card(
    state: &AppState,
    request: PublishAgentCardRequest,
) -> anyhow::Result<PublishedAgentCard> {
    Ok(publish_card_inner(state, request).await?.record)
}

async fn import_card(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ImportAgentCardRequest>,
) -> Result<Json<AgentCardPublishResponse>, (StatusCode, Json<Value>)> {
    import_card_inner(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

pub async fn import_agent_card(
    state: &AppState,
    request: ImportAgentCardRequest,
) -> anyhow::Result<PublishedAgentCard> {
    Ok(import_card_inner(state, request).await?.record)
}

async fn list_invocations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<InvocationListQuery>,
) -> Result<Json<Vec<RemoteAgentInvocationRecord>>, (StatusCode, Json<Value>)> {
    list_remote_invocations(&state, query.card_id.as_deref(), query.local_task_id)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_invocation(
    State(state): State<Arc<AppState>>,
    AxumPath(invocation_id): AxumPath<Uuid>,
) -> Result<Json<RemoteAgentInvocationRecord>, (StatusCode, Json<Value>)> {
    get_remote_invocation(&state, invocation_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("remote invocation not found"))
}

async fn list_settlements(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SettlementListQuery>,
) -> Result<Json<Vec<RemoteAgentSettlementRecord>>, (StatusCode, Json<Value>)> {
    list_remote_settlements(
        &state,
        query.card_id.as_deref(),
        query.invocation_id,
        query.local_task_id,
        query.transaction_id,
        query.status,
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn list_quotes(
    State(state): State<Arc<AppState>>,
    Query(query): Query<QuoteListQuery>,
) -> Result<Json<Vec<QuoteLedgerRecord>>, (StatusCode, Json<Value>)> {
    list_quote_records(
        &state,
        query.card_id.as_deref(),
        query.status,
        query.source_kind.as_deref(),
        query.transaction_id,
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn get_quote(
    State(state): State<Arc<AppState>>,
    AxumPath(quote_id): AxumPath<String>,
) -> Result<Json<QuoteLedgerRecord>, (StatusCode, Json<Value>)> {
    get_quote_record(&state, &quote_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("quote not found"))
}

async fn get_quote_state(
    State(state): State<Arc<AppState>>,
    AxumPath(quote_id): AxumPath<String>,
) -> Result<Json<QuoteStateSnapshot>, (StatusCode, Json<Value>)> {
    let record = get_quote_record(&state, &quote_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("quote not found"))?;
    if record.source_kind != "local" {
        return Err(not_found(
            "quote state is only published for locally issued quotes",
        ));
    }
    sign_quote_state(&record)
        .map(Json)
        .map_err(service_error)
}

async fn revoke_quote(
    State(state): State<Arc<AppState>>,
    AxumPath(quote_id): AxumPath<String>,
    Json(request): Json<RevokeQuoteRequest>,
) -> Result<Json<QuoteLedgerRecord>, (StatusCode, Json<Value>)> {
    revoke_quote_record(&state, &quote_id, request.reason.as_deref())
        .await
        .map(Json)
        .map_err(service_error)
}

async fn sync_quote(
    State(state): State<Arc<AppState>>,
    AxumPath((card_id, quote_id)): AxumPath<(String, String)>,
) -> Result<Json<QuoteLedgerRecord>, (StatusCode, Json<Value>)> {
    let card = find_card_record(&state, &card_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("agent card not found"))?;
    sync_remote_quote_state(&state, &card, &quote_id, 10)
        .await
        .map(Json)
        .map_err(service_error)
}

async fn get_settlement(
    State(state): State<Arc<AppState>>,
    AxumPath(settlement_id): AxumPath<Uuid>,
) -> Result<Json<RemoteAgentSettlementRecord>, (StatusCode, Json<Value>)> {
    get_remote_settlement(&state, settlement_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("remote settlement not found"))
}

async fn get_invocation_settlement(
    State(state): State<Arc<AppState>>,
    AxumPath(invocation_id): AxumPath<Uuid>,
) -> Result<Json<RemoteAgentSettlementRecord>, (StatusCode, Json<Value>)> {
    get_remote_settlement_by_invocation(&state, invocation_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("remote settlement not found for invocation"))
}

async fn invoke_card(
    State(state): State<Arc<AppState>>,
    AxumPath(card_id): AxumPath<String>,
    Json(request): Json<InvokeAgentCardRequest>,
) -> Result<Json<InvokeAgentCardResponse>, (StatusCode, Json<Value>)> {
    invoke_remote_agent_card(&state, &card_id, request, None)
        .await
        .map(Json)
        .map_err(service_error)
}

async fn publish_card_inner(
    state: &AppState,
    request: PublishAgentCardRequest,
) -> anyhow::Result<AgentCardPublishResponse> {
    let card_id = request
        .card_id
        .unwrap_or_else(|| derive_card_id(&request.card.name, &request.card.url));
    validate_card_id(&card_id)?;
    validate_card(&request.card)?;
    let locally_hosted = request.locally_hosted.unwrap_or(false);
    let published = request.published.unwrap_or(true);
    let enriched_card = enrich_local_card_with_quote_url(&card_id, locally_hosted, request.card);
    let merged_payment_roles =
        merge_unique_metadata(request.payment_roles, extract_ap2_roles(&enriched_card));
    let now = unix_timestamp_ms();
    let record = PublishedAgentCard {
        card_id: card_id.clone(),
        source_kind: if locally_hosted {
            "local".to_string()
        } else {
            "published".to_string()
        },
        card_url: if locally_hosted {
            Some("/.well-known/agent-card.json".to_string())
        } else {
            None
        },
        published,
        locally_hosted,
        issuer_did: request.issuer_did,
        signature_hex: request.signature_hex,
        regions: normalize_metadata(request.regions.unwrap_or_default()),
        languages: normalize_metadata(request.languages.unwrap_or_default()),
        model_providers: normalize_metadata(request.model_providers.unwrap_or_default()),
        chat_platforms: normalize_metadata(request.chat_platforms.unwrap_or_default()),
        payment_roles: normalize_metadata(merged_payment_roles),
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
        card: enriched_card,
    };
    save_card_record(state, &record).await?;
    let saved = find_card_record(state, &card_id)
        .await?
        .ok_or_else(|| anyhow!("agent card disappeared after publish"))?;
    Ok(AgentCardPublishResponse {
        well_known_card_url: if saved.locally_hosted && saved.published {
            Some("/.well-known/agent-card.json".to_string())
        } else {
            None
        },
        record: saved,
    })
}

async fn import_card_inner(
    state: &AppState,
    request: ImportAgentCardRequest,
) -> anyhow::Result<AgentCardPublishResponse> {
    let (resolved_card_url, card) = fetch_remote_agent_card(&request.card_url).await?;
    let publish_request = PublishAgentCardRequest {
        card_id: request
            .card_id
            .or_else(|| Some(derive_card_id(&card.name, &card.url))),
        card,
        regions: request.regions,
        languages: request.languages,
        model_providers: request.model_providers,
        chat_platforms: request.chat_platforms,
        payment_roles: request.payment_roles,
        locally_hosted: Some(false),
        published: request.published,
        issuer_did: request.issuer_did,
        signature_hex: request.signature_hex,
    };
    let mut response = publish_card_inner(state, publish_request).await?;
    response.record.source_kind = "imported".to_string();
    response.record.card_url = Some(resolved_card_url.clone());
    persist_card_source_metadata(
        state,
        &response.record.card_id,
        "imported",
        Some(resolved_card_url),
    )
    .await?;
    let saved = find_card_record(state, &response.record.card_id)
        .await?
        .ok_or_else(|| anyhow!("imported agent card disappeared after persistence"))?;
    Ok(AgentCardPublishResponse {
        well_known_card_url: None,
        record: saved,
    })
}

pub async fn invoke_remote_agent_card(
    state: &AppState,
    card_id: &str,
    request: InvokeAgentCardRequest,
    local_task_id: Option<Uuid>,
) -> anyhow::Result<InvokeAgentCardResponse> {
    let card = find_card_record(state, card_id)
        .await?
        .ok_or_else(|| anyhow!("agent card not found: {card_id}"))?;
    let await_completion = request.await_completion.unwrap_or(true);
    if request.settlement.is_some() && !await_completion {
        anyhow::bail!("remote settlement requires awaitCompletion = true");
    }
    let invocation_id = Uuid::new_v4();
    let remote_task_request = json!({
        "name": request.name,
        "parentTaskId": request.parent_task_id,
        "instruction": request.instruction
    });
    let create_url = remote_task_create_url(&card.card.url);
    let client = Client::new();
    let response = client
        .post(&create_url)
        .json(&remote_task_request)
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to invoke remote agent card {} at {}",
                card.card_id, create_url
            )
        })?;
    let status = response.status();
    let raw_body = response.text().await?;
    let raw_response = if raw_body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(&raw_body).with_context(|| {
            format!("remote agent invocation at {create_url} returned non-JSON body: {raw_body}")
        })?
    };

    if !status.is_success() {
        let record = RemoteAgentInvocationRecord {
            invocation_id,
            card_id: card.card_id.clone(),
            remote_agent_url: card.card.url.clone(),
            local_task_id,
            remote_task_id: None,
            request: remote_task_request.clone(),
            response: Some(raw_response.clone()),
            status: RemoteInvocationStatus::Failed,
            error: Some(format!("remote invoke failed with status {status}")),
            created_at_unix_ms: unix_timestamp_ms(),
            updated_at_unix_ms: unix_timestamp_ms(),
        };
        save_remote_invocation(state, &record).await?;
        anyhow::bail!("remote agent invocation failed with status {status}: {raw_response}");
    }

    let remote_task_id = extract_remote_task_id(&raw_response);
    let remote_status = extract_remote_task_status(&raw_response);
    let mut record = RemoteAgentInvocationRecord {
        invocation_id,
        card_id: card.card_id.clone(),
        remote_agent_url: card.card.url.clone(),
        local_task_id,
        remote_task_id: remote_task_id.clone(),
        request: remote_task_request,
        response: Some(raw_response.clone()),
        status: if is_remote_terminal_success(&raw_response) {
            RemoteInvocationStatus::Completed
        } else {
            RemoteInvocationStatus::Dispatched
        },
        error: None,
        created_at_unix_ms: unix_timestamp_ms(),
        updated_at_unix_ms: unix_timestamp_ms(),
    };
    save_remote_invocation(state, &record).await?;

    if await_completion && record.status == RemoteInvocationStatus::Dispatched {
        if let Some(remote_task_id) = remote_task_id {
            record = poll_remote_agent_task(
                state,
                record,
                &card.card.url,
                &remote_task_id,
                request.timeout_seconds.unwrap_or(30),
                request.poll_interval_ms.unwrap_or(1000),
            )
            .await?;
        }
    }

    let remote_status = record
        .response
        .as_ref()
        .and_then(extract_remote_task_status)
        .or(remote_status);
        let settlement = match request.settlement {
        Some(settlement) => {
            let validated_quote = validate_remote_settlement_request(state, &card, &settlement).await?;
            if record.status != RemoteInvocationStatus::Completed {
                anyhow::bail!(
                    "remote settlement requires a completed remote invocation; current status is {:?}",
                    record.status
                );
            }
            Some(
                create_remote_settlement(
                    state,
                    &card,
                    &record,
                    settlement,
                    Some(&validated_quote),
                    local_task_id,
                )
                .await?,
            )
        }
        None => None,
    };

    Ok(InvokeAgentCardResponse {
        remote_status,
        invocation: record,
        settlement,
    })
}

async fn poll_remote_agent_task(
    state: &AppState,
    mut record: RemoteAgentInvocationRecord,
    remote_agent_url: &str,
    remote_task_id: &str,
    timeout_seconds: u64,
    poll_interval_ms: u64,
) -> anyhow::Result<RemoteAgentInvocationRecord> {
    let detail_url = remote_task_detail_url(remote_agent_url, remote_task_id);
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds.max(1));
    let poll_interval = Duration::from_millis(poll_interval_ms.max(100));
    let client = Client::new();

    loop {
        if Instant::now() > deadline {
            record.status = RemoteInvocationStatus::Failed;
            record.error = Some(format!(
                "remote task {} did not complete within {} seconds",
                remote_task_id, timeout_seconds
            ));
            record.updated_at_unix_ms = unix_timestamp_ms();
            save_remote_invocation(state, &record).await?;
            return Ok(record);
        }

        let response = client
            .get(&detail_url)
            .send()
            .await
            .with_context(|| format!("failed polling remote task {}", remote_task_id))?;
        let status = response.status();
        let raw_body = response.text().await?;
        let raw_response = if raw_body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str::<Value>(&raw_body).with_context(|| {
                format!("remote task poll at {detail_url} returned non-JSON body: {raw_body}")
            })?
        };

        if !status.is_success() {
            record.status = RemoteInvocationStatus::Failed;
            record.error = Some(format!("remote task poll failed with status {status}"));
            record.response = Some(raw_response);
            record.updated_at_unix_ms = unix_timestamp_ms();
            save_remote_invocation(state, &record).await?;
            return Ok(record);
        }

        let remote_status = extract_remote_task_status(&raw_response);
        record.response = Some(raw_response.clone());
        record.status = if is_remote_terminal_success(&raw_response) {
            RemoteInvocationStatus::Completed
        } else if is_remote_terminal_failure(&raw_response) {
            RemoteInvocationStatus::Failed
        } else {
            RemoteInvocationStatus::Running
        };
        record.updated_at_unix_ms = unix_timestamp_ms();
        save_remote_invocation(state, &record).await?;

        if matches!(
            remote_status.as_deref(),
            Some("completed" | "authorized" | "succeeded" | "success")
        ) || is_remote_terminal_failure(&raw_response)
        {
            if is_remote_terminal_failure(&raw_response) && record.error.is_none() {
                record.error = Some("remote task reported failure".to_string());
                save_remote_invocation(state, &record).await?;
            }
            return Ok(record);
        }

        sleep(poll_interval).await;
    }
}

async fn create_remote_settlement(
    state: &AppState,
    card: &PublishedAgentCard,
    invocation: &RemoteAgentInvocationRecord,
    settlement: RemoteSettlementRequest,
    validated_quote: Option<&AgentSettlementQuote>,
    local_task_id: Option<Uuid>,
) -> anyhow::Result<RemoteAgentSettlementRecord> {
    let payment_response = ap2::request_payment_authorization(
        state,
        PaymentRequest {
            transaction_id: None,
            task_id: local_task_id,
            mandate_id: settlement.mandate_id,
            amount: settlement.amount,
            description: settlement.description.clone(),
            mcu_public_did: None,
            mcu_signature: None,
        },
    )
    .await?;

    let now = unix_timestamp_ms();
    let record = RemoteAgentSettlementRecord {
        settlement_id: Uuid::new_v4(),
        invocation_id: invocation.invocation_id,
        card_id: card.card_id.clone(),
        remote_agent_url: card.card.url.clone(),
        local_task_id,
        remote_task_id: invocation.remote_task_id.clone(),
        transaction_id: payment_response.transaction_id,
        mandate_id: settlement.mandate_id,
        quote_id: validated_quote
            .and_then(|quote| quote.quote_id.clone())
            .or(settlement.quote_id.clone()),
        amount: settlement.amount,
        description: settlement.description,
        status: payment_response.status,
        verification_message: payment_response.verification_message,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    };
    save_remote_settlement(state, &record).await?;
    consume_quote_record(
        state,
        &card.card_id,
        record.quote_id.as_deref(),
        payment_response.transaction_id,
    )
    .await?;
    Ok(record)
}

async fn validate_remote_settlement_request(
    state: &AppState,
    card: &PublishedAgentCard,
    settlement: &RemoteSettlementRequest,
) -> anyhow::Result<AgentSettlementQuote> {
    let quote = fetch_remote_settlement_quote(
        card,
        Some(settlement.amount),
        Some(&settlement.description),
        10,
        true,
        settlement.quote_id.as_deref(),
        settlement.counter_offer_amount,
    )
    .await
    .with_context(|| {
        format!(
            "failed to validate negotiated settlement quote for agent card '{}'",
            card.card_id
        )
    })?;
    if quote.quote_id.is_some() {
        record_quote_offer(state, card, &quote, "remote").await?;
        if let Some(quote_id) = quote.quote_id.as_deref() {
            let synced = sync_remote_quote_state(state, card, quote_id, 10).await?;
            match synced.status {
                QuoteLedgerStatus::Offered => {}
                QuoteLedgerStatus::Superseded => anyhow::bail!(
                    "remote quote '{}' has been superseded by '{}'",
                    quote_id,
                    synced
                        .superseded_by_quote_id
                        .as_deref()
                        .unwrap_or("another quote")
                ),
                QuoteLedgerStatus::Revoked => anyhow::bail!(
                    "remote quote '{}' has been revoked{}",
                    quote_id,
                    synced
                        .revoked_reason
                        .as_deref()
                        .map(|reason| format!(": {reason}"))
                        .unwrap_or_default()
                ),
                QuoteLedgerStatus::Consumed => anyhow::bail!(
                    "remote quote '{}' has already been consumed{}",
                    quote_id,
                    synced
                        .consumed_by_transaction_id
                        .map(|value| format!(" by transaction {value}"))
                        .unwrap_or_default()
                ),
            }
        }
    }
    if !quote.settlement_supported {
        anyhow::bail!(
            "agent card '{}' does not advertise AP2 payee or merchant settlement capability",
            card.card_id
        );
    }
    if settlement.amount <= 0.0 {
        anyhow::bail!("remote settlement amount must be positive");
    }
    if let Some(min_amount) = quote.min_amount {
        if settlement.amount < min_amount {
            anyhow::bail!(
                "remote settlement amount {:.2} is below agent-card minAmount {:.2}",
                settlement.amount,
                min_amount
            );
        }
    }
    if let Some(max_amount) = quote.max_amount {
        if settlement.amount > max_amount {
            anyhow::bail!(
                "remote settlement amount {:.2} exceeds quoted maxAmount {:.2}",
                settlement.amount,
                max_amount
            );
        }
    }
    if matches!(quote.quote_mode.as_str(), "flat" | "fixed")
        && quote
            .quoted_amount
            .map(|quoted| (quoted - settlement.amount).abs() > f64::EPSILON)
            .unwrap_or(false)
    {
        anyhow::bail!(
            "remote settlement amount {:.2} must match the agent-card flat quote {:.2}",
            settlement.amount,
            quote.quoted_amount.unwrap_or(settlement.amount)
        );
    }
    if matches!(quote.quote_mode.as_str(), "negotiated" | "counter_offer")
        && quote
            .quoted_amount
            .map(|quoted| (quoted - settlement.amount).abs() > f64::EPSILON)
            .unwrap_or(false)
    {
        anyhow::bail!(
            "remote settlement amount {:.2} does not match negotiated quote {:.2}; supply the accepted counter-offer amount",
            settlement.amount,
            quote.quoted_amount.unwrap_or(settlement.amount)
        );
    }
    Ok(quote)
}

pub async fn sync_remote_settlement_from_payment(
    state: &AppState,
    payment: &PaymentRecord,
) -> anyhow::Result<Option<RemoteAgentSettlementRecord>> {
    let Some(mut settlement) =
        get_remote_settlement_by_transaction(state, payment.transaction_id).await?
    else {
        return Ok(None);
    };
    settlement.status = payment.status;
    settlement.verification_message = payment.verification_message.clone();
    settlement.updated_at_unix_ms = unix_timestamp_ms();
    save_remote_settlement(state, &settlement).await?;
    Ok(Some(settlement))
}

async fn fetch_remote_agent_card(raw_url: &str) -> anyhow::Result<(String, AgentCard)> {
    let client = Client::new();
    let mut last_error: Option<anyhow::Error> = None;
    for candidate in discovery_candidates(raw_url) {
        match client.get(&candidate).send().await {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await?;
                if !status.is_success() {
                    last_error = Some(anyhow!(
                        "agent card fetch from {candidate} failed with status {status}: {body}"
                    ));
                    continue;
                }
                match serde_json::from_str::<AgentCard>(&body) {
                    Ok(card) => return Ok((candidate, card)),
                    Err(error) => {
                        last_error = Some(anyhow!(
                            "agent card payload from {candidate} was invalid JSON for AgentCard: {error}"
                        ));
                    }
                }
            }
            Err(error) => {
                last_error = Some(anyhow!("agent card fetch from {candidate} failed: {error}"));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("failed to fetch agent card")))
}

fn discovery_candidates(raw_url: &str) -> Vec<String> {
    if raw_url.ends_with(".json") {
        return vec![raw_url.to_string()];
    }
    let trimmed = raw_url.trim_end_matches('/');
    vec![
        raw_url.to_string(),
        format!("{trimmed}/.well-known/agent-card.json"),
        format!("{trimmed}/.well-known/agent.json"),
    ]
}

async fn find_card_record(
    state: &AppState,
    card_id: &str,
) -> anyhow::Result<Option<PublishedAgentCard>> {
    let row = sqlx::query_as::<_, AgentCardRow>(
        r#"
        SELECT
            card_id,
            card_json,
            source_kind,
            card_url,
            published,
            locally_hosted,
            issuer_did,
            signature_hex,
            regions,
            languages,
            model_providers,
            chat_platforms,
            payment_roles,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM agent_cards
        WHERE card_id = ?1
        "#,
    )
    .bind(card_id)
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch agent card {card_id}"))?;

    row.map(row_to_card_record).transpose()
}

async fn load_active_local_card(state: &AppState) -> anyhow::Result<Option<PublishedAgentCard>> {
    let row = sqlx::query_as::<_, AgentCardRow>(
        r#"
        SELECT
            card_id,
            card_json,
            source_kind,
            card_url,
            published,
            locally_hosted,
            issuer_did,
            signature_hex,
            regions,
            languages,
            model_providers,
            chat_platforms,
            payment_roles,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM agent_cards
        WHERE published = 1 AND locally_hosted = 1
        ORDER BY updated_at_unix_ms DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(state.pool())
    .await
    .context("failed to fetch active local agent card")?;

    row.map(row_to_card_record).transpose()
}

async fn list_card_records(state: &AppState) -> anyhow::Result<Vec<PublishedAgentCard>> {
    let rows = sqlx::query_as::<_, AgentCardRow>(
        r#"
        SELECT
            card_id,
            card_json,
            source_kind,
            card_url,
            published,
            locally_hosted,
            issuer_did,
            signature_hex,
            regions,
            languages,
            model_providers,
            chat_platforms,
            payment_roles,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM agent_cards
        ORDER BY published DESC, locally_hosted DESC, updated_at_unix_ms DESC, card_id ASC
        "#,
    )
    .fetch_all(state.pool())
    .await
    .context("failed to list agent cards")?;

    rows.into_iter().map(row_to_card_record).collect()
}

pub async fn list_agent_cards(state: &AppState) -> anyhow::Result<Vec<PublishedAgentCard>> {
    list_card_records(state).await
}

async fn save_card_record(state: &AppState, record: &PublishedAgentCard) -> anyhow::Result<()> {
    if record.locally_hosted && record.published {
        sqlx::query(
            r#"
            UPDATE agent_cards
            SET published = 0
            WHERE locally_hosted = 1 AND card_id != ?1
            "#,
        )
        .bind(&record.card_id)
        .execute(state.pool())
        .await
        .context("failed to deactivate previous locally hosted agent cards")?;
    }

    sqlx::query(
        r#"
        INSERT INTO agent_cards (
            card_id,
            card_json,
            source_kind,
            card_url,
            published,
            locally_hosted,
            issuer_did,
            signature_hex,
            regions,
            languages,
            model_providers,
            chat_platforms,
            payment_roles,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(card_id) DO UPDATE SET
            card_json = excluded.card_json,
            source_kind = excluded.source_kind,
            card_url = excluded.card_url,
            published = excluded.published,
            locally_hosted = excluded.locally_hosted,
            issuer_did = excluded.issuer_did,
            signature_hex = excluded.signature_hex,
            regions = excluded.regions,
            languages = excluded.languages,
            model_providers = excluded.model_providers,
            chat_platforms = excluded.chat_platforms,
            payment_roles = excluded.payment_roles,
            created_at_unix_ms = agent_cards.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&record.card_id)
    .bind(serde_json::to_string(&record.card)?)
    .bind(&record.source_kind)
    .bind(&record.card_url)
    .bind(record.published)
    .bind(record.locally_hosted)
    .bind(&record.issuer_did)
    .bind(&record.signature_hex)
    .bind(serde_json::to_string(&record.regions)?)
    .bind(serde_json::to_string(&record.languages)?)
    .bind(serde_json::to_string(&record.model_providers)?)
    .bind(serde_json::to_string(&record.chat_platforms)?)
    .bind(serde_json::to_string(&record.payment_roles)?)
    .bind(record.created_at_unix_ms as i64)
    .bind(record.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to save agent card {}", record.card_id))?;

    Ok(())
}

async fn save_remote_invocation(
    state: &AppState,
    record: &RemoteAgentInvocationRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO remote_agent_invocations (
            invocation_id,
            card_id,
            remote_agent_url,
            local_task_id,
            remote_task_id,
            request_json,
            response_json,
            status,
            error,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(invocation_id) DO UPDATE SET
            card_id = excluded.card_id,
            remote_agent_url = excluded.remote_agent_url,
            local_task_id = excluded.local_task_id,
            remote_task_id = excluded.remote_task_id,
            request_json = excluded.request_json,
            response_json = excluded.response_json,
            status = excluded.status,
            error = excluded.error,
            created_at_unix_ms = remote_agent_invocations.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(record.invocation_id.to_string())
    .bind(&record.card_id)
    .bind(&record.remote_agent_url)
    .bind(record.local_task_id.map(|value| value.to_string()))
    .bind(&record.remote_task_id)
    .bind(serde_json::to_string(&record.request)?)
    .bind(
        record
            .response
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(record.status.as_db())
    .bind(&record.error)
    .bind(record.created_at_unix_ms as i64)
    .bind(record.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to save remote invocation {}", record.invocation_id))?;

    Ok(())
}

async fn save_remote_settlement(
    state: &AppState,
    record: &RemoteAgentSettlementRecord,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO remote_agent_settlements (
            settlement_id,
            invocation_id,
            card_id,
            remote_agent_url,
            local_task_id,
            remote_task_id,
            transaction_id,
            mandate_id,
            quote_id,
            amount,
            description,
            status,
            verification_message,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(settlement_id) DO UPDATE SET
            invocation_id = excluded.invocation_id,
            card_id = excluded.card_id,
            remote_agent_url = excluded.remote_agent_url,
            local_task_id = excluded.local_task_id,
            remote_task_id = excluded.remote_task_id,
            transaction_id = excluded.transaction_id,
            mandate_id = excluded.mandate_id,
            quote_id = excluded.quote_id,
            amount = excluded.amount,
            description = excluded.description,
            status = excluded.status,
            verification_message = excluded.verification_message,
            created_at_unix_ms = remote_agent_settlements.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(record.settlement_id.to_string())
    .bind(record.invocation_id.to_string())
    .bind(&record.card_id)
    .bind(&record.remote_agent_url)
    .bind(record.local_task_id.map(|value| value.to_string()))
    .bind(&record.remote_task_id)
    .bind(record.transaction_id.to_string())
    .bind(record.mandate_id.to_string())
    .bind(&record.quote_id)
    .bind(record.amount)
    .bind(&record.description)
    .bind(record.status.as_db())
    .bind(&record.verification_message)
    .bind(record.created_at_unix_ms as i64)
    .bind(record.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to save remote settlement {}", record.settlement_id))?;

    Ok(())
}

async fn save_quote_record(state: &AppState, record: &QuoteLedgerRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO agent_quote_ledger (
            quote_id,
            card_id,
            source_kind,
            quote_url,
            previous_quote_id,
            superseded_by_quote_id,
            negotiation_round,
            settlement_supported,
            payment_roles,
            currency,
            quote_mode,
            requested_amount,
            quoted_amount,
            counter_offer_amount,
            min_amount,
            max_amount,
            description_template,
            warning,
            expires_at_unix_ms,
            issuer_did,
            signature_hex,
            status,
            consumed_by_transaction_id,
            revoked_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
        )
        ON CONFLICT(quote_id) DO UPDATE SET
            card_id = excluded.card_id,
            source_kind = excluded.source_kind,
            quote_url = excluded.quote_url,
            previous_quote_id = excluded.previous_quote_id,
            superseded_by_quote_id = excluded.superseded_by_quote_id,
            negotiation_round = excluded.negotiation_round,
            settlement_supported = excluded.settlement_supported,
            payment_roles = excluded.payment_roles,
            currency = excluded.currency,
            quote_mode = excluded.quote_mode,
            requested_amount = excluded.requested_amount,
            quoted_amount = excluded.quoted_amount,
            counter_offer_amount = excluded.counter_offer_amount,
            min_amount = excluded.min_amount,
            max_amount = excluded.max_amount,
            description_template = excluded.description_template,
            warning = excluded.warning,
            expires_at_unix_ms = excluded.expires_at_unix_ms,
            issuer_did = excluded.issuer_did,
            signature_hex = excluded.signature_hex,
            status = excluded.status,
            consumed_by_transaction_id = excluded.consumed_by_transaction_id,
            revoked_reason = excluded.revoked_reason,
            created_at_unix_ms = agent_quote_ledger.created_at_unix_ms,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&record.quote_id)
    .bind(&record.card_id)
    .bind(&record.source_kind)
    .bind(&record.quote_url)
    .bind(&record.previous_quote_id)
    .bind(&record.superseded_by_quote_id)
    .bind(i64::from(record.negotiation_round))
    .bind(if record.settlement_supported { 1_i64 } else { 0_i64 })
    .bind(serde_json::to_string(&record.payment_roles)?)
    .bind(&record.currency)
    .bind(&record.quote_mode)
    .bind(record.requested_amount)
    .bind(record.quoted_amount)
    .bind(record.counter_offer_amount)
    .bind(record.min_amount)
    .bind(record.max_amount)
    .bind(&record.description_template)
    .bind(&record.warning)
    .bind(record.expires_at_unix_ms.map(|value| value as i64))
    .bind(&record.issuer_did)
    .bind(&record.signature_hex)
    .bind(record.status.as_db())
    .bind(record.consumed_by_transaction_id.map(|value| value.to_string()))
    .bind(&record.revoked_reason)
    .bind(record.created_at_unix_ms as i64)
    .bind(record.updated_at_unix_ms as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to save quote ledger record {}", record.quote_id))?;

    Ok(())
}

async fn get_remote_invocation(
    state: &AppState,
    invocation_id: Uuid,
) -> anyhow::Result<Option<RemoteAgentInvocationRecord>> {
    let row = sqlx::query_as::<_, RemoteAgentInvocationRow>(
        r#"
        SELECT
            invocation_id,
            card_id,
            remote_agent_url,
            local_task_id,
            remote_task_id,
            request_json,
            response_json,
            status,
            error,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM remote_agent_invocations
        WHERE invocation_id = ?1
        "#,
    )
    .bind(invocation_id.to_string())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch remote invocation {invocation_id}"))?;

    row.map(row_to_remote_invocation).transpose()
}

async fn get_remote_settlement(
    state: &AppState,
    settlement_id: Uuid,
) -> anyhow::Result<Option<RemoteAgentSettlementRecord>> {
    let row = sqlx::query_as::<_, RemoteAgentSettlementRow>(
        r#"
        SELECT
            settlement_id,
            invocation_id,
            card_id,
            remote_agent_url,
            local_task_id,
            remote_task_id,
            transaction_id,
            mandate_id,
            quote_id,
            amount,
            description,
            status,
            verification_message,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM remote_agent_settlements
        WHERE settlement_id = ?1
        "#,
    )
    .bind(settlement_id.to_string())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch remote settlement {settlement_id}"))?;

    row.map(row_to_remote_settlement).transpose()
}

async fn get_quote_record(state: &AppState, quote_id: &str) -> anyhow::Result<Option<QuoteLedgerRecord>> {
    let row = sqlx::query_as::<_, QuoteLedgerRow>(
        r#"
        SELECT
            quote_id,
            card_id,
            source_kind,
            quote_url,
            previous_quote_id,
            superseded_by_quote_id,
            negotiation_round,
            settlement_supported,
            payment_roles,
            currency,
            quote_mode,
            requested_amount,
            quoted_amount,
            counter_offer_amount,
            min_amount,
            max_amount,
            description_template,
            warning,
            expires_at_unix_ms,
            issuer_did,
            signature_hex,
            status,
            consumed_by_transaction_id,
            revoked_reason,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM agent_quote_ledger
        WHERE quote_id = ?1
        "#,
    )
    .bind(quote_id)
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch quote ledger record {quote_id}"))?;

    row.map(row_to_quote_record).transpose()
}

async fn get_remote_settlement_by_invocation(
    state: &AppState,
    invocation_id: Uuid,
) -> anyhow::Result<Option<RemoteAgentSettlementRecord>> {
    let row = sqlx::query_as::<_, RemoteAgentSettlementRow>(
        r#"
        SELECT
            settlement_id,
            invocation_id,
            card_id,
            remote_agent_url,
            local_task_id,
            remote_task_id,
            transaction_id,
            mandate_id,
            quote_id,
            amount,
            description,
            status,
            verification_message,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM remote_agent_settlements
        WHERE invocation_id = ?1
        ORDER BY created_at_unix_ms DESC
        LIMIT 1
        "#,
    )
    .bind(invocation_id.to_string())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch settlement for invocation {invocation_id}"))?;

    row.map(row_to_remote_settlement).transpose()
}

async fn get_remote_settlement_by_transaction(
    state: &AppState,
    transaction_id: Uuid,
) -> anyhow::Result<Option<RemoteAgentSettlementRecord>> {
    let row = sqlx::query_as::<_, RemoteAgentSettlementRow>(
        r#"
        SELECT
            settlement_id,
            invocation_id,
            card_id,
            remote_agent_url,
            local_task_id,
            remote_task_id,
            transaction_id,
            mandate_id,
            quote_id,
            amount,
            description,
            status,
            verification_message,
            created_at_unix_ms,
            updated_at_unix_ms
        FROM remote_agent_settlements
        WHERE transaction_id = ?1
        LIMIT 1
        "#,
    )
    .bind(transaction_id.to_string())
    .fetch_optional(state.pool())
    .await
    .with_context(|| format!("failed to fetch settlement for transaction {transaction_id}"))?;

    row.map(row_to_remote_settlement).transpose()
}

async fn list_remote_invocations(
    state: &AppState,
    card_id: Option<&str>,
    local_task_id: Option<Uuid>,
) -> anyhow::Result<Vec<RemoteAgentInvocationRecord>> {
    let rows = match (card_id, local_task_id) {
        (Some(card_id), Some(local_task_id)) => sqlx::query_as::<_, RemoteAgentInvocationRow>(
            r#"
                SELECT
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    request_json,
                    response_json,
                    status,
                    error,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_invocations
                WHERE card_id = ?1 AND local_task_id = ?2
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(card_id)
        .bind(local_task_id.to_string())
        .fetch_all(state.pool())
        .await
        .context("failed to list remote invocations by card_id and local_task_id")?,
        (Some(card_id), None) => sqlx::query_as::<_, RemoteAgentInvocationRow>(
            r#"
                SELECT
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    request_json,
                    response_json,
                    status,
                    error,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_invocations
                WHERE card_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(card_id)
        .fetch_all(state.pool())
        .await
        .context("failed to list remote invocations by card_id")?,
        (None, Some(local_task_id)) => sqlx::query_as::<_, RemoteAgentInvocationRow>(
            r#"
                SELECT
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    request_json,
                    response_json,
                    status,
                    error,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_invocations
                WHERE local_task_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(local_task_id.to_string())
        .fetch_all(state.pool())
        .await
        .context("failed to list remote invocations by local_task_id")?,
        (None, None) => sqlx::query_as::<_, RemoteAgentInvocationRow>(
            r#"
                SELECT
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    request_json,
                    response_json,
                    status,
                    error,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_invocations
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .fetch_all(state.pool())
        .await
        .context("failed to list remote invocations")?,
    };

    rows.into_iter().map(row_to_remote_invocation).collect()
}

async fn list_remote_settlements(
    state: &AppState,
    card_id: Option<&str>,
    invocation_id: Option<Uuid>,
    local_task_id: Option<Uuid>,
    transaction_id: Option<Uuid>,
    status: Option<PaymentStatus>,
) -> anyhow::Result<Vec<RemoteAgentSettlementRecord>> {
    let rows = match (card_id, invocation_id, local_task_id, transaction_id, status) {
        (Some(card_id), _, _, _, _) => sqlx::query_as::<_, RemoteAgentSettlementRow>(
            r#"
                SELECT
                    settlement_id,
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    transaction_id,
                    mandate_id,
                    quote_id,
                    amount,
                    description,
                    status,
                    verification_message,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_settlements
                WHERE card_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(card_id)
        .fetch_all(state.pool())
        .await
        .context("failed to list remote settlements by card_id")?,
        (_, Some(invocation_id), _, _, _) => sqlx::query_as::<_, RemoteAgentSettlementRow>(
            r#"
                SELECT
                    settlement_id,
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    transaction_id,
                    mandate_id,
                    quote_id,
                    amount,
                    description,
                    status,
                    verification_message,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_settlements
                WHERE invocation_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(invocation_id.to_string())
        .fetch_all(state.pool())
        .await
        .context("failed to list remote settlements by invocation_id")?,
        (_, _, Some(local_task_id), _, _) => sqlx::query_as::<_, RemoteAgentSettlementRow>(
            r#"
                SELECT
                    settlement_id,
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    transaction_id,
                    mandate_id,
                    quote_id,
                    amount,
                    description,
                    status,
                    verification_message,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_settlements
                WHERE local_task_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(local_task_id.to_string())
        .fetch_all(state.pool())
        .await
        .context("failed to list remote settlements by local_task_id")?,
        (_, _, _, Some(transaction_id), _) => sqlx::query_as::<_, RemoteAgentSettlementRow>(
            r#"
                SELECT
                    settlement_id,
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    transaction_id,
                    mandate_id,
                    quote_id,
                    amount,
                    description,
                    status,
                    verification_message,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_settlements
                WHERE transaction_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(transaction_id.to_string())
        .fetch_all(state.pool())
        .await
        .context("failed to list remote settlements by transaction_id")?,
        (_, _, _, _, Some(status)) => sqlx::query_as::<_, RemoteAgentSettlementRow>(
            r#"
                SELECT
                    settlement_id,
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    transaction_id,
                    mandate_id,
                    quote_id,
                    amount,
                    description,
                    status,
                    verification_message,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_settlements
                WHERE status = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(status.as_db())
        .fetch_all(state.pool())
        .await
        .context("failed to list remote settlements by status")?,
        _ => sqlx::query_as::<_, RemoteAgentSettlementRow>(
            r#"
                SELECT
                    settlement_id,
                    invocation_id,
                    card_id,
                    remote_agent_url,
                    local_task_id,
                    remote_task_id,
                    transaction_id,
                    mandate_id,
                    quote_id,
                    amount,
                    description,
                    status,
                    verification_message,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM remote_agent_settlements
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .fetch_all(state.pool())
        .await
        .context("failed to list remote settlements")?,
    };

    rows.into_iter().map(row_to_remote_settlement).collect()
}

async fn list_quote_records(
    state: &AppState,
    card_id: Option<&str>,
    status: Option<QuoteLedgerStatus>,
    source_kind: Option<&str>,
    transaction_id: Option<Uuid>,
) -> anyhow::Result<Vec<QuoteLedgerRecord>> {
    let rows = match (card_id, status, source_kind, transaction_id) {
        (Some(card_id), _, _, _) => sqlx::query_as::<_, QuoteLedgerRow>(
            r#"
                SELECT
                    quote_id,
                    card_id,
                    source_kind,
                    quote_url,
                    previous_quote_id,
                    superseded_by_quote_id,
                    negotiation_round,
                    settlement_supported,
                    payment_roles,
                    currency,
                    quote_mode,
                    requested_amount,
                    quoted_amount,
                    counter_offer_amount,
                    min_amount,
                    max_amount,
                    description_template,
                    warning,
                    expires_at_unix_ms,
                    issuer_did,
                    signature_hex,
                    status,
                    consumed_by_transaction_id,
                    revoked_reason,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM agent_quote_ledger
                WHERE card_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(card_id)
        .fetch_all(state.pool())
        .await
        .context("failed to list quote ledger by card_id")?,
        (_, Some(status), _, _) => sqlx::query_as::<_, QuoteLedgerRow>(
            r#"
                SELECT
                    quote_id,
                    card_id,
                    source_kind,
                    quote_url,
                    previous_quote_id,
                    superseded_by_quote_id,
                    negotiation_round,
                    settlement_supported,
                    payment_roles,
                    currency,
                    quote_mode,
                    requested_amount,
                    quoted_amount,
                    counter_offer_amount,
                    min_amount,
                    max_amount,
                    description_template,
                    warning,
                    expires_at_unix_ms,
                    issuer_did,
                    signature_hex,
                    status,
                    consumed_by_transaction_id,
                    revoked_reason,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM agent_quote_ledger
                WHERE status = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(status.as_db())
        .fetch_all(state.pool())
        .await
        .context("failed to list quote ledger by status")?,
        (_, _, Some(source_kind), _) => sqlx::query_as::<_, QuoteLedgerRow>(
            r#"
                SELECT
                    quote_id,
                    card_id,
                    source_kind,
                    quote_url,
                    previous_quote_id,
                    superseded_by_quote_id,
                    negotiation_round,
                    settlement_supported,
                    payment_roles,
                    currency,
                    quote_mode,
                    requested_amount,
                    quoted_amount,
                    counter_offer_amount,
                    min_amount,
                    max_amount,
                    description_template,
                    warning,
                    expires_at_unix_ms,
                    issuer_did,
                    signature_hex,
                    status,
                    consumed_by_transaction_id,
                    revoked_reason,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM agent_quote_ledger
                WHERE source_kind = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(source_kind)
        .fetch_all(state.pool())
        .await
        .context("failed to list quote ledger by source_kind")?,
        (_, _, _, Some(transaction_id)) => sqlx::query_as::<_, QuoteLedgerRow>(
            r#"
                SELECT
                    quote_id,
                    card_id,
                    source_kind,
                    quote_url,
                    previous_quote_id,
                    superseded_by_quote_id,
                    negotiation_round,
                    settlement_supported,
                    payment_roles,
                    currency,
                    quote_mode,
                    requested_amount,
                    quoted_amount,
                    counter_offer_amount,
                    min_amount,
                    max_amount,
                    description_template,
                    warning,
                    expires_at_unix_ms,
                    issuer_did,
                    signature_hex,
                    status,
                    consumed_by_transaction_id,
                    revoked_reason,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM agent_quote_ledger
                WHERE consumed_by_transaction_id = ?1
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .bind(transaction_id.to_string())
        .fetch_all(state.pool())
        .await
        .context("failed to list quote ledger by consumed transaction")?,
        _ => sqlx::query_as::<_, QuoteLedgerRow>(
            r#"
                SELECT
                    quote_id,
                    card_id,
                    source_kind,
                    quote_url,
                    previous_quote_id,
                    superseded_by_quote_id,
                    negotiation_round,
                    settlement_supported,
                    payment_roles,
                    currency,
                    quote_mode,
                    requested_amount,
                    quoted_amount,
                    counter_offer_amount,
                    min_amount,
                    max_amount,
                    description_template,
                    warning,
                    expires_at_unix_ms,
                    issuer_did,
                    signature_hex,
                    status,
                    consumed_by_transaction_id,
                    revoked_reason,
                    created_at_unix_ms,
                    updated_at_unix_ms
                FROM agent_quote_ledger
                ORDER BY created_at_unix_ms DESC
                "#,
        )
        .fetch_all(state.pool())
        .await
        .context("failed to list quote ledger")?,
    };

    rows.into_iter().map(row_to_quote_record).collect()
}

async fn revoke_quote_record(
    state: &AppState,
    quote_id: &str,
    reason: Option<&str>,
) -> anyhow::Result<QuoteLedgerRecord> {
    let mut record = get_quote_record(state, quote_id)
        .await?
        .ok_or_else(|| anyhow!("quote not found: {quote_id}"))?;
    match record.status {
        QuoteLedgerStatus::Consumed => {
            anyhow::bail!("quote '{}' has already been consumed by a settlement", quote_id)
        }
        QuoteLedgerStatus::Superseded => {
            anyhow::bail!("quote '{}' has already been superseded", quote_id)
        }
        QuoteLedgerStatus::Revoked => return Ok(record),
        QuoteLedgerStatus::Offered => {}
    }
    record.status = QuoteLedgerStatus::Revoked;
    record.revoked_reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| Some("manually revoked".to_string()));
    record.updated_at_unix_ms = unix_timestamp_ms();
    save_quote_record(state, &record).await?;
    Ok(record)
}

async fn persist_card_source_metadata(
    state: &AppState,
    card_id: &str,
    source_kind: &str,
    card_url: Option<String>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE agent_cards
        SET source_kind = ?2, card_url = ?3, updated_at_unix_ms = ?4
        WHERE card_id = ?1
        "#,
    )
    .bind(card_id)
    .bind(source_kind)
    .bind(card_url)
    .bind(unix_timestamp_ms() as i64)
    .execute(state.pool())
    .await
    .with_context(|| format!("failed to update source metadata for agent card {card_id}"))?;
    Ok(())
}

fn row_to_card_record(row: AgentCardRow) -> anyhow::Result<PublishedAgentCard> {
    Ok(PublishedAgentCard {
        card_id: row.card_id,
        source_kind: row.source_kind,
        card_url: row.card_url,
        published: row.published != 0,
        locally_hosted: row.locally_hosted != 0,
        issuer_did: row.issuer_did,
        signature_hex: row.signature_hex,
        regions: serde_json::from_str(&row.regions)
            .context("failed to parse agent card regions")?,
        languages: serde_json::from_str(&row.languages)
            .context("failed to parse agent card languages")?,
        model_providers: serde_json::from_str(&row.model_providers)
            .context("failed to parse agent card model providers")?,
        chat_platforms: serde_json::from_str(&row.chat_platforms)
            .context("failed to parse agent card chat platforms")?,
        payment_roles: serde_json::from_str(&row.payment_roles)
            .context("failed to parse agent card payment roles")?,
        created_at_unix_ms: u128::try_from(row.created_at_unix_ms)
            .context("negative created_at_unix_ms in agent_cards")?,
        updated_at_unix_ms: u128::try_from(row.updated_at_unix_ms)
            .context("negative updated_at_unix_ms in agent_cards")?,
        card: serde_json::from_str(&row.card_json).context("failed to parse stored agent card")?,
    })
}

fn row_to_remote_invocation(
    row: RemoteAgentInvocationRow,
) -> anyhow::Result<RemoteAgentInvocationRecord> {
    Ok(RemoteAgentInvocationRecord {
        invocation_id: Uuid::parse_str(&row.invocation_id)
            .with_context(|| format!("invalid invocation_id '{}'", row.invocation_id))?,
        card_id: row.card_id,
        remote_agent_url: row.remote_agent_url,
        local_task_id: row
            .local_task_id
            .map(|value| {
                Uuid::parse_str(&value).with_context(|| format!("invalid local_task_id '{value}'"))
            })
            .transpose()?,
        remote_task_id: row.remote_task_id,
        request: serde_json::from_str(&row.request_json)
            .context("failed to parse remote invocation request_json")?,
        response: row
            .response_json
            .map(|value| {
                serde_json::from_str(&value)
                    .context("failed to parse remote invocation response_json")
            })
            .transpose()?,
        status: RemoteInvocationStatus::from_db(&row.status)?,
        error: row.error,
        created_at_unix_ms: u128::try_from(row.created_at_unix_ms)
            .context("negative created_at_unix_ms in remote_agent_invocations")?,
        updated_at_unix_ms: u128::try_from(row.updated_at_unix_ms)
            .context("negative updated_at_unix_ms in remote_agent_invocations")?,
    })
}

fn row_to_remote_settlement(
    row: RemoteAgentSettlementRow,
) -> anyhow::Result<RemoteAgentSettlementRecord> {
    Ok(RemoteAgentSettlementRecord {
        settlement_id: Uuid::parse_str(&row.settlement_id)
            .with_context(|| format!("invalid settlement_id '{}'", row.settlement_id))?,
        invocation_id: Uuid::parse_str(&row.invocation_id)
            .with_context(|| format!("invalid invocation_id '{}'", row.invocation_id))?,
        card_id: row.card_id,
        remote_agent_url: row.remote_agent_url,
        local_task_id: row
            .local_task_id
            .map(|value| {
                Uuid::parse_str(&value).with_context(|| format!("invalid local_task_id '{value}'"))
            })
            .transpose()?,
        remote_task_id: row.remote_task_id,
        transaction_id: Uuid::parse_str(&row.transaction_id)
            .with_context(|| format!("invalid transaction_id '{}'", row.transaction_id))?,
        mandate_id: Uuid::parse_str(&row.mandate_id)
            .with_context(|| format!("invalid mandate_id '{}'", row.mandate_id))?,
        quote_id: row.quote_id,
        amount: row.amount,
        description: row.description,
        status: PaymentStatus::from_db(&row.status)?,
        verification_message: row.verification_message,
        created_at_unix_ms: u128::try_from(row.created_at_unix_ms)
            .context("negative created_at_unix_ms in remote_agent_settlements")?,
        updated_at_unix_ms: u128::try_from(row.updated_at_unix_ms)
            .context("negative updated_at_unix_ms in remote_agent_settlements")?,
    })
}

fn row_to_quote_record(row: QuoteLedgerRow) -> anyhow::Result<QuoteLedgerRecord> {
    Ok(QuoteLedgerRecord {
        quote_id: row.quote_id,
        card_id: row.card_id,
        source_kind: row.source_kind,
        quote_url: row.quote_url,
        previous_quote_id: row.previous_quote_id,
        superseded_by_quote_id: row.superseded_by_quote_id,
        negotiation_round: u32::try_from(row.negotiation_round)
            .context("negative negotiation_round in agent_quote_ledger")?,
        settlement_supported: row.settlement_supported != 0,
        payment_roles: serde_json::from_str(&row.payment_roles)
            .context("failed to parse quote ledger payment_roles")?,
        currency: row.currency,
        quote_mode: row.quote_mode,
        requested_amount: row.requested_amount,
        quoted_amount: row.quoted_amount,
        counter_offer_amount: row.counter_offer_amount,
        min_amount: row.min_amount,
        max_amount: row.max_amount,
        description_template: row.description_template,
        warning: row.warning,
        expires_at_unix_ms: row
            .expires_at_unix_ms
            .map(|value| u128::try_from(value).context("negative expires_at_unix_ms in agent_quote_ledger"))
            .transpose()?,
        issuer_did: row.issuer_did,
        signature_hex: row.signature_hex,
        status: QuoteLedgerStatus::from_db(&row.status)?,
        consumed_by_transaction_id: row
            .consumed_by_transaction_id
            .map(|value| {
                Uuid::parse_str(&value)
                    .with_context(|| format!("invalid consumed_by_transaction_id '{value}'"))
            })
            .transpose()?,
        revoked_reason: row.revoked_reason,
        created_at_unix_ms: u128::try_from(row.created_at_unix_ms)
            .context("negative created_at_unix_ms in agent_quote_ledger")?,
        updated_at_unix_ms: u128::try_from(row.updated_at_unix_ms)
            .context("negative updated_at_unix_ms in agent_quote_ledger")?,
    })
}

fn validate_card_id(card_id: &str) -> anyhow::Result<()> {
    if card_id.is_empty() {
        anyhow::bail!("cardId cannot be empty");
    }
    if !card_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        anyhow::bail!(
            "cardId may only contain ASCII letters, digits, dash, underscore, and period"
        );
    }
    Ok(())
}

fn validate_card(card: &AgentCard) -> anyhow::Result<()> {
    if card.name.trim().is_empty() {
        anyhow::bail!("agent card name cannot be empty");
    }
    if card.description.trim().is_empty() {
        anyhow::bail!("agent card description cannot be empty");
    }
    if card.url.trim().is_empty() {
        anyhow::bail!("agent card url cannot be empty");
    }
    if card.version.trim().is_empty() {
        anyhow::bail!("agent card version cannot be empty");
    }
    Ok(())
}

fn derive_card_id(name: &str, url: &str) -> String {
    let slug = slugify(name);
    let hash = hex::encode(Sha256::digest(url.as_bytes()));
    format!("{slug}-{}", &hash[..8])
}

fn remote_task_create_url(agent_url: &str) -> String {
    if agent_url.ends_with("/task") {
        agent_url.to_string()
    } else {
        format!("{}/task", agent_url.trim_end_matches('/'))
    }
}

fn remote_task_detail_url(agent_url: &str, remote_task_id: &str) -> String {
    if agent_url.ends_with("/task") {
        format!("{}/{}", agent_url.trim_end_matches('/'), remote_task_id)
    } else {
        format!(
            "{}/task/{}",
            agent_url.trim_end_matches('/'),
            remote_task_id
        )
    }
}

fn extract_remote_task_id(response: &Value) -> Option<String> {
    response
        .get("task")
        .and_then(|task| task.get("taskId").or_else(|| task.get("task_id")))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            response
                .get("taskId")
                .or_else(|| response.get("task_id"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn extract_remote_task_status(response: &Value) -> Option<String> {
    response
        .get("task")
        .and_then(|task| task.get("status"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            response
                .get("status")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn is_remote_terminal_success(response: &Value) -> bool {
    matches!(
        extract_remote_task_status(response).as_deref(),
        Some("completed" | "authorized" | "succeeded" | "success")
    )
}

fn is_remote_terminal_failure(response: &Value) -> bool {
    matches!(
        extract_remote_task_status(response).as_deref(),
        Some("failed" | "rejected" | "error")
    )
}

fn slugify(input: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    output.trim_matches('-').to_string()
}

fn extract_ap2_roles(card: &AgentCard) -> Vec<String> {
    extract_ap2_terms(card).roles
}

fn extract_ap2_terms(card: &AgentCard) -> AgentPaymentTerms {
    let Some(params) = card
        .capabilities
        .extensions
        .iter()
        .find(|extension| extension.uri == AP2_EXTENSION_URI)
        .and_then(|extension| extension.params.as_ref())
    else {
        return AgentPaymentTerms::default();
    };

    let pricing = params.get("pricing").unwrap_or(params);
    AgentPaymentTerms {
        roles: params
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| {
                roles
                    .iter()
                    .filter_map(|role| role.as_str().map(|value| value.to_ascii_lowercase()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        currency: pricing
            .get("currency")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty()),
        quote_mode: pricing
            .get("quoteMode")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty()),
        quote_method: pricing
            .get("quoteMethod")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty()),
        quote_url: pricing
            .get("quoteUrl")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .filter(|value| !value.trim().is_empty()),
        quote_path: pricing
            .get("quotePath")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .filter(|value| !value.trim().is_empty()),
        quote_state_url_template: pricing
            .get("quoteStateUrlTemplate")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .filter(|value| !value.trim().is_empty()),
        quote_issuer_did: pricing
            .get("quoteIssuerDid")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty()),
        flat_amount: pricing.get("flatAmount").and_then(Value::as_f64),
        min_amount: pricing.get("minAmount").and_then(Value::as_f64),
        max_amount: pricing.get("maxAmount").and_then(Value::as_f64),
        description_template: pricing
            .get("descriptionTemplate")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .filter(|value| !value.trim().is_empty()),
    }
}

fn build_settlement_quote(
    card: &PublishedAgentCard,
    requested_amount: Option<f64>,
    description: Option<&str>,
) -> AgentSettlementQuote {
    let terms = extract_ap2_terms(&card.card);
    let quote_url = resolve_quote_url(card, &terms);
    let settlement_supported = contains_ci(&card.payment_roles, "payee")
        || contains_ci(&card.payment_roles, "merchant")
        || contains_ci(&terms.roles, "payee")
        || contains_ci(&terms.roles, "merchant");
    let quote_mode = terms
        .quote_mode
        .clone()
        .unwrap_or_else(|| if terms.flat_amount.is_some() { "flat" } else { "manual" }.to_string());
    let quoted_amount = terms.flat_amount.or(requested_amount);
    let warning = if !settlement_supported {
        Some("agent card does not advertise AP2 settlement capability".to_string())
    } else if let (Some(requested), Some(flat_amount)) = (requested_amount, terms.flat_amount) {
        if (requested - flat_amount).abs() > f64::EPSILON {
            Some(format!(
                "requested amount {:.2} differs from flat quote {:.2}",
                requested, flat_amount
            ))
        } else {
            None
        }
    } else if let (Some(requested), Some(min_amount)) = (requested_amount, terms.min_amount) {
        if requested < min_amount {
            Some(format!(
                "requested amount {:.2} is below minAmount {:.2}",
                requested, min_amount
            ))
        } else if let Some(max_amount) = terms.max_amount {
            if requested > max_amount {
                Some(format!(
                    "requested amount {:.2} exceeds maxAmount {:.2}",
                    requested, max_amount
                ))
            } else {
                None
            }
        } else {
            None
        }
    } else if let (Some(requested), Some(max_amount)) = (requested_amount, terms.max_amount) {
        if requested > max_amount {
            Some(format!(
                "requested amount {:.2} exceeds maxAmount {:.2}",
                requested, max_amount
            ))
        } else {
            None
        }
    } else {
        None
    };

    AgentSettlementQuote {
        card_id: card.card_id.clone(),
        settlement_supported,
        payment_roles: merge_unique_metadata(Some(card.payment_roles.clone()), terms.roles),
        currency: terms.currency,
        quote_mode,
        quote_source: "metadata".to_string(),
        quote_url,
        quote_id: None,
        previous_quote_id: None,
        counter_offer_amount: None,
        requested_amount,
        quoted_amount,
        min_amount: terms.min_amount,
        max_amount: terms.max_amount,
        description_template: description
            .map(ToString::to_string)
            .or(terms.description_template),
        warning,
        expires_at_unix_ms: None,
        issuer_did: None,
        signature_hex: None,
    }
}

fn apply_counter_offer_to_quote(
    mut quote: AgentSettlementQuote,
    counter_offer_amount: Option<f64>,
    previous_quote_id: Option<&str>,
) -> AgentSettlementQuote {
    let original_quote_mode = quote.quote_mode.clone();
    quote.previous_quote_id = previous_quote_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let Some(counter_offer_amount) = counter_offer_amount else {
        return quote;
    };

    quote.counter_offer_amount = Some(counter_offer_amount);
    if counter_offer_amount <= 0.0 {
        quote.quote_mode = "counter_offer".to_string();
        quote.warning = Some("counter-offer amount must be positive".to_string());
        return quote;
    }
    if let Some(min_amount) = quote.min_amount {
        if counter_offer_amount < min_amount {
            quote.warning = Some(format!(
                "counter-offer amount {:.2} is below minAmount {:.2}",
                counter_offer_amount, min_amount
            ));
            return quote;
        }
    }
    if let Some(max_amount) = quote.max_amount {
        if counter_offer_amount > max_amount {
            quote.quote_mode = "counter_offer".to_string();
            quote.warning = Some(format!(
                "counter-offer amount {:.2} exceeds maxAmount {:.2}",
                counter_offer_amount, max_amount
            ));
            return quote;
        }
    }

    if matches!(original_quote_mode.as_str(), "flat" | "fixed") {
        quote.quote_mode = "counter_offer".to_string();
        if let Some(quoted_amount) = quote.quoted_amount {
            if (quoted_amount - counter_offer_amount).abs() > f64::EPSILON {
                quote.warning = Some(format!(
                    "counter-offer amount {:.2} was not accepted; current quote remains {:.2}",
                    counter_offer_amount, quoted_amount
                ));
            }
        }
        return quote;
    }

    quote.quote_mode = "counter_offer".to_string();
    quote.quoted_amount = Some(counter_offer_amount);
    quote.warning = None;
    quote
}

fn default_quote_ttl_seconds() -> u64 {
    std::env::var("DAWN_QUOTE_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_QUOTE_TTL_SECONDS)
}

fn quote_signing_key() -> anyhow::Result<SigningKey> {
    let bytes = match std::env::var("DAWN_QUOTE_SIGNING_SEED_HEX") {
        Ok(value) => decode_fixed_hex::<32>(&value, "quote signing seed")?,
        Err(_) => [41_u8; 32],
    };
    Ok(SigningKey::from_bytes(&bytes))
}

fn quote_issuer_did_from_public_key_hex(public_key_hex: &str) -> anyhow::Result<String> {
    let bytes = decode_fixed_hex::<32>(public_key_hex, "quote public key")?;
    Ok(format!("{QUOTE_ISSUER_DID_PREFIX}{}", hex::encode(bytes)))
}

fn quote_issuer_did_from_signing_key(signing_key: &SigningKey) -> String {
    format!(
        "{QUOTE_ISSUER_DID_PREFIX}{}",
        hex::encode(signing_key.verifying_key().to_bytes())
    )
}

fn validate_quote_issuer_did(issuer_did: &str, public_key_hex: &str) -> anyhow::Result<()> {
    let expected = quote_issuer_did_from_public_key_hex(public_key_hex)?;
    let normalized = issuer_did.to_ascii_lowercase();
    if normalized != expected {
        anyhow::bail!(
            "quote issuer DID '{}' does not match public key; expected '{}'",
            issuer_did,
            expected
        );
    }
    Ok(())
}

fn decode_quote_verifying_key_from_did(issuer_did: &str) -> anyhow::Result<VerifyingKey> {
    let normalized = issuer_did.trim().to_ascii_lowercase();
    let raw = normalized
        .strip_prefix(QUOTE_ISSUER_DID_PREFIX)
        .ok_or_else(|| anyhow!("quote issuer DID must start with '{}'", QUOTE_ISSUER_DID_PREFIX))?;
    let public_key_bytes = decode_fixed_hex::<32>(raw, "quote issuer public key")?;
    VerifyingKey::from_bytes(&public_key_bytes).context("invalid quote issuer Ed25519 public key")
}

fn signed_quote_payload(document: &SignedSettlementQuoteDocument) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(document).context("failed to serialize settlement quote document")
}

fn sign_local_settlement_quote(
    card: &PublishedAgentCard,
    mut quote: AgentSettlementQuote,
) -> anyhow::Result<AgentSettlementQuote> {
    let signing_key = quote_signing_key()?;
    let issuer_did = quote_issuer_did_from_signing_key(&signing_key);
    let expires_at_unix_ms =
        unix_timestamp_ms() + u128::from(default_quote_ttl_seconds()).saturating_mul(1000);
    let document = SignedSettlementQuoteDocument {
        card_id: card.card_id.clone(),
        settlement_supported: quote.settlement_supported,
        payment_roles: quote.payment_roles.clone(),
        currency: quote.currency.clone(),
        quote_mode: quote.quote_mode.clone(),
        quote_source: "local_signed".to_string(),
        quote_url: quote.quote_url.clone(),
        quote_id: Uuid::new_v4().to_string(),
        previous_quote_id: quote.previous_quote_id.clone(),
        counter_offer_amount: quote.counter_offer_amount,
        requested_amount: quote.requested_amount,
        quoted_amount: quote.quoted_amount,
        min_amount: quote.min_amount,
        max_amount: quote.max_amount,
        description_template: quote.description_template.clone(),
        warning: quote.warning.clone(),
        expires_at_unix_ms,
        issuer_did: issuer_did.clone(),
    };
    let signature_hex = hex::encode(signing_key.sign(&signed_quote_payload(&document)?).to_bytes());
    quote.quote_source = document.quote_source;
    quote.quote_id = Some(document.quote_id);
    quote.previous_quote_id = document.previous_quote_id;
    quote.counter_offer_amount = document.counter_offer_amount;
    quote.expires_at_unix_ms = Some(document.expires_at_unix_ms);
    quote.issuer_did = Some(issuer_did);
    quote.signature_hex = Some(signature_hex);
    Ok(quote)
}

fn verify_signed_quote(
    card: &PublishedAgentCard,
    quote: &AgentSettlementQuote,
) -> anyhow::Result<()> {
    if quote.card_id != card.card_id {
        anyhow::bail!(
            "signed quote cardId '{}' does not match requested card '{}'",
            quote.card_id,
            card.card_id
        );
    }
    let Some(issuer_did) = quote.issuer_did.as_deref() else {
        anyhow::bail!("signed quote is missing issuerDid");
    };
    let Some(signature_hex) = quote.signature_hex.as_deref() else {
        anyhow::bail!("signed quote is missing signatureHex");
    };
    let expires_at_unix_ms = quote
        .expires_at_unix_ms
        .ok_or_else(|| anyhow!("signed quote is missing expiresAtUnixMs"))?;
    if expires_at_unix_ms < unix_timestamp_ms() {
        anyhow::bail!("signed quote '{}' has expired", quote.quote_id.as_deref().unwrap_or("?"));
    }

    let terms = extract_ap2_terms(&card.card);
    if let Some(expected_issuer_did) = terms.quote_issuer_did.as_deref() {
        if issuer_did.to_ascii_lowercase() != expected_issuer_did {
            anyhow::bail!(
                "quote issuer '{}' does not match expected issuer '{}'",
                issuer_did,
                expected_issuer_did
            );
        }
    }

    let verifying_key = decode_quote_verifying_key_from_did(issuer_did)?;
    validate_quote_issuer_did(issuer_did, &hex::encode(verifying_key.to_bytes()))?;
    let document = SignedSettlementQuoteDocument {
        card_id: quote.card_id.clone(),
        settlement_supported: quote.settlement_supported,
        payment_roles: quote.payment_roles.clone(),
        currency: quote.currency.clone(),
        quote_mode: quote.quote_mode.clone(),
        quote_source: quote.quote_source.clone(),
        quote_url: quote.quote_url.clone(),
        quote_id: quote
            .quote_id
            .clone()
            .ok_or_else(|| anyhow!("signed quote is missing quoteId"))?,
        previous_quote_id: quote.previous_quote_id.clone(),
        counter_offer_amount: quote.counter_offer_amount,
        requested_amount: quote.requested_amount,
        quoted_amount: quote.quoted_amount,
        min_amount: quote.min_amount,
        max_amount: quote.max_amount,
        description_template: quote.description_template.clone(),
        warning: quote.warning.clone(),
        expires_at_unix_ms,
        issuer_did: issuer_did.to_string(),
    };
    let signature_bytes = decode_fixed_hex::<64>(signature_hex, "quote signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(&signed_quote_payload(&document)?, &signature)
        .context("quote signature verification failed")?;
    Ok(())
}

fn signed_quote_state_payload(document: &SignedQuoteStateDocument) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(document).context("failed to serialize signed quote state document")
}

fn sign_quote_state(record: &QuoteLedgerRecord) -> anyhow::Result<QuoteStateSnapshot> {
    let signing_key = quote_signing_key()?;
    let issuer_did = quote_issuer_did_from_signing_key(&signing_key);
    let document = SignedQuoteStateDocument {
        quote_id: record.quote_id.clone(),
        card_id: record.card_id.clone(),
        status: record.status,
        previous_quote_id: record.previous_quote_id.clone(),
        superseded_by_quote_id: record.superseded_by_quote_id.clone(),
        negotiation_round: record.negotiation_round,
        consumed_by_transaction_id: record.consumed_by_transaction_id,
        revoked_reason: record.revoked_reason.clone(),
        expires_at_unix_ms: record.expires_at_unix_ms,
        updated_at_unix_ms: record.updated_at_unix_ms,
        issuer_did: issuer_did.clone(),
    };
    let signature = signing_key.sign(&signed_quote_state_payload(&document)?);
    Ok(QuoteStateSnapshot {
        quote_id: document.quote_id,
        card_id: document.card_id,
        status: document.status,
        previous_quote_id: document.previous_quote_id,
        superseded_by_quote_id: document.superseded_by_quote_id,
        negotiation_round: document.negotiation_round,
        consumed_by_transaction_id: document.consumed_by_transaction_id,
        revoked_reason: document.revoked_reason,
        expires_at_unix_ms: document.expires_at_unix_ms,
        updated_at_unix_ms: document.updated_at_unix_ms,
        issuer_did,
        signature_hex: hex::encode(signature.to_bytes()),
    })
}

fn verify_signed_quote_state(
    card: &PublishedAgentCard,
    state: &QuoteStateSnapshot,
) -> anyhow::Result<()> {
    let terms = extract_ap2_terms(&card.card);
    if let Some(expected_issuer_did) = terms.quote_issuer_did.as_deref() {
        if state.issuer_did.to_ascii_lowercase() != expected_issuer_did {
            anyhow::bail!(
                "quote state issuer '{}' does not match expected issuer '{}'",
                state.issuer_did,
                expected_issuer_did
            );
        }
    }

    let verifying_key = decode_quote_verifying_key_from_did(&state.issuer_did)?;
    validate_quote_issuer_did(&state.issuer_did, &hex::encode(verifying_key.to_bytes()))?;
    let document = SignedQuoteStateDocument {
        quote_id: state.quote_id.clone(),
        card_id: state.card_id.clone(),
        status: state.status,
        previous_quote_id: state.previous_quote_id.clone(),
        superseded_by_quote_id: state.superseded_by_quote_id.clone(),
        negotiation_round: state.negotiation_round,
        consumed_by_transaction_id: state.consumed_by_transaction_id,
        revoked_reason: state.revoked_reason.clone(),
        expires_at_unix_ms: state.expires_at_unix_ms,
        updated_at_unix_ms: state.updated_at_unix_ms,
        issuer_did: state.issuer_did.clone(),
    };
    let signature_bytes = decode_fixed_hex::<64>(&state.signature_hex, "quote state signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(&signed_quote_state_payload(&document)?, &signature)
        .context("quote state signature verification failed")?;
    Ok(())
}

fn parse_remote_quote_state(
    card: &PublishedAgentCard,
    quote_id: &str,
    raw_value: Value,
) -> anyhow::Result<QuoteStateSnapshot> {
    let body = raw_value.get("state").cloned().unwrap_or(raw_value);
    let state = serde_json::from_value::<QuoteStateSnapshot>(body)
        .context("remote quote state response was not a valid QuoteStateSnapshot")?;
    if state.quote_id != quote_id {
        anyhow::bail!(
            "remote quote state quoteId '{}' does not match requested quote '{}'",
            state.quote_id,
            quote_id
        );
    }
    if state.card_id != card.card_id {
        anyhow::bail!(
            "remote quote state cardId '{}' does not match requested card '{}'",
            state.card_id,
            card.card_id
        );
    }
    verify_signed_quote_state(card, &state)?;
    Ok(state)
}

async fn fetch_remote_quote_state(
    card: &PublishedAgentCard,
    quote_id: &str,
    timeout_seconds: u64,
) -> anyhow::Result<Option<QuoteStateSnapshot>> {
    let terms = extract_ap2_terms(&card.card);
    let Some(state_url) = resolve_quote_state_url(card, &terms, quote_id) else {
        return Ok(None);
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.max(1)))
        .build()?;
    let response = client
        .get(&state_url)
        .send()
        .await
        .with_context(|| format!("failed requesting remote quote state at {state_url}"))?;
    let status = response.status();
    let raw_body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!(
            "remote quote state endpoint {} returned status {}: {}",
            state_url,
            status,
            raw_body
        );
    }
    let raw_value = if raw_body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(&raw_body)
            .with_context(|| format!("remote quote state endpoint {state_url} returned non-JSON body"))?
    };
    parse_remote_quote_state(card, quote_id, raw_value).map(Some)
}

async fn record_quote_offer(
    state: &AppState,
    card: &PublishedAgentCard,
    quote: &AgentSettlementQuote,
    source_kind: &str,
) -> anyhow::Result<Option<QuoteLedgerRecord>> {
    let Some(quote_id) = quote.quote_id.clone() else {
        return Ok(None);
    };

    let now = unix_timestamp_ms();
    let existing = get_quote_record(state, &quote_id).await?;
    if let Some(existing) = &existing {
        match existing.status {
            QuoteLedgerStatus::Consumed => {
                anyhow::bail!("quote '{}' has already been consumed", quote_id)
            }
            QuoteLedgerStatus::Revoked => anyhow::bail!("quote '{}' has been revoked", quote_id),
            QuoteLedgerStatus::Superseded => anyhow::bail!("quote '{}' has been superseded", quote_id),
            QuoteLedgerStatus::Offered => {}
        }
    }

    let previous = match quote.previous_quote_id.as_deref() {
        Some(previous_quote_id) if previous_quote_id != quote_id => {
            let Some(mut previous) = get_quote_record(state, previous_quote_id).await? else {
                anyhow::bail!("previous quote '{}' was not found in the ledger", previous_quote_id);
            };
            match previous.status {
                QuoteLedgerStatus::Consumed => anyhow::bail!(
                    "previous quote '{}' has already been consumed",
                    previous_quote_id
                ),
                QuoteLedgerStatus::Revoked => {
                    anyhow::bail!("previous quote '{}' has been revoked", previous_quote_id)
                }
                QuoteLedgerStatus::Superseded => {
                    if previous.superseded_by_quote_id.as_deref() != Some(quote_id.as_str()) {
                        anyhow::bail!(
                            "previous quote '{}' has already been superseded by another quote",
                            previous_quote_id
                        );
                    }
                }
                QuoteLedgerStatus::Offered => {
                    previous.status = QuoteLedgerStatus::Superseded;
                    previous.superseded_by_quote_id = Some(quote_id.clone());
                    previous.updated_at_unix_ms = now;
                    save_quote_record(state, &previous).await?;
                }
            }
            Some(previous)
        }
        _ => None,
    };

    let record = QuoteLedgerRecord {
        quote_id: quote_id.clone(),
        card_id: card.card_id.clone(),
        source_kind: source_kind.to_string(),
        quote_url: quote.quote_url.clone(),
        previous_quote_id: quote.previous_quote_id.clone(),
        superseded_by_quote_id: existing
            .as_ref()
            .and_then(|record| record.superseded_by_quote_id.clone()),
        negotiation_round: previous
            .as_ref()
            .map(|record| record.negotiation_round.saturating_add(1))
            .unwrap_or(0),
        settlement_supported: quote.settlement_supported,
        payment_roles: quote.payment_roles.clone(),
        currency: quote.currency.clone(),
        quote_mode: quote.quote_mode.clone(),
        requested_amount: quote.requested_amount,
        quoted_amount: quote.quoted_amount,
        counter_offer_amount: quote.counter_offer_amount,
        min_amount: quote.min_amount,
        max_amount: quote.max_amount,
        description_template: quote.description_template.clone(),
        warning: quote.warning.clone(),
        expires_at_unix_ms: quote.expires_at_unix_ms,
        issuer_did: quote.issuer_did.clone(),
        signature_hex: quote.signature_hex.clone(),
        status: QuoteLedgerStatus::Offered,
        consumed_by_transaction_id: existing
            .as_ref()
            .and_then(|record| record.consumed_by_transaction_id),
        revoked_reason: existing.as_ref().and_then(|record| record.revoked_reason.clone()),
        created_at_unix_ms: existing.as_ref().map(|record| record.created_at_unix_ms).unwrap_or(now),
        updated_at_unix_ms: now,
    };
    save_quote_record(state, &record).await?;
    Ok(Some(record))
}

async fn consume_quote_record(
    state: &AppState,
    card_id: &str,
    quote_id: Option<&str>,
    transaction_id: Uuid,
) -> anyhow::Result<Option<QuoteLedgerRecord>> {
    let Some(quote_id) = quote_id else {
        return Ok(None);
    };
    let mut record = get_quote_record(state, quote_id)
        .await?
        .ok_or_else(|| anyhow!("quote '{}' was not found in the ledger", quote_id))?;
    if record.card_id != card_id {
        anyhow::bail!(
            "quote '{}' belongs to card '{}' rather than '{}'",
            quote_id,
            record.card_id,
            card_id
        );
    }
    match record.status {
        QuoteLedgerStatus::Offered => {}
        QuoteLedgerStatus::Consumed => anyhow::bail!("quote '{}' has already been consumed", quote_id),
        QuoteLedgerStatus::Revoked => anyhow::bail!("quote '{}' has been revoked", quote_id),
        QuoteLedgerStatus::Superseded => anyhow::bail!("quote '{}' has been superseded", quote_id),
    }
    record.status = QuoteLedgerStatus::Consumed;
    record.consumed_by_transaction_id = Some(transaction_id);
    record.updated_at_unix_ms = unix_timestamp_ms();
    save_quote_record(state, &record).await?;
    Ok(Some(record))
}

async fn sync_remote_quote_state(
    state: &AppState,
    card: &PublishedAgentCard,
    quote_id: &str,
    timeout_seconds: u64,
) -> anyhow::Result<QuoteLedgerRecord> {
    let mut record = get_quote_record(state, quote_id)
        .await?
        .ok_or_else(|| anyhow!("quote '{}' was not found in the ledger", quote_id))?;
    if record.card_id != card.card_id {
        anyhow::bail!(
            "quote '{}' belongs to card '{}' rather than '{}'",
            quote_id,
            record.card_id,
            card.card_id
        );
    }
    let Some(remote_state) = fetch_remote_quote_state(card, quote_id, timeout_seconds).await? else {
        return Ok(record);
    };

    record.previous_quote_id = remote_state.previous_quote_id;
    record.superseded_by_quote_id = remote_state.superseded_by_quote_id;
    record.negotiation_round = remote_state.negotiation_round;
    record.status = remote_state.status;
    record.consumed_by_transaction_id = remote_state.consumed_by_transaction_id;
    record.revoked_reason = remote_state.revoked_reason;
    record.expires_at_unix_ms = remote_state.expires_at_unix_ms.or(record.expires_at_unix_ms);
    record.updated_at_unix_ms = remote_state.updated_at_unix_ms.max(record.updated_at_unix_ms);
    save_quote_record(state, &record).await?;
    Ok(record)
}

fn decode_fixed_hex<const N: usize>(raw: &str, label: &str) -> anyhow::Result<[u8; N]> {
    let normalized = normalize_hex(raw)?;
    let bytes = hex::decode(normalized).with_context(|| format!("{label} must be valid hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("{label} must be {} bytes long", N))
}

fn normalize_hex(raw: &str) -> anyhow::Result<String> {
    Ok(hex::encode(
        hex::decode(raw.trim()).context("value must be valid hex")?,
    ))
}

fn resolve_quote_url(card: &PublishedAgentCard, terms: &AgentPaymentTerms) -> Option<String> {
    if let Some(quote_url) = terms.quote_url.as_deref() {
        return absolutize_against_card(card, quote_url);
    }
    terms
        .quote_path
        .as_deref()
        .and_then(|quote_path| absolutize_against_card(card, quote_path))
}

fn resolve_quote_state_url(
    card: &PublishedAgentCard,
    terms: &AgentPaymentTerms,
    quote_id: &str,
) -> Option<String> {
    let template = terms.quote_state_url_template.as_deref()?.trim();
    if template.is_empty() {
        return None;
    }
    let expanded = template.replace("{quoteId}", quote_id);
    absolutize_against_card(card, &expanded)
}

fn absolutize_against_card(card: &PublishedAgentCard, raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if Url::parse(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }

    let base = card
        .card_url
        .as_deref()
        .unwrap_or(card.card.url.as_str())
        .trim();
    let base = Url::parse(base).ok()?;
    let joined = if trimmed.starts_with('/') {
        let mut origin = base;
        origin.set_path(trimmed);
        origin.set_query(None);
        origin.set_fragment(None);
        origin
    } else {
        base.join(trimmed).ok()?
    };
    Some(joined.to_string())
}

fn enrich_local_card_with_quote_url(
    card_id: &str,
    locally_hosted: bool,
    mut card: AgentCard,
) -> AgentCard {
    if !locally_hosted {
        return card;
    }
    let Ok(base_url) = Url::parse(card.url.trim()) else {
        return card;
    };
    let Some(host) = base_url.host_str() else {
        return card;
    };
    let authority = match base_url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let quote_url = format!(
        "{}://{}/api/gateway/agent-cards/{}/quote",
        base_url.scheme(),
        authority,
        card_id
    );
    let quote_state_url_template = format!(
        "{}://{}/api/gateway/agent-cards/quotes/{{quoteId}}/state",
        base_url.scheme(),
        authority
    );
    let quote_issuer_did = quote_signing_key()
        .ok()
        .map(|signing_key| quote_issuer_did_from_signing_key(&signing_key));

    for extension in &mut card.capabilities.extensions {
        if extension.uri != AP2_EXTENSION_URI {
            continue;
        }
        let params = extension.params.get_or_insert_with(|| json!({}));
        if !params.is_object() {
            *params = json!({});
        }
        let Some(root) = params.as_object_mut() else {
            continue;
        };
        if root.get("pricing").is_some() {
            let Some(pricing) = root.get_mut("pricing").and_then(Value::as_object_mut) else {
                continue;
            };
            pricing
                .entry("quoteUrl".to_string())
                .or_insert_with(|| Value::String(quote_url.clone()));
            pricing
                .entry("quoteStateUrlTemplate".to_string())
                .or_insert_with(|| Value::String(quote_state_url_template.clone()));
            if let Some(quote_issuer_did) = &quote_issuer_did {
                pricing
                    .entry("quoteIssuerDid".to_string())
                    .or_insert_with(|| Value::String(quote_issuer_did.clone()));
            }
        } else {
            root.entry("pricing".to_string())
                .or_insert_with(|| {
                    json!({
                        "quoteUrl": quote_url.clone(),
                        "quoteStateUrlTemplate": quote_state_url_template.clone(),
                        "quoteIssuerDid": quote_issuer_did.clone()
                    })
                });
        }
    }
    card
}

async fn fetch_remote_settlement_quote(
    card: &PublishedAgentCard,
    requested_amount: Option<f64>,
    description: Option<&str>,
    timeout_seconds: u64,
    allow_metadata_fallback: bool,
    quote_id: Option<&str>,
    counter_offer_amount: Option<f64>,
) -> anyhow::Result<AgentSettlementQuote> {
    let metadata_quote = build_settlement_quote(card, requested_amount, description);
    let terms = extract_ap2_terms(&card.card);
    let Some(quote_url) = resolve_quote_url(card, &terms) else {
        if allow_metadata_fallback {
            return Ok(metadata_quote);
        }
        anyhow::bail!(
            "agent card '{}' does not expose a remote quoteUrl or quotePath",
            card.card_id
        );
    };

    let method = terms
        .quote_method
        .clone()
        .unwrap_or_else(|| "GET".to_string());
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_seconds.max(1)))
        .build()?;
    let response = if method == "POST" {
        client
            .post(&quote_url)
            .json(&json!({
                "cardId": card.card_id,
                "requestedAmount": requested_amount,
                "description": description,
                "quoteId": quote_id,
                "counterOfferAmount": counter_offer_amount,
            }))
            .send()
            .await
    } else {
        client
            .get(&quote_url)
            .query(&[
                ("requestedAmount", requested_amount.map(|value| value.to_string())),
                ("description", description.map(ToString::to_string)),
                ("quoteId", quote_id.map(ToString::to_string)),
                (
                    "counterOfferAmount",
                    counter_offer_amount.map(|value| value.to_string()),
                ),
            ])
            .send()
            .await
    };

    let response = match response {
        Ok(response) => response,
        Err(error) if allow_metadata_fallback => {
            let mut fallback = metadata_quote;
            fallback.warning = Some(format!(
                "remote quote fetch failed at {} and metadata quote was used instead: {}",
                quote_url, error
            ));
            return Ok(fallback);
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed requesting remote settlement quote at {quote_url}"));
        }
    };

    let status = response.status();
    let raw_body = response.text().await?;
    if !status.is_success() {
        if allow_metadata_fallback {
            let mut fallback = metadata_quote;
            fallback.warning = Some(format!(
                "remote quote endpoint {} returned status {}; metadata quote was used instead",
                quote_url, status
            ));
            return Ok(fallback);
        }
        anyhow::bail!(
            "remote quote endpoint {} returned status {}: {}",
            quote_url,
            status,
            raw_body
        );
    }

    let raw_value = if raw_body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(&raw_body)
            .with_context(|| format!("remote quote endpoint {quote_url} returned non-JSON body"))?
    };
    parse_remote_settlement_quote(card, &quote_url, &raw_value, metadata_quote)
}

fn parse_remote_settlement_quote(
    card: &PublishedAgentCard,
    quote_url: &str,
    raw_value: &Value,
    metadata_quote: AgentSettlementQuote,
) -> anyhow::Result<AgentSettlementQuote> {
    if let Ok(mut quote) = serde_json::from_value::<AgentSettlementQuote>(raw_value.clone()) {
        if quote.signature_hex.is_some() || quote.issuer_did.is_some() {
            verify_signed_quote(card, &quote)?;
        }
        quote.card_id = card.card_id.clone();
        if quote.quote_url.is_none() {
            quote.quote_url = Some(quote_url.to_string());
        }
        if quote.signature_hex.is_none() && quote.issuer_did.is_none() {
            quote.quote_source = "remote".to_string();
        }
        return Ok(quote);
    }

    let body = raw_value.get("quote").unwrap_or(raw_value);
    let Some(body) = body.as_object() else {
        anyhow::bail!("remote quote response was not an object");
    };

    let mut quote = metadata_quote;
    quote.settlement_supported = body
        .get("settlementSupported")
        .and_then(Value::as_bool)
        .unwrap_or(quote.settlement_supported);
    quote.payment_roles = body
        .get("paymentRoles")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .map(normalize_metadata)
        .unwrap_or(quote.payment_roles);
    quote.currency = body
        .get("currency")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty())
        .or(quote.currency);
    quote.quote_mode = body
        .get("quoteMode")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or(quote.quote_mode);
    quote.quote_source = body
        .get("quoteSource")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| "remote".to_string());
    quote.requested_amount = body
        .get("requestedAmount")
        .and_then(Value::as_f64)
        .or(quote.requested_amount);
    quote.quoted_amount = body
        .get("quotedAmount")
        .and_then(Value::as_f64)
        .or(quote.quoted_amount);
    quote.min_amount = body.get("minAmount").and_then(Value::as_f64).or(quote.min_amount);
    quote.max_amount = body.get("maxAmount").and_then(Value::as_f64).or(quote.max_amount);
    quote.description_template = body
        .get("descriptionTemplate")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .or(quote.description_template);
    quote.quote_id = body
        .get("quoteId")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(quote.quote_id);
    quote.previous_quote_id = body
        .get("previousQuoteId")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(quote.previous_quote_id);
    quote.counter_offer_amount = body
        .get("counterOfferAmount")
        .and_then(Value::as_f64)
        .or(quote.counter_offer_amount);
    quote.expires_at_unix_ms = body
        .get("expiresAtUnixMs")
        .and_then(Value::as_u64)
        .map(u128::from)
        .or(quote.expires_at_unix_ms);
    quote.issuer_did = body
        .get("issuerDid")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(quote.issuer_did);
    quote.signature_hex = body
        .get("signatureHex")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or(quote.signature_hex);
    quote.warning = body
        .get("warning")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .or(quote.warning);
    quote.quote_url = Some(quote_url.to_string());
    quote.card_id = card.card_id.clone();
    if quote.signature_hex.is_some() || quote.issuer_did.is_some() {
        verify_signed_quote(card, &quote)?;
    }
    Ok(quote)
}

fn merge_unique_metadata(primary: Option<Vec<String>>, fallback: Vec<String>) -> Vec<String> {
    let mut combined = primary.unwrap_or_default();
    combined.extend(fallback);
    normalize_metadata(combined)
}

fn normalize_metadata(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().to_ascii_lowercase();
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        normalized.push(value);
    }
    normalized
}

fn matches_search(
    card: &PublishedAgentCard,
    query: &SearchAgentCardsRequest,
    published_only: bool,
) -> bool {
    if published_only && !card.published {
        return false;
    }

    if let Some(filter) = query.skill_id.as_deref() {
        if !card
            .card
            .skills
            .iter()
            .any(|skill| skill.id.eq_ignore_ascii_case(filter))
        {
            return false;
        }
    }

    if let Some(filter) = query.skill_tag.as_deref() {
        if !card.card.skills.iter().any(|skill| {
            skill
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case(filter))
        }) {
            return false;
        }
    }

    if let Some(filter) = query.region.as_deref() {
        if !contains_ci(&card.regions, filter) {
            return false;
        }
    }

    if let Some(filter) = query.language.as_deref() {
        if !contains_ci(&card.languages, filter) {
            return false;
        }
    }

    if let Some(filter) = query.model_provider.as_deref() {
        if !contains_ci(&card.model_providers, filter) {
            return false;
        }
    }

    if let Some(filter) = query.chat_platform.as_deref() {
        if !contains_ci(&card.chat_platforms, filter) {
            return false;
        }
    }

    if let Some(filter) = query.payment_role.as_deref() {
        if !contains_ci(&card.payment_roles, filter) {
            return false;
        }
    }

    if let Some(filter) = query.streaming {
        if card.card.capabilities.streaming.unwrap_or(false) != filter {
            return false;
        }
    }

    if let Some(filter) = query.push_notifications {
        if card.card.capabilities.push_notifications.unwrap_or(false) != filter {
            return false;
        }
    }

    if let Some(q) = query.q.as_deref() {
        let haystack = build_search_haystack(card);
        if !haystack.contains(&q.to_ascii_lowercase()) {
            return false;
        }
    }

    true
}

fn build_search_haystack(card: &PublishedAgentCard) -> String {
    let mut parts = vec![
        card.card_id.clone(),
        card.card.name.clone(),
        card.card.description.clone(),
        card.card.url.clone(),
        card.card.version.clone(),
    ];
    if let Some(provider) = &card.card.provider {
        parts.push(provider.organization.clone());
        parts.push(provider.url.clone());
    }
    parts.extend(card.regions.clone());
    parts.extend(card.languages.clone());
    parts.extend(card.model_providers.clone());
    parts.extend(card.chat_platforms.clone());
    parts.extend(card.payment_roles.clone());
    for skill in &card.card.skills {
        parts.push(skill.id.clone());
        parts.push(skill.name.clone());
        if let Some(description) = &skill.description {
            parts.push(description.clone());
        }
        parts.extend(skill.tags.clone());
    }
    parts.join(" ").to_ascii_lowercase()
}

fn contains_ci(values: &[String], needle: &str) -> bool {
    values
        .iter()
        .any(|value| value.eq_ignore_ascii_case(needle))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    let status = if message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("requires")
        || message.contains("cannot be empty")
        || message.contains("invalid")
        || message.contains("does not advertise")
        || message.contains("superseded")
        || message.contains("revoked")
        || message.contains("consumed")
        || message.contains("counter-offer")
        || message.contains("must match")
        || message.contains("exceeds")
        || message.contains("below")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(json!({ "error": message })))
}

fn not_found(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": message.into()
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use axum::{Json, Router, extract::Path as AxumPath, routing::{get, post}};
    use reqwest::Client;
    use serde_json::json;
    use tokio::time::{Duration, sleep};
    use uuid::Uuid;
    use wasmtime::Engine;

    use super::{
        AP2_EXTENSION_URI, AgentAuthentication, AgentCapabilities, AgentCard, AgentExtension,
        AgentSkill, AppState, InvokeAgentCardRequest, PaymentStatus, PublishAgentCardRequest,
        PublishedAgentCard, QuoteLedgerRecord, QuoteLedgerStatus, RemoteInvocationStatus,
        RemoteSettlementRequest, SearchAgentCardsRequest,
        apply_counter_offer_to_quote, build_search_haystack, build_settlement_quote, derive_card_id,
        consume_quote_record, get_quote_record,
        enrich_local_card_with_quote_url, extract_ap2_roles, extract_ap2_terms,
        extract_remote_task_id, extract_remote_task_status, fetch_remote_settlement_quote,
        get_remote_settlement_by_invocation, invoke_remote_agent_card, matches_search,
        merge_unique_metadata, publish_card_inner, quote_issuer_did_from_signing_key,
        quote_signing_key, remote_task_create_url, remote_task_detail_url, sign_local_settlement_quote,
        record_quote_offer, revoke_quote_record, sign_quote_state, validate_remote_settlement_request,
        verify_signed_quote,
    };
    use crate::{
        app_state::{StoredTask, TaskStatus, unix_timestamp_ms},
        sandbox,
    };

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-agent-card-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    fn sample_card() -> PublishedAgentCard {
        PublishedAgentCard {
            card_id: "travel-agent-1234abcd".to_string(),
            source_kind: "published".to_string(),
            card_url: None,
            published: true,
            locally_hosted: false,
            issuer_did: None,
            signature_hex: None,
            regions: vec!["china".to_string()],
            languages: vec!["zh-cn".to_string()],
            model_providers: vec!["qwen".to_string()],
            chat_platforms: vec!["wechat_official_account".to_string()],
            payment_roles: vec!["payee".to_string()],
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
            card: AgentCard {
                name: "Travel Agent".to_string(),
                description: "Books trains and hotels in China".to_string(),
                url: "https://example.com/a2a".to_string(),
                provider: None,
                version: "1.0.0".to_string(),
                documentation_url: None,
                capabilities: AgentCapabilities {
                    streaming: Some(true),
                    push_notifications: Some(true),
                    state_transition_history: Some(true),
                    extensions: vec![AgentExtension {
                        uri: AP2_EXTENSION_URI.to_string(),
                        description: None,
                        required: Some(false),
                        params: Some(json!({
                            "roles": ["payee"],
                            "pricing": {
                                "currency": "CNY",
                                "quoteMode": "flat",
                                "flatAmount": 18.5,
                                "minAmount": 10.0,
                                "maxAmount": 20.0,
                                "descriptionTemplate": "Settle travel booking"
                            }
                        })),
                    }],
                },
                authentication: AgentAuthentication::default(),
                default_input_modes: vec!["text".to_string()],
                default_output_modes: vec!["text".to_string()],
                skills: vec![AgentSkill {
                    id: "booking".to_string(),
                    name: "Booking".to_string(),
                    description: Some("Finds tickets".to_string()),
                    tags: vec!["travel".to_string(), "china".to_string()],
                    examples: vec![],
                    input_modes: vec![],
                    output_modes: vec![],
                }],
            },
        }
    }

    #[test]
    fn extracts_payment_roles_from_ap2_extension() {
        assert_eq!(
            extract_ap2_roles(&sample_card().card),
            vec!["payee".to_string()]
        );
    }

    #[test]
    fn extracts_ap2_pricing_terms_from_extension() {
        let terms = extract_ap2_terms(&sample_card().card);
        assert_eq!(terms.currency.as_deref(), Some("CNY"));
        assert_eq!(terms.quote_mode.as_deref(), Some("flat"));
        assert_eq!(terms.flat_amount, Some(18.5));
        assert_eq!(terms.min_amount, Some(10.0));
        assert_eq!(terms.max_amount, Some(20.0));
    }

    #[test]
    fn derives_stable_card_id() {
        let card_id = derive_card_id("Travel Agent", "https://example.com/a2a");
        assert!(card_id.starts_with("travel-agent-"));
        assert_eq!(card_id.len(), "travel-agent-12345678".len());
    }

    #[test]
    fn search_matches_metadata_and_text_filters() {
        let query = SearchAgentCardsRequest {
            q: Some("trains".to_string()),
            skill_id: Some("booking".to_string()),
            skill_tag: Some("travel".to_string()),
            region: Some("china".to_string()),
            language: Some("zh-cn".to_string()),
            model_provider: Some("qwen".to_string()),
            chat_platform: Some("wechat_official_account".to_string()),
            payment_role: Some("payee".to_string()),
            streaming: Some(true),
            push_notifications: Some(true),
            published_only: Some(true),
        };

        let card = sample_card();
        assert!(matches_search(&card, &query, true));
        assert!(build_search_haystack(&card).contains("travel"));
    }

    #[test]
    fn search_rejects_unpublished_card_when_requested() {
        let mut card = sample_card();
        card.published = false;
        let query = SearchAgentCardsRequest {
            q: None,
            skill_id: None,
            skill_tag: None,
            region: None,
            language: None,
            model_provider: None,
            chat_platform: None,
            payment_role: None,
            streaming: None,
            push_notifications: None,
            published_only: Some(true),
        };

        assert!(!matches_search(&card, &query, true));
    }

    #[test]
    fn merge_unique_metadata_preserves_order() {
        assert_eq!(
            merge_unique_metadata(Some(vec!["payer".to_string()]), vec!["payee".to_string()]),
            vec!["payer".to_string(), "payee".to_string()]
        );
    }

    #[test]
    fn derives_remote_task_urls() {
        assert_eq!(
            remote_task_create_url("https://agent.example.com/api/a2a"),
            "https://agent.example.com/api/a2a/task"
        );
        assert_eq!(
            remote_task_create_url("https://agent.example.com/api/a2a/task"),
            "https://agent.example.com/api/a2a/task"
        );
        assert_eq!(
            remote_task_detail_url("https://agent.example.com/api/a2a", "abc"),
            "https://agent.example.com/api/a2a/task/abc"
        );
    }

    #[test]
    fn extracts_remote_task_id_and_status() {
        let payload = json!({
            "task": {
                "taskId": "remote-123",
                "status": "completed"
            }
        });

        assert_eq!(
            extract_remote_task_id(&payload).as_deref(),
            Some("remote-123")
        );
        assert_eq!(
            extract_remote_task_status(&payload).as_deref(),
            Some("completed")
        );
    }

    #[test]
    fn builds_settlement_quote_from_agent_card_pricing() {
        let quote = build_settlement_quote(&sample_card(), Some(12.0), None);
        assert!(quote.settlement_supported);
        assert_eq!(quote.currency.as_deref(), Some("CNY"));
        assert_eq!(quote.quote_mode, "flat");
        assert_eq!(quote.quote_source, "metadata");
        assert_eq!(quote.quoted_amount, Some(18.5));
        assert!(quote.warning.is_some());
    }

    #[test]
    fn signs_locally_hosted_quotes_with_expiry_and_signature() {
        let mut card = sample_card();
        card.locally_hosted = true;
        card.card = enrich_local_card_with_quote_url(&card.card_id, true, card.card.clone());
        let quote = sign_local_settlement_quote(
            &card,
            build_settlement_quote(&card, Some(18.5), Some("Settle travel booking")),
        )
        .unwrap();
        assert!(quote.quote_id.is_some());
        assert!(quote.expires_at_unix_ms.is_some());
        assert!(quote.signature_hex.is_some());
        assert!(quote.issuer_did.is_some());
        verify_signed_quote(&card, &quote).unwrap();
    }

    #[test]
    fn enriches_locally_hosted_card_with_quote_url() {
        let card = enrich_local_card_with_quote_url(
            "travel-agent-1234abcd",
            true,
            sample_card().card,
        );
        let terms = extract_ap2_terms(&card);
        let expected_quote_issuer_did =
            quote_issuer_did_from_signing_key(&quote_signing_key().unwrap());
        assert_eq!(
            terms.quote_url.as_deref(),
            Some("https://example.com/api/gateway/agent-cards/travel-agent-1234abcd/quote")
        );
        assert_eq!(
            terms.quote_state_url_template.as_deref(),
            Some("https://example.com/api/gateway/agent-cards/quotes/{quoteId}/state")
        );
        assert_eq!(
            terms.quote_issuer_did.as_deref(),
            Some(expected_quote_issuer_did.as_str())
        );
    }

    #[test]
    fn accepts_counter_offer_for_manual_quotes() {
        let mut card = sample_card();
        card.card.capabilities.extensions[0].params = Some(json!({
            "roles": ["payee"],
            "pricing": {
                "currency": "CNY",
                "quoteMode": "manual",
                "minAmount": 10.0,
                "maxAmount": 20.0
            }
        }));
        let quote = apply_counter_offer_to_quote(
            build_settlement_quote(&card, Some(12.0), Some("Settle travel booking")),
            Some(13.0),
            Some("quote-prev"),
        );
        assert_eq!(quote.quote_mode, "counter_offer");
        assert_eq!(quote.counter_offer_amount, Some(13.0));
        assert_eq!(quote.quoted_amount, Some(13.0));
        assert_eq!(quote.previous_quote_id.as_deref(), Some("quote-prev"));
        assert!(quote.warning.is_none());
    }

    #[tokio::test]
    async fn rejects_settlement_over_agent_card_cap() {
        let (state, path) = test_state().await.unwrap();
        let card = sample_card();
        let error = validate_remote_settlement_request(
            state.as_ref(),
            &card,
            &RemoteSettlementRequest {
                mandate_id: Uuid::new_v4(),
                amount: 25.0,
                description: "too much".to_string(),
                quote_id: None,
                counter_offer_amount: None,
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("maxAmount"));
        drop(state);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn records_counter_offer_quotes_as_superseded_rounds() {
        let (state, path) = test_state().await.unwrap();
        let mut card = sample_card();
        card.card.capabilities.extensions[0].params = Some(json!({
            "roles": ["payee"],
            "pricing": {
                "currency": "CNY",
                "quoteMode": "manual",
                "minAmount": 10.0,
                "maxAmount": 20.0
            }
        }));

        let first_quote = sign_local_settlement_quote(
            &card,
            build_settlement_quote(&card, Some(12.0), Some("Settle travel booking")),
        )
        .unwrap();
        record_quote_offer(state.as_ref(), &card, &first_quote, "local")
            .await
            .unwrap();

        let counter_quote = sign_local_settlement_quote(
            &card,
            apply_counter_offer_to_quote(
                build_settlement_quote(&card, Some(12.0), Some("Settle travel booking")),
                Some(13.0),
                first_quote.quote_id.as_deref(),
            ),
        )
        .unwrap();
        record_quote_offer(state.as_ref(), &card, &counter_quote, "local")
            .await
            .unwrap();

        let previous = get_quote_record(
            state.as_ref(),
            first_quote.quote_id.as_deref().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        let current = get_quote_record(
            state.as_ref(),
            counter_quote.quote_id.as_deref().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(previous.status.as_db(), "superseded");
        assert_eq!(
            previous.superseded_by_quote_id.as_deref(),
            counter_quote.quote_id.as_deref()
        );
        assert_eq!(previous.negotiation_round, 0);
        assert_eq!(current.status.as_db(), "offered");
        assert_eq!(current.previous_quote_id, first_quote.quote_id);
        assert_eq!(current.negotiation_round, 1);

        drop(state);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn revokes_quotes_and_blocks_consumption() {
        let (state, path) = test_state().await.unwrap();
        let card = sample_card();
        let quote = sign_local_settlement_quote(
            &card,
            build_settlement_quote(&card, Some(18.5), Some("Settle travel booking")),
        )
        .unwrap();
        record_quote_offer(state.as_ref(), &card, &quote, "local")
            .await
            .unwrap();

        let revoked = revoke_quote_record(
            state.as_ref(),
            quote.quote_id.as_deref().unwrap(),
            Some("quote expired during checkout"),
        )
        .await
        .unwrap();
        assert_eq!(revoked.status.as_db(), "revoked");
        assert_eq!(
            revoked.revoked_reason.as_deref(),
            Some("quote expired during checkout")
        );

        let error = consume_quote_record(
            state.as_ref(),
            &card.card_id,
            quote.quote_id.as_deref(),
            Uuid::new_v4(),
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("revoked"), "{}", message);

        drop(state);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn consumes_quotes_once_for_replay_protection() {
        let (state, path) = test_state().await.unwrap();
        let card = sample_card();
        let quote = sign_local_settlement_quote(
            &card,
            build_settlement_quote(&card, Some(18.5), Some("Settle travel booking")),
        )
        .unwrap();
        record_quote_offer(state.as_ref(), &card, &quote, "local")
            .await
            .unwrap();

        let transaction_id = Uuid::new_v4();
        let consumed = consume_quote_record(
            state.as_ref(),
            &card.card_id,
            quote.quote_id.as_deref(),
            transaction_id,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(consumed.status.as_db(), "consumed");
        assert_eq!(consumed.consumed_by_transaction_id, Some(transaction_id));

        let replay_error = consume_quote_record(
            state.as_ref(),
            &card.card_id,
            quote.quote_id.as_deref(),
            Uuid::new_v4(),
        )
        .await
        .unwrap_err();
        assert!(replay_error.to_string().contains("already been consumed"));

        drop(state);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn fetches_remote_quote_from_declared_quote_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/quotes/booking",
                get(|| async {
                    Json(json!({
                        "settlementSupported": true,
                        "paymentRoles": ["payee"],
                        "currency": "CNY",
                        "quoteMode": "negotiated",
                        "requestedAmount": 12.0,
                        "quotedAmount": 13.2,
                        "minAmount": 10.0,
                        "maxAmount": 15.0,
                        "descriptionTemplate": "Remote negotiated travel quote"
                    }))
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        sleep(Duration::from_millis(50)).await;

        let mut card = sample_card();
        card.card.url = format!("http://{address}/a2a");
        card.card.capabilities.extensions[0].params = Some(json!({
            "roles": ["payee"],
            "pricing": {
                "currency": "CNY",
                "quoteMode": "negotiated",
                "quoteUrl": format!("http://{address}/quotes/booking")
            }
        }));

        let expected_quote_url = format!("http://{address}/quotes/booking");
        let quote = fetch_remote_settlement_quote(
            &card,
            Some(12.0),
            Some("Book train"),
            5,
            false,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(quote.quote_source, "remote");
        assert_eq!(quote.quote_url.as_deref(), Some(expected_quote_url.as_str()));
        assert_eq!(quote.quoted_amount, Some(13.2));
        assert_eq!(quote.max_amount, Some(15.0));

        server.abort();
    }

    #[tokio::test]
    async fn fetches_and_verifies_signed_remote_quote() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut card = sample_card();
        card.card.url = format!("http://{address}/a2a");
        card.card.capabilities.extensions[0].params = Some(json!({
            "roles": ["payee"],
            "pricing": {
                "currency": "CNY",
                "quoteMode": "negotiated",
                "quoteUrl": format!("http://{address}/quotes/booking"),
                "quoteIssuerDid": quote_issuer_did_from_signing_key(&quote_signing_key().unwrap())
            }
        }));
        let signed_quote = sign_local_settlement_quote(
            &card,
            apply_counter_offer_to_quote(
                build_settlement_quote(&card, Some(12.0), Some("Book train")),
                Some(13.2),
                None,
            ),
        )
        .unwrap();
        let expected_quote = signed_quote.clone();

        let server = tokio::spawn(async move {
            let quote = signed_quote.clone();
            let app = Router::new().route(
                "/quotes/booking",
                get(move || {
                    let quote = quote.clone();
                    async move { Json(quote) }
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        sleep(Duration::from_millis(50)).await;

        let quote = fetch_remote_settlement_quote(
            &card,
            Some(12.0),
            Some("Book train"),
            5,
            false,
            Some("quote-prev"),
            Some(13.2),
        )
        .await
        .unwrap();
        assert_eq!(quote.quote_id, expected_quote.quote_id);
        assert_eq!(quote.counter_offer_amount, Some(13.2));
        assert_eq!(quote.quoted_amount, Some(13.2));
        assert!(quote.signature_hex.is_some());

        server.abort();
    }

    #[tokio::test]
    async fn rejects_remote_quote_when_state_sync_reports_revocation() {
        let (state, path) = test_state().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut card = sample_card();
        card.card.url = format!("http://{address}/a2a");
        card.card.capabilities.extensions[0].params = Some(json!({
            "roles": ["payee"],
            "pricing": {
                "currency": "CNY",
                "quoteMode": "negotiated",
                "quoteUrl": format!("http://{address}/quotes/booking"),
                "quoteStateUrlTemplate": format!("http://{address}/quotes/{{quoteId}}/state"),
                "quoteIssuerDid": quote_issuer_did_from_signing_key(&quote_signing_key().unwrap())
            }
        }));
        let signed_quote = sign_local_settlement_quote(
            &card,
            apply_counter_offer_to_quote(
                build_settlement_quote(&card, Some(12.0), Some("Book train")),
                Some(13.2),
                None,
            ),
        )
        .unwrap();
        let revoked_state = sign_quote_state(&QuoteLedgerRecord {
            quote_id: signed_quote.quote_id.clone().unwrap(),
            card_id: card.card_id.clone(),
            source_kind: "local".to_string(),
            quote_url: signed_quote.quote_url.clone(),
            previous_quote_id: signed_quote.previous_quote_id.clone(),
            superseded_by_quote_id: None,
            negotiation_round: 1,
            settlement_supported: true,
            payment_roles: signed_quote.payment_roles.clone(),
            currency: signed_quote.currency.clone(),
            quote_mode: signed_quote.quote_mode.clone(),
            requested_amount: signed_quote.requested_amount,
            quoted_amount: signed_quote.quoted_amount,
            counter_offer_amount: signed_quote.counter_offer_amount,
            min_amount: signed_quote.min_amount,
            max_amount: signed_quote.max_amount,
            description_template: signed_quote.description_template.clone(),
            warning: None,
            expires_at_unix_ms: signed_quote.expires_at_unix_ms,
            issuer_did: signed_quote.issuer_did.clone(),
            signature_hex: signed_quote.signature_hex.clone(),
            status: QuoteLedgerStatus::Revoked,
            consumed_by_transaction_id: None,
            revoked_reason: Some("merchant cancelled quote".to_string()),
            created_at_unix_ms: unix_timestamp_ms(),
            updated_at_unix_ms: unix_timestamp_ms(),
        })
        .unwrap();
        let expected_quote_id = signed_quote.quote_id.clone();
        let server_quote = signed_quote.clone();

        let server = tokio::spawn(async move {
            let quote = server_quote.clone();
            let state_snapshot = revoked_state.clone();
            let app = Router::new()
                .route(
                    "/quotes/booking",
                    get(move || {
                        let quote = quote.clone();
                        async move { Json(quote) }
                    }),
                )
                .route(
                    "/quotes/:quote_id/state",
                    get(move |AxumPath(_quote_id): AxumPath<String>| {
                        let state_snapshot = state_snapshot.clone();
                        async move { Json(state_snapshot) }
                    }),
                );
            let _ = axum::serve(listener, app).await;
        });
        sleep(Duration::from_millis(50)).await;

        let error = validate_remote_settlement_request(
            state.as_ref(),
            &card,
            &RemoteSettlementRequest {
                mandate_id: Uuid::new_v4(),
                amount: 13.2,
                description: "Book train".to_string(),
                quote_id: expected_quote_id,
                counter_offer_amount: None,
            },
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("revoked"), "{}", message);

        server.abort();
        drop(state);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn invoke_remote_agent_card_creates_pending_ap2_settlement() {
        let (state, db_path) = test_state().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/task",
                post(|| async {
                    Json(json!({
                        "task": {
                            "taskId": "remote-123",
                            "status": "completed"
                        }
                    }))
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        sleep(Duration::from_millis(50)).await;
        let probe = Client::new()
            .post(format!("http://{address}/task"))
            .json(&json!({"name":"probe"}))
            .send()
            .await
            .unwrap();
        assert!(probe.status().is_success());
        let probe_payload: serde_json::Value = probe.json().await.unwrap();
        assert_eq!(probe_payload["task"]["taskId"], "remote-123");

        let mut card = sample_card();
        card.card.url = format!("http://{address}");
        publish_card_inner(
            &state,
            PublishAgentCardRequest {
                card_id: Some(card.card_id.clone()),
                card: card.card.clone(),
                regions: Some(card.regions.clone()),
                languages: Some(card.languages.clone()),
                model_providers: Some(card.model_providers.clone()),
                chat_platforms: Some(card.chat_platforms.clone()),
                payment_roles: Some(card.payment_roles.clone()),
                locally_hosted: Some(false),
                published: Some(true),
                issuer_did: None,
                signature_hex: None,
            },
        )
        .await
        .unwrap();

        let task_id = Uuid::new_v4();
        state
            .insert_task(StoredTask {
                task_id,
                parent_task_id: None,
                name: "delegate booking".to_string(),
                instruction: "remote".to_string(),
                status: TaskStatus::Accepted,
                linked_payment_id: None,
                last_update_reason: "seed test task".to_string(),
                created_at_unix_ms: unix_timestamp_ms(),
                updated_at_unix_ms: unix_timestamp_ms(),
            })
            .await
            .unwrap();

        let response = invoke_remote_agent_card(
            &state,
            &card.card_id,
            InvokeAgentCardRequest {
                name: "delegate booking".to_string(),
                instruction: "Book train to Shanghai".to_string(),
                parent_task_id: Some(task_id),
                await_completion: Some(true),
                timeout_seconds: Some(5),
                poll_interval_ms: Some(100),
                settlement: Some(RemoteSettlementRequest {
                    mandate_id: Uuid::new_v4(),
                    amount: 18.5,
                    description: "Settle remote booking".to_string(),
                    quote_id: None,
                    counter_offer_amount: None,
                }),
            },
            Some(task_id),
        )
        .await
        .unwrap();

        let settlement = response.settlement.expect("settlement should be created");
        assert_eq!(response.invocation.status, RemoteInvocationStatus::Completed);
        assert_eq!(settlement.status, PaymentStatus::PendingPhysicalAuth);
        assert_eq!(settlement.local_task_id, Some(task_id));
        assert_eq!(settlement.remote_task_id.as_deref(), Some("remote-123"));

        let stored_settlement = get_remote_settlement_by_invocation(&state, response.invocation.invocation_id)
            .await
            .unwrap()
            .expect("settlement should persist");
        assert_eq!(stored_settlement.transaction_id, settlement.transaction_id);

        let payment = state
            .get_payment(settlement.transaction_id)
            .await
            .unwrap()
            .expect("payment should persist");
        assert_eq!(payment.status, PaymentStatus::PendingPhysicalAuth);

        let task = state.get_task(task_id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::WaitingPaymentAuthorization);
        assert_eq!(task.linked_payment_id, Some(settlement.transaction_id));

        server.abort();
        fs::remove_file(db_path).ok();
    }
}
