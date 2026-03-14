use std::{
    collections::BTreeMap,
    env,
    net::TcpListener,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{process::Command, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const MANAGED_BROWSER_READY_TIMEOUT: Duration = Duration::from_secs(15);
const MANAGED_BROWSER_HTML_BYTES: usize = 1_500_000;
const MANAGED_BROWSER_CONSOLE_CAPTURE_SOURCE: &str = r#"
(() => {
    const root = globalThis;
    const maxMessages = 200;
    const truncate = (value, limit) => {
        const stringValue = typeof value === "string" ? value : String(value ?? "");
        if (stringValue.length <= limit) {
            return stringValue;
        }
        return stringValue.slice(0, Math.max(0, limit - 3)) + "...";
    };
    const stringify = (value) => {
        if (typeof value === "string") {
            return value;
        }
        if (value instanceof Error) {
            return value.stack || value.message || String(value);
        }
        if (typeof value === "undefined") {
            return "undefined";
        }
        if (typeof value === "function") {
            return `[Function ${value.name || "anonymous"}]`;
        }
        try {
            return JSON.stringify(value);
        } catch (_error) {
            return String(value);
        }
    };
    const state = root.__dawnConsoleCaptureState || { messages: [] };
    const push = (level, args, source) => {
        const parts = Array.from(args || []).map((value) => truncate(stringify(value), 800));
        state.messages.push({
            level,
            text: truncate(parts.join(" "), 4000),
            source: source || null,
            url: typeof location !== "undefined" ? location.href : null,
            timestampMs: Date.now(),
            argumentCount: parts.length,
        });
        if (state.messages.length > maxMessages) {
            state.messages.splice(0, state.messages.length - maxMessages);
        }
    };
    root.__dawnConsoleCaptureState = state;
    if (root.__dawnConsoleCaptureInstalled) {
        return true;
    }
    root.__dawnConsoleCaptureInstalled = true;
    for (const level of ["log", "info", "warn", "error", "debug"]) {
        const original = typeof console[level] === "function" ? console[level].bind(console) : null;
        if (!original || console[`__dawnOriginal_${level}`]) {
            continue;
        }
        console[`__dawnOriginal_${level}`] = original;
        console[level] = (...args) => {
            try {
                push(level, args, "console");
            } catch (_error) {}
            return original(...args);
        };
    }
    if (typeof window !== "undefined" && typeof window.addEventListener === "function") {
        window.addEventListener("error", (event) => {
            push("error", [event.message || "Script error"], "window.error");
        });
        window.addEventListener("unhandledrejection", (event) => {
            push("error", [event.reason || "Unhandled promise rejection"], "unhandledrejection");
        });
    }
    return true;
})()
"#;
const MANAGED_BROWSER_NETWORK_CAPTURE_SOURCE: &str = r#"
(() => {
    const root = globalThis;
    const maxEntries = 200;
    const maxEvents = 400;
    const previewLimit = 4000;
    const state = root.__dawnNetworkCaptureState || { requests: [] };
    const truncate = (value, limit) => {
        const stringValue = typeof value === "string" ? value : String(value ?? "");
        if (stringValue.length <= limit) {
            return stringValue;
        }
        return stringValue.slice(0, Math.max(0, limit - 3)) + "...";
    };
    const normalizeContentType = (value) =>
        typeof value === "string" && value.trim()
            ? value.trim().toLowerCase()
            : null;
    const isPreviewableContentType = (contentType) => {
        if (!contentType) {
            return false;
        }
        return (
            contentType.includes("json") ||
            contentType.startsWith("text/") ||
            contentType.includes("xml") ||
            contentType.includes("javascript") ||
            contentType.includes("html") ||
            contentType.includes("svg")
        );
    };
    const buildPreviewRecord = (rawValue) => {
        const value = typeof rawValue === "string" ? rawValue : String(rawValue ?? "");
        return {
            bodyPreview: truncate(value, previewLimit),
            bodyTruncated: value.length > previewLimit,
        };
    };
    const pushEvent = (event) => {
        state.events = Array.isArray(state.events) ? state.events : [];
        state.events.push(event);
        if (state.events.length > maxEvents) {
            state.events.splice(0, state.events.length - maxEvents);
        }
    };
    const push = (entry) => {
        state.requests.push(entry);
        if (state.requests.length > maxEntries) {
            state.requests.splice(0, state.requests.length - maxEntries);
        }
    };
    state.events = Array.isArray(state.events) ? state.events : [];
    root.__dawnNetworkCaptureState = state;
    if (root.__dawnNetworkCaptureInstalled) {
        return true;
    }
    root.__dawnNetworkCaptureInstalled = true;

    const originalFetch = typeof root.fetch === "function" ? root.fetch.bind(root) : null;
    if (originalFetch && !root.__dawnOriginalFetch) {
        root.__dawnOriginalFetch = originalFetch;
        root.fetch = async (...args) => {
            const startedAtMs = Date.now();
            let method = "GET";
            let url = "";
            try {
                if (typeof args[0] === "string") {
                    url = new URL(args[0], location.href).toString();
                } else if (args[0] && typeof args[0].url === "string") {
                    url = new URL(args[0].url, location.href).toString();
                }
                if (args[0] && typeof args[0].method === "string") {
                    method = args[0].method.toUpperCase();
                }
                if (args[1] && typeof args[1].method === "string") {
                    method = args[1].method.toUpperCase();
                }
            } catch (_error) {}
            pushEvent({
                timestampMs: startedAtMs,
                source: "network",
                eventType: "fetch",
                phase: "request",
                url: url || null,
                method,
                status: null,
                text: null,
                contentType: null,
            });
            try {
                const response = await originalFetch(...args);
                const contentType = normalizeContentType(response.headers.get("content-type"));
                let bodyPreview = null;
                let bodyTruncated = null;
                let captureError = null;
                if (response.type !== "opaque" && isPreviewableContentType(contentType)) {
                    try {
                        const contentLength = Number(response.headers.get("content-length") || "");
                        if (!Number.isFinite(contentLength) || contentLength <= 100000) {
                            const preview = buildPreviewRecord(await response.clone().text());
                            bodyPreview = preview.bodyPreview;
                            bodyTruncated = preview.bodyTruncated;
                        } else {
                            captureError = `response body skipped because content-length ${contentLength} exceeds preview budget`;
                        }
                    } catch (error) {
                        captureError = truncate(error?.message || String(error || "response preview failed"), 200);
                    }
                }
                push({
                    url,
                    entryType: "fetch",
                    source: "capture",
                    initiatorType: "fetch",
                    method,
                    contentType,
                    bodyPreview,
                    bodyTruncated,
                    captureError,
                    durationMs: Date.now() - startedAtMs,
                    responseStatus: Number.isFinite(response.status) ? response.status : null,
                    statusText: truncate(response.statusText || "", 120) || null,
                    ok: Boolean(response.ok),
                    startedAtMs,
                    completedAtMs: Date.now(),
                    transferSize: null,
                    encodedBodySize: null,
                    decodedBodySize: null,
                    nextHopProtocol: null,
                });
                pushEvent({
                    timestampMs: Date.now(),
                    source: "network",
                    eventType: "fetch",
                    phase: "response",
                    url: url || null,
                    method,
                    status: Number.isFinite(response.status) ? response.status : null,
                    text: truncate(response.statusText || "", 120) || null,
                    contentType,
                });
                return response;
            } catch (error) {
                push({
                    url,
                    entryType: "fetch",
                    source: "capture",
                    initiatorType: "fetch",
                    method,
                    contentType: null,
                    bodyPreview: null,
                    bodyTruncated: null,
                    captureError: truncate(error?.message || String(error || "fetch failed"), 200),
                    durationMs: Date.now() - startedAtMs,
                    responseStatus: null,
                    statusText: truncate(error?.message || String(error || "fetch failed"), 160),
                    ok: false,
                    startedAtMs,
                    completedAtMs: Date.now(),
                    transferSize: null,
                    encodedBodySize: null,
                    decodedBodySize: null,
                    nextHopProtocol: null,
                });
                pushEvent({
                    timestampMs: Date.now(),
                    source: "network",
                    eventType: "fetch",
                    phase: "error",
                    url: url || null,
                    method,
                    status: null,
                    text: truncate(error?.message || String(error || "fetch failed"), 160),
                    contentType: null,
                });
                throw error;
            }
        };
    }

    const OriginalXHR = root.XMLHttpRequest;
    if (typeof OriginalXHR === "function" && !root.__dawnOriginalXMLHttpRequest) {
        root.__dawnOriginalXMLHttpRequest = OriginalXHR;
        const open = OriginalXHR.prototype.open;
        const send = OriginalXHR.prototype.send;
        OriginalXHR.prototype.open = function(method, url, ...rest) {
            this.__dawnMethod = typeof method === "string" ? method.toUpperCase() : "GET";
            try {
                this.__dawnUrl = new URL(url, location.href).toString();
            } catch (_error) {
                this.__dawnUrl = typeof url === "string" ? url : "";
            }
            return open.call(this, method, url, ...rest);
        };
        OriginalXHR.prototype.send = function(...args) {
            const startedAtMs = Date.now();
            pushEvent({
                timestampMs: startedAtMs,
                source: "network",
                eventType: "xhr",
                phase: "request",
                url: this.__dawnUrl || null,
                method: this.__dawnMethod || "GET",
                status: null,
                text: null,
                contentType: null,
            });
            const finalize = () => {
                const contentType = normalizeContentType(this.getResponseHeader("content-type"));
                let bodyPreview = null;
                let bodyTruncated = null;
                let captureError = null;
                try {
                    const responseType = typeof this.responseType === "string" ? this.responseType : "";
                    if (
                        isPreviewableContentType(contentType) &&
                        (responseType === "" || responseType === "text")
                    ) {
                        const preview = buildPreviewRecord(this.responseText || "");
                        bodyPreview = preview.bodyPreview;
                        bodyTruncated = preview.bodyTruncated;
                    } else if (responseType === "json" && this.response !== undefined) {
                        const preview = buildPreviewRecord(JSON.stringify(this.response));
                        bodyPreview = preview.bodyPreview;
                        bodyTruncated = preview.bodyTruncated;
                    }
                } catch (error) {
                    captureError = truncate(error?.message || String(error || "xhr response preview failed"), 200);
                }
                push({
                    url: this.__dawnUrl || "",
                    entryType: "xhr",
                    source: "capture",
                    initiatorType: "xmlhttprequest",
                    method: this.__dawnMethod || "GET",
                    contentType,
                    bodyPreview,
                    bodyTruncated,
                    captureError,
                    durationMs: Date.now() - startedAtMs,
                    responseStatus: Number.isFinite(this.status) ? this.status : null,
                    statusText: truncate(this.statusText || "", 120) || null,
                    ok: this.status >= 200 && this.status < 400,
                    startedAtMs,
                    completedAtMs: Date.now(),
                    transferSize: null,
                    encodedBodySize: null,
                    decodedBodySize: null,
                    nextHopProtocol: null,
                });
                pushEvent({
                    timestampMs: Date.now(),
                    source: "network",
                    eventType: "xhr",
                    phase:
                        Number.isFinite(this.status) && this.status >= 200 && this.status < 400
                            ? "response"
                            : "error",
                    url: this.__dawnUrl || null,
                    method: this.__dawnMethod || "GET",
                    status: Number.isFinite(this.status) ? this.status : null,
                    text: truncate(this.statusText || "", 120) || null,
                    contentType,
                });
                this.removeEventListener("loadend", finalize);
            };
            this.addEventListener("loadend", finalize);
            return send.apply(this, args);
        };
    }
    return true;
})()
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserSession {
    pub backend: String,
    pub executable: String,
    pub debug_port: u16,
    pub browser_pid: Option<u32>,
    pub profile_name: Option<String>,
    pub persistent_profile: bool,
    pub target_id: String,
    pub websocket_url: String,
    pub user_data_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ManagedBrowserPageState {
    pub current_url: String,
    pub title: Option<String>,
    pub html: String,
    pub ready_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserClickResult {
    pub tag: String,
    pub text: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserTypeResult {
    pub tag: String,
    pub field_type: Option<String>,
    pub value: String,
    pub submitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserUploadResult {
    pub tag: String,
    pub field_type: Option<String>,
    pub field_name: Option<String>,
    pub file_name: Option<String>,
    pub file_count: usize,
    pub submitted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserEvaluationResult {
    pub value: Value,
    pub value_type: String,
    pub subtype: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserScreenshotResult {
    pub saved_path: String,
    pub bytes_written: usize,
    pub format: &'static str,
    pub full_page: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserPdfResult {
    pub saved_path: String,
    pub bytes_written: usize,
    pub format: &'static str,
    pub landscape: bool,
    pub print_background: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserWaitResult {
    pub selector: Option<String>,
    pub text: Option<String>,
    pub text_gone: Option<String>,
    pub selector_matched: bool,
    pub text_found: bool,
    pub text_gone_satisfied: bool,
    pub elapsed_ms: u128,
    pub current_url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserKeyPressResult {
    pub key: String,
    pub code: String,
    pub modifiers: Vec<String>,
    pub text: Option<String>,
    pub active_tag: String,
    pub active_type: Option<String>,
    pub active_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserDialogResult {
    pub accepted: bool,
    pub prompt_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserNetworkEntry {
    pub url: String,
    pub entry_type: String,
    pub source: Option<String>,
    pub initiator_type: Option<String>,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub body_preview: Option<String>,
    pub body_truncated: Option<bool>,
    pub capture_error: Option<String>,
    pub duration_ms: f64,
    pub transfer_size: Option<u64>,
    pub encoded_body_size: Option<u64>,
    pub decoded_body_size: Option<u64>,
    pub response_status: Option<u16>,
    pub status_text: Option<String>,
    pub ok: Option<bool>,
    pub started_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
    pub next_hop_protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserNetworkLog {
    pub current_url: String,
    pub navigation_count: usize,
    pub resource_count: usize,
    pub entries: Vec<ManagedBrowserNetworkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserConsoleMessage {
    pub level: String,
    pub text: String,
    pub source: Option<String>,
    pub url: Option<String>,
    pub timestamp_ms: Option<u64>,
    pub argument_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserConsoleLog {
    pub current_url: String,
    pub count: usize,
    pub messages: Vec<ManagedBrowserConsoleMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserErrorLog {
    pub current_url: String,
    pub console_count: usize,
    pub network_count: usize,
    pub console_messages: Vec<ManagedBrowserConsoleMessage>,
    pub network_entries: Vec<ManagedBrowserNetworkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserTraceEvent {
    pub timestamp_ms: u64,
    pub source: String,
    pub event_type: String,
    pub phase: Option<String>,
    pub level: Option<String>,
    pub url: Option<String>,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub text: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserTraceLog {
    pub current_url: String,
    pub count: usize,
    pub events: Vec<ManagedBrowserTraceEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserCookieLog {
    pub current_url: String,
    pub count: usize,
    pub cookies: Vec<ManagedBrowserCookie>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserStorageEntry {
    pub key: String,
    pub value: String,
    pub value_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserStorageSnapshot {
    pub current_url: String,
    pub origin: Option<String>,
    pub local_count: usize,
    pub session_count: usize,
    pub local_storage: Vec<ManagedBrowserStorageEntry>,
    pub session_storage: Vec<ManagedBrowserStorageEntry>,
    pub local_error: Option<String>,
    pub session_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserStorageMutationResult {
    pub current_url: String,
    pub origin: Option<String>,
    pub storage_area: String,
    pub key: String,
    pub value: Option<String>,
    pub removed: bool,
    pub had_existing_value: bool,
    pub previous_value: Option<String>,
    pub local_count: usize,
    pub session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserHeadersResult {
    pub current_url: String,
    pub header_count: usize,
    pub headers: BTreeMap<String, String>,
    pub reloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserOfflineResult {
    pub current_url: String,
    pub offline: bool,
    pub latency_ms: u64,
    pub download_throughput_kbps: Option<u64>,
    pub upload_throughput_kbps: Option<u64>,
    pub reloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserGeolocationResult {
    pub current_url: String,
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: f64,
    pub reloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserDeviceEmulationResult {
    pub current_url: String,
    pub preset: Option<String>,
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
    pub mobile: bool,
    pub touch: bool,
    pub user_agent: Option<String>,
    pub platform: Option<String>,
    pub accept_language: Option<String>,
    pub reloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserTargetSummary {
    pub target_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub target_type: String,
    pub websocket_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserStatus {
    pub backend: String,
    pub executable: String,
    pub debug_port: u16,
    pub browser_pid: Option<u32>,
    pub profile_name: Option<String>,
    pub persistent_profile: bool,
    pub user_data_dir: String,
    pub current_target_id: String,
    pub target_count: usize,
    pub page_target_count: usize,
    pub targets: Vec<ManagedBrowserTargetSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserProfileSummary {
    pub profile_name: String,
    pub profile_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserProfileDeleteResult {
    pub profile_name: String,
    pub profile_path: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserProfileInspectResult {
    pub profile_name: String,
    pub profile_path: String,
    pub directory_count: usize,
    pub file_count: usize,
    pub total_bytes: u64,
    pub last_modified_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserProfileExportResult {
    pub profile_name: String,
    pub source_path: String,
    pub export_path: String,
    pub directory_count: usize,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserProfileImportResult {
    pub profile_name: String,
    pub source_path: String,
    pub profile_path: String,
    pub directory_count: usize,
    pub file_count: usize,
    pub total_bytes: u64,
    pub overwritten: bool,
}

#[derive(Debug, Clone)]
pub struct ManagedBrowserDownloadRequest {
    pub source_url: String,
    pub current_url: String,
    pub user_agent: Option<String>,
    pub cookie_header: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserTargetInfo {
    id: String,
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    web_socket_debugger_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserVersionInfo {
    #[serde(default)]
    web_socket_debugger_url: String,
}

#[derive(Debug)]
struct CdpConnection {
    writer: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    reader: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    next_id: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedBrowserActiveElement {
    tag: String,
    field_type: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Default)]
struct ManagedBrowserProfileStats {
    directory_count: usize,
    file_count: usize,
    total_bytes: u64,
    last_modified_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedBrowserWaitConditionState {
    matched: bool,
    selector_matched: bool,
    text_found: bool,
    text_gone_satisfied: bool,
    current_url: String,
    title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedBrowserKeySpec {
    key: String,
    code: String,
    modifiers_mask: i32,
    modifier_labels: Vec<String>,
    windows_virtual_key_code: i32,
    text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires: Option<f64>,
    pub size: Option<u64>,
    pub http_only: Option<bool>,
    pub secure: Option<bool>,
    pub session: Option<bool>,
    pub same_site: Option<String>,
    pub source_scheme: Option<String>,
    pub source_port: Option<i32>,
}

impl CdpConnection {
    async fn connect(websocket_url: &str) -> anyhow::Result<Self> {
        let (socket, _) = connect_async(websocket_url).await.with_context(|| {
            format!("failed to connect to managed browser websocket {websocket_url}")
        })?;
        let (writer, reader) = socket.split();
        Ok(Self {
            writer,
            reader,
            next_id: 1,
        })
    }

    async fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let request_id = self.next_id;
        self.next_id += 1;
        self.writer
            .send(Message::Text(
                json!({
                    "id": request_id,
                    "method": method,
                    "params": params,
                })
                .to_string()
                .into(),
            ))
            .await
            .with_context(|| format!("failed to send CDP command {method}"))?;

        while let Some(message) = self.reader.next().await {
            let message = message.context("managed browser websocket error")?;
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close(_) => anyhow::bail!(
                    "managed browser websocket closed while waiting for CDP response to {method}"
                ),
                Message::Frame(_) => continue,
            };
            let payload: Value = serde_json::from_str(&text)
                .with_context(|| format!("failed to parse CDP response for {method}"))?;
            if payload.get("id").and_then(Value::as_u64) != Some(request_id) {
                continue;
            }
            if let Some(error) = payload.get("error") {
                let detail = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown CDP error");
                anyhow::bail!("managed browser command {method} failed: {detail}");
            }
            return Ok(payload.get("result").cloned().unwrap_or_else(|| json!({})));
        }

        anyhow::bail!("managed browser websocket ended before CDP response to {method}");
    }
}

pub async fn launch_managed_browser(
    url: &str,
) -> anyhow::Result<(ManagedBrowserSession, ManagedBrowserPageState)> {
    launch_managed_browser_with_profile(url, None, false).await
}

pub async fn launch_managed_browser_with_profile(
    url: &str,
    profile_name: Option<&str>,
    persistent_profile: bool,
) -> anyhow::Result<(ManagedBrowserSession, ManagedBrowserPageState)> {
    let executable = detect_browser_executable()?;
    let debug_port = reserve_debug_port()?;
    let normalized_profile_name = profile_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(sanitize_managed_browser_profile_name)
        .or_else(|| persistent_profile.then(|| "default".to_string()));
    let user_data_dir = resolve_managed_browser_user_data_dir(
        normalized_profile_name.as_deref(),
        persistent_profile,
        debug_port,
    );
    std::fs::create_dir_all(&user_data_dir).with_context(|| {
        format!(
            "failed to create managed browser profile directory {}",
            user_data_dir.display()
        )
    })?;

    let browser_pid =
        spawn_managed_browser_process(&executable, debug_port, &user_data_dir).await?;
    let browser_client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build managed browser HTTP client")?;
    let mut target = wait_for_page_target(&browser_client, debug_port).await?;
    if target.web_socket_debugger_url.is_empty() {
        anyhow::bail!("managed browser target did not expose a websocket debugger URL");
    }

    let mut connection = CdpConnection::connect(&target.web_socket_debugger_url).await?;
    enable_cdp_page_domains(&mut connection).await?;
    navigate_with_connection(&mut connection, url).await?;
    let page = capture_page_state(&mut connection).await?;

    target.title = page.title.clone();
    target.url = Some(page.current_url.clone());
    let session = ManagedBrowserSession {
        backend: browser_backend_label(&executable),
        executable,
        debug_port,
        browser_pid,
        profile_name: normalized_profile_name,
        persistent_profile,
        target_id: target.id,
        websocket_url: target.web_socket_debugger_url,
        user_data_dir,
    };
    Ok((session, page))
}

pub async fn navigate_managed_browser(
    session: &ManagedBrowserSession,
    url: &str,
) -> anyhow::Result<ManagedBrowserPageState> {
    let mut connection = open_cdp_connection(session).await?;
    navigate_with_connection(&mut connection, url).await?;
    capture_page_state(&mut connection).await
}

pub async fn open_managed_browser_tab(
    session: &ManagedBrowserSession,
    url: &str,
) -> anyhow::Result<(ManagedBrowserSession, ManagedBrowserPageState)> {
    open_managed_browser_target(session, url, false).await
}

pub async fn open_managed_browser_window(
    session: &ManagedBrowserSession,
    url: &str,
) -> anyhow::Result<(ManagedBrowserSession, ManagedBrowserPageState)> {
    open_managed_browser_target(session, url, true).await
}

async fn open_managed_browser_target(
    session: &ManagedBrowserSession,
    url: &str,
    new_window: bool,
) -> anyhow::Result<(ManagedBrowserSession, ManagedBrowserPageState)> {
    let client = managed_browser_http_client()?;
    let browser_websocket_url = resolve_browser_websocket_url(&client, session.debug_port).await?;
    let mut browser_connection = CdpConnection::connect(&browser_websocket_url).await?;
    let target_id = browser_connection
        .call(
            "Target.createTarget",
            json!({
                "url": url,
                "newWindow": new_window,
            }),
        )
        .await?
        .get("targetId")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("managed browser Target.createTarget did not return targetId")
        })?;
    let _ = browser_connection
        .call("Target.activateTarget", json!({ "targetId": target_id }))
        .await;
    let target = wait_for_page_target_by_id(&client, session.debug_port, &target_id).await?;
    let mut page_connection = CdpConnection::connect(&target.web_socket_debugger_url).await?;
    enable_cdp_page_domains(&mut page_connection).await?;
    wait_for_page_settle(&mut page_connection).await?;
    let page = capture_page_state(&mut page_connection).await?;
    let next_session = ManagedBrowserSession {
        backend: session.backend.clone(),
        executable: session.executable.clone(),
        debug_port: session.debug_port,
        browser_pid: session.browser_pid,
        profile_name: session.profile_name.clone(),
        persistent_profile: session.persistent_profile,
        target_id: target.id,
        websocket_url: target.web_socket_debugger_url,
        user_data_dir: session.user_data_dir.clone(),
    };
    Ok((next_session, page))
}

pub async fn activate_managed_browser(session: &ManagedBrowserSession) -> anyhow::Result<()> {
    let client = managed_browser_http_client()?;
    client
        .get(format!(
            "http://127.0.0.1:{}/json/activate/{}",
            session.debug_port, session.target_id
        ))
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to activate managed browser target {}",
                session.target_id
            )
        })?
        .error_for_status()
        .context("managed browser activation endpoint returned an error")?;
    Ok(())
}

pub async fn inspect_managed_browser(
    session: &ManagedBrowserSession,
) -> anyhow::Result<ManagedBrowserStatus> {
    let client = managed_browser_http_client()?;
    let targets = list_browser_targets(&client, session.debug_port).await?;
    let page_target_count = targets
        .iter()
        .filter(|target| target.r#type == "page")
        .count();
    let targets = targets
        .into_iter()
        .map(|target| ManagedBrowserTargetSummary {
            target_id: target.id,
            title: target.title,
            url: target.url,
            target_type: target.r#type,
            websocket_available: !target.web_socket_debugger_url.is_empty(),
        })
        .collect::<Vec<_>>();
    Ok(ManagedBrowserStatus {
        backend: session.backend.clone(),
        executable: session.executable.clone(),
        debug_port: session.debug_port,
        browser_pid: session.browser_pid,
        profile_name: session.profile_name.clone(),
        persistent_profile: session.persistent_profile,
        user_data_dir: session.user_data_dir.display().to_string(),
        current_target_id: session.target_id.clone(),
        target_count: targets.len(),
        page_target_count,
        targets,
    })
}

pub async fn close_managed_browser(
    session: &ManagedBrowserSession,
    terminate_browser: bool,
) -> anyhow::Result<()> {
    let client = managed_browser_http_client()?;
    let close_result = client
        .get(format!(
            "http://127.0.0.1:{}/json/close/{}",
            session.debug_port, session.target_id
        ))
        .send()
        .await;
    if let Err(error) = close_result {
        tracing::warn!(
            "failed to close managed browser target {} over devtools endpoint: {error}",
            session.target_id
        );
    }
    if terminate_browser {
        if let Some(pid) = session.browser_pid {
            terminate_browser_process(pid).await?;
        }
        if session.user_data_dir.exists() && !session.persistent_profile {
            remove_managed_browser_profile_dir(&session.user_data_dir).await?;
        }
    }
    Ok(())
}

pub fn list_managed_browser_profiles() -> anyhow::Result<Vec<ManagedBrowserProfileSummary>> {
    let root = managed_browser_profile_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut profiles = std::fs::read_dir(&root)
        .with_context(|| {
            format!(
                "failed to read managed browser profile root {}",
                root.display()
            )
        })?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            Some(ManagedBrowserProfileSummary {
                profile_name: entry.file_name().to_string_lossy().to_string(),
                profile_path: path.display().to_string(),
            })
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| left.profile_name.cmp(&right.profile_name));
    Ok(profiles)
}

pub async fn delete_managed_browser_profile(
    profile_name: &str,
) -> anyhow::Result<ManagedBrowserProfileDeleteResult> {
    let profile_name = sanitize_managed_browser_profile_name(profile_name);
    let profile_path = managed_browser_profile_root().join(&profile_name);
    let exists = tokio::fs::try_exists(&profile_path)
        .await
        .with_context(|| {
            format!(
                "failed to inspect managed browser profile {}",
                profile_path.display()
            )
        })?;
    if !exists {
        anyhow::bail!(
            "managed browser profile `{}` does not exist at {}",
            profile_name,
            profile_path.display()
        );
    }
    remove_managed_browser_profile_dir(&profile_path).await?;
    Ok(ManagedBrowserProfileDeleteResult {
        profile_name,
        profile_path: profile_path.display().to_string(),
        deleted: true,
    })
}

pub fn inspect_managed_browser_profile(
    profile_name: &str,
) -> anyhow::Result<ManagedBrowserProfileInspectResult> {
    let profile_name = sanitize_managed_browser_profile_name(profile_name);
    let profile_path = managed_browser_profile_root().join(&profile_name);
    if !profile_path.exists() {
        anyhow::bail!(
            "managed browser profile `{}` does not exist at {}",
            profile_name,
            profile_path.display()
        );
    }
    let mut stats = ManagedBrowserProfileStats::default();
    collect_managed_browser_profile_stats(&profile_path, &mut stats)?;
    Ok(ManagedBrowserProfileInspectResult {
        profile_name,
        profile_path: profile_path.display().to_string(),
        directory_count: stats.directory_count,
        file_count: stats.file_count,
        total_bytes: stats.total_bytes,
        last_modified_unix_ms: stats.last_modified_unix_ms,
    })
}

pub fn export_managed_browser_profile(
    profile_name: &str,
    export_path: &Path,
) -> anyhow::Result<ManagedBrowserProfileExportResult> {
    let inspected = inspect_managed_browser_profile(profile_name)?;
    let source_path = PathBuf::from(&inspected.profile_path);
    if source_path == export_path {
        anyhow::bail!("browser profile export path must differ from the source profile path");
    }
    if export_path.exists() {
        anyhow::bail!(
            "browser profile export path {} already exists",
            export_path.display()
        );
    }
    copy_managed_browser_profile_dir(&source_path, export_path)?;
    Ok(ManagedBrowserProfileExportResult {
        profile_name: inspected.profile_name,
        source_path: inspected.profile_path,
        export_path: export_path.display().to_string(),
        directory_count: inspected.directory_count,
        file_count: inspected.file_count,
        total_bytes: inspected.total_bytes,
    })
}

pub async fn import_managed_browser_profile(
    source_path: &Path,
    profile_name: &str,
    overwrite: bool,
) -> anyhow::Result<ManagedBrowserProfileImportResult> {
    if !source_path.exists() {
        anyhow::bail!(
            "managed browser profile source {} does not exist",
            source_path.display()
        );
    }
    if !source_path.is_dir() {
        anyhow::bail!(
            "managed browser profile source {} is not a directory",
            source_path.display()
        );
    }
    let profile_name = sanitize_managed_browser_profile_name(profile_name);
    let profile_path = managed_browser_profile_root().join(&profile_name);
    let existed = profile_path.exists();
    if existed && !overwrite {
        anyhow::bail!(
            "managed browser profile `{}` already exists at {}",
            profile_name,
            profile_path.display()
        );
    }
    if existed {
        remove_managed_browser_profile_dir(&profile_path).await?;
    }
    copy_managed_browser_profile_dir(source_path, &profile_path)?;
    let inspected = inspect_managed_browser_profile(&profile_name)?;
    Ok(ManagedBrowserProfileImportResult {
        profile_name: inspected.profile_name,
        source_path: source_path.display().to_string(),
        profile_path: inspected.profile_path,
        directory_count: inspected.directory_count,
        file_count: inspected.file_count,
        total_bytes: inspected.total_bytes,
        overwritten: existed,
    })
}

pub async fn click_managed_browser(
    session: &ManagedBrowserSession,
    selector: &str,
    element_index: usize,
) -> anyhow::Result<(ManagedBrowserClickResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let result = runtime_call(
        &mut connection,
        r#"(selector, elementIndex) => {
                const matches = Array.from(document.querySelectorAll(selector));
                const element = matches[elementIndex];
                if (!element) {
                    throw new Error(`no element matched selector ${selector} at index ${elementIndex}`);
                }
                element.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
                if (typeof element.focus === "function") {
                    element.focus();
                }
                const tag = (element.tagName || "").toLowerCase();
                const text = String(
                    element.innerText || element.textContent || element.value || ""
                ).trim().slice(0, 600);
                const href = typeof element.href === "string"
                    ? element.href
                    : element.getAttribute("href");
                if (typeof element.click === "function") {
                    element.click();
                } else {
                    element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
                }
                return { tag, text, href };
            }"#,
        json!([selector, element_index]),
    )
    .await?;
    wait_for_page_settle(&mut connection).await?;
    let page = capture_page_state(&mut connection).await?;
    let click = serde_json::from_value::<ManagedBrowserClickResult>(result)
        .context("failed to parse managed browser click result")?;
    Ok((click, page))
}

pub async fn type_managed_browser(
    session: &ManagedBrowserSession,
    selector: &str,
    text: &str,
    append: bool,
    submit: bool,
) -> anyhow::Result<(ManagedBrowserTypeResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let result = runtime_call(
        &mut connection,
        r#"(selector, text, append, submit) => {
                const element = document.querySelector(selector);
                if (!element) {
                    throw new Error(`no element matched selector ${selector}`);
                }
                const tag = (element.tagName || "").toLowerCase();
                const fieldType = element.getAttribute("type")
                    ? element.getAttribute("type").toLowerCase()
                    : null;
                if (!["input", "textarea", "select"].includes(tag)) {
                    throw new Error("browser_type currently supports input, textarea, and select elements");
                }
                if (fieldType === "file") {
                    throw new Error("browser_type does not support file inputs; use browser_upload instead");
                }
                element.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
                if (typeof element.focus === "function") {
                    element.focus();
                }

                let nextValue;
                if (tag === "select") {
                    const options = Array.from(element.options || []);
                    const option = options.find((candidate) => candidate.value === text || candidate.text === text);
                    if (!option) {
                        throw new Error(`no <option> matched ${text}`);
                    }
                    element.value = option.value;
                    nextValue = element.value;
                } else {
                    const baseValue = typeof element.value === "string" ? element.value : "";
                    nextValue = append ? `${baseValue}${text}` : text;
                    element.value = nextValue;
                    if (tag === "textarea") {
                        element.textContent = nextValue;
                    }
                }

                element.dispatchEvent(new Event("input", { bubbles: true }));
                element.dispatchEvent(new Event("change", { bubbles: true }));

                if (submit) {
                    const form = element.form || element.closest("form");
                    if (form) {
                        if (typeof form.requestSubmit === "function") {
                            form.requestSubmit();
                        } else if (typeof form.submit === "function") {
                            form.submit();
                        }
                    }
                }

                return {
                    tag,
                    fieldType,
                    value: String(nextValue ?? ""),
                    submitted: Boolean(submit),
                };
            }"#,
        json!([selector, text, append, submit]),
    )
    .await?;
    wait_for_page_settle(&mut connection).await?;
    let page = capture_page_state(&mut connection).await?;
    let typed = serde_json::from_value::<ManagedBrowserTypeResult>(result)
        .context("failed to parse managed browser type result")?;
    Ok((typed, page))
}

pub async fn upload_managed_browser(
    session: &ManagedBrowserSession,
    selector: &str,
    file_path: &Path,
    submit: bool,
) -> anyhow::Result<(ManagedBrowserUploadResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let file_path = file_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize upload path {}", file_path.display()))?;
    let file_path_string = file_path.display().to_string();
    let _preflight = runtime_call(
        &mut connection,
        r#"(selector) => {
                const element = document.querySelector(selector);
                if (!element) {
                    throw new Error(`no element matched selector ${selector}`);
                }
                const tag = (element.tagName || "").toLowerCase();
                const fieldType = element.getAttribute("type")
                    ? element.getAttribute("type").toLowerCase()
                    : null;
                if (tag !== "input" || fieldType !== "file") {
                    throw new Error("browser_upload requires an <input type=\"file\"> element");
                }
                return {
                    tag,
                    fieldType,
                    fieldName: element.getAttribute("name") || element.getAttribute("id") || null,
                };
            }"#,
        json!([selector]),
    )
    .await?;
    let node_id = resolve_dom_node_id(&mut connection, selector).await?;
    connection
        .call(
            "DOM.setFileInputFiles",
            json!({
                "nodeId": node_id,
                "files": [file_path_string],
            }),
        )
        .await
        .with_context(|| format!("failed to set managed browser file input {selector}"))?;
    let mut uploaded = serde_json::from_value::<ManagedBrowserUploadResult>(
        runtime_call(
            &mut connection,
            r#"(selector) => {
                    const element = document.querySelector(selector);
                    if (!element) {
                        throw new Error(`no element matched selector ${selector}`);
                    }
                    const files = Array.from(element.files || []);
                    return {
                        tag: (element.tagName || "").toLowerCase(),
                        fieldType: element.getAttribute("type")
                            ? element.getAttribute("type").toLowerCase()
                            : null,
                        fieldName: element.getAttribute("name") || element.getAttribute("id") || null,
                        fileName: files.length > 0 ? files[0].name : null,
                        fileCount: files.length,
                        submitted: false,
                    };
                }"#,
            json!([selector]),
        )
        .await?,
    )
    .context("failed to parse managed browser upload result")?;
    if submit {
        let submitted = runtime_call(
            &mut connection,
            r#"(selector) => {
                    const element = document.querySelector(selector);
                    if (!element) {
                        throw new Error(`no element matched selector ${selector}`);
                    }
                    const form = element.form || element.closest("form");
                    if (!form) {
                        return false;
                    }
                    if (typeof form.requestSubmit === "function") {
                        form.requestSubmit();
                    } else if (typeof form.submit === "function") {
                        form.submit();
                    }
                    return true;
                }"#,
            json!([selector]),
        )
        .await?
        .as_bool()
        .unwrap_or(false);
        uploaded.submitted = submitted;
    }
    wait_for_page_settle(&mut connection).await?;
    let page = capture_page_state(&mut connection).await?;
    Ok((uploaded, page))
}

pub async fn prepare_managed_browser_download(
    session: &ManagedBrowserSession,
    raw_url: Option<&str>,
    selector: Option<&str>,
    element_index: usize,
) -> anyhow::Result<(ManagedBrowserDownloadRequest, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let source_url = if let Some(raw_url) = raw_url.map(str::trim).filter(|value| !value.is_empty())
    {
        runtime_call(
            &mut connection,
            r#"(rawUrl) => new URL(rawUrl, location.href).toString()"#,
            json!([raw_url]),
        )
        .await?
        .as_str()
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("managed browser did not resolve a download URL"))?
    } else if let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) {
        runtime_call(
            &mut connection,
            r#"(selector, elementIndex) => {
                    const matches = Array.from(document.querySelectorAll(selector));
                    const element = matches[elementIndex];
                    if (!element) {
                        throw new Error(`no element matched selector ${selector} at index ${elementIndex}`);
                    }
                    const candidate =
                        (typeof element.href === "string" && element.href) ||
                        element.getAttribute("href") ||
                        element.getAttribute("src") ||
                        element.getAttribute("data-download-url") ||
                        element.getAttribute("data-href");
                    if (!candidate) {
                        throw new Error("browser_download selector did not resolve an href/src target");
                    }
                    return new URL(candidate, location.href).toString();
                }"#,
            json!([selector, element_index]),
        )
        .await?
        .as_str()
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("managed browser did not resolve a selector download URL"))?
    } else {
        anyhow::bail!("browser_download requires payload.url or payload.selector");
    };
    let source_url = validate_managed_browser_download_source(&source_url)?;
    let cookies = connection
        .call(
            "Network.getCookies",
            json!({ "urls": [source_url.clone()] }),
        )
        .await?
        .get("cookies")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let cookies = serde_json::from_value::<Vec<ManagedBrowserCookie>>(cookies)
        .context("failed to decode managed browser cookies")?;
    let user_agent = runtime_call(
        &mut connection,
        r#"() => navigator.userAgent || null"#,
        json!([]),
    )
    .await?
    .as_str()
    .map(ToString::to_string)
    .filter(|value| !value.trim().is_empty());
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserDownloadRequest {
            source_url,
            current_url: page.current_url.clone(),
            user_agent,
            cookie_header: build_managed_browser_cookie_header(&cookies),
        },
        page,
    ))
}

pub async fn evaluate_managed_browser(
    session: &ManagedBrowserSession,
    expression: &str,
    return_by_value: bool,
) -> anyhow::Result<ManagedBrowserEvaluationResult> {
    let mut connection = open_cdp_connection(session).await?;
    let result = connection
        .call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": return_by_value,
            }),
        )
        .await?;
    parse_runtime_result(&result)
}

pub async fn refresh_managed_browser(
    session: &ManagedBrowserSession,
) -> anyhow::Result<ManagedBrowserPageState> {
    let mut connection = open_cdp_connection(session).await?;
    capture_page_state(&mut connection).await
}

pub async fn screenshot_managed_browser(
    session: &ManagedBrowserSession,
    output_path: &Path,
    full_page: bool,
) -> anyhow::Result<ManagedBrowserScreenshotResult> {
    let mut connection = open_cdp_connection(session).await?;
    let result = connection
        .call(
            "Page.captureScreenshot",
            json!({
                "format": "png",
                "fromSurface": true,
                "captureBeyondViewport": full_page,
            }),
        )
        .await?;
    let screenshot_base64 = result.get("data").and_then(Value::as_str).ok_or_else(|| {
        anyhow::anyhow!("managed browser screenshot response did not include image data")
    })?;
    let bytes = BASE64_STANDARD
        .decode(screenshot_base64)
        .context("failed to decode managed browser screenshot PNG")?;
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create browser screenshot directory {}",
                parent.display()
            )
        })?;
    }
    tokio::fs::write(output_path, &bytes)
        .await
        .with_context(|| {
            format!(
                "failed to write browser screenshot to {}",
                output_path.display()
            )
        })?;
    Ok(ManagedBrowserScreenshotResult {
        saved_path: output_path.display().to_string(),
        bytes_written: bytes.len(),
        format: "png",
        full_page,
    })
}

pub async fn print_to_pdf_managed_browser(
    session: &ManagedBrowserSession,
    output_path: &Path,
    landscape: bool,
    print_background: bool,
) -> anyhow::Result<ManagedBrowserPdfResult> {
    let mut connection = open_cdp_connection(session).await?;
    let result = connection
        .call(
            "Page.printToPDF",
            json!({
                "landscape": landscape,
                "printBackground": print_background,
                "preferCSSPageSize": true,
            }),
        )
        .await?;
    let pdf_base64 = result
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("managed browser PDF response did not include PDF data"))?;
    let bytes = BASE64_STANDARD
        .decode(pdf_base64)
        .context("failed to decode managed browser PDF")?;
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create browser PDF directory {}",
                parent.display()
            )
        })?;
    }
    tokio::fs::write(output_path, &bytes)
        .await
        .with_context(|| format!("failed to write browser PDF to {}", output_path.display()))?;
    Ok(ManagedBrowserPdfResult {
        saved_path: output_path.display().to_string(),
        bytes_written: bytes.len(),
        format: "pdf",
        landscape,
        print_background,
    })
}

pub async fn wait_for_managed_browser(
    session: &ManagedBrowserSession,
    selector: Option<&str>,
    text: Option<&str>,
    text_gone: Option<&str>,
    timeout: Duration,
    poll_interval: Duration,
) -> anyhow::Result<(ManagedBrowserWaitResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let deadline = Instant::now() + timeout;
    let started = Instant::now();
    loop {
        let state = serde_json::from_value::<ManagedBrowserWaitConditionState>(
            runtime_call(
                &mut connection,
                r#"(selector, text, textGone) => {
                        const bodyText = String(
                            document.body?.innerText ||
                            document.documentElement?.innerText ||
                            ""
                        );
                        const selectorMatched = !selector || Boolean(document.querySelector(selector));
                        const textFound = !text || bodyText.includes(text);
                        const textGoneSatisfied = !textGone || !bodyText.includes(textGone);
                        return {
                            matched: selectorMatched && textFound && textGoneSatisfied,
                            selectorMatched,
                            textFound,
                            textGoneSatisfied,
                            currentUrl: location.href,
                            title: document.title || null,
                        };
                    }"#,
                json!([selector, text, text_gone]),
            )
            .await?,
        )
        .context("failed to parse managed browser wait condition state")?;
        if state.matched {
            let page = capture_page_state(&mut connection).await?;
            return Ok((
                ManagedBrowserWaitResult {
                    selector: selector.map(ToString::to_string),
                    text: text.map(ToString::to_string),
                    text_gone: text_gone.map(ToString::to_string),
                    selector_matched: state.selector_matched,
                    text_found: state.text_found,
                    text_gone_satisfied: state.text_gone_satisfied,
                    elapsed_ms: started.elapsed().as_millis(),
                    current_url: page.current_url.clone(),
                    title: page.title.clone(),
                },
                page,
            ));
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "managed browser wait timed out after {} ms (selectorMatched={}, textFound={}, textGoneSatisfied={}, url={}, title={})",
                started.elapsed().as_millis(),
                state.selector_matched,
                state.text_found,
                state.text_gone_satisfied,
                state.current_url,
                state.title.unwrap_or_default()
            );
        }
        sleep(poll_interval).await;
    }
}

pub async fn handle_managed_browser_dialog(
    session: &ManagedBrowserSession,
    accept: bool,
    prompt_text: Option<&str>,
) -> anyhow::Result<(ManagedBrowserDialogResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let mut params = json!({ "accept": accept });
    if let Some(prompt_text) = prompt_text.map(str::trim).filter(|value| !value.is_empty()) {
        params["promptText"] = Value::String(prompt_text.to_string());
    }
    connection
        .call("Page.handleJavaScriptDialog", params)
        .await
        .context("failed to handle managed browser JavaScript dialog")?;
    wait_for_page_settle(&mut connection).await?;
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserDialogResult {
            accepted: accept,
            prompt_text: prompt_text.map(ToString::to_string),
        },
        page,
    ))
}

pub async fn collect_managed_browser_network_requests(
    session: &ManagedBrowserSession,
    limit: usize,
) -> anyhow::Result<ManagedBrowserNetworkLog> {
    let mut connection = open_cdp_connection(session).await?;
    let limit = limit.max(1);
    let result = runtime_call(
        &mut connection,
        r#"(limit) => {
                const toUnsigned = (value) =>
                    typeof value === "number" && Number.isFinite(value) && value >= 0
                        ? Math.round(value)
                        : null;
                const toTimestamp = (value) =>
                    typeof value === "number" && Number.isFinite(value) && value >= 0
                        ? Math.round(value)
                        : null;
                const timeOrigin =
                    typeof performance.timeOrigin === "number" && Number.isFinite(performance.timeOrigin)
                        ? performance.timeOrigin
                        : Date.now();
                const capturedState = globalThis.__dawnNetworkCaptureState || { requests: [] };
                const capturedEntries = (Array.isArray(capturedState.requests) ? capturedState.requests : [])
                    .map((entry) => ({
                        url: entry.url || null,
                        entryType: entry.entryType || "capture",
                        source: entry.source || "capture",
                        initiatorType: entry.initiatorType || null,
                        method: entry.method || null,
                        contentType:
                            typeof entry.contentType === "string" && entry.contentType
                                ? entry.contentType
                                : null,
                        bodyPreview:
                            typeof entry.bodyPreview === "string" && entry.bodyPreview
                                ? entry.bodyPreview
                                : null,
                        bodyTruncated:
                            typeof entry.bodyTruncated === "boolean" ? entry.bodyTruncated : null,
                        captureError:
                            typeof entry.captureError === "string" && entry.captureError
                                ? entry.captureError
                                : null,
                        durationMs:
                            typeof entry.durationMs === "number" && Number.isFinite(entry.durationMs)
                                ? entry.durationMs
                                : 0,
                        transferSize: toUnsigned(entry.transferSize),
                        encodedBodySize: toUnsigned(entry.encodedBodySize),
                        decodedBodySize: toUnsigned(entry.decodedBodySize),
                        responseStatus:
                            typeof entry.responseStatus === "number" && Number.isFinite(entry.responseStatus)
                                ? Math.round(entry.responseStatus)
                                : null,
                        statusText:
                            typeof entry.statusText === "string" && entry.statusText
                                ? entry.statusText
                                : null,
                        ok: typeof entry.ok === "boolean" ? entry.ok : null,
                        startedAtMs: toTimestamp(entry.startedAtMs),
                        completedAtMs: toTimestamp(entry.completedAtMs),
                        nextHopProtocol:
                            typeof entry.nextHopProtocol === "string" && entry.nextHopProtocol
                                ? entry.nextHopProtocol
                                : null,
                    }))
                    .filter((entry) => Boolean(entry.url));
                const capturedKeys = new Set(
                    capturedEntries.map((entry) => `${entry.initiatorType || entry.entryType}:${entry.url}`)
                );
                const navigationEntries = performance.getEntriesByType("navigation").map((entry) => ({
                    url: entry.name || location.href,
                    entryType: "navigation",
                    source: "performance",
                    initiatorType: "navigation",
                    method: null,
                    contentType: null,
                    bodyPreview: null,
                    bodyTruncated: null,
                    captureError: null,
                    durationMs: Number(entry.duration || 0),
                    transferSize: toUnsigned(entry.transferSize),
                    encodedBodySize: toUnsigned(entry.encodedBodySize),
                    decodedBodySize: toUnsigned(entry.decodedBodySize),
                    responseStatus:
                        typeof entry.responseStatus === "number" && Number.isFinite(entry.responseStatus)
                            ? Math.round(entry.responseStatus)
                            : null,
                    statusText: null,
                    ok: null,
                    startedAtMs: toTimestamp(timeOrigin + Number(entry.startTime || 0)),
                    completedAtMs: toTimestamp(timeOrigin + Number((entry.startTime || 0) + (entry.duration || 0))),
                    nextHopProtocol:
                        typeof entry.nextHopProtocol === "string" && entry.nextHopProtocol
                            ? entry.nextHopProtocol
                            : null,
                }));
                const resourceEntries = performance
                    .getEntriesByType("resource")
                    .map((entry) => ({
                        url: entry.name || null,
                        entryType: "resource",
                        source: "performance",
                        initiatorType:
                            typeof entry.initiatorType === "string" && entry.initiatorType
                                ? entry.initiatorType
                                : null,
                        method: null,
                        contentType: null,
                        bodyPreview: null,
                        bodyTruncated: null,
                        captureError: null,
                        durationMs: Number(entry.duration || 0),
                        transferSize: toUnsigned(entry.transferSize),
                        encodedBodySize: toUnsigned(entry.encodedBodySize),
                        decodedBodySize: toUnsigned(entry.decodedBodySize),
                        responseStatus:
                            typeof entry.responseStatus === "number" && Number.isFinite(entry.responseStatus)
                                ? Math.round(entry.responseStatus)
                                : null,
                        statusText: null,
                        ok: null,
                        startedAtMs: toTimestamp(timeOrigin + Number(entry.startTime || 0)),
                        completedAtMs: toTimestamp(timeOrigin + Number((entry.startTime || 0) + (entry.duration || 0))),
                        nextHopProtocol:
                            typeof entry.nextHopProtocol === "string" && entry.nextHopProtocol
                                ? entry.nextHopProtocol
                                : null,
                    }))
                    .filter((entry) => {
                        if (!entry.url) {
                            return false;
                        }
                        const initiatorKey = `${entry.initiatorType || entry.entryType}:${entry.url}`;
                        return !capturedKeys.has(initiatorKey);
                    });
                const combined = capturedEntries.concat(navigationEntries, resourceEntries);
                combined.sort((left, right) => {
                    const rightStarted = right.startedAtMs || 0;
                    const leftStarted = left.startedAtMs || 0;
                    if (rightStarted !== leftStarted) {
                        return rightStarted - leftStarted;
                    }
                    return right.durationMs - left.durationMs;
                });
                return {
                    currentUrl: location.href,
                    navigationCount: navigationEntries.length,
                    resourceCount: resourceEntries.length + capturedEntries.length,
                    entries: combined.slice(0, limit),
                };
            }"#,
        json!([limit]),
    )
    .await?;
    serde_json::from_value::<ManagedBrowserNetworkLog>(result)
        .context("failed to parse managed browser network log")
}

pub async fn collect_managed_browser_cookies(
    session: &ManagedBrowserSession,
    limit: usize,
) -> anyhow::Result<ManagedBrowserCookieLog> {
    let mut connection = open_cdp_connection(session).await?;
    let current_url = runtime_call(
        &mut connection,
        r#"() => location.href || "about:blank""#,
        json!([]),
    )
    .await?
    .as_str()
    .map(ToString::to_string)
    .unwrap_or_else(|| "about:blank".to_string());
    let params = cookie_query_params(&current_url);
    let cookie_result = connection.call("Network.getCookies", params).await?;
    let mut cookies = serde_json::from_value::<Vec<ManagedBrowserCookie>>(
        cookie_result
            .get("cookies")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .context("failed to decode managed browser cookies")?;
    let count = cookies.len();
    cookies.truncate(limit.max(1));
    Ok(ManagedBrowserCookieLog {
        current_url,
        count,
        cookies,
    })
}

pub async fn inspect_managed_browser_storage(
    session: &ManagedBrowserSession,
    limit: usize,
) -> anyhow::Result<ManagedBrowserStorageSnapshot> {
    let mut connection = open_cdp_connection(session).await?;
    let result = runtime_call(
        &mut connection,
        r#"(limit) => {
                const clamp = Math.max(1, Number(limit || 0));
                const snapshot = (storage) => {
                    try {
                        const keys = [];
                        for (let index = 0; index < storage.length; index += 1) {
                            const key = storage.key(index);
                            if (typeof key === "string") {
                                keys.push(key);
                            }
                        }
                        keys.sort((left, right) => left.localeCompare(right));
                        const entries = keys.slice(0, clamp).map((key) => {
                            const value = String(storage.getItem(key) ?? "");
                            return {
                                key,
                                value,
                                valueLength: value.length,
                            };
                        });
                        return {
                            count: keys.length,
                            entries,
                            error: null,
                        };
                    } catch (error) {
                        return {
                            count: 0,
                            entries: [],
                            error: String(error?.message || error || "storage access failed"),
                        };
                    }
                };
                const local = snapshot(window.localStorage);
                const session = snapshot(window.sessionStorage);
                return {
                    currentUrl: location.href,
                    origin: location.origin || null,
                    localCount: local.count,
                    sessionCount: session.count,
                    localStorage: local.entries,
                    sessionStorage: session.entries,
                    localError: local.error,
                    sessionError: session.error,
                };
            }"#,
        json!([limit.max(1)]),
    )
    .await?;
    serde_json::from_value::<ManagedBrowserStorageSnapshot>(result)
        .context("failed to parse managed browser storage snapshot")
}

pub async fn mutate_managed_browser_storage(
    session: &ManagedBrowserSession,
    storage_area: &str,
    key: &str,
    value: Option<&str>,
    remove: bool,
) -> anyhow::Result<(ManagedBrowserStorageMutationResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let result = runtime_call(
        &mut connection,
        r#"(storageArea, key, value, remove) => {
                const pickStorage = (area) => {
                    if (area === "localStorage") {
                        return window.localStorage;
                    }
                    if (area === "sessionStorage") {
                        return window.sessionStorage;
                    }
                    throw new Error(`unsupported browser storage area ${area}`);
                };
                const storage = pickStorage(storageArea);
                const previousValue = storage.getItem(key);
                if (remove) {
                    storage.removeItem(key);
                } else {
                    storage.setItem(key, String(value ?? ""));
                }
                return {
                    currentUrl: location.href,
                    origin: location.origin || null,
                    storageArea,
                    key,
                    value: remove ? null : String(value ?? ""),
                    removed: Boolean(remove),
                    hadExistingValue: previousValue !== null,
                    previousValue,
                    localCount: window.localStorage.length,
                    sessionCount: window.sessionStorage.length,
                };
            }"#,
        json!([storage_area, key, value, remove]),
    )
    .await?;
    let mutation = serde_json::from_value::<ManagedBrowserStorageMutationResult>(result)
        .context("failed to parse managed browser storage mutation result")?;
    let page = capture_page_state(&mut connection).await?;
    Ok((mutation, page))
}

pub async fn set_managed_browser_headers(
    session: &ManagedBrowserSession,
    headers: &BTreeMap<String, String>,
    reload: bool,
) -> anyhow::Result<(ManagedBrowserHeadersResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let header_map = serde_json::to_value(headers).context("failed to encode browser headers")?;
    connection
        .call(
            "Network.setExtraHTTPHeaders",
            json!({ "headers": header_map }),
        )
        .await
        .context("failed to set managed browser extra HTTP headers")?;
    if reload {
        reload_managed_browser_page(&mut connection).await?;
    }
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserHeadersResult {
            current_url: page.current_url.clone(),
            header_count: headers.len(),
            headers: headers.clone(),
            reloaded: reload,
        },
        page,
    ))
}

