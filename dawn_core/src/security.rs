use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::Context;
use axum::{
    Json,
    extract::{ConnectInfo, Request},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};
use reqwest::Url;
use serde_json::{Value, json};

const ADMIN_TOKEN_ENV: &str = "DAWN_GATEWAY_ADMIN_TOKEN";

pub async fn require_local_or_admin_token(
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<Value>)> {
    if is_loopback_request(&request) || admin_token_matches(request.headers()) {
        return Ok(next.run(request).await);
    }

    let status = if admin_token_configured() {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::FORBIDDEN
    };
    Err((
        status,
        Json(json!({
            "error": format!(
                "non-local gateway control requests require {ADMIN_TOKEN_ENV} and a matching bearer or x-dawn-admin-token header"
            )
        })),
    ))
}

pub fn bind_addr_from_env() -> String {
    std::env::var("DAWN_GATEWAY_BIND")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "127.0.0.1:8000".to_string())
}

pub fn validate_public_http_url(raw: &str, field_name: &str) -> anyhow::Result<Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{field_name} must not be empty");
    }
    let url = Url::parse(trimmed)
        .with_context(|| format!("{field_name} must be an absolute http(s) URL"))?;
    validate_public_http_url_parts(&url, field_name)?;
    Ok(url)
}

fn is_loopback_request(request: &Request) -> bool {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().is_loopback())
        .unwrap_or(true)
}

fn admin_token_configured() -> bool {
    configured_admin_token().is_some()
}

fn admin_token_matches(headers: &HeaderMap) -> bool {
    let Some(expected) = configured_admin_token() else {
        return false;
    };
    header_admin_tokens(headers)
        .any(|actual| constant_time_eq(actual.as_bytes(), expected.as_bytes()))
}

fn configured_admin_token() -> Option<String> {
    std::env::var(ADMIN_TOKEN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn header_admin_tokens(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim);
    let explicit = headers
        .get("x-dawn-admin-token")
        .or_else(|| headers.get("x-dawn-operator-token"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    bearer.into_iter().chain(explicit)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn validate_public_http_url_parts(url: &Url, field_name: &str) -> anyhow::Result<()> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!("{field_name} must use http or https, got '{scheme}'"),
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("{field_name} must not contain embedded credentials");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("{field_name} must include a host"))?;
    validate_public_host(host, field_name)
}

fn validate_public_host(host: &str, field_name: &str) -> anyhow::Result<()> {
    if allow_local_outbound_urls_for_development() {
        return Ok(());
    }
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized == "metadata.google.internal"
        || normalized.ends_with(".metadata.google.internal")
    {
        anyhow::bail!("{field_name} must not target local or metadata hosts");
    }
    if let Ok(ip) = normalized.parse::<IpAddr>() {
        if is_disallowed_outbound_ip(ip) {
            anyhow::bail!(
                "{field_name} must not target loopback, private, link-local, or metadata IP ranges"
            );
        }
    }
    Ok(())
}

fn is_disallowed_outbound_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => is_disallowed_outbound_ipv4(addr),
        IpAddr::V6(addr) => is_disallowed_outbound_ipv6(addr),
    }
}

fn is_disallowed_outbound_ipv4(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_unspecified()
        || addr.is_broadcast()
        || addr.is_multicast()
        || octets[0] == 0
        || octets[0] == 169 && octets[1] == 254
        || octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_disallowed_outbound_ipv6(addr: Ipv6Addr) -> bool {
    if let Some(mapped) = addr.to_ipv4_mapped() {
        return is_disallowed_outbound_ipv4(mapped);
    }
    let first_segment = addr.segments()[0];
    addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_unique_local()
        || addr.is_multicast()
        || (first_segment & 0xffc0) == 0xfe80
}

fn allow_local_outbound_urls_for_development() -> bool {
    cfg!(test)
        || std::env::var("DAWN_ALLOW_LOCAL_OUTBOUND_URLS")
            .ok()
            .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, validate_public_http_url};

    #[test]
    fn constant_time_eq_requires_exact_match() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokem"));
        assert!(!constant_time_eq(b"token", b"token-longer"));
    }

    #[test]
    fn public_http_url_rejects_non_http_and_credentials() {
        assert!(validate_public_http_url("file:///etc/passwd", "url").is_err());
        assert!(validate_public_http_url("https://user:pass@example.com", "url").is_err());
        assert!(validate_public_http_url("https://example.com/path", "url").is_ok());
    }
}
