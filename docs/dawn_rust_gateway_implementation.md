# Dawn Rust Gateway Implementation

## Summary

The Rust backend now has four active slices:

- `A2A` task intake with persistent SQLx/SQLite task/event state.
- `AP2` payment authorization with a two-step hardware signature flow.
- `Gateway` control-plane scaffolding for nodes, model connectors, and chat connectors.
- `Agent Card` publishing, discovery, remote invocation, and local `.well-known` exposure.
- `Chat ingress + Approval Center + Control Center` for inbound routing and operator action loops.
- `Marketplace` catalog + install endpoints for published agent cards and signed Wasm skills.

The project direction is now Rust-only for runtime startup. The legacy Django/Vue launch scripts should be treated as obsolete.

## Modules

- `dawn_core/src/main.rs`
  - Boots the Axum server and mounts `gateway`, `a2a`, and `ap2`.
- `dawn_core/src/app_state.rs`
  - Stores task records, payment records, node records, and command records in SQLite through SQLx, while keeping live node sessions in memory.
- `dawn_core/src/a2a.rs`
  - Accepts new tasks and tracks sandbox-binding state.
- `dawn_core/src/ap2.rs`
  - Creates pending payment transactions and verifies MCU signatures before authorizing them.
- `dawn_core/src/chat_ingress.rs`
  - Accepts inbound Telegram, Feishu, DingTalk, WeCom, WeChat Official Account, and QQ traffic, persists ingress events, and routes text into A2A tasks.
- `dawn_core/src/control_center.rs`
  - Serves the operator-facing dashboard at `/console`.
- `dawn_core/src/gateway.rs`
  - Exposes the high-level gateway status and nests the control-plane and connector routers.
- `dawn_core/src/marketplace.rs`
  - Exposes a public marketplace catalog, skill package install flow, agent-card install flow, and a lightweight `/marketplace` browser.
- `dawn_core/src/approval_center.rs`
  - Exposes a unified approval queue for pending node-command approvals and AP2 payment approvals.
- `dawn_core/src/control_plane.rs`
  - Handles node registration, heartbeats, queued commands, live WebSocket sessions, rollout bundles, and command results.
- `dawn_core/src/connectors.rs`
  - Exposes the first external connectors:
    - OpenAI Responses API
    - DeepSeek Chat Completions
    - Qwen via DashScope OpenAI-compatible Chat Completions
    - Zhipu BigModel Chat Completions
    - Moonshot Chat Completions
    - Doubao via Ark Chat Completions
    - Telegram Bot `sendMessage`
    - Feishu webhook bot
    - DingTalk webhook bot
    - WeCom webhook bot
- `dawn_core/src/agent_cards.rs`
  - Stores published and imported A2A Agent Cards, exposes registry search and remote invocation, and serves the active local card from `/.well-known/agent-card.json`.
- `dawn_core/src/skill_registry.rs`
  - Registers versioned Wasm skills, validates module bytes, verifies signed publisher envelopes, stores artifacts on disk, and exposes activation APIs for the A2A runtime.
- `dawn_node/src/main.rs`
  - A Rust node agent that connects to the gateway over WebSocket, emits heartbeats, verifies rollout trust roots, and returns command results for discovery, file, process, and shell capabilities.

## Key HTTP Endpoints

- `GET /health`
- `GET /.well-known/agent-card.json`
- `GET /.well-known/agent.json`
- `GET /.well-known/dawn-marketplace.json`
- `GET /api/gateway/status`
- `GET /api/gateway/capabilities`
- `GET /api/gateway/policy`
- `PUT /api/gateway/policy`
- `GET /api/gateway/policy/distribution`
- `PUT /api/gateway/policy/signed`
- `GET /api/gateway/policy/audit`
- `GET /api/gateway/policy/trust-roots`
- `POST /api/gateway/policy/trust-roots`
- `GET /api/gateway/approvals`
- `GET /api/gateway/approvals/{approval_id}`
- `POST /api/gateway/approvals/{approval_id}/decision`
- `GET /api/gateway/control-plane/nodes`
- `POST /api/gateway/control-plane/nodes/register`
- `GET /api/gateway/control-plane/nodes/trust-roots`
- `POST /api/gateway/control-plane/nodes/trust-roots`
- `POST /api/gateway/control-plane/nodes/{node_id}/heartbeat`
- `GET /api/gateway/control-plane/nodes/{node_id}/rollout`
- `POST /api/gateway/control-plane/nodes/{node_id}/rollout`
- `POST /api/gateway/control-plane/nodes/{node_id}/commands`
- `GET /api/gateway/control-plane/commands/{command_id}`
- `GET /api/gateway/control-plane/nodes/{node_id}/session`
  - WebSocket endpoint for live node sessions
