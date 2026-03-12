use std::{sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::error;
use uuid::Uuid;

use crate::{
    agent_cards::{self, ImportAgentCardRequest, PublishedAgentCard},
    app_state::{AppState, MarketplacePeerRecord, MarketplacePeerSyncStatus, unix_timestamp_ms},
    skill_registry::{self, InstallSkillPackageRequest, SkillRecord},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceCatalog {
    pub generated_at_unix_ms: u128,
    pub public_base_url: Option<String>,
    pub skills: Vec<MarketplaceSkillEntry>,
    pub agent_cards: Vec<MarketplaceAgentEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CatalogQuery {
    q: Option<String>,
    kind: Option<String>,
    signed_only: Option<bool>,
    published_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertMarketplacePeerRequest {
    peer_id: Option<String>,
    display_name: String,
    base_url: String,
    catalog_url: Option<String>,
    enabled: Option<bool>,
    trust_enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplaceSkillEntry {
    source_kind: String,
    source_peer_id: String,
    source_display_name: String,
    source_catalog_url: String,
    entry: MarketplaceSkillEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplaceAgentEntry {
    source_kind: String,
    source_peer_id: String,
    source_display_name: String,
    source_catalog_url: String,
    entry: MarketplaceAgentEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplacePeerSnapshot {
    peer: MarketplacePeerRecord,
    skill_count: usize,
    agent_card_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FederatedMarketplaceCatalog {
    generated_at_unix_ms: u128,
    peers: Vec<FederatedMarketplacePeerSnapshot>,
    skills: Vec<FederatedMarketplaceSkillEntry>,
    agent_cards: Vec<FederatedMarketplaceAgentEntry>,
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
        .route("/catalog/federated", get(get_federated_catalog))
        .route("/install/skill", post(install_skill))
        .route("/install/agent-card", post(install_agent_card))
        .route("/peers", get(list_peers).post(upsert_peer))
        .route("/peers/:peer_id", get(get_peer))
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

async fn get_federated_catalog(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<FederatedMarketplaceCatalog>, (StatusCode, Json<Value>)> {
    build_federated_catalog(&state, query)
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

async fn list_peers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<MarketplacePeerRecord>>, (StatusCode, Json<Value>)> {
    state
        .list_marketplace_peers()
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn get_peer(
    State(state): State<Arc<AppState>>,
    Path(peer_id): Path<String>,
) -> Result<Json<MarketplacePeerRecord>, (StatusCode, Json<Value>)> {
    state
        .get_marketplace_peer(&peer_id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or_else(|| not_found("marketplace peer not found"))
}

async fn upsert_peer(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpsertMarketplacePeerRequest>,
) -> Result<Json<MarketplacePeerRecord>, (StatusCode, Json<Value>)> {
    let display_name = request.display_name.trim();
    if display_name.is_empty() {
        return Err(bad_request("displayName must not be empty"));
    }
    let base_url = normalize_base_url(&request.base_url)?;
    let catalog_url = request
        .catalog_url
        .as_deref()
        .map(normalize_absolute_url)
        .transpose()?
        .unwrap_or_else(|| default_peer_catalog_url(&base_url));
    let now = unix_timestamp_ms();
    let peer_id = request
        .peer_id
        .as_deref()
        .and_then(normalize_peer_id)
        .unwrap_or_else(|| derive_peer_id(&base_url));
    let existing = state
        .get_marketplace_peer(&peer_id)
        .await
        .map_err(internal_error)?;
    let peer = state
        .upsert_marketplace_peer(MarketplacePeerRecord {
            peer_id,
            display_name: display_name.to_string(),
            base_url,
            catalog_url,
            enabled: request.enabled.unwrap_or(true),
            trust_enabled: request.trust_enabled.unwrap_or(true),
            sync_status: existing
                .as_ref()
                .map(|peer| peer.sync_status)
                .unwrap_or(MarketplacePeerSyncStatus::Pending),
            last_sync_error: existing
                .as_ref()
                .and_then(|peer| peer.last_sync_error.clone()),
            last_synced_at_unix_ms: existing
                .as_ref()
                .and_then(|peer| peer.last_synced_at_unix_ms),
            created_at_unix_ms: existing
                .as_ref()
                .map(|peer| peer.created_at_unix_ms)
                .unwrap_or(now),
            updated_at_unix_ms: now,
        })
        .await
        .map_err(internal_error)?;
    Ok(Json(peer))
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
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Dawn 市场</title>
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
      <div class="eyebrow" id="marketplace-eyebrow">Dawn 市场</div>
      <h1 id="marketplace-title">公开技能与 Agents</h1>
      <div class="copy" id="marketplace-copy">
        浏览当前网关发布的 Agent Cards 与已签名 Wasm 技能。可安装条目会直接暴露包地址或卡片地址，便于跨网关发现。
      </div>
      <div class="controls">
        <input id="q" type="search" placeholder="搜索技能、Agent、标签或平台" />
        <select id="kind">
          <option value="">全部</option>
          <option value="skill">技能</option>
          <option value="agent">Agents</option>
        </select>
        <button type="button" id="lang-toggle" onclick="toggleLanguage()">EN</button>
        <button type="button" onclick="refresh()">刷新</button>
      </div>
    </section>
    <section class="grid">
      <section class="panel">
        <h2 id="skills-title">技能</h2>
        <div id="skills" class="cards"></div>
      </section>
      <section class="panel">
        <h2 id="agents-title">Agent Cards</h2>
        <div id="agents" class="cards"></div>
      </section>
    </section>
  </div>
  <script>
    const ellipsis = (value, max = 88) => value && value.length > max ? `${value.slice(0, max)}…` : (value || "—");
    const localeKey = "dawnUiLanguage";
    const translations = {
      zh: {
        title: "Dawn 市场",
        eyebrow: "Dawn 市场",
        hero: "公开技能与 Agents",
        copy: "浏览当前网关发布的 Agent Cards 与已签名 Wasm 技能。可安装条目会直接暴露包地址或卡片地址，便于跨网关发现。",
        searchPlaceholder: "搜索技能、Agent、标签或平台",
        all: "全部",
        skill: "技能",
        agent: "Agents",
        skillsTitle: "技能",
        agentsTitle: "Agent Cards",
        refresh: "刷新",
        toggle: "EN",
        signed: "已签名发布方",
        unsigned: "未签名",
        active: "启用中",
        inactive: "未启用",
        package: "安装包",
        noSkills: "没有匹配的技能。",
        local: "本地",
        remote: "远端",
        noChat: "没有聊天元数据",
        card: "卡片",
        noAgents: "没有匹配的 Agent Card。"
      },
      en: {
        title: "Dawn Marketplace",
        eyebrow: "Dawn Marketplace",
        hero: "Public Skills And Agents",
        copy: "Browse published agent cards and signed Wasm skills from this gateway. Installable entries expose direct package or card URLs for cross-gateway discovery.",
        searchPlaceholder: "Search skills, agents, tags, platforms",
        all: "All",
        skill: "Skills",
        agent: "Agents",
        skillsTitle: "Skills",
        agentsTitle: "Agent Cards",
        refresh: "Refresh",
        toggle: "中文",
        signed: "Signed publisher",
        unsigned: "Unsigned",
        active: "active",
        inactive: "inactive",
        package: "Package",
        noSkills: "No skills matched.",
        local: "local",
        remote: "remote",
        noChat: "no chat metadata",
        card: "Card",
        noAgents: "No agent cards matched."
      }
    };
    let currentLanguage = window.localStorage?.getItem(localeKey) || "zh";
    function t(key) {
      return translations[currentLanguage]?.[key] || translations.zh[key] || key;
    }
    function applyLanguage() {
      document.documentElement.lang = currentLanguage === "zh" ? "zh-CN" : "en";
      document.title = t("title");
      document.getElementById("marketplace-eyebrow").textContent = t("eyebrow");
      document.getElementById("marketplace-title").textContent = t("hero");
      document.getElementById("marketplace-copy").textContent = t("copy");
      document.getElementById("q").placeholder = t("searchPlaceholder");
      const kind = document.getElementById("kind");
      kind.options[0].textContent = t("all");
      kind.options[1].textContent = t("skill");
      kind.options[2].textContent = t("agent");
      document.getElementById("skills-title").textContent = t("skillsTitle");
      document.getElementById("agents-title").textContent = t("agentsTitle");
      document.getElementById("lang-toggle").textContent = t("toggle");
      document.querySelector("button[onclick='refresh()']").textContent = t("refresh");
    }
    function toggleLanguage() {
      currentLanguage = currentLanguage === "zh" ? "en" : "zh";
      try {
        window.localStorage?.setItem(localeKey, currentLanguage);
      } catch (_error) {}
      applyLanguage();
      refresh();
    }
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
          <div class="meta">${skill.signed ? t("signed") : t("unsigned")} · ${skill.active ? t("active") : t("inactive")}</div>
          <div class="tags">${(skill.capabilities || []).map((tag) => `<span class="tag">${tag}</span>`).join("")}</div>
          <div class="meta">${t("package")}：${ellipsis(skill.packageUrl, 110)}</div>
        </article>`).join("") || `<div class="meta">${t("noSkills")}</div>`;
      document.getElementById("agents").innerHTML = (catalog.agentCards || []).map((agent) => `
        <article class="item">
          <strong>${agent.name} <small>${agent.cardId}</small></strong>
          <div class="meta">${ellipsis(agent.description)}</div>
          <div class="meta">${agent.locallyHosted ? t("local") : t("remote")} · ${(agent.chatPlatforms || []).join(", ") || t("noChat")}</div>
          <div class="tags">${(agent.paymentRoles || []).map((tag) => `<span class="tag">${tag}</span>`).join("")}</div>
          <div class="meta">${t("card")}：${ellipsis(agent.cardUrl || agent.url, 110)}</div>
        </article>`).join("") || `<div class="meta">${t("noAgents")}</div>`;
    }
    applyLanguage();
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
        generated_at_unix_ms: unix_timestamp_ms(),
        public_base_url,
        skills,
        agent_cards,
    })
}

async fn build_federated_catalog(
    state: &Arc<AppState>,
    query: CatalogQuery,
) -> anyhow::Result<FederatedMarketplaceCatalog> {
    let local_catalog = build_catalog(state, query.clone()).await?;
    let local_catalog_url = join_public_url(
        local_catalog.public_base_url.as_deref(),
        "/api/gateway/marketplace/catalog",
    );
    let mut skills = local_catalog
        .skills
        .into_iter()
        .map(|entry| FederatedMarketplaceSkillEntry {
            source_kind: "local".to_string(),
            source_peer_id: "local".to_string(),
            source_display_name: "Local Gateway".to_string(),
            source_catalog_url: local_catalog_url.clone(),
            entry,
        })
        .collect::<Vec<_>>();
    let mut agent_cards = local_catalog
        .agent_cards
        .into_iter()
        .map(|entry| FederatedMarketplaceAgentEntry {
            source_kind: "local".to_string(),
            source_peer_id: "local".to_string(),
            source_display_name: "Local Gateway".to_string(),
            source_catalog_url: local_catalog_url.clone(),
            entry,
        })
        .collect::<Vec<_>>();
    let mut snapshots = Vec::new();
    let client = Client::builder().timeout(Duration::from_secs(8)).build()?;

    for peer in state.list_marketplace_peers().await? {
        let mut peer_for_snapshot = peer.clone();
        if !peer.enabled || !peer.trust_enabled {
            snapshots.push(FederatedMarketplacePeerSnapshot {
                peer: peer_for_snapshot,
                skill_count: 0,
                agent_card_count: 0,
            });
            continue;
        }

        match fetch_peer_catalog(&client, &peer, &query).await {
            Ok(remote_catalog) => {
                let remote_catalog = normalize_remote_catalog(&peer, remote_catalog)?;
                let skill_count = remote_catalog.skills.len();
                let agent_card_count = remote_catalog.agent_cards.len();
                peer_for_snapshot.sync_status = MarketplacePeerSyncStatus::Healthy;
                peer_for_snapshot.last_sync_error = None;
                peer_for_snapshot.last_synced_at_unix_ms = Some(unix_timestamp_ms());
                peer_for_snapshot.updated_at_unix_ms = unix_timestamp_ms();
                let peer_for_snapshot = state.upsert_marketplace_peer(peer_for_snapshot).await?;

                skills.extend(remote_catalog.skills.into_iter().map(|entry| {
                    FederatedMarketplaceSkillEntry {
                        source_kind: "peer".to_string(),
                        source_peer_id: peer_for_snapshot.peer_id.clone(),
                        source_display_name: peer_for_snapshot.display_name.clone(),
                        source_catalog_url: peer_for_snapshot.catalog_url.clone(),
                        entry,
                    }
                }));
                agent_cards.extend(remote_catalog.agent_cards.into_iter().map(|entry| {
                    FederatedMarketplaceAgentEntry {
                        source_kind: "peer".to_string(),
                        source_peer_id: peer_for_snapshot.peer_id.clone(),
                        source_display_name: peer_for_snapshot.display_name.clone(),
                        source_catalog_url: peer_for_snapshot.catalog_url.clone(),
                        entry,
                    }
                }));
                snapshots.push(FederatedMarketplacePeerSnapshot {
                    peer: peer_for_snapshot,
                    skill_count,
                    agent_card_count,
                });
            }
            Err(error) => {
                peer_for_snapshot.sync_status = classify_peer_sync_error(&error);
                peer_for_snapshot.last_sync_error = Some(error.to_string());
                peer_for_snapshot.last_synced_at_unix_ms = Some(unix_timestamp_ms());
                peer_for_snapshot.updated_at_unix_ms = unix_timestamp_ms();
                let peer_for_snapshot = state.upsert_marketplace_peer(peer_for_snapshot).await?;
                snapshots.push(FederatedMarketplacePeerSnapshot {
                    peer: peer_for_snapshot,
                    skill_count: 0,
                    agent_card_count: 0,
                });
            }
        }
    }

    Ok(FederatedMarketplaceCatalog {
        generated_at_unix_ms: unix_timestamp_ms(),
        peers: snapshots,
        skills,
        agent_cards,
    })
}

async fn fetch_peer_catalog(
    client: &Client,
    peer: &MarketplacePeerRecord,
    query: &CatalogQuery,
) -> anyhow::Result<MarketplaceCatalog> {
    let mut url = Url::parse(&peer.catalog_url)
        .with_context(|| format!("invalid peer catalog url '{}'", peer.catalog_url))?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(q) = query.q.as_deref() {
            pairs.append_pair("q", q);
        }
        if let Some(kind) = query.kind.as_deref() {
            pairs.append_pair("kind", kind);
        }
        if let Some(signed_only) = query.signed_only {
            pairs.append_pair("signedOnly", if signed_only { "true" } else { "false" });
        }
        if let Some(published_only) = query.published_only {
            pairs.append_pair(
                "publishedOnly",
                if published_only { "true" } else { "false" },
            );
        }
    }
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to reach peer '{}'", peer.display_name))?;
    let response = response.error_for_status().with_context(|| {
        format!(
            "peer '{}' returned a non-success federated catalog response",
            peer.display_name
        )
    })?;
    response
        .json::<MarketplaceCatalog>()
        .await
        .with_context(|| {
            format!(
                "peer '{}' returned an invalid marketplace catalog",
                peer.display_name
            )
        })
}

fn normalize_remote_catalog(
    peer: &MarketplacePeerRecord,
    mut catalog: MarketplaceCatalog,
) -> anyhow::Result<MarketplaceCatalog> {
    for skill in &mut catalog.skills {
        skill.package_url = normalize_remote_url(&peer.base_url, &skill.package_url)?;
        skill.install_url = normalize_remote_url(&peer.base_url, &skill.install_url)?;
    }
    for agent in &mut catalog.agent_cards {
        agent.url = normalize_remote_url(&peer.base_url, &agent.url)?;
        agent.card_url = agent
            .card_url
            .as_deref()
            .map(|value| normalize_remote_url(&peer.base_url, value))
            .transpose()?;
        agent.install_url = agent
            .install_url
            .as_deref()
            .map(|value| normalize_remote_url(&peer.base_url, value))
            .transpose()?;
    }
    Ok(catalog)
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

fn normalize_base_url(raw: &str) -> Result<String, (StatusCode, Json<Value>)> {
    normalize_http_url(raw, "baseUrl").map_err(|error| bad_request(error.to_string()))
}

fn normalize_absolute_url(raw: &str) -> Result<String, (StatusCode, Json<Value>)> {
    normalize_http_url(raw, "catalogUrl").map_err(|error| bad_request(error.to_string()))
}

fn normalize_http_url(raw: &str, field_name: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{field_name} must not be empty");
    }
    let url = Url::parse(trimmed)
        .with_context(|| format!("{field_name} must be an absolute http(s) URL"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!("{field_name} must use http or https, got '{scheme}'"),
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

fn default_peer_catalog_url(base_url: &str) -> String {
    format!(
        "{}/api/gateway/marketplace/catalog",
        base_url.trim_end_matches('/')
    )
}

fn normalize_peer_id(raw: &str) -> Option<String> {
    let normalized = raw
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn derive_peer_id(base_url: &str) -> String {
    let Some(url) = Url::parse(base_url).ok() else {
        return Uuid::new_v4().to_string();
    };
    let mut parts = Vec::new();
    if let Some(host) = url.host_str().and_then(normalize_peer_id) {
        parts.push(host);
    }
    if let Some(port) = url.port() {
        parts.push(port.to_string());
    }
    if let Some(path) = normalize_peer_id(url.path()) {
        parts.push(path);
    }
    if parts.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        parts.join("-")
    }
}

fn normalize_remote_url(base_url: &str, raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("marketplace entry URL must not be empty");
    }
    if Url::parse(trimmed).is_ok() {
        return normalize_http_url(trimmed, "remoteUrl");
    }
    let base = if base_url.ends_with('/') {
        base_url.to_string()
    } else {
        format!("{base_url}/")
    };
    let base =
        Url::parse(&base).with_context(|| format!("invalid peer base URL '{}'", base_url))?;
    let joined = base.join(trimmed).with_context(|| {
        format!(
            "failed to join peer URL '{}' against '{}'",
            trimmed, base_url
        )
    })?;
    normalize_http_url(joined.as_str(), "remoteUrl")
}

fn classify_peer_sync_error(error: &anyhow::Error) -> MarketplacePeerSyncStatus {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("invalid marketplace catalog")
        || message.contains("invalid peer catalog url")
        || message.contains("non-success federated catalog response")
        || message.contains("failed to join peer url")
        || message.contains("remoteurl")
    {
        MarketplacePeerSyncStatus::InvalidCatalog
    } else {
        MarketplacePeerSyncStatus::Unreachable
    }
}

fn join_public_url(base: Option<&str>, path: &str) -> String {
    match base {
        Some(base) => format!("{base}{path}"),
        None => path.to_string(),
    }
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": message.into()
        })),
    )
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": message
        })),
    )
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
    use std::{fs, path::PathBuf, sync::Arc, time::Duration};

    use axum::{Json, Router, routing::get};
    use base64::Engine as _;
    use tokio::time::sleep;
    use wasmtime::Engine;

    use super::{
        CatalogQuery, MarketplaceAgentEntry, MarketplaceCatalog, MarketplacePeerSyncStatus,
        MarketplaceSkillEntry, build_catalog, build_federated_catalog, default_peer_catalog_url,
        derive_peer_id,
    };
    use crate::{
        agent_cards,
        agent_cards::{AgentAuthentication, AgentCapabilities, AgentCard, PublishAgentCardRequest},
        app_state::{AppState, MarketplacePeerRecord, unix_timestamp_ms},
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

    #[tokio::test]
    async fn federated_catalog_merges_trusted_peer_and_normalizes_remote_urls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/api/gateway/marketplace/catalog",
                get(|| async {
                    Json(MarketplaceCatalog {
                        generated_at_unix_ms: 1_700_000_000_000,
                        public_base_url: None,
                        skills: vec![MarketplaceSkillEntry {
                            skill_id: "remote-skill".to_string(),
                            version: "1.0.0".to_string(),
                            display_name: "Remote Skill".to_string(),
                            description: Some("Signed remote skill".to_string()),
                            capabilities: vec!["search".to_string()],
                            signed: true,
                            active: true,
                            issuer_did: Some("did:dawn:skill-publisher:remote".to_string()),
                            package_url: "packages/remote-skill-1.0.0.wasm".to_string(),
                            install_url: "/api/gateway/marketplace/install/skill".to_string(),
                        }],
                        agent_cards: vec![MarketplaceAgentEntry {
                            card_id: "remote-agent".to_string(),
                            name: "Remote Agent".to_string(),
                            description: "Federated agent".to_string(),
                            url: "a2a/remote-agent".to_string(),
                            published: true,
                            locally_hosted: false,
                            chat_platforms: vec!["telegram".to_string()],
                            model_providers: vec!["openai".to_string()],
                            payment_roles: vec!["payee".to_string()],
                            issuer_did: Some("did:dawn:agent:remote".to_string()),
                            card_url: Some(".well-known/agent-card/remote-agent.json".to_string()),
                            install_url: Some(
                                "/api/gateway/marketplace/install/agent-card".to_string(),
                            ),
                        }],
                    })
                }),
            );
            let _ = axum::serve(listener, app).await;
        });
        sleep(Duration::from_millis(50)).await;

        let (state, db_path) = test_state().await.unwrap();
        let now = unix_timestamp_ms();
        let base_url = format!("http://{address}");
        let peer_id = derive_peer_id(&base_url);
        state
            .upsert_marketplace_peer(MarketplacePeerRecord {
                peer_id: peer_id.clone(),
                display_name: "Remote Peer".to_string(),
                base_url: base_url.clone(),
                catalog_url: default_peer_catalog_url(&base_url),
                enabled: true,
                trust_enabled: true,
                sync_status: MarketplacePeerSyncStatus::Pending,
                last_sync_error: None,
                last_synced_at_unix_ms: None,
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            })
            .await
            .unwrap();

        let catalog = build_federated_catalog(
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

        assert_eq!(catalog.peers.len(), 1);
        assert_eq!(catalog.peers[0].peer.peer_id, peer_id);
        assert_eq!(
            catalog.peers[0].peer.sync_status,
            MarketplacePeerSyncStatus::Healthy
        );
        assert!(catalog.peers[0].peer.last_synced_at_unix_ms.is_some());
        assert_eq!(catalog.peers[0].skill_count, 1);
        assert_eq!(catalog.peers[0].agent_card_count, 1);

        let remote_skill = catalog
            .skills
            .iter()
            .find(|entry| entry.source_peer_id == peer_id)
            .unwrap();
        assert_eq!(remote_skill.source_kind, "peer");
        assert_eq!(
            remote_skill.entry.package_url,
            format!("{base_url}/packages/remote-skill-1.0.0.wasm")
        );
        assert_eq!(
            remote_skill.entry.install_url,
            format!("{base_url}/api/gateway/marketplace/install/skill")
        );

        let remote_agent = catalog
            .agent_cards
            .iter()
            .find(|entry| entry.source_peer_id == peer_id)
            .unwrap();
        assert_eq!(
            remote_agent.entry.url,
            format!("{base_url}/a2a/remote-agent")
        );
        assert_eq!(
            remote_agent.entry.card_url.as_deref(),
            Some(format!("{base_url}/.well-known/agent-card/remote-agent.json").as_str())
        );
        assert_eq!(
            remote_agent.entry.install_url.as_deref(),
            Some(format!("{base_url}/api/gateway/marketplace/install/agent-card").as_str())
        );

        let persisted = state.get_marketplace_peer(&peer_id).await.unwrap().unwrap();
        assert_eq!(persisted.sync_status, MarketplacePeerSyncStatus::Healthy);
        assert!(persisted.last_sync_error.is_none());
        assert!(persisted.last_synced_at_unix_ms.is_some());

        server.abort();
        drop(state);
        fs::remove_file(db_path).ok();
    }
}