pub async fn emulate_managed_browser_network(
    session: &ManagedBrowserSession,
    offline: bool,
    latency_ms: u64,
    download_throughput_kbps: Option<u64>,
    upload_throughput_kbps: Option<u64>,
    reload: bool,
) -> anyhow::Result<(ManagedBrowserOfflineResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let download_throughput = download_throughput_kbps
        .map(|value| (value.saturating_mul(1024) / 8) as i64)
        .unwrap_or(-1);
    let upload_throughput = upload_throughput_kbps
        .map(|value| (value.saturating_mul(1024) / 8) as i64)
        .unwrap_or(-1);
    connection
        .call(
            "Network.emulateNetworkConditions",
            json!({
                "offline": offline,
                "latency": latency_ms,
                "downloadThroughput": download_throughput,
                "uploadThroughput": upload_throughput,
                "connectionType": if offline { "none" } else { "wifi" },
            }),
        )
        .await
        .context("failed to emulate managed browser network conditions")?;
    if reload {
        reload_managed_browser_page(&mut connection).await?;
    }
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserOfflineResult {
            current_url: page.current_url.clone(),
            offline,
            latency_ms,
            download_throughput_kbps,
            upload_throughput_kbps,
            reloaded: reload,
        },
        page,
    ))
}

pub async fn set_managed_browser_geolocation(
    session: &ManagedBrowserSession,
    latitude: f64,
    longitude: f64,
    accuracy: f64,
    reload: bool,
) -> anyhow::Result<(ManagedBrowserGeolocationResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    connection
        .call(
            "Emulation.setGeolocationOverride",
            json!({
                "latitude": latitude,
                "longitude": longitude,
                "accuracy": accuracy,
            }),
        )
        .await
        .context("failed to set managed browser geolocation override")?;
    if reload {
        reload_managed_browser_page(&mut connection).await?;
    }
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserGeolocationResult {
            current_url: page.current_url.clone(),
            latitude,
            longitude,
            accuracy,
            reloaded: reload,
        },
        page,
    ))
}