- `GET /api/gateway/connectors/status`
- `POST /api/gateway/connectors/model/openai/respond`
- `POST /api/gateway/connectors/model/deepseek/respond`
- `POST /api/gateway/connectors/model/qwen/respond`
- `POST /api/gateway/connectors/model/zhipu/respond`
- `POST /api/gateway/connectors/model/moonshot/respond`
- `POST /api/gateway/connectors/model/doubao/respond`
- `POST /api/gateway/connectors/chat/telegram/send`
- `POST /api/gateway/connectors/chat/feishu/send`
- `POST /api/gateway/connectors/chat/dingtalk/send`
- `POST /api/gateway/connectors/chat/wecom/send`
- `POST /api/gateway/connectors/chat/wechat-official-account/send`
- `POST /api/gateway/connectors/chat/qq/send`
- `GET /api/gateway/ingress/status`
- `GET /api/gateway/ingress/events`
- `POST /api/gateway/ingress/telegram/webhook/{secret}`
- `POST /api/gateway/ingress/feishu/events`
- `POST /api/gateway/ingress/dingtalk/events`
- `GET /api/gateway/ingress/wecom/events`
- `POST /api/gateway/ingress/wecom/events`
- `GET /api/gateway/ingress/wechat-official-account/events`
- `POST /api/gateway/ingress/wechat-official-account/events`
- `POST /api/gateway/ingress/qq/events`
- `GET /api/gateway/marketplace/catalog`
- `POST /api/gateway/marketplace/install/skill`
- `POST /api/gateway/marketplace/install/agent-card`
- `GET /console`
- `GET /marketplace`
- `GET /api/gateway/agent-cards/status`
- `GET /api/gateway/agent-cards/`
- `GET /api/gateway/agent-cards/search`
- `GET /api/gateway/agent-cards/{card_id}/quote`
- `GET /api/gateway/agent-cards/quotes`
- `GET /api/gateway/agent-cards/quotes/{quote_id}`
- `GET /api/gateway/agent-cards/quotes/{quote_id}/state`
- `GET /api/gateway/agent-cards/invocations`
- `GET /api/gateway/agent-cards/settlements`
- `POST /api/gateway/agent-cards/publish`
- `POST /api/gateway/agent-cards/import`
- `POST /api/gateway/agent-cards/quotes/{quote_id}/revoke`
- `POST /api/gateway/agent-cards/{card_id}/quotes/{quote_id}/sync`
- `POST /api/gateway/agent-cards/{card_id}/invoke`
- `GET /api/gateway/agent-cards/{card_id}`
- `GET /api/gateway/agent-cards/invocations/{invocation_id}`
- `GET /api/gateway/agent-cards/invocations/{invocation_id}/settlement`
- `GET /api/gateway/agent-cards/settlements/{settlement_id}`
- `GET /api/gateway/skills/status`
- `GET /api/gateway/skills/distribution`
- `GET /api/gateway/skills/`
- `POST /api/gateway/skills/register`
- `POST /api/gateway/skills/register/signed`
- `GET /api/gateway/skills/trust-roots`
- `POST /api/gateway/skills/trust-roots`
- `GET /api/gateway/skills/{skill_id}`
- `GET /api/gateway/skills/{skill_id}/{version}`
- `POST /api/gateway/skills/{skill_id}/{version}/activate`
- `POST /api/a2a/task`
- `GET /api/a2a/tasks`
- `GET /api/a2a/task/{task_id}`
- `POST /api/ap2/authorize`
- `GET /api/ap2/transactions`

## A2A Orchestration

`POST /api/a2a/task` now supports two execution paths:

- `instruction` starts with `wasm:`
  - The task resolves a registered Wasm skill and executes it immediately when an active binding exists.
  - If no matching skill version is registered, the task remains in `awaiting_skill_binding`.
- `instruction` starts with `orchestrate:` and is followed by JSON
  - The task is queued and executed as a background orchestration graph.

Supported orchestration step kinds:

- `node_command`
- `model_connector`
- `chat_connector`
- `ap2_authorize`
- `remote_a2a_agent`

Example:

```json
{
  "name": "orchestration-smoke",
  "instruction": "orchestrate:{\"steps\":[{\"kind\":\"node_command\",\"nodeId\":\"node-local\",\"commandType\":\"agent_ping\"},{\"kind\":\"model_connector\",\"provider\":\"openai\",\"input\":\"Summarize {{last.result.nodeName}} from {{task.name}}\"},{\"kind\":\"chat_connector\",\"platform\":\"feishu\",\"text\":\"{{last.outputText}}\"}]}"
}
```

Wasm binding syntax:

- `wasm:skill_id`
  - execute the active version of `skill_id`
- `wasm:skill_id@version`
  - execute a specific version
- `wasm:skill_id@version#function_name`
  - override the registered entry function for one task

Template variables supported inside orchestration strings and JSON payloads:

- `{{task.id}}`
- `{{task.name}}`
- `{{task.instruction}}`
- `{{step.index}}`
- `{{last.json}}`
- `{{last.text}}`
- `{{last.<fieldPath>}}`

This allows one step to feed the next, for example:

- a node command returns machine state
- a model connector summarizes it
- a chat connector delivers the summary to a channel

## Chat Ingress And Control Center

The gateway now has a first inbound chat layer plus a lightweight operator console.

Ingress endpoints:

- `GET /api/gateway/ingress/status`
- `GET /api/gateway/ingress/events?limit=20`
- `POST /api/gateway/ingress/telegram/webhook/{secret}`
- `POST /api/gateway/ingress/feishu/events`
- `POST /api/gateway/ingress/dingtalk/events`
- `GET /api/gateway/ingress/wecom/events`
- `POST /api/gateway/ingress/wecom/events`
- `GET /api/gateway/ingress/wechat-official-account/events`
- `POST /api/gateway/ingress/wechat-official-account/events`
- `POST /api/gateway/ingress/qq/events`

Behavior:

- Telegram ingress validates `DAWN_TELEGRAM_WEBHOOK_SECRET` when configured.
- Feishu ingress supports the standard challenge response plus text-message delivery events.
- DingTalk ingress accepts text-message style callback payloads and optionally validates `DAWN_DINGTALK_CALLBACK_TOKEN` against the incoming JSON `token` field.
- WeCom ingress supports a lightweight `echostr` verification endpoint and accepts text-message style callback payloads; it can optionally validate `DAWN_WECOM_CALLBACK_TOKEN` against the incoming JSON token field.
- WeChat Official Account ingress supports the common `GET` signature verification handshake and `POST` XML text-message callback flow; when `DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN` is configured, the gateway verifies the standard `signature/timestamp/nonce` query triplet before accepting the request.
- QQ ingress accepts normalized JSON event callbacks and routes text events into A2A tasks after stripping leading bot mentions.
- inbound text is persisted in `chat_ingress_events` and then routed into the existing A2A task pipeline.
- `/orchestrate ...`, `/wasm ...`, and `/task ...` are normalized into their corresponding A2A instruction formats before task creation.
- successful ingress records link back to the created task, so the console can pivot from chat traffic to orchestration state.
- the current DingTalk and WeCom handlers intentionally target normalized JSON callbacks and do not yet implement encrypted enterprise callback envelopes.
- the current QQ ingress intentionally stops at normalized JSON event handling and does not yet implement full official callback signature verification.

Control Center:

