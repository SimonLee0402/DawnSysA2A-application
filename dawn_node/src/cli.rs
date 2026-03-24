use std::{
    collections::BTreeSet,
    io::{self, IsTerminal, Write},
    process::Stdio,
};

use anyhow::{Context, anyhow, bail};
use base64::Engine as _;
use clap::{Args, Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::Digest;
use std::{collections::BTreeMap, env, fs, path::PathBuf, process::Command as StdCommand};

use crate::profile::{
    DawnCliProfile, default_gateway_base_url, load_profile_or_default, normalize_http_base_url,
    profile_path, save_profile,
};

pub enum CliOutcome {
    Exit,
    RunNode,
}

enum StartupAction {
    Start,
    Setup,
    Status,
    Exit,
}

const DEFAULT_BOOTSTRAP_TOKEN: &str = "dawn-dev-bootstrap";
const DEFAULT_OPERATOR_NAME: &str = "desktop-operator";
const DEFAULT_REGION: &str = "global";
const DEFAULT_WORKSPACE_DISPLAY_NAME: &str = "Dawn Agent Commerce";
const DEFAULT_WORKSPACE_TENANT_ID: &str = "dawn-labs";
const DEFAULT_WORKSPACE_PROJECT_ID: &str = "agent-commerce";

#[derive(Parser)]
#[command(
    name = "dawn-node",
    version,
    about = "Desktop CLI for Dawn onboarding, connector setup, marketplace skills, and local node control."
)]
struct DawnCli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Start(StartArgs),
    Run,
    Login(LoginArgs),
    Onboard(OnboardArgs),
    Setup(SetupArgs),
    Doctor(DoctorArgs),
    Logout,
    Status,
    Connectors(ConnectorsArgs),
    Gateway(GatewayArgs),
    Secrets(SecretsArgs),
    Models(ModelArgs),
    Channels(ChannelArgs),
    Ingress(IngressArgs),
    Skills(SkillsArgs),
    Agents(AgentsArgs),
    Delegate(DelegateArgs),
    Chat(ChatArgs),
    Tasks(TasksArgs),
    Ap2(Ap2Args),
    Approvals(ApprovalsArgs),
    Node(NodeArgs),
    #[command(name = "node-command")]
    NodeCommands(NodeCommandOps),
}

#[derive(Args, Clone, Default)]
struct StartArgs {
    #[arg(long)]
    skip_gateway: bool,
    #[arg(long)]
    app: bool,
    #[arg(long)]
    release: bool,
}

#[derive(Args)]
struct LoginArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long, default_value = DEFAULT_BOOTSTRAP_TOKEN)]
    bootstrap_token: String,
    #[arg(long)]
    operator_name: Option<String>,
    #[arg(long)]
    model: Vec<String>,
    #[arg(long)]
    channel: Vec<String>,
    #[arg(long)]
    skill: Vec<String>,
    #[arg(long)]
    federated_skills: bool,
    #[arg(long)]
    allow_unsigned_skills: bool,
    #[arg(long)]
    allow_shell: bool,
    #[arg(long)]
    no_claim: bool,
    #[arg(long)]
    session_only: bool,
    #[arg(long)]
    yes: bool,
    #[arg(long)]
    advanced: bool,
    #[arg(long)]
    env: Vec<String>,
}

#[derive(Args)]
struct OnboardArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long, default_value = "dawn-dev-bootstrap")]
    bootstrap_token: String,
    #[arg(long, default_value = "desktop-operator")]
    operator_name: String,
    #[arg(long)]
    model: Vec<String>,
    #[arg(long)]
    channel: Vec<String>,
    #[arg(long)]
    tenant_id: Option<String>,
    #[arg(long)]
    project_id: Option<String>,
    #[arg(long)]
    display_name: Option<String>,
    #[arg(long)]
    region: Option<String>,
    #[arg(long)]
    node_id: Option<String>,
    #[arg(long)]
    node_name: Option<String>,
    #[arg(long)]
    allow_shell: bool,
    #[arg(long)]
    no_claim: bool,
}

#[derive(Args)]
struct SetupArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long, default_value = DEFAULT_BOOTSTRAP_TOKEN)]
    bootstrap_token: String,
    #[arg(long)]
    operator_name: Option<String>,
    #[arg(long)]
    display_name: Option<String>,
    #[arg(long)]
    tenant_id: Option<String>,
    #[arg(long)]
    project_id: Option<String>,
    #[arg(long)]
    region: Option<String>,
    #[arg(long)]
    model: Vec<String>,
    #[arg(long)]
    channel: Vec<String>,
    #[arg(long)]
    skill: Vec<String>,
    #[arg(long)]
    federated_skills: bool,
    #[arg(long)]
    allow_unsigned_skills: bool,
    #[arg(long)]
    allow_shell: bool,
    #[arg(long)]
    no_claim: bool,
    #[arg(long)]
    yes: bool,
    #[arg(long)]
    advanced: bool,
    #[arg(long)]
    env: Vec<String>,
}

#[derive(Args)]
struct DoctorArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    deep: bool,
}

#[derive(Args)]
struct ConnectorsArgs {
    #[command(subcommand)]
    command: ConnectorsCommand,
}

#[derive(Args)]
struct GatewayArgs {
    #[command(subcommand)]
    command: GatewayCommand,
}

#[derive(Subcommand)]
enum GatewayCommand {
    Env(SecretsExportArgs),
    Start(GatewayStartArgs),
}

#[derive(Args)]
struct GatewayStartArgs {
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long)]
    release: bool,
}

#[derive(Args)]
struct SecretsArgs {
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand)]
enum SecretsCommand {
    List,
    Set { key: String, value: String },
    Unset { key: String },
    Export(SecretsExportArgs),
}

#[derive(Subcommand)]
enum ConnectorsCommand {
    Status {
        #[arg(long)]
        gateway: Option<String>,
    },
    Verify {
        surface: String,
        target: String,
        #[arg(long)]
        gateway: Option<String>,
    },
}

#[derive(Args)]
struct ModelArgs {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Args)]
struct IngressArgs {
    #[command(subcommand)]
    command: IngressCommand,
}

#[derive(Subcommand)]
enum IngressCommand {
    Connect(IngressConnectArgs),
    Status {
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Verify {
        target: String,
        #[arg(long)]
        gateway: Option<String>,
    },
}

#[derive(Args)]
struct IngressConnectArgs {
    target: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    secret: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    dm_policy: Option<String>,
    #[arg(long, value_delimiter = ',')]
    allow_from: Vec<String>,
    #[arg(long)]
    env: Vec<String>,
}

#[derive(Args)]
struct ChannelArgs {
    #[command(subcommand)]
    command: ChannelCommand,
}

#[derive(Subcommand)]
enum ModelCommand {
    List {
        #[arg(long)]
        gateway: Option<String>,
    },
    #[command(name = "auth-login")]
    AuthLogin {
        provider: String,
    },
    #[command(name = "auth-status")]
    AuthStatus {
        provider: Option<String>,
    },
    #[command(name = "auth-logout")]
    AuthLogout {
        provider: String,
    },
    Connect(ConnectorConnectArgs),
    Test(ModelTestArgs),
    Add {
        values: Vec<String>,
        #[arg(long)]
        gateway: Option<String>,
    },
    Remove {
        values: Vec<String>,
        #[arg(long)]
        gateway: Option<String>,
    },
}

#[derive(Subcommand)]
enum ChannelCommand {
    List {
        #[arg(long)]
        gateway: Option<String>,
    },
    Connect(ConnectorConnectArgs),
    Send(ChannelSendArgs),
    Pairings(ChannelPairingArgs),
    Add {
        values: Vec<String>,
        #[arg(long)]
        gateway: Option<String>,
    },
    Remove {
        values: Vec<String>,
        #[arg(long)]
        gateway: Option<String>,
    },
}

#[derive(Args)]
struct ConnectorConnectArgs {
    target: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    access_token: Option<String>,
    #[arg(long)]
    webhook_url: Option<String>,
    #[arg(long)]
    app_id: Option<String>,
    #[arg(long)]
    app_secret: Option<String>,
    #[arg(long)]
    client_secret: Option<String>,
    #[arg(long)]
    endpoint_id: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    env: Vec<String>,
}

#[derive(Args)]
struct ModelTestArgs {
    target: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long, default_value = "Respond with exactly: OK")]
    input: String,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    instructions: Option<String>,
}

#[derive(Args)]
struct ChannelSendArgs {
    target: String,
    text: Option<String>,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    chat_id: Option<String>,
    #[arg(long)]
    account_key: Option<String>,
    #[arg(long)]
    attachment_file: Option<String>,
    #[arg(long)]
    attachment_name: Option<String>,
    #[arg(long)]
    attachment_content_type: Option<String>,
    #[arg(long)]
    reaction: Option<String>,
    #[arg(long)]
    target_message_id: Option<String>,
    #[arg(long)]
    target_author: Option<String>,
    #[arg(long)]
    remove_reaction: bool,
    #[arg(long)]
    receipt_type: Option<String>,
    #[arg(long)]
    typing: Option<String>,
    #[arg(long)]
    mark_read: bool,
    #[arg(long)]
    mark_unread: bool,
    #[arg(long)]
    part_index: Option<i64>,
    #[arg(long)]
    effect_id: Option<String>,
    #[arg(long)]
    edit_message_id: Option<String>,
    #[arg(long)]
    edited_text: Option<String>,
    #[arg(long)]
    unsend_message_id: Option<String>,
    #[arg(long)]
    participant_action: Option<String>,
    #[arg(long)]
    participant_address: Option<String>,
    #[arg(long)]
    group_action: Option<String>,
    #[arg(long)]
    group_id: Option<String>,
    #[arg(long)]
    group_name: Option<String>,
    #[arg(long)]
    group_description: Option<String>,
    #[arg(long)]
    group_link_mode: Option<String>,
    #[arg(long = "group-member")]
    group_members: Vec<String>,
    #[arg(long = "group-admin")]
    group_admins: Vec<String>,
    #[arg(long)]
    parse_mode: Option<String>,
    #[arg(long)]
    disable_notification: bool,
    #[arg(long)]
    target_type: Option<String>,
    #[arg(long)]
    event_id: Option<String>,
    #[arg(long)]
    msg_id: Option<String>,
    #[arg(long)]
    msg_seq: Option<i64>,
    #[arg(long)]
    is_wakeup: bool,
}

#[derive(Args)]
struct ChannelPairingArgs {
    #[command(subcommand)]
    command: ChannelPairingCommand,
}

#[derive(Subcommand)]
enum ChannelPairingCommand {
    List {
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Approve {
        platform: String,
        identity_key: String,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long, default_value = "desktop-operator")]
        actor: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Reject {
        platform: String,
        identity_key: String,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long, default_value = "desktop-operator")]
        actor: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Args)]
struct SecretsExportArgs {
    #[arg(long, default_value = "dotenv")]
    format: String,
    #[arg(long)]
    path: Option<String>,
}

#[derive(Args)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Subcommand)]
enum SkillCommand {
    Search(SkillSearchArgs),
    Install(SkillInstallArgs),
}

