use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::error;

use crate::{
    agent_cards::{self, ImportAgentCardRequest, PublishedAgentCard},
    app_state::AppState,
    skill_registry::{self, InstallSkillPackageRequest, SkillRecord},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkillEntry {
    pub skill_id: String,
    pub version: String,
    pub display_name: String,
    pub description: Option<String>,
    pub capabilities: Vec<String>,
    pub signed: bool,
    pub active: bool,
    pub issuer_did: Option<String>,
    pub package_url: String,
    pub install_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceAgentEntry {
    pub card_id: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub published: bool,
    pub locally_hosted: bool,
    pub chat_platforms: Vec<String>,
    pub model_providers: Vec<String>,
    pub payment_roles: Vec<String>,
    pub issuer_did: Option<String>,
    pub card_url: Option<String>,
    pub install_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceCatalog {
    pub generated_at_unix_ms: u128,
    pub public_base_url: Option<String>,
    pub skills: Vec<MarketplaceSkillEntry>,
    pub agent_cards: Vec<MarketplaceAgentEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogQuery {
    q: Option<String>,
    kind: Option<String>,
    signed_only: Option<bool>,
    published_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
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

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/catalog", get(get_catalog))
        .route("/install/skill", post(install_skill))
        .route("/install/agent-card", post(install_agent_card))
}

pub fn page_router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(page))
}

async fn get_catalog(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<MarketplaceCatalog>, (StatusCode, Json<Value>)> {
    build_catalog(&state, query)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn install_skill(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallSkillPackageRequest>,
) -> Result<Json<skill_registry::SkillActivationResponse>, (StatusCode, Json<Value>)> {
    skill_registry::install_skill_package_from_url(&state, request)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn install_agent_card(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallAgentCardRequest>,
) -> Result<Json<PublishedAgentCard>, (StatusCode, Json<Value>)> {
    agent_cards::import_agent_card(
        &state,
        ImportAgentCardRequest {
            card_id: request.card_id,
            card_url: request.card_url,
            regions: request.regions,
            languages: request.languages,
            model_providers: request.model_providers,
            chat_platforms: request.chat_platforms,
            payment_roles: request.payment_roles,
            published: request.published,
            issuer_did: None,
            signature_hex: None,
        },
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

pub async fn well_known_catalog(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MarketplaceCatalog>, (StatusCode, Json<Value>)> {
    build_catalog(
        &state,
        CatalogQuery {
            q: None,
            kind: None,
            signed_only: Some(true),
            published_only: Some(true),
        },
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

async fn page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Dawn Marketplace</title>
  <style>
    :root {
      --bg: #081018;
      --panel: rgba(15, 25, 34, 0.92);
      --line: rgba(255,255,255,0.08);
      --text: #edf2f6;
      --muted: #9cb1be;
      --accent: #f1c15d;
      --accent2: #58b4cb;
      --ok: #74d8a7;
      --font: "Segoe UI Variable", "Segoe UI", sans-serif;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: var(--font);
      background:
        radial-gradient(circle at top left, rgba(88,180,203,0.18), transparent 35%),
        radial-gradient(circle at top right, rgba(241,193,93,0.18), transparent 32%),
        linear-gradient(180deg, #091018 0%, #0d1720 100%);
      color: var(--text);
    }
    .shell { max-width: 1380px; margin: 0 auto; padding: 28px; }
    .hero, .panel {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 24px;
      padding: 22px;
      box-shadow: 0 24px 70px rgba(0,0,0,0.34);
      backdrop-filter: blur(14px);
    }
    .hero { margin-bottom: 18px; }
    .eyebrow { text-transform: uppercase; letter-spacing: .18em; font-size: 11px; color: var(--accent); }
    h1 { margin: 8px 0 10px; font-size: clamp(32px, 5vw, 56px); line-height: .95; }
    .copy { max-width: 64ch; color: var(--muted); line-height: 1.6; }
    .controls { display: flex; gap: 12px; margin: 18px 0 0; flex-wrap: wrap; }
    input, select, button {
      border-radius: 999px;
      border: 1px solid var(--line);
      background: rgba(255,255,255,0.04);
      color: var(--text);
      padding: 10px 14px;
      font: inherit;
    }
    button { cursor: pointer; background: var(--accent); color: #111a20; border: 0; font-weight: 700; }
    .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 18px; margin-top: 18px; }
    .cards { display: grid; gap: 12px; }
    .item {
      padding: 16px;
      border-radius: 18px;
      border: 1px solid var(--line);
      background: rgba(255,255,255,0.03);
    }
    .meta { color: var(--muted); font-size: 13px; line-height: 1.5; }
    .tags { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 10px; }
    .tag {
      display: inline-flex;
      padding: 6px 10px;
      border-radius: 999px;
      font-size: 11px;
      background: rgba(88,180,203,0.14);
      color: #cdeaf2;
      border: 1px solid rgba(88,180,203,0.2);
    }
    @media (max-width: 960px) { .grid { grid-template-columns: 1fr; } }
  </style>
</head>
<body>
  <div class="shell">
    <section class="hero">
      <div class="eyebrow">Dawn Marketplace</div>
      <h1>Public Skills And Agents</h1>
      <div class="copy">
        Browse published agent cards and signed Wasm skills from this gateway. Installable entries expose direct package or card URLs for cross-gateway discovery.
      </div>
      <div class="controls">
        <input id="q" type="search" placeholder="Search skills, agents, tags, platforms" />
        <select id="kind">
          <option value="">All</option>
          <option value="skill">Skills</option>
          <option value="agent">Agents</option>
        </select>
        <button type="button" onclick="refresh()">Refresh</button>
      </div>
    </section>
    <section class="grid">
      <section class="panel">
        <h2>Skills</h2>
        <div id="skills" class="cards"></div>
      </section>
      <section class="panel">
        <h2>Agent Cards</h2>
        <div id="agents" class="cards"></div>
      </section>
    </section>
  </div>
  <script>
    const ellipsis = (value, max = 88) => value && value.length > max ? `${value.slice(0, max)}…` : (value || "—");
    async function refresh() {
      const q = document.getElementById("q").value;
      const kind = document.getElementById("kind").value;
      const params = new URLSearchParams();
      if (q) params.set("q", q);
      if (kind) params.set("kind", kind);
      params.set("signedOnly", "true");
      params.set("publishedOnly", "true");
      const response = await fetch(`/api/gateway/marketplace/catalog?${params}`);
      const catalog = await response.json();
      document.getElementById("skills").innerHTML = (catalog.skills || []).map((skill) => `
        <article class="item">
          <strong>${skill.displayName} <small>${skill.skillId}@${skill.version}</small></strong>
          <div class="meta">${ellipsis(skill.description)}</div>
          <div class="meta">${skill.signed ? "Signed publisher" : "Unsigned"} · ${skill.active ? "active" : "inactive"}</div>
          <div class="tags">${(skill.capabilities || []).map((tag) => `<span class="tag">${tag}</span>`).join("")}</div>
          <div class="meta">Package: ${ellipsis(skill.packageUrl, 110)}</div>
        </article>`).join("") || `<div class="meta">No skills matched.</div>`;
      document.getElementById("agents").innerHTML = (catalog.agentCards || []).map((agent) => `
        <article class="item">
          <strong>${agent.name} <small>${agent.cardId}</small></strong>
          <div class="meta">${ellipsis(agent.description)}</div>
          <div class="meta">${agent.locallyHosted ? "local" : "remote"} · ${(agent.chatPlatforms || []).join(", ") || "no chat metadata"}</div>
          <div class="tags">${(agent.paymentRoles || []).map((tag) => `<span class="tag">${tag}</span>`).join("")}</div>
          <div class="meta">Card: ${ellipsis(agent.cardUrl || agent.url, 110)}</div>
        </article>`).join("") || `<div class="meta">No agent cards matched.</div>`;
    }
    refresh();
  </script>
</body>
</html>"#,
    )
}

async fn build_catalog(
    state: &Arc<AppState>,
    query: CatalogQuery,
) -> anyhow::Result<MarketplaceCatalog> {
    let public_base_url = public_base_url();
    let q = query.q.as_deref().map(str::to_ascii_lowercase);
    let kind = query.kind.as_deref();
    let signed_only = query.signed_only.unwrap_or(true);
    let published_only = query.published_only.unwrap_or(true);

    let skills = if kind == Some("agent") {
        Vec::new()
    } else {
        let distribution = skill_registry::current_distribution(state).await?;
        distribution
            .skills
            .into_iter()
            .filter(|skill| {
                !signed_only || (skill.signature_hex.is_some() && skill.issuer_did.is_some())
            })
            .filter(|skill| q.as_ref().is_none_or(|needle| matches_skill(skill, needle)))
            .map(|skill| skill_to_marketplace_entry(skill, public_base_url.as_deref()))
            .collect()
    };

    let agent_cards = if kind == Some("skill") {
        Vec::new()
    } else {
        agent_cards::list_agent_cards(state)
            .await?
            .into_iter()
            .filter(|card| !published_only || card.published)
            .filter(|card| q.as_ref().is_none_or(|needle| matches_agent(card, needle)))
            .map(|card| card_to_marketplace_entry(card, public_base_url.as_deref()))
            .collect()
    };

    Ok(MarketplaceCatalog {
        generated_at_unix_ms: crate::app_state::unix_timestamp_ms(),
        public_base_url,
        skills,
        agent_cards,
    })
}

fn skill_to_marketplace_entry(
    skill: SkillRecord,
    public_base_url: Option<&str>,
) -> MarketplaceSkillEntry {
    let package_path = format!(
        "/api/gateway/skills/{}/{}/package",
        skill.skill_id, skill.version
    );
    MarketplaceSkillEntry {
        skill_id: skill.skill_id.clone(),
        version: skill.version.clone(),
        display_name: skill.display_name.clone(),
        description: skill.description.clone(),
        capabilities: skill.capabilities.clone(),
        signed: skill.signature_hex.is_some() && skill.issuer_did.is_some(),
        active: skill.active,
        issuer_did: skill.issuer_did.clone(),
        install_url: join_public_url(public_base_url, "/api/gateway/marketplace/install/skill"),
        package_url: join_public_url(public_base_url, &package_path),
    }
}

fn card_to_marketplace_entry(
    card: PublishedAgentCard,
    public_base_url: Option<&str>,
) -> MarketplaceAgentEntry {
    MarketplaceAgentEntry {
        card_id: card.card_id.clone(),
        name: card.card.name.clone(),
        description: card.card.description.clone(),
        url: card.card.url.clone(),
        published: card.published,
        locally_hosted: card.locally_hosted,
        chat_platforms: card.chat_platforms.clone(),
        model_providers: card.model_providers.clone(),
        payment_roles: card.payment_roles.clone(),
        issuer_did: card.issuer_did.clone(),
        install_url: Some(join_public_url(
            public_base_url,
            "/api/gateway/marketplace/install/agent-card",
        )),
        card_url: card
            .card_url
            .as_ref()
            .map(|value| join_public_url(public_base_url, value)),
    }
}

fn matches_skill(skill: &SkillRecord, needle: &str) -> bool {
    skill.skill_id.to_ascii_lowercase().contains(needle)
        || skill.display_name.to_ascii_lowercase().contains(needle)
        || skill
            .description
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(needle)
        || skill
            .capabilities
            .iter()
            .any(|capability| capability.to_ascii_lowercase().contains(needle))
}

fn matches_agent(card: &PublishedAgentCard, needle: &str) -> bool {
    card.card_id.to_ascii_lowercase().contains(needle)
        || card.card.name.to_ascii_lowercase().contains(needle)
        || card.card.description.to_ascii_lowercase().contains(needle)
        || card
            .chat_platforms
            .iter()
            .any(|platform| platform.to_ascii_lowercase().contains(needle))
        || card
            .model_providers
            .iter()
            .any(|provider| provider.to_ascii_lowercase().contains(needle))
        || card
            .payment_roles
            .iter()
            .any(|role| role.to_ascii_lowercase().contains(needle))
        || card.card.skills.iter().any(|skill| {
            skill.id.to_ascii_lowercase().contains(needle)
                || skill.name.to_ascii_lowercase().contains(needle)
                || skill
                    .tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(needle))
        })
}

fn public_base_url() -> Option<String> {
    std::env::var("DAWN_PUBLIC_BASE_URL")
        .ok()
        .map(|value| value.trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn join_public_url(base: Option<&str>, path: &str) -> String {
    match base {
        Some(base) => format!("{base}{path}"),
        None => path.to_string(),
    }
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "Marketplace failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error.to_string()
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use base64::Engine as _;
    use wasmtime::Engine;

    use super::{CatalogQuery, build_catalog};
    use crate::{
        agent_cards,
        agent_cards::{AgentAuthentication, AgentCapabilities, AgentCard, PublishAgentCardRequest},
        app_state::AppState,
        sandbox,
        skill_registry::{
            RegisterSignedSkillRequest, SignedSkillDocument, SignedSkillEnvelope,
            SkillPublisherTrustRootUpsertRequest, register_signed_skill_inner,
            upsert_skill_publisher_trust_root_inner,
        },
    };
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    fn temp_database_url() -> (String, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("dawn-core-marketplace-test-{}.db", Uuid::new_v4()));
        (format!("sqlite://{}", path.display()), path)
    }

    async fn test_state() -> anyhow::Result<(Arc<AppState>, PathBuf)> {
        let (database_url, path) = temp_database_url();
        let engine: Engine = sandbox::init_engine()?;
        let state = AppState::new_with_database_url(engine, &database_url).await?;
        Ok((state, path))
    }

    #[tokio::test]
    async fn catalog_lists_signed_skills_and_published_cards() {
        let (state, db_path) = test_state().await.unwrap();

        let signing_key = SigningKey::from_bytes(&[61_u8; 32]);
        let public_key_hex = hex::encode(signing_key.verifying_key().as_bytes());
        let issuer_did = format!("did:dawn:skill-publisher:{public_key_hex}");
        upsert_skill_publisher_trust_root_inner(
            &state,
            SkillPublisherTrustRootUpsertRequest {
                actor: "test".to_string(),
                reason: "seed marketplace publisher".to_string(),
                issuer_did: issuer_did.clone(),
                label: "marketplace".to_string(),
                public_key_hex: public_key_hex.clone(),
            },
        )
        .await
        .unwrap();

        let wasm_base64 = "AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL";
        let document = SignedSkillDocument {
            skill_id: "market-skill".to_string(),
            version: "1.0.0".to_string(),
            display_name: "Market Skill".to_string(),
            description: Some("Signed marketplace export".to_string()),
            entry_function: "run_skill".to_string(),
            capabilities: vec!["echo".to_string()],
            artifact_sha256: hex::encode(Sha256::digest(
                base64::prelude::BASE64_STANDARD
                    .decode(wasm_base64.as_bytes())
                    .unwrap(),
            )),
            issuer_did,
            issued_at_unix_ms: 1_700_000_100_000,
        };
        let signature = signing_key.sign(&serde_json::to_vec(&document).unwrap());
        register_signed_skill_inner(
            &state,
            RegisterSignedSkillRequest {
                envelope: SignedSkillEnvelope {
                    document,
                    signature_hex: hex::encode(signature.to_bytes()),
                },
                wasm_base64: wasm_base64.to_string(),
                activate: Some(true),
            },
        )
        .await
        .unwrap();

        agent_cards::publish_agent_card(
            &state,
            PublishAgentCardRequest {
                card_id: Some("market-agent".to_string()),
                card: AgentCard {
                    name: "Market Agent".to_string(),
                    description: "Published marketplace agent".to_string(),
                    url: "http://example.com/api/a2a".to_string(),
                    provider: None,
                    version: "1.0.0".to_string(),
                    documentation_url: None,
                    capabilities: AgentCapabilities::default(),
                    authentication: AgentAuthentication::default(),
                    default_input_modes: vec!["text".to_string()],
                    default_output_modes: vec!["text".to_string()],
                    skills: Vec::new(),
                },
                regions: Some(vec!["global".to_string()]),
                languages: Some(vec!["en".to_string()]),
                model_providers: Some(vec!["deepseek".to_string()]),
                chat_platforms: Some(vec!["feishu".to_string()]),
                payment_roles: Some(vec!["payee".to_string()]),
                locally_hosted: Some(false),
                published: Some(true),
                issuer_did: None,
                signature_hex: None,
            },
        )
        .await
        .unwrap();

        let catalog = build_catalog(
            &state,
            CatalogQuery {
                q: None,
                kind: None,
                signed_only: Some(true),
                published_only: Some(true),
            },
        )
        .await
        .unwrap();

        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].skill_id, "market-skill");
        assert_eq!(catalog.agent_cards.len(), 1);
        assert_eq!(catalog.agent_cards[0].card_id, "market-agent");

        drop(state);
        fs::remove_file(db_path).ok();
    }
}
