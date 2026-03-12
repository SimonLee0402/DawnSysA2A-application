use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

const PROFILE_DIR_NAME: &str = ".dawn";
const PROFILE_FILE_NAME: &str = "desktop-cli.json";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DawnCliProfile {
    pub gateway_base_url: Option<String>,
    pub session_token: Option<String>,
    pub operator_name: Option<String>,
    pub bootstrap_mode: Option<String>,
    pub node_id: Option<String>,
    pub node_name: Option<String>,
    pub claim_token: Option<String>,
    pub requested_capabilities: Vec<String>,
    #[serde(default)]
    pub connector_env: BTreeMap<String, String>,
}

pub fn load_profile() -> anyhow::Result<DawnCliProfile> {
    let path = profile_path()?;
    load_profile_from_path(&path)
}

pub fn load_profile_or_default() -> DawnCliProfile {
    load_profile().unwrap_or_default()
}

pub fn save_profile(profile: &DawnCliProfile) -> anyhow::Result<PathBuf> {
    let path = profile_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create profile directory {}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(profile).context("failed to serialize CLI profile")?;
    fs::write(&path, body)
        .with_context(|| format!("failed to write CLI profile {}", path.display()))?;
    Ok(path)
}

pub fn load_profile_from_path(path: &Path) -> anyhow::Result<DawnCliProfile> {
    if !path.exists() {
        return Ok(DawnCliProfile::default());
    }
    let raw =
        fs::read(path).with_context(|| format!("failed to read CLI profile {}", path.display()))?;
    serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse CLI profile {}", path.display()))
}

pub fn profile_path() -> anyhow::Result<PathBuf> {
    let home = user_home_dir()?;
    Ok(home.join(PROFILE_DIR_NAME).join(PROFILE_FILE_NAME))
}

pub fn default_gateway_base_url() -> String {
    normalize_http_base_url(
        env::var("DAWN_GATEWAY_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8000".to_string()),
    )
}

pub fn normalize_http_base_url(raw: impl AsRef<str>) -> String {
    raw.as_ref().trim().trim_end_matches('/').to_string()
}

pub fn http_base_to_ws_base(raw: &str) -> String {
    let normalized = normalize_http_base_url(raw);
    if let Some(rest) = normalized.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = normalized.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if normalized.starts_with("ws://") || normalized.starts_with("wss://") {
        normalized
    } else {
        format!("ws://{normalized}")
    }
}

fn user_home_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("HOME") {
        return Ok(PathBuf::from(path));
    }
    Err(anyhow!(
        "unable to locate the user home directory from USERPROFILE or HOME"
    ))
}

#[cfg(test)]
mod tests {
    use super::{http_base_to_ws_base, normalize_http_base_url};

    #[test]
    fn normalizes_gateway_base_url() {
        assert_eq!(
            normalize_http_base_url("http://127.0.0.1:8000/"),
            "http://127.0.0.1:8000"
        );
    }

    #[test]
    fn converts_http_urls_to_websocket_urls() {
        assert_eq!(
            http_base_to_ws_base("https://gateway.example.com/"),
            "wss://gateway.example.com"
        );
        assert_eq!(
            http_base_to_ws_base("http://127.0.0.1:8000"),
            "ws://127.0.0.1:8000"
        );
    }
}