pub async fn emulate_managed_browser_device(
    session: &ManagedBrowserSession,
    preset: Option<&str>,
    width: u32,
    height: u32,
    device_scale_factor: f64,
    mobile: bool,
    touch: bool,
    user_agent: Option<&str>,
    platform: Option<&str>,
    accept_language: Option<&str>,
    reload: bool,
) -> anyhow::Result<(ManagedBrowserDeviceEmulationResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    connection
        .call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": device_scale_factor,
                "mobile": mobile,
            }),
        )
        .await
        .context("failed to set managed browser device metrics override")?;
    connection
        .call(
            "Emulation.setTouchEmulationEnabled",
            json!({
                "enabled": touch,
                "maxTouchPoints": if touch { 5 } else { 1 },
            }),
        )
        .await
        .context("failed to set managed browser touch emulation")?;
    if user_agent.is_some() || platform.is_some() || accept_language.is_some() {
        let mut payload = json!({});
        if let Some(user_agent) = user_agent {
            payload["userAgent"] = Value::String(user_agent.to_string());
        }
        if let Some(platform) = platform {
            payload["platform"] = Value::String(platform.to_string());
        }
        if let Some(language) = accept_language {
            payload["acceptLanguage"] = Value::String(language.to_string());
        }
        connection
            .call("Network.setUserAgentOverride", payload)
            .await
            .context("failed to set managed browser user-agent override")?;
    }
    if reload {
        reload_managed_browser_page(&mut connection).await?;
    }
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserDeviceEmulationResult {
            current_url: page.current_url.clone(),
            preset: preset.map(ToString::to_string),
            width,
            height,
            device_scale_factor,
            mobile,
            touch,
            user_agent: user_agent.map(ToString::to_string),
            platform: platform.map(ToString::to_string),
            accept_language: accept_language.map(ToString::to_string),
            reloaded: reload,
        },
        page,
    ))
}