#[derive(Args)]
struct AgentsArgs {
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Args)]
struct DelegateArgs {
    card_id: String,
    instruction: String,
    #[arg(long, default_value = "delegate-task")]
    name: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    await_completion: bool,
    #[arg(long)]
    timeout_seconds: Option<u64>,
    #[arg(long)]
    mandate_id: Option<String>,
    #[arg(long)]
    amount: Option<f64>,
    #[arg(long)]
    settlement_description: Option<String>,
    #[arg(long)]
    quote_id: Option<String>,
    #[arg(long)]
    counter_offer_amount: Option<f64>,
    #[arg(long)]
    remote_quote: bool,
    #[arg(long)]
    print_quote: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ChatArgs {
    target: String,
    card_id: String,
    instruction: String,
    #[arg(long, default_value = "chat-task")]
    name: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    timeout_seconds: Option<u64>,
    #[arg(long)]
    chat_id: Option<String>,
    #[arg(long)]
    parse_mode: Option<String>,
    #[arg(long)]
    disable_notification: bool,
    #[arg(long)]
    target_type: Option<String>,
    #[arg(long)]
    event_id: Option<String>,
    #[arg(long)]
    msg_id: Option<String>,
    #[arg(long)]
    msg_seq: Option<i64>,
    #[arg(long)]
    is_wakeup: bool,
    #[arg(long)]
    mandate_id: Option<String>,
    #[arg(long)]
    amount: Option<f64>,
    #[arg(long)]
    settlement_description: Option<String>,
    #[arg(long)]
    quote_id: Option<String>,
    #[arg(long)]
    counter_offer_amount: Option<f64>,
    #[arg(long)]
    remote_quote: bool,
    #[arg(long)]
    print_quote: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum AgentCommand {
    Search(AgentSearchArgs),
    Install(AgentInstallArgs),
    Quote(AgentQuoteArgs),
    Invoke(AgentInvokeArgs),
}

#[derive(Args)]
struct TasksArgs {
    #[command(subcommand)]
    command: TaskCommand,
}

#[derive(Subcommand)]
enum TaskCommand {
    List {
        #[arg(long)]
        gateway: Option<String>,
    },
    Create {
        name: String,
        instruction: String,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        parent_task_id: Option<String>,
    },
}

#[derive(Args)]
struct Ap2Args {
    #[command(subcommand)]
    command: Ap2Command,
}

#[derive(Subcommand)]
enum Ap2Command {
    List {
        #[arg(long)]
        gateway: Option<String>,
    },
    Prepare(Ap2PrepareArgs),
    Sign(Ap2SignArgs),
    Signer(Ap2SignerArgs),
    Request {
        mandate_id: String,
        amount: f64,
        description: String,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        task_id: Option<String>,
    },
    Approve(Ap2ApproveArgs),
    #[command(name = "approve-local", visible_alias = "approve-signed")]
    ApproveLocal(Ap2ApproveLocalArgs),
    Reject(Ap2RejectArgs),
}

#[derive(Args)]
struct ApprovalsArgs {
    #[command(subcommand)]
    command: ApprovalCommand,
}

#[derive(Subcommand)]
enum ApprovalCommand {
    List {
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    Decide(ApprovalDecideArgs),
}

#[derive(Args, Clone)]
struct SkillSearchArgs {
    query: Option<String>,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    federated: bool,
    #[arg(long)]
    all: bool,
}

#[derive(Args)]
struct SkillInstallArgs {
    skill_id: String,
    #[arg(long)]
    version: Option<String>,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    federated: bool,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    allow_unsigned: bool,
    #[arg(long)]
    no_activate: bool,
}

#[derive(Args, Clone)]
struct AgentSearchArgs {
    query: Option<String>,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    federated: bool,
    #[arg(long)]
    all: bool,
}

#[derive(Args)]
struct AgentInstallArgs {
    card_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    federated: bool,
    #[arg(long)]
    all: bool,
}

#[derive(Args)]
struct AgentQuoteArgs {
    card_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    amount: Option<f64>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    remote: bool,
    #[arg(long)]
    quote_id: Option<String>,
    #[arg(long)]
    counter_offer_amount: Option<f64>,
}

#[derive(Args)]
struct AgentInvokeArgs {
    card_id: String,
    name: String,
    instruction: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    await_completion: bool,
    #[arg(long)]
    timeout_seconds: Option<u64>,
    #[arg(long)]
    mandate_id: Option<String>,
    #[arg(long)]
    settlement_amount: Option<f64>,
    #[arg(long)]
    settlement_description: Option<String>,
    #[arg(long)]
    quote_id: Option<String>,
}

#[derive(Args)]
struct ApprovalDecideArgs {
    approval_id: String,
    decision: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    mcu_public_did: Option<String>,
    #[arg(long)]
    mcu_signature: Option<String>,
}

#[derive(Args)]
struct Ap2ApproveArgs {
    transaction_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    mcu_public_did: String,
    #[arg(long)]
    mcu_signature: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct Ap2PrepareArgs {
    transaction_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct Ap2SignArgs {
    transaction_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    seed_hex: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct Ap2ApproveLocalArgs {
    transaction_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    seed_hex: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct Ap2SignerArgs {
    #[command(subcommand)]
    command: Ap2SignerCommand,
}

#[derive(Subcommand)]
enum Ap2SignerCommand {
    Status,
    Local {
        #[arg(long)]
        seed_hex: Option<String>,
    },
    Serial(Ap2SignerSerialArgs),
    Clear,
}

#[derive(Args)]
struct Ap2SignerSerialArgs {
    port: String,
    #[arg(long, default_value_t = 115200)]
    baud: u32,
    #[arg(long, default_value = "dawn-ap2-v1")]
    protocol: String,
    #[arg(long)]
    mock_seed_hex: Option<String>,
}

#[derive(Args)]
struct Ap2RejectArgs {
    transaction_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct NodeArgs {
    #[command(subcommand)]
    command: NodeCommand,
}

#[derive(Subcommand)]
enum NodeCommand {
    Claim(NodeClaimArgs),
    #[command(name = "trust-self")]
    TrustSelf(NodeTrustSelfArgs),
}

#[derive(Args)]
struct NodeCommandOps {
    #[command(subcommand)]
    command: NodeCommandAction,
}

#[derive(Subcommand)]
enum NodeCommandAction {
    Approve(NodeCommandApproveArgs),
    Reject(NodeCommandRejectArgs),
}

#[derive(Args)]
struct NodeCommandApproveArgs {
    command_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct NodeCommandRejectArgs {
    command_id: String,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct NodeClaimArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    node_id: Option<String>,
    #[arg(long)]
    display_name: Option<String>,
    #[arg(long)]
    capability: Vec<String>,
    #[arg(long)]
    allow_shell: bool,
    #[arg(long, default_value_t = 1800)]
    expires_seconds: u64,
}

#[derive(Args)]
struct NodeTrustSelfArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    node_id: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    label: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeTrustRootUpsertRequest {
    actor: String,
    reason: String,
    issuer_did: String,
    label: String,
    public_key_hex: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeTrustRootRecord {
    issuer_did: String,
    label: String,
    public_key_hex: String,
    updated_by: String,
    updated_reason: String,
    created_at_unix_ms: u128,
    updated_at_unix_ms: u128,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeTrustRootUpsertResponse {
    trust_root: NodeTrustRootRecord,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapSessionResponse {
    session: OperatorSessionRecord,
    session_token: String,
    bootstrap_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorSessionRecord {
    operator_name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorkspaceProfileRecord {
    tenant_id: String,
    project_id: String,
    display_name: String,
    region: String,
    default_model_providers: Vec<String>,
    default_chat_platforms: Vec<String>,
    onboarding_status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceProfileUpdateRequest {
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
struct WorkspaceProfileUpdateResponse {
    workspace: WorkspaceProfileRecord,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ApprovalRequestSummary {
    approval_id: String,
    kind: String,
    reference_id: String,
    status: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PaymentRecordSummary {
    transaction_id: String,
    task_id: Option<String>,
    mandate_id: String,
    amount: f64,
    description: String,
    status: String,
    verification_message: String,
    mcu_public_did: Option<String>,
}

#[derive(Debug, Clone)]
struct SignedAp2Payload {
    signer_label: String,
    mcu_public_did: String,
    mcu_signature: String,
}

#[derive(Debug, Clone)]
struct SetupSkillCandidate {
    skill_id: String,
    version: String,
    federated: bool,
    signed: bool,
    label: String,
}

#[derive(Debug, Clone, Copy)]
enum SetupFieldKind {
    ApiKey,
    AccessToken,
    WebhookUrl,
    AppId,
    AppSecret,
    ClientSecret,
    EndpointId,
    BaseUrl,
}

#[derive(Debug, Clone, Copy)]
struct SetupFieldSpec {
    kind: SetupFieldKind,
    label: &'static str,
    required: bool,
    default_value: Option<&'static str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplaceCatalog {
    skills: Vec<MarketplaceSkillEntry>,
    #[serde(default)]
    agent_cards: Vec<MarketplaceAgentEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplaceCatalog {
    skills: Vec<FederatedMarketplaceSkillEnvelope>,
    #[serde(default)]
    agent_cards: Vec<FederatedMarketplaceAgentEnvelope>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MarketplaceSkillEntry {
    skill_id: String,
    version: String,
    display_name: String,
    description: Option<String>,
    capabilities: Vec<String>,
    signed: bool,
    active: bool,
    package_url: String,
    install_url: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplaceSkillEnvelope {
    source_display_name: String,
    source_peer_id: String,
    entry: MarketplaceSkillEntry,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MarketplaceAgentEntry {
    card_id: String,
    name: String,
    description: String,
    url: String,
    published: bool,
    locally_hosted: bool,
    chat_platforms: Vec<String>,
    model_providers: Vec<String>,
    payment_roles: Vec<String>,
    card_url: Option<String>,
    install_url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplaceAgentEnvelope {
    source_display_name: String,
    source_peer_id: String,
    entry: MarketplaceAgentEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallSkillPackageRequest {
    package_url: String,
    activate: Option<bool>,
    allow_unsigned: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillActivationResponse {
    skill: InstalledSkillRecord,
    activated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledSkillRecord {
    skill_id: String,
    version: String,
    active: bool,
    source_kind: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallAgentCardRequest {
    card_url: String,
    card_id: Option<String>,
    published: Option<bool>,
    regions: Option<Vec<String>>,
    languages: Option<Vec<String>>,
    model_providers: Option<Vec<String>>,
    chat_platforms: Option<Vec<String>>,
    payment_roles: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InvokeAgentCardRequest {
    name: String,
    instruction: String,
    parent_task_id: Option<String>,
    await_completion: Option<bool>,
    timeout_seconds: Option<u64>,
    poll_interval_ms: Option<u64>,
    settlement: Option<RemoteSettlementRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteSettlementRequest {
    mandate_id: String,
    amount: f64,
    description: String,
    quote_id: Option<String>,
    counter_offer_amount: Option<f64>,
}

#[derive(Debug)]
struct DelegateExecution {
    quote: Option<Value>,
    invocation: Value,
}

#[derive(Debug)]
struct DelegateExecutionRequest {
    card_id: String,
    name: String,
    instruction: String,
    await_completion: bool,
    timeout_seconds: Option<u64>,
    mandate_id: Option<String>,
    amount: Option<f64>,
    settlement_description: Option<String>,
    quote_id: Option<String>,
    counter_offer_amount: Option<f64>,
    remote_quote: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeClaimCreateRequest {
    session_token: String,
    node_id: String,
    display_name: Option<String>,
    transport: Option<String>,
    requested_capabilities: Option<Vec<String>>,
    expires_in_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeClaimCreateResponse {
    claim: NodeClaimRecord,
    claim_token: String,
    session_url: String,
    launch_url: String,
    token_hint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeClaimRecord {
    node_id: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeClaimSummaryRecord {
    claim_id: String,
    node_id: String,
    status: String,
    expires_at_unix_ms: u128,
}

pub async fn dispatch_from_args() -> anyhow::Result<CliOutcome> {
    let cli = DawnCli::parse();
    let Some(command) = cli.command else {
        let mut profile = load_profile_or_default();
        let interactive_terminal = io::stdin().is_terminal() && io::stdout().is_terminal();
        if interactive_terminal {
            if should_auto_launch_setup(&profile) {
                println!("No Dawn CLI login found. Starting guided setup...");
                setup(SetupArgs {
                    gateway: None,
                    bootstrap_token: DEFAULT_BOOTSTRAP_TOKEN.to_string(),
                    operator_name: None,
                    display_name: None,
                    tenant_id: None,
                    project_id: None,
                    region: None,
                    model: Vec::new(),
                    channel: Vec::new(),
                    skill: Vec::new(),
                    federated_skills: false,
                    allow_unsigned_skills: false,
                    allow_shell: false,
                    no_claim: false,
                    yes: false,
                    advanced: false,
                    env: Vec::new(),
                })
                .await?;
                return Ok(CliOutcome::Exit);
            }

            match prompt_startup_action(&profile)? {
                StartupAction::Start => {
                    start_stack(&mut profile, &StartArgs::default()).await?;
                    println!("Starting local node runtime...");
                    return Ok(CliOutcome::RunNode);
                }
                StartupAction::Setup => {
                    println!("Opening guided setup...");
                    setup(SetupArgs {
                        gateway: None,
                        bootstrap_token: DEFAULT_BOOTSTRAP_TOKEN.to_string(),
                        operator_name: None,
                        display_name: None,
                        tenant_id: None,
                        project_id: None,
                        region: None,
                        model: Vec::new(),
                        channel: Vec::new(),
                        skill: Vec::new(),
                        federated_skills: false,
                        allow_unsigned_skills: false,
                        allow_shell: false,
                        no_claim: false,
                        yes: false,
                        advanced: false,
                        env: Vec::new(),
                    })
                    .await?;
                    return Ok(CliOutcome::Exit);
                }
                StartupAction::Status => {
                    print_local_status()?;
                    return Ok(CliOutcome::Exit);
                }
                StartupAction::Exit => return Ok(CliOutcome::Exit),
            }
        }
        ensure_node_runtime_preflight(&mut profile).await?;
        return Ok(CliOutcome::RunNode);
    };

    match command {
        Commands::Start(args) => {
            let mut profile = load_profile_or_default();
            start_stack(&mut profile, &args).await?;
            Ok(CliOutcome::RunNode)
        }
        Commands::Run => {
            let mut profile = load_profile_or_default();
            ensure_node_runtime_preflight(&mut profile).await?;
            Ok(CliOutcome::RunNode)
        }
        Commands::Login(args) => {
            login(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Onboard(args) => {
            onboard(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Setup(args) => {
            setup(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Doctor(args) => {
            doctor(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Logout => {
            logout()?;
            Ok(CliOutcome::Exit)
        }
        Commands::Status => {
            print_local_status()?;
            Ok(CliOutcome::Exit)
        }
        Commands::Connectors(args) => {
            handle_connectors(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Gateway(args) => {
            handle_gateway(args)?;
            Ok(CliOutcome::Exit)
        }
        Commands::Secrets(args) => {
            handle_secrets(args)?;
            Ok(CliOutcome::Exit)
        }
        Commands::Models(args) => {
            handle_models(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Channels(args) => {
            handle_channels(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Ingress(args) => {
            handle_ingress(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Skills(args) => {
            handle_skills(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Agents(args) => {
            handle_agents(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Delegate(args) => {
            delegate(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Chat(args) => {
            chat(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Tasks(args) => {
            handle_tasks(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Ap2(args) => {
            handle_ap2(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Approvals(args) => {
            handle_approvals(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Node(args) => {
            handle_node(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::NodeCommands(args) => {
            handle_node_commands(args).await?;
            Ok(CliOutcome::Exit)
        }
    }
}

async fn login(args: LoginArgs) -> anyhow::Result<()> {
    if !args.session_only {
        return setup(SetupArgs {
            gateway: args.gateway,
            bootstrap_token: args.bootstrap_token,
            operator_name: args.operator_name,
            display_name: None,
            tenant_id: None,
            project_id: None,
            region: None,
            model: args.model,
            channel: args.channel,
            skill: args.skill,
            federated_skills: args.federated_skills,
            allow_unsigned_skills: args.allow_unsigned_skills,
            allow_shell: args.allow_shell,
            no_claim: args.no_claim,
            yes: args.yes,
            advanced: args.advanced,
            env: args.env,
        })
        .await;
    }
    let mut profile = load_profile_or_default();
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url.clone())?;
    let interactive = !args.yes;
    let operator_default = default_operator_name(&profile);
    let operator_name = prompt_setup_text(
        "Operator name",
        args.operator_name.as_deref(),
        Some(operator_default.as_str()),
        interactive,
    )?;
    let response = bootstrap_session(&client, &args.bootstrap_token, &operator_name).await?;

    profile.gateway_base_url = Some(gateway_base_url.clone());
    profile.session_token = Some(response.session_token);
    profile.operator_name = Some(response.session.operator_name.clone());
    profile.bootstrap_mode = Some(response.bootstrap_mode.clone());
    let path = save_profile(&profile)?;

    println!("Logged in to {gateway_base_url}");
    println!("Operator: {}", response.session.operator_name);
    println!("Bootstrap mode: {}", response.bootstrap_mode);
    println!("Profile saved: {}", path.display());
    println!(
        "Next: run `dawn-node login` for the full guided setup or `dawn-node setup --advanced`."
    );
    Ok(())
}

async fn onboard(args: OnboardArgs) -> anyhow::Result<()> {
    let mut profile = load_profile_or_default();
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url.clone())?;
    let response = bootstrap_session(&client, &args.bootstrap_token, &args.operator_name).await?;

    profile.gateway_base_url = Some(gateway_base_url.clone());
    profile.session_token = Some(response.session_token.clone());
    profile.operator_name = Some(response.session.operator_name.clone());
    profile.bootstrap_mode = Some(response.bootstrap_mode.clone());

    let workspace: WorkspaceProfileRecord =
        client.get_json("/api/gateway/identity/workspace").await?;
    let default_model_providers = if args.model.is_empty() {
        workspace.default_model_providers.clone()
    } else {
        update_values(&workspace.default_model_providers, &args.model, true)
    };
    let default_chat_platforms = if args.channel.is_empty() {
        workspace.default_chat_platforms.clone()
    } else {
        update_values(&workspace.default_chat_platforms, &args.channel, true)
    };
    let updated_workspace = upsert_workspace(
        &client,
        &response.session_token,
        WorkspaceProfileUpdateRequest {
            session_token: response.session_token.clone(),
            tenant_id: args.tenant_id.unwrap_or(workspace.tenant_id),
            project_id: args.project_id.unwrap_or(workspace.project_id),
            display_name: args.display_name.unwrap_or(workspace.display_name),
            region: args.region.unwrap_or(workspace.region),
            default_model_providers,
            default_chat_platforms,
            onboarding_status: Some("configured".to_string()),
        },
    )
    .await?;

    let mut claim_summary = None;
    if !args.no_claim {
        let node_id = args
            .node_id
            .clone()
            .or_else(|| profile.node_id.clone())
            .unwrap_or_else(|| "node-local".to_string());
        let node_name = args
            .node_name
            .clone()
            .or_else(|| profile.node_name.clone())
            .unwrap_or_else(|| "Dawn Local Node".to_string());
        let requested_capabilities = default_requested_capabilities(args.allow_shell);
        let claim = issue_node_claim(
            &client,
            &response.session_token,
            &node_id,
            &node_name,
            requested_capabilities,
            1800,
        )
        .await?;
        profile.node_id = Some(claim.claim.node_id.clone());
        profile.node_name = Some(claim.claim.display_name.clone());
        profile.claim_token = Some(claim.claim_token.clone());
        profile.requested_capabilities = default_requested_capabilities(args.allow_shell);
        claim_summary = Some(claim);
    }

    let path = save_profile(&profile)?;
    println!("Onboarded against {gateway_base_url}");
    println!("Operator: {}", response.session.operator_name);
    println!(
        "Workspace: {} [{}] models={} channels={}",
        updated_workspace.display_name,
        updated_workspace.region,
        updated_workspace.default_model_providers.join(", "),
        updated_workspace.default_chat_platforms.join(", ")
    );
    if let Some(claim) = claim_summary {
        println!(
            "Node claim: {} ({}) tokenHint={}",
            claim.claim.node_id, claim.claim.display_name, claim.token_hint
        );
    } else {
        println!("Node claim: skipped");
    }
    println!("Profile saved: {}", path.display());
    Ok(())
}

async fn setup(args: SetupArgs) -> anyhow::Result<()> {
    let mut profile = load_profile_or_default();
    let previous_operator_name = profile.operator_name.clone();
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url.clone())?;
    let interactive = !args.yes;
    let parsed_env = parse_env_assignments(&args.env)?;
    for (key, value) in parsed_env {
        profile.connector_env.insert(key, value);
    }

    if interactive {
        println!("Dawn CLI guided setup");
        println!(
            "This flow will log you in, choose an AI model, choose a chat app, and optionally install skills and claim a local node."
        );
        println!(
            "Fast path: choose OpenAI Codex and sign in with ChatGPT, or choose OpenAI / Anthropic Claude / Google Gemini and paste an API key; choose Telegram Bot and paste a bot token."
        );
        println!();
    }
    let default_operator = args
        .operator_name
        .clone()
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| default_operator_name(&profile));
    let operator_name = prompt_setup_text(
        "Operator name",
        args.operator_name.as_deref(),
        Some(default_operator.as_str()),
        interactive,
    )?;
    let response = bootstrap_session(&client, &args.bootstrap_token, &operator_name).await?;

    profile.gateway_base_url = Some(gateway_base_url.clone());
    profile.session_token = Some(response.session_token.clone());
    profile.operator_name = Some(response.session.operator_name.clone());
    profile.bootstrap_mode = Some(response.bootstrap_mode.clone());

    let workspace: WorkspaceProfileRecord =
        client.get_json("/api/gateway/identity/workspace").await?;
    let connectors_status: Value = client.get_json("/api/gateway/connectors/status").await?;
    let supported_models =
        connector_targets_from_status(&connectors_status, "supportedModelProviders", "provider");
    let supported_channels =
        connector_targets_from_status(&connectors_status, "supportedChatPlatforms", "platform");

    println!(
        "Logged in to {gateway_base_url} as {}",
        response.session.operator_name
    );
    if interactive {
        println!(
            "Current workspace defaults: models = {}; chats = {}",
            if workspace.default_model_providers.is_empty() {
                "<none>".to_string()
            } else {
                workspace.default_model_providers.join(", ")
            },
            if workspace.default_chat_platforms.is_empty() {
                "<none>".to_string()
            } else {
                workspace.default_chat_platforms.join(", ")
            }
        );
        println!(
            "If you pick a model/chat that is already configured, setup will ask whether to reuse the live/staged credentials or replace them."
        );
    }

    let selected_models = choose_setup_targets(
        "AI models",
        &args.model,
        &supported_models,
        &workspace.default_model_providers,
        true,
        interactive,
    )?;
    let selected_channels = choose_setup_targets(
        "Chat apps",
        &args.channel,
        &supported_channels,
        &workspace.default_chat_platforms,
        false,
        interactive,
    )?;

    let mut staged_env = profile.connector_env.clone();
    let original_staged_env = staged_env.clone();
    for target in &selected_models {
        ensure_setup_connector_ready(
            &mut staged_env,
            &connectors_status,
            true,
            target,
            interactive,
            args.gateway.as_deref(),
        )?;
    }
    for target in &selected_channels {
        ensure_setup_connector_ready(
            &mut staged_env,
            &connectors_status,
            false,
            target,
            interactive,
            args.gateway.as_deref(),
        )?;
    }

    let workspace_defaults =
        suggest_setup_workspace_identity(&workspace, &response.session.operator_name);
    let advanced_identity = args.advanced
        || args.display_name.is_some()
        || args.tenant_id.is_some()
        || args.project_id.is_some()
        || args.region.is_some();
    let (display_name, tenant_id, project_id, region) = if advanced_identity {
        (
            prompt_setup_text(
                "Workspace display name",
                args.display_name.as_deref(),
                Some(workspace_defaults.display_name.as_str()),
                interactive,
            )?,
            prompt_setup_text(
                "Tenant ID",
                args.tenant_id.as_deref(),
                Some(workspace_defaults.tenant_id.as_str()),
                interactive,
            )?,
            prompt_setup_text(
                "Project ID",
                args.project_id.as_deref(),
                Some(workspace_defaults.project_id.as_str()),
                interactive,
            )?,
            prompt_setup_text(
                "Region",
                args.region.as_deref(),
                Some(workspace_defaults.region.as_str()),
                interactive,
            )?,
        )
    } else {
        let display_name = args
            .display_name
            .clone()
            .unwrap_or(workspace_defaults.display_name.clone());
        let tenant_id = args
            .tenant_id
            .clone()
            .unwrap_or(workspace_defaults.tenant_id.clone());
        let project_id = args
            .project_id
            .clone()
            .unwrap_or(workspace_defaults.project_id.clone());
        let region = args
            .region
            .clone()
            .unwrap_or(workspace_defaults.region.clone());
        let operator_changed = previous_operator_name
            .as_deref()
            .map(|value| !value.eq_ignore_ascii_case(&response.session.operator_name))
            .unwrap_or(false);
        let identity_differs_from_suggested = workspace.display_name.trim()
            != workspace_defaults.display_name.trim()
            || workspace.tenant_id.trim() != workspace_defaults.tenant_id.trim()
            || workspace.project_id.trim() != workspace_defaults.project_id.trim()
            || workspace.region.trim() != workspace_defaults.region.trim();
        let (display_name, tenant_id, project_id, region) = if interactive
            && operator_changed
            && identity_differs_from_suggested
            && prompt_confirm(
                &format!(
                    "Update workspace identity to match operator `{}`",
                    response.session.operator_name
                ),
                true,
            )?
        {
            (
                workspace_defaults.display_name.clone(),
                workspace_defaults.tenant_id.clone(),
                workspace_defaults.project_id.clone(),
                workspace_defaults.region.clone(),
            )
        } else {
            (display_name, tenant_id, project_id, region)
        };
        println!(
            "Using workspace identity: {} [{}] tenant={} project={}",
            display_name, region, tenant_id, project_id
        );
        println!("Use `dawn-node setup --advanced` if you want to edit these fields manually.");
        (display_name, tenant_id, project_id, region)
    };

    let updated_workspace = upsert_workspace(
        &client,
        &response.session_token,
        WorkspaceProfileUpdateRequest {
            session_token: response.session_token.clone(),
            tenant_id,
            project_id,
            display_name,
            region,
            default_model_providers: selected_models.clone(),
            default_chat_platforms: selected_channels.clone(),
            onboarding_status: Some("configured".to_string()),
        },
    )
    .await?;

    let selected_skills = if !args.skill.is_empty() {
        args.skill
            .iter()
            .map(|skill_id| SetupSkillCandidate {
                skill_id: skill_id.trim().to_string(),
                version: "latest".to_string(),
                federated: args.federated_skills,
                signed: true,
                label: skill_id.trim().to_string(),
            })
            .collect::<Vec<_>>()
    } else if interactive {
        prompt_setup_skills(&client).await?
    } else {
        Vec::new()
    };

    let connectors_reconfigured = staged_env != original_staged_env;
    profile.connector_env = staged_env;
    let mut allow_unsigned_skills = args.allow_unsigned_skills;
    if interactive
        && !allow_unsigned_skills
        && selected_skills.iter().any(|skill| !skill.signed)
    {
        println!("One or more selected skills are unsigned.");
        allow_unsigned_skills = prompt_confirm(
            "Allow installing unsigned skills for this setup run",
            false,
        )?;
        if !allow_unsigned_skills {
            println!("Skipping unsigned skills. Re-run setup or use `--allow-unsigned-skills` if you trust that source.");
        }
    }
    let mut installed_skills = Vec::new();
    for skill in &selected_skills {
        if !allow_unsigned_skills && !skill.signed {
            continue;
        }
        let install_args = SkillInstallArgs {
            skill_id: skill.skill_id.clone(),
            version: if skill.version == "latest" {
                None
            } else {
                Some(skill.version.clone())
            },
            gateway: Some(gateway_base_url.clone()),
            federated: skill.federated,
            all: true,
            allow_unsigned: allow_unsigned_skills,
            no_activate: false,
        };
        install_skill(install_args).await?;
        installed_skills.push(skill.label.clone());
    }

    let should_claim = if args.no_claim {
        false
    } else if interactive {
        prompt_confirm("Issue a local node claim now", true)?
    } else {
        true
    };
    let allow_shell = if should_claim && interactive && !args.allow_shell {
        prompt_confirm("Allow shell_exec capability for this node", false)?
    } else {
        args.allow_shell
    };
    if should_claim {
        let node_id = profile
            .node_id
            .clone()
            .unwrap_or_else(|| "node-local".to_string());
        let node_name = profile
            .node_name
            .clone()
            .unwrap_or_else(|| "Dawn Local Node".to_string());
        let requested_capabilities = default_requested_capabilities(allow_shell);
        let claim = issue_node_claim(
            &client,
            &response.session_token,
            &node_id,
            &node_name,
            requested_capabilities.clone(),
            1800,
        )
        .await?;
        profile.node_id = Some(claim.claim.node_id.clone());
        profile.node_name = Some(claim.claim.display_name.clone());
        profile.claim_token = Some(claim.claim_token.clone());
        profile.requested_capabilities = requested_capabilities;
        println!(
            "Issued node claim for {}. claimToken hint: {}",
            claim.claim.node_id, claim.token_hint
        );
        println!("Next: run `dawn-node node trust-self` so the gateway trusts this local node.");
    }

    let path = save_profile(&profile)?;
    println!("Setup complete.");
    println!("Operator login: ready");
    println!(
        "Workspace defaults: models = {}; channels = {}",
        updated_workspace.default_model_providers.join(", "),
        updated_workspace.default_chat_platforms.join(", ")
    );
    if installed_skills.is_empty() {
        println!("Skills: <none installed during setup>");
    } else {
        println!("Skills installed: {}", installed_skills.join(", "));
    }
    println!("Profile saved: {}", path.display());
    if connectors_reconfigured {
        println!(
            "Connector credentials were updated locally. Restart the gateway to switch the live model/chat configuration to the new credentials."
        );
    }
    if updated_workspace
        .default_chat_platforms
        .iter()
        .any(|platform| platform == "telegram")
    {
        println!("Telegram bot commands: /help, /new, /skills, /skill, /model, /status");
    }
    print_setup_runtime_summary(
        &connectors_status,
        &profile.connector_env,
        &selected_models,
        &selected_channels,
    );
    println!("Next: run `dawn-node start` for the simple path, or `dawn-node gateway start` / `dawn-node run` for manual control.");
    Ok(())
}

#[derive(Debug, Clone)]
struct SetupWorkspaceIdentity {
    display_name: String,
    tenant_id: String,
    project_id: String,
    region: String,
}

fn should_auto_launch_setup(profile: &DawnCliProfile) -> bool {
    profile.session_token.is_none()
}

fn default_operator_name(profile: &DawnCliProfile) -> String {
    profile
        .operator_name
        .clone()
        .or_else(|| {
            env::var("USERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| {
            env::var("USER")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| DEFAULT_OPERATOR_NAME.to_string())
}

fn print_setup_runtime_summary(
    connectors_status: &Value,
    staged_env: &BTreeMap<String, String>,
    selected_models: &[String],
    selected_channels: &[String],
) {
    if selected_models.is_empty() && selected_channels.is_empty() {
        return;
    }
    println!("Runtime readiness:");
    for target in selected_models {
        println!(
            "  model {:<20} {}",
            connector_setup_label(true, target),
            setup_runtime_status_label(connectors_status, staged_env, true, target)
        );
    }
    for target in selected_channels {
        println!(
            "  chat  {:<20} {}",
            connector_setup_label(false, target),
            setup_runtime_status_label(connectors_status, staged_env, false, target)
        );
    }
}

fn setup_runtime_status_label(
    connectors_status: &Value,
    staged_env: &BTreeMap<String, String>,
    is_models: bool,
    target: &str,
) -> &'static str {
    if connector_is_live_configured(connectors_status, target) {
        "live on gateway"
    } else if connector_env_ready(is_models, target, staged_env) {
        "staged locally (restart gateway to switch live config)"
    } else {
        "needs additional credentials"
    }
}

fn suggest_setup_workspace_identity(
    workspace: &WorkspaceProfileRecord,
    operator_name: &str,
) -> SetupWorkspaceIdentity {
    let operator_slug = slugify_setup_identifier(operator_name, "desktop");
    let host_slug = env::var("COMPUTERNAME")
        .ok()
        .or_else(|| env::var("HOSTNAME").ok())
        .map(|value| slugify_setup_identifier(&value, "desktop"))
        .unwrap_or_else(|| "desktop".to_string());
    SetupWorkspaceIdentity {
        display_name: preferred_setup_value(
            &workspace.display_name,
            DEFAULT_WORKSPACE_DISPLAY_NAME,
            format!("{operator_name} workspace"),
        ),
        tenant_id: preferred_setup_value(
            &workspace.tenant_id,
            DEFAULT_WORKSPACE_TENANT_ID,
            operator_slug,
        ),
        project_id: preferred_setup_value(
            &workspace.project_id,
            DEFAULT_WORKSPACE_PROJECT_ID,
            format!("{host_slug}-desktop"),
        ),
        region: if workspace.region.trim().is_empty() {
            DEFAULT_REGION.to_string()
        } else {
            workspace.region.clone()
        },
    }
}

fn prompt_startup_action(profile: &DawnCliProfile) -> anyhow::Result<StartupAction> {
    println!("Dawn CLI");
    println!(
        "Operator: {}",
        profile
            .operator_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("<not logged in>")
    );
    println!(
        "Gateway: {}",
        profile
            .gateway_base_url
            .clone()
            .unwrap_or_else(default_gateway_base_url)
    );
    println!("1. Start Dawn (gateway + local node runtime)");
    println!("2. Guided setup / change AI model and chat app");
    println!("3. Show local status");
    println!("4. Exit");
    loop {
        let input = prompt_line("Choose an action [1]: ")?;
        match input.trim().to_ascii_lowercase().as_str() {
            "" | "1" | "start" | "run" => return Ok(StartupAction::Start),
            "2" | "setup" | "login" => return Ok(StartupAction::Setup),
            "3" | "status" => return Ok(StartupAction::Status),
            "4" | "exit" | "quit" => return Ok(StartupAction::Exit),
            _ => println!("Enter 1, 2, 3, or 4."),
        }
    }
}

async fn start_stack(profile: &mut DawnCliProfile, args: &StartArgs) -> anyhow::Result<()> {
    if !args.skip_gateway {
        ensure_gateway_running(profile, args.release).await?;
    }
    ensure_node_runtime_preflight(profile).await?;
    if args.app {
        let app_url = format!(
            "{}/app",
            profile
                .gateway_base_url
                .clone()
                .unwrap_or_else(default_gateway_base_url)
                .trim_end_matches('/')
        );
        open_default_browser(&app_url)?;
        println!("Opened {app_url}");
    }
    Ok(())
}

async fn ensure_gateway_running(profile: &DawnCliProfile, release: bool) -> anyhow::Result<()> {
    let gateway_base_url = resolve_gateway_base_url(profile.gateway_base_url.as_deref(), profile);
    if gateway_health_ok(&gateway_base_url).await {
        println!("Gateway already running at {gateway_base_url}");
        return Ok(());
    }

    let exe = env::current_exe().context("failed to resolve current dawn-node executable")?;
    let mut command = StdCommand::new(exe);
    command.arg("gateway").arg("start");
    if release {
        command.arg("--release");
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    for (key, value) in &profile.connector_env {
        command.env(key, value);
    }
    if let Some(gateway) = profile.gateway_base_url.as_deref() {
        command.env("DAWN_PUBLIC_BASE_URL", gateway);
    }
    let child = command
        .spawn()
        .context("failed to spawn background Dawn gateway launcher")?;
    println!(
        "Starting Dawn gateway in the background (launcher pid={})...",
        child.id()
    );

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(25);
    loop {
        if gateway_health_ok(&gateway_base_url).await {
            println!("Gateway is ready at {gateway_base_url}");
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "gateway did not become ready at {} within the expected startup window",
                gateway_base_url
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

async fn gateway_health_ok(base_url: &str) -> bool {
    let health_url = format!("{}/health", base_url.trim_end_matches('/'));
    match reqwest::get(&health_url).await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn open_default_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let status = StdCommand::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .context("failed to invoke Windows browser launcher")?;
        if !status.success() {
            bail!("browser launcher exited with status {status}");
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let status = StdCommand::new("open")
            .arg(url)
            .status()
            .context("failed to invoke macOS browser launcher")?;
        if !status.success() {
            bail!("browser launcher exited with status {status}");
        }
        return Ok(());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let status = StdCommand::new("xdg-open")
            .arg(url)
            .status()
            .context("failed to invoke browser launcher")?;
        if !status.success() {
            bail!("browser launcher exited with status {status}");
        }
        Ok(())
    }
}

fn preferred_setup_value(current: &str, placeholder: &str, fallback: String) -> String {
    let trimmed = current.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(placeholder) {
        fallback
    } else {
        trimmed.to_string()
    }
}

fn slugify_setup_identifier(value: &str, fallback: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    }
}

fn prompt_setup_text(
    label: &str,
    provided: Option<&str>,
    default: Option<&str>,
    interactive: bool,
) -> anyhow::Result<String> {
    if let Some(value) = provided.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(value.to_string());
    }
    if !interactive {
        return default
            .map(str::to_string)
            .ok_or_else(|| anyhow!("{label} is required"));
    }
    loop {
        let prompt = match default.filter(|value| !value.is_empty()) {
            Some(default) => format!("{label} [{default}]: "),
            None => format!("{label}: "),
        };
        let input = prompt_line(&prompt)?;
        let value = if input.trim().is_empty() {
            default.unwrap_or("")
        } else {
            input.trim()
        };
        if !value.is_empty() {
            return Ok(value.to_string());
        }
        println!("{label} is required.");
    }
}

fn prompt_confirm(label: &str, default: bool) -> anyhow::Result<bool> {
    loop {
        let suffix = if default { "[Y/n]" } else { "[y/N]" };
        let input = prompt_line(&format!("{label} {suffix}: "))?;
        let normalized = input.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(default);
        }
        match normalized.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Enter y or n."),
        }
    }
}

fn prompt_line(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read from stdin")?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

fn choose_setup_targets(
    label: &str,
    provided: &[String],
    supported: &[String],
    current: &[String],
    is_models: bool,
    interactive: bool,
) -> anyhow::Result<Vec<String>> {
    if !provided.is_empty() {
        return Ok(unique_targets(
            provided
                .iter()
                .map(|value| normalize_connector_target(is_models, value))
                .collect(),
        ));
    }
    if !interactive {
        return Ok(unique_targets(current.to_vec()));
    }
    if supported.is_empty() {
        return Ok(unique_targets(current.to_vec()));
    }
    let ordered = order_setup_targets(supported, is_models);
    println!("{label}:");
    for (index, option) in ordered.iter().enumerate() {
        println!(
            "  {}. {}",
            index + 1,
            connector_setup_option_label(is_models, option)
        );
    }
    let default_display = if current.is_empty() {
        "<none>".to_string()
    } else {
        current
            .iter()
            .map(|value| connector_setup_option_label(is_models, value))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let input = prompt_line(&format!(
        "{label} (comma-separated names or indices, blank keeps {default_display}): "
    ))?;
    if input.trim().is_empty() {
        return Ok(unique_targets(current.to_vec()));
    }
    parse_named_selection(&input, &ordered, is_models)
}

async fn prompt_setup_skills(client: &GatewayClient) -> anyhow::Result<Vec<SetupSkillCandidate>> {
    let local = fetch_local_catalog(client, None, true)
        .await
        .unwrap_or(MarketplaceCatalog {
            skills: vec![],
            agent_cards: vec![],
        });
    let federated =
        fetch_federated_catalog(client, None, true)
            .await
            .unwrap_or(FederatedMarketplaceCatalog {
                skills: vec![],
                agent_cards: vec![],
            });
    let mut candidates = Vec::new();
    candidates.extend(local.skills.into_iter().map(|skill| SetupSkillCandidate {
        skill_id: skill.skill_id.clone(),
        version: skill.version.clone(),
        federated: false,
        signed: skill.signed,
        label: format!(
            "{}@{} [local, {}] {}",
            skill.skill_id,
            skill.version,
            if skill.signed { "signed" } else { "unsigned" },
            skill.display_name
        ),
    }));
    candidates.extend(
        federated
            .skills
            .into_iter()
            .map(|skill| SetupSkillCandidate {
                skill_id: skill.entry.skill_id.clone(),
                version: skill.entry.version.clone(),
                federated: true,
                signed: skill.entry.signed,
                label: format!(
                    "{}@{} [federated {}:{}, {}] {}",
                    skill.entry.skill_id,
                    skill.entry.version,
                    skill.source_display_name,
                    skill.source_peer_id,
                    if skill.entry.signed { "signed" } else { "unsigned" },
                    skill.entry.display_name
                ),
            }),
    );
    if candidates.is_empty() {
        println!("No installable skills found in the local or federated catalog.");
        return Ok(Vec::new());
    }
    println!("Available skills:");
    for (index, skill) in candidates.iter().enumerate() {
        println!("  {}. {}", index + 1, skill.label);
    }
    let input =
        prompt_line("Skills to install (comma-separated indices, blank skips installation): ")?;
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    parse_index_selection(&input, &candidates)
}

fn ensure_setup_connector_ready(
    staged_env: &mut BTreeMap<String, String>,
    connectors_status: &Value,
    is_models: bool,
    target: &str,
    interactive: bool,
    gateway: Option<&str>,
) -> anyhow::Result<()> {
    let env_ready = connector_env_ready(is_models, target, staged_env);
    let live_ready = connector_is_live_configured(connectors_status, target);
    if env_ready || live_ready {
        if interactive {
            let target_label = connector_setup_label(is_models, target);
            let reuse_existing = prompt_confirm(
                &format!(
                    "Reuse existing {} `{}` credentials{}",
                    if is_models {
                        "model provider"
                    } else {
                        "chat platform"
                    },
                    target_label,
                    if live_ready && !env_ready {
                        " from the running gateway"
                    } else if env_ready {
                        " from the local staged profile"
                    } else {
                        ""
                    }
                ),
                true,
            )?;
            if reuse_existing {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }
    if is_models && target == "openai_codex" {
        if !interactive {
            bail!(
                "selected model provider `OpenAI Codex` is not ready on the gateway and requires an interactive `codex login`"
            );
        }
        ensure_openai_codex_auth_ready(true)?;
        return Ok(());
    }
    let target_label = connector_setup_label(is_models, target);
    if !interactive {
        bail!(
            "selected {} `{}` is not configured on the gateway and no staged credentials were provided",
            if is_models {
                "model provider"
            } else {
                "chat platform"
            },
            target_label
        );
    }
    println!(
        "Configure {} `{}`",
        if is_models {
            "model provider"
        } else {
            "chat platform"
        },
        target_label
    );
    let prompt_args = prompt_connector_connect_args(is_models, target, staged_env, gateway)?;
    let env_pairs = connector_secret_pairs(is_models, target, &prompt_args)?;
    for (key, value) in env_pairs {
        staged_env.insert(key, value);
    }
    if !connector_env_ready(is_models, target, staged_env)
        && !connector_is_live_configured(connectors_status, target)
    {
        bail!(
            "credentials for {} `{}` are still incomplete",
            if is_models {
                "model provider"
            } else {
                "chat platform"
            },
            target
        );
    }
    Ok(())
}

fn prompt_connector_connect_args(
    is_models: bool,
    target: &str,
    staged_env: &BTreeMap<String, String>,
    gateway: Option<&str>,
) -> anyhow::Result<ConnectorConnectArgs> {
    let target_label = connector_setup_label(is_models, target);
    let mut args = ConnectorConnectArgs {
        target: target.to_string(),
        gateway: gateway.map(str::to_string),
        api_key: None,
        access_token: None,
        webhook_url: None,
        app_id: None,
        app_secret: None,
        client_secret: None,
        endpoint_id: None,
        base_url: None,
        env: vec![],
    };
    for spec in connector_prompt_specs(is_models, target) {
        let existing = connector_existing_value(is_models, target, spec.kind, staged_env);
        let value = prompt_connector_field(&target_label, spec, existing.as_deref())?;
        if let Some(value) = value {
            apply_setup_field(&mut args, spec.kind, value);
        }
    }
    Ok(args)
}

fn prompt_connector_field(
    target: &str,
    spec: SetupFieldSpec,
    existing: Option<&str>,
) -> anyhow::Result<Option<String>> {
    loop {
        let mut prompt = format!("  {} for {}", spec.label, target);
        if existing.is_some() {
            prompt.push_str(" [stored; press Enter to keep]");
        } else if let Some(default) = spec.default_value {
            prompt.push_str(&format!(" [{default}]"));
        }
        prompt.push_str(": ");
        let input = prompt_line(&prompt)?;
        if input.trim().is_empty() {
            if existing.is_some() {
                return Ok(None);
            }
            if let Some(default) = spec.default_value {
                return Ok(Some(default.to_string()));
            }
            if spec.required {
                println!("  {} is required.", spec.label);
                continue;
            }
            return Ok(None);
        }
        return Ok(Some(input.trim().to_string()));
    }
}

fn connector_prompt_specs(is_models: bool, target: &str) -> Vec<SetupFieldSpec> {
    match (is_models, target) {
        (true, "openai")
        | (true, "anthropic")
        | (true, "google")
        | (true, "github_models")
        | (true, "huggingface")
        | (true, "openrouter")
        | (true, "groq")
        | (true, "together")
        | (true, "vercel_ai_gateway")
        | (true, "mistral")
        | (true, "nvidia")
        | (true, "deepseek")
        | (true, "qwen")
        | (true, "zhipu")
        | (true, "moonshot") => vec![SetupFieldSpec {
            kind: SetupFieldKind::ApiKey,
            label: "API key",
            required: true,
            default_value: None,
        }],
        (true, "vllm") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "Base URL",
                required: true,
                default_value: Some("http://127.0.0.1:8000/v1"),
            },
            SetupFieldSpec {
                kind: SetupFieldKind::ApiKey,
                label: "API key (optional)",
                required: false,
                default_value: None,
            },
        ],
        (true, "bedrock") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "Runtime endpoint, /openai/v1 base URL, or /chat/completions URL",
                required: true,
                default_value: Some("https://bedrock-runtime.us-east-1.amazonaws.com"),
            },
            SetupFieldSpec {
                kind: SetupFieldKind::ApiKey,
                label: "API key",
                required: true,
                default_value: None,
            },
        ],
        (true, "cloudflare_ai_gateway") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::ApiKey,
                label: "Upstream API key",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::AppId,
                label: "Cloudflare account ID",
                required: false,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::EndpointId,
                label: "Gateway ID",
                required: false,
                default_value: Some("default"),
            },
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "Gateway base URL or /chat/completions URL (optional)",
                required: false,
                default_value: None,
            },
        ],
        (true, "litellm") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "LiteLLM base URL or /chat/completions URL",
                required: true,
                default_value: Some("http://127.0.0.1:4000"),
            },
            SetupFieldSpec {
                kind: SetupFieldKind::ApiKey,
                label: "API key (optional)",
                required: false,
                default_value: None,
            },
        ],
        (true, "doubao") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::ApiKey,
                label: "API key",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::EndpointId,
                label: "Endpoint ID",
                required: true,
                default_value: None,
            },
        ],
        (true, "ollama") => vec![SetupFieldSpec {
            kind: SetupFieldKind::BaseUrl,
            label: "Base URL",
            required: true,
            default_value: Some("http://127.0.0.1:11434"),
        }],
        (false, "telegram") => vec![SetupFieldSpec {
            kind: SetupFieldKind::AccessToken,
            label: "Bot token",
            required: true,
            default_value: None,
        }],
        (false, "slack")
        | (false, "discord")
        | (false, "mattermost")
        | (false, "msteams")
        | (false, "google_chat")
        | (false, "feishu")
        | (false, "dingtalk")
        | (false, "wecom_bot") => vec![SetupFieldSpec {
            kind: SetupFieldKind::WebhookUrl,
            label: "Webhook URL",
            required: true,
            default_value: None,
        }],
        (false, "whatsapp") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::AccessToken,
                label: "Access token",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::AppId,
                label: "Phone number ID",
                required: true,
                default_value: None,
            },
        ],
        (false, "line") => vec![SetupFieldSpec {
            kind: SetupFieldKind::AccessToken,
            label: "Channel access token",
            required: true,
            default_value: None,
        }],
        (false, "matrix") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::AccessToken,
                label: "Access token",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "Homeserver URL",
                required: true,
                default_value: Some("https://matrix-client.matrix.org"),
            },
        ],
        (false, "signal") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::AccessToken,
                label: "Signal account / phone number",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "signal-cli REST base URL or /v2/send URL",
                required: false,
                default_value: Some("http://127.0.0.1:8080"),
            },
        ],
        (false, "bluebubbles") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::ClientSecret,
                label: "Server password / GUID",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::BaseUrl,
                label: "Server URL or /api/v1/message/text URL",
                required: true,
                default_value: None,
            },
        ],
        (false, "wechat_official_account") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::AppId,
                label: "App ID",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::AppSecret,
                label: "App secret",
                required: true,
                default_value: None,
            },
        ],
        (false, "qq") => vec![
            SetupFieldSpec {
                kind: SetupFieldKind::AppId,
                label: "App ID",
                required: true,
                default_value: None,
            },
            SetupFieldSpec {
                kind: SetupFieldKind::ClientSecret,
                label: "Client secret",
                required: true,
                default_value: None,
            },
        ],
        _ => vec![],
    }
}

fn connector_existing_value(
    is_models: bool,
    target: &str,
    kind: SetupFieldKind,
    staged_env: &BTreeMap<String, String>,
) -> Option<String> {
    let keys: &[&str] = match (is_models, target, kind) {
        (true, "openai", SetupFieldKind::ApiKey) => &["OPENAI_API_KEY"],
        (true, "anthropic", SetupFieldKind::ApiKey) => &["ANTHROPIC_API_KEY"],
        (true, "google", SetupFieldKind::ApiKey) => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        (true, "bedrock", SetupFieldKind::ApiKey) => &["BEDROCK_API_KEY"],
        (true, "bedrock", SetupFieldKind::BaseUrl) => &[
            "BEDROCK_CHAT_COMPLETIONS_URL",
            "BEDROCK_BASE_URL",
            "BEDROCK_RUNTIME_ENDPOINT",
        ],
        (true, "github_models", SetupFieldKind::ApiKey) => {
            &["GITHUB_MODELS_API_KEY", "GITHUB_TOKEN"]
        }
        (true, "github_models", SetupFieldKind::BaseUrl) => &["GITHUB_MODELS_CHAT_COMPLETIONS_URL"],
        (true, "huggingface", SetupFieldKind::ApiKey) => &["HUGGINGFACE_API_KEY", "HF_TOKEN"],
        (true, "huggingface", SetupFieldKind::BaseUrl) => &["HUGGINGFACE_CHAT_COMPLETIONS_URL"],
        (true, "openrouter", SetupFieldKind::ApiKey) => &["OPENROUTER_API_KEY"],
        (true, "cloudflare_ai_gateway", SetupFieldKind::ApiKey) => {
            &["CLOUDFLARE_AI_GATEWAY_API_KEY"]
        }
        (true, "cloudflare_ai_gateway", SetupFieldKind::AppId) => {
            &["CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID"]
        }
        (true, "cloudflare_ai_gateway", SetupFieldKind::EndpointId) => {
            &["CLOUDFLARE_AI_GATEWAY_ID"]
        }
        (true, "cloudflare_ai_gateway", SetupFieldKind::BaseUrl) => &[
            "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL",
            "CLOUDFLARE_AI_GATEWAY_BASE_URL",
        ],
        (true, "groq", SetupFieldKind::ApiKey) => &["GROQ_API_KEY"],
        (true, "together", SetupFieldKind::ApiKey) => &["TOGETHER_API_KEY"],
        (true, "vercel_ai_gateway", SetupFieldKind::ApiKey) => {
            &["VERCEL_AI_GATEWAY_API_KEY", "AI_GATEWAY_API_KEY"]
        }
        (true, "vercel_ai_gateway", SetupFieldKind::BaseUrl) => &[
            "VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL",
            "VERCEL_AI_GATEWAY_BASE_URL",
        ],
        (true, "vllm", SetupFieldKind::ApiKey) => &["VLLM_API_KEY"],
        (true, "vllm", SetupFieldKind::BaseUrl) => &["VLLM_BASE_URL"],
        (true, "mistral", SetupFieldKind::ApiKey) => &["MISTRAL_API_KEY"],
        (true, "nvidia", SetupFieldKind::ApiKey) => &["NVIDIA_API_KEY", "NVIDIA_NIM_API_KEY"],
        (true, "litellm", SetupFieldKind::ApiKey) => &["LITELLM_API_KEY"],
        (true, "litellm", SetupFieldKind::BaseUrl) => {
            &["LITELLM_CHAT_COMPLETIONS_URL", "LITELLM_BASE_URL"]
        }
        (true, "deepseek", SetupFieldKind::ApiKey) => &["DEEPSEEK_API_KEY"],
        (true, "qwen", SetupFieldKind::ApiKey) => &["QWEN_API_KEY"],
        (true, "zhipu", SetupFieldKind::ApiKey) => &["ZHIPU_API_KEY"],
        (true, "moonshot", SetupFieldKind::ApiKey) => &["MOONSHOT_API_KEY"],
        (true, "doubao", SetupFieldKind::ApiKey) => &["DOUBAO_API_KEY"],
        (true, "doubao", SetupFieldKind::EndpointId) => &["DOUBAO_ENDPOINT_ID"],
        (true, "ollama", SetupFieldKind::BaseUrl) => &["OLLAMA_BASE_URL"],
        (false, "telegram", SetupFieldKind::AccessToken) => &["TELEGRAM_BOT_TOKEN"],
        (false, "slack", SetupFieldKind::WebhookUrl) => &["SLACK_BOT_WEBHOOK_URL"],
        (false, "discord", SetupFieldKind::WebhookUrl) => &["DISCORD_BOT_WEBHOOK_URL"],
        (false, "mattermost", SetupFieldKind::WebhookUrl) => &["MATTERMOST_BOT_WEBHOOK_URL"],
        (false, "msteams", SetupFieldKind::WebhookUrl) => &["MSTEAMS_BOT_WEBHOOK_URL"],
        (false, "whatsapp", SetupFieldKind::AccessToken) => &["WHATSAPP_ACCESS_TOKEN"],
        (false, "whatsapp", SetupFieldKind::AppId) => &["WHATSAPP_PHONE_NUMBER_ID"],
        (false, "line", SetupFieldKind::AccessToken) => &["LINE_CHANNEL_ACCESS_TOKEN"],
        (false, "matrix", SetupFieldKind::AccessToken) => &["MATRIX_ACCESS_TOKEN"],
        (false, "matrix", SetupFieldKind::BaseUrl) => &["MATRIX_HOMESERVER_URL"],
        (false, "google_chat", SetupFieldKind::WebhookUrl) => &["GOOGLE_CHAT_BOT_WEBHOOK_URL"],
        (false, "signal", SetupFieldKind::AccessToken) => &["SIGNAL_ACCOUNT", "SIGNAL_NUMBER"],
        (false, "signal", SetupFieldKind::BaseUrl) => &[
            "SIGNAL_SEND_API_URL",
            "SIGNAL_HTTP_URL",
            "SIGNAL_CLI_REST_API_URL",
        ],
        (false, "bluebubbles", SetupFieldKind::ClientSecret) => &["BLUEBUBBLES_PASSWORD"],
        (false, "bluebubbles", SetupFieldKind::BaseUrl) => {
            &["BLUEBUBBLES_SEND_MESSAGE_URL", "BLUEBUBBLES_SERVER_URL"]
        }
        (false, "feishu", SetupFieldKind::WebhookUrl) => &["FEISHU_BOT_WEBHOOK_URL"],
        (false, "dingtalk", SetupFieldKind::WebhookUrl) => &["DINGTALK_BOT_WEBHOOK_URL"],
        (false, "wecom_bot", SetupFieldKind::WebhookUrl) => &["WECOM_BOT_WEBHOOK_URL"],
        (false, "wechat_official_account", SetupFieldKind::AppId) => {
            &["WECHAT_OFFICIAL_ACCOUNT_APP_ID"]
        }
        (false, "wechat_official_account", SetupFieldKind::AppSecret) => {
            &["WECHAT_OFFICIAL_ACCOUNT_APP_SECRET"]
        }
        (false, "qq", SetupFieldKind::AppId) => &["QQ_BOT_APP_ID"],
        (false, "qq", SetupFieldKind::ClientSecret) => &["QQ_BOT_CLIENT_SECRET"],
        _ => &[],
    };
    keys.iter().find_map(|key| staged_env.get(*key).cloned())
}

fn apply_setup_field(args: &mut ConnectorConnectArgs, kind: SetupFieldKind, value: String) {
    match kind {
        SetupFieldKind::ApiKey => args.api_key = Some(value),
        SetupFieldKind::AccessToken => args.access_token = Some(value),
        SetupFieldKind::WebhookUrl => args.webhook_url = Some(value),
        SetupFieldKind::AppId => args.app_id = Some(value),
        SetupFieldKind::AppSecret => args.app_secret = Some(value),
        SetupFieldKind::ClientSecret => args.client_secret = Some(value),
        SetupFieldKind::EndpointId => args.endpoint_id = Some(value),
        SetupFieldKind::BaseUrl => args.base_url = Some(value),
    }
}

fn connector_env_ready(
    is_models: bool,
    target: &str,
    staged_env: &BTreeMap<String, String>,
) -> bool {
    if is_models && target == "openai_codex" {
        return openai_codex_auth_ready();
    }
    connector_requirement_groups(is_models, target)
        .iter()
        .any(|group| {
            group.iter().all(|key| {
                staged_env
                    .get(*key)
                    .is_some_and(|value| !value.trim().is_empty())
            })
        })
}

fn connector_requirement_groups(is_models: bool, target: &str) -> Vec<Vec<&'static str>> {
    match (is_models, target) {
        (true, "openai_codex") => vec![vec!["OPENAI_CODEX_AUTH"]],
        (true, "openai") => vec![vec!["OPENAI_API_KEY"]],
        (true, "anthropic") => vec![vec!["ANTHROPIC_API_KEY"]],
        (true, "google") => vec![vec!["GEMINI_API_KEY"], vec!["GOOGLE_API_KEY"]],
        (true, "bedrock") => vec![
            vec!["BEDROCK_API_KEY", "BEDROCK_CHAT_COMPLETIONS_URL"],
            vec!["BEDROCK_API_KEY", "BEDROCK_BASE_URL"],
            vec!["BEDROCK_API_KEY", "BEDROCK_RUNTIME_ENDPOINT"],
        ],
        (true, "github_models") => vec![vec!["GITHUB_MODELS_API_KEY"], vec!["GITHUB_TOKEN"]],
        (true, "huggingface") => vec![vec!["HUGGINGFACE_API_KEY"], vec!["HF_TOKEN"]],
        (true, "openrouter") => vec![vec!["OPENROUTER_API_KEY"]],
        (true, "cloudflare_ai_gateway") => vec![
            vec![
                "CLOUDFLARE_AI_GATEWAY_API_KEY",
                "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL",
            ],
            vec![
                "CLOUDFLARE_AI_GATEWAY_API_KEY",
                "CLOUDFLARE_AI_GATEWAY_BASE_URL",
            ],
            vec![
                "CLOUDFLARE_AI_GATEWAY_API_KEY",
                "CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID",
                "CLOUDFLARE_AI_GATEWAY_ID",
            ],
        ],
        (true, "groq") => vec![vec!["GROQ_API_KEY"]],
        (true, "together") => vec![vec!["TOGETHER_API_KEY"]],
        (true, "vercel_ai_gateway") => vec![
            vec!["VERCEL_AI_GATEWAY_API_KEY"],
            vec!["AI_GATEWAY_API_KEY"],
            vec!["VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL"],
            vec!["VERCEL_AI_GATEWAY_BASE_URL"],
        ],
        (true, "vllm") => vec![vec!["VLLM_BASE_URL"]],
        (true, "mistral") => vec![vec!["MISTRAL_API_KEY"]],
        (true, "nvidia") => vec![vec!["NVIDIA_API_KEY"], vec!["NVIDIA_NIM_API_KEY"]],
        (true, "litellm") => {
            vec![
                vec!["LITELLM_CHAT_COMPLETIONS_URL"],
                vec!["LITELLM_BASE_URL"],
            ]
        }
        (true, "deepseek") => vec![vec!["DEEPSEEK_API_KEY"]],
        (true, "qwen") => vec![vec!["QWEN_API_KEY"]],
        (true, "zhipu") => vec![vec!["ZHIPU_API_KEY"]],
        (true, "moonshot") => vec![vec!["MOONSHOT_API_KEY"]],
        (true, "doubao") => vec![vec!["DOUBAO_API_KEY", "DOUBAO_ENDPOINT_ID"]],
        (true, "ollama") => vec![vec!["OLLAMA_BASE_URL"]],
        (false, "telegram") => vec![vec!["TELEGRAM_BOT_TOKEN"]],
        (false, "slack") => vec![vec!["SLACK_BOT_WEBHOOK_URL"]],
        (false, "discord") => vec![vec!["DISCORD_BOT_WEBHOOK_URL"]],
        (false, "mattermost") => vec![vec!["MATTERMOST_BOT_WEBHOOK_URL"]],
        (false, "msteams") => vec![vec!["MSTEAMS_BOT_WEBHOOK_URL"]],
        (false, "whatsapp") => {
            vec![vec!["WHATSAPP_ACCESS_TOKEN", "WHATSAPP_PHONE_NUMBER_ID"]]
        }
        (false, "line") => vec![vec!["LINE_CHANNEL_ACCESS_TOKEN"]],
        (false, "matrix") => vec![vec!["MATRIX_ACCESS_TOKEN", "MATRIX_HOMESERVER_URL"]],
        (false, "google_chat") => vec![vec!["GOOGLE_CHAT_BOT_WEBHOOK_URL"]],
        (false, "signal") => vec![vec!["SIGNAL_ACCOUNT"], vec!["SIGNAL_NUMBER"]],
        (false, "bluebubbles") => vec![
            vec!["BLUEBUBBLES_PASSWORD", "BLUEBUBBLES_SEND_MESSAGE_URL"],
            vec!["BLUEBUBBLES_PASSWORD", "BLUEBUBBLES_SERVER_URL"],
        ],
        (false, "feishu") => vec![vec!["FEISHU_BOT_WEBHOOK_URL"]],
        (false, "dingtalk") => vec![vec!["DINGTALK_BOT_WEBHOOK_URL"]],
        (false, "wecom_bot") => vec![vec!["WECOM_BOT_WEBHOOK_URL"]],
        (false, "wechat_official_account") => vec![
            vec!["WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN"],
            vec![
                "WECHAT_OFFICIAL_ACCOUNT_APP_ID",
                "WECHAT_OFFICIAL_ACCOUNT_APP_SECRET",
            ],
        ],
        (false, "qq") => vec![vec!["QQ_BOT_APP_ID", "QQ_BOT_CLIENT_SECRET"]],
        _ => vec![vec![]],
    }
}

fn connector_is_live_configured(connectors_status: &Value, target: &str) -> bool {
    let configured_key = connector_configured_key(target);
    connectors_status["configured"]
        .get(configured_key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn connector_configured_key(target: &str) -> &str {
    match target {
        "openai_codex" => "openaiCodex",
        "cloudflare_ai_gateway" => "cloudflareAiGateway",
        "google_chat" => "googleChat",
        "wecom_bot" => "wecomBot",
        "wechat_official_account" => "wechatOfficialAccount",
        other => other,
    }
}

fn connector_targets_from_status(status: &Value, key: &str, field: &str) -> Vec<String> {
    status[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry[field].as_str().map(ToString::to_string))
        .collect()
}

fn parse_named_selection(
    input: &str,
    options: &[String],
    is_models: bool,
) -> anyhow::Result<Vec<String>> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in input.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let value = if let Ok(index) = token.parse::<usize>() {
            options
                .get(index.saturating_sub(1))
                .ok_or_else(|| anyhow!("selection index {index} is out of range"))?
                .clone()
        } else {
            let normalized = normalize_connector_target(is_models, token);
            options
                .iter()
                .find(|option| option.eq_ignore_ascii_case(&normalized))
                .cloned()
                .ok_or_else(|| anyhow!("unknown selection `{token}`"))?
        };
        if seen.insert(value.clone()) {
            selected.push(value);
        }
    }
    Ok(selected)
}

fn order_setup_targets(options: &[String], is_models: bool) -> Vec<String> {
    let preferred: &[&str] = if is_models {
        &[
            "openai_codex",
            "openai",
            "anthropic",
            "google",
            "deepseek",
            "qwen",
            "zhipu",
            "moonshot",
            "doubao",
        ]
    } else {
        &[
            "telegram",
            "signal",
            "bluebubbles",
            "feishu",
            "dingtalk",
            "wecom_bot",
            "wechat_official_account",
            "qq",
        ]
    };
    let mut remaining = options.to_vec();
    remaining.sort();
    let mut ordered = Vec::new();
    for preferred_target in preferred {
        if let Some(index) = remaining
            .iter()
            .position(|option| option.eq_ignore_ascii_case(preferred_target))
        {
            ordered.push(remaining.remove(index));
        }
    }
    ordered.extend(remaining);
    ordered
}

fn connector_setup_label(is_models: bool, target: &str) -> String {
    if is_models {
        match target {
            "openai_codex" => "OpenAI Codex".to_string(),
            "openai" => "OpenAI".to_string(),
            "anthropic" => "Anthropic Claude".to_string(),
            "google" => "Google Gemini".to_string(),
            "bedrock" => "AWS Bedrock".to_string(),
            "github_models" => "GitHub Models".to_string(),
            "huggingface" => "Hugging Face".to_string(),
            "openrouter" => "OpenRouter".to_string(),
            "cloudflare_ai_gateway" => "Cloudflare AI Gateway".to_string(),
            "groq" => "Groq".to_string(),
            "together" => "Together AI".to_string(),
            "vercel_ai_gateway" => "Vercel AI Gateway".to_string(),
            "vllm" => "vLLM".to_string(),
            "mistral" => "Mistral".to_string(),
            "nvidia" => "NVIDIA NIM".to_string(),
            "litellm" => "LiteLLM".to_string(),
            "deepseek" => "DeepSeek".to_string(),
            "qwen" => "Qwen".to_string(),
            "zhipu" => "Zhipu".to_string(),
            "moonshot" => "Moonshot".to_string(),
            "doubao" => "Doubao".to_string(),
            "ollama" => "Ollama".to_string(),
            other => other.to_string(),
        }
    } else {
        match target {
            "telegram" => "Telegram Bot".to_string(),
            "slack" => "Slack".to_string(),
            "discord" => "Discord".to_string(),
            "mattermost" => "Mattermost".to_string(),
            "msteams" => "Microsoft Teams".to_string(),
            "whatsapp" => "WhatsApp".to_string(),
            "line" => "LINE".to_string(),
            "matrix" => "Matrix".to_string(),
            "google_chat" => "Google Chat".to_string(),
            "signal" => "Signal".to_string(),
            "bluebubbles" => "BlueBubbles".to_string(),
            "feishu" => "Feishu".to_string(),
            "dingtalk" => "DingTalk".to_string(),
            "wecom_bot" => "WeCom Bot".to_string(),
            "wechat_official_account" => "WeChat Official Account".to_string(),
            "qq" => "QQ Bot".to_string(),
            other => other.to_string(),
        }
    }
}

fn connector_setup_option_label(is_models: bool, target: &str) -> String {
    let label = connector_setup_label(is_models, target);
    let hint = match (is_models, target) {
        (true, "openai_codex") => "ChatGPT login",
        (true, "openai") | (true, "anthropic") | (true, "google") => "API key",
        (true, "deepseek") | (true, "qwen") | (true, "zhipu") | (true, "moonshot") => "API key",
        (true, "doubao") => "API key + endpoint ID",
        (true, "vllm") | (true, "ollama") => "base URL",
        (false, "telegram") => "bot token",
        (false, "signal") => "account + server URL",
        (false, "bluebubbles") => "server URL + password",
        (false, "feishu") | (false, "dingtalk") | (false, "wecom_bot") => "webhook",
        (false, "wechat_official_account") => "app id + app secret",
        (false, "qq") => "app id + client secret",
        _ => "connector credentials",
    };
    format!("{} ({}) [{}]", label, hint, target)
}

fn parse_index_selection(
    input: &str,
    options: &[SetupSkillCandidate],
) -> anyhow::Result<Vec<SetupSkillCandidate>> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in input.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let index = token
            .parse::<usize>()
            .with_context(|| format!("invalid selection `{token}`; expected numeric indices"))?;
        let candidate = options
            .get(index.saturating_sub(1))
            .cloned()
            .ok_or_else(|| anyhow!("selection index {index} is out of range"))?;
        let key = format!(
            "{}:{}:{}",
            candidate.skill_id, candidate.version, candidate.federated
        );
        if seen.insert(key) {
            selected.push(candidate);
        }
    }
    Ok(selected)
}

fn unique_targets(values: Vec<String>) -> Vec<String> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for value in values {
        if seen.insert(value.clone()) {
            selected.push(value);
        }
    }
    selected
}

async fn doctor(args: DoctorArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let identity: Value = client.get_json("/api/gateway/identity/status").await?;
    let connectors: Value = client.get_json("/api/gateway/connectors/status").await?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "identity": identity,
                "connectors": connectors,
            }))?
        );
        return Ok(());
    }

    let workspace = &identity["workspace"];
    let readiness = &identity["readiness"];
    println!(
        "Workspace: {} [{}]",
        workspace["displayName"].as_str().unwrap_or("unknown"),
        workspace["region"].as_str().unwrap_or("unknown")
    );
    println!(
        "Onboarding: {} ({}%)",
        readiness["overallStatus"].as_str().unwrap_or("unknown"),
        readiness["completionPercent"].as_u64().unwrap_or(0)
    );
    if let Some(next_step) = readiness["nextStep"].as_str() {
        println!("Next step: {next_step}");
    }
    println!(
        "Default models: {}",
        join_json_array(&workspace["defaultModelProviders"])
    );
    println!(
        "Default channels: {}",
        join_json_array(&workspace["defaultChatPlatforms"])
    );
    println!("Configured connectors:");
    if let Some(configured) = connectors["configured"].as_object() {
        for (key, value) in configured {
            println!("  {key}={}", value.as_bool().unwrap_or(false));
        }
    }
    if profile.connector_env.is_empty() {
        println!("Local staged connector env: <none>");
    } else {
        let staged = profile
            .connector_env
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        println!("Local staged connector env: {staged}");
    }
    if args.deep {
        println!("Deep checks:");
        if let Some(models) = workspace["defaultModelProviders"].as_array() {
            if models.is_empty() {
                println!("  models: <none configured>");
            } else {
                for value in models {
                    if let Some(provider) = value.as_str() {
                        match run_model_test(
                            &client,
                            provider,
                            "Respond with exactly: OK",
                            None,
                            Some("Return only the token OK if the connector is healthy."),
                        )
                        .await
                        {
                            Ok(response) => println!(
                                "  model {}: mode={} output={}",
                                provider, response["mode"], response["outputText"]
                            ),
                            Err(error) => println!("  model {}: ERROR {}", provider, error),
                        }
                    }
                }
            }
        }
        println!(
            "  chat: skipped automatic live send to avoid side effects; use `channels send` explicitly."
        );
    }
    Ok(())
}

async fn bootstrap_session(
    client: &GatewayClient,
    bootstrap_token: &str,
    operator_name: &str,
) -> anyhow::Result<BootstrapSessionResponse> {
    client
        .post_json(
            "/api/gateway/identity/bootstrap/session",
            &json!({
                "bootstrapToken": bootstrap_token,
                "operatorName": operator_name,
            }),
        )
        .await
}

fn logout() -> anyhow::Result<()> {
    let mut profile = load_profile_or_default();
    profile.session_token = None;
    profile.operator_name = None;
    profile.bootstrap_mode = None;
    let path = save_profile(&profile)?;
    println!("Cleared local session from {}", path.display());
    Ok(())
}

fn print_local_status() -> anyhow::Result<()> {
    let path = profile_path()?;
    let profile = load_profile_or_default();
    println!("Profile path: {}", path.display());
    println!(
        "Gateway: {}",
        profile
            .gateway_base_url
            .unwrap_or_else(default_gateway_base_url)
    );
    println!(
        "Operator: {}",
        profile
            .operator_name
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!(
        "Session token: {}",
        if profile.session_token.is_some() {
            "stored"
        } else {
            "<none>"
        }
    );
    println!(
        "Node id: {}",
        profile.node_id.unwrap_or_else(|| "node-local".to_string())
    );
    println!(
        "Node display: {}",
        profile
            .node_name
            .unwrap_or_else(|| "Dawn Local Node".to_string())
    );
    println!(
        "Claim token: {}",
        if profile.claim_token.is_some() {
            "stored"
        } else {
            "<none>"
        }
    );
    if profile.session_token.is_none() {
        println!("Tip: run `dawn-node login` to start the guided CLI onboarding flow.");
    }
    Ok(())
}

async fn handle_connectors(args: ConnectorsArgs) -> anyhow::Result<()> {
    match args.command {
        ConnectorsCommand::Status { gateway } => {
            let profile = load_profile_or_default();
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client.get_json("/api/gateway/connectors/status").await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        ConnectorsCommand::Verify {
            surface,
            target,
            gateway,
        } => {
            let profile = load_profile_or_default();
            let session_token = require_session_token(&profile)?;
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client
                .post_json(
                    "/api/gateway/identity/setup-verifications",
                    &json!({
                        "sessionToken": session_token,
                        "surface": surface,
                        "target": target,
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
    }
}

fn handle_secrets(args: SecretsArgs) -> anyhow::Result<()> {
    let mut profile = load_profile_or_default();
    match args.command {
        SecretsCommand::List => {
            if profile.connector_env.is_empty() {
                println!("<no stored connector env vars>");
            } else {
                for key in profile.connector_env.keys() {
                    println!("{key}=<stored>");
                }
            }
            Ok(())
        }
        SecretsCommand::Set { key, value } => {
            profile.connector_env.insert(key, value);
            let path = save_profile(&profile)?;
            println!("Stored connector secret in {}", path.display());
            Ok(())
        }
        SecretsCommand::Unset { key } => {
            profile.connector_env.remove(&key);
            let path = save_profile(&profile)?;
            println!("Removed connector secret from {}", path.display());
            Ok(())
        }
        SecretsCommand::Export(args) => export_secrets(profile, args),
    }
}

fn handle_gateway(args: GatewayArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        GatewayCommand::Env(args) => export_secrets(profile, args),
        GatewayCommand::Start(args) => start_gateway(profile, args),
    }
}

async fn handle_models(args: ModelArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        ModelCommand::List { gateway } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let workspace: WorkspaceProfileRecord =
                client.get_json("/api/gateway/identity/workspace").await?;
            println!("{}", workspace.default_model_providers.join(", "));
            Ok(())
        }
        ModelCommand::AuthLogin { provider } => {
            handle_model_auth(&normalize_connector_target(true, &provider), ModelAuthAction::Login)
        }
        ModelCommand::AuthStatus { provider } => handle_model_auth(
            &normalize_connector_target(true, provider.as_deref().unwrap_or("openai_codex")),
            ModelAuthAction::Status,
        ),
        ModelCommand::AuthLogout { provider } => handle_model_auth(
            &normalize_connector_target(true, &provider),
            ModelAuthAction::Logout,
        ),
        ModelCommand::Connect(args) => connect_metadata_target(profile, true, args).await,
        ModelCommand::Test(args) => test_model(profile, args).await,
        ModelCommand::Add { values, gateway } => {
            update_workspace_metadata(profile, gateway, values, true, true, "model providers").await
        }
        ModelCommand::Remove { values, gateway } => {
            update_workspace_metadata(profile, gateway, values, true, false, "model providers")
                .await
        }
    }
}

enum ModelAuthAction {
    Login,
    Status,
    Logout,
}

fn handle_model_auth(provider: &str, action: ModelAuthAction) -> anyhow::Result<()> {
    if provider != "openai_codex" {
        bail!("model auth is currently only supported for openai-codex");
    }
    match action {
        ModelAuthAction::Login => {
            ensure_openai_codex_auth_ready(true)?;
            println!("OpenAI Codex login is ready.");
            Ok(())
        }
        ModelAuthAction::Status => {
            let output = run_codex_login_status()?;
            print!("{}", output);
            Ok(())
        }
        ModelAuthAction::Logout => {
            let status = new_codex_command(&["logout"])
                .status()
                .context("failed to launch `codex logout`")?;
            if !status.success() {
                bail!("`codex logout` exited with status {status}");
            }
            println!("Logged out of OpenAI Codex.");
            Ok(())
        }
    }
}

async fn handle_channels(args: ChannelArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        ChannelCommand::List { gateway } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let workspace: WorkspaceProfileRecord =
                client.get_json("/api/gateway/identity/workspace").await?;
            println!("{}", workspace.default_chat_platforms.join(", "));
            Ok(())
        }
        ChannelCommand::Connect(args) => connect_metadata_target(profile, false, args).await,
        ChannelCommand::Send(args) => send_channel_message(profile, args).await,
        ChannelCommand::Pairings(args) => handle_channel_pairings(profile, args).await,
        ChannelCommand::Add { values, gateway } => {
            update_workspace_metadata(profile, gateway, values, false, true, "chat platforms").await
        }
        ChannelCommand::Remove { values, gateway } => {
            update_workspace_metadata(profile, gateway, values, false, false, "chat platforms")
                .await
        }
    }
}

async fn handle_channel_pairings(
    profile: DawnCliProfile,
    args: ChannelPairingArgs,
) -> anyhow::Result<()> {
    match args.command {
        ChannelPairingCommand::List {
            gateway,
            platform,
            status,
            json,
        } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let mut query = Vec::new();
            if let Some(platform) = platform
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query.push(format!("platform={platform}"));
            }
            if let Some(status) = status
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query.push(format!("status={status}"));
            }
            let path = if query.is_empty() {
                "/api/gateway/ingress/pairings".to_string()
            } else {
                format!("/api/gateway/ingress/pairings?{}", query.join("&"))
            };
            let response: Value = client.get_json(&path).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else if let Some(items) = response.as_array() {
                for item in items {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        string_at_path(item, &["platform"])
                            .unwrap_or_else(|| "<unknown>".to_string()),
                        string_at_path(item, &["identityKey"])
                            .unwrap_or_else(|| "<unknown>".to_string()),
                        string_at_path(item, &["status"])
                            .unwrap_or_else(|| "<unknown>".to_string()),
                        string_at_path(item, &["pairingCode"]).unwrap_or_else(|| "-".to_string()),
                        string_at_path(item, &["senderDisplay"])
                            .or_else(|| string_at_path(item, &["senderId"]))
                            .unwrap_or_else(|| "-".to_string())
                    );
                }
            }
            Ok(())
        }
        ChannelPairingCommand::Approve {
            platform,
            identity_key,
            gateway,
            actor,
            reason,
        } => {
            decide_channel_pairing(
                profile,
                gateway,
                &platform,
                &identity_key,
                true,
                &actor,
                reason.as_deref(),
            )
            .await
        }
        ChannelPairingCommand::Reject {
            platform,
            identity_key,
            gateway,
            actor,
            reason,
        } => {
            decide_channel_pairing(
                profile,
                gateway,
                &platform,
                &identity_key,
                false,
                &actor,
                reason.as_deref(),
            )
            .await
        }
    }
}

async fn decide_channel_pairing(
    profile: DawnCliProfile,
    gateway: Option<String>,
    platform: &str,
    identity_key: &str,
    approved: bool,
    actor: &str,
    reason: Option<&str>,
) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
    let target = normalize_connector_target(false, platform);
    let action = if approved { "approve" } else { "reject" };
    let response: Value = client
        .post_json(
            &format!("/api/gateway/ingress/pairings/{target}/{identity_key}/{action}"),
            &json!({
                "actor": actor,
                "reason": reason,
            }),
        )
        .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn handle_ingress(args: IngressArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        IngressCommand::Connect(args) => connect_ingress_target(profile, args),
        IngressCommand::Status { gateway, json } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client.get_json("/api/gateway/ingress/status").await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!(
                    "supported={}",
                    string_at_path(&response, &["supportedPlatforms"])
                        .unwrap_or_else(|| "[]".to_string())
                );
                let telegram_secret = value_at_path(&response, &["telegramWebhookSecretConfigured"])
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let telegram_polling = value_at_path(&response, &["telegramPollingEnabled"])
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let telegram_mode = string_at_path(&response, &["telegramIngressMode"])
                    .unwrap_or_else(|| {
                        if telegram_polling {
                            "polling".to_string()
                        } else {
                            "webhook".to_string()
                        }
                    });
                println!(
                    "telegram\tsecret={telegram_secret}\tmode={telegram_mode}\tpolling={telegram_polling}"
                );
                for platform in ["signal", "bluebubbles"] {
                    let configured =
                        value_at_path(&response, &[&format!("{platform}CallbackSecretConfigured")])
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                    let policy = string_at_path(&response, &[&format!("{platform}DmPolicy")])
                        .unwrap_or_else(|| "open".to_string());
                    let allowlist_count =
                        value_at_path(&response, &[&format!("{platform}AllowlistCount")])
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                    let pending_pairings =
                        value_at_path(&response, &[&format!("{platform}PendingPairings")])
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                    println!(
                        "{platform}\tsecret={configured}\tdmPolicy={policy}\tallowlist={allowlist_count}\tpendingPairings={pending_pairings}"
                    );
                }
            }
            Ok(())
        }
        IngressCommand::Verify { target, gateway } => {
            let session_token = require_session_token(&profile)?;
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client
                .post_json(
                    "/api/gateway/identity/setup-verifications",
                    &json!({
                        "sessionToken": session_token,
                        "surface": "ingress",
                        "target": target,
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
    }
}

async fn update_workspace_metadata(
    profile: DawnCliProfile,
    gateway: Option<String>,
    values: Vec<String>,
    is_models: bool,
    add: bool,
    label: &str,
) -> anyhow::Result<()> {
    if values.is_empty() {
        bail!("provide at least one value to update {label}");
    }
    let session_token = require_session_token(&profile)?;
    let client = GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
    let workspace: WorkspaceProfileRecord =
        client.get_json("/api/gateway/identity/workspace").await?;

    let next_models = if is_models {
        update_values(&workspace.default_model_providers, &values, add)
    } else {
        workspace.default_model_providers.clone()
    };
    let next_channels = if is_models {
        workspace.default_chat_platforms.clone()
    } else {
        update_values(&workspace.default_chat_platforms, &values, add)
    };

    let response: WorkspaceProfileUpdateResponse = client
        .put_json(
            "/api/gateway/identity/workspace",
            &WorkspaceProfileUpdateRequest {
                session_token,
                tenant_id: workspace.tenant_id,
                project_id: workspace.project_id,
                display_name: workspace.display_name,
                region: workspace.region,
                default_model_providers: next_models,
                default_chat_platforms: next_channels,
                onboarding_status: Some(workspace.onboarding_status),
            },
        )
        .await?;

    let items = if is_models {
        response.workspace.default_model_providers
    } else {
        response.workspace.default_chat_platforms
    };
    println!("Updated {label}: {}", items.join(", "));
    Ok(())
}

async fn connect_metadata_target(
    mut profile: DawnCliProfile,
    is_models: bool,
    args: ConnectorConnectArgs,
) -> anyhow::Result<()> {
    let session_token = require_session_token(&profile)?;
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url.clone())?;
    let workspace: WorkspaceProfileRecord =
        client.get_json("/api/gateway/identity/workspace").await?;
    let target = normalize_connector_target(is_models, &args.target);
    if is_models && target == "openai_codex" {
        ensure_openai_codex_auth_ready(true)?;
    }
    let env_pairs = connector_secret_pairs(is_models, &target, &args)?;
    if env_pairs.is_empty() && !(is_models && target == "openai_codex") {
        bail!("no connector credentials were provided for {}", args.target);
    }

    for (key, value) in env_pairs {
        profile.connector_env.insert(key, value);
    }

    let next_models = if is_models {
        update_values(
            &workspace.default_model_providers,
            std::slice::from_ref(&target),
            true,
        )
    } else {
        workspace.default_model_providers.clone()
    };
    let next_channels = if is_models {
        workspace.default_chat_platforms.clone()
    } else {
        update_values(
            &workspace.default_chat_platforms,
            std::slice::from_ref(&target),
            true,
        )
    };

    let updated_workspace = upsert_workspace(
        &client,
        &session_token,
        WorkspaceProfileUpdateRequest {
            session_token: session_token.clone(),
            tenant_id: workspace.tenant_id,
            project_id: workspace.project_id,
            display_name: workspace.display_name,
            region: workspace.region,
            default_model_providers: next_models,
            default_chat_platforms: next_channels,
            onboarding_status: Some(workspace.onboarding_status),
        },
    )
    .await?;
    let path = save_profile(&profile)?;

    if is_models {
        println!(
            "Connected model provider {}. Workspace defaults: {}",
            target,
            updated_workspace.default_model_providers.join(", ")
        );
    } else {
        println!(
            "Connected chat platform {}. Workspace defaults: {}",
            target,
            updated_workspace.default_chat_platforms.join(", ")
        );
    }
    println!("Stored connector env locally in {}", path.display());
    println!(
        "Run `dawn-node secrets export --format dotenv` and restart the gateway to apply them."
    );
    Ok(())
}

fn connect_ingress_target(
    mut profile: DawnCliProfile,
    args: IngressConnectArgs,
) -> anyhow::Result<()> {
    let target = normalize_ingress_target_name(&args.target);
    let env_pairs = ingress_secret_pairs(&target, &args)?;
    if env_pairs.is_empty() {
        bail!("no ingress credentials were provided for {}", args.target);
    }
    for (key, value) in env_pairs {
        profile.connector_env.insert(key, value);
    }
    let path = save_profile(&profile)?;
    println!("Connected ingress target {target}");
    println!("Stored ingress env locally in {}", path.display());
    println!("Run `dawn-node gateway start` after exporting or staging the new env.");
    Ok(())
}

async fn fetch_agent_quote(
    client: &GatewayClient,
    card_id: &str,
    amount: Option<f64>,
    description: Option<&str>,
    remote: bool,
    quote_id: Option<&str>,
    counter_offer_amount: Option<f64>,
) -> anyhow::Result<Value> {
    let mut path = format!("/api/gateway/agent-cards/{card_id}/quote");
    let mut params = Vec::new();
    if let Some(amount) = amount {
        params.push(format!("requestedAmount={amount}"));
    }
    if let Some(description) = description.map(str::trim).filter(|value| !value.is_empty()) {
        params.push(format!("description={}", description.replace(' ', "%20")));
    }
    if remote {
        params.push("remote=true".to_string());
    }
    if let Some(quote_id) = quote_id.map(str::trim).filter(|value| !value.is_empty()) {
        params.push(format!("quoteId={quote_id}"));
    }
    if let Some(counter_offer_amount) = counter_offer_amount {
        params.push(format!("counterOfferAmount={counter_offer_amount}"));
    }
    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }
    client.get_json(&path).await
}

async fn test_model(profile: DawnCliProfile, args: ModelTestArgs) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let response = run_model_test(
        &client,
        &normalize_connector_target(true, &args.target),
        &args.input,
        args.model.as_deref(),
        args.instructions.as_deref(),
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn run_model_test(
    client: &GatewayClient,
    provider: &str,
    input: &str,
    model: Option<&str>,
    instructions: Option<&str>,
) -> anyhow::Result<Value> {
    let route_provider = model_provider_route_segment(provider);
    client
        .post_json(
            &format!("/api/gateway/connectors/model/{route_provider}/respond"),
            &json!({
                "input": input,
                "model": model,
                "instructions": instructions,
            }),
        )
        .await
}

fn model_provider_route_segment(provider: &str) -> String {
    match provider {
        "cloudflare_ai_gateway" => "cloudflare-ai-gateway".to_string(),
        "github_models" => "github-models".to_string(),
        "vercel_ai_gateway" => "vercel-ai-gateway".to_string(),
        "openai_codex" => "openai-codex".to_string(),
        _ => provider.to_string(),
    }
}

fn run_codex_login_status() -> anyhow::Result<String> {
    let output = new_codex_command(&["login", "status"])
        .output()
        .context("failed to run `codex login status`")?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        bail!(
            "`codex login status` exited with status {}: {}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            }
        );
    }
    Ok(stdout)
}

fn openai_codex_auth_ready() -> bool {
    if codex_auth_file_present() {
        return true;
    }
    run_codex_login_status()
        .map(|output| {
            output.contains("Logged in using")
                || output.to_ascii_lowercase().contains("logged in")
        })
        .unwrap_or(false)
}

fn codex_auth_file_present() -> bool {
    let base = env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            env::var_os("USERPROFILE")
                .or_else(|| env::var_os("HOME"))
                .map(PathBuf::from)
                .map(|home| home.join(".codex"))
        });
    base.map(|dir| dir.join("auth.json").exists()).unwrap_or(false)
}

fn ensure_openai_codex_auth_ready(interactive: bool) -> anyhow::Result<()> {
    if openai_codex_auth_ready() {
        return Ok(());
    }
    if !interactive {
        bail!("OpenAI Codex login is required; run `dawn-node models auth-login openai-codex`");
    }
    println!("OpenAI Codex uses your local ChatGPT login via the official `codex` CLI.");
    let status = new_codex_command(&["login"])
        .status()
        .context("failed to launch `codex login`")?;
    if !status.success() {
        bail!("`codex login` exited with status {status}");
    }
    if !openai_codex_auth_ready() {
        bail!("OpenAI Codex login did not become ready after `codex login`");
    }
    Ok(())
}

fn resolve_codex_cli_path() -> PathBuf {
    if let Ok(explicit) = env::var("CODEX_CLI_PATH") {
        let path = PathBuf::from(explicit);
        if path.exists() {
            return path;
        }
    }
    if let Some(raw_path) = env::var_os("PATH") {
        let path_dirs = env::split_paths(&raw_path).collect::<Vec<_>>();
        for candidate_name in ["codex.cmd", "codex.exe", "codex"] {
            if let Some(path) = path_dirs
                .iter()
                .map(|dir| dir.join(candidate_name))
                .find(|candidate| candidate.exists())
            {
                return path;
            }
        }
    }
    if let Ok(output) = StdCommand::new("where").arg("codex").output() {
        if output.status.success() {
            let mut candidates = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            candidates.sort_by_key(|path| {
                if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"))
                {
                    0
                } else if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                {
                    1
                } else {
                    2
                }
            });
            if let Some(first) = candidates.into_iter().find(|path| path.exists())
            {
                return first;
            }
        }
    }
    PathBuf::from("codex")
}

fn new_codex_command(args: &[&str]) -> StdCommand {
    let path = resolve_codex_cli_path();
    let is_cmd_wrapper = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"));
    let mut command = if is_cmd_wrapper {
        let mut command = StdCommand::new("cmd");
        command.arg("/C").arg(&path);
        command
    } else {
        StdCommand::new(&path)
    };
    command.args(args);
    command
}

async fn send_channel_message(
    profile: DawnCliProfile,
    args: ChannelSendArgs,
) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let response = send_channel_message_with_client(&client, &args).await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn send_channel_message_with_client(
    client: &GatewayClient,
    args: &ChannelSendArgs,
) -> anyhow::Result<Value> {
    let target = normalize_connector_target(false, &args.target);
    let (path, body) = build_channel_send_request(&target, &args)?;
    client.post_json(&path, &body).await
}

fn build_channel_send_request(
    target: &str,
    args: &ChannelSendArgs,
) -> anyhow::Result<(String, Value)> {
    let path = match target {
        "telegram" => "/api/gateway/connectors/chat/telegram/send",
        "slack" => "/api/gateway/connectors/chat/slack/send",
        "discord" => "/api/gateway/connectors/chat/discord/send",
        "mattermost" => "/api/gateway/connectors/chat/mattermost/send",
        "msteams" => "/api/gateway/connectors/chat/msteams/send",
        "whatsapp" => "/api/gateway/connectors/chat/whatsapp/send",
        "line" => "/api/gateway/connectors/chat/line/send",
        "matrix" => "/api/gateway/connectors/chat/matrix/send",
        "google_chat" => "/api/gateway/connectors/chat/google-chat/send",
        "signal" => "/api/gateway/connectors/chat/signal/send",
        "bluebubbles" => "/api/gateway/connectors/chat/bluebubbles/send",
        "feishu" => "/api/gateway/connectors/chat/feishu/send",
        "dingtalk" => "/api/gateway/connectors/chat/dingtalk/send",
        "wecom_bot" => "/api/gateway/connectors/chat/wecom/send",
        "wechat_official_account" => "/api/gateway/connectors/chat/wechat-official-account/send",
        "qq" => "/api/gateway/connectors/chat/qq/send",
        _ => bail!("unsupported chat target `{target}`"),
    };

    let body = match target {
        "telegram" => {
            let chat_id = args
                .chat_id
                .as_deref()
                .ok_or_else(|| anyhow!("telegram send requires --chat-id"))?;
            let text = require_channel_text(args, "telegram")?;
            let mut payload = json!({
                "chatId": chat_id,
                "text": text,
                "disableNotification": args.disable_notification,
            });
            if let Some(parse_mode) = args
                .parse_mode
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                payload["parseMode"] = Value::String(parse_mode.to_string());
            }
            payload
        }
        "slack" | "discord" | "mattermost" | "msteams" | "google_chat" | "feishu" | "dingtalk"
        | "wecom_bot" => {
            let text = require_channel_text(args, target)?;
            json!({
                "text": text,
            })
        }
        "whatsapp" | "line" | "matrix" => {
            let chat_id = args
                .chat_id
                .as_deref()
                .ok_or_else(|| anyhow!("{target} send requires --chat-id"))?;
            let text = require_channel_text(args, target)?;
            json!({
                "chatId": chat_id,
                "text": text,
            })
        }
        "signal" => {
            let has_group_action = args
                .group_action
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            let (attachment_name, attachment_base64, attachment_content_type) =
                encode_attachment_payload(args)?;
            let has_text = args
                .text
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !has_group_action
                && !has_text
                && attachment_base64.is_none()
                && args.reaction.is_none()
                && !args.remove_reaction
                && args.receipt_type.is_none()
            {
                bail!(
                    "signal send requires text, --attachment-file, --reaction, or --receipt-type"
                );
            }
            if args.typing.is_some() || args.mark_read {
                bail!("signal send does not support --typing or --mark-read");
            }
            let mut payload = json!({});
            if let Some(chat_id) = args
                .chat_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["chatId"] = json!(chat_id);
            }
            if let Some(account_key) = args
                .account_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["accountKey"] = json!(account_key);
            }
            if let Some(text) = args
                .text
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["text"] = json!(text);
            }
            if let Some(attachment_name) = attachment_name {
                payload["attachmentName"] = json!(attachment_name);
            }
            if let Some(attachment_base64) = attachment_base64 {
                payload["attachmentBase64"] = json!(attachment_base64);
            }
            if let Some(attachment_content_type) = attachment_content_type {
                payload["attachmentContentType"] = json!(attachment_content_type);
            }
            if let Some(reaction) = args.reaction.as_deref() {
                payload["reaction"] = json!(reaction);
            }
            if let Some(target_message_id) = args.target_message_id.as_deref() {
                payload["targetMessageId"] = json!(target_message_id);
            }
            if let Some(target_author) = args.target_author.as_deref() {
                payload["targetAuthor"] = json!(target_author);
            }
            if args.remove_reaction {
                payload["removeReaction"] = json!(true);
            }
            if let Some(receipt_type) = args.receipt_type.as_deref() {
                payload["receiptType"] = json!(receipt_type);
            }
            if let Some(group_action) = args
                .group_action
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["groupAction"] = json!(group_action);
            }
            if let Some(group_id) = args
                .group_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["groupId"] = json!(group_id);
            }
            if let Some(group_name) = args
                .group_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["groupName"] = json!(group_name);
            }
            if let Some(group_description) = args
                .group_description
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["groupDescription"] = json!(group_description);
            }
            if let Some(group_link_mode) = args
                .group_link_mode
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["groupLinkMode"] = json!(group_link_mode);
            }
            if !args.group_members.is_empty() {
                payload["groupMembers"] = json!(args.group_members);
            }
            if !args.group_admins.is_empty() {
                payload["groupAdmins"] = json!(args.group_admins);
            }
            payload
        }
        "bluebubbles" => {
            let (attachment_name, attachment_base64, attachment_content_type) =
                encode_attachment_payload(args)?;
            if attachment_base64.is_some()
                && args
                    .text
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
            {
                bail!("bluebubbles attachment send currently does not support text captions");
            }
            if !args.mark_read
                && !args.mark_unread
                && args.typing.is_none()
                && attachment_base64.is_none()
                && args.reaction.is_none()
                && args.edit_message_id.is_none()
                && args.edited_text.is_none()
                && args.unsend_message_id.is_none()
                && args.participant_action.is_none()
                && args.participant_address.is_none()
                && args.group_name.is_none()
                && args
                    .text
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
            {
                bail!(
                    "bluebubbles send requires text or a native action such as attachment, reaction, typing, mark-read, mark-unread, edit, unsend, participant, or group rename"
                );
            }
            let mut payload = json!({});
            if let Some(chat_id) = args
                .chat_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["chatId"] = json!(chat_id);
            }
            if let Some(account_key) = args
                .account_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["accountKey"] = json!(account_key);
            }
            if let Some(text) = args
                .text
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["text"] = json!(text);
            }
            if let Some(attachment_name) = attachment_name {
                payload["attachmentName"] = json!(attachment_name);
            }
            if let Some(attachment_base64) = attachment_base64 {
                payload["attachmentBase64"] = json!(attachment_base64);
            }
            if let Some(attachment_content_type) = attachment_content_type {
                payload["attachmentContentType"] = json!(attachment_content_type);
            }
            if let Some(reaction) = args.reaction.as_deref() {
                payload["reaction"] = json!(reaction);
            }
            if let Some(target_message_id) = args.target_message_id.as_deref() {
                payload["targetMessageId"] = json!(target_message_id);
            }
            if args.remove_reaction {
                payload["removeReaction"] = json!(true);
            }
            if let Some(typing) = args.typing.as_deref() {
                payload["typing"] = json!(typing);
            }
            if args.mark_read {
                payload["markRead"] = json!(true);
            }
            if args.mark_unread {
                payload["markUnread"] = json!(true);
            }
            if let Some(part_index) = args.part_index {
                payload["partIndex"] = json!(part_index);
            }
            if let Some(effect_id) = args
                .effect_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["effectId"] = json!(effect_id);
            }
            if let Some(edit_message_id) = args
                .edit_message_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["editMessageId"] = json!(edit_message_id);
            }
            if let Some(edited_text) = args
                .edited_text
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["editedText"] = json!(edited_text);
            }
            if let Some(unsend_message_id) = args
                .unsend_message_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["unsendMessageId"] = json!(unsend_message_id);
            }
            if let Some(participant_action) = args
                .participant_action
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["participantAction"] = json!(participant_action);
            }
            if let Some(participant_address) = args
                .participant_address
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["participantAddress"] = json!(participant_address);
            }
            if let Some(group_name) = args
                .group_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                payload["groupName"] = json!(group_name);
            }
            payload
        }
        "wechat_official_account" => {
            let open_id = args.chat_id.as_deref().ok_or_else(|| {
                anyhow!("wechat_official_account send requires --chat-id as openId")
            })?;
            let text = require_channel_text(args, "wechat_official_account")?;
            json!({
                "openId": open_id,
                "text": text,
            })
        }
        "qq" => {
            let recipient_id = args
                .chat_id
                .as_deref()
                .ok_or_else(|| anyhow!("qq send requires --chat-id as recipientId"))?;
            let text = require_channel_text(args, "qq")?;
            json!({
                "recipientId": recipient_id,
                "text": text,
                "targetType": args.target_type,
                "eventId": args.event_id,
                "msgId": args.msg_id,
                "msgSeq": args.msg_seq,
                "isWakeup": args.is_wakeup,
            })
        }
        _ => unreachable!(),
    };

    Ok((path.to_string(), body))
}

fn require_channel_text(args: &ChannelSendArgs, target: &str) -> anyhow::Result<String> {
    args.text
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("{target} send requires text"))
}

fn encode_attachment_payload(
    args: &ChannelSendArgs,
) -> anyhow::Result<(Option<String>, Option<String>, Option<String>)> {
    let Some(path) = args.attachment_file.as_deref() else {
        return Ok((None, None, None));
    };
    let path = PathBuf::from(path);
    let bytes =
        fs::read(&path).with_context(|| format!("failed to read attachment {}", path.display()))?;
    let name = args
        .attachment_name
        .clone()
        .or_else(|| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
        })
        .ok_or_else(|| anyhow!("unable to infer attachment name from {}", path.display()))?;
    let content_type = args
        .attachment_content_type
        .clone()
        .or_else(|| infer_attachment_content_type(&name));
    Ok((
        Some(name),
        Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
        content_type,
    ))
}

fn infer_attachment_content_type(name: &str) -> Option<String> {
    let extension = PathBuf::from(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    let content_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => return None,
    };
    Some(content_type.to_string())
}

fn export_secrets(profile: DawnCliProfile, args: SecretsExportArgs) -> anyhow::Result<()> {
    if profile.connector_env.is_empty() {
        println!("<no stored connector env vars>");
        return Ok(());
    }
    let rendered = render_secret_block(&profile.connector_env, &args.format)?;
    if let Some(path) = args.path {
        let path = PathBuf::from(path);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, rendered.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        println!("Wrote connector env block to {}", path.display());
    } else {
        println!("{rendered}");
    }
    Ok(())
}

fn start_gateway(profile: DawnCliProfile, args: GatewayStartArgs) -> anyhow::Result<()> {
    let dawn_core_dir = resolve_dawn_core_dir(args.cwd.as_deref())?;
    let mut command = StdCommand::new("cargo");
    command.current_dir(&dawn_core_dir);
    command.arg("run");
    if args.release {
        command.arg("--release");
    }
    for (key, value) in &profile.connector_env {
        command.env(key, value);
    }
    if let Some(gateway) = profile.gateway_base_url.as_deref() {
        command.env("DAWN_PUBLIC_BASE_URL", gateway);
    }
    println!("Starting DawnCore in {}", dawn_core_dir.display());
    if !profile.connector_env.is_empty() {
        let keys = profile
            .connector_env
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        println!("Injecting connector env: {keys}");
    }
    let status = command
        .status()
        .with_context(|| format!("failed to start DawnCore in {}", dawn_core_dir.display()))?;
    if !status.success() {
        bail!("DawnCore exited with status {status}");
    }
    Ok(())
}

async fn handle_skills(args: SkillsArgs) -> anyhow::Result<()> {
    match args.command {
        SkillCommand::Search(search) => search_skills(search).await,
        SkillCommand::Install(install) => install_skill(install).await,
    }
}

async fn handle_agents(args: AgentsArgs) -> anyhow::Result<()> {
    match args.command {
        AgentCommand::Search(search) => search_agents(search).await,
        AgentCommand::Install(install) => install_agent(install).await,
        AgentCommand::Quote(quote) => quote_agent(quote).await,
        AgentCommand::Invoke(invoke) => invoke_agent(invoke).await,
    }
}

async fn search_skills(args: SkillSearchArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    if args.federated {
        let catalog = fetch_federated_catalog(&client, args.query.as_deref(), args.all).await?;
        for skill in catalog.skills {
            println!(
                "{}@{} [{}:{}] signed={} active={} capabilities={}",
                skill.entry.skill_id,
                skill.entry.version,
                skill.source_display_name,
                skill.source_peer_id,
                skill.entry.signed,
                skill.entry.active,
                skill.entry.capabilities.join(", ")
            );
            println!("  {}", skill.entry.display_name);
            if let Some(description) = skill.entry.description.as_deref() {
                println!("  {}", description);
            }
        }
    } else {
        let catalog = fetch_local_catalog(&client, args.query.as_deref(), args.all).await?;
        for skill in catalog.skills {
            println!(
                "{}@{} signed={} active={} capabilities={}",
                skill.skill_id,
                skill.version,
                skill.signed,
                skill.active,
                skill.capabilities.join(", ")
            );
            println!("  {}", skill.display_name);
            if let Some(description) = skill.description.as_deref() {
                println!("  {}", description);
            }
        }
    }
    Ok(())
}

async fn install_skill(args: SkillInstallArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let selected = select_skill_entry(&client, &args).await?;
    let response: SkillActivationResponse = client
        .post_json(
            &selected.install_url,
            &InstallSkillPackageRequest {
                package_url: selected.package_url.clone(),
                activate: Some(!args.no_activate),
                allow_unsigned: Some(args.allow_unsigned),
            },
        )
        .await?;

    println!(
        "Installed skill {}@{} activated={} sourceKind={} active={}",
        response.skill.skill_id,
        response.skill.version,
        response.activated,
        response.skill.source_kind,
        response.skill.active
    );
    Ok(())
}

async fn search_agents(args: AgentSearchArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    if args.federated {
        let catalog =
            fetch_federated_catalog_kind(&client, args.query.as_deref(), "agent", args.all).await?;
        for agent in catalog.agent_cards {
            println!(
                "{} [{}:{}] published={} hosted={} models={} chats={}",
                agent.entry.card_id,
                agent.source_display_name,
                agent.source_peer_id,
                agent.entry.published,
                agent.entry.locally_hosted,
                agent.entry.model_providers.join(", "),
                agent.entry.chat_platforms.join(", ")
            );
            println!("  {}", agent.entry.name);
            println!("  {}", agent.entry.description);
        }
    } else {
        let catalog =
            fetch_local_catalog_kind(&client, args.query.as_deref(), "agent", args.all).await?;
        for agent in catalog.agent_cards {
            println!(
                "{} published={} hosted={} models={} chats={}",
                agent.card_id,
                agent.published,
                agent.locally_hosted,
                agent.model_providers.join(", "),
                agent.chat_platforms.join(", ")
            );
            println!("  {}", agent.name);
            println!("  {}", agent.description);
        }
    }
    Ok(())
}

async fn install_agent(args: AgentInstallArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let selected = select_agent_entry(&client, &args).await?;
    let install_url = selected
        .install_url
        .clone()
        .ok_or_else(|| anyhow!("selected agent card is missing installUrl"))?;
    let card_url = selected
        .card_url
        .clone()
        .unwrap_or_else(|| selected.url.clone());
    let response: Value = client
        .post_json(
            &install_url,
            &InstallAgentCardRequest {
                card_url,
                card_id: Some(selected.card_id.clone()),
                published: Some(selected.published),
                regions: None,
                languages: None,
                model_providers: Some(selected.model_providers.clone()),
                chat_platforms: Some(selected.chat_platforms.clone()),
                payment_roles: Some(selected.payment_roles.clone()),
            },
        )
        .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn quote_agent(args: AgentQuoteArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let response = fetch_agent_quote(
        &client,
        &args.card_id,
        args.amount,
        args.description.as_deref(),
        args.remote,
        args.quote_id.as_deref(),
        args.counter_offer_amount,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn execute_delegate_request(
    client: &GatewayClient,
    request: DelegateExecutionRequest,
) -> anyhow::Result<DelegateExecution> {
    let (settlement, quote) = resolve_delegate_settlement(
        client,
        &request.card_id,
        request.mandate_id,
        request.amount,
        request.settlement_description,
        request.quote_id,
        request.counter_offer_amount,
        request.remote_quote,
    )
    .await?;
    let invocation = invoke_agent_card_request(
        client,
        &request.card_id,
        &InvokeAgentCardRequest {
            name: request.name,
            instruction: request.instruction,
            parent_task_id: None,
            await_completion: Some(request.await_completion),
            timeout_seconds: request.timeout_seconds,
            poll_interval_ms: None,
            settlement,
        },
    )
    .await?;
    Ok(DelegateExecution { quote, invocation })
}

async fn resolve_delegate_settlement(
    client: &GatewayClient,
    card_id: &str,
    mandate_id: Option<String>,
    amount: Option<f64>,
    settlement_description: Option<String>,
    quote_id: Option<String>,
    counter_offer_amount: Option<f64>,
    remote_quote: bool,
) -> anyhow::Result<(Option<RemoteSettlementRequest>, Option<Value>)> {
    match (mandate_id, amount, settlement_description) {
        (Some(mandate_id), Some(amount), Some(description)) => {
            let mut effective_quote_id = quote_id;
            let quote = if effective_quote_id.is_none() {
                let quote = fetch_agent_quote(
                    client,
                    card_id,
                    Some(amount),
                    Some(&description),
                    remote_quote,
                    None,
                    counter_offer_amount,
                )
                .await?;
                effective_quote_id = quote
                    .get("quoteId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                Some(quote)
            } else {
                None
            };
            Ok((
                Some(RemoteSettlementRequest {
                    mandate_id,
                    amount,
                    description,
                    quote_id: effective_quote_id,
                    counter_offer_amount,
                }),
                quote,
            ))
        }
        (None, None, None) => Ok((None, None)),
        _ => bail!(
            "to delegate with settlement, provide --mandate-id, --amount, and --settlement-description together"
        ),
    }
}

async fn invoke_agent_card_request(
    client: &GatewayClient,
    card_id: &str,
    request: &InvokeAgentCardRequest,
) -> anyhow::Result<Value> {
    client
        .post_json(
            &format!("/api/gateway/agent-cards/{card_id}/invoke"),
            request,
        )
        .await
}

async fn invoke_agent(args: AgentInvokeArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let settlement = match (
        args.mandate_id.clone(),
        args.settlement_amount,
        args.settlement_description.clone(),
    ) {
        (Some(mandate_id), Some(amount), Some(description)) => Some(RemoteSettlementRequest {
            mandate_id,
            amount,
            description,
            quote_id: args.quote_id.clone(),
            counter_offer_amount: None,
        }),
        (None, None, None) => None,
        _ => {
            bail!(
                "to send settlement data, provide --mandate-id, --settlement-amount, and --settlement-description together"
            )
        }
    };
    let response = invoke_agent_card_request(
        &client,
        &args.card_id,
        &InvokeAgentCardRequest {
            name: args.name,
            instruction: args.instruction,
            parent_task_id: None,
            await_completion: Some(args.await_completion),
            timeout_seconds: args.timeout_seconds,
            poll_interval_ms: None,
            settlement,
        },
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn delegate(args: DelegateArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let execution = execute_delegate_request(
        &client,
        DelegateExecutionRequest {
            card_id: args.card_id,
            name: args.name,
            instruction: args.instruction,
            await_completion: args.await_completion,
            timeout_seconds: args.timeout_seconds,
            mandate_id: args.mandate_id,
            amount: args.amount,
            settlement_description: args.settlement_description,
            quote_id: args.quote_id,
            counter_offer_amount: args.counter_offer_amount,
            remote_quote: args.remote_quote,
        },
    )
    .await?;

    if let Some(quote) = execution.quote.as_ref().filter(|_| !args.json) {
        println!("{}", format_quote_summary(quote));
    }

    println!("{}", serde_json::to_string_pretty(&execution.invocation)?);
    Ok(())
}

async fn chat(args: ChatArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let execution = execute_delegate_request(
        &client,
        DelegateExecutionRequest {
            card_id: args.card_id.clone(),
            name: args.name,
            instruction: args.instruction,
            await_completion: true,
            timeout_seconds: args.timeout_seconds,
            mandate_id: args.mandate_id,
            amount: args.amount,
            settlement_description: args.settlement_description,
            quote_id: args.quote_id,
            counter_offer_amount: args.counter_offer_amount,
            remote_quote: args.remote_quote,
        },
    )
    .await?;

    if let Some(quote) = execution
        .quote
        .as_ref()
        .filter(|_| args.print_quote || !args.json)
    {
        if args.json {
            println!("{}", serde_json::to_string_pretty(quote)?);
        } else {
            println!("{}", format_quote_summary(quote));
        }
    }

    let reply_text = build_chat_reply(&args.card_id, &execution.invocation);
    let channel_response = send_channel_message_with_client(
        &client,
        &ChannelSendArgs {
            target: args.target.clone(),
            text: Some(reply_text.clone()),
            gateway: None,
            chat_id: args.chat_id,
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: args.parse_mode,
            disable_notification: args.disable_notification,
            target_type: args.target_type,
            event_id: args.event_id,
            msg_id: args.msg_id,
            msg_seq: args.msg_seq,
            is_wakeup: args.is_wakeup,
        },
    )
    .await?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "quote": execution.quote,
                "invocation": execution.invocation,
                "replyText": reply_text,
                "channelResponse": channel_response,
            }))?
        );
    } else {
        println!(
            "Sent agent result to {}. Invocation: {}",
            normalize_connector_target(false, &args.target),
            format_delegate_summary(&execution.invocation)
        );
    }
    Ok(())
}

fn format_quote_summary(quote: &Value) -> String {
    format!(
        "Quote {} requestedAmount={} quotedAmount={} source={}",
        string_at_path(quote, &["quoteId"]).unwrap_or_else(|| "<none>".to_string()),
        value_at_path(quote, &["requestedAmount"])
            .map(render_inline_json_value)
            .unwrap_or_else(|| "null".to_string()),
        value_at_path(quote, &["quotedAmount"])
            .map(render_inline_json_value)
            .unwrap_or_else(|| "null".to_string()),
        string_at_path(quote, &["quoteSource"]).unwrap_or_else(|| "unknown".to_string()),
    )
}

fn format_delegate_summary(response: &Value) -> String {
    let invocation_id = string_at_path(response, &["invocation", "invocationId"])
        .unwrap_or_else(|| "<unknown>".to_string());
    let status = string_at_path(response, &["invocation", "status"])
        .unwrap_or_else(|| "unknown".to_string());
    let remote_status = string_at_path(response, &["remoteStatus"]);
    match remote_status.filter(|value| value != &status) {
        Some(remote_status) => {
            format!("{invocation_id} status={status} remoteStatus={remote_status}")
        }
        None => format!("{invocation_id} status={status}"),
    }
}

fn build_chat_reply(card_id: &str, response: &Value) -> String {
    const CHAT_REPLY_MAX_CHARS: usize = 3000;
    let invocation_status = string_at_path(response, &["invocation", "status"])
        .unwrap_or_else(|| "unknown".to_string());

    let mut lines = vec![format!(
        "Agent {} finished with status {}",
        card_id, invocation_status
    )];

    if let Some(remote_status) =
        string_at_path(response, &["remoteStatus"]).filter(|value| value != &invocation_status)
    {
        lines.push(format!("Remote status: {remote_status}"));
    }
    if let Some(remote_task_id) = string_at_path(response, &["invocation", "remoteTaskId"])
        .or_else(|| string_at_path(response, &["invocation", "response", "task", "taskId"]))
    {
        lines.push(format!("Remote task: {remote_task_id}"));
    }
    if let Some(settlement_status) = string_at_path(response, &["settlement", "status"]) {
        let amount = value_at_path(response, &["settlement", "amount"])
            .map(render_inline_json_value)
            .unwrap_or_else(|| "unknown".to_string());
        lines.push(format!("Settlement: {settlement_status} amount={amount}"));
    }

    let body = value_at_path(response, &["invocation", "response"])
        .and_then(extract_text_from_value)
        .or_else(|| {
            string_at_path(response, &["invocation", "error"])
                .map(|error| format!("Invocation error: {error}"))
        })
        .unwrap_or_else(|| "No textual result was returned by the delegated agent.".to_string());

    lines.push(String::new());
    lines.push(body);
    truncate_text(&lines.join("\n"), CHAT_REPLY_MAX_CHARS)
}

fn extract_text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(values) => {
            let fragments = values
                .iter()
                .filter_map(extract_text_from_value)
                .take(4)
                .collect::<Vec<_>>();
            if fragments.is_empty() {
                None
            } else {
                Some(fragments.join("\n"))
            }
        }
        Value::Object(_) => {
            for path in [
                &["text"][..],
                &["message"][..],
                &["content"][..],
                &["output"][..],
                &["result"][..],
                &["response"][..],
                &["answer"][..],
                &["summary"][..],
                &["detail"][..],
                &["final"][..],
                &["body"][..],
            ] {
                if let Some(found) = value_at_path(value, path).and_then(extract_text_from_value) {
                    return Some(found);
                }
            }
            let task_id = string_at_path(value, &["task", "taskId"]);
            let task_status = string_at_path(value, &["task", "status"]);
            if task_id.is_some() || task_status.is_some() {
                return Some(match (task_id, task_status) {
                    (Some(task_id), Some(status)) => {
                        format!("Remote task {task_id} status={status}")
                    }
                    (Some(task_id), None) => format!("Remote task {task_id}"),
                    (None, Some(status)) => format!("Remote task status={status}"),
                    (None, None) => unreachable!(),
                });
            }
            None
        }
        _ => Some(render_inline_json_value(value)),
    }
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn string_at_path(value: &Value, path: &[&str]) -> Option<String> {
    value_at_path(value, path).and_then(|value| {
        if value.is_null() {
            None
        } else {
            Some(render_inline_json_value(value))
        }
    })
}

fn render_inline_json_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => "null".to_string(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut iter = text.chars();
    let truncated = iter.by_ref().take(max_chars).collect::<String>();
    if iter.next().is_some() {
        format!("{truncated}\n...[truncated]")
    } else {
        truncated
    }
}

async fn handle_tasks(args: TasksArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        TaskCommand::List { gateway } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client.get_json("/api/a2a/tasks").await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        TaskCommand::Create {
            name,
            instruction,
            gateway,
            parent_task_id,
        } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client
                .post_json(
                    "/api/a2a/task",
                    &json!({
                        "name": name,
                        "instruction": instruction,
                        "parentTaskId": parent_task_id,
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
    }
}

async fn handle_ap2(args: Ap2Args) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        Ap2Command::List { gateway } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client.get_json("/api/ap2/transactions").await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        Ap2Command::Prepare(args) => prepare_ap2_payment(args, profile).await,
        Ap2Command::Sign(args) => sign_ap2_payment(args, profile).await,
        Ap2Command::Signer(args) => handle_ap2_signer(args, profile),
        Ap2Command::Request {
            mandate_id,
            amount,
            description,
            gateway,
            task_id,
        } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let response: Value = client
                .post_json(
                    "/api/ap2/authorize",
                    &json!({
                        "taskId": task_id,
                        "mandateId": mandate_id,
                        "amount": amount,
                        "description": description,
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        Ap2Command::Approve(args) => approve_ap2_payment(args, profile).await,
        Ap2Command::ApproveLocal(args) => approve_ap2_payment_locally(args, profile).await,
        Ap2Command::Reject(args) => reject_ap2_payment(args, profile).await,
    }
}

async fn prepare_ap2_payment(args: Ap2PrepareArgs, profile: DawnCliProfile) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "payment", &args.transaction_id).await?;
    let payment = fetch_payment_record(&client, &args.transaction_id).await?;
    let payload = build_ap2_signature_payload(&payment);
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "approvalId": approval.approval_id,
                "transactionId": payment.transaction_id,
                "taskId": payment.task_id,
                "mandateId": payment.mandate_id,
                "amount": payment.amount,
                "description": payment.description,
                "status": payment.status,
                "verificationMessage": payment.verification_message,
                "mcuPublicDid": payment.mcu_public_did,
                "payload": payload,
            }))?
        );
    } else {
        println!("AP2 transaction: {}", payment.transaction_id);
        println!("Pending approval: {}", approval.approval_id);
        println!("Task: {}", payment.task_id.as_deref().unwrap_or("<none>"));
        println!("Mandate: {}", payment.mandate_id);
        println!("Amount: {:.4}", payment.amount);
        println!("Description: {}", payment.description);
        println!("Status: {}", payment.status);
        println!("Payload:");
        println!("{payload}");
    }
    Ok(())
}

async fn sign_ap2_payment(args: Ap2SignArgs, profile: DawnCliProfile) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "payment", &args.transaction_id).await?;
    let payment = fetch_payment_record(&client, &args.transaction_id).await?;
    let payload = build_ap2_signature_payload(&payment);
    let signed = sign_ap2_payload(&profile, &payload, args.seed_hex.as_deref())?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "approvalId": approval.approval_id,
                "transactionId": payment.transaction_id,
                "payload": payload,
                "signer": signed.signer_label,
                "mcuPublicDid": signed.mcu_public_did,
                "mcuSignature": signed.mcu_signature,
            }))?
        );
    } else {
        println!("AP2 transaction: {}", payment.transaction_id);
        println!("Pending approval: {}", approval.approval_id);
        println!("Signer: {}", signed.signer_label);
        println!("Payload:");
        println!("{payload}");
        println!("MCU DID: {}", signed.mcu_public_did);
        println!("Signature: {}", signed.mcu_signature);
    }
    Ok(())
}

