use std::sync::Arc;

use std::{convert::Infallible, time::Duration};

use axum::{
    Router,
    extract::State,
    response::{
        Html,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use futures_util::stream;

use crate::app_state::{AppState, ConsoleStreamEvent, unix_timestamp_ms};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(dashboard))
        .route("/events", get(console_event_stream))
}

async fn console_event_stream(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let initial = ConsoleStreamEvent {
        channel: "console".to_string(),
        entity_id: None,
        status: Some("connected".to_string()),
        detail: "live event stream connected".to_string(),
        created_at_unix_ms: unix_timestamp_ms(),
    };
    let stream = stream::unfold((Some(initial), state.subscribe_console_events()), |(pending, mut receiver)| async move {
        if let Some(event) = pending {
            let payload = serde_json::to_string(&event).unwrap_or_else(|_| "{\"channel\":\"console\",\"detail\":\"serialization_error\"}".to_string());
            return Some((Ok(Event::default().event("console_update").data(payload)), (None, receiver)));
        }

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let payload = serde_json::to_string(&event).unwrap_or_else(|_| "{\"channel\":\"console\",\"detail\":\"serialization_error\"}".to_string());
                    return Some((Ok(Event::default().event("console_update").data(payload)), (None, receiver)));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let payload = serde_json::to_string(&ConsoleStreamEvent {
                        channel: "console".to_string(),
                        entity_id: None,
                        status: Some("lagged".to_string()),
                        detail: format!("skipped {skipped} console updates"),
                        created_at_unix_ms: unix_timestamp_ms(),
                    })
                    .unwrap_or_else(|_| "{\"channel\":\"console\",\"detail\":\"lagged\"}".to_string());
                    return Some((Ok(Event::default().event("console_update").data(payload)), (None, receiver)));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}

async fn dashboard() -> Html<&'static str> {
    Html(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Dawn Control Center</title>
  <style>
    :root {
      --bg: #08111a;
      --panel: rgba(18, 29, 44, 0.58);
      --panel-strong: rgba(20, 34, 52, 0.76);
      --panel-soft: rgba(255, 255, 255, 0.05);
      --border: rgba(162, 208, 233, 0.2);
      --border-strong: rgba(233, 247, 255, 0.22);
      --text: #eef6fb;
      --muted: #9bb5c7;
      --accent: #ffd77b;
      --accent-2: #69d6ff;
      --accent-3: #8ff7ca;
      --danger: #ff8266;
      --success: #64d2a3;
      --shadow: 0 32px 120px rgba(2, 10, 19, 0.45);
      --shadow-soft: 0 18px 55px rgba(2, 10, 19, 0.25);
      --radius: 24px;
      --font: "Aptos", "Segoe UI Variable Text", "Microsoft YaHei UI", sans-serif;
      --font-display: "Bahnschrift", "Aptos Display", "Segoe UI Variable Display", sans-serif;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      font-family: var(--font);
      color: var(--text);
      background:
        radial-gradient(circle at 12% 18%, rgba(105, 214, 255, 0.32), transparent 24%),
        radial-gradient(circle at 84% 12%, rgba(255, 215, 123, 0.24), transparent 22%),
        radial-gradient(circle at 78% 78%, rgba(143, 247, 202, 0.16), transparent 28%),
        linear-gradient(150deg, #06111a 0%, #09141f 24%, #0c1724 58%, #0a121b 100%);
      overflow-x: hidden;
    }
    body::before,
    body::after {
      content: "";
      position: fixed;
      inset: auto;
      width: 38vw;
      height: 38vw;
      border-radius: 50%;
      filter: blur(36px);
      opacity: 0.38;
      pointer-events: none;
      z-index: 0;
      animation: drift 18s ease-in-out infinite;
    }
    body::before {
      top: -8vw;
      left: -10vw;
      background: radial-gradient(circle, rgba(102, 205, 255, 0.48), transparent 62%);
    }
    body::after {
      right: -10vw;
      bottom: -14vw;
      background: radial-gradient(circle, rgba(255, 215, 123, 0.28), transparent 58%);
      animation-delay: -7s;
    }
    .shell {
      max-width: 1480px;
      margin: 0 auto;
      padding: 30px;
      position: relative;
      z-index: 1;
    }
    .shell::before {
      content: "";
      position: fixed;
      inset: 0;
      background-image:
        linear-gradient(rgba(255,255,255,0.03) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255,255,255,0.03) 1px, transparent 1px);
      background-size: 110px 110px;
      mask-image: radial-gradient(circle at center, black 42%, transparent 82%);
      pointer-events: none;
      opacity: 0.45;
    }
    .hero {
      display: grid;
      grid-template-columns: 1.2fr 0.8fr;
      gap: 18px;
      margin-bottom: 22px;
    }
    .hero-card, .panel {
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: var(--radius);
      box-shadow: var(--shadow);
      backdrop-filter: blur(28px) saturate(145%);
      -webkit-backdrop-filter: blur(28px) saturate(145%);
      position: relative;
      overflow: hidden;
    }
    .hero-card::before,
    .panel::before {
      content: "";
      position: absolute;
      inset: 1px 1px auto 1px;
      height: 38%;
      border-radius: calc(var(--radius) - 1px);
      background: linear-gradient(180deg, rgba(255,255,255,0.16), rgba(255,255,255,0.02) 58%, transparent 100%);
      pointer-events: none;
    }
    .hero-card::after,
    .panel::after {
      content: "";
      position: absolute;
      inset: auto -16% -26% 44%;
      height: 180px;
      background: radial-gradient(circle, rgba(105, 214, 255, 0.16), transparent 68%);
      pointer-events: none;
    }
    .hero-card {
      padding: 28px;
      min-height: 230px;
    }
    .eyebrow {
      letter-spacing: 0.18em;
      text-transform: uppercase;
      font-size: 11px;
      color: var(--accent-2);
      margin-bottom: 12px;
    }
    h1 {
      margin: 0 0 10px 0;
      font-family: var(--font-display);
      font-size: clamp(36px, 4.6vw, 60px);
      line-height: 0.9;
      max-width: 11ch;
      letter-spacing: -0.05em;
    }
    .subcopy {
      color: var(--muted);
      max-width: 64ch;
      line-height: 1.7;
      margin-bottom: 20px;
    }
    .hero-meta {
      display: flex;
      gap: 12px;
      flex-wrap: wrap;
    }
    .pill {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 10px 14px;
      border-radius: 999px;
      background: linear-gradient(180deg, rgba(255,255,255,0.1), rgba(255,255,255,0.04));
      border: 1px solid rgba(255, 255, 255, 0.12);
      color: #d4e6f1;
      font-size: 13px;
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.12);
    }
    .stats {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(132px, 1fr));
      gap: 12px;
      padding: 22px;
      align-content: end;
    }
    .stat {
      background: linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.02));
      border: 1px solid rgba(255,255,255,0.12);
      border-radius: 18px;
      padding: 18px;
      box-shadow: var(--shadow-soft);
    }
    .stat-label {
      color: var(--muted);
      font-size: 12px;
      margin-bottom: 8px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }
    .stat-value {
      font-family: var(--font-display);
      font-size: 36px;
      font-weight: 700;
      letter-spacing: -0.03em;
    }
    .layout {
      display: grid;
      grid-template-columns: 1.2fr 0.8fr;
      gap: 18px;
    }
    .stack {
      display: grid;
      gap: 18px;
    }
    .panel {
      padding: 18px;
    }
    .panel-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-bottom: 14px;
    }
    .panel h2 {
      margin: 0;
      font-family: var(--font-display);
      font-size: 19px;
      letter-spacing: -0.03em;
    }
    .tiny {
      color: var(--muted);
      font-size: 12px;
    }
    .hero-grid {
      display: grid;
      grid-template-columns: 1.4fr 0.9fr;
      gap: 18px;
    }
    .signal-cluster {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 10px;
      margin-top: 22px;
    }
    .signal {
      padding: 12px 14px;
      border-radius: 18px;
      background: linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.03));
      border: 1px solid rgba(255,255,255,0.1);
    }
    .signal span {
      display: block;
      color: var(--muted);
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      margin-bottom: 8px;
    }
    .signal strong {
      font-family: var(--font-display);
      font-size: 18px;
      letter-spacing: -0.03em;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      font-size: 13px;
    }
    th, td {
      text-align: left;
      padding: 10px 8px;
      border-bottom: 1px solid rgba(255, 255, 255, 0.06);
      vertical-align: top;
    }
    th {
      color: var(--muted);
      font-weight: 500;
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }
    .status {
      display: inline-flex;
      align-items: center;
      padding: 5px 10px;
      border-radius: 999px;
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      border: 1px solid transparent;
    }
    .status.ok { color: #092217; background: rgba(100, 210, 163, 0.92); }
    .status.warn { color: #281d02; background: rgba(232, 185, 73, 0.92); }
    .status.bad { color: #2b0c06; background: rgba(255, 130, 102, 0.92); }
    .feed {
      display: grid;
      gap: 10px;
    }
    .feed-item {
      padding: 14px;
      border-radius: 16px;
      background: linear-gradient(180deg, rgba(255,255,255,0.07), rgba(255,255,255,0.03));
      border: 1px solid rgba(255, 255, 255, 0.08);
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
    }
    .feed-item strong {
      display: block;
      margin-bottom: 6px;
    }
    .feed-item p {
      margin: 0;
      color: var(--muted);
      line-height: 1.5;
    }
    .clickable-card {
      cursor: pointer;
      transition: transform 180ms ease, background 180ms ease, border-color 180ms ease;
    }
    .clickable-card:hover {
      transform: translateY(-1px);
      background: linear-gradient(180deg, rgba(255,255,255,0.1), rgba(255,255,255,0.04));
      border-color: rgba(255,255,255,0.14);
    }
    .approval-actions {
      display: flex;
      gap: 10px;
      margin-top: 12px;
      flex-wrap: wrap;
    }
    .console-form {
      display: grid;
      gap: 12px;
    }
    .form-grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 12px;
    }
    .field {
      display: grid;
      gap: 6px;
    }
    .field label {
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }
    input, select, textarea {
      width: 100%;
      border-radius: 18px;
      border: 1px solid rgba(255, 255, 255, 0.12);
      background: rgba(255, 255, 255, 0.07);
      color: var(--text);
      font: inherit;
      padding: 12px 14px;
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
    }
    textarea {
      min-height: 132px;
      resize: vertical;
      font-family: ui-monospace, "Cascadia Code", monospace;
      font-size: 12px;
      line-height: 1.5;
    }
    .toolbar {
      display: flex;
      gap: 10px;
      align-items: center;
      flex-wrap: wrap;
    }
    .chip-row {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }
    .chip {
      padding: 8px 12px;
      border-radius: 999px;
      border: 1px solid rgba(255,255,255,0.1);
      background: rgba(255,255,255,0.05);
      color: var(--text);
      font-size: 12px;
      cursor: pointer;
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.06);
    }
    .result-box {
      padding: 14px;
      border-radius: 18px;
      background: linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.03));
      border: 1px solid rgba(255, 255, 255, 0.08);
      color: var(--muted);
      line-height: 1.6;
      min-height: 52px;
    }
    .detail-card {
      padding: 18px;
      border-radius: 18px;
      background: linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.03));
      border: 1px solid rgba(255,255,255,0.1);
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
    }
    .detail-grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
      margin-bottom: 14px;
    }
    .detail-metric {
      padding: 12px 14px;
      border-radius: 16px;
      background: rgba(255,255,255,0.05);
      border: 1px solid rgba(255,255,255,0.08);
    }
    .detail-metric span {
      display: block;
      color: var(--muted);
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      margin-bottom: 6px;
    }
    .detail-metric strong {
      font-size: 14px;
      color: var(--text);
      word-break: break-word;
    }
    .detail-pre {
      margin: 0;
      padding: 16px;
      border-radius: 18px;
      background: rgba(4, 10, 18, 0.54);
      border: 1px solid rgba(255,255,255,0.08);
      color: #d8eef7;
      overflow: auto;
      max-height: 360px;
      font-family: ui-monospace, "Cascadia Code", monospace;
      font-size: 12px;
      line-height: 1.6;
      white-space: pre-wrap;
      word-break: break-word;
    }
    .command-row {
      cursor: pointer;
      transition: background 160ms ease, transform 160ms ease;
    }
    .command-row:hover {
      background: rgba(255,255,255,0.04);
    }
    .command-row.active {
      background: rgba(105,214,255,0.1);
    }
    .interactive-row {
      cursor: pointer;
      transition: background 160ms ease, transform 160ms ease;
    }
    .interactive-row:hover {
      background: rgba(255,255,255,0.04);
    }
    .drawer-backdrop {
      position: fixed;
      inset: 0;
      background: rgba(4, 10, 18, 0.45);
      backdrop-filter: blur(10px);
      -webkit-backdrop-filter: blur(10px);
      opacity: 0;
      pointer-events: none;
      transition: opacity 220ms ease;
      z-index: 20;
    }
    .drawer-backdrop.open {
      opacity: 1;
      pointer-events: auto;
    }
    .detail-drawer {
      position: fixed;
      top: 18px;
      right: 18px;
      bottom: 18px;
      width: min(460px, calc(100vw - 24px));
      padding: 18px;
      border-radius: 28px;
      background: linear-gradient(180deg, rgba(15, 27, 42, 0.9), rgba(13, 22, 34, 0.82));
      border: 1px solid rgba(255,255,255,0.12);
      box-shadow: 0 38px 120px rgba(0,0,0,0.42);
      backdrop-filter: blur(30px) saturate(150%);
      -webkit-backdrop-filter: blur(30px) saturate(150%);
      transform: translateX(calc(100% + 30px));
      transition: transform 260ms ease;
      z-index: 21;
      display: grid;
      grid-template-rows: auto auto auto auto auto 1fr;
      gap: 14px;
      overflow: hidden;
    }
    .detail-drawer.open {
      transform: translateX(0);
    }
    .drawer-head {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 12px;
    }
    .drawer-title {
      margin: 0;
      font-family: var(--font-display);
      font-size: 24px;
      letter-spacing: -0.04em;
    }
    .drawer-subtitle {
      color: var(--muted);
      font-size: 13px;
      line-height: 1.5;
      margin-top: 6px;
    }
    .icon-button {
      width: 38px;
      height: 38px;
      border-radius: 999px;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      padding: 0;
      background: rgba(255,255,255,0.08);
      color: var(--text);
      box-shadow: none;
    }
    .drawer-meta {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
    }
    .drawer-actions {
      display: flex;
      gap: 10px;
      flex-wrap: wrap;
    }
    .drawer-form {
      display: none;
      gap: 12px;
      padding: 14px;
      border-radius: 22px;
      background: linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.03));
      border: 1px solid rgba(255,255,255,0.1);
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
      overflow: auto;
    }
    .drawer-form.open {
      display: grid;
    }
    .drawer-note {
      padding: 12px 14px;
      border-radius: 18px;
      background: rgba(255,255,255,0.05);
      border: 1px solid rgba(255,255,255,0.08);
      color: var(--muted);
      line-height: 1.6;
      font-size: 13px;
    }
    .drawer-status {
      display: none;
    }
    .drawer-status.open {
      display: block;
    }
    .catalog-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
      gap: 12px;
    }
    .catalog-card {
      padding: 16px;
      border-radius: 20px;
      background: linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.03));
      border: 1px solid rgba(255,255,255,0.1);
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
      display: grid;
      gap: 10px;
    }
    .catalog-card strong {
      font-size: 15px;
      letter-spacing: -0.02em;
      display: block;
    }
    .catalog-meta {
      color: var(--muted);
      font-size: 12px;
      line-height: 1.6;
    }
    .catalog-tags {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }
    .catalog-tag {
      padding: 6px 10px;
      border-radius: 999px;
      background: rgba(255,255,255,0.06);
      border: 1px solid rgba(255,255,255,0.08);
      color: #d5ebf5;
      font-size: 11px;
      line-height: 1;
    }
    .catalog-actions {
      display: flex;
      gap: 10px;
      flex-wrap: wrap;
    }
    .setup-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
      gap: 12px;
    }
    .setup-card {
      padding: 16px;
      border-radius: 20px;
      background: linear-gradient(180deg, rgba(255,255,255,0.09), rgba(255,255,255,0.03));
      border: 1px solid rgba(255,255,255,0.1);
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
    }
    .setup-card span {
      display: block;
      color: var(--muted);
      font-size: 11px;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      margin-bottom: 8px;
    }
    .setup-card strong {
      display: block;
      font-family: var(--font-display);
      font-size: 24px;
      letter-spacing: -0.03em;
      margin-bottom: 6px;
    }
    .setup-card p {
      margin: 0;
      color: var(--muted);
      font-size: 12px;
      line-height: 1.6;
    }
    .setup-requirements {
      display: grid;
      gap: 10px;
    }
    .setup-requirement {
      padding: 12px 14px;
      border-radius: 18px;
      background: rgba(255,255,255,0.05);
      border: 1px solid rgba(255,255,255,0.08);
    }
    .setup-requirement strong {
      display: block;
      font-size: 13px;
      letter-spacing: -0.01em;
      margin-bottom: 6px;
    }
    .setup-requirement p {
      margin: 0;
      color: var(--muted);
      font-size: 12px;
      line-height: 1.6;
    }
    .toolbar input[type="search"] {
      min-width: 220px;
      flex: 1 1 240px;
    }
    .toolbar select {
      min-width: 140px;
    }
    button {
      border: 0;
      border-radius: 999px;
      padding: 9px 14px;
      font: inherit;
      font-size: 12px;
      font-weight: 600;
      letter-spacing: 0.04em;
      cursor: pointer;
      color: #081018;
      background: linear-gradient(180deg, #ffe39e, #ffcf67);
      box-shadow: 0 10px 24px rgba(255, 207, 103, 0.22);
    }
    button.secondary {
      color: var(--text);
      background: rgba(255, 255, 255, 0.08);
      border: 1px solid rgba(255, 255, 255, 0.08);
      box-shadow: none;
    }
    code {
      font-family: ui-monospace, "Cascadia Code", monospace;
      color: #ffe39e;
      font-size: 12px;
    }
    @keyframes drift {
      0%, 100% { transform: translate3d(0, 0, 0) scale(1); }
      50% { transform: translate3d(4vw, 2vw, 0) scale(1.08); }
    }
    @media (max-width: 1080px) {
      .hero, .layout { grid-template-columns: 1fr; }
      .hero-grid, .detail-grid, .form-grid { grid-template-columns: 1fr; }
      .detail-drawer {
        top: auto;
        left: 12px;
        right: 12px;
        bottom: 12px;
        width: auto;
        max-height: 78vh;
        transform: translateY(calc(100% + 30px));
      }
      .detail-drawer.open {
        transform: translateY(0);
      }
      .drawer-meta {
        grid-template-columns: 1fr;
      }
    }
  </style>
</head>
<body>
  <div class="shell">
    <section class="hero">
      <div class="hero-card">
        <div class="eyebrow">Dawn Gateway</div>
        <h1>Control Center</h1>
        <div class="subcopy">
          Liquid-glass operations deck for inbound chat, A2A execution, node trust, AP2 settlement, and agent-to-agent commerce.
        </div>
        <div class="hero-meta">
          <span class="pill">A2A-native routing</span>
          <span class="pill">AP2-aware payments</span>
          <span class="pill">Node attestation</span>
          <span class="pill">China connector path</span>
          <span class="pill" id="console-stream-pill">Console stream · connecting</span>
        </div>
        <div class="signal-cluster">
          <div class="signal">
            <span>Ops Mode</span>
            <strong>Attested Runtime</strong>
          </div>
          <div class="signal">
            <span>Control Plane</span>
            <strong>Live Dispatch</strong>
          </div>
          <div class="signal">
            <span>Design Tone</span>
            <strong>Liquid Glass</strong>
          </div>
        </div>
      </div>
      <div class="hero-card">
        <div class="eyebrow">Gateway Pulse</div>
        <div class="stats" id="stats"></div>
      </div>
    </section>

    <section class="layout">
      <div class="stack">
        <section class="panel">
          <div class="panel-head">
            <h2>Inbound Chat Feed</h2>
            <span class="tiny">Latest ingress events</span>
          </div>
          <div class="feed" id="ingress-feed"></div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Approval Center</h2>
            <span class="tiny">Pending node and AP2 approvals</span>
          </div>
          <div class="feed" id="approval-feed"></div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Recent Tasks</h2>
            <span class="tiny">A2A and inbound-created tasks</span>
          </div>
          <table>
            <thead><tr><th>Name</th><th>Status</th><th>Instruction</th></tr></thead>
            <tbody id="task-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Remote Invocations</h2>
            <span class="tiny">Agent-card execution calls and their remote task state</span>
          </div>
          <table>
            <thead><tr><th>Card</th><th>Status</th><th>Remote Task</th></tr></thead>
            <tbody id="invocation-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Quote Ledger</h2>
            <span class="tiny">Signed quote rounds, revocations, and replay state</span>
          </div>
          <table>
            <thead><tr><th>Quote</th><th>Status</th><th>Amount</th></tr></thead>
            <tbody id="quote-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Agent Delegation Studio</h2>
            <span class="tiny">Preview quotes, counter-offer, and invoke remote agent cards</span>
          </div>
          <div class="console-form">
            <div class="form-grid">
              <div class="field">
                <label for="delegate-card-id">Target Agent Card</label>
                <select id="delegate-card-id"></select>
              </div>
              <div class="field">
                <label for="delegate-name">Invocation Name</label>
                <input id="delegate-name" type="text" value="delegate task" />
              </div>
            </div>
            <div class="field">
              <label for="delegate-instruction">Instruction</label>
              <textarea id="delegate-instruction">Coordinate a remote agent action.</textarea>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="delegate-amount">Settlement Amount</label>
                <input id="delegate-amount" type="number" step="0.01" min="0" placeholder="18.50" />
              </div>
              <div class="field">
                <label for="delegate-quote-id">Quote Id</label>
                <input id="delegate-quote-id" type="text" placeholder="quote-..." />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="delegate-counter-offer">Counter Offer</label>
                <input id="delegate-counter-offer" type="number" step="0.01" min="0" placeholder="15.00" />
              </div>
              <div class="field">
                <label for="delegate-await-completion">Await Completion</label>
                <select id="delegate-await-completion">
                  <option value="true" selected>Wait for remote completion</option>
                  <option value="false">Return after dispatch</option>
                </select>
              </div>
            </div>
            <div class="field">
              <label for="delegate-description">Settlement Description</label>
              <input id="delegate-description" type="text" value="Settle remote delegation" />
            </div>
            <div class="toolbar">
              <button type="button" onclick="previewSettlementQuote(false)">Preview Quote</button>
              <button type="button" class="secondary" onclick="previewSettlementQuote(true)">Counter Offer</button>
              <button type="button" class="secondary" onclick="invokeAgentCard()">Invoke Agent</button>
            </div>
            <div class="result-box" id="delegate-status">Select an agent card to preview or invoke a remote delegation.</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Marketplace Studio</h2>
            <span class="tiny">Install skill packages, import remote cards, and publish local agent cards</span>
          </div>
          <div class="console-form">
            <div class="result-box" id="marketplace-signal">Marketplace catalog is loading.</div>
            <div class="form-grid">
              <div class="field">
                <label for="marketplace-skill-package-url">Skill Package URL</label>
                <input id="marketplace-skill-package-url" type="url" placeholder="https://gateway.example/api/gateway/skills/echo-skill/1.0.0/package" />
              </div>
              <div class="field">
                <label for="marketplace-card-url">Remote Agent Card URL</label>
                <input id="marketplace-card-url" type="url" placeholder="https://agent.example/.well-known/agent-card.json" />
              </div>
            </div>
            <div class="toolbar">
              <button type="button" onclick="installSkillPackage()">Install Skill</button>
              <button type="button" class="secondary" onclick="importAgentCardFromUrl()">Import Card</button>
              <button type="button" class="secondary" onclick="window.open('/marketplace', '_blank')">Open Marketplace</button>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-id">Local Card Id</label>
                <input id="publish-card-id" type="text" placeholder="travel-agent-cn" />
              </div>
              <div class="field">
                <label for="publish-card-regions">Regions</label>
                <input id="publish-card-regions" type="text" value="china" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-languages">Languages</label>
                <input id="publish-card-languages" type="text" value="zh-cn,en" />
              </div>
              <div class="field">
                <label for="publish-card-model-providers">Model Providers</label>
                <input id="publish-card-model-providers" type="text" value="qwen,deepseek" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-chat-platforms">Chat Platforms</label>
                <input id="publish-card-chat-platforms" type="text" value="wechat_official_account,feishu,dingtalk" />
              </div>
              <div class="field">
                <label for="publish-card-payment-roles">Payment Roles</label>
                <input id="publish-card-payment-roles" type="text" value="payee" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-local">Hosted Here</label>
                <select id="publish-card-local">
                  <option value="true" selected>Locally hosted</option>
                  <option value="false">Remote metadata only</option>
                </select>
              </div>
              <div class="field">
                <label for="publish-card-published">Visibility</label>
                <select id="publish-card-published">
                  <option value="true" selected>Published</option>
                  <option value="false">Draft</option>
                </select>
              </div>
            </div>
            <div class="field">
              <label for="publish-card-json">Agent Card JSON</label>
              <textarea id="publish-card-json">{
  "name": "Dawn China Travel Agent",
  "description": "Coordinates bookings, messaging, and AP2 settlement for China-market tasks.",
  "url": "https://example.com/a2a",
  "version": "1.0.0",
  "defaultInputModes": ["text"],
  "defaultOutputModes": ["text"],
  "capabilities": {
    "streaming": true,
    "pushNotifications": true,
    "stateTransitionHistory": true,
    "extensions": []
  },
  "authentication": {
    "schemes": ["none"]
  },
  "skills": [
    {
      "id": "travel-plan",
      "name": "Travel Planning",
      "description": "Plans and delegates itinerary actions."
    }
  ]
}</textarea>
            </div>
            <div class="toolbar">
              <button type="button" onclick="publishAgentCard()">Publish Agent Card</button>
            </div>
            <div class="result-box" id="marketplace-status">Install, import, or publish marketplace assets from here.</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Marketplace Catalog</h2>
            <span class="tiny">Browse signed skills and published agents without leaving the ops deck</span>
          </div>
          <div class="console-form">
            <div class="toolbar">
              <input id="catalog-search" type="search" placeholder="Search skills, agents, capabilities, platforms" />
              <select id="catalog-kind">
                <option value="">All</option>
                <option value="skill">Skills</option>
                <option value="agent">Agents</option>
              </select>
              <button type="button" class="secondary" onclick="loadMarketplaceCatalog()">Search Catalog</button>
            </div>
            <div class="result-box" id="marketplace-catalog-status">Catalog browser is loading.</div>
            <div class="catalog-grid" id="marketplace-catalog-grid"></div>
          </div>
        </section>
      </div>

      <div class="stack">
        <section class="panel">
          <div class="panel-head">
            <h2>Nodes</h2>
            <span class="tiny">Session and attestation state</span>
          </div>
          <table>
            <thead><tr><th>Node</th><th>Status</th><th>Trust</th></tr></thead>
            <tbody id="node-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Rollout Fabric</h2>
            <span class="tiny">Latest signed policy and skill distribution state per node</span>
          </div>
          <table>
            <thead><tr><th>Node</th><th>Status</th><th>Policy / Skill</th></tr></thead>
            <tbody id="rollout-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Rollout Console</h2>
            <span class="tiny">Manually dispatch the current signed policy and skill bundle</span>
          </div>
          <div class="console-form">
            <div class="field">
              <label for="rollout-node-id">Target Node</label>
              <select id="rollout-node-id"></select>
            </div>
            <div class="toolbar">
              <button type="button" onclick="dispatchManualRollout()">Dispatch Rollout</button>
              <button type="button" class="secondary" onclick="refresh(document.getElementById('rollout-node-id')?.value)">Refresh Nodes</button>
            </div>
            <div class="result-box" id="rollout-status">Select a node to push the current rollout bundle.</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Node Command Console</h2>
            <span class="tiny">Dispatch attested file, process, and discovery commands</span>
          </div>
          <div class="console-form">
            <div class="field">
              <label for="node-command-node">Target Node</label>
              <select id="node-command-node"></select>
            </div>
            <div class="field">
              <label for="node-command-type">Command Template</label>
              <select id="node-command-type"></select>
            </div>
            <div class="chip-row" id="command-template-chips"></div>
            <div class="field">
              <label for="node-command-payload">Payload JSON</label>
              <textarea id="node-command-payload"></textarea>
            </div>
            <div class="toolbar">
              <button type="button" onclick="dispatchNodeCommand()">Dispatch Command</button>
              <button type="button" class="secondary" onclick="applyCommandTemplate(true)">Reset Payload</button>
              <span class="tiny">`shell_exec` still follows approval and policy gates.</span>
            </div>
            <div class="result-box" id="command-status">Select a node and dispatch a command.</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Recent Node Commands</h2>
            <span class="tiny">Latest execution results for the selected node</span>
          </div>
          <table>
            <thead><tr><th>Command</th><th>Status</th><th>Payload / Result</th></tr></thead>
            <tbody id="command-rows"></tbody>
          </table>
          <div class="detail-card" style="margin-top:14px;">
            <div class="panel-head" style="margin-bottom:12px;">
              <h2>Command Detail</h2>
              <span class="tiny" id="command-detail-hint">Select a command row to inspect the full result.</span>
            </div>
            <div class="detail-grid" id="command-detail-grid"></div>
            <pre class="detail-pre" id="command-detail-pre">No command selected.</pre>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Settlements</h2>
            <span class="tiny">Remote agent settlement activity</span>
          </div>
          <table>
            <thead><tr><th>Card</th><th>Status</th><th>Amount</th></tr></thead>
            <tbody id="settlement-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Reconciliation Fabric</h2>
            <span class="tiny">Cross-gateway settlement receipt delivery and acknowledgment</span>
          </div>
          <table>
            <thead><tr><th>Settlement</th><th>Status</th><th>Counterparty</th></tr></thead>
            <tbody id="reconciliation-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Agent Cards</h2>
            <span class="tiny">Registry footprint</span>
          </div>
          <table>
            <thead><tr><th>Card</th><th>Hosted</th><th>Signals</th></tr></thead>
            <tbody id="card-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Connector Matrix</h2>
            <span class="tiny">Model, chat, and ingress readiness across global and China paths</span>
          </div>
          <div class="detail-grid" id="connector-summary-grid"></div>
          <div class="feed" id="connector-matrix" style="margin-top:14px;"></div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Setup Navigator</h2>
            <span class="tiny">Turn connector and ingress readiness into a concrete go-live checklist</span>
          </div>
          <div class="console-form">
            <div class="setup-grid" id="setup-summary-grid"></div>
            <div class="chip-row" id="setup-preset-chips"></div>
            <div class="form-grid">
              <div class="field">
                <label for="setup-surface">Surface</label>
                <select id="setup-surface">
                  <option value="model">Model Connector</option>
                  <option value="chat">Chat Connector</option>
                  <option value="ingress">Ingress Route</option>
                </select>
              </div>
              <div class="field">
                <label for="setup-target">Target</label>
                <select id="setup-target"></select>
              </div>
            </div>
            <div class="result-box" id="setup-navigator-status">Choose a surface to see required secrets, test routes, and next steps.</div>
            <div class="setup-requirements" id="setup-requirements"></div>
            <div class="field">
              <label for="setup-env-block">Suggested Environment Block</label>
              <textarea id="setup-env-block" readonly></textarea>
            </div>
            <div class="toolbar">
              <button type="button" onclick="copySetupEnvBlock()">Copy Env Block</button>
              <button type="button" class="secondary" onclick="loadSetupPreset('china')">China Go-Live</button>
              <button type="button" class="secondary" onclick="loadSetupPreset('global')">Global MVP</button>
              <button type="button" class="secondary" onclick="loadSetupPreset('ingress')">Ingress First</button>
            </div>
          </div>
        </section>
      </div>
    </section>
  </div>
  <div class="drawer-backdrop" id="detail-drawer-backdrop" onclick="closeDetailDrawer()"></div>
  <aside class="detail-drawer" id="detail-drawer">
    <div class="drawer-head">
      <div>
        <div class="eyebrow" id="detail-drawer-eyebrow">Inspector</div>
        <h2 class="drawer-title" id="detail-drawer-title">Detail Drawer</h2>
        <div class="drawer-subtitle" id="detail-drawer-subtitle">Select an approval, command, settlement, or reconciliation record.</div>
      </div>
      <button type="button" class="icon-button" onclick="closeDetailDrawer()">×</button>
    </div>
    <div class="drawer-meta" id="detail-drawer-meta"></div>
    <div class="drawer-actions" id="detail-drawer-actions"></div>
    <div class="drawer-form" id="detail-drawer-form"></div>
    <div class="result-box drawer-status" id="detail-drawer-status"></div>
    <pre class="detail-pre" id="detail-drawer-pre">Nothing selected.</pre>
  </aside>

  <script>
    const fmt = (value) => value ?? "—";
    let selectedCommandId = null;
    let visibleCommandsCache = [];
    let detailDrawerState = null;
    let marketplaceCatalogCache = { skills: [], agentCards: [] };
    let consoleEventSource = null;
    let consoleRefreshTimer = null;
    let consoleReconnectTimer = null;
    let latestConnectorStatus = null;
    let latestIngressStatus = null;
    const commandTemplates = {
      agent_ping: {},
      list_capabilities: {},
      system_info: {},
      process_snapshot: { limit: 20 },
      list_directory: { path: ".", limit: 20 },
      read_file_preview: { path: "./README.md", maxBytes: 512 },
      stat_path: { path: "." },
      shell_exec: { command: "echo dawn node" }
    };
    const commandTemplateDescriptions = {
      agent_ping: "Fast liveness probe",
      list_capabilities: "Attested capability list",
      system_info: "OS, host, and runtime profile",
      process_snapshot: "Bounded process inventory",
      list_directory: "Directory listing with metadata",
      read_file_preview: "Safe bounded file preview",
      stat_path: "Filesystem metadata",
      shell_exec: "Policy-gated shell execution"
    };
    const setupProfiles = {
      model: {
        openai: {
          label: "OpenAI",
          region: "global",
          mode: "live",
          envs: ["OPENAI_API_KEY"],
          endpoint: "/api/gateway/connectors/model/openai/respond",
          note: "Fastest path for global reasoning and responses."
        },
        deepseek: {
          label: "DeepSeek",
          region: "china",
          mode: "live",
          envs: ["DEEPSEEK_API_KEY"],
          endpoint: "/api/gateway/connectors/model/deepseek/respond",
          note: "Primary China-market reasoning provider."
        },
        qwen: {
          label: "Qwen",
          region: "china",
          mode: "live_openai_compatible",
          envs: ["QWEN_API_KEY or DASHSCOPE_API_KEY"],
          endpoint: "/api/gateway/connectors/model/qwen/respond",
          note: "Strong China-native default for public-facing agents."
        },
        zhipu: {
          label: "Zhipu",
          region: "china",
          mode: "live",
          envs: ["ZHIPU_API_KEY"],
          endpoint: "/api/gateway/connectors/model/zhipu/respond",
          note: "Useful backup path for domestic deployments."
        },
        moonshot: {
          label: "Moonshot",
          region: "china",
          mode: "live",
          envs: ["MOONSHOT_API_KEY"],
          endpoint: "/api/gateway/connectors/model/moonshot/respond",
          note: "Long-context China model path."
        },
        doubao: {
          label: "Doubao",
          region: "china",
          mode: "live",
          envs: ["DOUBAO_API_KEY or ARK_API_KEY"],
          endpoint: "/api/gateway/connectors/model/doubao/respond",
          note: "Volcengine Ark path for ByteDance-aligned deployments."
        }
      },
      chat: {
        telegram: {
          label: "Telegram",
          region: "global",
          mode: "live_bot",
          envs: ["TELEGRAM_BOT_TOKEN"],
          endpoint: "/api/gateway/connectors/chat/telegram/send",
          note: "Global outbound bot delivery."
        },
        feishu: {
          label: "Feishu",
          region: "china",
          mode: "live_webhook",
          envs: ["FEISHU_BOT_WEBHOOK_URL"],
          endpoint: "/api/gateway/connectors/chat/feishu/send",
          note: "China collaboration default with inbound and outbound coverage."
        },
        dingtalk: {
          label: "DingTalk",
          region: "china",
          mode: "live_webhook",
          envs: ["DINGTALK_BOT_WEBHOOK_URL"],
          endpoint: "/api/gateway/connectors/chat/dingtalk/send",
          note: "Best for enterprise China deployment."
        },
        wecom_bot: {
          label: "WeCom Bot",
          region: "china",
          mode: "live_webhook",
          envs: ["WECOM_BOT_WEBHOOK_URL"],
          endpoint: "/api/gateway/connectors/chat/wecom/send",
          note: "Outbound enterprise WeCom webhook path."
        },
        wechat_official_account: {
          label: "WeChat Official Account",
          region: "china",
          mode: "live_official_account_text",
          envs: ["WECHAT_OFFICIAL_ACCOUNT_APP_ID", "WECHAT_OFFICIAL_ACCOUNT_APP_SECRET"],
          endpoint: "/api/gateway/connectors/chat/wechat-official-account/send",
          note: "Consumer-facing China messaging surface."
        },
        qq: {
          label: "QQ Bot",
          region: "china",
          mode: "live_openapi_c2c_group",
          envs: ["QQ_BOT_APP_ID", "QQ_BOT_CLIENT_SECRET"],
          endpoint: "/api/gateway/connectors/chat/qq/send",
          note: "Youth and community-oriented China entry point."
        }
      },
      ingress: {
        telegram: {
          label: "Telegram Webhook",
          region: "global",
          mode: "secret_path_webhook",
          envs: ["DAWN_TELEGRAM_WEBHOOK_SECRET"],
          endpoint: "/api/gateway/ingress/telegram/webhook/{secret}",
          note: "Inbound Telegram task creation path."
        },
        feishu: {
          label: "Feishu Events",
          region: "china",
          mode: "challenge_callback",
          envs: ["No secret required for basic challenge mode"],
          endpoint: "/api/gateway/ingress/feishu/events",
          note: "Inbound Feishu event challenge and message route."
        },
        dingtalk: {
          label: "DingTalk Events",
          region: "china",
          mode: "callback_token",
          envs: ["DAWN_DINGTALK_CALLBACK_TOKEN"],
          endpoint: "/api/gateway/ingress/dingtalk/events",
          note: "Inbound DingTalk task launch route."
        },
        wecom: {
          label: "WeCom Events",
          region: "china",
          mode: "callback_token",
          envs: ["DAWN_WECOM_CALLBACK_TOKEN"],
          endpoint: "/api/gateway/ingress/wecom/events",
          note: "Inbound enterprise WeCom route."
        },
        wechat_official_account: {
          label: "WeChat Official Account Events",
          region: "china",
          mode: "token_verification",
          envs: ["DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN"],
          endpoint: "/api/gateway/ingress/wechat-official-account/events",
          note: "Inbound WeChat OA verification and XML message route."
        },
        qq: {
          label: "QQ Bot Events",
          region: "china",
          mode: "callback_secret",
          envs: ["DAWN_QQ_BOT_CALLBACK_SECRET"],
          endpoint: "/api/gateway/ingress/qq/events",
          note: "Inbound QQ bot event route."
        }
      }
    };
    const ellipsis = (value, max = 66) => {
      if (!value) return "—";
      return value.length > max ? `${value.slice(0, max)}…` : value;
    };
    const generateUuid = () => {
      if (window.crypto?.randomUUID) return window.crypto.randomUUID();
      return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (char) => {
        const random = Math.random() * 16 | 0;
        const value = char === "x" ? random : ((random & 0x3) | 0x8);
        return value.toString(16);
      });
    };
    const escapeHtml = (value) => String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;");
    const badge = (value) => {
      const normalized = String(value || "").toLowerCase();
      const tone = /complete|authorized|connected|acknowledged|task_created/.test(normalized)
        ? "ok"
        : /failed|rejected|disconnected/.test(normalized)
          ? "bad"
          : "warn";
      return `<span class="status ${tone}">${value}</span>`;
    };
    async function postJson(url, body) {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body)
      });
      const payload = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(payload.error || `${url} -> ${response.status}`);
      return payload;
    }
    async function fetchJson(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${url} -> ${response.status}`);
      return response.json();
    }
    async function fetchJsonOptional(url) {
      const response = await fetch(url);
      if (response.status === 404) return null;
      if (!response.ok) throw new Error(`${url} -> ${response.status}`);
      return response.json();
    }
    function setConsoleStreamStatus(label, detail = "") {
      const pill = document.getElementById("console-stream-pill");
      if (!pill) return;
      pill.innerHTML = `Console stream · ${escapeHtml(label)}${detail ? ` · <code>${escapeHtml(detail)}</code>` : ""}`;
    }
    function scheduleConsoleRefresh(preferredNodeId) {
      if (consoleRefreshTimer) return;
      consoleRefreshTimer = window.setTimeout(async () => {
        consoleRefreshTimer = null;
        try {
          await refresh(preferredNodeId);
        } catch (error) {
          setConsoleStreamStatus("refresh error", error.message);
        }
      }, 220);
    }
    function connectConsoleStream() {
      if (consoleEventSource) {
        consoleEventSource.close();
      }
      if (consoleReconnectTimer) {
        window.clearTimeout(consoleReconnectTimer);
        consoleReconnectTimer = null;
      }
      setConsoleStreamStatus("connecting");
      const source = new EventSource("/console/events");
      consoleEventSource = source;
      source.onopen = () => {
        setConsoleStreamStatus("live");
      };
      source.addEventListener("console_update", (event) => {
        try {
          const payload = JSON.parse(event.data);
          setConsoleStreamStatus(payload.status || payload.channel || "live", payload.channel);
          scheduleConsoleRefresh();
        } catch (_error) {
          setConsoleStreamStatus("live");
          scheduleConsoleRefresh();
        }
      });
      source.onerror = () => {
        setConsoleStreamStatus("reconnecting");
        source.close();
        consoleEventSource = null;
        if (!consoleReconnectTimer) {
          consoleReconnectTimer = window.setTimeout(() => {
            consoleReconnectTimer = null;
            connectConsoleStream();
          }, 3000);
        }
      };
    }
    function commandOptionsForNode(node) {
      const capabilities = Array.isArray(node?.capabilities) ? node.capabilities : [];
      return Object.keys(commandTemplates).filter((commandType) =>
        capabilities.length === 0 ? commandType !== "shell_exec" : capabilities.includes(commandType)
      );
    }
    function applyCommandTemplate(force = false) {
      const commandSelect = document.getElementById("node-command-type");
      const payloadInput = document.getElementById("node-command-payload");
      if (!commandSelect || !payloadInput) return;
      if (!force && payloadInput.dataset.dirty === "true") return;
      const payload = commandTemplates[commandSelect.value] || {};
      payloadInput.value = JSON.stringify(payload, null, 2);
      payloadInput.dataset.dirty = "false";
    }
    function renderCommandTemplateChips(commandTypes) {
      const container = document.getElementById("command-template-chips");
      if (!container) return;
      container.innerHTML = commandTypes.map((commandType) => `
        <button type="button" class="chip" onclick="selectCommandTemplate('${commandType}')">
          ${commandType}<br /><span class="tiny">${commandTemplateDescriptions[commandType] || "Node command"}</span>
        </button>
      `).join("");
    }
    function selectCommandTemplate(commandType) {
      const commandSelect = document.getElementById("node-command-type");
      if (!commandSelect) return;
      commandSelect.value = commandType;
      applyCommandTemplate(true);
    }
    function syncNodeCommandForm(nodes, selectedNodeId) {
      const nodeSelect = document.getElementById("node-command-node");
      const commandSelect = document.getElementById("node-command-type");
      if (!nodeSelect || !commandSelect) return;

      const nodeOptions = nodes.map((node) =>
        `<option value="${node.nodeId}">${node.displayName || node.nodeId}</option>`
      ).join("");
      nodeSelect.innerHTML = nodeOptions || `<option value="">No nodes</option>`;
      if (selectedNodeId && nodes.some((node) => node.nodeId === selectedNodeId)) {
        nodeSelect.value = selectedNodeId;
      }

      const selectedNode = nodes.find((node) => node.nodeId === nodeSelect.value) || nodes[0];
      const commandTypes = commandOptionsForNode(selectedNode);
      const currentCommand = commandSelect.value;
      commandSelect.innerHTML = commandTypes.map((commandType) =>
        `<option value="${commandType}">${commandType}</option>`
      ).join("") || `<option value="">No supported commands</option>`;
      if (currentCommand && commandTypes.includes(currentCommand)) {
        commandSelect.value = currentCommand;
      }
      renderCommandTemplateChips(commandTypes);
      applyCommandTemplate(false);
    }
    function syncRolloutConsole(nodes, selectedNodeId) {
      const rolloutSelect = document.getElementById("rollout-node-id");
      if (!rolloutSelect) return;
      const options = nodes.map((node) =>
        `<option value="${escapeHtml(node.nodeId)}">${escapeHtml(node.displayName || node.nodeId)}</option>`
      ).join("");
      rolloutSelect.innerHTML = options || `<option value="">No nodes</option>`;
      if (selectedNodeId && nodes.some((node) => node.nodeId === selectedNodeId)) {
        rolloutSelect.value = selectedNodeId;
      }
    }
    function syncAgentDelegationForm(cards) {
      const cardSelect = document.getElementById("delegate-card-id");
      if (!cardSelect) return;
      const selectedCardId = cardSelect.value;
      const options = cards.map((card) => {
        const label = `${card.card?.name || card.cardId} · ${card.locallyHosted ? "local" : "remote"}`;
        return `<option value="${escapeHtml(card.cardId)}">${escapeHtml(label)}</option>`;
      }).join("");
      cardSelect.innerHTML = options || `<option value="">No cards</option>`;
      if (selectedCardId && cards.some((card) => card.cardId === selectedCardId)) {
        cardSelect.value = selectedCardId;
      }
    }
    function parseOptionalNumber(rawValue, label) {
      const trimmed = String(rawValue || "").trim();
      if (!trimmed) return null;
      const parsed = Number(trimmed);
      if (!Number.isFinite(parsed) || parsed < 0) {
        throw new Error(`${label} must be a non-negative number.`);
      }
      return parsed;
    }
    function buildSettlementDraft() {
      const amount = parseOptionalNumber(document.getElementById("delegate-amount")?.value, "Settlement amount");
      const counterOfferAmount = parseOptionalNumber(document.getElementById("delegate-counter-offer")?.value, "Counter offer");
      const quoteId = document.getElementById("delegate-quote-id")?.value?.trim() || null;
      const description = document.getElementById("delegate-description")?.value?.trim() || "Settle remote delegation";
      if (amount == null && !quoteId && counterOfferAmount == null) return null;
      if (amount == null) {
        throw new Error("Settlement amount is required when using quote settlement actions.");
      }
      return {
        mandateId: generateUuid(),
        amount,
        description,
        quoteId,
        counterOfferAmount
      };
    }
    function splitList(rawValue) {
      return String(rawValue || "")
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean);
    }
    function readConfiguredConnector(configured, key) {
      if (!configured) return false;
      const normalizedKey = String(key || "").replace(/_([a-z])/g, (_, value) => value.toUpperCase());
      return Boolean(configured[normalizedKey]);
    }
    function renderConnectorMatrix(connectorStatus, ingressStatus) {
      const summaryGrid = document.getElementById("connector-summary-grid");
      const container = document.getElementById("connector-matrix");
      if (!summaryGrid || !container) return;

      const configured = connectorStatus?.configured || {};
      const supportedModels = connectorStatus?.supportedModelProviders || [];
      const supportedChats = connectorStatus?.supportedChatPlatforms || [];
      const supportedIngress = ingressStatus?.supportedPlatforms || [];
      const configuredModelCount = supportedModels.filter((provider) => readConfiguredConnector(configured, provider.provider)).length;
      const configuredChatCount = supportedChats.filter((platform) => readConfiguredConnector(configured, platform.platform)).length;
      const ingressSecretsCount = [
        ingressStatus?.telegramWebhookSecretConfigured,
        ingressStatus?.dingtalkCallbackTokenConfigured,
        ingressStatus?.wecomCallbackTokenConfigured,
        ingressStatus?.wechatOfficialAccountTokenConfigured,
        ingressStatus?.qqBotCallbackSecretConfigured
      ].filter(Boolean).length;

      summaryGrid.innerHTML = renderDrawerMetrics([
        ["Model connectors", `${configuredModelCount}/${supportedModels.length}`],
        ["Chat connectors", `${configuredChatCount}/${supportedChats.length}`],
        ["Ingress secrets", `${ingressSecretsCount}/${supportedIngress.length}`],
        ["Ingress tasks", ingressStatus?.taskCreatedEvents ?? 0]
      ]);

      const connectorCards = [
        ...supportedModels.map((provider) => ({
          title: provider.provider,
          subtitle: `${provider.region} · ${provider.integrationMode}`,
          active: readConfiguredConnector(configured, provider.provider),
          type: "Model"
        })),
        ...supportedChats.map((platform) => ({
          title: platform.platform,
          subtitle: `${platform.region} · ${platform.integrationMode}`,
          active: readConfiguredConnector(configured, platform.platform),
          type: "Chat"
        })),
        ...supportedIngress.map((platform) => ({
          title: platform,
          subtitle: "Inbound gateway route",
          active: Boolean({
            telegram: ingressStatus?.telegramWebhookSecretConfigured,
            dingtalk: ingressStatus?.dingtalkCallbackTokenConfigured,
            wecom: ingressStatus?.wecomCallbackTokenConfigured,
            wechat_official_account: ingressStatus?.wechatOfficialAccountTokenConfigured,
            qq: ingressStatus?.qqBotCallbackSecretConfigured,
            feishu: true
          }[platform]),
          type: "Ingress"
        }))
      ];

      container.innerHTML = connectorCards.map((item) => `
        <article class="feed-item">
          <strong>${escapeHtml(item.type)} · ${escapeHtml(item.title)}</strong>
          <p>${escapeHtml(item.subtitle)}</p>
          <p>${item.active ? badge("configured") : badge("dry_run")}</p>
        </article>
      `).join("");
    }
    function surfaceOptions(surface, connectorStatus, ingressStatus) {
      if (surface === "model") {
        return (connectorStatus?.supportedModelProviders || []).map((provider) => ({
          key: provider.provider,
          label: provider.provider,
          region: provider.region,
          configured: readConfiguredConnector(connectorStatus?.configured, provider.provider)
        }));
      }
      if (surface === "chat") {
        return (connectorStatus?.supportedChatPlatforms || []).map((platform) => ({
          key: platform.platform,
          label: platform.platform,
          region: platform.region,
          configured: readConfiguredConnector(connectorStatus?.configured, platform.platform)
        }));
      }
      const ingressMap = {
        telegram: ingressStatus?.telegramWebhookSecretConfigured,
        feishu: true,
        dingtalk: ingressStatus?.dingtalkCallbackTokenConfigured,
        wecom: ingressStatus?.wecomCallbackTokenConfigured,
        wechat_official_account: ingressStatus?.wechatOfficialAccountTokenConfigured,
        qq: ingressStatus?.qqBotCallbackSecretConfigured
      };
      return (ingressStatus?.supportedPlatforms || []).map((platform) => ({
        key: platform,
        label: platform,
        region: ["telegram"].includes(platform) ? "global" : "china",
        configured: Boolean(ingressMap[platform])
      }));
    }
    function currentSetupProfile() {
      const surface = document.getElementById("setup-surface")?.value || "model";
      const target = document.getElementById("setup-target")?.value || "";
      return {
        surface,
        target,
        profile: setupProfiles[surface]?.[target] || null
      };
    }
    function setupEnvBlock(profile) {
      if (!profile) return "";
      return profile.envs.map((env) => {
        if (env.includes(" or ")) {
          return `# choose one\n${env.split(" or ").map((item) => `${item}=`).join("\n")}`;
        }
        if (env.startsWith("No secret required")) {
          return `# ${env}`;
        }
        return `${env}=`;
      }).join("\n");
    }
    function updateSetupNavigator(surface, connectorStatus, ingressStatus) {
      const surfaceSelect = document.getElementById("setup-surface");
      const targetSelect = document.getElementById("setup-target");
      const summaryGrid = document.getElementById("setup-summary-grid");
      const requirements = document.getElementById("setup-requirements");
      const status = document.getElementById("setup-navigator-status");
      const envBlock = document.getElementById("setup-env-block");
      const presetChips = document.getElementById("setup-preset-chips");
      if (!surfaceSelect || !targetSelect || !summaryGrid || !requirements || !status || !envBlock || !presetChips) return;

      if (surface && surfaceSelect.value !== surface) {
        surfaceSelect.value = surface;
      }

      const options = surfaceOptions(surfaceSelect.value, connectorStatus, ingressStatus);
      const previousTarget = targetSelect.value;
      targetSelect.innerHTML = options.map((option) =>
        `<option value="${escapeHtml(option.key)}">${escapeHtml(option.label)} · ${escapeHtml(option.region)}</option>`
      ).join("") || `<option value="">No targets</option>`;
      if (previousTarget && options.some((option) => option.key === previousTarget)) {
        targetSelect.value = previousTarget;
      }

      const current = currentSetupProfile();
      const option = options.find((entry) => entry.key === current.target) || options[0];
      if (option && targetSelect.value !== option.key) {
        targetSelect.value = option.key;
      }
      const profile = setupProfiles[surfaceSelect.value]?.[targetSelect.value] || null;
      const configuredCount = options.filter((entry) => entry.configured).length;
      const regionCount = options.filter((entry) => entry.region === "china").length;
      summaryGrid.innerHTML = [
        {
          label: "Configured",
          value: `${configuredCount}/${options.length || 0}`,
          note: "Surfaces ready right now"
        },
        {
          label: "China Paths",
          value: `${regionCount}`,
          note: "Domestic connectors or routes"
        },
        {
          label: "Surface",
          value: surfaceSelect.value,
          note: "Current setup lens"
        }
      ].map((card) => `
        <article class="setup-card">
          <span>${escapeHtml(card.label)}</span>
          <strong>${escapeHtml(card.value)}</strong>
          <p>${escapeHtml(card.note)}</p>
        </article>
      `).join("");

      presetChips.innerHTML = [
        ["china", "China Go-Live"],
        ["global", "Global MVP"],
        ["ingress", "Ingress First"]
      ].map(([preset, label]) => `
        <button type="button" class="chip" onclick="loadSetupPreset('${preset}')">${escapeHtml(label)}</button>
      `).join("");

      if (!profile || !option) {
        requirements.innerHTML = `<div class="setup-requirement"><strong>No target selected</strong><p>Choose a surface and target to see setup instructions.</p></div>`;
        status.textContent = "Choose a surface to see required secrets, test routes, and next steps.";
        envBlock.value = "";
        return;
      }

      status.innerHTML = `${option.configured ? badge("configured") : badge("missing")} <strong>${escapeHtml(profile.label)}</strong> · ${escapeHtml(profile.region)} · ${escapeHtml(profile.mode)} · test via <code>${escapeHtml(profile.endpoint)}</code>`;
      requirements.innerHTML = [
        {
          title: "Secrets / Env Vars",
          body: profile.envs.join(", ")
        },
        {
          title: "Activation Path",
          body: profile.note
        },
        {
          title: "Verification",
          body: `After setting the variables, confirm readiness in Connector Matrix and test ${profile.endpoint}.`
        }
      ].map((item) => `
        <article class="setup-requirement">
          <strong>${escapeHtml(item.title)}</strong>
          <p>${escapeHtml(item.body)}</p>
        </article>
      `).join("");
      envBlock.value = setupEnvBlock(profile);
    }
    function loadSetupPreset(preset) {
      const surfaceSelect = document.getElementById("setup-surface");
      const targetSelect = document.getElementById("setup-target");
      if (!surfaceSelect || !targetSelect) return;
      const presets = {
        china: { surface: "model", target: "qwen" },
        global: { surface: "model", target: "openai" },
        ingress: { surface: "ingress", target: "wechat_official_account" }
      };
      const next = presets[preset];
      if (!next) return;
      surfaceSelect.value = next.surface;
      updateSetupNavigator(next.surface, latestConnectorStatus, latestIngressStatus);
      targetSelect.value = next.target;
      updateSetupNavigator(next.surface, latestConnectorStatus, latestIngressStatus);
    }
    async function copySetupEnvBlock() {
      const value = document.getElementById("setup-env-block")?.value || "";
      const status = document.getElementById("setup-navigator-status");
      if (!value.trim()) {
        if (status) status.textContent = "No environment block is available for the current selection.";
        return;
      }
      try {
        await navigator.clipboard.writeText(value);
        if (status) status.innerHTML = `${badge("copied")} Suggested environment block copied to clipboard.`;
      } catch (_error) {
        if (status) status.textContent = "Clipboard write failed. Copy directly from the environment block.";
      }
    }
    function selectCommand(commandId) {
      selectedCommandId = commandId;
      const rows = document.querySelectorAll(".command-row");
      rows.forEach((row) => row.classList.toggle("active", row.dataset.commandId === commandId));
      renderCommandDetail(
        visibleCommandsCache.find((command) => command.commandId === selectedCommandId) || null
      );
    }
    function closeDetailDrawer() {
      detailDrawerState = null;
      renderDetailDrawer(null);
      document.getElementById("detail-drawer")?.classList.remove("open");
      document.getElementById("detail-drawer-backdrop")?.classList.remove("open");
    }
    function openDetailDrawer(payload) {
      detailDrawerState = payload;
      renderDetailDrawer(payload);
      document.getElementById("detail-drawer")?.classList.add("open");
      document.getElementById("detail-drawer-backdrop")?.classList.add("open");
    }
    function formatJsonBlock(value) {
      if (value == null) return "null";
      if (typeof value === "string") return value;
      try {
        return JSON.stringify(value, null, 2);
      } catch (_error) {
        return String(value);
      }
    }
    function renderDrawerMetrics(metrics) {
      return metrics.map(([label, value]) => `
        <div class="detail-metric">
          <span>${escapeHtml(label)}</span>
          <strong>${escapeHtml(value)}</strong>
        </div>
      `).join("");
    }
    function renderDrawerActions(actions) {
      return actions.map((action) => `
        <button
          type="button"
          class="${action.secondary ? "secondary" : ""}"
          onclick="${action.onclick}"
        >${escapeHtml(action.label)}</button>
      `).join("");
    }
    function setDrawerStatus(message, isError = false) {
      const status = document.getElementById("detail-drawer-status");
      if (!status) return;
      if (!message) {
        status.classList.remove("open");
        status.innerHTML = "";
        status.style.borderColor = "rgba(255,255,255,0.08)";
        status.style.color = "var(--muted)";
        return;
      }
      status.classList.add("open");
      status.innerHTML = message;
      status.style.borderColor = isError ? "rgba(255,130,102,0.32)" : "rgba(255,255,255,0.08)";
      status.style.color = isError ? "#ffd2c7" : "var(--muted)";
    }
    function renderApprovalWorkflowForm(approval, state) {
      const form = document.getElementById("detail-drawer-form");
      if (!form) return;
      if (!approval || approval.status !== "pending") {
        form.classList.remove("open");
        form.innerHTML = "";
        return;
      }
      const payment = state?.payment;
      const isPayment = approval.kind === "payment";
      const defaultReason = isPayment
        ? "approved via liquid glass approval center"
        : "approved via command approval drawer";
      const workflowNote = isPayment
        ? "Payment approvals require the same MCU DID and signature pair used by the AP2 authorization path. Rejects still record an operator reason."
        : "Node-command approvals release the pending command into the control-plane queue. Rejects mark the command as failed and preserve the operator reason.";
      form.classList.add("open");
      form.innerHTML = `
        <div class="eyebrow">Workflow Form</div>
        <div class="drawer-note">${escapeHtml(workflowNote)}</div>
        <div class="form-grid">
          <div class="field">
            <label for="approval-actor">Actor</label>
            <input id="approval-actor" type="text" value="console-operator" />
          </div>
          <div class="field">
            <label for="approval-reference">${isPayment ? "Transaction" : "Reference"}</label>
            <input id="approval-reference" type="text" value="${escapeHtml(approval.referenceId)}" disabled />
          </div>
        </div>
        ${isPayment ? `
          <div class="form-grid">
            <div class="field">
              <label for="approval-mcu-public-did">MCU Public DID</label>
              <input id="approval-mcu-public-did" type="text" placeholder="did:dawn:mcu:..." value="${escapeHtml(payment?.mcuPublicDid || "")}" />
            </div>
            <div class="field">
              <label for="approval-transaction-state">Payment State</label>
              <input id="approval-transaction-state" type="text" value="${escapeHtml(payment?.status || "pending_physical_auth")}" disabled />
            </div>
          </div>
          <div class="field">
            <label for="approval-mcu-signature">MCU Signature Hex</label>
            <textarea id="approval-mcu-signature" placeholder="Paste MCU signature hex captured from the signing device."></textarea>
          </div>
        ` : ""}
        <div class="field">
          <label for="approval-reason">Reason</label>
          <textarea id="approval-reason" placeholder="Record why this action should proceed or be rejected.">${escapeHtml(defaultReason)}</textarea>
        </div>
        <div class="toolbar">
          <button type="button" onclick="submitDrawerApproval('approve')">Approve</button>
          <button type="button" class="secondary" onclick="submitDrawerApproval('reject')">Reject</button>
          <button type="button" class="secondary" onclick="openApprovalDetail('${approval.approvalId}')">Reload</button>
        </div>
      `;
    }
    async function submitDrawerApproval(decision) {
      const approval = detailDrawerState?.approval;
      if (!approval) {
        setDrawerStatus("No approval is selected.", true);
        return;
      }
      try {
        const actor = document.getElementById("approval-actor")?.value?.trim() || "console-operator";
        const reason = document.getElementById("approval-reason")?.value?.trim() || undefined;
        const payload = { actor, decision, reason };
        if (approval.kind === "payment" && decision === "approve") {
          const mcuPublicDid = document.getElementById("approval-mcu-public-did")?.value?.trim();
          const mcuSignature = document.getElementById("approval-mcu-signature")?.value?.trim();
          if (!mcuPublicDid) {
            setDrawerStatus("MCU public DID is required before approving a payment.", true);
            return;
          }
          if (!mcuSignature) {
            setDrawerStatus("MCU signature hex is required before approving a payment.", true);
            return;
          }
          payload.mcuPublicDid = mcuPublicDid;
          payload.mcuSignature = mcuSignature;
        }
        setDrawerStatus(`Submitting <strong>${escapeHtml(decision)}</strong> for <code>${escapeHtml(approval.approvalId)}</code>…`);
        await decideApproval(approval.approvalId, approval.kind, decision, payload);
      } catch (error) {
        setDrawerStatus(error.message, true);
      }
    }
    function currentMarketplaceCatalogQuery() {
      const params = new URLSearchParams({
        signedOnly: "true",
        publishedOnly: "true"
      });
      const q = document.getElementById("catalog-search")?.value?.trim();
      const kind = document.getElementById("catalog-kind")?.value;
      if (q) params.set("q", q);
      if (kind) params.set("kind", kind);
      return params.toString();
    }
    function renderCatalogTags(values) {
      const tags = (values || []).filter(Boolean).slice(0, 5);
      return tags.length
        ? `<div class="catalog-tags">${tags.map((tag) => `<span class="catalog-tag">${escapeHtml(tag)}</span>`).join("")}</div>`
        : `<div class="catalog-meta">No declared tags.</div>`;
    }
    function renderMarketplaceCatalog(catalog) {
      marketplaceCatalogCache = catalog || { skills: [], agentCards: [] };
      const grid = document.getElementById("marketplace-catalog-grid");
      const status = document.getElementById("marketplace-catalog-status");
      if (!grid || !status) return;
      const skills = marketplaceCatalogCache.skills || [];
      const agentCards = marketplaceCatalogCache.agentCards || [];
      status.innerHTML = `Showing <strong>${skills.length}</strong> signed skills and <strong>${agentCards.length}</strong> published agents in the current query.`;
      const skillCards = skills.slice(0, 4).map((skill) => `
        <article class="catalog-card">
          <div>
            <strong>${escapeHtml(skill.displayName)} <small>${escapeHtml(skill.skillId)}@${escapeHtml(skill.version)}</small></strong>
            <div class="catalog-meta">${escapeHtml(skill.description || "Signed marketplace skill package")}</div>
            <div class="catalog-meta">${skill.signed ? "Trusted publisher" : "Unsigned"} · ${skill.active ? "active locally" : "installable"}</div>
          </div>
          ${renderCatalogTags(skill.capabilities)}
          <div class="catalog-actions">
            <button type="button" onclick="installSkillPackage(decodeURIComponent('${encodeURIComponent(skill.packageUrl)}'))">Install Skill</button>
            <button type="button" class="secondary" onclick="window.open(decodeURIComponent('${encodeURIComponent(skill.packageUrl)}'), '_blank')">View Package</button>
          </div>
        </article>
      `);
      const agentCardsMarkup = agentCards.slice(0, 4).map((agent) => `
        <article class="catalog-card">
          <div>
            <strong>${escapeHtml(agent.name)} <small>${escapeHtml(agent.cardId)}</small></strong>
            <div class="catalog-meta">${escapeHtml(agent.description || "Published agent card")}</div>
            <div class="catalog-meta">${agent.locallyHosted ? "Local host" : "Remote import"} · ${escapeHtml((agent.regions || []).join(", ") || "global")}</div>
          </div>
          ${renderCatalogTags([...(agent.chatPlatforms || []), ...(agent.paymentRoles || []), ...(agent.modelProviders || [])])}
          <div class="catalog-actions">
            ${agent.cardUrl
              ? `<button type="button" onclick="importAgentCardFromUrl(decodeURIComponent('${encodeURIComponent(agent.cardUrl)}'))">Import Card</button>`
              : `<button type="button" class="secondary" disabled>No Card URL</button>`}
            ${agent.cardUrl
              ? `<button type="button" class="secondary" onclick="window.open(decodeURIComponent('${encodeURIComponent(agent.cardUrl)}'), '_blank')">Open Card</button>`
              : ""}
          </div>
        </article>
      `);
      grid.innerHTML = [...skillCards, ...agentCardsMarkup].join("")
        || `<article class="catalog-card"><strong>No marketplace matches</strong><div class="catalog-meta">Broaden the search or switch the catalog kind filter.</div></article>`;
    }
    async function loadMarketplaceCatalog() {
      const status = document.getElementById("marketplace-catalog-status");
      try {
        if (status) status.textContent = "Refreshing marketplace catalog…";
        const catalog = await fetchJson(`/api/gateway/marketplace/catalog?${currentMarketplaceCatalogQuery()}`);
        renderMarketplaceCatalog(catalog);
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    function renderDetailDrawer(state) {
      const eyebrow = document.getElementById("detail-drawer-eyebrow");
      const title = document.getElementById("detail-drawer-title");
      const subtitle = document.getElementById("detail-drawer-subtitle");
      const meta = document.getElementById("detail-drawer-meta");
      const actions = document.getElementById("detail-drawer-actions");
      const form = document.getElementById("detail-drawer-form");
      const pre = document.getElementById("detail-drawer-pre");
      if (!eyebrow || !title || !subtitle || !meta || !actions || !form || !pre) return;

      form.classList.remove("open");
      form.innerHTML = "";
      setDrawerStatus("");

      if (!state) {
        eyebrow.textContent = "Inspector";
        title.textContent = "Detail Drawer";
        subtitle.textContent = "Select an approval, command, settlement, or reconciliation record.";
        meta.innerHTML = "";
        actions.innerHTML = "";
        pre.textContent = "Nothing selected.";
        return;
      }

      if (state.kind === "approval") {
        const approval = state.approval;
        eyebrow.textContent = "Approval";
        title.textContent = approval.title;
        subtitle.textContent = `${approval.kind} · ${approval.referenceId}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Status", approval.status],
          ["Task", approval.taskId || "—"],
          ["Actor", approval.actor || "—"],
          ["Updated", approval.updatedAtUnixMs]
        ]);
        renderApprovalWorkflowForm(approval, state);
        const actionSet = [
          {
            label: "Refresh",
            onclick: `openApprovalDetail('${approval.approvalId}')`,
            secondary: true
          }
        ];
        actions.innerHTML = renderDrawerActions(actionSet);
        pre.textContent = formatJsonBlock({
          approval,
          nodeCommand: state.nodeCommand,
          payment: state.payment
        });
        return;
      }

      if (state.kind === "command") {
        const command = state.command;
        eyebrow.textContent = "Node Command";
        title.textContent = command.commandType;
        subtitle.textContent = `${command.nodeId} · ${command.commandId}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Status", command.status],
          ["Node", command.nodeId],
          ["Updated", command.updatedAtUnixMs],
          ["Error", command.error || "—"]
        ]);
        actions.innerHTML = renderDrawerActions([
          {
            label: "Refresh",
            onclick: `openCommandDetail('${command.commandId}')`,
            secondary: true
          }
        ]);
        pre.textContent = formatJsonBlock({
          payload: command.payload,
          result: command.result,
          error: command.error
        });
        return;
      }

      if (state.kind === "settlement") {
        const settlement = state.settlement;
        eyebrow.textContent = "Settlement";
        title.textContent = settlement.cardId;
        subtitle.textContent = `${settlement.amount} · ${settlement.transactionId}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Status", settlement.status],
          ["Quote", settlement.quoteId || "—"],
          ["Invocation", settlement.invocationId],
          ["Updated", settlement.updatedAtUnixMs]
        ]);
        actions.innerHTML = renderDrawerActions([
          {
            label: "Refresh",
            onclick: `openSettlementDetail('${settlement.settlementId}')`,
            secondary: true
          },
          {
            label: "Reconcile",
            onclick: `reconcileSettlement('${settlement.settlementId}')`
          },
          ...(state.reconciliation
            ? [{
                label: "Receipt Ledger",
                onclick: `openReconciliationDetail('${state.reconciliation.reconciliationId}')`,
                secondary: true
              }]
            : [])
        ]);
        pre.textContent = formatJsonBlock({
          settlement,
          reconciliation: state.reconciliation
        });
        return;
      }

      if (state.kind === "reconciliation") {
        const reconciliation = state.reconciliation;
        eyebrow.textContent = "Reconciliation";
        title.textContent = reconciliation.cardId;
        subtitle.textContent = `${reconciliation.direction} · ${reconciliation.settlementId}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Status", reconciliation.reconciliationStatus],
          ["Settlement", reconciliation.settlementStatus],
          ["Transaction", reconciliation.transactionId],
          ["Last sync", reconciliation.lastSyncAtUnixMs || "—"]
        ]);
        actions.innerHTML = renderDrawerActions([
          {
            label: "Refresh",
            onclick: `openReconciliationDetail('${reconciliation.reconciliationId}')`,
            secondary: true
          },
          ...(reconciliation.direction === "outbound"
            ? [{
                label: "Push Receipt",
                onclick: `reconcileSettlement('${reconciliation.settlementId}')`
              }]
            : [])
        ]);
        pre.textContent = formatJsonBlock(reconciliation);
        return;
      }

      if (state.kind === "invocation") {
        const invocation = state.invocation;
        eyebrow.textContent = "Remote Invocation";
        title.textContent = invocation.cardId;
        subtitle.textContent = `${invocation.status} · ${invocation.invocationId}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Remote task", invocation.remoteTaskId || "—"],
          ["Local task", invocation.localTaskId || "—"],
          ["Updated", invocation.updatedAtUnixMs],
          ["Error", invocation.error || "—"]
        ]);
        actions.innerHTML = renderDrawerActions([
          {
            label: "Refresh",
            onclick: `openInvocationDetail('${invocation.invocationId}')`,
            secondary: true
          },
          ...(state.settlement
            ? [{
                label: "Settlement",
                onclick: `openSettlementDetail('${state.settlement.settlementId}')`,
                secondary: true
              }]
            : [])
        ]);
        pre.textContent = formatJsonBlock({
          invocation,
          settlement: state.settlement
        });
        return;
      }

      if (state.kind === "quote") {
        const quote = state.quote;
        eyebrow.textContent = "Quote Ledger";
        title.textContent = quote.quoteId;
        subtitle.textContent = `${quote.cardId} · round ${quote.negotiationRound}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Status", quote.status],
          ["Mode", quote.quoteMode],
          ["Quoted", quote.quotedAmount ?? "—"],
          ["Expires", quote.expiresAtUnixMs || "—"]
        ]);
        const quoteActions = [
          {
            label: "Refresh",
            onclick: `openQuoteDetail(decodeURIComponent('${encodeURIComponent(quote.quoteId)}'))`,
            secondary: true
          }
        ];
        if (quote.sourceKind === "remote") {
          quoteActions.push({
            label: "Sync State",
            onclick: `syncQuoteState(decodeURIComponent('${encodeURIComponent(quote.cardId)}'), decodeURIComponent('${encodeURIComponent(quote.quoteId)}'))`,
            secondary: true
          });
        }
        if (quote.status === "offered") {
          quoteActions.push({
            label: "Revoke",
            onclick: `revokeQuote(decodeURIComponent('${encodeURIComponent(quote.quoteId)}'))`
          });
        }
        actions.innerHTML = renderDrawerActions(quoteActions);
        pre.textContent = formatJsonBlock(quote);
        return;
      }

      if (state.kind === "rollout") {
        const rollout = state.rollout;
        eyebrow.textContent = "Node Rollout";
        title.textContent = rollout.nodeId;
        subtitle.textContent = `${rollout.status} · ${rollout.bundleHash}`;
        meta.innerHTML = renderDrawerMetrics([
          ["Policy version", rollout.policyVersion],
          ["Policy hash", rollout.policyDocumentHash || "—"],
          ["Skill hash", rollout.skillDistributionHash],
          ["Last ack", rollout.lastAckAtUnixMs || "—"]
        ]);
        actions.innerHTML = renderDrawerActions([
          {
            label: "Refresh",
            onclick: `openRolloutDetail(decodeURIComponent('${encodeURIComponent(rollout.nodeId)}'))`,
            secondary: true
          }
        ]);
        pre.textContent = formatJsonBlock(rollout);
      }
    }
    async function openApprovalDetail(approvalId) {
      try {
        const detail = await fetchJson(`/api/gateway/approvals/${encodeURIComponent(approvalId)}`);
        openDetailDrawer({
          kind: "approval",
          approval: detail.approval,
          nodeCommand: detail.nodeCommand,
          payment: detail.payment
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function openCommandDetail(commandId) {
      try {
        const command = await fetchJson(`/api/gateway/control-plane/commands/${encodeURIComponent(commandId)}`);
        openDetailDrawer({
          kind: "command",
          command
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function openSettlementDetail(settlementId) {
      try {
        const [settlement, reconciliation] = await Promise.all([
          fetchJson(`/api/gateway/agent-cards/settlements/${encodeURIComponent(settlementId)}`),
          fetchJsonOptional(`/api/gateway/agent-cards/settlements/${encodeURIComponent(settlementId)}/reconciliation`)
        ]);
        openDetailDrawer({
          kind: "settlement",
          settlement,
          reconciliation
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function openReconciliationDetail(reconciliationId) {
      try {
        const reconciliation = await fetchJson(`/api/gateway/agent-cards/reconciliation/${encodeURIComponent(reconciliationId)}`);
        openDetailDrawer({
          kind: "reconciliation",
          reconciliation
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function openInvocationDetail(invocationId) {
      try {
        const [invocation, settlement] = await Promise.all([
          fetchJson(`/api/gateway/agent-cards/invocations/${encodeURIComponent(invocationId)}`),
          fetchJsonOptional(`/api/gateway/agent-cards/invocations/${encodeURIComponent(invocationId)}/settlement`)
        ]);
        openDetailDrawer({
          kind: "invocation",
          invocation,
          settlement
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function openQuoteDetail(quoteId) {
      try {
        const quote = await fetchJson(`/api/gateway/agent-cards/quotes/${encodeURIComponent(quoteId)}`);
        openDetailDrawer({
          kind: "quote",
          quote
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function openRolloutDetail(nodeId) {
      try {
        const rollout = await fetchJson(`/api/gateway/control-plane/nodes/${encodeURIComponent(nodeId)}/rollout`);
        openDetailDrawer({
          kind: "rollout",
          rollout
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function previewSettlementQuote(useCounterOffer) {
      const cardId = document.getElementById("delegate-card-id")?.value;
      const amount = parseOptionalNumber(document.getElementById("delegate-amount")?.value, "Settlement amount");
      const quoteId = document.getElementById("delegate-quote-id")?.value?.trim() || null;
      const counterOfferAmount = useCounterOffer
        ? parseOptionalNumber(document.getElementById("delegate-counter-offer")?.value, "Counter offer")
        : null;
      const description = document.getElementById("delegate-description")?.value?.trim() || "";
      const status = document.getElementById("delegate-status");
      if (!cardId) {
        window.alert("Select an agent card first.");
        return;
      }
      if (amount == null) {
        window.alert("Settlement amount is required to preview a quote.");
        return;
      }
      if (useCounterOffer && counterOfferAmount == null) {
        window.alert("Enter a counter-offer amount first.");
        return;
      }

      try {
        const params = new URLSearchParams({
          remote: "true",
          requestedAmount: String(amount),
          description,
          allowMetadataFallback: "true",
          timeoutSeconds: "10"
        });
        if (quoteId) params.set("quoteId", quoteId);
        if (counterOfferAmount != null) params.set("counterOfferAmount", String(counterOfferAmount));
        const quote = await fetchJson(`/api/gateway/agent-cards/${encodeURIComponent(cardId)}/quote?${params.toString()}`);
        if (quote.quoteId) {
          document.getElementById("delegate-quote-id").value = quote.quoteId;
        }
        if (quote.quotedAmount != null) {
          document.getElementById("delegate-amount").value = quote.quotedAmount;
        }
        if (quote.counterOfferAmount != null) {
          document.getElementById("delegate-counter-offer").value = quote.counterOfferAmount;
        }
        status.innerHTML = `Quote <code>${escapeHtml(quote.quoteId || "metadata")}</code> returned <strong>${escapeHtml(quote.quotedAmount ?? amount)}</strong> ${escapeHtml(quote.currency || "")}.`;
        await refresh();
        if (quote.quoteId) {
          await openQuoteDetail(quote.quoteId);
        }
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function invokeAgentCard() {
      const cardId = document.getElementById("delegate-card-id")?.value;
      const name = document.getElementById("delegate-name")?.value?.trim() || "delegate task";
      const instruction = document.getElementById("delegate-instruction")?.value?.trim();
      const awaitCompletion = document.getElementById("delegate-await-completion")?.value !== "false";
      const status = document.getElementById("delegate-status");
      if (!cardId) {
        window.alert("Select an agent card first.");
        return;
      }
      if (!instruction) {
        window.alert("Instruction is required.");
        return;
      }

      let settlement;
      try {
        settlement = buildSettlementDraft();
      } catch (error) {
        window.alert(error.message);
        return;
      }

      try {
        const response = await postJson(`/api/gateway/agent-cards/${encodeURIComponent(cardId)}/invoke`, {
          name,
          instruction,
          awaitCompletion,
          timeoutSeconds: 15,
          pollIntervalMs: 500,
          settlement
        });
        status.innerHTML = `Invocation <code>${escapeHtml(response.invocation.invocationId)}</code> is <strong>${escapeHtml(response.invocation.status)}</strong>.`;
        await refresh();
        await openInvocationDetail(response.invocation.invocationId);
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function revokeQuote(quoteId) {
      try {
        const reason = window.prompt("Revocation reason", "revoked via control center");
        if (!reason) return;
        const quote = await postJson(`/api/gateway/agent-cards/quotes/${encodeURIComponent(quoteId)}/revoke`, {
          reason
        });
        await refresh();
        openDetailDrawer({
          kind: "quote",
          quote
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function reconcileSettlement(settlementId) {
      try {
        const reconciliation = await postJson(`/api/gateway/agent-cards/settlements/${encodeURIComponent(settlementId)}/reconcile`, {});
        await refresh();
        openDetailDrawer({
          kind: "reconciliation",
          reconciliation
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function dispatchManualRollout() {
      const nodeId = document.getElementById("rollout-node-id")?.value;
      const status = document.getElementById("rollout-status");
      if (!nodeId) {
        window.alert("Select a node first.");
        return;
      }
      try {
        const response = await postJson(`/api/gateway/control-plane/nodes/${encodeURIComponent(nodeId)}/rollout`, {});
        status.innerHTML = `Rollout for <code>${escapeHtml(nodeId)}</code> is <strong>${escapeHtml(response.delivery)}</strong> with bundle <code>${escapeHtml(response.rollout.bundleHash)}</code>.`;
        await refresh(nodeId);
        await openRolloutDetail(nodeId);
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function installSkillPackage(packageUrlOverride = null) {
      const input = document.getElementById("marketplace-skill-package-url");
      const packageUrl = (packageUrlOverride || input?.value || "").trim();
      const status = document.getElementById("marketplace-status");
      if (input && packageUrlOverride) input.value = packageUrl;
      if (!packageUrl) {
        window.alert("Skill package URL is required.");
        return;
      }
      try {
        const response = await postJson("/api/gateway/marketplace/install/skill", {
          packageUrl,
          activate: true,
          allowUnsigned: false
        });
        if (status) {
          status.innerHTML = `Installed skill <code>${escapeHtml(response.skill.skillId)}@${escapeHtml(response.skill.version)}</code> with activation <strong>${escapeHtml(response.activated)}</strong>.`;
        }
        await refresh();
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function importAgentCardFromUrl(cardUrlOverride = null) {
      const input = document.getElementById("marketplace-card-url");
      const cardUrl = (cardUrlOverride || input?.value || "").trim();
      const status = document.getElementById("marketplace-status");
      if (input && cardUrlOverride) input.value = cardUrl;
      if (!cardUrl) {
        window.alert("Agent card URL is required.");
        return;
      }
      try {
        const response = await postJson("/api/gateway/marketplace/install/agent-card", {
          cardUrl,
          published: true
        });
        if (status) {
          status.innerHTML = `Imported agent card <code>${escapeHtml(response.cardId)}</code> from <code>${escapeHtml(cardUrl)}</code>.`;
        }
        await refresh();
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function publishAgentCard() {
      const status = document.getElementById("marketplace-status");
      const rawCard = document.getElementById("publish-card-json")?.value || "{}";
      let card;
      try {
        card = JSON.parse(rawCard);
      } catch (error) {
        window.alert(`Invalid agent card JSON: ${error.message}`);
        return;
      }

      try {
        const response = await postJson("/api/gateway/agent-cards/publish", {
          cardId: document.getElementById("publish-card-id")?.value?.trim() || null,
          card,
          regions: splitList(document.getElementById("publish-card-regions")?.value),
          languages: splitList(document.getElementById("publish-card-languages")?.value),
          modelProviders: splitList(document.getElementById("publish-card-model-providers")?.value),
          chatPlatforms: splitList(document.getElementById("publish-card-chat-platforms")?.value),
          paymentRoles: splitList(document.getElementById("publish-card-payment-roles")?.value),
          locallyHosted: document.getElementById("publish-card-local")?.value !== "false",
          published: document.getElementById("publish-card-published")?.value !== "false"
        });
        if (status) {
          status.innerHTML = `Published card <code>${escapeHtml(response.record.cardId)}</code>${response.wellKnownCardUrl ? ` · <code>${escapeHtml(response.wellKnownCardUrl)}</code>` : ""}.`;
        }
        await refresh();
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function syncQuoteState(cardId, quoteId) {
      try {
        const quote = await postJson(
          `/api/gateway/agent-cards/${encodeURIComponent(cardId)}/quotes/${encodeURIComponent(quoteId)}/sync`,
          {}
        );
        await refresh();
        openDetailDrawer({
          kind: "quote",
          quote
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function showCommand(commandId) {
      selectCommand(commandId);
      await openCommandDetail(commandId);
    }
    function renderCommandDetail(command) {
      const grid = document.getElementById("command-detail-grid");
      const pre = document.getElementById("command-detail-pre");
      const hint = document.getElementById("command-detail-hint");
      if (!grid || !pre || !hint) return;

      if (!command) {
        hint.textContent = "Select a command row to inspect the full result.";
        grid.innerHTML = "";
        pre.textContent = "No command selected.";
        return;
      }

      hint.textContent = `${command.commandType} · ${command.commandId}`;
      const detailMetrics = [
        ["Node", command.nodeId],
        ["Status", command.status],
        ["Updated", command.updatedAtUnixMs],
        ["Error", command.error || "—"]
      ];
      grid.innerHTML = detailMetrics.map(([label, value]) => `
        <div class="detail-metric">
          <span>${label}</span>
          <strong>${escapeHtml(value)}</strong>
        </div>
      `).join("");
      pre.textContent = JSON.stringify({
        payload: command.payload,
        result: command.result,
        error: command.error
      }, null, 2);
    }
    async function dispatchNodeCommand() {
      const nodeId = document.getElementById("node-command-node")?.value;
      const commandType = document.getElementById("node-command-type")?.value;
      const payloadRaw = document.getElementById("node-command-payload")?.value || "{}";
      if (!nodeId || !commandType) {
        window.alert("Select a node and command type first.");
        return;
      }

      let payload;
      try {
        payload = payloadRaw.trim() ? JSON.parse(payloadRaw) : {};
      } catch (error) {
        window.alert(`Invalid payload JSON: ${error.message}`);
        return;
      }

      try {
        const response = await postJson(`/api/gateway/control-plane/nodes/${encodeURIComponent(nodeId)}/commands`, {
          commandType,
          payload
        });
        selectedCommandId = response.command.commandId;
        document.getElementById("command-status").innerHTML =
          `Queued <code>${response.command.commandType}</code> for <code>${response.command.nodeId}</code> with delivery <strong>${response.delivery}</strong>.`;
        await refresh(nodeId);
      } catch (error) {
        document.getElementById("command-status").textContent = error.message;
      }
    }
    async function decideApproval(approvalId, kind, decision, draft = {}) {
      try {
        const payload = {
          actor: draft.actor || "console-operator",
          decision
        };
        if (draft.reason) {
          payload.reason = draft.reason;
        }
        if (kind === "payment" && decision === "approve") {
          if (!draft.mcuPublicDid || !draft.mcuSignature) {
            throw new Error("Payment approvals require MCU DID and MCU signature.");
          }
          payload.mcuPublicDid = draft.mcuPublicDid;
          payload.mcuSignature = draft.mcuSignature;
        }
        const response = await postJson(`/api/gateway/approvals/${approvalId}/decision`, payload);
        await refresh();
        await openApprovalDetail(approvalId);
        setDrawerStatus(`Approval <code>${escapeHtml(response.approval.approvalId)}</code> is now <strong>${escapeHtml(response.approval.status)}</strong>.`);
      } catch (error) {
        setDrawerStatus(error.message, true);
      }
    }
    async function refresh(preferredNodeId) {
      const [tasks, nodes, settlements, cards, ingress, approvals, invocations, quotes, reconciliation, marketplaceCatalog, connectorStatus, ingressStatus] = await Promise.all([
        fetchJson("/api/a2a/tasks"),
        fetchJson("/api/gateway/control-plane/nodes"),
        fetchJson("/api/gateway/agent-cards/settlements"),
        fetchJson("/api/gateway/agent-cards/"),
        fetchJson("/api/gateway/ingress/events?limit=8"),
        fetchJson("/api/gateway/approvals?status=pending"),
        fetchJson("/api/gateway/agent-cards/invocations"),
        fetchJson("/api/gateway/agent-cards/quotes"),
        fetchJson("/api/gateway/agent-cards/reconciliation"),
        fetchJson(`/api/gateway/marketplace/catalog?${currentMarketplaceCatalogQuery()}`),
        fetchJson("/api/gateway/connectors/status"),
        fetchJson("/api/gateway/ingress/status")
      ]);
      latestConnectorStatus = connectorStatus;
      latestIngressStatus = ingressStatus;
      const rollouts = await Promise.all(
        nodes.slice(0, 8).map(async (node) => ({
          nodeId: node.nodeId,
          rollout: await fetchJsonOptional(`/api/gateway/control-plane/nodes/${encodeURIComponent(node.nodeId)}/rollout`)
        }))
      );
      const selectedNodeId =
        preferredNodeId ||
        document.getElementById("node-command-node")?.value ||
        nodes[0]?.nodeId ||
        "";
      const commands = selectedNodeId
        ? await fetchJson(`/api/gateway/control-plane/nodes/${encodeURIComponent(selectedNodeId)}/commands`)
        : [];

      document.getElementById("stats").innerHTML = [
        ["Tasks", tasks.length],
        ["Nodes", nodes.length],
        ["Settlements", settlements.length],
        ["Inbound", ingress.length],
        ["Approvals", approvals.length],
        ["Quotes", quotes.length],
        ["Invocations", invocations.length],
        ["Reconciliation", reconciliation.length]
      ].map(([label, value]) => `
        <div class="stat">
          <div class="stat-label">${label}</div>
          <div class="stat-value">${value}</div>
        </div>`).join("");

      document.getElementById("ingress-feed").innerHTML = ingress.map((event) => `
        <article class="feed-item">
          <strong>${event.platform} · ${fmt(event.senderDisplay || event.senderId || event.chatId)}</strong>
          <p>${ellipsis(event.text, 120)}</p>
          <p><code>${event.eventType}</code> · ${event.linkedTaskId ? `task ${event.linkedTaskId}` : "no task yet"} · ${event.status}</p>
        </article>`).join("") || `<div class="tiny">No inbound events yet.</div>`;

      document.getElementById("approval-feed").innerHTML = approvals.map((approval) => `
        <article class="feed-item clickable-card" onclick="openApprovalDetail('${approval.approvalId}')">
          <strong>${approval.title}</strong>
          <p>${ellipsis(approval.summary, 120)}</p>
          <p><code>${approval.kind}</code> · ${approval.referenceId}</p>
          <div class="approval-actions">
            <button type="button" onclick="event.stopPropagation(); openApprovalDetail('${approval.approvalId}')">Open Workflow</button>
            <button type="button" class="secondary" onclick="event.stopPropagation(); openApprovalDetail('${approval.approvalId}')">Inspect</button>
          </div>
        </article>`).join("") || `<div class="tiny">No pending approvals.</div>`;

      document.getElementById("task-rows").innerHTML = tasks.slice(0, 8).map((task) => `
        <tr>
          <td>${ellipsis(task.name, 32)}</td>
          <td>${badge(task.status)}</td>
          <td><code>${ellipsis(task.instruction, 72)}</code></td>
        </tr>`).join("");

      document.getElementById("node-rows").innerHTML = nodes.slice(0, 8).map((node) => `
        <tr>
          <td>${ellipsis(node.displayName || node.nodeId, 28)}</td>
          <td>${badge(node.status)}</td>
          <td>${node.attestationVerified ? badge("trusted") : badge("unverified")}</td>
        </tr>`).join("");

      document.getElementById("rollout-rows").innerHTML = rollouts.map(({ nodeId, rollout }) => `
        <tr class="${rollout ? "interactive-row" : ""}" ${rollout ? `onclick="openRolloutDetail(decodeURIComponent('${encodeURIComponent(nodeId)}'))"` : ""}>
          <td>${ellipsis(nodeId, 28)}</td>
          <td>${rollout ? badge(rollout.status) : `<span class="tiny">none</span>`}</td>
          <td><code>${ellipsis(rollout ? `${rollout.policyVersion} / ${rollout.skillDistributionHash}` : "no rollout", 48)}</code></td>
        </tr>`).join("");

      syncNodeCommandForm(nodes, selectedNodeId);
      syncRolloutConsole(nodes, selectedNodeId);
      syncAgentDelegationForm(cards);
      renderConnectorMatrix(connectorStatus, ingressStatus);
      updateSetupNavigator(document.getElementById("setup-surface")?.value || "model", connectorStatus, ingressStatus);

      const marketplaceSignal = document.getElementById("marketplace-signal");
      if (marketplaceSignal) {
        marketplaceSignal.innerHTML =
          `Catalog exposes <strong>${marketplaceCatalog.skills?.length || 0}</strong> signed skills and <strong>${marketplaceCatalog.agentCards?.length || 0}</strong> published agent cards.`;
      }
      renderMarketplaceCatalog(marketplaceCatalog);

      const visibleCommands = commands.slice(0, 8);
      visibleCommandsCache = visibleCommands;
      if (!selectedCommandId && visibleCommands.length) {
        selectedCommandId = visibleCommands[0].commandId;
      }
      document.getElementById("command-rows").innerHTML = visibleCommands.map((command) => `
        <tr class="command-row ${command.commandId === selectedCommandId ? "active" : ""}" data-command-id="${command.commandId}" onclick="showCommand('${command.commandId}')">
          <td>
            <strong>${command.commandType}</strong><br />
            <code>${command.commandId}</code>
          </td>
          <td>${badge(command.status)}</td>
          <td><code>${ellipsis(JSON.stringify(command.result || command.payload || {}), 96)}</code></td>
        </tr>`).join("") || `<tr><td colspan="3" class="tiny">No commands for the selected node.</td></tr>`;
      renderCommandDetail(visibleCommands.find((command) => command.commandId === selectedCommandId) || visibleCommands[0] || null);

      document.getElementById("settlement-rows").innerHTML = settlements.slice(0, 8).map((settlement) => `
        <tr class="interactive-row" onclick="openSettlementDetail('${settlement.settlementId}')">
          <td>${ellipsis(settlement.cardId, 26)}</td>
          <td>${badge(settlement.status)}</td>
          <td>${settlement.amount}</td>
        </tr>`).join("");

      document.getElementById("reconciliation-rows").innerHTML = reconciliation.slice(0, 8).map((record) => `
        <tr class="interactive-row" onclick="openReconciliationDetail('${record.reconciliationId}')">
          <td>
            <strong>${ellipsis(record.cardId, 24)}</strong><br />
            <code>${ellipsis(record.settlementId, 32)}</code>
          </td>
          <td>${badge(record.reconciliationStatus)}</td>
          <td><code>${ellipsis(record.remoteAgentUrl || record.receiptIssuerDid || "counterparty receipt", 44)}</code></td>
        </tr>`).join("") || `<tr><td colspan="3" class="tiny">No reconciliation records yet.</td></tr>`;

      document.getElementById("invocation-rows").innerHTML = invocations.slice(0, 8).map((invocation) => `
        <tr class="interactive-row" onclick="openInvocationDetail('${invocation.invocationId}')">
          <td>${ellipsis(invocation.cardId, 26)}</td>
          <td>${badge(invocation.status)}</td>
          <td><code>${ellipsis(invocation.remoteTaskId || "pending", 36)}</code></td>
        </tr>`).join("") || `<tr><td colspan="3" class="tiny">No remote invocations yet.</td></tr>`;

      document.getElementById("quote-rows").innerHTML = quotes.slice(0, 8).map((quote) => `
        <tr class="interactive-row" onclick="openQuoteDetail(decodeURIComponent('${encodeURIComponent(quote.quoteId)}'))">
          <td>
            <strong>${ellipsis(quote.cardId, 24)}</strong><br />
            <code>${ellipsis(quote.quoteId, 32)}</code>
          </td>
          <td>${badge(quote.status)}</td>
          <td>${quote.quotedAmount ?? quote.requestedAmount ?? "—"}</td>
        </tr>`).join("") || `<tr><td colspan="3" class="tiny">No quotes in ledger.</td></tr>`;

      document.getElementById("card-rows").innerHTML = cards.slice(0, 8).map((card) => `
        <tr>
          <td>${ellipsis(card.card?.name || card.cardId, 28)}</td>
          <td>${card.locallyHosted ? badge("local") : badge("remote")}</td>
          <td>${ellipsis((card.chatPlatforms || []).join(", "), 42)}</td>
        </tr>`).join("");
    }
    document.addEventListener("input", (event) => {
      if (event.target?.id === "node-command-payload") {
        event.target.dataset.dirty = "true";
      }
    });
    document.addEventListener("change", (event) => {
      if (event.target?.id === "node-command-type") {
        applyCommandTemplate(true);
      }
      if (event.target?.id === "node-command-node") {
        refresh(event.target.value);
      }
      if (event.target?.id === "setup-surface") {
        updateSetupNavigator(event.target.value, latestConnectorStatus, latestIngressStatus);
      }
      if (event.target?.id === "setup-target") {
        updateSetupNavigator(document.getElementById("setup-surface")?.value || "model", latestConnectorStatus, latestIngressStatus);
      }
      if (event.target?.id === "catalog-kind") {
        loadMarketplaceCatalog();
      }
    });
    document.addEventListener("keydown", (event) => {
      if (event.target?.id === "catalog-search" && event.key === "Enter") {
        event.preventDefault();
        loadMarketplaceCatalog();
      }
    });
    refresh();
    connectConsoleStream();
    setInterval(() => refresh().catch((error) => setConsoleStreamStatus("polling", error.message)), 30000);
  </script>
</body>
</html>"##,
    )
}

#[cfg(test)]
mod tests {
    use axum::response::Html;

    use super::dashboard;

    #[tokio::test]
    async fn dashboard_includes_operator_action_studios() {
        let Html(markup) = dashboard().await;
        assert!(markup.contains("Agent Delegation Studio"));
        assert!(markup.contains("delegate-card-id"));
        assert!(markup.contains("previewSettlementQuote"));
        assert!(markup.contains("Rollout Console"));
        assert!(markup.contains("rollout-node-id"));
        assert!(markup.contains("dispatchManualRollout"));
        assert!(markup.contains("Marketplace Studio"));
        assert!(markup.contains("installSkillPackage"));
        assert!(markup.contains("publishAgentCard"));
        assert!(markup.contains("Marketplace Catalog"));
        assert!(markup.contains("marketplace-catalog-grid"));
        assert!(markup.contains("loadMarketplaceCatalog"));
        assert!(markup.contains("Connector Matrix"));
        assert!(markup.contains("connector-matrix"));
        assert!(markup.contains("Setup Navigator"));
        assert!(markup.contains("setup-navigator-status"));
        assert!(markup.contains("copySetupEnvBlock"));
        assert!(markup.contains("Reconciliation Fabric"));
        assert!(markup.contains("reconciliation-rows"));
        assert!(markup.contains("openReconciliationDetail"));
        assert!(markup.contains("reconcileSettlement"));
        assert!(markup.contains("detail-drawer-form"));
        assert!(markup.contains("submitDrawerApproval"));
        assert!(markup.contains("console-stream-pill"));
        assert!(markup.contains("connectConsoleStream"));
        assert!(markup.contains("/console/events"));
    }
}