pub async fn collect_managed_browser_console_messages(
    session: &ManagedBrowserSession,
    limit: usize,
) -> anyhow::Result<ManagedBrowserConsoleLog> {
    let mut connection = open_cdp_connection(session).await?;
    let limit = limit.max(1);
    let result = runtime_call(
        &mut connection,
        r#"(limit) => {
                const state = globalThis.__dawnConsoleCaptureState || { messages: [] };
                const messages = Array.isArray(state.messages) ? state.messages : [];
                return {
                    currentUrl: location.href,
                    count: messages.length,
                    messages: messages.slice(-limit),
                };
            }"#,
        json!([limit]),
    )
    .await?;
    serde_json::from_value::<ManagedBrowserConsoleLog>(result)
        .context("failed to parse managed browser console log")
}

pub async fn collect_managed_browser_errors(
    session: &ManagedBrowserSession,
    console_limit: usize,
    network_limit: usize,
) -> anyhow::Result<ManagedBrowserErrorLog> {
    let console = collect_managed_browser_console_messages(session, console_limit).await?;
    let network = collect_managed_browser_network_requests(session, network_limit).await?;
    let console_messages = console
        .messages
        .into_iter()
        .filter(|message| {
            message.level.eq_ignore_ascii_case("error")
                || matches!(
                    message.source.as_deref(),
                    Some("window.error") | Some("unhandledrejection")
                )
        })
        .collect::<Vec<_>>();
    let network_entries = network
        .entries
        .into_iter()
        .filter(|entry| {
            entry.ok == Some(false)
                || entry
                    .response_status
                    .map(|status| status >= 400)
                    .unwrap_or(false)
                || entry.capture_error.is_some()
        })
        .collect::<Vec<_>>();
    Ok(ManagedBrowserErrorLog {
        current_url: console.current_url,
        console_count: console_messages.len(),
        network_count: network_entries.len(),
        console_messages,
        network_entries,
    })
}