fn handle_ap2_signer(args: Ap2SignerArgs, mut profile: DawnCliProfile) -> anyhow::Result<()> {
    match args.command {
        Ap2SignerCommand::Status => {
            print_ap2_signer_status(&profile);
            Ok(())
        }
        Ap2SignerCommand::Local { seed_hex } => {
            profile
                .connector_env
                .insert("DAWN_AP2_SIGNER_MODE".to_string(), "local_seed".to_string());
            if let Some(seed_hex) = seed_hex.as_deref() {
                profile.connector_env.insert(
                    "DAWN_AP2_MCU_SEED_HEX".to_string(),
                    normalize_hex_string(seed_hex)?,
                );
            }
            let path = save_profile(&profile)?;
            println!("Configured AP2 signer mode: local_seed");
            println!("Profile saved: {}", path.display());
            print_ap2_signer_status(&profile);
            Ok(())
        }
        Ap2SignerCommand::Serial(args) => {
            profile
                .connector_env
                .insert("DAWN_AP2_SIGNER_MODE".to_string(), "serial".to_string());
            profile.connector_env.insert(
                "DAWN_AP2_SERIAL_PORT".to_string(),
                args.port.trim().to_string(),
            );
            profile
                .connector_env
                .insert("DAWN_AP2_SERIAL_BAUD".to_string(), args.baud.to_string());
            profile.connector_env.insert(
                "DAWN_AP2_SERIAL_PROTOCOL".to_string(),
                args.protocol.trim().to_string(),
            );
            if let Some(mock_seed_hex) = args.mock_seed_hex.as_deref() {
                profile.connector_env.insert(
                    "DAWN_AP2_SERIAL_MOCK_SEED_HEX".to_string(),
                    normalize_hex_string(mock_seed_hex)?,
                );
            }
            let path = save_profile(&profile)?;
            println!("Configured AP2 signer mode: serial");
            println!("Profile saved: {}", path.display());
            print_ap2_signer_status(&profile);
            Ok(())
        }
        Ap2SignerCommand::Clear => {
            for key in [
                "DAWN_AP2_SIGNER_MODE",
                "DAWN_AP2_SERIAL_PORT",
                "DAWN_AP2_SERIAL_BAUD",
                "DAWN_AP2_SERIAL_PROTOCOL",
                "DAWN_AP2_SERIAL_MOCK_SEED_HEX",
                "DAWN_AP2_MCU_SEED_HEX",
            ] {
                profile.connector_env.remove(key);
            }
            let path = save_profile(&profile)?;
            println!("Cleared AP2 signer configuration");
            println!("Profile saved: {}", path.display());
            Ok(())
        }
    }
}

