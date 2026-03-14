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
    let stream = stream::unfold(
        (Some(initial), state.subscribe_console_events()),
        |(pending, mut receiver)| async move {
            if let Some(event) = pending {
                let payload = serde_json::to_string(&event).unwrap_or_else(|_| {
                    "{\"channel\":\"console\",\"detail\":\"serialization_error\"}".to_string()
                });
                return Some((
                    Ok(Event::default().event("console_update").data(payload)),
                    (None, receiver),
                ));
            }

            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let payload = serde_json::to_string(&event).unwrap_or_else(|_| {
                            "{\"channel\":\"console\",\"detail\":\"serialization_error\"}"
                                .to_string()
                        });
                        return Some((
                            Ok(Event::default().event("console_update").data(payload)),
                            (None, receiver),
                        ));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let payload = serde_json::to_string(&ConsoleStreamEvent {
                            channel: "console".to_string(),
                            entity_id: None,
                            status: Some("lagged".to_string()),
                            detail: format!("skipped {skipped} console updates"),
                            created_at_unix_ms: unix_timestamp_ms(),
                        })
                        .unwrap_or_else(|_| {
                            "{\"channel\":\"console\",\"detail\":\"lagged\"}".to_string()
                        });
                        return Some((
                            Ok(Event::default().event("console_update").data(payload)),
                            (None, receiver),
                        ));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    );

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
  <title>Dawn 网关控制台</title>
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
        <div class="eyebrow">Dawn 网关</div>
        <h1>控制台</h1>
        <div class="subcopy">
          面向入站聊天、A2A 执行、节点信任、AP2 结算与 agent-to-agent 商业链路的液态玻璃运维面板。
        </div>
        <div class="hero-meta">
          <span class="pill">A2A 原生路由</span>
          <span class="pill">AP2 感知支付</span>
          <span class="pill">节点证明</span>
          <span class="pill">中国连接器路径</span>
          <span class="pill" id="console-stream-pill">控制台事件流 · 连接中</span>
        </div>
        <div class="signal-cluster">
          <div class="signal">
            <span>运维模式</span>
            <strong>可信运行时</strong>
          </div>
          <div class="signal">
            <span>控制平面</span>
            <strong>实时派发</strong>
          </div>
          <div class="signal">
            <span>设计语气</span>
            <strong>液态玻璃</strong>
          </div>
        </div>
      </div>
      <div class="hero-card">
        <div class="eyebrow">网关脉冲</div>
        <div class="stats" id="stats"></div>
      </div>
    </section>

    <section class="layout">
      <div class="stack">
        <section class="panel">
          <div class="panel-head">
            <h2>入站聊天流</h2>
            <span class="tiny">最新入口事件</span>
          </div>
          <div class="feed" id="ingress-feed"></div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>审批中心</h2>
            <span class="tiny">待处理的节点与 AP2 审批</span>
          </div>
          <div class="feed" id="approval-feed"></div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>最近任务</h2>
            <span class="tiny">由 A2A 与入站事件创建的任务</span>
          </div>
          <table>
            <thead><tr><th>名称</th><th>状态</th><th>指令</th></tr></thead>
            <tbody id="task-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>远端调用</h2>
            <span class="tiny">Agent Card 执行调用及对应远端任务状态</span>
          </div>
          <table>
            <thead><tr><th>卡片</th><th>状态</th><th>远端任务</th></tr></thead>
            <tbody id="invocation-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>报价账本</h2>
            <span class="tiny">已签名报价轮次、撤销与重放状态</span>
          </div>
          <table>
            <thead><tr><th>报价</th><th>状态</th><th>金额</th></tr></thead>
            <tbody id="quote-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Agent 委托工作台</h2>
            <span class="tiny">预览报价、发起还价并调用远端 Agent Card</span>
          </div>
          <div class="console-form">
            <div class="form-grid">
              <div class="field">
                <label for="delegate-card-id">目标 Agent Card</label>
                <select id="delegate-card-id"></select>
              </div>
              <div class="field">
                <label for="delegate-name">调用名称</label>
                <input id="delegate-name" type="text" value="委托任务" />
              </div>
            </div>
            <div class="field">
              <label for="delegate-instruction">指令</label>
              <textarea id="delegate-instruction">协调一次远端 agent 动作。</textarea>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="delegate-amount">结算金额</label>
                <input id="delegate-amount" type="number" step="0.01" min="0" placeholder="18.50" />
              </div>
              <div class="field">
                <label for="delegate-quote-id">报价 ID</label>
                <input id="delegate-quote-id" type="text" placeholder="quote-..." />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="delegate-counter-offer">还价金额</label>
                <input id="delegate-counter-offer" type="number" step="0.01" min="0" placeholder="15.00" />
              </div>
              <div class="field">
                <label for="delegate-await-completion">等待完成</label>
                <select id="delegate-await-completion">
                  <option value="true" selected>等待远端完成</option>
                  <option value="false">派发后立即返回</option>
                </select>
              </div>
            </div>
            <div class="field">
              <label for="delegate-description">结算说明</label>
              <input id="delegate-description" type="text" value="为远端委托结算" />
            </div>
            <div class="toolbar">
              <button type="button" onclick="previewSettlementQuote(false)">预览报价</button>
              <button type="button" class="secondary" onclick="previewSettlementQuote(true)">发起还价</button>
              <button type="button" class="secondary" onclick="invokeAgentCard()">调用 Agent</button>
            </div>
            <div class="result-box" id="delegate-status">选择一张 Agent Card 以预览报价或发起远端委托。</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>市场工作台</h2>
            <span class="tiny">安装技能包、导入远端卡片并发布本地 Agent Card</span>
          </div>
          <div class="console-form">
            <div class="result-box" id="marketplace-signal">市场目录加载中。</div>
            <div class="form-grid">
              <div class="field">
                <label for="marketplace-skill-package-url">技能包 URL</label>
                <input id="marketplace-skill-package-url" type="url" placeholder="https://gateway.example/api/gateway/skills/echo-skill/1.0.0/package" />
              </div>
              <div class="field">
                <label for="marketplace-card-url">远端 Agent Card URL</label>
                <input id="marketplace-card-url" type="url" placeholder="https://agent.example/.well-known/agent-card.json" />
              </div>
            </div>
            <div class="toolbar">
              <button type="button" onclick="installSkillPackage()">安装技能</button>
              <button type="button" class="secondary" onclick="importAgentCardFromUrl()">导入卡片</button>
              <button type="button" class="secondary" onclick="window.open('/marketplace', '_blank')">打开市场页</button>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-id">本地卡片 ID</label>
                <input id="publish-card-id" type="text" placeholder="travel-agent-cn" />
              </div>
              <div class="field">
                <label for="publish-card-regions">区域</label>
                <input id="publish-card-regions" type="text" value="china" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-languages">语言</label>
                <input id="publish-card-languages" type="text" value="zh-cn,en" />
              </div>
              <div class="field">
                <label for="publish-card-model-providers">模型提供方</label>
                <input id="publish-card-model-providers" type="text" value="qwen,deepseek" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-chat-platforms">聊天平台</label>
                <input id="publish-card-chat-platforms" type="text" value="wechat_official_account,feishu,dingtalk" />
              </div>
              <div class="field">
                <label for="publish-card-payment-roles">支付角色</label>
                <input id="publish-card-payment-roles" type="text" value="payee" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="publish-card-local">是否本地托管</label>
                <select id="publish-card-local">
                  <option value="true" selected>本地托管</option>
                  <option value="false">仅远端元数据</option>
                </select>
              </div>
              <div class="field">
                <label for="publish-card-published">可见性</label>
                <select id="publish-card-published">
                  <option value="true" selected>已发布</option>
                  <option value="false">草稿</option>
                </select>
              </div>
            </div>
            <div class="field">
              <label for="publish-card-json">Agent Card JSON</label>
              <textarea id="publish-card-json">{
  "name": "Dawn 中国出行 Agent",
  "description": "为中国市场任务协调预订、消息编排与 AP2 结算。",
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
        "name": "出行规划",
        "description": "规划并委托行程动作。"
    }
  ]
}</textarea>
            </div>
            <div class="toolbar">
              <button type="button" onclick="publishAgentCard()">发布 Agent Card</button>
            </div>
            <div class="result-box" id="marketplace-status">在这里安装、导入或发布市场资产。</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>市场目录</h2>
            <span class="tiny">无需离开运维面板即可浏览已签名技能与已发布 Agent</span>
          </div>
          <div class="console-form">
            <div class="toolbar">
              <input id="catalog-search" type="search" placeholder="搜索技能、Agent、能力或平台" />
              <select id="catalog-kind">
                <option value="">全部</option>
                <option value="skill">技能</option>
                <option value="agent">Agent</option>
              </select>
              <button type="button" class="secondary" onclick="loadMarketplaceCatalog()">搜索目录</button>
            </div>
            <div class="result-box" id="marketplace-catalog-status">目录浏览器加载中。</div>
            <div class="catalog-grid" id="marketplace-catalog-grid"></div>
          </div>
        </section>
      </div>

      <div class="stack">
        <section class="panel">
          <div class="panel-head">
            <h2>节点</h2>
            <span class="tiny">会话与证明状态</span>
          </div>
          <table>
            <thead><tr><th>节点</th><th>状态</th><th>信任</th></tr></thead>
            <tbody id="node-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>发布链路</h2>
            <span class="tiny">每个节点的最新签名策略与技能分发状态</span>
          </div>
          <table>
            <thead><tr><th>节点</th><th>状态</th><th>策略 / 技能</th></tr></thead>
            <tbody id="rollout-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>发布控制台</h2>
            <span class="tiny">手动派发当前签名策略与技能包</span>
          </div>
          <div class="console-form">
            <div class="field">
              <label for="rollout-node-id">目标节点</label>
              <select id="rollout-node-id"></select>
            </div>
            <div class="toolbar">
              <button type="button" onclick="dispatchManualRollout()">派发发布包</button>
              <button type="button" class="secondary" onclick="refresh(document.getElementById('rollout-node-id')?.value)">刷新节点</button>
            </div>
            <div class="result-box" id="rollout-status">选择一个节点以推送当前发布包。</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>节点命令台</h2>
            <span class="tiny">派发已证明的文件、进程与发现类命令</span>
          </div>
          <div class="console-form">
            <div class="field">
              <label for="node-command-node">目标节点</label>
              <select id="node-command-node"></select>
            </div>
            <div class="field">
              <label for="node-command-type">命令模板</label>
              <select id="node-command-type"></select>
            </div>
            <div class="chip-row" id="command-template-chips"></div>
            <div class="field">
              <label for="node-command-payload">Payload JSON</label>
              <textarea id="node-command-payload"></textarea>
            </div>
            <div class="toolbar">
              <button type="button" onclick="dispatchNodeCommand()">派发命令</button>
              <button type="button" class="secondary" onclick="applyCommandTemplate(true)">重置载荷</button>
              <span class="tiny">`shell_exec` 仍然受审批与策略门控约束。</span>
            </div>
            <div class="result-box" id="command-status">选择一个节点并派发命令。</div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>最近节点命令</h2>
            <span class="tiny">当前所选节点的最新执行结果</span>
          </div>
          <table>
            <thead><tr><th>命令</th><th>状态</th><th>载荷 / 结果</th></tr></thead>
            <tbody id="command-rows"></tbody>
          </table>
          <div class="detail-card" style="margin-top:14px;">
            <div class="panel-head" style="margin-bottom:12px;">
              <h2>命令详情</h2>
              <span class="tiny" id="command-detail-hint">选择一条命令记录以查看完整结果。</span>
            </div>
            <div class="detail-grid" id="command-detail-grid"></div>
            <pre class="detail-pre" id="command-detail-pre">尚未选择命令。</pre>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>结算</h2>
            <span class="tiny">远端 Agent 结算活动</span>
          </div>
          <table>
            <thead><tr><th>卡片</th><th>状态</th><th>金额</th></tr></thead>
            <tbody id="settlement-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>对账链路</h2>
            <span class="tiny">跨网关结算回执投递与确认</span>
          </div>
          <table>
            <thead><tr><th>结算</th><th>状态</th><th>对端</th></tr></thead>
            <tbody id="reconciliation-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>投递 Outbox</h2>
            <span class="tiny">面向结算回执与报价状态推送的持久化网关投递链</span>
          </div>
          <div class="detail-grid" id="delivery-outbox-summary-grid"></div>
          <div class="result-box" id="delivery-outbox-status">正在等待 outbox 遥测数据。</div>
          <div class="toolbar">
            <button type="button" onclick="replayDeliveryOutboxDeadLetters()">批量重放死信</button>
            <button type="button" class="secondary" onclick="refresh()">刷新 Outbox</button>
          </div>
          <table>
            <thead><tr><th>链路</th><th>状态</th><th>尝试次数</th><th>下一动作</th><th>操作</th></tr></thead>
            <tbody id="delivery-outbox-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>Agent Cards</h2>
            <span class="tiny">注册面概览</span>
          </div>
          <table>
            <thead><tr><th>卡片</th><th>托管方式</th><th>信号</th></tr></thead>
            <tbody id="card-rows"></tbody>
          </table>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>连接器矩阵</h2>
            <span class="tiny">覆盖全球与中国路径的模型、聊天与入口就绪度</span>
          </div>
          <div class="detail-grid" id="connector-summary-grid"></div>
          <div class="feed" id="connector-matrix" style="margin-top:14px;"></div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>部署导航</h2>
            <span class="tiny">把连接器与入口就绪度收敛成可执行的上线清单</span>
          </div>
          <div class="console-form">
            <div class="setup-grid" id="setup-summary-grid"></div>
            <div class="chip-row" id="setup-preset-chips"></div>
            <div class="form-grid">
              <div class="field">
                <label for="setup-surface">面向层</label>
                <select id="setup-surface">
                  <option value="model">模型连接器</option>
                  <option value="chat">聊天连接器</option>
                  <option value="ingress">入口路由</option>
                </select>
              </div>
              <div class="field">
                <label for="setup-target">目标</label>
                <select id="setup-target"></select>
              </div>
            </div>
            <div class="result-box" id="setup-navigator-status">选择一个面向层以查看所需密钥、测试路径与下一步。</div>
            <div class="feed" id="setup-guidance-feed"></div>
            <div class="feed" id="setup-receipt-feed"></div>
            <div class="setup-requirements" id="setup-requirements"></div>
            <div class="field">
              <label for="setup-env-block">建议环境变量块</label>
              <textarea id="setup-env-block" readonly></textarea>
            </div>
            <div class="toolbar">
              <button type="button" onclick="verifySetupTarget()">验证目标</button>
              <button type="button" onclick="copySetupEnvBlock()">复制环境变量</button>
              <button type="button" class="secondary" onclick="loadSetupPreset('china')">中国上线</button>
              <button type="button" class="secondary" onclick="loadSetupPreset('global')">全球 MVP</button>
              <button type="button" class="secondary" onclick="loadSetupPreset('ingress')">入口优先</button>
            </div>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>身份与入驻</h2>
            <span class="tiny">初始化操作员会话、工作区上下文与首次节点认领</span>
          </div>
          <div class="console-form">
            <div class="result-box" id="identity-status">先初始化操作员会话，再配置工作区身份与节点入驻。</div>
            <div class="setup-grid" id="identity-readiness-grid"></div>
            <div class="feed" id="identity-next-steps"></div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-bootstrap-token">初始化令牌</label>
                <input id="identity-bootstrap-token" type="text" value="dawn-dev-bootstrap" />
              </div>
              <div class="field">
                <label for="identity-operator-name">操作员名称</label>
                <input id="identity-operator-name" type="text" value="console-operator" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-session-token">操作员会话令牌</label>
                <input id="identity-session-token" type="text" placeholder="先初始化生成，或粘贴已有会话令牌" />
              </div>
              <div class="field">
                <label for="identity-workspace-status">入驻状态</label>
                <input id="identity-workspace-status" type="text" value="bootstrap_pending" />
              </div>
            </div>
            <div class="toolbar">
              <button type="button" onclick="bootstrapOperatorSession()">初始化会话</button>
              <button type="button" class="secondary" onclick="clearOperatorSession()">清除会话</button>
              <span class="tiny" id="identity-session-pill">当前没有操作员会话</span>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-workspace-name">工作区名称</label>
                <input id="identity-workspace-name" type="text" value="Dawn Agent Commerce" />
              </div>
              <div class="field">
                <label for="identity-region">区域</label>
                <input id="identity-region" type="text" value="global" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-tenant-id">租户 ID</label>
                <input id="identity-tenant-id" type="text" value="dawn-labs" />
              </div>
              <div class="field">
                <label for="identity-project-id">项目 ID</label>
                <input id="identity-project-id" type="text" value="agent-commerce" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-model-providers">默认模型提供方</label>
                <input id="identity-model-providers" type="text" value="deepseek,qwen" />
              </div>
              <div class="field">
                <label for="identity-chat-platforms">默认聊天平台</label>
                <input id="identity-chat-platforms" type="text" value="feishu,wechat_official_account" />
              </div>
            </div>
            <div class="toolbar">
              <button type="button" onclick="saveWorkspaceProfile()">保存工作区资料</button>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-claim-node-id">节点 ID</label>
                <input id="identity-claim-node-id" type="text" value="node-cn-edge-01" />
              </div>
              <div class="field">
                <label for="identity-claim-display-name">节点显示名</label>
                <input id="identity-claim-display-name" type="text" value="上海边缘节点" />
              </div>
            </div>
            <div class="form-grid">
              <div class="field">
                <label for="identity-claim-transport">传输方式</label>
                <input id="identity-claim-transport" type="text" value="websocket" />
              </div>
              <div class="field">
                <label for="identity-claim-expiry">过期秒数</label>
                <input id="identity-claim-expiry" type="number" min="60" step="60" value="1800" />
              </div>
            </div>
            <div class="field">
              <label for="identity-claim-capabilities">请求能力</label>
              <input id="identity-claim-capabilities" type="text" value="system_info,process_snapshot,list_directory,read_file_preview,stat_path" />
            </div>
            <div class="toolbar">
              <button type="button" onclick="issueNodeClaim()">签发节点认领</button>
            </div>
            <div class="result-box" id="identity-claim-output">签发后的节点认领与启动 URL 会显示在这里。</div>
            <div class="toolbar">
              <button type="button" class="secondary" onclick="copyLatestNodeClaimBundle('launchUrl')">复制启动 URL</button>
              <button type="button" class="secondary" onclick="copyLatestNodeClaimBundle('claimToken')">复制认领令牌</button>
              <button type="button" class="secondary" onclick="copyLatestNodeClaimBundle('envBlock')">复制环境变量</button>
            </div>
            <div class="feed" id="identity-claim-feed"></div>
            <div class="feed" id="identity-claim-history"></div>
          </div>
        </section>
      </div>
    </section>
  </div>
  <div class="drawer-backdrop" id="detail-drawer-backdrop" onclick="closeDetailDrawer()"></div>
  <aside class="detail-drawer" id="detail-drawer">
    <div class="drawer-head">
      <div>
        <div class="eyebrow" id="detail-drawer-eyebrow">检查器</div>
        <h2 class="drawer-title" id="detail-drawer-title">详情抽屉</h2>
        <div class="drawer-subtitle" id="detail-drawer-subtitle">选择一条审批、命令、结算、对账或投递记录。</div>
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
    let latestIdentityStatus = null;
    let latestDeliveryOutboxSummary = null;
    let latestDeliveryOutbox = [];
    let latestSetupVerificationReceipts = [];
    let latestNodeClaimAuditEvents = [];
    let latestNodeClaimBundle = null;
    let currentOperatorSessionToken = (() => {
      try {
        return window.localStorage.getItem("dawnOperatorSessionToken") || "";
      } catch (_error) {
        return "";
      }
    })();
    const commandTemplates = {
      agent_ping: {},
      list_capabilities: {},
      browser_start: { sessionId: "browser-managed-1", url: "https://example.com", profileName: "ops", persistProfile: true, approvalRequired: true },
      browser_profiles: { approvalRequired: true },
      browser_profile_delete: { profileName: "ops", approvalRequired: true },
      browser_status: { sessionId: "browser-managed-1", approvalRequired: true },
      browser_stop: { sessionId: "browser-managed-1", approvalRequired: true },
      browser_navigate: { sessionId: "browser-default", url: "https://example.com", managed: true, approvalRequired: true },
      browser_new_tab: { sessionId: "browser-default", newSessionId: "browser-default-tab-1", url: "https://example.com/docs", approvalRequired: true },
      browser_new_window: { sessionId: "browser-default", newSessionId: "browser-default-window-1", url: "https://example.com/dashboard", approvalRequired: true },
      browser_extract: { sessionId: "browser-default", selector: "main", limit: 3, limitChars: 600, approvalRequired: true },
      browser_click: { sessionId: "browser-default", selector: "a", elementIndex: 0, approvalRequired: true },
      browser_back: { sessionId: "browser-default", approvalRequired: true },
      browser_forward: { sessionId: "browser-default", approvalRequired: true },
      browser_reload: { sessionId: "browser-default", approvalRequired: true },
      browser_focus: { sessionId: "browser-default", approvalRequired: true },
      browser_close: { sessionId: "browser-default", approvalRequired: true },
      browser_tabs: { sessionId: "browser-default", approvalRequired: true },
      browser_snapshot: { sessionId: "browser-default", limit: 5, limitChars: 600, approvalRequired: true },
      browser_screenshot: { sessionId: "browser-default", fullPage: true, path: "artifacts/browser/demo.png", approvalRequired: true },
      browser_pdf: { sessionId: "browser-default", landscape: false, printBackground: true, path: "artifacts/browser/demo.pdf", approvalRequired: true },
      browser_console_messages: { sessionId: "browser-default", limit: 50, approvalRequired: true },
      browser_network_requests: { sessionId: "browser-default", limit: 25, approvalRequired: true },
      browser_trace: { sessionId: "browser-default", limit: 100, approvalRequired: true },
      browser_trace_export: { sessionId: "browser-default", limit: 200, path: "artifacts/browser/trace.json", approvalRequired: true },
      browser_errors: { sessionId: "browser-default", consoleLimit: 50, networkLimit: 50, approvalRequired: true },
      browser_cookies: { sessionId: "browser-default", limit: 25, approvalRequired: true },
      browser_storage: { sessionId: "browser-default", limit: 50, approvalRequired: true },
      browser_storage_set: { sessionId: "browser-default", storageArea: "localStorage", key: "theme", value: "light", approvalRequired: true },
      browser_set_headers: { sessionId: "browser-default", headers: { "x-dawn-profile": "demo" }, reload: false, approvalRequired: true },
      browser_set_offline: { sessionId: "browser-default", offline: true, latencyMs: 150, reload: false, approvalRequired: true },
      browser_set_geolocation: { sessionId: "browser-default", latitude: 40.7128, longitude: -74.0060, accuracy: 25, reload: false, approvalRequired: true },
      browser_emulate_device: { sessionId: "browser-default", preset: "iphone-13", reload: true, approvalRequired: true },
      browser_evaluate: { sessionId: "browser-default", expression: "document.title", returnByValue: true, refresh: false, approvalRequired: true },
      browser_wait_for: { sessionId: "browser-default", selector: "#results", timeoutMs: 10000, pollMs: 250, approvalRequired: true },
      browser_handle_dialog: { sessionId: "browser-default", accept: true, promptText: "dawn", approvalRequired: true },
      browser_press_key: { sessionId: "browser-default", key: "Enter", approvalRequired: true },
      browser_type: { sessionId: "browser-default", selector: "input[name=q]", text: "dawn browser control", submit: false, approvalRequired: true },
      browser_upload: { sessionId: "browser-default", selector: "input[type=file]", path: "uploads/demo.txt", submit: false, approvalRequired: true },
      browser_download: { sessionId: "browser-default", selector: "a", elementIndex: 0, path: "downloads/result.bin", approvalRequired: true },
      browser_form_fill: { sessionId: "browser-default", formSelector: "form", fields: { q: "dawn" }, approvalRequired: true },
      browser_form_submit: { sessionId: "browser-default", formSelector: "form", fields: { q: "openclaw parity" }, approvalRequired: true },
      browser_open: { url: "https://example.com", approvalRequired: true },
      browser_search: { query: "Dawn browser control MVP", engine: "google", approvalRequired: true },
      desktop_open: { target: "notepad", args: [], approvalRequired: true },
      desktop_clipboard_set: { text: "hello from dawn", approvalRequired: true },
      desktop_type_text: { text: "hello from dawn", delayMs: 250, approvalRequired: true },
      desktop_key_press: { keys: "CTRL+L", delayMs: 150, approvalRequired: true },
      desktop_windows_list: { limit: 10, approvalRequired: true },
      desktop_window_focus: { title: "Notepad", approvalRequired: true },
      desktop_wait_for_window: { processName: "notepad", timeoutMs: 8000, pollMs: 250, approvalRequired: true },
      desktop_focus_app: { processName: "notepad", approvalRequired: true },
      desktop_launch_and_focus: { target: "notepad", processName: "notepad", timeoutMs: 10000, approvalRequired: true },
      desktop_mouse_move: { x: 400, y: 300, approvalRequired: true },
      desktop_mouse_click: { button: "left", x: 400, y: 300, doubleClick: false, approvalRequired: true },
      desktop_screenshot: { approvalRequired: true },
      desktop_ocr: { x: 0, y: 0, width: 800, height: 600, backend: "tesseract", language: "eng", keepImage: false, approvalRequired: true },
      desktop_accessibility_query: { processName: "notepad", controlType: "edit", className: "RichEditD2DPT", matchMode: "contains", preferVisible: true, preferEnabled: true, limit: 5, approvalRequired: true },
      desktop_accessibility_click: { processName: "notepad", controlType: "button", name: "OK", matchMode: "exact", preferVisible: true, preferEnabled: true, elementIndex: 0, button: "left", doubleClick: false, approvalRequired: true },
      desktop_accessibility_wait_for: { processName: "notepad", controlType: "edit", className: "RichEditD2DPT", preferVisible: true, timeoutMs: 8000, pollMs: 250, approvalRequired: true },
      desktop_accessibility_fill: { processName: "notepad", controlType: "edit", className: "RichEditD2DPT", matchMode: "contains", preferVisible: true, preferEnabled: true, elementIndex: 0, value: "hello from dawn", clearExisting: true, submit: false, approvalRequired: true },
      desktop_accessibility_workflow: { processName: "notepad", className: "RichEditD2DPT", preferVisible: true, preferEnabled: true, steps: [{ kind: "wait_for", controlType: "edit", timeoutMs: 8000 }, { kind: "fill", controlType: "edit", value: "hello from dawn", clearExisting: true }, { kind: "sleep", durationMs: 300 }], approvalRequired: true },
      desktop_accessibility_snapshot: { processName: "notepad", depth: 2, childrenLimit: 20, approvalRequired: true },
      desktop_accessibility_focus: { processName: "notepad", controlType: "edit", className: "RichEditD2DPT", matchMode: "contains", preferVisible: true, preferEnabled: true, approvalRequired: true },
      desktop_accessibility_invoke: { processName: "notepad", controlType: "button", name: "Close", matchMode: "exact", preferVisible: true, preferEnabled: true, approvalRequired: true },
      desktop_accessibility_set_value: { processName: "notepad", controlType: "edit", className: "RichEditD2DPT", matchMode: "contains", preferVisible: true, preferEnabled: true, value: "hello from dawn", approvalRequired: true },
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
      browser_start: "Launch a fresh managed Chromium or Edge process, optionally backed by a reusable local profile directory",
      browser_profiles: "List the managed browser profiles currently saved on disk and any tracked sessions using them",
      browser_profile_delete: "Delete a saved managed browser profile directory when it is not currently in use by tracked sessions",
      browser_status: "Inspect one managed browser process, including its tracked Dawn sessions and live CDP targets",
      browser_stop: "Stop a managed browser process and remove every tracked Dawn session that shares it",
      browser_navigate: "Fetch a browser page into a lightweight HTTP session, or launch a visible managed browser tab when payload.managed=true",
      browser_new_tab: "Open a new managed browser tab inside the same controlled Chromium instance and track it as a new session",
      browser_new_window: "Open a new managed browser window inside the same controlled Chromium instance and track it as a new session",
      browser_extract: "Extract text or links from the stored browser session DOM",
      browser_click: "Follow a link from the stored DOM session, or perform a live DOM click in a managed browser session",
      browser_back: "Return to the previous page in browser session history",
      browser_forward: "Advance to the next page in browser session forward history",
      browser_reload: "Reload the current page while preserving the tracked browser session",
      browser_focus: "Make one tracked browser session the active tab and bring managed tabs to the foreground when possible",
      browser_close: "Close a tracked browser session and promote the next active tab",
      browser_tabs: "List the currently tracked browser sessions, including managed tabs",
      browser_snapshot: "Summarize headings, links, forms, and pending fields for a browser session; managed sessions refresh from the live browser first",
      browser_screenshot: "Capture a PNG screenshot from a managed browser session",
      browser_pdf: "Render the current managed browser tab to a PDF file on local storage",
      browser_console_messages: "Read recent console, error, and unhandled rejection messages captured from a managed browser session",
      browser_network_requests: "List recent managed browser fetch/XHR captures plus navigation and resource timing entries",
      browser_trace: "Inspect a time-ordered trace that merges managed-browser network lifecycle events with console activity",
      browser_trace_export: "Export the merged managed-browser trace to a local JSON file for later analysis or evidence capture",
      browser_errors: "Aggregate console errors plus failed or suspicious managed-browser network activity into one view",
      browser_cookies: "Inspect the cookies currently visible to one managed browser session",
      browser_storage: "Inspect localStorage and sessionStorage entries for one managed browser session",
      browser_storage_set: "Set or remove a localStorage/sessionStorage key inside one managed browser session",
      browser_set_headers: "Apply extra HTTP headers to one managed browser tab and optionally reload it",
      browser_set_offline: "Apply offline or throttled network conditions to one managed browser tab",
      browser_set_geolocation: "Override the reported geolocation for one managed browser tab",
      browser_emulate_device: "Apply a desktop or mobile device-emulation profile to one managed browser tab",
      browser_evaluate: "Run JavaScript inside a managed browser session and optionally refresh the stored DOM snapshot",
      browser_wait_for: "Poll a managed browser session until a selector appears, text appears, or text disappears",
      browser_handle_dialog: "Accept or dismiss a blocking JavaScript dialog in a managed browser session",
      browser_press_key: "Send a key or shortcut such as Enter or Control+L into a managed browser session",
      browser_type: "Stage typed text for an HTTP browser session, or drive a live input/select element in a managed browser session",
      browser_upload: "Stage a local file against a file input, or drive a live managed browser file input and optionally submit its form",
        browser_download: "Download a URL or link target through the HTTP or managed browser session cookie jar to local storage",
      browser_form_fill: "Stage named form field values inside the browser session",
      browser_form_submit: "Submit a DOM form through the browser session HTTP client",
      browser_open: "Open an HTTP(S) URL in the default browser",
      browser_search: "Launch a browser search via the default browser",
      desktop_open: "Open a local app, file, folder, or URL on the host desktop",
      desktop_clipboard_set: "Write text into the host clipboard",
      desktop_type_text: "Type text into the currently focused desktop window",
      desktop_key_press: "Send a keyboard shortcut to the currently focused desktop window",
      desktop_windows_list: "Enumerate visible desktop windows for later focus and input",
      desktop_window_focus: "Focus a visible desktop window by title or handle",
      desktop_wait_for_window: "Poll until a visible desktop window appears",
      desktop_focus_app: "Focus the first visible window for a named process",
      desktop_launch_and_focus: "Launch an app and wait until its window becomes focusable",
      desktop_mouse_move: "Move the desktop pointer to screen coordinates",
      desktop_mouse_click: "Send a mouse click, optionally after moving to coordinates",
      desktop_screenshot: "Capture a desktop screenshot to a PNG file on the host machine",
      desktop_ocr: "Run local OCR against an existing image or a captured desktop region; currently uses a local Tesseract backend when available",
      desktop_accessibility_query: "Search a Windows accessibility tree, rank matching nodes, and return their bounds and metadata",
      desktop_accessibility_click: "Find the best-ranked Windows accessibility node and click the center of its bounding rectangle",
      desktop_accessibility_wait_for: "Poll until a ranked Windows accessibility node matching the selector appears",
      desktop_accessibility_fill: "Fill the best-ranked Windows accessibility node using ValuePattern first, then fall back to focus plus keyboard input",
      desktop_accessibility_workflow: "Run a multi-step desktop accessibility workflow such as ranked waits, fill, click, key press, OCR, or short sleeps",
      desktop_accessibility_snapshot: "Read a focused desktop window through the Windows accessibility tree",
      desktop_accessibility_focus: "Find the best-ranked Windows accessibility node and move focus to it",
      desktop_accessibility_invoke: "Find the best-ranked Windows accessibility node and trigger an invoke-capable pattern",
      desktop_accessibility_set_value: "Find the best-ranked Windows accessibility node with ValuePattern and write text into it",
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
    const formatTimestamp = (value) => {
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed <= 0) return "—";
      try {
        return new Date(parsed).toLocaleString();
      } catch (_error) {
        return String(value);
      }
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
    const humanizeToken = (value) => String(value ?? "")
      .split("_")
      .filter(Boolean)
      .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
      .join(" ");
    const badge = (value) => {
      const normalized = String(value || "").toLowerCase();
      const tone = /complete|authorized|connected|acknowledged|task_created|configured|trusted|local|ready|approved|session_created|workspace_updated/.test(normalized)
        ? "ok"
        : /failed|rejected|disconnected|expired|revoked/.test(normalized)
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
      nodeSelect.innerHTML = nodeOptions || `<option value="">暂无节点</option>`;
      if (selectedNodeId && nodes.some((node) => node.nodeId === selectedNodeId)) {
        nodeSelect.value = selectedNodeId;
      }

      const selectedNode = nodes.find((node) => node.nodeId === nodeSelect.value) || nodes[0];
      const commandTypes = commandOptionsForNode(selectedNode);
      const currentCommand = commandSelect.value;
      commandSelect.innerHTML = commandTypes.map((commandType) =>
        `<option value="${commandType}">${commandType}</option>`
      ).join("") || `<option value="">暂无可用命令</option>`;
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
      rolloutSelect.innerHTML = options || `<option value="">暂无节点</option>`;
      if (selectedNodeId && nodes.some((node) => node.nodeId === selectedNodeId)) {
        rolloutSelect.value = selectedNodeId;
      }
    }
    function syncAgentDelegationForm(cards) {
      const cardSelect = document.getElementById("delegate-card-id");
      if (!cardSelect) return;
      const selectedCardId = cardSelect.value;
      const options = cards.map((card) => {
        const label = `${card.card?.name || card.cardId} · ${card.locallyHosted ? "本地" : "远端"}`;
        return `<option value="${escapeHtml(card.cardId)}">${escapeHtml(label)}</option>`;
      }).join("");
      cardSelect.innerHTML = options || `<option value="">暂无卡片</option>`;
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
    function setOperatorSessionToken(token) {
      const normalized = String(token || "").trim();
      currentOperatorSessionToken = normalized;
      try {
        if (normalized) {
          window.localStorage.setItem("dawnOperatorSessionToken", normalized);
        } else {
          window.localStorage.removeItem("dawnOperatorSessionToken");
        }
      } catch (_error) {}
      const tokenInput = document.getElementById("identity-session-token");
      const pill = document.getElementById("identity-session-pill");
      if (tokenInput) tokenInput.value = normalized;
      if (pill) pill.textContent = normalized ? "Operator session ready" : "No operator session";
      return normalized;
    }
    function readOperatorSessionToken() {
      const tokenInput = document.getElementById("identity-session-token");
      return String(tokenInput?.value || currentOperatorSessionToken || "").trim();
    }
    function renderReadinessFeed(items, emptyMessage) {
      if (!Array.isArray(items) || items.length === 0) {
        return `<div class="tiny">${escapeHtml(emptyMessage)}</div>`;
      }
      return items.map((item) => {
        const actionLine = item.action
          ? `<p><strong>Next:</strong> ${escapeHtml(item.action)}</p>`
          : "";
        const setupButton = item.surface && item.target
          ? `<button type="button" class="secondary" onclick='focusSetupTarget(${JSON.stringify(item.surface)}, ${JSON.stringify(item.target)})'>Open In Setup Navigator</button>`
          : "";
        return `
          <article class="feed-item">
            <strong>${escapeHtml(item.label || item.key || "step")}</strong>
            <p>${badge(item.status || "action_required")} · ${escapeHtml(item.detail || "Action required")}</p>
            ${actionLine}
            ${setupButton ? `<div class="approval-actions">${setupButton}</div>` : ""}
          </article>
        `;
      }).join("");
    }
    function focusSetupTarget(surface, target) {
      const surfaceSelect = document.getElementById("setup-surface");
      const targetSelect = document.getElementById("setup-target");
      if (!surfaceSelect || !targetSelect || !surface) return;
      surfaceSelect.value = surface;
      updateSetupNavigator(surface, latestConnectorStatus, latestIngressStatus, latestIdentityStatus, latestSetupVerificationReceipts);
      if (target) {
        targetSelect.value = target;
      }
      updateSetupNavigator(surfaceSelect.value, latestConnectorStatus, latestIngressStatus, latestIdentityStatus, latestSetupVerificationReceipts);
      document.getElementById("setup-navigator-status")?.scrollIntoView({ behavior: "smooth", block: "center" });
    }
    function rememberNodeClaimBundle(bundle) {
      latestNodeClaimBundle = bundle ? {
        claimId: bundle.claim?.claimId || "",
        nodeId: bundle.claim?.nodeId || "",
        displayName: bundle.claim?.displayName || "",
        claimToken: bundle.claimToken || "",
        tokenHint: bundle.tokenHint || "",
        sessionUrl: bundle.sessionUrl || "",
        launchUrl: bundle.launchUrl || "",
        reissuedFromClaimId: bundle.reissuedFromClaimId || null
      } : null;
      return latestNodeClaimBundle;
    }
    function renderNodeClaimBundle(bundle) {
      if (!bundle) {
        return "Issue a node claim to mint a first-connect URL.";
      }
      const header = bundle.reissuedFromClaimId
        ? `${badge("claim_reissued")} Reissued claim <code>${escapeHtml(bundle.claimId)}</code> for <strong>${escapeHtml(bundle.nodeId)}</strong> from <code>${escapeHtml(bundle.reissuedFromClaimId)}</code>.`
        : `${badge("claim_issued")} Issued claim <code>${escapeHtml(bundle.claimId)}</code> for <strong>${escapeHtml(bundle.nodeId)}</strong>.`;
      return `${header}<br /><code>${escapeHtml(bundle.launchUrl || bundle.sessionUrl)}</code><br /><code>DAWN_NODE_CLAIM_TOKEN=${escapeHtml(bundle.claimToken)}</code><br /><span class="tiny">token hint ${escapeHtml(bundle.tokenHint || "—")}</span>`;
    }
    async function copyLatestNodeClaimBundle(kind) {
      const output = document.getElementById("identity-claim-output");
      if (!latestNodeClaimBundle) {
        if (output) output.textContent = "Issue or reissue a node claim first.";
        return;
      }
      const value = kind === "launchUrl"
        ? latestNodeClaimBundle.launchUrl
        : kind === "claimToken"
          ? latestNodeClaimBundle.claimToken
          : `DAWN_NODE_CLAIM_TOKEN=${latestNodeClaimBundle.claimToken}\nDAWN_NODE_LAUNCH_URL=${latestNodeClaimBundle.launchUrl}`;
      try {
        await navigator.clipboard.writeText(value);
        if (output) {
          output.dataset.touched = "true";
          output.innerHTML = `${renderNodeClaimBundle(latestNodeClaimBundle)}<br /><span class="tiny">${escapeHtml(kind)} copied to clipboard.</span>`;
        }
      } catch (_error) {
        if (output) {
          output.dataset.touched = "true";
          output.innerHTML = `${renderNodeClaimBundle(latestNodeClaimBundle)}<br /><span class="tiny">Clipboard write failed.</span>`;
        }
      }
    }
    async function verifySetupTarget() {
      const sessionToken = readOperatorSessionToken();
      const current = currentSetupProfile();
      const status = document.getElementById("setup-navigator-status");
      if (!sessionToken) {
        window.alert("Bootstrap or paste an operator session token first.");
        return;
      }
      if (!current.profile || !current.surface || !current.target) {
        if (status) status.textContent = "Choose a surface and target before running verification.";
        return;
      }
      try {
        const response = await postJson("/api/gateway/identity/setup-verifications", {
          sessionToken,
          surface: current.surface,
          target: current.target
        });
        if (status) {
          status.innerHTML = `${badge(response.receipt?.status || "ready")} ${escapeHtml(response.receipt?.summary || "Verification recorded.")}`;
        }
        await refresh();
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function revokeNodeClaim(claimId, nodeId) {
      const sessionToken = readOperatorSessionToken();
      const output = document.getElementById("identity-claim-output");
      if (!sessionToken) {
        window.alert("Bootstrap or paste an operator session token first.");
        return;
      }
      try {
        const response = await postJson(`/api/gateway/identity/node-claims/${encodeURIComponent(claimId)}/revoke`, {
          sessionToken,
          reason: "revoked from control center"
        });
        if (output) {
          output.dataset.touched = "true";
          output.innerHTML = `${badge("revoked")} Revoked claim <code>${escapeHtml(response.claim.claimId)}</code> for <strong>${escapeHtml(response.claim.nodeId)}</strong>.`;
        }
        await refresh(nodeId);
      } catch (error) {
        if (output) {
          output.dataset.touched = "true";
          output.textContent = error.message;
        }
      }
    }
    async function reissueNodeClaim(claimId, nodeId) {
      const sessionToken = readOperatorSessionToken();
      const output = document.getElementById("identity-claim-output");
      if (!sessionToken) {
        window.alert("Bootstrap or paste an operator session token first.");
        return;
      }
      try {
        const response = await postJson(`/api/gateway/identity/node-claims/${encodeURIComponent(claimId)}/reissue`, {
          sessionToken,
          expiresInSeconds: Number(document.getElementById("identity-claim-expiry")?.value || 1800)
        });
        const bundle = rememberNodeClaimBundle(response);
        if (output) {
          output.dataset.touched = "true";
          output.innerHTML = renderNodeClaimBundle(bundle);
        }
        await refresh(nodeId);
      } catch (error) {
        if (output) {
          output.dataset.touched = "true";
          output.textContent = error.message;
        }
      }
    }
    function renderSetupReceiptFeed(surface, target, receipts) {
      const matching = (receipts || []).filter((receipt) =>
        receipt.surface === surface && receipt.target === target
      ).slice(0, 3);
      return matching.map((receipt) => `
        <article class="feed-item">
          <strong>${escapeHtml(receipt.label || `${surface}:${target}`)}</strong>
          <p>${badge(receipt.status || "action_required")} · ${escapeHtml(receipt.summary || receipt.detail || "Verification recorded")}</p>
          <p>${escapeHtml(receipt.detail || "No detail")} · ${escapeHtml(formatTimestamp(receipt.createdAtUnixMs))}</p>
        </article>
      `).join("") || `<div class="tiny">No verification receipts recorded for this target yet.</div>`;
    }
    function renderNodeClaimAuditFeed(events) {
      return (events || []).slice(0, 6).map((event) => `
        <article class="feed-item">
          <strong>${escapeHtml(event.nodeId || "node claim")}</strong>
          <p>${badge(event.eventType || "event")} · ${escapeHtml(event.detail || "audit event")}</p>
          <p>${escapeHtml(event.actor || "system")} · ${escapeHtml(formatTimestamp(event.createdAtUnixMs))}${event.tokenHint ? ` · token ${escapeHtml(event.tokenHint)}` : ""}</p>
        </article>
      `).join("") || `<div class="tiny">No node-claim audit history yet.</div>`;
    }
    function syncIdentityStudio(identityStatus, identitySessions, identityClaims, nodes, nodeClaimAuditEvents) {
      const workspace = identityStatus?.workspace || {};
      const readiness = identityStatus?.readiness || {};
      const metrics = readiness.metrics || {};
      const status = document.getElementById("identity-status");
      const readinessGrid = document.getElementById("identity-readiness-grid");
      const nextSteps = document.getElementById("identity-next-steps");
      const claimFeed = document.getElementById("identity-claim-feed");
      const claimHistory = document.getElementById("identity-claim-history");
      const claimOutput = document.getElementById("identity-claim-output");
      const nodeIndex = new Map((nodes || []).map((node) => [node.nodeId, node]));
      const sessionToken = setOperatorSessionToken(readOperatorSessionToken());
      const activeSessions = Array.isArray(identitySessions)
        ? identitySessions.filter((session) => !session.revoked).length
        : 0;
      if (status) {
        status.innerHTML = `${badge(readiness.overallStatus || identityStatus?.bootstrapMode || "bootstrap")} <strong>${escapeHtml(workspace.displayName || "Workspace")}</strong> · ${escapeHtml(workspace.region || "global")} · sessions <strong>${activeSessions}</strong> · pending claims <strong>${identityStatus?.pendingNodeClaims ?? 0}</strong><br /><span class="tiny">${escapeHtml(readiness.nextStep || "Continue the onboarding checklist below.")}</span>`;
      }
      if (readinessGrid) {
        readinessGrid.innerHTML = [
          {
            label: "Completion",
            value: `${readiness.completionPercent ?? 0}%`,
            note: `${readiness.readySteps ?? 0}/${readiness.totalSteps ?? 0} checklist items ready`
          },
          {
            label: "Trusted Nodes",
            value: `${metrics.trustedNodes ?? 0}/${metrics.connectedNodes ?? 0}`,
            note: "verified / connected"
          },
          {
            label: "Approval Path",
            value: metrics.publicBaseUrlConfigured ? "public" : "local-only",
            note: `${metrics.pendingEndUserSessions ?? 0} live end-user sessions`
          }
        ].map((card) => `
          <article class="setup-card">
            <span>${escapeHtml(card.label)}</span>
            <strong>${escapeHtml(card.value)}</strong>
            <p>${escapeHtml(card.note)}</p>
          </article>
        `).join("");
      }
      if (nextSteps) {
        nextSteps.innerHTML = renderReadinessFeed(
          (readiness.checklist || []).filter((item) => item.status !== "ready").slice(0, 4),
          "Identity checklist is green. Bootstrap, workspace, and node onboarding all look ready."
        );
      }
      if (claimOutput && !claimOutput.dataset.touched) {
        claimOutput.innerHTML = sessionToken
          ? `Operator session is stored locally. Issue a node claim to mint a first-connect URL.`
          : `Bootstrap an operator session first, then issue a node claim for a new Dawn node.`;
      }
      const assignValue = (id, value) => {
        const element = document.getElementById(id);
        if (!element || element.dataset.dirty === "true") return;
        element.value = value ?? "";
      };
      assignValue("identity-workspace-name", workspace.displayName || "Dawn Agent Commerce");
      assignValue("identity-region", workspace.region || "global");
      assignValue("identity-tenant-id", workspace.tenantId || "dawn-labs");
      assignValue("identity-project-id", workspace.projectId || "agent-commerce");
      assignValue("identity-model-providers", (workspace.defaultModelProviders || ["deepseek", "qwen"]).join(","));
      assignValue("identity-chat-platforms", (workspace.defaultChatPlatforms || ["feishu", "wechat_official_account"]).join(","));
      assignValue("identity-workspace-status", workspace.onboardingStatus || "bootstrap_pending");

      if (claimFeed) {
        claimFeed.innerHTML = (identityClaims || []).slice(0, 6).map((claim) => {
          const node = nodeIndex.get(claim.nodeId);
          const nodeStatus = node
            ? (node.attestationVerified ? badge("trusted") : badge(node.connected ? "connected" : node.status || "registered"))
            : `<span class="tiny">awaiting first connect</span>`;
          const actions = [];
          if (claim.status === "pending" && sessionToken) {
            actions.push(`<button type="button" class="secondary" onclick="revokeNodeClaim('${claim.claimId}', '${claim.nodeId}')">Revoke Claim</button>`);
          }
          if (claim.status !== "consumed" && sessionToken) {
            actions.push(`<button type="button" class="secondary" onclick="reissueNodeClaim('${claim.claimId}', '${claim.nodeId}')">Reissue Claim</button>`);
          }
          return `
            <article class="feed-item">
              <strong>${escapeHtml(claim.displayName || claim.nodeId)}</strong>
              <p><code>${escapeHtml(claim.nodeId)}</code> · ${badge(claim.status)} · ${nodeStatus}</p>
              <p>expires ${escapeHtml(formatTimestamp(claim.expiresAtUnixMs))}</p>
              <p>${escapeHtml((claim.requestedCapabilities || []).join(", ") || "no declared capabilities")}</p>
              ${actions.length ? `<div class="approval-actions">${actions.join("")}</div>` : ""}
            </article>
          `;
        }).join("") || `<div class="tiny">No node claims issued yet.</div>`;
      }
      if (claimHistory) {
        claimHistory.innerHTML = renderNodeClaimAuditFeed(nodeClaimAuditEvents);
      }
    }
    async function bootstrapOperatorSession() {
      const bootstrapToken = document.getElementById("identity-bootstrap-token")?.value?.trim();
      const operatorName = document.getElementById("identity-operator-name")?.value?.trim();
      const status = document.getElementById("identity-status");
      if (!bootstrapToken || !operatorName) {
        window.alert("Bootstrap token and operator name are required.");
        return;
      }
      try {
        const response = await postJson("/api/gateway/identity/bootstrap/session", {
          bootstrapToken,
          operatorName
        });
        setOperatorSessionToken(response.sessionToken);
        if (status) {
          status.innerHTML = `${badge("session_created")} Operator <strong>${escapeHtml(response.session.operatorName)}</strong> bootstrapped a session.`;
        }
        await refresh();
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    function clearOperatorSession() {
      setOperatorSessionToken("");
      latestNodeClaimBundle = null;
      const output = document.getElementById("identity-claim-output");
      if (output) {
        output.dataset.touched = "";
        output.textContent = "Operator session cleared. Bootstrap again to issue new node claims.";
      }
    }
    async function saveWorkspaceProfile() {
      const sessionToken = readOperatorSessionToken();
      const status = document.getElementById("identity-status");
      if (!sessionToken) {
        window.alert("Bootstrap or paste an operator session token first.");
        return;
      }
      try {
        const response = await fetch("/api/gateway/identity/workspace", {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            sessionToken,
            tenantId: document.getElementById("identity-tenant-id")?.value?.trim(),
            projectId: document.getElementById("identity-project-id")?.value?.trim(),
            displayName: document.getElementById("identity-workspace-name")?.value?.trim(),
            region: document.getElementById("identity-region")?.value?.trim(),
            defaultModelProviders: splitList(document.getElementById("identity-model-providers")?.value),
            defaultChatPlatforms: splitList(document.getElementById("identity-chat-platforms")?.value),
            onboardingStatus: document.getElementById("identity-workspace-status")?.value?.trim() || "configured"
          })
        });
        if (!response.ok) {
          const payload = await response.json().catch(() => ({}));
          throw new Error(payload.error || `workspace update failed (${response.status})`);
        }
        const payload = await response.json();
        if (status) {
          status.innerHTML = `${badge("workspace_updated")} Workspace <strong>${escapeHtml(payload.workspace.displayName)}</strong> updated by ${escapeHtml(payload.actor)}.`;
        }
        await refresh();
      } catch (error) {
        if (status) status.textContent = error.message;
      }
    }
    async function issueNodeClaim() {
      const sessionToken = readOperatorSessionToken();
      const output = document.getElementById("identity-claim-output");
      if (!sessionToken) {
        window.alert("Bootstrap or paste an operator session token first.");
        return;
      }
      try {
        const response = await postJson("/api/gateway/identity/node-claims", {
          sessionToken,
          nodeId: document.getElementById("identity-claim-node-id")?.value?.trim(),
          displayName: document.getElementById("identity-claim-display-name")?.value?.trim(),
          transport: document.getElementById("identity-claim-transport")?.value?.trim() || "websocket",
          requestedCapabilities: splitList(document.getElementById("identity-claim-capabilities")?.value),
          expiresInSeconds: Number(document.getElementById("identity-claim-expiry")?.value || 1800)
        });
        const bundle = rememberNodeClaimBundle(response);
        if (output) {
          output.dataset.touched = "true";
          output.innerHTML = renderNodeClaimBundle(bundle);
        }
        await refresh(response.claim.nodeId);
      } catch (error) {
        if (output) {
          output.dataset.touched = "true";
          output.textContent = error.message;
        }
      }
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
    function updateSetupNavigator(surface, connectorStatus, ingressStatus, identityStatus = latestIdentityStatus, setupReceipts = latestSetupVerificationReceipts) {
      const surfaceSelect = document.getElementById("setup-surface");
      const targetSelect = document.getElementById("setup-target");
      const summaryGrid = document.getElementById("setup-summary-grid");
      const requirements = document.getElementById("setup-requirements");
      const status = document.getElementById("setup-navigator-status");
      const envBlock = document.getElementById("setup-env-block");
      const presetChips = document.getElementById("setup-preset-chips");
      const guidanceFeed = document.getElementById("setup-guidance-feed");
      const receiptFeed = document.getElementById("setup-receipt-feed");
      if (!surfaceSelect || !targetSelect || !summaryGrid || !requirements || !status || !envBlock || !presetChips || !guidanceFeed || !receiptFeed) return;

      if (surface && surfaceSelect.value !== surface) {
        surfaceSelect.value = surface;
      }

      const options = surfaceOptions(surfaceSelect.value, connectorStatus, ingressStatus);
      const previousTarget = targetSelect.value;
      targetSelect.innerHTML = options.map((option) =>
        `<option value="${escapeHtml(option.key)}">${escapeHtml(option.label)} · ${escapeHtml(option.region)}</option>`
      ).join("") || `<option value="">暂无目标</option>`;
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
      const readinessItems = (identityStatus?.readiness?.checklist || []).filter((item) => {
        if (!item?.surface) {
          return surfaceSelect.value === "ingress" && item.key === "public_gateway_url";
        }
        return item.surface === surfaceSelect.value;
      });
      const pendingReadinessItems = readinessItems.filter((item) => item.status !== "ready");
      const nextGap = readinessItems.find((item) => item.status !== "ready");
      const targetReceipts = (setupReceipts || []).filter((receipt) =>
        receipt.surface === surfaceSelect.value && receipt.target === (option?.key || targetSelect.value)
      );
      const latestReceipt = targetReceipts[0] || null;
      summaryGrid.innerHTML = [
        {
          label: "已配置",
          value: `${configuredCount}/${options.length || 0}`,
          note: "当前已就绪目标"
        },
        {
          label: "中国路径",
          value: `${regionCount}`,
          note: "国内连接器或路由"
        },
        {
          label: "面向层",
          value: surfaceSelect.value,
          note: "当前部署视角"
        },
        {
          label: "下一缺口",
          value: nextGap ? nextGap.label : "已覆盖",
          note: nextGap ? nextGap.status : "该面向层默认路径已接通"
        },
        {
          label: "最近验证",
          value: latestReceipt ? latestReceipt.status : "无",
          note: latestReceipt ? formatTimestamp(latestReceipt.createdAtUnixMs) : "运行“验证目标”以保存回执"
        }
      ].map((card) => `
        <article class="setup-card">
          <span>${escapeHtml(card.label)}</span>
          <strong>${escapeHtml(card.value)}</strong>
          <p>${escapeHtml(card.note)}</p>
        </article>
      `).join("");

      presetChips.innerHTML = [
        ["china", "中国上线"],
        ["global", "全球 MVP"],
        ["ingress", "入口优先"]
      ].map(([preset, label]) => `
        <button type="button" class="chip" onclick="loadSetupPreset('${preset}')">${escapeHtml(label)}</button>
      `).join("");
      guidanceFeed.innerHTML = renderReadinessFeed(
        (pendingReadinessItems.length ? pendingReadinessItems : readinessItems).slice(0, 3),
        "当前这个面向层没有映射到新的入驻阻塞项。"
      );
      receiptFeed.innerHTML = renderSetupReceiptFeed(surfaceSelect.value, option?.key || targetSelect.value, setupReceipts);

      if (!profile || !option) {
        requirements.innerHTML = `<div class="setup-requirement"><strong>尚未选择目标</strong><p>选择面向层与目标后，这里会显示部署说明。</p></div>`;
        status.textContent = "选择一个面向层以查看所需密钥、测试路径与下一步。";
        envBlock.value = "";
        receiptFeed.innerHTML = `<div class="tiny">这个目标还没有验证回执。</div>`;
        return;
      }

      status.innerHTML = `${option.configured ? badge("configured") : badge("missing")} <strong>${escapeHtml(profile.label)}</strong> · ${escapeHtml(profile.region)} · ${escapeHtml(profile.mode)} · test via <code>${escapeHtml(profile.endpoint)}</code>${latestReceipt ? `<br /><span class="tiny">Last verification: ${escapeHtml(formatTimestamp(latestReceipt.createdAtUnixMs))} · ${escapeHtml(latestReceipt.summary || latestReceipt.status)}</span>` : ""}${nextGap ? `<br /><span class="tiny">${escapeHtml(nextGap.action || nextGap.detail)}</span>` : ""}`;
      requirements.innerHTML = [
        {
          title: "密钥 / 环境变量",
          body: profile.envs.join(", ")
        },
        {
          title: "启用路径",
          body: profile.note
        },
        {
          title: "验证方式",
          body: `设置变量后，在连接器矩阵里确认就绪状态，并测试 ${profile.endpoint}。`
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
      updateSetupNavigator(next.surface, latestConnectorStatus, latestIngressStatus, latestIdentityStatus, latestSetupVerificationReceipts);
      targetSelect.value = next.target;
      updateSetupNavigator(next.surface, latestConnectorStatus, latestIngressStatus, latestIdentityStatus, latestSetupVerificationReceipts);
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
    function deliveryOutboxPriority(record) {
      const normalized = String(record?.status || "");
      if (normalized === "failed") return 0;
      if (normalized === "retry_scheduled") return 1;
      if (normalized === "queued") return 2;
      return 3;
    }
    function renderDeliveryOutboxPanel(summary, records) {
      const summaryGrid = document.getElementById("delivery-outbox-summary-grid");
      const status = document.getElementById("delivery-outbox-status");
      const rows = document.getElementById("delivery-outbox-rows");
      if (!summaryGrid || !status || !rows) return;

      const pendingCount = summary?.pendingCount ?? 0;
      const failedCount = summary?.failedCount ?? 0;
      const deliveredCount = summary?.deliveredCount ?? 0;
      summaryGrid.innerHTML = renderDrawerMetrics([
        ["待处理", pendingCount],
        ["失败", failedCount],
        ["已送达", deliveredCount],
        ["回执", summary?.settlementReceiptCount ?? 0],
        ["报价状态", summary?.quoteStateCount ?? 0],
        ["下次重试", summary?.nextAttemptAtUnixMs ? formatTimestamp(summary.nextAttemptAtUnixMs) : "—"]
      ]);
      if (failedCount > 0) {
        status.innerHTML = `${badge("failed")} 有 <strong>${failedCount}</strong> 条投递记录需要操作员处理。${summary?.lastFailureAtUnixMs ? ` 最近一次终态失败：<code>${escapeHtml(formatTimestamp(summary.lastFailureAtUnixMs))}</code>。` : ""}`;
      } else if (pendingCount > 0) {
        status.innerHTML = `${badge("retrying")} Outbox 当前有 <strong>${pendingCount}</strong> 条排队或重试中的投递。${summary?.oldestPendingCreatedAtUnixMs ? ` 最早待处理项创建于 <code>${escapeHtml(formatTimestamp(summary.oldestPendingCreatedAtUnixMs))}</code>。` : ""}`;
      } else {
        status.innerHTML = `${badge("ready")} 当前没有待处理或失败的投递记录。`;
      }

      const visible = [...(records || [])]
        .sort((left, right) => {
          const priorityDelta = deliveryOutboxPriority(left) - deliveryOutboxPriority(right);
          if (priorityDelta !== 0) return priorityDelta;
          return (Number(right.updatedAtUnixMs) || 0) - (Number(left.updatedAtUnixMs) || 0);
        })
        .slice(0, 8);
      rows.innerHTML = visible.map((record) => {
        const routeId = record.quoteId || record.settlementId || record.deliveryId;
        const nextAction = record.status === "delivered"
          ? (record.deliveredAtUnixMs ? `已送达 ${formatTimestamp(record.deliveredAtUnixMs)}` : "已送达")
          : (record.nextAttemptAtUnixMs ? `重试于 ${formatTimestamp(record.nextAttemptAtUnixMs)}` : "等待中");
        return `
          <tr class="interactive-row" onclick="openDeliveryOutboxDetail('${record.deliveryId}')">
            <td>
              <strong>${escapeHtml(humanizeToken(record.deliveryKind))}</strong>
              <div class="tiny"><code>${escapeHtml(routeId)}</code></div>
            </td>
            <td>${badge(record.status)}</td>
            <td><code>${escapeHtml(`${record.attemptCount}/${record.maxAttempts}`)}</code></td>
            <td>${escapeHtml(nextAction)}</td>
            <td>
              <button type="button" class="secondary" onclick="event.stopPropagation(); openDeliveryOutboxDetail('${record.deliveryId}')">查看</button>
              ${record.status !== "delivered"
                ? `<button type="button" onclick="event.stopPropagation(); retryDeliveryOutbox('${record.deliveryId}', false)">重试</button>`
                : ""}
            </td>
          </tr>`;
      }).join("") || `<tr><td colspan="5" class="tiny">当前还没有 delivery outbox 记录。</td></tr>`;
    }
    async function openDeliveryOutboxDetail(deliveryId) {
      try {
        const delivery = await fetchJson(`/api/gateway/agent-cards/delivery-outbox/${encodeURIComponent(deliveryId)}`);
        openDetailDrawer({
          kind: "deliveryOutbox",
          delivery
        });
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function retryDeliveryOutbox(deliveryId, reopen = true) {
      const panelStatus = document.getElementById("delivery-outbox-status");
      try {
        const delivery = await postJson(`/api/gateway/agent-cards/delivery-outbox/${encodeURIComponent(deliveryId)}/retry`, {});
        await refresh();
        if (panelStatus) {
          panelStatus.innerHTML = `${badge("queued")} 投递 <code>${escapeHtml(delivery.deliveryId)}</code> 已重新入队并立即处理。`;
        }
        if (reopen) {
          openDetailDrawer({
            kind: "deliveryOutbox",
            delivery
          });
        }
      } catch (error) {
        if (panelStatus) panelStatus.textContent = error.message;
        if (reopen) setDrawerStatus(error.message, true);
      }
    }
    async function replayDeliveryOutboxDeadLetters() {
      const panelStatus = document.getElementById("delivery-outbox-status");
      try {
        const response = await postJson("/api/gateway/agent-cards/delivery-outbox-dead-letter/replay", { limit: 12 });
        await refresh();
        if (panelStatus) {
          panelStatus.innerHTML = `${badge("queued")} 已重放 <strong>${escapeHtml(response.replayedCount ?? 0)}</strong> / <strong>${escapeHtml(response.matchedCount ?? 0)}</strong> 条死信记录。`;
        }
      } catch (error) {
        if (panelStatus) panelStatus.textContent = error.message;
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
          <label for="approval-reason">原因说明</label>
          <textarea id="approval-reason" placeholder="记录这次操作应当放行或拒绝的原因。">${escapeHtml(defaultReason)}</textarea>
        </div>
        <div class="toolbar">
          <button type="button" onclick="submitDrawerApproval('approve')">批准</button>
          <button type="button" class="secondary" onclick="submitDrawerApproval('reject')">拒绝</button>
          <button type="button" class="secondary" onclick="openApprovalDetail('${approval.approvalId}')">重新加载</button>
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
        || `<article class="catalog-card"><strong>没有匹配的市场结果</strong><div class="catalog-meta">请放宽搜索条件，或切换目录类型筛选。</div></article>`;
    }
    async function loadMarketplaceCatalog() {
      const status = document.getElementById("marketplace-catalog-status");
      try {
        if (status) status.textContent = "正在刷新市场目录…";
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
        eyebrow.textContent = "检查器";
        title.textContent = "详情抽屉";
        subtitle.textContent = "选择一条审批、命令、结算、对账或投递记录。";
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

      if (state.kind === "deliveryOutbox") {
        const delivery = state.delivery;
        eyebrow.textContent = "投递 Outbox";
        title.textContent = delivery.quoteId || delivery.settlementId || delivery.deliveryId;
        subtitle.textContent = `${humanizeToken(delivery.deliveryKind)} · ${delivery.cardId}`;
        meta.innerHTML = renderDrawerMetrics([
          ["状态", delivery.status],
          ["尝试次数", `${delivery.attemptCount}/${delivery.maxAttempts}`],
          ["目标", delivery.targetUrl],
          ["HTTP", delivery.lastHttpStatus || "—"],
          ["下次重试", delivery.status === "delivered"
            ? (delivery.deliveredAtUnixMs ? formatTimestamp(delivery.deliveredAtUnixMs) : "—")
            : (delivery.nextAttemptAtUnixMs ? formatTimestamp(delivery.nextAttemptAtUnixMs) : "—")]
        ]);
        actions.innerHTML = renderDrawerActions([
          {
            label: "刷新",
            onclick: `openDeliveryOutboxDetail('${delivery.deliveryId}')`,
            secondary: true
          },
          ...(delivery.status !== "delivered"
            ? [{
                label: "立即重试",
                onclick: `retryDeliveryOutbox('${delivery.deliveryId}')`
              }]
            : []),
          ...(delivery.reconciliationId
            ? [{
                label: "回执账本",
                onclick: `openReconciliationDetail('${delivery.reconciliationId}')`,
                secondary: true
              }]
            : []),
          ...(delivery.settlementId
            ? [{
                label: "结算",
                onclick: `openSettlementDetail('${delivery.settlementId}')`,
                secondary: true
              }]
            : []),
          ...(delivery.quoteId
            ? [{
                label: "报价账本",
                onclick: `openQuoteDetail(decodeURIComponent('${encodeURIComponent(delivery.quoteId)}'))`,
                secondary: true
              }]
            : [])
        ]);
        pre.textContent = formatJsonBlock(delivery);
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
        hint.textContent = "选择一条命令记录以查看完整结果。";
        grid.innerHTML = "";
        pre.textContent = "尚未选择命令。";
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
      const [tasks, nodes, settlements, cards, ingress, approvals, invocations, quotes, reconciliation, deliveryOutboxSummary, deliveryOutbox, marketplaceCatalog, connectorStatus, ingressStatus, identityStatus, identitySessions, identityClaims, setupVerificationReceipts, nodeClaimAuditEvents] = await Promise.all([
        fetchJson("/api/a2a/tasks"),
        fetchJson("/api/gateway/control-plane/nodes"),
        fetchJson("/api/gateway/agent-cards/settlements"),
        fetchJson("/api/gateway/agent-cards/"),
        fetchJson("/api/gateway/ingress/events?limit=8"),
        fetchJson("/api/gateway/approvals?status=pending"),
        fetchJson("/api/gateway/agent-cards/invocations"),
        fetchJson("/api/gateway/agent-cards/quotes"),
        fetchJson("/api/gateway/agent-cards/reconciliation"),
        fetchJson("/api/gateway/agent-cards/delivery-outbox-summary"),
        fetchJson("/api/gateway/agent-cards/delivery-outbox?limit=24"),
        fetchJson(`/api/gateway/marketplace/catalog?${currentMarketplaceCatalogQuery()}`),
        fetchJson("/api/gateway/connectors/status"),
        fetchJson("/api/gateway/ingress/status"),
        fetchJson("/api/gateway/identity/status"),
        fetchJson("/api/gateway/identity/sessions"),
        fetchJson("/api/gateway/identity/node-claims"),
        fetchJson("/api/gateway/identity/setup-verifications?limit=12"),
        fetchJson("/api/gateway/identity/node-claim-events?limit=12")
      ]);
      latestConnectorStatus = connectorStatus;
      latestIngressStatus = ingressStatus;
      latestIdentityStatus = identityStatus;
      latestDeliveryOutboxSummary = deliveryOutboxSummary;
      latestDeliveryOutbox.splice(0, latestDeliveryOutbox.length, ...(deliveryOutbox || []));
      latestSetupVerificationReceipts.splice(0, latestSetupVerificationReceipts.length, ...(setupVerificationReceipts || []));
      latestNodeClaimAuditEvents.splice(0, latestNodeClaimAuditEvents.length, ...(nodeClaimAuditEvents || []));
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
        ["Reconciliation", reconciliation.length],
        ["Outbox Failures", deliveryOutboxSummary.failedCount ?? 0],
        ["Sessions", identityStatus.activeSessions ?? identitySessions.length],
        ["Node Claims", identityClaims.length]
      ].map(([label, value]) => `
        <div class="stat">
          <div class="stat-label">${label}</div>
          <div class="stat-value">${value}</div>
        </div>`).join("");

      document.getElementById("ingress-feed").innerHTML = ingress.map((event) => `
        <article class="feed-item">
          <strong>${event.platform} · ${fmt(event.senderDisplay || event.senderId || event.chatId)}</strong>
          <p>${ellipsis(event.text, 120)}</p>
          <p><code>${event.eventType}</code> · ${event.linkedTaskId ? `任务 ${event.linkedTaskId}` : "尚未关联任务"} · ${event.status}</p>
        </article>`).join("") || `<div class="tiny">当前还没有入站事件。</div>`;

      document.getElementById("approval-feed").innerHTML = approvals.map((approval) => `
        <article class="feed-item clickable-card" onclick="openApprovalDetail('${approval.approvalId}')">
          <strong>${approval.title}</strong>
          <p>${ellipsis(approval.summary, 120)}</p>
          <p><code>${approval.kind}</code> · ${approval.referenceId}</p>
          <div class="approval-actions">
            <button type="button" onclick="event.stopPropagation(); openApprovalDetail('${approval.approvalId}')">打开流程</button>
            <button type="button" class="secondary" onclick="event.stopPropagation(); openApprovalDetail('${approval.approvalId}')">查看</button>
          </div>
        </article>`).join("") || `<div class="tiny">当前没有待处理审批。</div>`;

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
      syncIdentityStudio(identityStatus, identitySessions, identityClaims, nodes, latestNodeClaimAuditEvents);
      renderConnectorMatrix(connectorStatus, ingressStatus);
      renderDeliveryOutboxPanel(deliveryOutboxSummary, deliveryOutbox);
      updateSetupNavigator(document.getElementById("setup-surface")?.value || "model", connectorStatus, ingressStatus, identityStatus, latestSetupVerificationReceipts);

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
      if ([
        "identity-workspace-name",
        "identity-region",
        "identity-tenant-id",
        "identity-project-id",
        "identity-model-providers",
        "identity-chat-platforms",
        "identity-workspace-status"
      ].includes(event.target?.id)) {
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
        assert!(markup.contains("Agent 委托工作台"));
        assert!(markup.contains("delegate-card-id"));
        assert!(markup.contains("previewSettlementQuote"));
        assert!(markup.contains("发布控制台"));
        assert!(markup.contains("rollout-node-id"));
        assert!(markup.contains("dispatchManualRollout"));
        assert!(markup.contains("市场工作台"));
        assert!(markup.contains("installSkillPackage"));
        assert!(markup.contains("publishAgentCard"));
        assert!(markup.contains("市场目录"));
        assert!(markup.contains("marketplace-catalog-grid"));
        assert!(markup.contains("loadMarketplaceCatalog"));
        assert!(markup.contains("连接器矩阵"));
        assert!(markup.contains("connector-matrix"));
        assert!(markup.contains("部署导航"));
        assert!(markup.contains("setup-navigator-status"));
        assert!(markup.contains("copySetupEnvBlock"));
        assert!(markup.contains("身份与入驻"));
        assert!(markup.contains("bootstrapOperatorSession"));
        assert!(markup.contains("saveWorkspaceProfile"));
        assert!(markup.contains("issueNodeClaim"));
        assert!(markup.contains("identity-session-token"));
        assert!(markup.contains("对账链路"));
        assert!(markup.contains("reconciliation-rows"));
        assert!(markup.contains("openReconciliationDetail"));
        assert!(markup.contains("reconcileSettlement"));
        assert!(markup.contains("投递 Outbox"));
        assert!(markup.contains("delivery-outbox-rows"));
        assert!(markup.contains("openDeliveryOutboxDetail"));
        assert!(markup.contains("retryDeliveryOutbox"));
        assert!(markup.contains("replayDeliveryOutboxDeadLetters"));
        assert!(markup.contains("detail-drawer-form"));
        assert!(markup.contains("submitDrawerApproval"));
        assert!(markup.contains("console-stream-pill"));
        assert!(markup.contains("connectConsoleStream"));
        assert!(markup.contains("/console/events"));
    }
}