pub async fn collect_managed_browser_trace(
    session: &ManagedBrowserSession,
    limit: usize,
) -> anyhow::Result<ManagedBrowserTraceLog> {
    let console = collect_managed_browser_console_messages(session, limit).await?;
    let mut connection = open_cdp_connection(session).await?;
    let limit = limit.max(1);
    let network_events = runtime_call(
        &mut connection,
        r#"(limit) => {
                const state = globalThis.__dawnNetworkCaptureState || { events: [] };
                const events = Array.isArray(state.events) ? state.events : [];
                return {
                    currentUrl: location.href,
                    events: events.slice(-Math.max(1, Number(limit || 0))),
                };
            }"#,
        json!([limit]),
    )
    .await?;
    let current_url = network_events
        .get("currentUrl")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| console.current_url.clone());
    let mut events = serde_json::from_value::<Vec<ManagedBrowserTraceEvent>>(
        network_events
            .get("events")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .context("failed to parse managed browser trace events")?;
    events.extend(
        console
            .messages
            .into_iter()
            .map(|message| ManagedBrowserTraceEvent {
                timestamp_ms: message.timestamp_ms.unwrap_or_default(),
                source: "console".to_string(),
                event_type: "console".to_string(),
                phase: None,
                level: Some(message.level),
                url: message.url,
                method: None,
                status: None,
                text: Some(message.text),
                content_type: None,
            }),
    );
    events.sort_by(|left, right| {
        right
            .timestamp_ms
            .cmp(&left.timestamp_ms)
            .then_with(|| left.source.cmp(&right.source))
    });
    let count = events.len();
    events.truncate(limit);
    Ok(ManagedBrowserTraceLog {
        current_url,
        count,
        events,
    })
}