async fn approve_ap2_payment(args: Ap2ApproveArgs, profile: DawnCliProfile) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "payment", &args.transaction_id).await?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| "desktop-operator".to_string());
    let response = decide_approval_with_client(
        &client,
        approval.approval_id.trim(),
        "approve",
        &actor,
        args.reason.as_deref(),
        Some(args.mcu_public_did.as_str()),
        Some(args.mcu_signature.as_str()),
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "{}",
            format_payment_approval_summary("approve", &args.transaction_id, &response)
        );
    }
    Ok(())
}

async fn approve_ap2_payment_locally(
    args: Ap2ApproveLocalArgs,
    profile: DawnCliProfile,
) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "payment", &args.transaction_id).await?;
    let payment = fetch_payment_record(&client, &args.transaction_id).await?;
    let payload = build_ap2_signature_payload(&payment);
    let signed = sign_ap2_payload(&profile, &payload, args.seed_hex.as_deref())?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| "desktop-operator".to_string());
    let response = decide_approval_with_client(
        &client,
        approval.approval_id.trim(),
        "approve",
        &actor,
        args.reason.as_deref(),
        Some(signed.mcu_public_did.as_str()),
        Some(signed.mcu_signature.as_str()),
    )
    .await?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "approvalId": approval.approval_id,
                "transactionId": payment.transaction_id,
                "payload": payload,
                "signer": signed.signer_label,
                "mcuPublicDid": signed.mcu_public_did,
                "mcuSignature": signed.mcu_signature,
                "decision": response,
            }))?
        );
    } else {
        println!(
            "{}",
            format_payment_approval_summary("approve", &args.transaction_id, &response)
        );
        println!("Signer: {}", signed.signer_label);
        println!("Payload: {payload}");
        println!("MCU DID: {}", signed.mcu_public_did);
    }
    Ok(())
}