- `GET /console` serves a live dashboard for inbound chat, tasks, nodes, settlements, and agent cards.
- the dashboard now includes an `Approval Center` feed for pending node-command and AP2 approvals.
- the dashboard now includes a `Node Command Console` for dispatching attested node capabilities such as `system_info`, `process_snapshot`, `list_directory`, `read_file_preview`, and `stat_path` through the existing control-plane API.
- the current console visual direction is a liquid-glass operations deck rather than a plain admin table, and it now includes command template chips plus a full command-detail inspector for recent node results.
- the console now has a unified right-side liquid-glass detail drawer so operators can inspect and act on approvals, node commands, and settlements without leaving the dashboard.
- the same control surface now also exposes remote invocations, quote-ledger rounds, and per-node rollout fabric state, all routed into the same detail drawer and operator action loop.
- the console now includes an `Agent Delegation Studio` for remote quote preview, counter-offer negotiation, and direct `Agent Card` invocation, plus a `Rollout Console` for manual dispatch of the current signed policy/skill bundle to a selected node.
- the console now also includes a `Marketplace Studio` for skill-package install, remote card import, and local card publishing, alongside a `Connector Matrix` that visualizes model/chat/ingress readiness across the current global and China connector set.
- approval decisions no longer rely on browser prompts; the right-side detail drawer now includes an inline workflow form for operator reason capture and AP2 MCU DID/signature entry.
- the same console now embeds a `Marketplace Catalog` browser so operators can search signed skills and published agent cards, then install or import them without leaving `/console`.
- the console now also subscribes to `/console/events` over SSE, so task, approval, ingress, node, command, rollout, payment, and policy writes can trigger near-real-time refresh instead of depending only on polling.
- the page refreshes against the existing API surface and does not require a separate frontend build pipeline.

Approval Center:

- `GET /api/gateway/approvals?status=pending`
- `GET /api/gateway/approvals/{approval_id}`
- `POST /api/gateway/approvals/{approval_id}/decision`
- `node_command` approvals are created when a node command is dispatched with `payload.approvalRequired = true` or when the command type is `shell_exec`.
- `payment` approvals are created automatically when AP2 enters `pending_physical_auth`.
- approving a node-command request releases the command into the existing control-plane queue.
- rejecting a node-command request marks the command as `failed`.
- approving a payment approval requires the same MCU DID + signature payload that `POST /api/ap2/authorize` already accepts.
- rejecting a payment approval marks the payment and any linked settlement as `rejected`.

## Marketplace

The gateway now exposes a public marketplace layer over the existing agent-card registry and signed skill registry.

Marketplace endpoints:

- `GET /.well-known/dawn-marketplace.json`
- `GET /api/gateway/marketplace/catalog`
- `POST /api/gateway/marketplace/install/skill`
- `POST /api/gateway/marketplace/install/agent-card`
- `GET /marketplace`

Behavior:

- the catalog combines published agent cards with signed skill versions into one discovery surface
- skill entries expose a concrete package URL at `/api/gateway/skills/{skill_id}/{version}/package`
- package export includes skill metadata, signed envelope when present, and the Wasm artifact encoded as `wasmBase64`
- remote skill installation can fetch a package URL and register it locally; signed packages are verified against existing skill-publisher trust roots
- remote agent-card installation reuses the existing import path and stores the imported card in the local registry
- if `DAWN_PUBLIC_BASE_URL` is configured, marketplace package and install URLs are emitted as absolute URLs; otherwise they remain root-relative

An orchestration graph can also pause on payment approval:

- `ap2_authorize` creates a pending AP2 transaction
- the task moves to `waiting_payment_authorization`
- once a valid MCU signature is submitted to `POST /api/ap2/authorize`, DawnCore resumes the orchestration automatically from the next step

An orchestration graph can also delegate to another discovered agent:

- `remote_a2a_agent` resolves a published or imported Agent Card by `cardId`
- DawnCore posts a task to the remote agent's `A2A` endpoint and records a `remote_agent_invocations` row
- when `awaitCompletion = true`, DawnCore polls the remote task status until it reaches a terminal state
- `remote_a2a_agent` can optionally include a `settlement` block
  - DawnCore only creates the AP2 transaction after the remote invocation has completed successfully
  - the gateway persists a `remote_agent_settlements` row that links `invocationId` to `transactionId`
  - if the step requested settlement, orchestration pauses in `waiting_payment_authorization` until `POST /api/ap2/authorize` verifies the MCU signature

Example step:

```json
{
  "kind": "remote_a2a_agent",
  "cardId": "local-travel-agent",
  "name": "delegate-booking",
  "instruction": "wasm:echo-skill",
  "awaitCompletion": true,
  "timeoutSeconds": 10,
  "settlement": {
    "mandateId": "11111111-1111-1111-1111-111111111111",
    "amount": 18.5,
    "description": "Settle {{task.name}}"
  }
}
```

## Agent Card Registry

Because A2A is a core differentiator, the gateway now includes Agent Card publishing, search, import, and `.well-known` discovery.

Agent Card endpoints:

- `GET /.well-known/agent-card.json`
- `GET /.well-known/agent.json`
- `GET /api/gateway/agent-cards/status`
- `GET /api/gateway/agent-cards/`
- `GET /api/gateway/agent-cards/search`
- `GET /api/gateway/agent-cards/{card_id}/quote`
- `GET /api/gateway/agent-cards/quotes`
- `GET /api/gateway/agent-cards/quotes/{quote_id}`
- `GET /api/gateway/agent-cards/quotes/{quote_id}/state`
- `GET /api/gateway/agent-cards/invocations`
- `GET /api/gateway/agent-cards/settlements`
- `POST /api/gateway/agent-cards/publish`
- `POST /api/gateway/agent-cards/import`
- `POST /api/gateway/agent-cards/quotes/{quote_id}/revoke`
- `POST /api/gateway/agent-cards/{card_id}/quotes/{quote_id}/sync`
- `POST /api/gateway/agent-cards/{card_id}/invoke`
- `GET /api/gateway/agent-cards/{card_id}`
- `GET /api/gateway/agent-cards/invocations/{invocation_id}`
- `GET /api/gateway/agent-cards/invocations/{invocation_id}/settlement`
- `GET /api/gateway/agent-cards/settlements/{settlement_id}`

`POST /api/gateway/agent-cards/{card_id}/invoke` now also accepts an optional `settlement` block:

```json
{
  "name": "delegate-booking",
  "instruction": "wasm:echo-skill",
  "awaitCompletion": true,
  "settlement": {
    "mandateId": "11111111-1111-1111-1111-111111111111",
    "amount": 18.5,
    "description": "Settle delegated booking"
  }
}
```

The response now includes:

- `invocation`
- `remoteStatus`
- `settlement`
  - present only when AP2 settlement was requested and the remote invocation reached a terminal success state

Agent Cards can now also expose AP2 pricing metadata through the AP2 extension params, for example:

```json
{
  "uri": "https://github.com/google-agentic-commerce/ap2/tree/v0.1",
  "params": {
    "roles": ["payee"],
    "pricing": {
      "currency": "CNY",
      "quoteMode": "flat",
      "quoteMethod": "GET",
      "quoteUrl": "https://gateway.example.com/api/gateway/agent-cards/local-travel-agent/quote",
      "quoteStateUrlTemplate": "https://gateway.example.com/api/gateway/agent-cards/quotes/{quoteId}/state",
      "quoteIssuerDid": "did:dawn:quote:...",
      "flatAmount": 18.5,
      "minAmount": 10.0,
      "maxAmount": 20.0,
      "descriptionTemplate": "Settle travel booking"
    }
  }
}
```

`GET /api/gateway/agent-cards/{card_id}/quote?requestedAmount=12.0` now returns:

- whether the card advertises settlement capability
- the normalized payment roles
- pricing metadata such as `currency`, `quoteMode`, `flatAmount`, `minAmount`, and `maxAmount`
- a warning when the requested amount differs from the flat quote or falls outside the advertised range
- for locally hosted cards, a signed quote object with `quoteId`, `expiresAtUnixMs`, `issuerDid`, and `signatureHex`

If the card advertises `pricing.quoteUrl` or `pricing.quotePath`, `GET /api/gateway/agent-cards/{card_id}/quote?requestedAmount=12.0&remote=true` now performs a live remote quote fetch. DawnCore accepts either the native `AgentSettlementQuote` JSON shape or a looser `{ "quote": { ... } }` envelope.

The quote endpoint also supports `quoteId` and `counterOfferAmount` so a client can negotiate a second-pass quote:

- `GET /api/gateway/agent-cards/{card_id}/quote?requestedAmount=12.0&counterOfferAmount=13.2&quoteId=<prior-quote-id>`
- for `manual` / `negotiated` pricing, DawnCore can accept the counter-offer and mint a new signed quote
- for `flat` / `fixed` pricing, DawnCore keeps the original quote and returns a warning that the counter-offer was not accepted

Every signed or negotiated quote is now persisted in a local replay ledger:

- `GET /api/gateway/agent-cards/quotes` lists ledger entries and supports `cardId`, `status`, `sourceKind`, and `transactionId` filters
- `GET /api/gateway/agent-cards/quotes/{quote_id}` returns the full quote lifecycle record
- `GET /api/gateway/agent-cards/quotes/{quote_id}/state` returns a signed quote-state snapshot for locally issued quotes
- `POST /api/gateway/agent-cards/quotes/{quote_id}/revoke` marks an unconsumed quote as revoked
- `POST /api/gateway/agent-cards/{card_id}/quotes/{quote_id}/sync` fetches and applies the latest signed quote-state snapshot from the remote issuer
- when a new quote references `quoteId=<prior-quote-id>`, the prior quote is marked `superseded` and the new quote advances the `negotiationRound`
- when a remote settlement is created from an accepted quote, the quote is marked `consumed` and linked to the AP2 `transactionId`, which blocks replay
- when a remote card advertises `quoteStateUrlTemplate`, DawnCore verifies the issuer-signed remote state before authorizing settlement, so revoked, superseded, or already-consumed quotes are rejected before AP2 authorization starts

When a locally hosted card is published through DawnCore, the gateway now auto-populates `pricing.quoteUrl` when the AP2 extension exists but no quote endpoint is declared. That lets another Dawn gateway discover and negotiate against the local quote endpoint without hand-editing the card JSON.

When a settlement request is actually submitted, DawnCore now validates the amount against the negotiated quote first, verifies signed remote quotes when `issuerDid/signatureHex` are present, enforces `expiresAtUnixMs`, and only falls back to metadata pricing if the remote quote endpoint is unavailable and fallback is allowed.

Publish request shape:

```json
{
  "cardId": "local-travel-agent",
  "locallyHosted": true,
  "published": true,
  "regions": ["china"],
  "languages": ["zh-CN", "en-US"],
  "modelProviders": ["qwen", "deepseek"],
  "chatPlatforms": ["wechat_official_account", "qq"],
  "card": {
    "name": "Local Travel Agent",
    "description": "Books domestic travel and supports AP2 payments.",
    "url": "https://gateway.example.com/api/a2a",
    "version": "1.0.0",
    "authentication": {
      "schemes": ["bearer"]
    },
    "defaultInputModes": ["text"],
    "defaultOutputModes": ["text"],
    "capabilities": {
      "streaming": true,
      "pushNotifications": true,
      "stateTransitionHistory": true,
      "extensions": [
        {
          "uri": "https://github.com/google-agentic-commerce/ap2/tree/v0.1",
          "params": {
            "roles": ["payee"]
          }
        }
      ]
    },
    "skills": [
      {
        "id": "travel-booking",
        "name": "Travel Booking",
        "tags": ["travel", "china"]
      }
    ]
  }
}
```

Registry behavior:

- local published cards are exposed through `/.well-known/agent-card.json`
- `import` can fetch either a direct card URL or a base URL and will probe common discovery paths
- search filters support `q`, `skillId`, `skillTag`, `region`, `language`, `modelProvider`, `chatPlatform`, `paymentRole`, `streaming`, and `pushNotifications`
- AP2 payment roles are extracted automatically from the AP2 extension when present
- imported and published cards share the same persisted registry table
- the registry stores metadata needed for remote A2A discovery and invocation
- remote invocations are persisted with local task linkage, remote task ids, last remote response payload, and final status

Invoke request shape:

```json
{
  "name": "delegate-echo",
  "instruction": "wasm:echo-skill",
  "awaitCompletion": true,
  "timeoutSeconds": 10
}
```

## Wasm Skill Registry

The gateway now has a persisted Wasm skill registry. Skill metadata lives in SQLite, module binaries are stored under the skill artifact root on disk, and the registry can accept signed skill bundles from trusted publishers.

Skill registry endpoints:

- `GET /api/gateway/skills/status`
- `GET /api/gateway/skills/distribution`
- `GET /api/gateway/skills/`
- `POST /api/gateway/skills/register`
- `POST /api/gateway/skills/register/signed`
- `GET /api/gateway/skills/trust-roots`
- `POST /api/gateway/skills/trust-roots`
- `GET /api/gateway/skills/{skill_id}`
- `GET /api/gateway/skills/{skill_id}/{version}`
- `POST /api/gateway/skills/{skill_id}/{version}/activate`