pub async fn press_key_managed_browser(
    session: &ManagedBrowserSession,
    key_spec: &str,
) -> anyhow::Result<(ManagedBrowserKeyPressResult, ManagedBrowserPageState)> {
    let mut connection = open_cdp_connection(session).await?;
    let parsed = parse_managed_browser_key_spec(key_spec)?;
    let _ = connection.call("Page.bringToFront", json!({})).await;
    dispatch_managed_browser_key_event(&mut connection, &parsed).await?;
    wait_for_page_settle(&mut connection).await?;
    let active = serde_json::from_value::<ManagedBrowserActiveElement>(
        runtime_call(
            &mut connection,
            r#"() => {
                    const element = document.activeElement || document.body || document.documentElement;
                    return {
                        tag: (element?.tagName || "").toLowerCase(),
                        fieldType: element?.getAttribute ? (element.getAttribute("type") || null) : null,
                        name: element?.getAttribute
                            ? (element.getAttribute("name") || element.getAttribute("id") || null)
                            : null,
                    };
                }"#,
            json!([]),
        )
        .await?,
    )
    .context("failed to parse managed browser active element state")?;
    let page = capture_page_state(&mut connection).await?;
    Ok((
        ManagedBrowserKeyPressResult {
            key: parsed.key,
            code: parsed.code,
            modifiers: parsed.modifier_labels,
            text: parsed.text,
            active_tag: active.tag,
            active_type: active.field_type,
            active_name: active.name,
        },
        page,
    ))
}

async fn open_cdp_connection(session: &ManagedBrowserSession) -> anyhow::Result<CdpConnection> {
    let websocket_url = if session.websocket_url.is_empty() {
        resolve_target_websocket_url(session).await?
    } else {
        session.websocket_url.clone()
    };
    let mut connection = CdpConnection::connect(&websocket_url).await?;
    enable_cdp_page_domains(&mut connection).await?;
    Ok(connection)
}

async fn enable_cdp_page_domains(connection: &mut CdpConnection) -> anyhow::Result<()> {
    connection.call("Page.enable", json!({})).await?;
    connection.call("Runtime.enable", json!({})).await?;
    connection.call("DOM.enable", json!({})).await?;
    connection.call("Network.enable", json!({})).await?;
    connection
        .call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": MANAGED_BROWSER_CONSOLE_CAPTURE_SOURCE }),
        )
        .await?;
    connection
        .call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": MANAGED_BROWSER_NETWORK_CAPTURE_SOURCE }),
        )
        .await?;
    connection
        .call(
            "Runtime.evaluate",
            json!({
                "expression": MANAGED_BROWSER_CONSOLE_CAPTURE_SOURCE,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )
        .await
        .and_then(|result| parse_runtime_result(&result).map(|_| result))?;
    connection
        .call(
            "Runtime.evaluate",
            json!({
                "expression": MANAGED_BROWSER_NETWORK_CAPTURE_SOURCE,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )
        .await
        .and_then(|result| parse_runtime_result(&result).map(|_| result))?;
    Ok(())
}

async fn navigate_with_connection(connection: &mut CdpConnection, url: &str) -> anyhow::Result<()> {
    connection
        .call("Page.navigate", json!({ "url": url }))
        .await
        .with_context(|| format!("failed to navigate managed browser to {url}"))?;
    wait_for_page_settle(connection).await
}

async fn reload_managed_browser_page(connection: &mut CdpConnection) -> anyhow::Result<()> {
    connection
        .call("Page.reload", json!({ "ignoreCache": false }))
        .await
        .context("failed to reload managed browser page")?;
    wait_for_page_settle(connection).await
}

async fn wait_for_page_settle(connection: &mut CdpConnection) -> anyhow::Result<()> {
    let deadline = Instant::now() + MANAGED_BROWSER_READY_TIMEOUT;
    loop {
        let result = connection
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": "document.readyState",
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;
        let ready_state = result
            .get("result")
            .and_then(Value::as_object)
            .and_then(|entry| entry.get("value"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(ready_state, "interactive" | "complete") {
            sleep(Duration::from_millis(250)).await;
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("managed browser page did not become ready before timeout");
        }
        sleep(Duration::from_millis(200)).await;
    }
}

async fn capture_page_state(
    connection: &mut CdpConnection,
) -> anyhow::Result<ManagedBrowserPageState> {
    let result = runtime_call(
        connection,
        r#"() => {
            const documentElement = document.documentElement;
            if (!documentElement) {
                return {
                    currentUrl: location.href,
                    title: document.title || null,
                    readyState: document.readyState || null,
                    html: "",
                };
            }

            const clone = documentElement.cloneNode(true);
            const synchronize = (source, target) => {
                if (!source || !target || source.nodeType !== Node.ELEMENT_NODE || target.nodeType !== Node.ELEMENT_NODE) {
                    return;
                }
                const tag = source.tagName;
                if (tag === "INPUT") {
                    const inputType = (source.getAttribute("type") || "text").toLowerCase();
                    if (inputType === "checkbox" || inputType === "radio") {
                        if (source.checked) {
                            target.setAttribute("checked", "");
                        } else {
                            target.removeAttribute("checked");
                        }
                    } else if (inputType === "file") {
                        target.setAttribute("data-dawn-file-count", String(source.files ? source.files.length : 0));
                    } else {
                        target.setAttribute("value", source.value || "");
                    }
                } else if (tag === "TEXTAREA") {
                    target.textContent = source.value || "";
                } else if (tag === "SELECT") {
                    const sourceOptions = Array.from(source.options || []);
                    const targetOptions = Array.from(target.options || []);
                    for (let index = 0; index < sourceOptions.length; index += 1) {
                        const sourceOption = sourceOptions[index];
                        const targetOption = targetOptions[index];
                        if (!targetOption) {
                            continue;
                        }
                        if (sourceOption.selected) {
                            targetOption.setAttribute("selected", "");
                        } else {
                            targetOption.removeAttribute("selected");
                        }
                    }
                }

                const sourceChildren = Array.from(source.children || []);
                const targetChildren = Array.from(target.children || []);
                for (let index = 0; index < sourceChildren.length; index += 1) {
                    synchronize(sourceChildren[index], targetChildren[index]);
                }
            };

            synchronize(documentElement, clone);

            return {
                currentUrl: location.href,
                title: document.title || null,
                readyState: document.readyState || null,
                html: "<!DOCTYPE html>\n" + clone.outerHTML,
            };
        }"#,
        json!([]),
    )
    .await?;

    let current_url = result
        .get("currentUrl")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = result
        .get("title")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let ready_state = result
        .get("readyState")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let html = result
        .get("html")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(MANAGED_BROWSER_HTML_BYTES)
        .collect::<String>();

    Ok(ManagedBrowserPageState {
        current_url,
        title,
        html,
        ready_state,
    })
}

async fn runtime_call(
    connection: &mut CdpConnection,
    function_declaration: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let args_source = arguments
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|value| {
            serde_json::to_string(&value).context("failed to serialize managed browser JS argument")
        })
        .collect::<anyhow::Result<Vec<_>>>()?
        .join(", ");
    let expression = format!("({function_declaration})({args_source})");
    let result = connection
        .call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )
        .await?;
    let evaluation = parse_runtime_result(&result)?;
    Ok(evaluation.value)
}

fn parse_runtime_result(result: &Value) -> anyhow::Result<ManagedBrowserEvaluationResult> {
    if let Some(exception) = result.get("exceptionDetails") {
        let description = exception
            .get("exception")
            .and_then(Value::as_object)
            .and_then(|value| value.get("description"))
            .and_then(Value::as_str)
            .or_else(|| exception.get("text").and_then(Value::as_str))
            .unwrap_or("unknown managed browser JavaScript error");
        anyhow::bail!("{description}");
    }
    let runtime_result = result
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("managed browser runtime result missing `result` field"))?;
    let value = runtime_result.get("value").cloned().unwrap_or(Value::Null);
    Ok(ManagedBrowserEvaluationResult {
        value,
        value_type: runtime_result
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("undefined")
            .to_string(),
        subtype: runtime_result
            .get("subtype")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        description: runtime_result
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

async fn resolve_target_websocket_url(session: &ManagedBrowserSession) -> anyhow::Result<String> {
    let client = managed_browser_http_client()?;
    let targets = list_browser_targets(&client, session.debug_port).await?;
    let target = targets
        .into_iter()
        .find(|candidate| candidate.id == session.target_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "managed browser target {} no longer exists",
                session.target_id
            )
        })?;
    if target.web_socket_debugger_url.is_empty() {
        anyhow::bail!(
            "managed browser target {} did not expose a websocket debugger URL",
            session.target_id
        );
    }
    Ok(target.web_socket_debugger_url)
}

async fn resolve_browser_websocket_url(client: &Client, debug_port: u16) -> anyhow::Result<String> {
    let version = client
        .get(format!("http://127.0.0.1:{debug_port}/json/version"))
        .send()
        .await
        .with_context(|| format!("failed to query managed browser version on port {debug_port}"))?
        .error_for_status()
        .context("managed browser version endpoint returned an error")?
        .json::<BrowserVersionInfo>()
        .await
        .context("failed to decode managed browser version payload")?;
    if version.web_socket_debugger_url.trim().is_empty() {
        anyhow::bail!("managed browser version payload did not expose a browser websocket URL");
    }
    Ok(version.web_socket_debugger_url)
}

async fn wait_for_page_target(
    client: &Client,
    debug_port: u16,
) -> anyhow::Result<BrowserTargetInfo> {
    let deadline = Instant::now() + MANAGED_BROWSER_READY_TIMEOUT;
    loop {
        let targets = list_browser_targets(client, debug_port).await?;
        if let Some(target) = targets.into_iter().find(|candidate| {
            candidate.r#type == "page" && !candidate.web_socket_debugger_url.is_empty()
        }) {
            return Ok(target);
        }
        if Instant::now() >= deadline {
            anyhow::bail!("managed browser did not expose a debuggable page target before timeout");
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_for_page_target_by_id(
    client: &Client,
    debug_port: u16,
    target_id: &str,
) -> anyhow::Result<BrowserTargetInfo> {
    let deadline = Instant::now() + MANAGED_BROWSER_READY_TIMEOUT;
    loop {
        let targets = list_browser_targets(client, debug_port).await?;
        if let Some(target) = targets.into_iter().find(|candidate| {
            candidate.id == target_id
                && candidate.r#type == "page"
                && !candidate.web_socket_debugger_url.is_empty()
        }) {
            return Ok(target);
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "managed browser target {target_id} did not expose a debuggable page before timeout"
            );
        }
        sleep(Duration::from_millis(200)).await;
    }
}

async fn list_browser_targets(
    client: &Client,
    debug_port: u16,
) -> anyhow::Result<Vec<BrowserTargetInfo>> {
    client
        .get(format!("http://127.0.0.1:{debug_port}/json/list"))
        .send()
        .await
        .with_context(|| format!("failed to query managed browser targets on port {debug_port}"))?
        .error_for_status()
        .context("managed browser target listing returned an error")?
        .json::<Vec<BrowserTargetInfo>>()
        .await
        .context("failed to decode managed browser target list")
}

fn managed_browser_http_client() -> anyhow::Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build managed browser HTTP client")
}

fn reserve_debug_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .context("failed to reserve a debug port for the managed browser")?;
    let port = listener
        .local_addr()
        .context("failed to read managed browser debug port")?
        .port();
    drop(listener);
    Ok(port)
}

async fn spawn_managed_browser_process(
    executable: &str,
    debug_port: u16,
    user_data_dir: &Path,
) -> anyhow::Result<Option<u32>> {
    let mut command = Command::new(executable);
    command
        .arg(format!("--remote-debugging-port={debug_port}"))
        .arg(format!("--user-data-dir={}", user_data_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--new-window")
        .arg("about:blank")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = command
        .spawn()
        .with_context(|| format!("failed to launch managed browser executable {executable}"))?;
    Ok(child.id())
}

async fn terminate_browser_process(pid: u32) -> anyhow::Result<()> {
    let output = if cfg!(target_os = "windows") {
        Command::new("taskkill")
            .arg("/PID")
            .arg(pid.to_string())
            .arg("/T")
            .arg("/F")
            .output()
            .await
            .context("failed to launch taskkill for managed browser cleanup")?
    } else {
        Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output()
            .await
            .context("failed to launch kill for managed browser cleanup")?
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
        if combined.contains("no running instance")
            || combined.contains("not found")
            || combined.contains("no such process")
        {
            return Ok(());
        }
        anyhow::bail!("failed to terminate managed browser process {pid}");
    }
    Ok(())
}

fn managed_browser_profile_root() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("DawnNode")
                .join("managed-browser-profiles");
        }
    } else if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".dawn")
            .join("managed-browser-profiles");
    }
    env::temp_dir().join("dawn-managed-browser-profiles")
}