async fn reject_ap2_payment(args: Ap2RejectArgs, profile: DawnCliProfile) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "payment", &args.transaction_id).await?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| "desktop-operator".to_string());
    let response = decide_approval_with_client(
        &client,
        approval.approval_id.trim(),
        "reject",
        &actor,
        args.reason.as_deref(),
        None,
        None,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "{}",
            format_payment_approval_summary("reject", &args.transaction_id, &response)
        );
    }
    Ok(())
}

async fn handle_approvals(args: ApprovalsArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        ApprovalCommand::List { gateway, status } => {
            let client =
                GatewayClient::new(resolve_gateway_base_url(gateway.as_deref(), &profile))?;
            let path = if let Some(status) = status.as_deref() {
                format!("/api/gateway/approvals?status={status}")
            } else {
                "/api/gateway/approvals".to_string()
            };
            let response: Value = client.get_json(&path).await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        ApprovalCommand::Decide(args) => decide_approval(args, profile).await,
    }
}

async fn decide_approval(args: ApprovalDecideArgs, profile: DawnCliProfile) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| "desktop-operator".to_string());
    let response = decide_approval_with_client(
        &client,
        args.approval_id.trim(),
        &args.decision,
        &actor,
        args.reason.as_deref(),
        args.mcu_public_did.as_deref(),
        args.mcu_signature.as_deref(),
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn decide_approval_with_client(
    client: &GatewayClient,
    approval_id: &str,
    decision: &str,
    actor: &str,
    reason: Option<&str>,
    mcu_public_did: Option<&str>,
    mcu_signature: Option<&str>,
) -> anyhow::Result<Value> {
    client
        .post_json(
            &format!("/api/gateway/approvals/{}/decision", approval_id.trim()),
            &json!({
                "actor": actor,
                "decision": decision,
                "reason": reason,
                "mcuPublicDid": mcu_public_did,
                "mcuSignature": mcu_signature,
            }),
        )
        .await
}

async fn list_approval_requests(
    client: &GatewayClient,
    status: Option<&str>,
) -> anyhow::Result<Vec<ApprovalRequestSummary>> {
    let path = if let Some(status) = status {
        format!("/api/gateway/approvals?status={status}")
    } else {
        "/api/gateway/approvals".to_string()
    };
    client.get_json(&path).await
}

async fn find_pending_approval_by_reference(
    client: &GatewayClient,
    kind: &str,
    reference_id: &str,
) -> anyhow::Result<ApprovalRequestSummary> {
    let approvals = list_approval_requests(client, Some("pending")).await?;
    find_pending_approval_record(&approvals, kind, reference_id)
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "no pending {} approval found for reference {}",
                kind,
                reference_id.trim()
            )
        })
}