Register request shape:

```json
{
  "skillId": "echo-skill",
  "version": "1.0.0",
  "displayName": "Echo Skill",
  "description": "minimal smoke skill",
  "entryFunction": "run_skill",
  "capabilities": ["echo"],
  "wasmBase64": "AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL",
  "activate": true
}
```

Registry behavior:

- each `(skillId, version)` pair is stored in `wasm_skills`
- module bytes are validated by Wasmtime before registration succeeds
- active version selection is per-skill
- A2A `wasm:` tasks resolve the active version unless a specific version is requested
- default artifact root is `dawn_core/data/skills`
- override artifact root with `DAWN_SKILL_ARTIFACTS_DIR`
- unsigned local registration is still allowed for development
- signed registration requires a trusted publisher DID in the form `did:dawn:skill-publisher:{ed25519_public_key_hex}`
- the signed envelope covers skill metadata plus `artifactSha256`, and DawnCore verifies that hash against the uploaded Wasm bytes before accepting the skill

Signed registration shape:

```json
{
  "envelope": {
    "document": {
      "skillId": "echo-skill",
      "version": "1.0.0",
      "displayName": "Echo Skill",
      "description": "signed smoke skill",
      "entryFunction": "run_skill",
      "capabilities": ["echo"],
      "artifactSha256": "{sha256_hex_of_wasm_bytes}",
      "issuerDid": "did:dawn:skill-publisher:{ed25519_public_key_hex}",
      "issuedAtUnixMs": 1700000000000
    },
    "signatureHex": "{ed25519_signature_hex}"
  },
  "wasmBase64": "AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL",
  "activate": true
}
```

## Policy Layer

The gateway now has a persisted, versioned policy profile plus audit history. The orchestration engine reads the active policy profile before risky steps execute. The policy layer now also supports signed policy distribution with Ed25519 issuer verification and an explicit trust-root allowlist.

Policy control-plane endpoints:

- `GET /api/gateway/policy`
- `PUT /api/gateway/policy`
- `GET /api/gateway/policy/distribution`
- `PUT /api/gateway/policy/signed`
- `GET /api/gateway/policy/audit`
- `GET /api/gateway/policy/trust-roots`
- `POST /api/gateway/policy/trust-roots`

`PUT /api/gateway/policy` updates the active policy, bumps the version, and writes an audit event with an actor, reason, and full policy snapshot.

`PUT /api/gateway/policy/signed` activates a signed policy envelope after the gateway verifies:

- the issuer DID is self-certifying in the form `did:dawn:policy:{ed25519_public_key_hex}`
- the issuer exists in `policy_trust_roots`
- the Ed25519 signature matches the serialized policy document
- the signed version is newer than the currently active version

`GET /api/gateway/policy/distribution` returns the active policy profile plus its signed envelope when the active version was activated from a trusted issuer.

Trust-root management:

- `POST /api/gateway/policy/trust-roots` upserts a trusted issuer
- `GET /api/gateway/policy/trust-roots` lists trusted issuers known to the gateway

Current policy gates:

- `shell_exec`
  - Denied by default unless `allowShellExec = true`
- model providers
  - Optionally restricted by `allowedModelProviders`
- chat platforms
  - Optionally restricted by `allowedChatPlatforms`
- AP2 payments
  - Denied if amount is not positive
  - Optionally capped by `maxPaymentAmount`

Policy decisions are written into the task event stream as `policy_decision`.

Signed policy document shape:

```json
{
  "document": {
    "policyId": "default",
    "version": 7,
    "issuerDid": "did:dawn:policy:{ed25519_public_key_hex}",
    "issuedAtUnixMs": 1700000000000,
    "allowShellExec": false,
    "allowedModelProviders": ["deepseek", "qwen"],
    "allowedChatPlatforms": ["feishu", "wecom_bot"],
    "maxPaymentAmount": 50.0,
    "updatedReason": "signed policy rollout"
  },
  "signatureHex": "{ed25519_signature_hex}"
}
```

## Node Session Message Shapes

## Node Attestation

The gateway now supports signed node capability tokens. A node is visible to the control plane even before it is trusted, but command dispatch is blocked until the node presents a verifiable capability attestation from a trusted issuer.

Node trust-root endpoints:

- `GET /api/gateway/control-plane/nodes/trust-roots`
- `POST /api/gateway/control-plane/nodes/trust-roots`

Node issuer DID format:

- `did:dawn:node:{ed25519_public_key_hex}`

Node capability attestation shape:

```json
{
  "document": {
    "nodeId": "node-alpha",
    "issuerDid": "did:dawn:node:{ed25519_public_key_hex}",
    "issuedAtUnixMs": 1700000000001,
    "displayName": "Node Alpha",
    "transport": "websocket",
    "capabilities": ["agent_ping", "echo"]
  },
  "signatureHex": "{ed25519_signature_hex}"
}
```

Gateway behavior:

- the node record stores the latest attestation issuer, signature hash, verification state, and error
- command dispatch is denied unless `attestationVerified = true`
- command dispatch is denied if the requested command is not in the attested capability list
- once a trusted attestation arrives, the gateway uses the attested capability list for subsequent dispatches

## Cross-Node Rollout

The control plane now supports cross-node rollout of the active signed policy distribution plus the current skill distribution summary. Rollout dispatch is blocked unless the target node is already attested by a trusted issuer.

Rollout endpoints:

- `GET /api/gateway/control-plane/nodes/{node_id}/rollout`
- `POST /api/gateway/control-plane/nodes/{node_id}/rollout`

Gateway behavior:

- the rollout bundle contains the active policy distribution, the current skill distribution, a stable `bundleHash`, the active `policyVersion`, and a `skillDistributionHash`
- rollout state is stored per node in `node_rollouts`
- if a node is offline, the rollout is persisted as `pending`
- if a node is connected and attested, the rollout is sent immediately over the existing WebSocket session
- a verified node heartbeat or session reconnect will retry the current rollout when the stored bundle is outdated or still unacknowledged
- `dawn_node` can independently verify the signed policy envelope and signed skill publisher records before acknowledging the rollout
- strict node-side rollout enforcement is opt-in so local development can still run without pre-seeded trust roots

Gateway to node rollout message:

```json
{
  "messageType": "rollout_bundle",
  "nodeId": "node-alpha",
  "bundle": {
    "generatedAtUnixMs": 1700000000100,
    "bundleHash": "{sha256_hex}",
    "policyVersion": 7,
    "policyDocumentHash": "{sha256_hex}",
    "skillDistributionHash": "{sha256_hex}",
    "policy": {
      "profile": {
        "policyId": "default"
      }
    },
    "skills": {
      "skills": []
    }
  }
}
```

Node to gateway acknowledgment:

```json
{
  "messageType": "rollout_ack",
  "bundleHash": "{sha256_hex}",
  "accepted": true,
  "policyVersion": 7,
  "skillDistributionHash": "{sha256_hex}"
}
```

### Gateway to node

```json
{
  "messageType": "command_dispatch",
  "nodeId": "node-alpha",
  "commandId": "uuid",
  "commandType": "shell_exec",
  "payload": {
    "command": "dir"
  }
}
```

### Node to gateway

```json
{
  "messageType": "command_result",
  "commandId": "uuid",
  "status": "succeeded",
  "result": {
    "stdout": "..."
  }
}
```

Heartbeat updates can be sent as:

```json
{
  "messageType": "heartbeat",
  "displayName": "Office Node",
  "capabilities": ["agent_ping", "echo"],
  "capabilityAttestation": {
    "document": {
      "nodeId": "node-alpha",
      "issuerDid": "did:dawn:node:{ed25519_public_key_hex}",
      "issuedAtUnixMs": 1700000000001,
      "displayName": "Office Node",
      "transport": "websocket",
      "capabilities": ["agent_ping", "echo"]
    },
    "signatureHex": "{ed25519_signature_hex}"
  }
}
```

Supported node command types in the sample Rust node:

- `echo`
- `list_capabilities`
- `agent_ping`
- `system_info`
- `list_directory`
- `read_file_preview`
- `stat_path`
- `process_snapshot`
- `shell_exec`
  - Disabled by default
  - Enable with `DAWN_NODE_ALLOW_SHELL=1`
  - Only attested when shell is enabled

Device-facing node behavior:

- `system_info` returns node identity, OS, architecture, current working directory, executable path, CPU count, hostname, and effective capabilities.
- `list_directory` lists directory entries with basic metadata and a bounded result size.
- `read_file_preview` reads a bounded UTF-8 preview of a file without requiring shell access.
- `stat_path` returns basic filesystem metadata for a file or directory.
- `process_snapshot` returns a bounded process list using `tasklist` on Windows or `ps` on Unix-like hosts.

## AP2 Flow

1. Client or orchestrator posts a payment request without `transactionId` and without `mcuSignature`.
2. DawnCore creates a transaction in `pending_physical_auth`.
3. MCU signs the payload:

```text
{transaction_id}:{mandate_id}:{amount:.4}:{description}
```

4. MCU posts the same request back with:
   - `transactionId`
   - `mcuPublicDid`
   - `mcuSignature`
5. DawnCore verifies the Ed25519 signature and moves the payment to `authorized` or `rejected`.

## Connector Configuration

## Persistence

- Default database URL: `sqlite://data/dawn_core.db`
- Override with: `DAWN_DATABASE_URL`
- DawnCore creates the `data/` directory automatically when using the default SQLite path.
- Tables created on boot:
  - `tasks`
  - `task_events`
  - `payments`
  - `nodes`
  - `node_commands`
  - `node_rollouts`
  - `orchestration_runs`
  - `agent_cards`
  - `remote_agent_invocations`
  - `wasm_skills`
  - `skill_publisher_trust_roots`
  - `policy_profiles`
  - `policy_audit_events`
  - `policy_trust_roots`

This gives the gateway restart-safe task, payment, node, command, and orchestration checkpoint state without pulling in Postgres yet. The repository shape is still compatible with a later SQLx/Postgres migration.

### OpenAI

- Environment variable: `OPENAI_API_KEY`
- Live endpoint used by the gateway: `POST https://api.openai.com/v1/responses`
- If the key is missing, the connector returns `mode = dry_run`

### DeepSeek

- Environment variable: `DEEPSEEK_API_KEY`
- Live endpoint used by the gateway: `POST https://api.deepseek.com/chat/completions`
- If the key is missing, the connector returns `mode = dry_run`

### Qwen

- Environment variables:
  - `QWEN_API_KEY`
  - or `DASHSCOPE_API_KEY`
- Optional endpoint override: `QWEN_CHAT_COMPLETIONS_URL`
- Live endpoint used by the gateway:
  - `POST https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions`
- Default model: `qwen-plus`
- If the key is missing, the connector returns `mode = dry_run`

### Zhipu

- Environment variable: `ZHIPU_API_KEY`
- Optional endpoint override: `ZHIPU_CHAT_COMPLETIONS_URL`
- Live endpoint used by the gateway:
  - `POST https://open.bigmodel.cn/api/paas/v4/chat/completions`
- Default model: `glm-4.5-air`
- If the key is missing, the connector returns `mode = dry_run`

### Moonshot

- Environment variable: `MOONSHOT_API_KEY`
- Optional endpoint override: `MOONSHOT_CHAT_COMPLETIONS_URL`
- Live endpoint used by the gateway:
  - `POST https://api.moonshot.cn/v1/chat/completions`
- Default model: `moonshot-v1-8k`
- If the key is missing, the connector returns `mode = dry_run`

### Doubao

- Environment variables:
  - `DOUBAO_API_KEY`
  - or `ARK_API_KEY`
- Endpoint id source:
  - request `model`
  - or `DOUBAO_ENDPOINT_ID`
  - or `ARK_MODEL_ENDPOINT_ID`
- Optional endpoint override: `DOUBAO_CHAT_COMPLETIONS_URL`
- Live endpoint used by the gateway:
  - `POST https://ark.cn-beijing.volces.com/api/v3/chat/completions`
- If the key is missing, the connector returns `mode = dry_run`

### Telegram

- Environment variable: `TELEGRAM_BOT_TOKEN`
- Live endpoint used by the gateway: `POST https://api.telegram.org/bot{token}/sendMessage`
- If the token is missing, the connector returns `mode = dry_run`

### China chat webhook connectors

- `FEISHU_BOT_WEBHOOK_URL`
- `DINGTALK_BOT_WEBHOOK_URL`
- `WECOM_BOT_WEBHOOK_URL`