fn sanitize_managed_browser_profile_name(profile_name: &str) -> String {
    let sanitized = profile_name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn resolve_managed_browser_user_data_dir(
    profile_name: Option<&str>,
    persistent_profile: bool,
    debug_port: u16,
) -> PathBuf {
    if persistent_profile {
        let profile_name = profile_name
            .filter(|value| !value.trim().is_empty())
            .map(sanitize_managed_browser_profile_name)
            .unwrap_or_else(|| "default".to_string());
        return managed_browser_profile_root().join(profile_name);
    }
    let prefix = profile_name
        .filter(|value| !value.trim().is_empty())
        .map(sanitize_managed_browser_profile_name)
        .unwrap_or_else(|| "session".to_string());
    env::temp_dir().join(format!(
        "dawn-managed-browser-{}-{}-{}",
        prefix,
        debug_port,
        unix_timestamp_ms()
    ))
}

fn collect_managed_browser_profile_stats(
    path: &Path,
    stats: &mut ManagedBrowserProfileStats,
) -> anyhow::Result<()> {
    let metadata = std::fs::metadata(path).with_context(|| {
        format!(
            "failed to stat managed browser profile path {}",
            path.display()
        )
    })?;
    if metadata.is_dir() {
        stats.directory_count += 1;
        update_managed_browser_profile_modified(stats, metadata.modified().ok());
        for entry in std::fs::read_dir(path).with_context(|| {
            format!(
                "failed to read managed browser profile path {}",
                path.display()
            )
        })? {
            let entry = entry.with_context(|| {
                format!(
                    "failed to read one managed browser profile entry in {}",
                    path.display()
                )
            })?;
            collect_managed_browser_profile_stats(&entry.path(), stats)?;
        }
        return Ok(());
    }
    stats.file_count += 1;
    stats.total_bytes = stats.total_bytes.saturating_add(metadata.len());
    update_managed_browser_profile_modified(stats, metadata.modified().ok());
    Ok(())
}

fn update_managed_browser_profile_modified(
    stats: &mut ManagedBrowserProfileStats,
    modified: Option<std::time::SystemTime>,
) {
    let modified_ms = modified
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_millis());
    if let Some(candidate) = modified_ms {
        stats.last_modified_unix_ms = Some(
            stats
                .last_modified_unix_ms
                .map(|current| current.max(candidate))
                .unwrap_or(candidate),
        );
    }
}