async fn fetch_payment_record(
    client: &GatewayClient,
    transaction_id: &str,
) -> anyhow::Result<PaymentRecordSummary> {
    client
        .get_json(&format!("/api/ap2/transactions/{}", transaction_id.trim()))
        .await
}

fn build_ap2_signature_payload(payment: &PaymentRecordSummary) -> String {
    format!(
        "{}:{}:{:.4}:{}",
        payment.transaction_id, payment.mandate_id, payment.amount, payment.description
    )
}

fn print_ap2_signer_status(profile: &DawnCliProfile) {
    let mode = profile
        .connector_env
        .get("DAWN_AP2_SIGNER_MODE")
        .map(String::as_str)
        .unwrap_or("unset");
    let serial_port = profile
        .connector_env
        .get("DAWN_AP2_SERIAL_PORT")
        .map(String::as_str)
        .unwrap_or("<none>");
    let serial_baud = profile
        .connector_env
        .get("DAWN_AP2_SERIAL_BAUD")
        .map(String::as_str)
        .unwrap_or("115200");
    let serial_protocol = profile
        .connector_env
        .get("DAWN_AP2_SERIAL_PROTOCOL")
        .map(String::as_str)
        .unwrap_or("dawn-ap2-v1");
    println!("AP2 signer mode: {mode}");
    println!(
        "Local seed staged: {}",
        if profile.connector_env.contains_key("DAWN_AP2_MCU_SEED_HEX") {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "Serial signer config: port={serial_port} baud={serial_baud} protocol={serial_protocol}"
    );
    println!(
        "Serial mock seed staged: {}",
        if profile
            .connector_env
            .contains_key("DAWN_AP2_SERIAL_MOCK_SEED_HEX")
        {
            "yes"
        } else {
            "no"
        }
    );
    if mode == "serial" {
        println!(
            "Hardware note: real MCU serial transport is not implemented yet; use --mock-seed-hex to simulate approvals until the device arrives."
        );
    }
}

fn sign_ap2_payload(
    profile: &DawnCliProfile,
    payload: &str,
    explicit_seed_hex: Option<&str>,
) -> anyhow::Result<SignedAp2Payload> {
    if let Some(seed_hex) = explicit_seed_hex
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return sign_ap2_payload_with_seed(seed_hex, "override-seed", payload);
    }

    let mode = profile
        .connector_env
        .get("DAWN_AP2_SIGNER_MODE")
        .map(String::as_str)
        .unwrap_or("");
    match mode {
        "serial" => {
            let port = profile
                .connector_env
                .get("DAWN_AP2_SERIAL_PORT")
                .map(String::as_str)
                .unwrap_or("<unset>");
            let baud = profile
                .connector_env
                .get("DAWN_AP2_SERIAL_BAUD")
                .map(String::as_str)
                .unwrap_or("115200");
            let protocol = profile
                .connector_env
                .get("DAWN_AP2_SERIAL_PROTOCOL")
                .map(String::as_str)
                .unwrap_or("dawn-ap2-v1");
            if let Some(mock_seed_hex) = profile
                .connector_env
                .get("DAWN_AP2_SERIAL_MOCK_SEED_HEX")
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                return sign_ap2_payload_with_seed(
                    mock_seed_hex,
                    &format!("serial-mock:{port}@{baud}/{protocol}"),
                    payload,
                );
            }
            bail!(
                "serial signer is configured for {}@{} ({}), but the MCU transport is not available yet; stage DAWN_AP2_SERIAL_MOCK_SEED_HEX or switch to local_seed mode",
                port,
                baud,
                protocol
            )
        }
        "local_seed" | "" => {
            let seed_hex = resolve_ap2_mcu_seed_hex(None, profile)?;
            sign_ap2_payload_with_seed(&seed_hex, "local-seed", payload)
        }
        other => bail!("unsupported AP2 signer mode `{other}`"),
    }
}

fn sign_ap2_payload_with_seed(
    seed_hex: &str,
    signer_label: &str,
    payload: &str,
) -> anyhow::Result<SignedAp2Payload> {
    let signing_key = signing_key_from_seed_hex(seed_hex)?;
    Ok(SignedAp2Payload {
        signer_label: signer_label.to_string(),
        mcu_public_did: format!(
            "did:dawn:mcu:{}",
            hex::encode(signing_key.verifying_key().as_bytes())
        ),
        mcu_signature: hex::encode(signing_key.sign(payload.as_bytes()).to_bytes()),
    })
}

fn resolve_ap2_mcu_seed_hex(
    explicit_seed_hex: Option<&str>,
    profile: &DawnCliProfile,
) -> anyhow::Result<String> {
    if let Some(seed_hex) = explicit_seed_hex
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return normalize_hex_string(seed_hex);
    }
    if let Some(seed_hex) = profile
        .connector_env
        .get("DAWN_AP2_MCU_SEED_HEX")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return normalize_hex_string(seed_hex);
    }
    if let Ok(seed_hex) = env::var("DAWN_AP2_MCU_SEED_HEX") {
        if !seed_hex.trim().is_empty() {
            return normalize_hex_string(&seed_hex);
        }
    }
    bail!(
        "missing AP2 MCU seed; pass --seed-hex or store DAWN_AP2_MCU_SEED_HEX with `dawn-node secrets set`"
    )
}

fn signing_key_from_seed_hex(seed_hex: &str) -> anyhow::Result<SigningKey> {
    let seed_bytes = decode_fixed_hex::<32>(seed_hex, "AP2 MCU seed")?;
    Ok(SigningKey::from_bytes(&seed_bytes))
}

fn decode_fixed_hex<const N: usize>(raw: &str, label: &str) -> anyhow::Result<[u8; N]> {
    let bytes = hex::decode(normalize_hex_string(raw)?)
        .with_context(|| format!("{label} must be valid hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("{label} must be {N} bytes"))
}

fn normalize_hex_string(raw: &str) -> anyhow::Result<String> {
    Ok(hex::encode(
        hex::decode(raw.trim()).context("value must be valid hex")?,
    ))
}

fn find_pending_approval_record<'a>(
    approvals: &'a [ApprovalRequestSummary],
    kind: &str,
    reference_id: &str,
) -> Option<&'a ApprovalRequestSummary> {
    let normalized_kind = kind.trim().to_ascii_lowercase();
    let normalized_reference = reference_id.trim();
    approvals.iter().find(|approval| {
        approval.status == "pending"
            && approval.kind == normalized_kind
            && approval.reference_id == normalized_reference
    })
}

fn format_payment_approval_summary(
    decision: &str,
    transaction_id: &str,
    response: &Value,
) -> String {
    let approval_id = string_at_path(response, &["approval", "approvalId"])
        .unwrap_or_else(|| "<unknown>".to_string());
    let approval_status =
        string_at_path(response, &["approval", "status"]).unwrap_or_else(|| "unknown".to_string());
    let payment_status = string_at_path(response, &["paymentResponse", "status"])
        .or_else(|| string_at_path(response, &["payment", "status"]))
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "{} AP2 payment {}. approval={} approvalStatus={} paymentStatus={}",
        decision_past_tense(decision),
        transaction_id.trim(),
        approval_id,
        approval_status,
        payment_status
    )
}

fn format_node_command_approval_summary(
    decision: &str,
    command_id: &str,
    response: &Value,
) -> String {
    let approval_id = string_at_path(response, &["approval", "approvalId"])
        .unwrap_or_else(|| "<unknown>".to_string());
    let approval_status =
        string_at_path(response, &["approval", "status"]).unwrap_or_else(|| "unknown".to_string());
    let command_status = string_at_path(response, &["nodeCommand", "status"])
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "{} node command {}. approval={} approvalStatus={} commandStatus={}",
        decision_past_tense(decision),
        command_id.trim(),
        approval_id,
        approval_status,
        command_status
    )
}

fn decision_past_tense(decision: &str) -> String {
    match decision.trim().to_ascii_lowercase().as_str() {
        "approve" => "Approved".to_string(),
        "reject" => "Rejected".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => format!("{}{}d", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
    }
}

async fn handle_node(args: NodeArgs) -> anyhow::Result<()> {
    match args.command {
        NodeCommand::Claim(claim) => create_node_claim(claim).await,
        NodeCommand::TrustSelf(args) => trust_self_node(args).await,
    }
}

async fn handle_node_commands(args: NodeCommandOps) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        NodeCommandAction::Approve(args) => approve_node_command(args, profile).await,
        NodeCommandAction::Reject(args) => reject_node_command(args, profile).await,
    }
}

async fn approve_node_command(
    args: NodeCommandApproveArgs,
    profile: DawnCliProfile,
) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "node_command", &args.command_id).await?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| "desktop-operator".to_string());
    let response = decide_approval_with_client(
        &client,
        approval.approval_id.trim(),
        "approve",
        &actor,
        args.reason.as_deref(),
        None,
        None,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "{}",
            format_node_command_approval_summary("approve", &args.command_id, &response)
        );
    }
    Ok(())
}

async fn reject_node_command(
    args: NodeCommandRejectArgs,
    profile: DawnCliProfile,
) -> anyhow::Result<()> {
    let client = GatewayClient::new(resolve_gateway_base_url(args.gateway.as_deref(), &profile))?;
    let approval =
        find_pending_approval_by_reference(&client, "node_command", &args.command_id).await?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| "desktop-operator".to_string());
    let response = decide_approval_with_client(
        &client,
        approval.approval_id.trim(),
        "reject",
        &actor,
        args.reason.as_deref(),
        None,
        None,
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "{}",
            format_node_command_approval_summary("reject", &args.command_id, &response)
        );
    }
    Ok(())
}

async fn create_node_claim(args: NodeClaimArgs) -> anyhow::Result<()> {
    let mut profile = load_profile_or_default();
    let session_token = require_session_token(&profile)?;
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url.clone())?;
    let node_id = args
        .node_id
        .clone()
        .or_else(|| profile.node_id.clone())
        .unwrap_or_else(|| "node-local".to_string());
    let node_name = args
        .display_name
        .clone()
        .or_else(|| profile.node_name.clone())
        .unwrap_or_else(|| "Dawn Local Node".to_string());
    let requested_capabilities = if args.capability.is_empty() {
        default_requested_capabilities(args.allow_shell)
    } else {
        normalized_values(&args.capability)
    };

    let response = issue_node_claim(
        &client,
        &session_token,
        &node_id,
        &node_name,
        requested_capabilities.clone(),
        args.expires_seconds,
    )
    .await?;

    profile.gateway_base_url = Some(gateway_base_url);
    profile.node_id = Some(response.claim.node_id.clone());
    profile.node_name = Some(response.claim.display_name.clone());
    profile.claim_token = Some(response.claim_token.clone());
    profile.requested_capabilities = requested_capabilities;
    let path = save_profile(&profile)?;

    println!("Issued node claim for {}", response.claim.node_id);
    println!("Display name: {}", response.claim.display_name);
    println!("Token hint: {}", response.token_hint);
    println!("Session URL: {}", response.session_url);
    println!("Launch URL: {}", response.launch_url);
    println!("Profile saved: {}", path.display());
    println!("Next: run `dawn-node node trust-self` so the gateway trusts this local node.");
    Ok(())
}

async fn ensure_node_runtime_preflight(profile: &mut DawnCliProfile) -> anyhow::Result<()> {
    if profile.session_token.is_none() {
        return Ok(());
    }

    let node_id = profile
        .node_id
        .clone()
        .unwrap_or_else(|| "node-local".to_string());
    let node_name = profile
        .node_name
        .clone()
        .unwrap_or_else(|| "Dawn Local Node".to_string());
    let requested_capabilities = if profile.requested_capabilities.is_empty() {
        default_requested_capabilities(false)
    } else {
        profile.requested_capabilities.clone()
    };
    let gateway_base_url = resolve_gateway_base_url(None, profile);
    let client = GatewayClient::new(gateway_base_url)?;

    if gateway_node_exists(&client, &node_id).await? {
        if profile.claim_token.take().is_some() {
            let path = save_profile(profile)?;
            println!(
                "Node {node_id} is already registered. Cleared the stale local claim token in {}.",
                path.display()
            );
        }
        return Ok(());
    }

    let latest_claim = latest_node_claim_for(&client, &node_id).await?;
    let needs_fresh_claim = latest_claim
        .as_ref()
        .map(|claim| claim.status != "pending")
        .unwrap_or(true);

    if needs_fresh_claim {
        let mut session_token = require_session_token(profile)?;
        let claim = match issue_node_claim(
            &client,
            &session_token,
            &node_id,
            &node_name,
            requested_capabilities.clone(),
            1800,
        )
        .await
        {
            Ok(claim) => claim,
            Err(error) if error.to_string().contains("invalid or unknown session token") => {
                session_token = refresh_stored_cli_session(profile, &client).await?;
                issue_node_claim(
                    &client,
                    &session_token,
                    &node_id,
                    &node_name,
                    requested_capabilities,
                    1800,
                )
                .await?
            }
            Err(error) => return Err(error),
        };
        profile.node_id = Some(node_id);
        profile.node_name = Some(node_name);
        profile.claim_token = Some(claim.claim_token.clone());
        let path = save_profile(profile)?;
        println!(
            "Refreshed node claim for {} (reason: {}). claimToken hint: {}",
            claim.claim.node_id,
            latest_claim
                .as_ref()
                .map(|record| format!("latest claim {} is {}", record.claim_id, record.status))
                .unwrap_or_else(|| "no pending claim was available".to_string()),
            claim.token_hint
        );
        println!("Updated local profile: {}", path.display());
    }

    Ok(())
}

async fn refresh_stored_cli_session(
    profile: &mut DawnCliProfile,
    client: &GatewayClient,
) -> anyhow::Result<String> {
    let bootstrap_mode = profile.bootstrap_mode.as_deref().unwrap_or("development_default");
    if bootstrap_mode != "development_default" {
        bail!(
            "stored CLI session expired and bootstrap mode `{bootstrap_mode}` does not support automatic refresh; run `dawn-node login` again"
        );
    }
    let operator_name = default_operator_name(profile);
    let response = bootstrap_session(client, DEFAULT_BOOTSTRAP_TOKEN, &operator_name).await?;
    profile.session_token = Some(response.session_token.clone());
    profile.operator_name = Some(response.session.operator_name);
    profile.bootstrap_mode = Some(response.bootstrap_mode);
    let path = save_profile(profile)?;
    println!(
        "Refreshed the local CLI session automatically for {}. Updated profile: {}",
        operator_name,
        path.display()
    );
    Ok(response.session_token)
}

async fn gateway_node_exists(client: &GatewayClient, node_id: &str) -> anyhow::Result<bool> {
    let url = format!("{}/api/gateway/control-plane/nodes/{node_id}", client.base_url);
    let response = client
        .http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to query node presence from {url}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unavailable>".to_string());
        bail!("request to {url} failed with status {status}: {body}");
    }
    Ok(true)
}

async fn latest_node_claim_for(
    client: &GatewayClient,
    node_id: &str,
) -> anyhow::Result<Option<NodeClaimSummaryRecord>> {
    let claims: Vec<NodeClaimSummaryRecord> = client.get_json("/api/gateway/identity/node-claims").await?;
    Ok(claims
        .into_iter()
        .filter(|record| record.node_id == node_id)
        .max_by_key(|record| record.expires_at_unix_ms))
}

async fn trust_self_node(args: NodeTrustSelfArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url)?;
    let node_id = args
        .node_id
        .clone()
        .or_else(|| profile.node_id.clone())
        .unwrap_or_else(|| "node-local".to_string());
    let (issuer_did, public_key_hex) = derive_local_node_trust_root(&node_id)?;
    let actor = args
        .actor
        .or_else(|| profile.operator_name.clone())
        .unwrap_or_else(|| default_operator_name(&profile));
    let reason = args
        .reason
        .unwrap_or_else(|| format!("trust local CLI node {node_id}"));
    let label = args
        .label
        .or_else(|| profile.node_name.clone())
        .unwrap_or_else(|| format!("Local node {node_id}"));
    let response: NodeTrustRootUpsertResponse = client
        .post_json(
            "/api/gateway/control-plane/nodes/trust-roots",
            &NodeTrustRootUpsertRequest {
                actor,
                reason,
                issuer_did,
                label,
                public_key_hex,
            },
        )
        .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "Trusted local node issuer {} ({})",
            response.trust_root.issuer_did, response.trust_root.label
        );
    }
    Ok(())
}

async fn upsert_workspace(
    client: &GatewayClient,
    session_token: &str,
    mut request: WorkspaceProfileUpdateRequest,
) -> anyhow::Result<WorkspaceProfileRecord> {
    request.session_token = session_token.to_string();
    let response: WorkspaceProfileUpdateResponse = client
        .put_json("/api/gateway/identity/workspace", &request)
        .await?;
    Ok(response.workspace)
}

async fn issue_node_claim(
    client: &GatewayClient,
    session_token: &str,
    node_id: &str,
    node_name: &str,
    requested_capabilities: Vec<String>,
    expires_seconds: u64,
) -> anyhow::Result<NodeClaimCreateResponse> {
    client
        .post_json(
            "/api/gateway/identity/node-claims",
            &NodeClaimCreateRequest {
                session_token: session_token.to_string(),
                node_id: node_id.to_string(),
                display_name: Some(node_name.to_string()),
                transport: Some("websocket".to_string()),
                requested_capabilities: Some(requested_capabilities),
                expires_in_seconds: Some(expires_seconds),
            },
        )
        .await
}

fn derive_local_node_trust_root(node_id: &str) -> anyhow::Result<(String, String)> {
    let signing_seed = resolve_local_node_signing_seed(node_id)?;
    let signing_key = SigningKey::from_bytes(&signing_seed);
    let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());
    Ok((format!("did:dawn:node:{public_key_hex}"), public_key_hex))
}

fn resolve_local_node_signing_seed(node_id: &str) -> anyhow::Result<[u8; 32]> {
    if let Ok(raw) = env::var("DAWN_NODE_SIGNING_SEED_HEX") {
        let bytes =
            hex::decode(raw.trim()).context("failed to decode DAWN_NODE_SIGNING_SEED_HEX")?;
        return <[u8; 32]>::try_from(bytes.as_slice())
            .context("DAWN_NODE_SIGNING_SEED_HEX must decode to 32 bytes");
    }
    let digest = sha2::Sha256::digest(format!("dawn-node:{node_id}").as_bytes());
    let mut seed = [0_u8; 32];
    seed.copy_from_slice(&digest[..32]);
    Ok(seed)
}

async fn fetch_local_catalog(
    client: &GatewayClient,
    query: Option<&str>,
    include_all: bool,
) -> anyhow::Result<MarketplaceCatalog> {
    fetch_local_catalog_kind(client, query, "skill", include_all).await
}

async fn fetch_local_catalog_kind(
    client: &GatewayClient,
    query: Option<&str>,
    kind: &str,
    include_all: bool,
) -> anyhow::Result<MarketplaceCatalog> {
    let path = format!(
        "/api/gateway/marketplace/catalog?{}",
        build_catalog_query(query, kind, include_all)
    );
    client.get_json(&path).await
}

async fn fetch_federated_catalog(
    client: &GatewayClient,
    query: Option<&str>,
    include_all: bool,
) -> anyhow::Result<FederatedMarketplaceCatalog> {
    fetch_federated_catalog_kind(client, query, "skill", include_all).await
}

async fn fetch_federated_catalog_kind(
    client: &GatewayClient,
    query: Option<&str>,
    kind: &str,
    include_all: bool,
) -> anyhow::Result<FederatedMarketplaceCatalog> {
    let path = format!(
        "/api/gateway/marketplace/catalog/federated?{}",
        build_catalog_query(query, kind, include_all)
    );
    client.get_json(&path).await
}