If a webhook URL is missing, the connector returns `mode = dry_run`.

### WeChat Official Account

- Supported path:
  - `POST /api/gateway/connectors/chat/wechat-official-account/send`
- Request shape:

```json
{
  "openId": "user-openid",
  "text": "hello from dawn"
}
```

- Credential options:
  - `WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN`
  - or `WECHAT_OFFICIAL_ACCOUNT_APP_ID` plus `WECHAT_OFFICIAL_ACCOUNT_APP_SECRET`
- Live token flow used by the gateway:
  - `GET https://api.weixin.qq.com/cgi-bin/token?grant_type=client_credential&appid=...&secret=...`
- Live message flow used by the gateway:
  - `POST https://api.weixin.qq.com/cgi-bin/message/custom/send?access_token=...`
- Current message type:
  - text custom service message to a known `openId`
- If credentials are missing, the connector returns `mode = dry_run`

### QQ Official Bot

- Supported path:
  - `POST /api/gateway/connectors/chat/qq/send`
- Request shape:

```json
{
  "recipientId": "user-or-group-openid",
  "targetType": "user",
  "text": "hello from dawn",
  "eventId": "optional-event-id",
  "msgId": "optional-msg-id",
  "msgSeq": 1,
  "isWakeup": false
}
```

- Supported `targetType` values:
  - `user`
  - `group`
- Credential requirements:
  - `QQ_BOT_APP_ID`
  - `QQ_BOT_CLIENT_SECRET`
- Live token flow used by the gateway:
  - `POST https://bots.qq.com/app/getAppAccessToken`
- Live message flows used by the gateway:
  - `POST https://api.sgroup.qq.com/v2/users/{recipientId}/messages`
  - `POST https://api.sgroup.qq.com/v2/groups/{recipientId}/messages`
- If credentials are missing, the connector returns `mode = dry_run`

### Node identity

- Optional environment variable: `DAWN_NODE_SIGNING_SEED_HEX`
- If omitted, `dawn_node` derives a deterministic development identity from `DAWN_NODE_ID`
- The derived identity is convenient for local development, but production deployments should set an explicit signing seed and register the issuer via `POST /api/gateway/control-plane/nodes/trust-roots`
- Optional environment variable: `DAWN_NODE_POLICY_TRUST_ROOTS`
  - Format: `did:dawn:policy:{pubkey_hex}={pubkey_hex},did:dawn:policy:{pubkey_hex2}={pubkey_hex2}`
- Optional environment variable: `DAWN_NODE_SKILL_PUBLISHER_TRUST_ROOTS`
  - Format: `did:dawn:skill-publisher:{pubkey_hex}={pubkey_hex}`
- Optional environment variable: `DAWN_NODE_ENFORCE_TRUSTED_ROLLOUT`
  - If set to `1`, the node rejects rollout bundles when policy or signed skill verification fails
- Optional environment variable: `DAWN_NODE_REQUIRE_SIGNED_SKILLS`
  - If set to `1`, unsigned development skills are rejected during rollout verification

## China Support Direction

The gateway capability model now explicitly includes China-facing providers and chat ecosystems.

Live now:

- `deepseek`
- `qwen`
- `zhipu`
- `moonshot`
- `doubao`
- `feishu`
- `dingtalk`
- `wecom_bot`
- `wechat_official_account`
- `qq`

Planned adapter path:

- deeper WeChat Official Account coverage beyond text custom service messages
- broader QQ surfaces beyond `user` and `group` OpenAPI messaging

This means the gateway data model and public capability surface already account for Chinese deployment targets, even where a production adapter is still pending.

## Rust Startup

Use the Rust launcher:

- [Start-DawnRust.ps1](/D:/Agent2Agent应用/Start-DawnRust.ps1)

This script starts:

- `dawn_core`
- `dawn_node`

By default, DawnCore persists to:

- `dawn_core/data/dawn_core.db`

Removed legacy launchers:

- `start-a2a-dev.ps1`
- `start-a2a-simple.ps1`
- `Start-A2A.ps1`
- `Start-A2APlatform.ps1`

## Verification Completed

- `cargo check` in `dawn_core`
- `cargo check` in `dawn_mcu`
- `cargo check` in `dawn_node`
- `cargo test` in `dawn_core`
- Runtime smoke test on `http://127.0.0.1:8000/health`
- Runtime orchestration smoke test:
  - `A2A task -> node_command(agent_ping) -> model_connector(openai dry_run) -> chat_connector(feishu dry_run)`
- Runtime AP2 pause/resume smoke test:
  - `A2A task -> ap2_authorize -> MCU signature submit -> node_command -> model_connector -> chat_connector`
- Runtime policy-control smoke test:
  - `PUT /api/gateway/policy` restricted providers to `deepseek`
  - `A2A task -> model_connector(openai)` failed with a policy denial
  - `GET /api/gateway/policy/audit` returned the versioned change history
- Runtime China connector smoke test:
  - `GET /api/gateway/connectors/status` reported `qwen`, `zhipu`, `moonshot`, and `doubao` as live-capable providers
  - `POST /api/gateway/connectors/model/qwen/respond`
  - `POST /api/gateway/connectors/model/zhipu/respond`
  - `POST /api/gateway/connectors/model/moonshot/respond`
  - `POST /api/gateway/connectors/model/doubao/respond`
  - all four returned `mode = dry_run` with the expected provider-specific default model when API keys were not configured
- Runtime China chat ingress smoke test:
  - `GET /api/gateway/connectors/status` reported `wechat_official_account` and `qq` as live-capable chat platforms
  - `POST /api/gateway/connectors/chat/wechat-official-account/send`
  - `POST /api/gateway/connectors/chat/qq/send`
  - both returned `mode = dry_run` with the expected missing-credential reason and echoed target identifiers
- Runtime inbound chat ingress tests:
  - Telegram webhook ingress persists a `chat_ingress_events` row and creates a linked A2A task
  - Feishu event ingress supports the challenge round-trip without creating a task
- Approval Center tests:
  - approval-required node commands now persist a pending approval request before dispatch
  - approving the request releases the node command back into the queued control-plane path
  - `POST /api/gateway/approvals/{approval_id}/decision` can reject a pending AP2 transaction and updates both the approval record and payment state
