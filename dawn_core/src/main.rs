use axum::{Router, routing::get};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod a2a;
mod agent_cards;
mod ap2;
mod app_state;
mod approval_center;
mod chat_ingress;
mod connectors;
mod control_center;
mod control_plane;
mod end_user_approvals;
mod gateway;
mod identity;
mod marketplace;
mod node_attestation;
mod policy;
mod sandbox;
mod skill_registry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dawn_core=debug,axum=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting DawnCore Server...");

    // Initialize the WebAssembly secure sandbox engine
    let engine = sandbox::init_engine()?;
    let state = app_state::AppState::new(engine).await?;

    let app = build_app(state);

    // Define the address to run on
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    info!("DawnCore listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "DawnCore is operational."
}

pub fn build_app(state: std::sync::Arc<app_state::AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .nest("/console", control_center::router())
        .nest("/end-user", end_user_approvals::page_router())
        .route(
            "/.well-known/agent-card.json",
            get(agent_cards::well_known_agent_card_json),
        )
        .route(
            "/.well-known/agent.json",
            get(agent_cards::well_known_agent_card_json),
        )
        .route(
            "/.well-known/dawn-marketplace.json",
            get(marketplace::well_known_catalog),
        )
        .nest("/marketplace", marketplace::page_router())
        .nest("/api/gateway", gateway::router())
        .nest("/api/ap2", ap2::router())
        .nest("/api/a2a", a2a::router())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use anyhow::Context;
    use base64::{Engine as _, prelude::BASE64_STANDARD};
    use ed25519_dalek::{Signer, SigningKey};
    use futures_util::{SinkExt, StreamExt};
    use reqwest::Client;
    use serde_json::{Value, json};
    use sha2::Digest;
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    use uuid::Uuid;
    use wasmtime::Engine;

    use super::build_app;
    use crate::{app_state::AppState, node_attestation, policy, sandbox, skill_registry};

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-app-smoke-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn spawn_test_server() -> anyhow::Result<(String, tokio::task::JoinHandle<()>, PathBuf)> {
        let (database_url, db_path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        let app = build_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((format!("http://{addr}"), handle, db_path))
    }

    async fn post_json(client: &Client, url: &str, body: Value) -> anyhow::Result<Value> {
        let response = client.post(url).json(&body).send().await?;
        let response = response.error_for_status()?;
        Ok(response.json().await?)
    }

    async fn put_json(client: &Client, url: &str, body: Value) -> anyhow::Result<Value> {
        let response = client.put(url).json(&body).send().await?;
        let response = response.error_for_status()?;
        Ok(response.json().await?)
    }

    async fn get_json(client: &Client, url: &str) -> anyhow::Result<Value> {
        let response = client.get(url).send().await?;
        let response = response.error_for_status()?;
        Ok(response.json().await?)
    }

    async fn get_text(client: &Client, url: &str) -> anyhow::Result<String> {
        let response = client.get(url).send().await?;
        let response = response.error_for_status()?;
        Ok(response.text().await?)
    }

    async fn next_text<S>(reader: &mut S, label: &str) -> anyhow::Result<String>
    where
        S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        loop {
            let message = tokio::time::timeout(std::time::Duration::from_secs(5), reader.next())
                .await
                .map_err(|_| anyhow::anyhow!("timed out waiting for websocket message: {label}"))?
                .ok_or_else(|| anyhow::anyhow!("websocket stream closed: {label}"))??;
            match message {
                Message::Text(text) => return Ok(text.to_string()),
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn in_process_smoke_exercises_rollout_and_command_loop() -> anyhow::Result<()> {
        let (base_url, handle, db_path) = spawn_test_server().await?;
        let client = Client::new();

        let bootstrap = post_json(
            &client,
            &format!("{base_url}/api/gateway/identity/bootstrap/session"),
            json!({
                "bootstrapToken": "dawn-dev-bootstrap",
                "operatorName": "smoke-operator"
            }),
        )
        .await?;
        let session_token = bootstrap["sessionToken"]
            .as_str()
            .context("missing identity session token")?
            .to_string();
        let claim = post_json(
            &client,
            &format!("{base_url}/api/gateway/identity/node-claims"),
            json!({
                "sessionToken": session_token,
                "nodeId": "node-smoke",
                "displayName": "Smoke Node",
                "transport": "websocket",
                "requestedCapabilities": ["agent_ping"],
                "expiresInSeconds": 600
            }),
        )
        .await?;
        let claim_token = claim["claimToken"]
            .as_str()
            .context("missing node claim token")?
            .to_string();

        let node_signing_key = SigningKey::from_bytes(&[41_u8; 32]);
        let node_public_key_hex = hex::encode(node_signing_key.verifying_key().as_bytes());
        let node_issuer_did = format!("did:dawn:node:{node_public_key_hex}");
        post_json(
            &client,
            &format!("{base_url}/api/gateway/control-plane/nodes/trust-roots"),
            json!({
                "actor": "smoke-test",
                "reason": "seed node trust root",
                "issuerDid": node_issuer_did,
                "label": "smoke node",
                "publicKeyHex": node_public_key_hex
            }),
        )
        .await?;

        let policy_signing_key = SigningKey::from_bytes(&[43_u8; 32]);
        let policy_public_key_hex = hex::encode(policy_signing_key.verifying_key().as_bytes());
        let policy_issuer_did = format!("did:dawn:policy:{policy_public_key_hex}");
        post_json(
            &client,
            &format!("{base_url}/api/gateway/policy/trust-roots"),
            json!({
                "actor": "smoke-test",
                "reason": "seed policy trust root",
                "issuerDid": policy_issuer_did,
                "label": "smoke policy issuer",
                "publicKeyHex": policy_public_key_hex
            }),
        )
        .await?;

        let policy_document = policy::PolicyDocument {
            policy_id: "default".to_string(),
            version: 2,
            issuer_did: policy_issuer_did,
            issued_at_unix_ms: 1_700_000_000_000_u128,
            allow_shell_exec: false,
            allowed_model_providers: vec!["deepseek".to_string()],
            allowed_chat_platforms: vec!["feishu".to_string()],
            max_payment_amount: Some(12.5),
            updated_reason: "smoke rollout".to_string(),
        };
        let policy_signature = policy_signing_key.sign(&serde_json::to_vec(&policy_document)?);
        put_json(
            &client,
            &format!("{base_url}/api/gateway/policy/signed"),
            json!({
                "actor": "smoke-test",
                "reason": "activate smoke policy",
                "envelope": {
                    "document": policy_document,
                    "signatureHex": hex::encode(policy_signature.to_bytes())
                }
            }),
        )
        .await?;

        let skill_signing_key = SigningKey::from_bytes(&[47_u8; 32]);
        let skill_public_key_hex = hex::encode(skill_signing_key.verifying_key().as_bytes());
        let skill_issuer_did = format!("did:dawn:skill-publisher:{skill_public_key_hex}");
        post_json(
            &client,
            &format!("{base_url}/api/gateway/skills/trust-roots"),
            json!({
                "actor": "smoke-test",
                "reason": "seed skill trust root",
                "issuerDid": skill_issuer_did,
                "label": "smoke skill issuer",
                "publicKeyHex": skill_public_key_hex
            }),
        )
        .await?;

        let wasm_base64 = "AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL";
        let artifact_sha256 = hex::encode(sha2::Sha256::digest(
            BASE64_STANDARD.decode(wasm_base64.as_bytes())?,
        ));
        let skill_document = skill_registry::SignedSkillDocument {
            skill_id: "echo-skill".to_string(),
            version: "1.0.0".to_string(),
            display_name: "Echo Skill".to_string(),
            description: Some("signed smoke skill".to_string()),
            entry_function: "run_skill".to_string(),
            capabilities: vec!["echo".to_string()],
            artifact_sha256,
            issuer_did: skill_issuer_did,
            issued_at_unix_ms: 1_700_000_000_001_u128,
        };
        let skill_signature = skill_signing_key.sign(&serde_json::to_vec(&skill_document)?);
        post_json(
            &client,
            &format!("{base_url}/api/gateway/skills/register/signed"),
            json!({
                "envelope": {
                    "document": skill_document,
                    "signatureHex": hex::encode(skill_signature.to_bytes())
                },
                "wasmBase64": wasm_base64,
                "activate": true
            }),
        )
        .await?;

        let ws_url = format!(
            "{}/api/gateway/control-plane/nodes/node-smoke/session?displayName=Smoke%20Node&transport=websocket&claimToken={}",
            base_url.replace("http://", "ws://"),
            claim_token
        );
        let (stream, _) = connect_async(&ws_url).await?;
        let (mut writer, mut reader) = stream.split();

        let greeting = next_text(&mut reader, "session greeting").await?;
        let greeting_json: Value = serde_json::from_str(&greeting)?;
        assert_eq!(greeting_json["messageType"], "session_ready");

        let attestation_document = node_attestation::NodeCapabilityAttestationDocument {
            node_id: "node-smoke".to_string(),
            issuer_did: node_issuer_did,
            issued_at_unix_ms: 1_700_000_000_002_u128,
            display_name: "Smoke Node".to_string(),
            transport: "websocket".to_string(),
            capabilities: vec!["agent_ping".to_string()],
        };
        let attestation_signature =
            node_signing_key.sign(&serde_json::to_vec(&attestation_document)?);
        writer
            .send(Message::Text(
                json!({
                    "messageType": "heartbeat",
                    "displayName": "Smoke Node",
                    "capabilities": ["agent_ping"],
                    "capabilityAttestation": {
                        "document": attestation_document,
                        "signatureHex": hex::encode(attestation_signature.to_bytes())
                    }
                })
                .to_string()
                .into(),
            ))
            .await?;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let node_record = get_json(
            &client,
            &format!("{base_url}/api/gateway/control-plane/nodes/node-smoke"),
        )
        .await?;
        assert_eq!(node_record["attestationVerified"], true);

        let rollout_raw = next_text(&mut reader, "rollout bundle").await?;
        let rollout_json: Value = serde_json::from_str(&rollout_raw)?;
        assert_eq!(rollout_json["messageType"], "rollout_bundle");
        let bundle_hash = rollout_json["bundle"]["bundleHash"]
            .as_str()
            .unwrap()
            .to_string();
        let policy_version = rollout_json["bundle"]["policyVersion"].as_u64().unwrap();
        let skill_distribution_hash = rollout_json["bundle"]["skillDistributionHash"]
            .as_str()
            .unwrap()
            .to_string();

        writer
            .send(Message::Text(
                json!({
                    "messageType": "rollout_ack",
                    "bundleHash": bundle_hash,
                    "accepted": true,
                    "policyVersion": policy_version,
                    "skillDistributionHash": skill_distribution_hash
                })
                .to_string()
                .into(),
            ))
            .await?;

        let command_response = post_json(
            &client,
            &format!("{base_url}/api/gateway/control-plane/nodes/node-smoke/commands"),
            json!({
                "commandType": "agent_ping",
                "payload": {}
            }),
        )
        .await?;
        let command_id = command_response["command"]["commandId"]
            .as_str()
            .unwrap()
            .to_string();

        let dispatch_raw = next_text(&mut reader, "command dispatch").await?;
        let dispatch_json: Value = serde_json::from_str(&dispatch_raw)?;
        assert_eq!(dispatch_json["messageType"], "command_dispatch");
        assert_eq!(dispatch_json["commandId"], command_id);

        writer
            .send(Message::Text(
                json!({
                    "messageType": "command_result",
                    "commandId": command_id,
                    "status": "succeeded",
                    "result": {
                        "nodeId": "node-smoke",
                        "nodeName": "Smoke Node",
                        "observedAtUnixMs": 1_700_000_000_100_u128
                    }
                })
                .to_string()
                .into(),
            ))
            .await?;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let rollout_record = get_json(
            &client,
            &format!("{base_url}/api/gateway/control-plane/nodes/node-smoke/rollout"),
        )
        .await?;
        assert_eq!(rollout_record["status"], "acknowledged");

        let command_record = get_json(
            &client,
            &format!("{base_url}/api/gateway/control-plane/commands/{command_id}"),
        )
        .await?;
        assert_eq!(command_record["status"], "succeeded");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn in_process_remote_settlement_flow_authorizes_payment() -> anyhow::Result<()> {
        let (base_url, handle, db_path) = spawn_test_server().await?;
        let client = Client::new();

        post_json(
            &client,
            &format!("{base_url}/api/gateway/skills/register"),
            json!({
                "skillId": "echo-skill",
                "version": "1.0.0",
                "displayName": "Echo Skill",
                "description": "minimal smoke skill",
                "entryFunction": "run_skill",
                "capabilities": ["echo"],
                "wasmBase64": "AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL",
                "activate": true
            }),
        )
        .await?;

        let card_id = "settlement-agent";
        post_json(
            &client,
            &format!("{base_url}/api/gateway/agent-cards/publish"),
            json!({
                "cardId": card_id,
                "card": {
                    "name": "Settlement Agent",
                    "description": "Locally hosted remote settlement smoke agent",
                    "url": format!("{base_url}/api/a2a"),
                    "version": "1.0.0",
                    "capabilities": {
                        "stateTransitionHistory": true,
                        "extensions": [{
                            "uri": "https://github.com/google-agentic-commerce/ap2/tree/v0.1",
                            "required": false,
                            "params": {
                                "roles": ["payee"]
                            }
                        }]
                    },
                    "authentication": { "schemes": [] },
                    "defaultInputModes": ["text"],
                    "defaultOutputModes": ["text"],
                    "skills": [{
                        "id": "echo",
                        "name": "Echo",
                        "description": "Echo wasm bridge",
                        "tags": ["echo"],
                        "examples": [],
                        "inputModes": ["text"],
                        "outputModes": ["text"]
                    }]
                },
                "paymentRoles": ["payee"],
                "published": true,
                "locallyHosted": false
            }),
        )
        .await?;

        let mandate_id = Uuid::new_v4();
        let invoke_response = post_json(
            &client,
            &format!("{base_url}/api/gateway/agent-cards/{card_id}/invoke"),
            json!({
                "name": "delegate echo",
                "instruction": "wasm:echo-skill",
                "awaitCompletion": true,
                "settlement": {
                    "mandateId": mandate_id,
                    "amount": 9.5,
                    "description": "Settle delegated echo"
                }
            }),
        )
        .await?;

        assert_eq!(invoke_response["invocation"]["status"], "completed");
        assert_eq!(
            invoke_response["settlement"]["status"],
            "pending_physical_auth"
        );
        let settlement_id = Uuid::parse_str(
            invoke_response["settlement"]["settlementId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing settlementId"))?,
        )?;
        let transaction_id = Uuid::parse_str(
            invoke_response["settlement"]["transactionId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing transactionId"))?,
        )?;

        let mcu_signing_key = SigningKey::from_bytes(&[59_u8; 32]);
        let mcu_public_did = format!(
            "did:dawn:mcu:{}",
            hex::encode(mcu_signing_key.verifying_key().as_bytes())
        );
        let payload =
            crate::ap2::signature_payload(transaction_id, mandate_id, 9.5, "Settle delegated echo");
        let signature = mcu_signing_key.sign(payload.as_bytes());
        let authorize_response = post_json(
            &client,
            &format!("{base_url}/api/ap2/authorize"),
            json!({
                "transactionId": transaction_id,
                "mandateId": mandate_id,
                "amount": 9.5,
                "description": "Settle delegated echo",
                "mcuPublicDid": mcu_public_did,
                "mcuSignature": hex::encode(signature.to_bytes())
            }),
        )
        .await?;
        assert_eq!(authorize_response["status"], "authorized");

        let settlement_record = get_json(
            &client,
            &format!("{base_url}/api/gateway/agent-cards/settlements/{settlement_id}"),
        )
        .await?;
        assert_eq!(settlement_record["status"], "authorized");

        let payment_record = get_json(
            &client,
            &format!("{base_url}/api/ap2/transactions/{transaction_id}"),
        )
        .await?;
        assert_eq!(payment_record["status"], "authorized");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn in_process_approval_center_rejects_pending_payment() -> anyhow::Result<()> {
        let (base_url, handle, db_path) = spawn_test_server().await?;
        let client = Client::new();

        let mandate_id = Uuid::new_v4();
        let authorize_response = post_json(
            &client,
            &format!("{base_url}/api/ap2/authorize"),
            json!({
                "mandateId": mandate_id,
                "amount": 7.25,
                "description": "Reject via approval center"
            }),
        )
        .await?;
        assert_eq!(authorize_response["status"], "pending_physical_auth");

        let transaction_id = authorize_response["transactionId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing transactionId"))?
            .to_string();
        let approvals = get_json(
            &client,
            &format!("{base_url}/api/gateway/approvals?status=pending"),
        )
        .await?;
        let approval = approvals
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["kind"] == "payment" && item["referenceId"] == transaction_id)
            })
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing pending payment approval"))?;
        let approval_id = approval["approvalId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing approvalId"))?;

        let decision = post_json(
            &client,
            &format!("{base_url}/api/gateway/approvals/{approval_id}/decision"),
            json!({
                "actor": "smoke-test",
                "decision": "reject",
                "reason": "operator denied the payment"
            }),
        )
        .await?;

        assert_eq!(decision["approval"]["status"], "rejected");
        assert_eq!(decision["paymentResponse"]["status"], "rejected");

        let payment_record = get_json(
            &client,
            &format!("{base_url}/api/ap2/transactions/{transaction_id}"),
        )
        .await?;
        assert_eq!(payment_record["status"], "rejected");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }

    #[tokio::test]
    async fn in_process_end_user_approval_portal_authorizes_pending_payment() -> anyhow::Result<()>
    {
        let (base_url, handle, db_path) = spawn_test_server().await?;
        let client = Client::new();

        let ingress_response = post_json(
            &client,
            &format!("{base_url}/api/gateway/ingress/telegram/webhook/end-user-flow"),
            json!({
                "update_id": 42,
                "message": {
                    "message_id": 9,
                    "text": "Book a train to Shanghai",
                    "chat": { "id": 7788, "title": "Alice" },
                    "from": { "id": 5566, "first_name": "Alice", "username": "alice" }
                }
            }),
        )
        .await?;
        let task_id = ingress_response["taskId"]
            .as_str()
            .context("missing task id from ingress")?
            .to_string();

        let mandate_id = Uuid::new_v4();
        let authorize_response = post_json(
            &client,
            &format!("{base_url}/api/ap2/authorize"),
            json!({
                "taskId": task_id,
                "mandateId": mandate_id,
                "amount": 11.5,
                "description": "Book train to Shanghai"
            }),
        )
        .await?;
        assert_eq!(authorize_response["status"], "pending_physical_auth");
        let approval_url = authorize_response["endUserApprovalUrl"]
            .as_str()
            .context("missing end-user approval url")?;
        assert!(approval_url.starts_with("/end-user/approvals/"));
        let approval_token = approval_url
            .rsplit('/')
            .next()
            .context("missing approval token")?;

        let approval_detail = get_json(
            &client,
            &format!("{base_url}/api/gateway/end-user/approvals/{approval_token}"),
        )
        .await?;
        assert_eq!(approval_detail["session"]["status"], "pending");
        assert_eq!(
            approval_detail["payment"]["status"],
            "pending_physical_auth"
        );
        assert_eq!(approval_detail["task"]["taskId"], task_id);
        let signature_payload = approval_detail["signaturePayload"]
            .as_str()
            .context("missing signature payload")?;

        let page_html = get_text(&client, &format!("{base_url}{approval_url}")).await?;
        assert!(
            page_html.contains("Dawn End-User Approval") || page_html.contains("Dawn 终端用户审批")
        );

        let ingress_events = get_json(
            &client,
            &format!("{base_url}/api/gateway/ingress/events?limit=5"),
        )
        .await?;
        let latest_reply = ingress_events[0]["replyText"]
            .as_str()
            .context("missing ingress reply text")?;
        assert!(latest_reply.contains(approval_url));

        let signing_key = SigningKey::from_bytes(&[51_u8; 32]);
        let mcu_public_did = format!(
            "did:dawn:mcu:{}",
            hex::encode(signing_key.verifying_key().as_bytes())
        );
        let mcu_signature = hex::encode(signing_key.sign(signature_payload.as_bytes()).to_bytes());
        let decision = post_json(
            &client,
            &format!("{base_url}/api/gateway/end-user/approvals/{approval_token}/decision"),
            json!({
                "actor": "Alice",
                "decision": "approve",
                "reason": "Looks correct",
                "mcuPublicDid": mcu_public_did,
                "mcuSignature": mcu_signature
            }),
        )
        .await?;
        assert_eq!(decision["session"]["status"], "approved");
        assert_eq!(decision["approval"]["status"], "approved");
        assert_eq!(decision["paymentResponse"]["status"], "authorized");

        let payment_record = get_json(
            &client,
            &format!(
                "{base_url}/api/ap2/transactions/{}",
                authorize_response["transactionId"]
                    .as_str()
                    .context("missing transaction id")?
            ),
        )
        .await?;
        assert_eq!(payment_record["status"], "authorized");

        let task_record = get_json(&client, &format!("{base_url}/api/a2a/task/{task_id}")).await?;
        assert_eq!(task_record["task"]["status"], "queued");

        handle.abort();
        fs::remove_file(db_path).ok();
        Ok(())
    }
}