async fn select_skill_entry(
    client: &GatewayClient,
    args: &SkillInstallArgs,
) -> anyhow::Result<MarketplaceSkillEntry> {
    if args.federated {
        let catalog = fetch_federated_catalog(client, Some(&args.skill_id), args.all).await?;
        let mut matches = catalog
            .skills
            .into_iter()
            .filter(|entry| {
                entry.entry.skill_id == args.skill_id
                    && args
                        .version
                        .as_deref()
                        .is_none_or(|version| entry.entry.version == version)
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            bail!("no matching federated skill found for {}", args.skill_id);
        }
        if matches.len() > 1 {
            let options = matches
                .iter()
                .map(|entry| {
                    format!(
                        "{}@{} from {}:{}",
                        entry.entry.skill_id,
                        entry.entry.version,
                        entry.source_display_name,
                        entry.source_peer_id
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            bail!("multiple matching skills found: {options}. Re-run with --version.");
        }
        return Ok(matches.remove(0).entry);
    }

    let catalog = fetch_local_catalog(client, Some(&args.skill_id), args.all).await?;
    let mut matches = catalog
        .skills
        .into_iter()
        .filter(|entry| {
            entry.skill_id == args.skill_id
                && args
                    .version
                    .as_deref()
                    .is_none_or(|version| entry.version == version)
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        bail!("no matching local skill found for {}", args.skill_id);
    }
    if matches.len() > 1 {
        let options = matches
            .iter()
            .map(|entry| format!("{}@{}", entry.skill_id, entry.version))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("multiple matching skills found: {options}. Re-run with --version.");
    }
    Ok(matches.remove(0))
}

async fn select_agent_entry(
    client: &GatewayClient,
    args: &AgentInstallArgs,
) -> anyhow::Result<MarketplaceAgentEntry> {
    if args.federated {
        let catalog =
            fetch_federated_catalog_kind(client, Some(&args.card_id), "agent", args.all).await?;
        let mut matches = catalog
            .agent_cards
            .into_iter()
            .filter(|entry| entry.entry.card_id == args.card_id)
            .collect::<Vec<_>>();
        if matches.is_empty() {
            bail!(
                "no matching federated agent card found for {}",
                args.card_id
            );
        }
        if matches.len() > 1 {
            let options = matches
                .iter()
                .map(|entry| {
                    format!(
                        "{} from {}:{}",
                        entry.entry.card_id, entry.source_display_name, entry.source_peer_id
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            bail!("multiple matching agent cards found: {options}");
        }
        return Ok(matches.remove(0).entry);
    }

    let catalog = fetch_local_catalog_kind(client, Some(&args.card_id), "agent", args.all).await?;
    let mut matches = catalog
        .agent_cards
        .into_iter()
        .filter(|entry| entry.card_id == args.card_id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        bail!("no matching local agent card found for {}", args.card_id);
    }
    if matches.len() > 1 {
        let options = matches
            .iter()
            .map(|entry| entry.card_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("multiple matching agent cards found: {options}");
    }
    Ok(matches.remove(0))
}

fn build_catalog_query(query: Option<&str>, kind: &str, include_all: bool) -> String {
    let mut parts = vec![format!("kind={kind}")];
    if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("q={}", query.replace(' ', "%20")));
    }
    parts.push(format!("signedOnly={}", !include_all));
    parts.push("publishedOnly=true".to_string());
    parts.join("&")
}

fn normalize_connector_target(is_models: bool, target: &str) -> String {
    let normalized = target.trim().to_ascii_lowercase();
    if is_models {
        match normalized.as_str() {
            "openai-codex" | "openai_codex" | "codex" | "chatgpt" => {
                "openai_codex".to_string()
            }
            "claude" => "anthropic".to_string(),
            "gemini" => "google".to_string(),
            "aws-bedrock" | "aws_bedrock" => "bedrock".to_string(),
            "github-models" | "github_models" | "github" => "github_models".to_string(),
            "hf" | "hugging-face" | "hugging_face" => "huggingface".to_string(),
            "cloudflare-gateway"
            | "cloudflare_gateway"
            | "cloudflare-ai-gateway"
            | "cloudflare_ai_gateway" => "cloudflare_ai_gateway".to_string(),
            "grok" => "groq".to_string(),
            "togetherai" => "together".to_string(),
            "vercel-gateway" | "vercel_gateway" | "vercel-ai-gateway" => {
                "vercel_ai_gateway".to_string()
            }
            "openai-local" | "local-openai" => "vllm".to_string(),
            "mistralai" => "mistral".to_string(),
            "nvidia-nim" | "nvidia_nim" | "nim" => "nvidia".to_string(),
            "lite-llm" | "lite_llm" => "litellm".to_string(),
            other => other.to_string(),
        }
    } else {
        match normalized.as_str() {
            "wecom" => "wecom_bot".to_string(),
            "teams" | "microsoft_teams" | "microsoft-teams" => "msteams".to_string(),
            "whatsapp-cloud" | "whatsapp_cloud" => "whatsapp".to_string(),
            "line-messaging" => "line".to_string(),
            "matrix-org" => "matrix".to_string(),
            "googlechat" | "google-chat" | "gchat" => "google_chat".to_string(),
            "signal-cli" | "signal_cli" | "signal-messenger" => "signal".to_string(),
            "blue-bubbles" | "blue_bubbles" => "bluebubbles".to_string(),
            other => other.to_string(),
        }
    }
}

fn connector_secret_pairs(
    is_models: bool,
    target: &str,
    args: &ConnectorConnectArgs,
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut pairs = BTreeMap::new();
    for (key, value) in parse_env_assignments(&args.env)? {
        pairs.insert(key, value);
    }

    match (is_models, target) {
        (true, "openai") => insert_if_some(&mut pairs, "OPENAI_API_KEY", args.api_key.as_deref()),
        (true, "anthropic") => {
            insert_if_some(&mut pairs, "ANTHROPIC_API_KEY", args.api_key.as_deref())
        }
        (true, "google") => {
            insert_if_some(&mut pairs, "GEMINI_API_KEY", args.api_key.as_deref());
            insert_if_some(&mut pairs, "GOOGLE_API_KEY", args.api_key.as_deref());
        }
        (true, "bedrock") => {
            insert_if_some(&mut pairs, "BEDROCK_API_KEY", args.api_key.as_deref());
            if let Some(base_or_url) = args.base_url.as_deref() {
                if base_or_url.contains("/chat/completions") {
                    insert_if_some(
                        &mut pairs,
                        "BEDROCK_CHAT_COMPLETIONS_URL",
                        Some(base_or_url),
                    );
                } else if base_or_url.contains("/openai/v1") {
                    insert_if_some(&mut pairs, "BEDROCK_BASE_URL", Some(base_or_url));
                } else {
                    insert_if_some(&mut pairs, "BEDROCK_RUNTIME_ENDPOINT", Some(base_or_url));
                }
            }
        }
        (true, "github_models") => {
            insert_if_some(&mut pairs, "GITHUB_MODELS_API_KEY", args.api_key.as_deref());
            insert_if_some(&mut pairs, "GITHUB_TOKEN", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "GITHUB_MODELS_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "huggingface") => {
            insert_if_some(&mut pairs, "HUGGINGFACE_API_KEY", args.api_key.as_deref());
            insert_if_some(&mut pairs, "HF_TOKEN", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "HUGGINGFACE_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "cloudflare_ai_gateway") => {
            insert_if_some(
                &mut pairs,
                "CLOUDFLARE_AI_GATEWAY_API_KEY",
                args.api_key.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID",
                args.app_id.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "CLOUDFLARE_AI_GATEWAY_ID",
                args.endpoint_id.as_deref(),
            );
            if let Some(base_or_url) = args.base_url.as_deref() {
                if base_or_url.contains("/chat/completions") {
                    insert_if_some(
                        &mut pairs,
                        "CLOUDFLARE_AI_GATEWAY_CHAT_COMPLETIONS_URL",
                        Some(base_or_url),
                    );
                } else {
                    insert_if_some(
                        &mut pairs,
                        "CLOUDFLARE_AI_GATEWAY_BASE_URL",
                        Some(base_or_url),
                    );
                }
            }
        }
        (true, "openrouter") => {
            insert_if_some(&mut pairs, "OPENROUTER_API_KEY", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "OPENROUTER_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "groq") => {
            insert_if_some(&mut pairs, "GROQ_API_KEY", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "GROQ_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "together") => {
            insert_if_some(&mut pairs, "TOGETHER_API_KEY", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "TOGETHER_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "vercel_ai_gateway") => {
            insert_if_some(
                &mut pairs,
                "VERCEL_AI_GATEWAY_API_KEY",
                args.api_key.as_deref(),
            );
            insert_if_some(&mut pairs, "AI_GATEWAY_API_KEY", args.api_key.as_deref());
            if let Some(base_or_url) = args.base_url.as_deref() {
                if base_or_url.contains("/chat/completions") {
                    insert_if_some(
                        &mut pairs,
                        "VERCEL_AI_GATEWAY_CHAT_COMPLETIONS_URL",
                        Some(base_or_url),
                    );
                } else {
                    insert_if_some(&mut pairs, "VERCEL_AI_GATEWAY_BASE_URL", Some(base_or_url));
                }
            }
        }
        (true, "vllm") => {
            insert_if_some(&mut pairs, "VLLM_API_KEY", args.api_key.as_deref());
            insert_if_some(&mut pairs, "VLLM_BASE_URL", args.base_url.as_deref());
        }
        (true, "mistral") => {
            insert_if_some(&mut pairs, "MISTRAL_API_KEY", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "MISTRAL_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "nvidia") => {
            insert_if_some(&mut pairs, "NVIDIA_API_KEY", args.api_key.as_deref());
            insert_if_some(&mut pairs, "NVIDIA_NIM_API_KEY", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "NVIDIA_CHAT_COMPLETIONS_URL",
                args.base_url.as_deref(),
            );
        }
        (true, "litellm") => {
            insert_if_some(&mut pairs, "LITELLM_API_KEY", args.api_key.as_deref());
            if let Some(base_or_url) = args.base_url.as_deref() {
                if base_or_url.contains("/chat/completions") {
                    insert_if_some(
                        &mut pairs,
                        "LITELLM_CHAT_COMPLETIONS_URL",
                        Some(base_or_url),
                    );
                } else {
                    insert_if_some(&mut pairs, "LITELLM_BASE_URL", Some(base_or_url));
                }
            }
        }
        (true, "deepseek") => {
            insert_if_some(&mut pairs, "DEEPSEEK_API_KEY", args.api_key.as_deref())
        }
        (true, "qwen") => {
            insert_if_some(&mut pairs, "QWEN_API_KEY", args.api_key.as_deref());
        }
        (true, "zhipu") => insert_if_some(&mut pairs, "ZHIPU_API_KEY", args.api_key.as_deref()),
        (true, "moonshot") => {
            insert_if_some(&mut pairs, "MOONSHOT_API_KEY", args.api_key.as_deref())
        }
        (true, "doubao") => {
            insert_if_some(&mut pairs, "DOUBAO_API_KEY", args.api_key.as_deref());
            insert_if_some(
                &mut pairs,
                "DOUBAO_ENDPOINT_ID",
                args.endpoint_id.as_deref(),
            );
        }
        (true, "ollama") => {
            insert_if_some(&mut pairs, "OLLAMA_BASE_URL", args.base_url.as_deref());
        }
        (false, "telegram") => {
            insert_if_some(
                &mut pairs,
                "TELEGRAM_BOT_TOKEN",
                args.access_token.as_deref(),
            );
        }
        (false, "slack") => {
            insert_if_some(
                &mut pairs,
                "SLACK_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "discord") => {
            insert_if_some(
                &mut pairs,
                "DISCORD_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "mattermost") => {
            insert_if_some(
                &mut pairs,
                "MATTERMOST_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "msteams") => {
            insert_if_some(
                &mut pairs,
                "MSTEAMS_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "whatsapp") => {
            insert_if_some(
                &mut pairs,
                "WHATSAPP_ACCESS_TOKEN",
                args.access_token.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "WHATSAPP_PHONE_NUMBER_ID",
                args.app_id.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "WHATSAPP_MESSAGES_URL",
                args.base_url.as_deref(),
            );
        }
        (false, "line") => {
            insert_if_some(
                &mut pairs,
                "LINE_CHANNEL_ACCESS_TOKEN",
                args.access_token.as_deref(),
            );
            insert_if_some(&mut pairs, "LINE_PUSH_API_URL", args.base_url.as_deref());
        }
        (false, "matrix") => {
            insert_if_some(
                &mut pairs,
                "MATRIX_ACCESS_TOKEN",
                args.access_token.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "MATRIX_HOMESERVER_URL",
                args.base_url.as_deref(),
            );
        }
        (false, "google_chat") => {
            insert_if_some(
                &mut pairs,
                "GOOGLE_CHAT_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "signal") => {
            insert_if_some(&mut pairs, "SIGNAL_ACCOUNT", args.access_token.as_deref());
            if let Some(base_or_url) = args.base_url.as_deref().map(str::trim) {
                if !base_or_url.is_empty() {
                    if base_or_url.trim_end_matches('/').ends_with("/v2/send") {
                        insert_if_some(&mut pairs, "SIGNAL_SEND_API_URL", Some(base_or_url));
                    } else {
                        insert_if_some(&mut pairs, "SIGNAL_HTTP_URL", Some(base_or_url));
                    }
                }
            }
        }
        (false, "bluebubbles") => {
            insert_if_some(
                &mut pairs,
                "BLUEBUBBLES_PASSWORD",
                args.client_secret.as_deref(),
            );
            if let Some(base_or_url) = args.base_url.as_deref().map(str::trim) {
                if !base_or_url.is_empty() {
                    if base_or_url.contains("/api/v1/message/text") {
                        insert_if_some(
                            &mut pairs,
                            "BLUEBUBBLES_SEND_MESSAGE_URL",
                            Some(base_or_url),
                        );
                    } else {
                        insert_if_some(&mut pairs, "BLUEBUBBLES_SERVER_URL", Some(base_or_url));
                    }
                }
            }
        }
        (false, "feishu") => {
            insert_if_some(
                &mut pairs,
                "FEISHU_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "dingtalk") => {
            insert_if_some(
                &mut pairs,
                "DINGTALK_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "wecom_bot") => {
            insert_if_some(
                &mut pairs,
                "WECOM_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
        }
        (false, "wechat_official_account") => {
            insert_if_some(
                &mut pairs,
                "WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN",
                args.access_token.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "WECHAT_OFFICIAL_ACCOUNT_APP_ID",
                args.app_id.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "WECHAT_OFFICIAL_ACCOUNT_APP_SECRET",
                args.app_secret.as_deref(),
            );
        }
        (false, "qq") => {
            insert_if_some(&mut pairs, "QQ_BOT_APP_ID", args.app_id.as_deref());
            insert_if_some(
                &mut pairs,
                "QQ_BOT_CLIENT_SECRET",
                args.client_secret.as_deref(),
            );
        }
        _ => bail!("unsupported connector target `{target}`"),
    }

    Ok(pairs)
}

fn ingress_secret_pairs(
    target: &str,
    args: &IngressConnectArgs,
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut pairs = BTreeMap::new();
    for (key, value) in parse_env_assignments(&args.env)? {
        pairs.insert(key, value);
    }

    match target {
        "telegram" => insert_if_some(
            &mut pairs,
            "DAWN_TELEGRAM_WEBHOOK_SECRET",
            args.secret.as_deref(),
        ),
        "signal" => {
            insert_if_some(
                &mut pairs,
                "DAWN_SIGNAL_CALLBACK_SECRET",
                args.secret.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "DAWN_SIGNAL_DM_POLICY",
                normalize_dm_policy(args.dm_policy.as_deref())?.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "DAWN_SIGNAL_ALLOWLIST",
                join_allowlist_values(&args.allow_from).as_deref(),
            );
        }
        "bluebubbles" => {
            insert_if_some(
                &mut pairs,
                "DAWN_BLUEBUBBLES_CALLBACK_SECRET",
                args.secret.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "DAWN_BLUEBUBBLES_DM_POLICY",
                normalize_dm_policy(args.dm_policy.as_deref())?.as_deref(),
            );
            insert_if_some(
                &mut pairs,
                "DAWN_BLUEBUBBLES_ALLOWLIST",
                join_allowlist_values(&args.allow_from).as_deref(),
            );
        }
        "feishu" => {}
        "dingtalk" => insert_if_some(
            &mut pairs,
            "DAWN_DINGTALK_CALLBACK_TOKEN",
            args.token.as_deref(),
        ),
        "wecom" => insert_if_some(
            &mut pairs,
            "DAWN_WECOM_CALLBACK_TOKEN",
            args.token.as_deref(),
        ),
        "wechat_official_account" => insert_if_some(
            &mut pairs,
            "DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN",
            args.token.as_deref(),
        ),
        "qq" => insert_if_some(
            &mut pairs,
            "DAWN_QQ_BOT_CALLBACK_SECRET",
            args.secret.as_deref(),
        ),
        _ => bail!("unsupported ingress target `{target}`"),
    }

    Ok(pairs)
}

fn normalize_ingress_target_name(target: &str) -> String {
    match target.trim().to_ascii_lowercase().as_str() {
        "signal-cli" | "signal_cli" | "signal-messenger" => "signal".to_string(),
        "blue-bubbles" | "blue_bubbles" => "bluebubbles".to_string(),
        other => other.to_string(),
    }
}

fn normalize_dm_policy(value: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = match value.to_ascii_lowercase().as_str() {
        "open" => "open",
        "allowlist" | "allow_list" => "allowlist",
        "pairing" | "pair" => "pairing",
        "disabled" | "off" => "disabled",
        other => {
            bail!("unsupported dm policy `{other}`; expected open, allowlist, pairing, or disabled")
        }
    };
    Ok(Some(normalized.to_string()))
}

fn join_allowlist_values(values: &[String]) -> Option<String> {
    let joined = values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn parse_env_assignments(values: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|entry| {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| anyhow!("invalid --env assignment `{entry}`; expected KEY=VALUE"))?;
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                bail!("invalid --env assignment `{entry}`; expected non-empty KEY and VALUE");
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

fn insert_if_some(pairs: &mut BTreeMap<String, String>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        pairs.insert(key.to_string(), value.to_string());
    }
}

fn render_secret_block(env: &BTreeMap<String, String>, format: &str) -> anyhow::Result<String> {
    let format = format.trim().to_ascii_lowercase();
    let lines = env
        .iter()
        .map(|(key, value)| match format.as_str() {
            "dotenv" => Ok(format!("{key}={value}")),
            "powershell" | "pwsh" => Ok(format!("$env:{key} = '{}'", value.replace('\'', "''"))),
            _ => bail!("unsupported export format `{format}`; use dotenv or powershell"),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(lines.join("\n"))
}

fn resolve_dawn_core_dir(override_cwd: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_cwd {
        let path = PathBuf::from(path);
        if path.join("Cargo.toml").exists() {
            return Ok(path);
        }
        bail!("gateway cwd {} does not contain Cargo.toml", path.display());
    }

    let current = env::current_dir().context("failed to read current working directory")?;
    let direct = current.join("dawn_core");
    if direct.join("Cargo.toml").exists() {
        return Ok(direct);
    }
    if current
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("dawn_node"))
    {
        let sibling = current
            .parent()
            .map(|parent| parent.join("dawn_core"))
            .ok_or_else(|| anyhow!("failed to locate workspace root from {}", current.display()))?;
        if sibling.join("Cargo.toml").exists() {
            return Ok(sibling);
        }
    }
    bail!(
        "could not locate dawn_core. Run from the workspace root or pass `--cwd <path-to-dawn_core>`"
    )
}

fn require_session_token(profile: &DawnCliProfile) -> anyhow::Result<String> {
    profile
        .session_token
        .clone()
        .ok_or_else(|| anyhow!("no stored session token. Run `dawn-node login` first."))
}

fn resolve_gateway_base_url(override_gateway: Option<&str>, profile: &DawnCliProfile) -> String {
    override_gateway
        .map(normalize_http_base_url)
        .or_else(|| {
            profile
                .gateway_base_url
                .as_deref()
                .map(normalize_http_base_url)
        })
        .unwrap_or_else(default_gateway_base_url)
}

fn update_values(current: &[String], incoming: &[String], add: bool) -> Vec<String> {
    let mut values = current.iter().cloned().collect::<BTreeSet<_>>();
    for value in normalized_values(incoming) {
        if add {
            values.insert(value);
        } else {
            values.remove(&value);
        }
    }
    values.into_iter().collect()
}

fn normalized_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn default_requested_capabilities(allow_shell: bool) -> Vec<String> {
    let mut capabilities = vec![
        "agent_ping".to_string(),
        "browser_start".to_string(),
        "browser_profiles".to_string(),
        "browser_profile_inspect".to_string(),
        "browser_profile_import".to_string(),
        "browser_profile_export".to_string(),
        "browser_profile_delete".to_string(),
        "browser_status".to_string(),
        "browser_stop".to_string(),
        "browser_navigate".to_string(),
        "browser_new_tab".to_string(),
        "browser_new_window".to_string(),
        "browser_extract".to_string(),
        "browser_click".to_string(),
        "browser_back".to_string(),
        "browser_forward".to_string(),
        "browser_reload".to_string(),
        "browser_focus".to_string(),
        "browser_close".to_string(),
        "browser_tabs".to_string(),
        "browser_snapshot".to_string(),
        "browser_screenshot".to_string(),
        "browser_pdf".to_string(),
        "browser_console_messages".to_string(),
        "browser_network_requests".to_string(),
        "browser_network_export".to_string(),
        "browser_trace".to_string(),
        "browser_trace_export".to_string(),
        "browser_errors".to_string(),
        "browser_errors_export".to_string(),
        "browser_cookies".to_string(),
        "browser_storage".to_string(),
        "browser_storage_set".to_string(),
        "browser_set_headers".to_string(),
        "browser_set_offline".to_string(),
        "browser_set_geolocation".to_string(),
        "browser_emulate_device".to_string(),
        "browser_evaluate".to_string(),
        "browser_wait_for".to_string(),
        "browser_handle_dialog".to_string(),
        "browser_press_key".to_string(),
        "browser_type".to_string(),
        "browser_upload".to_string(),
        "browser_download".to_string(),
        "browser_form_fill".to_string(),
        "browser_form_submit".to_string(),
        "browser_open".to_string(),
        "browser_search".to_string(),
        "desktop_open".to_string(),
        "system_lock".to_string(),
        "system_sleep".to_string(),
        "desktop_notification".to_string(),
        "desktop_clipboard_set".to_string(),
        "desktop_type_text".to_string(),
        "desktop_key_press".to_string(),
        "desktop_windows_list".to_string(),
        "desktop_window_focus".to_string(),
        "desktop_wait_for_window".to_string(),
        "desktop_focus_app".to_string(),
        "desktop_launch_and_focus".to_string(),
        "desktop_mouse_move".to_string(),
        "desktop_mouse_click".to_string(),
        "desktop_screenshot".to_string(),
        "desktop_ocr".to_string(),
        "desktop_accessibility_query".to_string(),
        "desktop_accessibility_click".to_string(),
        "desktop_accessibility_wait_for".to_string(),
        "desktop_accessibility_fill".to_string(),
        "desktop_accessibility_workflow".to_string(),
        "desktop_accessibility_snapshot".to_string(),
        "desktop_accessibility_focus".to_string(),
        "desktop_accessibility_invoke".to_string(),
        "desktop_accessibility_set_value".to_string(),
        "echo".to_string(),
        "list_capabilities".to_string(),
        "list_directory".to_string(),
        "process_snapshot".to_string(),
        "read_file_preview".to_string(),
        "stat_path".to_string(),
        "system_info".to_string(),
    ];
    if allow_shell {
        capabilities.push("shell_exec".to_string());
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn join_json_array(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|joined| !joined.is_empty())
        .unwrap_or_else(|| "<none>".to_string())
}

struct GatewayClient {
    base_url: String,
    http: Client,
}

impl GatewayClient {
    fn new(base_url: String) -> anyhow::Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { base_url, http })
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        self.send(Method::GET, path, Option::<&Value>::None).await
    }

    async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        self.send(Method::POST, path, Some(body)).await
    }

    async fn put_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        self.send(Method::PUT, path, Some(body)).await
    }

    async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> anyhow::Result<T> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        };
        let mut request = self.http.request(method, &url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("request to {url} failed"))?;
        let status = response.status();
        let payload = response
            .bytes()
            .await
            .with_context(|| format!("failed to read response body from {url}"))?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&payload);
            bail!("request to {url} failed with status {status}: {body}");
        }
        serde_json::from_slice(&payload)
            .with_context(|| format!("failed to decode JSON response from {url}"))
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{
        ApprovalRequestSummary, ChannelSendArgs, PaymentRecordSummary, build_ap2_signature_payload,
        build_catalog_query, build_channel_send_request, build_chat_reply, connector_secret_pairs,
        connector_setup_option_label, default_requested_capabilities, derive_local_node_trust_root,
        extract_text_from_value, find_pending_approval_record, format_payment_approval_summary,
        ingress_secret_pairs, normalize_connector_target, normalize_ingress_target_name,
        parse_named_selection, resolve_ap2_mcu_seed_hex, sign_ap2_payload, update_values,
    };
    use crate::profile::DawnCliProfile;
    use serde_json::json;

    #[test]
    fn updates_metadata_lists_without_duplicates() {
        let next = update_values(
            &["qwen".to_string(), "deepseek".to_string()],
            &["openai".to_string(), "qwen".to_string()],
            true,
        );
        assert_eq!(next, vec!["deepseek", "openai", "qwen"]);
    }

    #[test]
    fn builds_catalog_query_for_signed_skill_search() {
        assert_eq!(
            build_catalog_query(Some("travel agent"), "skill", false),
            "kind=skill&q=travel%20agent&signedOnly=true&publishedOnly=true"
        );
    }

    #[test]
    fn extracts_nested_text_from_agent_payloads() {
        let payload = json!({
            "output": {
                "content": [
                    {"text": "Booked the hotel."},
                    {"text": "Check-in is Friday."}
                ]
            }
        });

        assert_eq!(
            extract_text_from_value(&payload).as_deref(),
            Some("Booked the hotel.\nCheck-in is Friday.")
        );
    }

    #[test]
    fn builds_chat_reply_from_remote_task_status() {
        let response = json!({
            "invocation": {
                "status": "completed",
                "response": {
                    "task": {
                        "taskId": "remote-123",
                        "status": "completed"
                    }
                }
            },
            "remoteStatus": "completed"
        });

        let reply = build_chat_reply("travel-agent", &response);
        assert!(reply.contains("Agent travel-agent finished with status completed"));
        assert!(reply.contains("Remote task: remote-123"));
        assert!(reply.contains("Remote task remote-123 status=completed"));
    }

    #[test]
    fn finds_pending_approval_by_kind_and_reference() {
        let approvals = vec![
            ApprovalRequestSummary {
                approval_id: "approval-1".to_string(),
                kind: "payment".to_string(),
                reference_id: "tx-1".to_string(),
                status: "approved".to_string(),
            },
            ApprovalRequestSummary {
                approval_id: "approval-2".to_string(),
                kind: "payment".to_string(),
                reference_id: "tx-2".to_string(),
                status: "pending".to_string(),
            },
        ];

        let approval = find_pending_approval_record(&approvals, "payment", "tx-2")
            .expect("pending approval should be found");
        assert_eq!(approval.approval_id, "approval-2");
    }

    #[test]
    fn formats_payment_approval_summary_with_payment_status() {
        let response = json!({
            "approval": {
                "approvalId": "approval-22",
                "status": "approved"
            },
            "paymentResponse": {
                "status": "authorized"
            }
        });

        let summary = format_payment_approval_summary("approve", "tx-22", &response);
        assert_eq!(
            summary,
            "Approved AP2 payment tx-22. approval=approval-22 approvalStatus=approved paymentStatus=authorized"
        );
    }

    #[test]
    fn builds_ap2_signature_payload_with_fixed_precision() {
        let payment = PaymentRecordSummary {
            transaction_id: "tx-9".to_string(),
            task_id: Some("task-9".to_string()),
            mandate_id: "mandate-9".to_string(),
            amount: 18.5,
            description: "Book hotel".to_string(),
            status: "pending_physical_auth".to_string(),
            verification_message: "waiting".to_string(),
            mcu_public_did: None,
        };

        assert_eq!(
            build_ap2_signature_payload(&payment),
            "tx-9:mandate-9:18.5000:Book hotel"
        );
    }

    #[test]
    fn resolves_ap2_seed_from_profile_secret() {
        let mut profile = DawnCliProfile::default();
        profile
            .connector_env
            .insert("DAWN_AP2_MCU_SEED_HEX".to_string(), "aa".repeat(32));

        let seed_hex = resolve_ap2_mcu_seed_hex(None, &profile).expect("seed should resolve");
        assert_eq!(seed_hex, "aa".repeat(32));
    }

    #[test]
    fn signs_ap2_payload_with_serial_mock_seed() {
        let mut profile = DawnCliProfile::default();
        profile
            .connector_env
            .insert("DAWN_AP2_SIGNER_MODE".to_string(), "serial".to_string());
        profile
            .connector_env
            .insert("DAWN_AP2_SERIAL_PORT".to_string(), "COM7".to_string());
        profile
            .connector_env
            .insert("DAWN_AP2_SERIAL_BAUD".to_string(), "115200".to_string());
        profile
            .connector_env
            .insert("DAWN_AP2_SERIAL_MOCK_SEED_HEX".to_string(), "11".repeat(32));

        let signed = sign_ap2_payload(&profile, "tx:mandate:1.0000:test", None)
            .expect("serial mock signer should work");
        assert!(signed.signer_label.starts_with("serial-mock:COM7@115200/"));
        assert!(signed.mcu_public_did.starts_with("did:dawn:mcu:"));
        assert_eq!(signed.mcu_signature.len(), 128);
    }

    #[test]
    fn serial_signer_without_mock_seed_is_blocked() {
        let mut profile = DawnCliProfile::default();
        profile
            .connector_env
            .insert("DAWN_AP2_SIGNER_MODE".to_string(), "serial".to_string());
        profile
            .connector_env
            .insert("DAWN_AP2_SERIAL_PORT".to_string(), "COM9".to_string());

        let error = sign_ap2_payload(&profile, "payload", None)
            .expect_err("serial signer without mock seed should fail")
            .to_string();
        assert!(error.contains("transport is not available yet"));
    }

    #[test]
    fn normalizes_connector_aliases_for_new_targets() {
        assert_eq!(normalize_connector_target(true, "gemini"), "google");
        assert_eq!(normalize_connector_target(true, "aws-bedrock"), "bedrock");
        assert_eq!(
            normalize_connector_target(true, "cloudflare-ai-gateway"),
            "cloudflare_ai_gateway"
        );
        assert_eq!(normalize_connector_target(true, "grok"), "groq");
        assert_eq!(normalize_connector_target(true, "togetherai"), "together");
        assert_eq!(normalize_connector_target(true, "local-openai"), "vllm");
        assert_eq!(normalize_connector_target(true, "mistralai"), "mistral");
        assert_eq!(normalize_connector_target(true, "nim"), "nvidia");
        assert_eq!(normalize_connector_target(true, "lite-llm"), "litellm");
        assert_eq!(normalize_connector_target(false, "teams"), "msteams");
        assert_eq!(
            normalize_connector_target(false, "whatsapp-cloud"),
            "whatsapp"
        );
        assert_eq!(normalize_connector_target(false, "line-messaging"), "line");
        assert_eq!(normalize_connector_target(false, "matrix-org"), "matrix");
        assert_eq!(
            normalize_connector_target(false, "google-chat"),
            "google_chat"
        );
        assert_eq!(normalize_connector_target(false, "signal-cli"), "signal");
        assert_eq!(
            normalize_connector_target(false, "blue-bubbles"),
            "bluebubbles"
        );
    }

    #[test]
    fn parse_named_selection_accepts_model_aliases() {
        let selected = parse_named_selection(
            "gemini, claude, 1",
            &[
                "openai".to_string(),
                "anthropic".to_string(),
                "google".to_string(),
            ],
            true,
        )
        .expect("aliases should resolve");
        assert_eq!(selected, vec!["google", "anthropic", "openai"]);
    }

    #[test]
    fn parse_named_selection_accepts_chat_aliases() {
        let selected = parse_named_selection(
            "telegram, google-chat, qq",
            &[
                "telegram".to_string(),
                "google_chat".to_string(),
                "qq".to_string(),
            ],
            false,
        )
        .expect("chat aliases should resolve");
        assert_eq!(selected, vec!["telegram", "google_chat", "qq"]);
    }

    #[test]
    fn setup_option_labels_show_friendly_provider_names() {
        assert_eq!(
            connector_setup_option_label(true, "google"),
            "Google Gemini (API key) [google]"
        );
        assert_eq!(
            connector_setup_option_label(false, "telegram"),
            "Telegram Bot (bot token) [telegram]"
        );
    }

    #[test]
    fn requested_capabilities_include_desktop_notification() {
        let capabilities = default_requested_capabilities(false);
        assert!(
            capabilities
                .iter()
                .any(|value| value == "desktop_notification")
        );
    }

    #[test]
    fn telegram_channel_request_omits_parse_mode_when_not_provided() {
        let args = ChannelSendArgs {
            target: "telegram".to_string(),
            text: Some("hello".to_string()),
            gateway: None,
            chat_id: Some("123".to_string()),
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
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
            group_members: Vec::new(),
            group_admins: Vec::new(),
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };

        let (_, body) = build_channel_send_request("telegram", &args).unwrap();
        assert_eq!(body["chatId"], "123");
        assert!(body.get("parseMode").is_none());
    }

    #[test]
    fn derives_stable_local_node_trust_root() {
        let (issuer_did, public_key_hex) = derive_local_node_trust_root("node-local").unwrap();
        assert!(issuer_did.starts_with("did:dawn:node:"));
        assert_eq!(
            issuer_did.strip_prefix("did:dawn:node:"),
            Some(public_key_hex.as_str())
        );
        assert_eq!(public_key_hex.len(), 64);
    }

    #[test]
    fn builds_secret_pairs_for_google_bedrock_cloudflare_openrouter_groq_together_vllm_mistral_nvidia_and_litellm()
     {
        let google_args = super::ConnectorConnectArgs {
            target: "google".to_string(),
            gateway: None,
            api_key: Some("google-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: None,
            env: Vec::new(),
        };
        let google_pairs =
            connector_secret_pairs(true, "google", &google_args).expect("google secrets");
        assert_eq!(
            google_pairs.get("GEMINI_API_KEY").map(String::as_str),
            Some("google-key")
        );
        assert_eq!(
            google_pairs.get("GOOGLE_API_KEY").map(String::as_str),
            Some("google-key")
        );

        let bedrock_args = super::ConnectorConnectArgs {
            target: "bedrock".to_string(),
            gateway: None,
            api_key: Some("bedrock-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://bedrock-runtime.us-west-2.amazonaws.com".to_string()),
            env: Vec::new(),
        };
        let bedrock_pairs =
            connector_secret_pairs(true, "bedrock", &bedrock_args).expect("bedrock secrets");
        assert_eq!(
            bedrock_pairs.get("BEDROCK_API_KEY").map(String::as_str),
            Some("bedrock-key")
        );
        assert_eq!(
            bedrock_pairs
                .get("BEDROCK_RUNTIME_ENDPOINT")
                .map(String::as_str),
            Some("https://bedrock-runtime.us-west-2.amazonaws.com")
        );

        let cloudflare_gateway_args = super::ConnectorConnectArgs {
            target: "cloudflare_ai_gateway".to_string(),
            gateway: None,
            api_key: Some("openai-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: Some("cf-account".to_string()),
            app_secret: None,
            client_secret: None,
            endpoint_id: Some("gateway-main".to_string()),
            base_url: None,
            env: Vec::new(),
        };
        let cloudflare_gateway_pairs =
            connector_secret_pairs(true, "cloudflare_ai_gateway", &cloudflare_gateway_args)
                .expect("cloudflare ai gateway secrets");
        assert_eq!(
            cloudflare_gateway_pairs
                .get("CLOUDFLARE_AI_GATEWAY_API_KEY")
                .map(String::as_str),
            Some("openai-key")
        );
        assert_eq!(
            cloudflare_gateway_pairs
                .get("CLOUDFLARE_AI_GATEWAY_ACCOUNT_ID")
                .map(String::as_str),
            Some("cf-account")
        );
        assert_eq!(
            cloudflare_gateway_pairs
                .get("CLOUDFLARE_AI_GATEWAY_ID")
                .map(String::as_str),
            Some("gateway-main")
        );

        let openrouter_args = super::ConnectorConnectArgs {
            target: "openrouter".to_string(),
            gateway: None,
            api_key: Some("openrouter-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://openrouter.ai/api/v1/chat/completions".to_string()),
            env: Vec::new(),
        };
        let openrouter_pairs = connector_secret_pairs(true, "openrouter", &openrouter_args)
            .expect("openrouter secrets");
        assert_eq!(
            openrouter_pairs
                .get("OPENROUTER_API_KEY")
                .map(String::as_str),
            Some("openrouter-key")
        );
        assert_eq!(
            openrouter_pairs
                .get("OPENROUTER_CHAT_COMPLETIONS_URL")
                .map(String::as_str),
            Some("https://openrouter.ai/api/v1/chat/completions")
        );

        let groq_args = super::ConnectorConnectArgs {
            target: "groq".to_string(),
            gateway: None,
            api_key: Some("groq-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://api.groq.com/openai/v1/chat/completions".to_string()),
            env: Vec::new(),
        };
        let groq_pairs = connector_secret_pairs(true, "groq", &groq_args).expect("groq secrets");
        assert_eq!(
            groq_pairs.get("GROQ_API_KEY").map(String::as_str),
            Some("groq-key")
        );
        assert_eq!(
            groq_pairs
                .get("GROQ_CHAT_COMPLETIONS_URL")
                .map(String::as_str),
            Some("https://api.groq.com/openai/v1/chat/completions")
        );

        let together_args = super::ConnectorConnectArgs {
            target: "together".to_string(),
            gateway: None,
            api_key: Some("together-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://api.together.xyz/v1/chat/completions".to_string()),
            env: Vec::new(),
        };
        let together_pairs =
            connector_secret_pairs(true, "together", &together_args).expect("together secrets");
        assert_eq!(
            together_pairs.get("TOGETHER_API_KEY").map(String::as_str),
            Some("together-key")
        );
        assert_eq!(
            together_pairs
                .get("TOGETHER_CHAT_COMPLETIONS_URL")
                .map(String::as_str),
            Some("https://api.together.xyz/v1/chat/completions")
        );

        let vllm_args = super::ConnectorConnectArgs {
            target: "vllm".to_string(),
            gateway: None,
            api_key: Some("local-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("http://127.0.0.1:8000".to_string()),
            env: Vec::new(),
        };
        let vllm_pairs = connector_secret_pairs(true, "vllm", &vllm_args).expect("vllm secrets");
        assert_eq!(
            vllm_pairs.get("VLLM_API_KEY").map(String::as_str),
            Some("local-key")
        );
        assert_eq!(
            vllm_pairs.get("VLLM_BASE_URL").map(String::as_str),
            Some("http://127.0.0.1:8000")
        );

        let mistral_args = super::ConnectorConnectArgs {
            target: "mistral".to_string(),
            gateway: None,
            api_key: Some("mistral-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://api.mistral.ai/v1/chat/completions".to_string()),
            env: Vec::new(),
        };
        let mistral_pairs =
            connector_secret_pairs(true, "mistral", &mistral_args).expect("mistral secrets");
        assert_eq!(
            mistral_pairs.get("MISTRAL_API_KEY").map(String::as_str),
            Some("mistral-key")
        );
        assert_eq!(
            mistral_pairs
                .get("MISTRAL_CHAT_COMPLETIONS_URL")
                .map(String::as_str),
            Some("https://api.mistral.ai/v1/chat/completions")
        );

        let nvidia_args = super::ConnectorConnectArgs {
            target: "nvidia".to_string(),
            gateway: None,
            api_key: Some("nvidia-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://integrate.api.nvidia.com/v1/chat/completions".to_string()),
            env: Vec::new(),
        };
        let nvidia_pairs =
            connector_secret_pairs(true, "nvidia", &nvidia_args).expect("nvidia secrets");
        assert_eq!(
            nvidia_pairs.get("NVIDIA_API_KEY").map(String::as_str),
            Some("nvidia-key")
        );
        assert_eq!(
            nvidia_pairs.get("NVIDIA_NIM_API_KEY").map(String::as_str),
            Some("nvidia-key")
        );
        assert_eq!(
            nvidia_pairs
                .get("NVIDIA_CHAT_COMPLETIONS_URL")
                .map(String::as_str),
            Some("https://integrate.api.nvidia.com/v1/chat/completions")
        );

        let litellm_args = super::ConnectorConnectArgs {
            target: "litellm".to_string(),
            gateway: None,
            api_key: Some("litellm-key".to_string()),
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("http://127.0.0.1:4000/v1".to_string()),
            env: Vec::new(),
        };
        let litellm_pairs =
            connector_secret_pairs(true, "litellm", &litellm_args).expect("litellm secrets");
        assert_eq!(
            litellm_pairs.get("LITELLM_API_KEY").map(String::as_str),
            Some("litellm-key")
        );
        assert_eq!(
            litellm_pairs.get("LITELLM_BASE_URL").map(String::as_str),
            Some("http://127.0.0.1:4000/v1")
        );
    }

    #[test]
    fn builds_channel_secret_pairs_for_whatsapp_line_and_matrix() {
        let whatsapp_args = super::ConnectorConnectArgs {
            target: "whatsapp".to_string(),
            gateway: None,
            api_key: None,
            access_token: Some("wa-token".to_string()),
            webhook_url: None,
            app_id: Some("1234567890".to_string()),
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://graph.facebook.com/v23.0/1234567890/messages".to_string()),
            env: Vec::new(),
        };
        let whatsapp_pairs =
            connector_secret_pairs(false, "whatsapp", &whatsapp_args).expect("whatsapp secrets");
        assert_eq!(
            whatsapp_pairs
                .get("WHATSAPP_ACCESS_TOKEN")
                .map(String::as_str),
            Some("wa-token")
        );
        assert_eq!(
            whatsapp_pairs
                .get("WHATSAPP_PHONE_NUMBER_ID")
                .map(String::as_str),
            Some("1234567890")
        );
        assert_eq!(
            whatsapp_pairs
                .get("WHATSAPP_MESSAGES_URL")
                .map(String::as_str),
            Some("https://graph.facebook.com/v23.0/1234567890/messages")
        );

        let line_args = super::ConnectorConnectArgs {
            target: "line".to_string(),
            gateway: None,
            api_key: None,
            access_token: Some("line-token".to_string()),
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://api.line.me/v2/bot/message/push".to_string()),
            env: Vec::new(),
        };
        let line_pairs = connector_secret_pairs(false, "line", &line_args).expect("line secrets");
        assert_eq!(
            line_pairs
                .get("LINE_CHANNEL_ACCESS_TOKEN")
                .map(String::as_str),
            Some("line-token")
        );
        assert_eq!(
            line_pairs.get("LINE_PUSH_API_URL").map(String::as_str),
            Some("https://api.line.me/v2/bot/message/push")
        );

        let matrix_args = super::ConnectorConnectArgs {
            target: "matrix".to_string(),
            gateway: None,
            api_key: None,
            access_token: Some("matrix-token".to_string()),
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("https://matrix-client.matrix.org".to_string()),
            env: Vec::new(),
        };
        let matrix_pairs =
            connector_secret_pairs(false, "matrix", &matrix_args).expect("matrix secrets");
        assert_eq!(
            matrix_pairs.get("MATRIX_ACCESS_TOKEN").map(String::as_str),
            Some("matrix-token")
        );
        assert_eq!(
            matrix_pairs
                .get("MATRIX_HOMESERVER_URL")
                .map(String::as_str),
            Some("https://matrix-client.matrix.org")
        );
    }

    #[test]
    fn builds_channel_secret_pairs_for_signal_and_bluebubbles() {
        let signal_args = super::ConnectorConnectArgs {
            target: "signal".to_string(),
            gateway: None,
            api_key: None,
            access_token: Some("+15550001111".to_string()),
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: None,
            endpoint_id: None,
            base_url: Some("http://127.0.0.1:8080".to_string()),
            env: Vec::new(),
        };
        let signal_pairs =
            connector_secret_pairs(false, "signal", &signal_args).expect("signal secrets");
        assert_eq!(
            signal_pairs.get("SIGNAL_ACCOUNT").map(String::as_str),
            Some("+15550001111")
        );
        assert_eq!(
            signal_pairs.get("SIGNAL_HTTP_URL").map(String::as_str),
            Some("http://127.0.0.1:8080")
        );

        let bluebubbles_args = super::ConnectorConnectArgs {
            target: "bluebubbles".to_string(),
            gateway: None,
            api_key: None,
            access_token: None,
            webhook_url: None,
            app_id: None,
            app_secret: None,
            client_secret: Some("server-guid".to_string()),
            endpoint_id: None,
            base_url: Some("https://bluebubbles.example.com".to_string()),
            env: Vec::new(),
        };
        let bluebubbles_pairs = connector_secret_pairs(false, "bluebubbles", &bluebubbles_args)
            .expect("bluebubbles secrets");
        assert_eq!(
            bluebubbles_pairs
                .get("BLUEBUBBLES_PASSWORD")
                .map(String::as_str),
            Some("server-guid")
        );
        assert_eq!(
            bluebubbles_pairs
                .get("BLUEBUBBLES_SERVER_URL")
                .map(String::as_str),
            Some("https://bluebubbles.example.com")
        );
    }

    #[test]
    fn builds_send_requests_for_new_webhook_channels() {
        let args = super::ChannelSendArgs {
            target: "mattermost".to_string(),
            text: Some("hello channel".to_string()),
            gateway: None,
            chat_id: None,
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };

        let (path, body) =
            build_channel_send_request("mattermost", &args).expect("mattermost send request");
        assert_eq!(path, "/api/gateway/connectors/chat/mattermost/send");
        assert_eq!(body, json!({ "text": "hello channel" }));

        let (path, body) =
            build_channel_send_request("msteams", &args).expect("msteams send request");
        assert_eq!(path, "/api/gateway/connectors/chat/msteams/send");
        assert_eq!(body, json!({ "text": "hello channel" }));

        let (path, body) =
            build_channel_send_request("google_chat", &args).expect("google chat send request");
        assert_eq!(path, "/api/gateway/connectors/chat/google-chat/send");
        assert_eq!(body, json!({ "text": "hello channel" }));

        let targeted_args = super::ChannelSendArgs {
            target: "whatsapp".to_string(),
            text: Some("hello channel".to_string()),
            gateway: None,
            chat_id: Some("15551234567".to_string()),
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };
        let (path, body) =
            build_channel_send_request("whatsapp", &targeted_args).expect("whatsapp send request");
        assert_eq!(path, "/api/gateway/connectors/chat/whatsapp/send");
        assert_eq!(
            body,
            json!({ "chatId": "15551234567", "text": "hello channel" })
        );

        let (path, body) =
            build_channel_send_request("line", &targeted_args).expect("line send request");
        assert_eq!(path, "/api/gateway/connectors/chat/line/send");
        assert_eq!(
            body,
            json!({ "chatId": "15551234567", "text": "hello channel" })
        );

        let (path, body) =
            build_channel_send_request("matrix", &targeted_args).expect("matrix send request");
        assert_eq!(path, "/api/gateway/connectors/chat/matrix/send");
        assert_eq!(
            body,
            json!({ "chatId": "15551234567", "text": "hello channel" })
        );

        let (path, body) =
            build_channel_send_request("signal", &targeted_args).expect("signal send request");
        assert_eq!(path, "/api/gateway/connectors/chat/signal/send");
        assert_eq!(
            body,
            json!({ "chatId": "15551234567", "text": "hello channel" })
        );

        let (path, body) = build_channel_send_request("bluebubbles", &targeted_args)
            .expect("bluebubbles send request");
        assert_eq!(path, "/api/gateway/connectors/chat/bluebubbles/send");
        assert_eq!(
            body,
            json!({ "chatId": "15551234567", "text": "hello channel" })
        );
    }

    #[test]
    fn builds_signal_attachment_and_reaction_requests() {
        let attachment_path = env::temp_dir().join("dawn-cli-signal-test.txt");
        fs::write(&attachment_path, b"hello signal attachment").expect("write attachment");

        let args = super::ChannelSendArgs {
            target: "signal".to_string(),
            text: Some("hello channel".to_string()),
            gateway: None,
            chat_id: Some("+15551234567".to_string()),
            account_key: None,
            attachment_file: Some(attachment_path.display().to_string()),
            attachment_name: Some("proof.txt".to_string()),
            attachment_content_type: Some("text/plain".to_string()),
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };

        let (path, body) =
            build_channel_send_request("signal", &args).expect("signal attachment request");
        assert_eq!(path, "/api/gateway/connectors/chat/signal/send");
        assert_eq!(body["chatId"], json!("+15551234567"));
        assert_eq!(body["text"], json!("hello channel"));
        assert_eq!(body["attachmentName"], json!("proof.txt"));
        assert_eq!(body["attachmentContentType"], json!("text/plain"));
        assert!(body["attachmentBase64"].as_str().is_some());

        let reaction_args = super::ChannelSendArgs {
            target: "signal".to_string(),
            text: None,
            gateway: None,
            chat_id: Some("+15551234567".to_string()),
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: Some("❤️".to_string()),
            target_message_id: Some("1712345678901".to_string()),
            target_author: Some("+15550001111".to_string()),
            remove_reaction: true,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };

        let (_, reaction_body) =
            build_channel_send_request("signal", &reaction_args).expect("signal reaction request");
        assert_eq!(reaction_body["reaction"], json!("❤️"));
        assert_eq!(reaction_body["targetMessageId"], json!("1712345678901"));
        assert_eq!(reaction_body["targetAuthor"], json!("+15550001111"));
        assert_eq!(reaction_body["removeReaction"], json!(true));

        fs::remove_file(&attachment_path).ok();
    }

    #[test]
    fn builds_bluebubbles_native_action_requests() {
        let reaction_args = super::ChannelSendArgs {
            target: "bluebubbles".to_string(),
            text: None,
            gateway: None,
            chat_id: Some("iMessage;+15551234567".to_string()),
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: Some("love".to_string()),
            target_message_id: Some("message-guid-1".to_string()),
            target_author: None,
            remove_reaction: true,
            receipt_type: None,
            typing: None,
            mark_read: false,
            mark_unread: false,
            part_index: Some(1),
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };
        let (_, reaction_body) = build_channel_send_request("bluebubbles", &reaction_args)
            .expect("bluebubbles reaction request");
        assert_eq!(reaction_body["reaction"], json!("love"));
        assert_eq!(reaction_body["targetMessageId"], json!("message-guid-1"));
        assert_eq!(reaction_body["removeReaction"], json!(true));
        assert_eq!(reaction_body["partIndex"], json!(1));

        let typing_args = super::ChannelSendArgs {
            target: "bluebubbles".to_string(),
            text: None,
            gateway: None,
            chat_id: Some("iMessage;+15551234567".to_string()),
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: Some("start".to_string()),
            mark_read: false,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };
        let (_, typing_body) = build_channel_send_request("bluebubbles", &typing_args)
            .expect("bluebubbles typing request");
        assert_eq!(typing_body["typing"], json!("start"));

        let mark_read_args = super::ChannelSendArgs {
            target: "bluebubbles".to_string(),
            text: None,
            gateway: None,
            chat_id: Some("iMessage;+15551234567".to_string()),
            account_key: None,
            attachment_file: None,
            attachment_name: None,
            attachment_content_type: None,
            reaction: None,
            target_message_id: None,
            target_author: None,
            remove_reaction: false,
            receipt_type: None,
            typing: None,
            mark_read: true,
            mark_unread: false,
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
            group_members: vec![],
            group_admins: vec![],
            parse_mode: None,
            disable_notification: false,
            target_type: None,
            event_id: None,
            msg_id: None,
            msg_seq: None,
            is_wakeup: false,
        };
        let (_, mark_read_body) = build_channel_send_request("bluebubbles", &mark_read_args)
            .expect("bluebubbles mark-read request");
        assert_eq!(mark_read_body["markRead"], json!(true));
    }

    #[test]
    fn builds_ingress_secret_pairs_for_signal_and_bluebubbles() {
        let signal_args = super::IngressConnectArgs {
            target: "signal-cli".to_string(),
            gateway: None,
            secret: Some("signal-secret".to_string()),
            token: None,
            dm_policy: Some("pair".to_string()),
            allow_from: vec!["+15550001111".to_string(), "+15550002222".to_string()],
            env: Vec::new(),
        };
        let signal_pairs =
            ingress_secret_pairs("signal", &signal_args).expect("signal ingress secrets");
        assert_eq!(
            signal_pairs
                .get("DAWN_SIGNAL_CALLBACK_SECRET")
                .map(String::as_str),
            Some("signal-secret")
        );
        assert_eq!(
            signal_pairs
                .get("DAWN_SIGNAL_DM_POLICY")
                .map(String::as_str),
            Some("pairing")
        );
        assert_eq!(
            signal_pairs
                .get("DAWN_SIGNAL_ALLOWLIST")
                .map(String::as_str),
            Some("+15550001111,+15550002222")
        );

        let bluebubbles_args = super::IngressConnectArgs {
            target: "blue-bubbles".to_string(),
            gateway: None,
            secret: Some("blue-secret".to_string()),
            token: None,
            dm_policy: Some("allow_list".to_string()),
            allow_from: vec!["iMessage;+15550003333,+15550004444".to_string()],
            env: Vec::new(),
        };
        let bluebubbles_pairs = ingress_secret_pairs("bluebubbles", &bluebubbles_args)
            .expect("bluebubbles ingress secrets");
        assert_eq!(
            bluebubbles_pairs
                .get("DAWN_BLUEBUBBLES_CALLBACK_SECRET")
                .map(String::as_str),
            Some("blue-secret")
        );
        assert_eq!(
            bluebubbles_pairs
                .get("DAWN_BLUEBUBBLES_DM_POLICY")
                .map(String::as_str),
            Some("allowlist")
        );
        assert_eq!(
            bluebubbles_pairs
                .get("DAWN_BLUEBUBBLES_ALLOWLIST")
                .map(String::as_str),
            Some("iMessage;+15550003333,+15550004444")
        );
    }

    #[test]
    fn normalizes_ingress_aliases_for_new_targets() {
        assert_eq!(normalize_ingress_target_name("signal-cli"), "signal");
        assert_eq!(normalize_ingress_target_name("blue-bubbles"), "bluebubbles");
    }

    #[test]
    fn normalizes_ingress_dm_policy_aliases() {
        assert_eq!(
            super::normalize_dm_policy(Some("pair")).expect("pair policy"),
            Some("pairing".to_string())
        );
        assert_eq!(
            super::normalize_dm_policy(Some("allow_list")).expect("allowlist policy"),
            Some("allowlist".to_string())
        );
        assert_eq!(
            super::normalize_dm_policy(Some("off")).expect("disabled policy"),
            Some("disabled".to_string())
        );
        assert!(super::normalize_dm_policy(Some("unknown")).is_err());
    }

    #[test]
    fn suggests_easy_workspace_identity_from_placeholder_defaults() {
        let workspace = super::WorkspaceProfileRecord {
            tenant_id: super::DEFAULT_WORKSPACE_TENANT_ID.to_string(),
            project_id: super::DEFAULT_WORKSPACE_PROJECT_ID.to_string(),
            display_name: super::DEFAULT_WORKSPACE_DISPLAY_NAME.to_string(),
            region: super::DEFAULT_REGION.to_string(),
            default_model_providers: vec![],
            default_chat_platforms: vec![],
            onboarding_status: "bootstrap_pending".to_string(),
        };
        let suggested = super::suggest_setup_workspace_identity(&workspace, "Lenovo");
        assert_eq!(suggested.display_name, "Lenovo workspace");
        assert_eq!(suggested.tenant_id, "lenovo");
        assert_eq!(suggested.region, super::DEFAULT_REGION);
        assert!(suggested.project_id.ends_with("-desktop"));
    }

    #[test]
    fn preserves_existing_workspace_identity_when_not_placeholder() {
        let workspace = super::WorkspaceProfileRecord {
            tenant_id: "team-alpha".to_string(),
            project_id: "ops-hub".to_string(),
            display_name: "Alpha Ops".to_string(),
            region: "china".to_string(),
            default_model_providers: vec![],
            default_chat_platforms: vec![],
            onboarding_status: "configured".to_string(),
        };
        let suggested = super::suggest_setup_workspace_identity(&workspace, "Lenovo");
        assert_eq!(suggested.display_name, "Alpha Ops");
        assert_eq!(suggested.tenant_id, "team-alpha");
        assert_eq!(suggested.project_id, "ops-hub");
        assert_eq!(suggested.region, "china");
    }

    #[test]
    fn auto_launches_setup_when_profile_has_no_session() {
        assert!(super::should_auto_launch_setup(&DawnCliProfile::default()));
        let profile = DawnCliProfile {
            session_token: Some("session-123".to_string()),
            ..DawnCliProfile::default()
        };
        assert!(!super::should_auto_launch_setup(&profile));
    }
}