- Marketplace tests:
  - the marketplace catalog now includes signed skill packages and published agent cards in one combined discovery document
  - signed skill entries expose installable package URLs
- Runtime Agent Card smoke test:
  - `POST /api/gateway/agent-cards/publish` published a locally hosted `local-travel-agent`
  - `GET /.well-known/agent-card.json` returned the active local card
  - `GET /api/gateway/agent-cards/search?q=travel&chatPlatform=wechat_official_account&paymentRole=payee` returned the published card
  - `POST /api/gateway/agent-cards/import` imported the same card back through the `.well-known` URL as `imported-travel-agent`
- Runtime remote A2A smoke test:
  - `POST /api/gateway/agent-cards/local-travel-agent/invoke` dispatched `instruction = "wasm:echo-skill"` to the published local card and persisted a `remote_agent_invocations` row with `status = completed`
  - `GET /api/gateway/agent-cards/invocations?cardId=local-travel-agent` returned the persisted invocation record and remote task id
  - `POST /api/a2a/task` with `instruction = "orchestrate:{...remote_a2a_agent...}"` completed successfully and delegated one step through the Agent Card registry
- Runtime remote settlement smoke test:
  - `POST /api/gateway/agent-cards/settlement-agent/invoke` with a `settlement` block created a `remote_agent_settlements` row and an AP2 transaction in `pending_physical_auth`
  - `POST /api/ap2/authorize` with a valid MCU signature transitioned both the payment record and the linked settlement record to `authorized`
  - `GET /api/gateway/agent-cards/settlements/{settlement_id}` returned the synchronized settlement status
- Agent Card quote tests:
  - AP2 extension pricing metadata now parses `currency`, `quoteMode`, `flatAmount`, `minAmount`, and `maxAmount`
  - quote generation warns when a requested amount differs from an advertised flat quote
  - locally hosted cards now auto-advertise a concrete `quoteUrl`
  - locally hosted quote responses are now signed and carry `quoteId`, `expiresAtUnixMs`, `issuerDid`, and `signatureHex`
  - counter-offers are accepted for `manual` quotes and rejected for `flat` quotes
  - DawnCore can fetch a live remote quote from the declared quote endpoint before settlement validation
  - DawnCore verifies a signed remote quote before using it for settlement validation
  - settlement creation is rejected when the requested amount exceeds the agent-card `maxAmount`
  - a second-pass counter-offer supersedes the prior quote and increments `negotiationRound`
  - revoked quotes cannot be consumed into AP2 settlements
  - consumed quotes are linked to `transactionId` and reject replay consumption
  - remote settlement validation now rejects a signed remote quote when the issuer's signed quote-state endpoint reports that quote as revoked
- Runtime Wasm registry smoke test:
  - `POST /api/gateway/skills/register` accepted a minimal `echo-skill@1.0.0`
  - `GET /api/gateway/skills/echo-skill` returned the active version and stored artifact hash
  - `POST /api/a2a/task` with `instruction = "wasm:echo-skill"` completed successfully
  - task events recorded `skill_binding_resolved` and `skill_executed`
- Signed skill distribution test:
  - `POST /api/gateway/skills/trust-roots` seeded a trusted publisher DID derived from an Ed25519 public key
  - `POST /api/gateway/skills/register/signed` accepted a signed `echo-skill@1.0.0` envelope after verifying publisher DID, signature, and `artifactSha256`
  - the stored skill version now persists `sourceKind`, `issuerDid`, `signatureHex`, and `documentHash`
- Signed policy activation test:
  - trusted issuer DID derived from Ed25519 public key
  - signed policy envelope verified and activated through the policy service
  - active profile persisted with issuer DID, signature hex, and document hash
- Node attestation tests:
  - self-certifying node DID derived from Ed25519 public key
  - trusted node capability attestation verified and converted into gateway attestation state
  - dispatch gating now depends on `attestationVerified` plus attested capabilities
- Control-plane rollout tests:
  - an attested but offline node now receives a persisted `pending` rollout state built from the active policy and skill distributions
  - a matching `rollout_ack` transitions the stored node rollout state to `acknowledged`
- Node rollout verification tests:
  - a signed rollout bundle verifies successfully when the node has matching local policy and skill publisher trust roots
  - strict signed-skill mode rejects a rollout bundle that contains an unsigned development skill
- In-process gateway/node smoke test:
  - `cargo test` now runs an in-process HTTP + WebSocket loop that seeds trusted node/policy/skill issuers, opens a node session, submits a signed heartbeat attestation, receives `rollout_bundle`, acknowledges it, receives `command_dispatch`, and persists `command_result = succeeded`

## Current Limits

- The Wasm runtime now executes registered artifacts and supports signed publisher envelopes, but host calls are still minimal and there is not yet a remote skill marketplace or cross-gateway replication flow.
- Agent Card discovery and invocation now work for Dawn-compatible task endpoints, but the compatibility layer is still pragmatic rather than a fully heterogeneous A2A adapter matrix.
- The node agent is real but still minimal; it is not yet a full production agent runtime.
- Connectors are real HTTP integrations, but they are still isolated endpoints rather than part of a full orchestration graph.
- Inbound chat ingress now covers Telegram, Feishu, DingTalk, WeCom, WeChat Official Account, and normalized QQ events, but it is still early-stage: there is not yet a full inbound reply orchestration layer, and the approval UX is currently gateway-console driven rather than native inside each chat platform.
- Marketplace discovery now exists, but it is still gateway-hosted rather than federation-native; there is not yet ranking, reviews, download telemetry, or cross-gateway reputation.
- The persistence backend is SQLite today; multi-node production deployment will still want a Postgres-grade shared store later.
- Remote A2A settlement is now persisted and AP2-linked, but it still assumes a local settlement authority; there is not yet a distributed AP2 settlement network or reconciliation flow across gateways.
- Agent Card quote support now includes a local replay ledger plus on-demand cross-gateway quote-state verification, but it is still pull-based; there is not yet a push replication bus, federation-wide revocation feed, or quote-state gossip protocol.
- Policy and skill rollout now reach attested nodes and the node can independently verify trusted policy and skill publisher signatures, but there is not yet a node-side persisted trust-root store or full artifact-by-artifact Wasm binary verification against downloaded module bytes.
- Runtime multi-process smoke is still blocked by the current host command policy, but the rollout + attestation + command loop is now covered by an in-process integration test instead of relying only on unit tests.
