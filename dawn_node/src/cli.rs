use std::collections::BTreeSet;

use anyhow::{Context, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fs, path::PathBuf, process::Command as StdCommand};

use crate::profile::{
    DawnCliProfile, default_gateway_base_url, load_profile_or_default, normalize_http_base_url,
    profile_path, save_profile,
};

pub enum CliOutcome {
    Exit,
    RunNode,
}

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
    Run,
    Login(LoginArgs),
    Onboard(OnboardArgs),
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

#[derive(Args)]
struct LoginArgs {
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long, default_value = "dawn-dev-bootstrap")]
    bootstrap_token: String,
    #[arg(long, default_value = "desktop-operator")]
    operator_name: String,
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
    text: String,
    #[arg(long)]
    gateway: Option<String>,
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

pub async fn dispatch_from_args() -> anyhow::Result<CliOutcome> {
    let cli = DawnCli::parse();
    let Some(command) = cli.command else {
        return Ok(CliOutcome::RunNode);
    };

    match command {
        Commands::Run => Ok(CliOutcome::RunNode),
        Commands::Login(args) => {
            login(args).await?;
            Ok(CliOutcome::Exit)
        }
        Commands::Onboard(args) => {
            onboard(args).await?;
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
    let mut profile = load_profile_or_default();
    let gateway_base_url = resolve_gateway_base_url(args.gateway.as_deref(), &profile);
    let client = GatewayClient::new(gateway_base_url.clone())?;
    let response = bootstrap_session(&client, &args.bootstrap_token, &args.operator_name).await?;

    profile.gateway_base_url = Some(gateway_base_url.clone());
    profile.session_token = Some(response.session_token);
    profile.operator_name = Some(response.session.operator_name.clone());
    profile.bootstrap_mode = Some(response.bootstrap_mode.clone());
    let path = save_profile(&profile)?;

    println!("Logged in to {gateway_base_url}");
    println!("Operator: {}", response.session.operator_name);
    println!("Bootstrap mode: {}", response.bootstrap_mode);
    println!("Profile saved: {}", path.display());
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
        ChannelCommand::Add { values, gateway } => {
            update_workspace_metadata(profile, gateway, values, false, true, "chat platforms").await
        }
        ChannelCommand::Remove { values, gateway } => {
            update_workspace_metadata(profile, gateway, values, false, false, "chat platforms")
                .await
        }
    }
}

async fn handle_ingress(args: IngressArgs) -> anyhow::Result<()> {
    let profile = load_profile_or_default();
    match args.command {
        IngressCommand::Connect(args) => connect_ingress_target(profile, args),
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
    let env_pairs = connector_secret_pairs(is_models, &target, &args)?;
    if env_pairs.is_empty() {
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
    let target = args.target.trim().to_ascii_lowercase();
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
    client
        .post_json(
            &format!("/api/gateway/connectors/model/{provider}/respond"),
            &json!({
                "input": input,
                "model": model,
                "instructions": instructions,
            }),
        )
        .await
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
        "google_chat" => "/api/gateway/connectors/chat/google-chat/send",
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
            json!({
                "chatId": chat_id,
                "text": args.text,
                "parseMode": args.parse_mode,
                "disableNotification": args.disable_notification,
            })
        }
        "slack" | "discord" | "mattermost" | "msteams" | "google_chat" | "feishu" | "dingtalk"
        | "wecom_bot" => {
            json!({
                "text": args.text,
            })
        }
        "wechat_official_account" => {
            let open_id = args.chat_id.as_deref().ok_or_else(|| {
                anyhow!("wechat_official_account send requires --chat-id as openId")
            })?;
            json!({
                "openId": open_id,
                "text": args.text,
            })
        }
        "qq" => {
            let recipient_id = args
                .chat_id
                .as_deref()
                .ok_or_else(|| anyhow!("qq send requires --chat-id as recipientId"))?;
            json!({
                "recipientId": recipient_id,
                "text": args.text,
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
            text: reply_text.clone(),
            gateway: None,
            chat_id: args.chat_id,
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
            "claude" => "anthropic".to_string(),
            "gemini" => "google".to_string(),
            "grok" => "groq".to_string(),
            "togetherai" => "together".to_string(),
            "openai-local" | "local-openai" => "vllm".to_string(),
            other => other.to_string(),
        }
    } else {
        match normalized.as_str() {
            "wecom" => "wecom_bot".to_string(),
            "teams" | "microsoft_teams" | "microsoft-teams" => "msteams".to_string(),
            "googlechat" | "google-chat" | "gchat" => "google_chat".to_string(),
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
        (true, "vllm") => {
            insert_if_some(&mut pairs, "VLLM_API_KEY", args.api_key.as_deref());
            insert_if_some(&mut pairs, "VLLM_BASE_URL", args.base_url.as_deref());
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
        (false, "google_chat") => {
            insert_if_some(
                &mut pairs,
                "GOOGLE_CHAT_BOT_WEBHOOK_URL",
                args.webhook_url.as_deref(),
            );
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
    use super::{
        ApprovalRequestSummary, PaymentRecordSummary, build_ap2_signature_payload,
        build_catalog_query, build_channel_send_request, build_chat_reply, connector_secret_pairs,
        extract_text_from_value, find_pending_approval_record, format_payment_approval_summary,
        normalize_connector_target, resolve_ap2_mcu_seed_hex, sign_ap2_payload, update_values,
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
        assert_eq!(normalize_connector_target(true, "grok"), "groq");
        assert_eq!(normalize_connector_target(true, "togetherai"), "together");
        assert_eq!(normalize_connector_target(true, "local-openai"), "vllm");
        assert_eq!(normalize_connector_target(false, "teams"), "msteams");
        assert_eq!(
            normalize_connector_target(false, "google-chat"),
            "google_chat"
        );
    }

    #[test]
    fn builds_secret_pairs_for_google_openrouter_groq_together_and_vllm() {
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
    }

    #[test]
    fn builds_send_requests_for_new_webhook_channels() {
        let args = super::ChannelSendArgs {
            target: "mattermost".to_string(),
            text: "hello channel".to_string(),
            gateway: None,
            chat_id: None,
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
    }
}