fn copy_managed_browser_profile_dir(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination).with_context(|| {
        format!(
            "failed to create managed browser export directory {}",
            destination.display()
        )
    })?;
    for entry in std::fs::read_dir(source).with_context(|| {
        format!(
            "failed to read managed browser profile path {}",
            source.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed to read one managed browser profile entry in {}",
                source.display()
            )
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", source_path.display()))?;
        if metadata.is_dir() {
            copy_managed_browser_profile_dir(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed to create managed browser export directory {}",
                        parent.display()
                    )
                })?;
            }
            std::fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy managed browser profile file {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

async fn remove_managed_browser_profile_dir(path: &Path) -> anyhow::Result<()> {
    let mut last_error = None;
    for attempt in 0..5 {
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt < 4 {
                    sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }
    let error = last_error.expect("expected managed browser profile removal error");
    Err(error).with_context(|| {
        format!(
            "failed to remove managed browser profile directory {}",
            path.display()
        )
    })
}

fn detect_browser_executable() -> anyhow::Result<String> {
    if let Ok(explicit) = env::var("DAWN_MANAGED_BROWSER_BIN") {
        let explicit = explicit.trim();
        if !explicit.is_empty() {
            return Ok(explicit.to_string());
        }
    }

    for candidate in managed_browser_candidates() {
        if Path::new(&candidate).is_file() {
            return Ok(candidate);
        }
    }

    if cfg!(target_os = "windows") {
        for binary in ["msedge.exe", "chrome.exe"] {
            if let Some(path) = lookup_executable_path(binary) {
                return Ok(path);
            }
        }
    } else {
        for binary in [
            "microsoft-edge",
            "google-chrome",
            "chromium",
            "chromium-browser",
        ] {
            if let Some(path) = lookup_executable_path(binary) {
                return Ok(path);
            }
        }
    }

    anyhow::bail!(
        "failed to find a Chromium-based browser for managed browser mode; set DAWN_MANAGED_BROWSER_BIN to Edge, Chrome, or Chromium"
    )
}

fn managed_browser_candidates() -> Vec<String> {
    if cfg!(target_os = "windows") {
        let mut candidates = Vec::new();
        for variable in ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
            if let Ok(root) = env::var(variable) {
                candidates.push(format!(r"{root}\Microsoft\Edge\Application\msedge.exe"));
                candidates.push(format!(r"{root}\Google\Chrome\Application\chrome.exe"));
                candidates.push(format!(r"{root}\Chromium\Application\chrome.exe"));
            }
        }
        return candidates;
    }
    if cfg!(target_os = "macos") {
        return vec![
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".to_string(),
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string(),
            "/Applications/Chromium.app/Contents/MacOS/Chromium".to_string(),
        ];
    }
    vec![
        "/usr/bin/microsoft-edge".to_string(),
        "/usr/bin/google-chrome".to_string(),
        "/usr/bin/chromium".to_string(),
        "/usr/bin/chromium-browser".to_string(),
        "/snap/bin/chromium".to_string(),
    ]
}

fn lookup_executable_path(binary: &str) -> Option<String> {
    let lookup_command = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let output = std::process::Command::new(lookup_command)
        .arg(binary)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

fn browser_backend_label(executable: &str) -> String {
    let lowercase = executable.to_ascii_lowercase();
    if lowercase.contains("edge") {
        "edge".to_string()
    } else if lowercase.contains("chrome") {
        "chrome".to_string()
    } else if lowercase.contains("chromium") {
        "chromium".to_string()
    } else {
        Path::new(executable)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("chromium")
            .to_ascii_lowercase()
    }
}

fn validate_managed_browser_download_source(source_url: &str) -> anyhow::Result<String> {
    let parsed = Url::parse(source_url)
        .with_context(|| format!("failed to parse managed browser download URL {source_url}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        other => anyhow::bail!("browser_download only supports http/https URLs, got `{other}`"),
    }
}

fn build_managed_browser_cookie_header(cookies: &[ManagedBrowserCookie]) -> Option<String> {
    let header = cookies
        .iter()
        .filter_map(|cookie| {
            let name = cookie.name.trim();
            if name.is_empty() {
                None
            } else {
                Some(format!("{name}={}", cookie.value))
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    if header.is_empty() {
        None
    } else {
        Some(header)
    }
}

fn cookie_query_params(current_url: &str) -> Value {
    match Url::parse(current_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => {
            json!({ "urls": [url.as_str()] })
        }
        _ => json!({}),
    }
}

fn unix_timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn resolve_dom_node_id(
    connection: &mut CdpConnection,
    selector: &str,
) -> anyhow::Result<i32> {
    let root_id = connection
        .call("DOM.getDocument", json!({ "depth": 0 }))
        .await?
        .get("root")
        .and_then(Value::as_object)
        .and_then(|value| value.get("nodeId"))
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("managed browser DOM root did not include nodeId"))?;
    let node_id = connection
        .call(
            "DOM.querySelector",
            json!({
                "nodeId": root_id,
                "selector": selector,
            }),
        )
        .await?
        .get("nodeId")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if node_id <= 0 {
        anyhow::bail!("no element matched selector {selector}");
    }
    i32::try_from(node_id).context("managed browser DOM node id exceeded i32 range")
}

async fn dispatch_managed_browser_key_event(
    connection: &mut CdpConnection,
    key_spec: &ManagedBrowserKeySpec,
) -> anyhow::Result<()> {
    let down_type = if key_spec.text.is_some() {
        "keyDown"
    } else {
        "rawKeyDown"
    };
    let mut down_payload = json!({
        "type": down_type,
        "modifiers": key_spec.modifiers_mask,
        "key": key_spec.key,
        "code": key_spec.code,
        "windowsVirtualKeyCode": key_spec.windows_virtual_key_code,
        "nativeVirtualKeyCode": key_spec.windows_virtual_key_code,
    });
    if let Some(text) = key_spec.text.as_ref() {
        down_payload["text"] = json!(text);
        down_payload["unmodifiedText"] = json!(text);
    }
    connection
        .call("Input.dispatchKeyEvent", down_payload)
        .await
        .context("failed to dispatch managed browser keyDown event")?;
    connection
        .call(
            "Input.dispatchKeyEvent",
            json!({
                "type": "keyUp",
                "modifiers": key_spec.modifiers_mask,
                "key": key_spec.key,
                "code": key_spec.code,
                "windowsVirtualKeyCode": key_spec.windows_virtual_key_code,
                "nativeVirtualKeyCode": key_spec.windows_virtual_key_code,
            }),
        )
        .await
        .context("failed to dispatch managed browser keyUp event")?;
    Ok(())
}

fn parse_managed_browser_key_spec(key_spec: &str) -> anyhow::Result<ManagedBrowserKeySpec> {
    let tokens = key_spec
        .split('+')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        anyhow::bail!("browser_press_key requires a non-empty key specification");
    }
    let key_token = *tokens
        .last()
        .ok_or_else(|| anyhow::anyhow!("browser_press_key requires a non-empty key"))?;
    let mut modifiers_mask = 0;
    let mut modifier_labels = Vec::new();
    for token in &tokens[..tokens.len().saturating_sub(1)] {
        match token.to_ascii_lowercase().as_str() {
            "alt" | "option" => {
                if modifiers_mask & 1 == 0 {
                    modifiers_mask |= 1;
                    modifier_labels.push("Alt".to_string());
                }
            }
            "ctrl" | "control" => {
                if modifiers_mask & 2 == 0 {
                    modifiers_mask |= 2;
                    modifier_labels.push("Control".to_string());
                }
            }
            "cmd" | "command" | "meta" | "super" | "win" => {
                if modifiers_mask & 4 == 0 {
                    modifiers_mask |= 4;
                    modifier_labels.push("Meta".to_string());
                }
            }
            "shift" => {
                if modifiers_mask & 8 == 0 {
                    modifiers_mask |= 8;
                    modifier_labels.push("Shift".to_string());
                }
            }
            other => anyhow::bail!("unsupported managed browser modifier `{other}`"),
        }
    }
    let allow_text = modifiers_mask & 0b111 == 0;
    let shift = modifiers_mask & 8 != 0;
    let normalized = key_token.to_ascii_lowercase();
    let key = match normalized.as_str() {
        "enter" | "return" => ManagedBrowserKeySpec {
            key: "Enter".to_string(),
            code: "Enter".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 13,
            text: allow_text.then(|| "\r".to_string()),
        },
        "tab" => ManagedBrowserKeySpec {
            key: "Tab".to_string(),
            code: "Tab".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 9,
            text: None,
        },
        "escape" | "esc" => ManagedBrowserKeySpec {
            key: "Escape".to_string(),
            code: "Escape".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 27,
            text: None,
        },
        "space" | "spacebar" => ManagedBrowserKeySpec {
            key: " ".to_string(),
            code: "Space".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 32,
            text: allow_text.then(|| " ".to_string()),
        },
        "backspace" => ManagedBrowserKeySpec {
            key: "Backspace".to_string(),
            code: "Backspace".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 8,
            text: None,
        },
        "delete" | "del" => ManagedBrowserKeySpec {
            key: "Delete".to_string(),
            code: "Delete".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 46,
            text: None,
        },
        "home" => ManagedBrowserKeySpec {
            key: "Home".to_string(),
            code: "Home".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 36,
            text: None,
        },
        "end" => ManagedBrowserKeySpec {
            key: "End".to_string(),
            code: "End".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 35,
            text: None,
        },
        "pageup" => ManagedBrowserKeySpec {
            key: "PageUp".to_string(),
            code: "PageUp".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 33,
            text: None,
        },
        "pagedown" => ManagedBrowserKeySpec {
            key: "PageDown".to_string(),
            code: "PageDown".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 34,
            text: None,
        },
        "arrowup" | "up" => ManagedBrowserKeySpec {
            key: "ArrowUp".to_string(),
            code: "ArrowUp".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 38,
            text: None,
        },
        "arrowdown" | "down" => ManagedBrowserKeySpec {
            key: "ArrowDown".to_string(),
            code: "ArrowDown".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 40,
            text: None,
        },
        "arrowleft" | "left" => ManagedBrowserKeySpec {
            key: "ArrowLeft".to_string(),
            code: "ArrowLeft".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 37,
            text: None,
        },
        "arrowright" | "right" => ManagedBrowserKeySpec {
            key: "ArrowRight".to_string(),
            code: "ArrowRight".to_string(),
            modifiers_mask,
            modifier_labels,
            windows_virtual_key_code: 39,
            text: None,
        },
        _ => {
            if normalized.starts_with('f') {
                if let Ok(index) = normalized[1..].parse::<i32>() {
                    if (1..=12).contains(&index) {
                        return Ok(ManagedBrowserKeySpec {
                            key: format!("F{index}"),
                            code: format!("F{index}"),
                            modifiers_mask,
                            modifier_labels,
                            windows_virtual_key_code: 111 + index,
                            text: None,
                        });
                    }
                }
            }
            if key_token.chars().count() == 1 {
                let character = key_token.chars().next().unwrap_or_default();
                if character.is_ascii_alphabetic() {
                    let upper = character.to_ascii_uppercase();
                    let lower = character.to_ascii_lowercase();
                    return Ok(ManagedBrowserKeySpec {
                        key: if shift {
                            upper.to_string()
                        } else {
                            lower.to_string()
                        },
                        code: format!("Key{upper}"),
                        modifiers_mask,
                        modifier_labels,
                        windows_virtual_key_code: upper as i32,
                        text: allow_text.then(|| {
                            if shift {
                                upper.to_string()
                            } else {
                                lower.to_string()
                            }
                        }),
                    });
                }
                if character.is_ascii_digit() {
                    return Ok(ManagedBrowserKeySpec {
                        key: character.to_string(),
                        code: format!("Digit{character}"),
                        modifiers_mask,
                        modifier_labels,
                        windows_virtual_key_code: character as i32,
                        text: allow_text.then(|| character.to_string()),
                    });
                }
            }
            anyhow::bail!("unsupported managed browser key specification `{key_spec}`")
        }
    };
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::{
        ManagedBrowserCookie, build_managed_browser_cookie_header, close_managed_browser,
        collect_managed_browser_trace, cookie_query_params, delete_managed_browser_profile,
        evaluate_managed_browser, export_managed_browser_profile, import_managed_browser_profile,
        inspect_managed_browser_profile, launch_managed_browser, parse_managed_browser_key_spec,
        resolve_managed_browser_user_data_dir, sanitize_managed_browser_profile_name,
        validate_managed_browser_download_source,
    };
    use serde_json::json;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parses_printable_managed_browser_key_specs() {
        let parsed = parse_managed_browser_key_spec("a").expect("expected key spec to parse");
        assert_eq!(parsed.key, "a");
        assert_eq!(parsed.code, "KeyA");
        assert_eq!(parsed.text.as_deref(), Some("a"));
    }

    #[test]
    fn parses_shortcut_managed_browser_key_specs() {
        let parsed = parse_managed_browser_key_spec("Control+L")
            .expect("expected shortcut key spec to parse");
        assert_eq!(parsed.key, "l");
        assert_eq!(parsed.code, "KeyL");
        assert_eq!(parsed.modifiers_mask, 2);
        assert!(parsed.text.is_none());
    }

    #[test]
    fn rejects_unknown_managed_browser_key_specs() {
        let error = parse_managed_browser_key_spec("Control+Plus")
            .expect_err("expected invalid key spec to fail");
        assert!(
            error
                .to_string()
                .contains("unsupported managed browser key")
        );
    }

    #[test]
    fn builds_managed_browser_cookie_header() {
        let header = build_managed_browser_cookie_header(&[
            ManagedBrowserCookie {
                name: "sid".to_string(),
                value: "abc".to_string(),
                ..Default::default()
            },
            ManagedBrowserCookie {
                name: "theme".to_string(),
                value: "dark".to_string(),
                ..Default::default()
            },
        ]);
        assert_eq!(header.as_deref(), Some("sid=abc; theme=dark"));
    }

    #[test]
    fn builds_cookie_query_params_for_http_pages() {
        assert_eq!(
            cookie_query_params("https://example.com/app"),
            json!({ "urls": ["https://example.com/app"] })
        );
        assert_eq!(cookie_query_params("about:blank"), json!({}));
    }

    #[test]
    fn rejects_non_http_managed_browser_download_urls() {
        let error = validate_managed_browser_download_source("file:///tmp/demo.txt")
            .expect_err("expected non-http managed download URL to fail");
        assert!(error.to_string().contains("only supports http/https"));
    }

    #[test]
    fn sanitizes_managed_browser_profile_names() {
        assert_eq!(
            sanitize_managed_browser_profile_name("  Sales Team / QA  "),
            "Sales-Team---QA"
        );
        assert_eq!(sanitize_managed_browser_profile_name(""), "default");
    }

    #[test]
    fn resolves_persistent_managed_browser_profile_dir() {
        let path = resolve_managed_browser_user_data_dir(Some("ops"), true, 9222);
        assert!(
            path.to_string_lossy().contains("managed-browser-profiles"),
            "expected persistent profile root in {}",
            path.display()
        );
        assert_eq!(
            path.file_name()
                .map(|value| value.to_string_lossy().to_string()),
            Some("ops".to_string())
        );
    }

    #[tokio::test]
    async fn deletes_managed_browser_profile_dir() {
        let profile_name = format!(
            "test-profile-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let path = resolve_managed_browser_user_data_dir(Some(&profile_name), true, 9222);
        std::fs::create_dir_all(&path).expect("expected profile directory to be created");
        std::fs::write(path.join("Preferences"), "{}").expect("expected profile file to write");
        let deleted = delete_managed_browser_profile(&profile_name)
            .await
            .expect("expected profile deletion to succeed");
        assert_eq!(deleted.profile_name, profile_name);
        assert!(!path.exists(), "expected profile directory to be removed");
    }

    #[test]
    fn inspects_managed_browser_profile_dir() {
        let profile_name = format!(
            "inspect-profile-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let path = resolve_managed_browser_user_data_dir(Some(&profile_name), true, 9222);
        std::fs::create_dir_all(path.join("Default")).expect("expected nested profile dir");
        std::fs::write(path.join("Preferences"), "{}").expect("expected root file");
        std::fs::write(path.join("Default").join("Cookies"), "sqlite")
            .expect("expected nested file");
        let inspected = inspect_managed_browser_profile(&profile_name)
            .expect("expected profile inspect to succeed");
        assert_eq!(inspected.profile_name, profile_name);
        assert!(inspected.directory_count >= 2);
        assert!(inspected.file_count >= 2);
        assert!(inspected.total_bytes >= 8);
        std::fs::remove_dir_all(path).expect("expected test profile cleanup");
    }

    #[test]
    fn exports_managed_browser_profile_dir() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let profile_name = format!("export-profile-{suffix}");
        let export_name = format!("export-copy-{suffix}");
        let path = resolve_managed_browser_user_data_dir(Some(&profile_name), true, 9222);
        let export_root = env::temp_dir().join(export_name);
        std::fs::create_dir_all(path.join("Default")).expect("expected nested profile dir");
        std::fs::write(path.join("Preferences"), "{\"a\":1}").expect("expected root file");
        std::fs::write(path.join("Default").join("Cookies"), "sqlite")
            .expect("expected nested file");
        let exported = export_managed_browser_profile(&profile_name, &export_root)
            .expect("expected profile export to succeed");
        assert_eq!(exported.profile_name, profile_name);
        assert!(export_root.join("Preferences").exists());
        assert!(export_root.join("Default").join("Cookies").exists());
        std::fs::remove_dir_all(path).expect("expected test profile cleanup");
        std::fs::remove_dir_all(export_root).expect("expected export cleanup");
    }

    #[tokio::test]
    async fn imports_managed_browser_profile_dir() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let source_root = env::temp_dir().join(format!("profile-import-source-{suffix}"));
        let profile_name = format!("import-profile-{suffix}");
        std::fs::create_dir_all(source_root.join("Default")).expect("expected source dir");
        std::fs::write(source_root.join("Preferences"), "{\"b\":2}").expect("expected source file");
        std::fs::write(source_root.join("Default").join("Cookies"), "sqlite")
            .expect("expected nested source file");
        let imported = import_managed_browser_profile(&source_root, &profile_name, false)
            .await
            .expect("expected profile import to succeed");
        let imported_root = resolve_managed_browser_user_data_dir(Some(&profile_name), true, 9222);
        assert_eq!(imported.profile_name, profile_name);
        assert!(imported_root.join("Preferences").exists());
        assert!(imported_root.join("Default").join("Cookies").exists());
        std::fs::remove_dir_all(source_root).expect("expected source cleanup");
        std::fs::remove_dir_all(imported_root).expect("expected imported cleanup");
    }

    #[tokio::test]
    #[ignore = "live network/browser smoke against https://example.com"]
    async fn live_managed_browser_trace_smoke() {
        let artifact_path = env::temp_dir().join(format!(
            "dawn-live-browser-trace-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        let (session, _page) = launch_managed_browser("https://example.com")
            .await
            .expect("expected managed browser to launch");
        let trace_result = async {
            let fetch_result = evaluate_managed_browser(
                &session,
                "fetch(location.href, { cache: 'no-store' }).then(async (response) => ({ status: response.status, length: (await response.text()).length }))",
                true,
            )
            .await?;
            anyhow::ensure!(
                fetch_result
                    .value
                    .get("status")
                    .and_then(|value| value.as_u64())
                    == Some(200),
                "expected live fetch to return HTTP 200, got {:?}",
                fetch_result.value
            );
            let trace = collect_managed_browser_trace(&session, 20).await?;
            let encoded = serde_json::to_vec_pretty(&trace)?;
            std::fs::write(&artifact_path, &encoded)?;
            Ok::<_, anyhow::Error>(trace)
        }
        .await;
        let close_result = close_managed_browser(&session, true).await;
        if let Err(error) = close_result {
            panic!("expected managed browser cleanup to succeed: {error}");
        }
        let trace = trace_result.expect("expected managed browser trace to succeed");
        println!(
            "live managed browser trace artifact: {} ({} events)",
            artifact_path.display(),
            trace.count
        );
        assert!(
            trace.events.iter().any(|event| {
                event.source == "network"
                    && event
                        .method
                        .as_deref()
                        .map(|method| method.eq_ignore_ascii_case("GET"))
                        .unwrap_or(false)
            }),
            "expected at least one captured network event in {:?}",
            artifact_path
        );
    }
}
