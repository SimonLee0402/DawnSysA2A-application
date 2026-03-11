use std::sync::Arc;

use axum::{Router, response::Html, routing::get};

use crate::app_state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(dashboard))
}

async fn dashboard() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Dawn Control Center</title>
  <style>
    :root {
      --bg: #0e141b;
      --panel: rgba(19, 28, 37, 0.92);
      --panel-strong: rgba(28, 40, 51, 0.98);
      --border: rgba(132, 175, 199, 0.18);
      --text: #f2f5f7;
      --muted: #9fb5c2;
      --accent: #e8b949;
      --accent-2: #53b3cb;
      --danger: #ff8266;
      --success: #64d2a3;
      --shadow: 0 28px 80px rgba(0, 0, 0, 0.38);
      --radius: 22px;
      --font: "Segoe UI Variable", "Segoe UI", "Noto Sans", sans-serif;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      font-family: var(--font);
      color: var(--text);
      background:
        radial-gradient(circle at top left, rgba(83, 179, 203, 0.22), transparent 36%),
        radial-gradient(circle at top right, rgba(232, 185, 73, 0.2), transparent 34%),
        linear-gradient(160deg, #071018 0%, #0c1620 45%, #111a23 100%);
    }
    .shell {
      max-width: 1480px;
      margin: 0 auto;
      padding: 28px;
    }
    .hero {
      display: grid;
      grid-template-columns: 1.2fr 0.8fr;
      gap: 18px;
      margin-bottom: 18px;
    }
    .hero-card, .panel {
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: var(--radius);
      box-shadow: var(--shadow);
      backdrop-filter: blur(16px);
    }
    .hero-card {
      padding: 26px;
      position: relative;
      overflow: hidden;
    }
    .hero-card::after {
      content: "";
      position: absolute;
      inset: auto -20% -36% 36%;
      height: 220px;
      background: radial-gradient(circle, rgba(83, 179, 203, 0.18), transparent 60%);
      pointer-events: none;
    }
    .eyebrow {
      letter-spacing: 0.18em;
      text-transform: uppercase;
      font-size: 11px;
      color: var(--accent);
      margin-bottom: 10px;
    }
    h1 {
      margin: 0 0 10px 0;
      font-size: clamp(32px, 4vw, 52px);
      line-height: 0.94;
      max-width: 10ch;
    }
    .subcopy {
      color: var(--muted);
      max-width: 64ch;
      line-height: 1.6;
      margin-bottom: 18px;
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
      background: rgba(255, 255, 255, 0.04);
      border: 1px solid rgba(255, 255, 255, 0.08);
      color: var(--muted);
      font-size: 13px;
    }
    .stats {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 12px;
      padding: 20px;
    }
    .stat {
      background: var(--panel-strong);
      border: 1px solid var(--border);
      border-radius: 18px;
      padding: 18px;
    }
    .stat-label {
      color: var(--muted);
      font-size: 13px;
      margin-bottom: 10px;
    }
    .stat-value {
      font-size: 34px;
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
      font-size: 18px;
      letter-spacing: -0.03em;
    }
    .tiny {
      color: var(--muted);
      font-size: 12px;
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
      background: rgba(255, 255, 255, 0.03);
      border: 1px solid rgba(255, 255, 255, 0.05);
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
      border-radius: 16px;
      border: 1px solid rgba(255, 255, 255, 0.08);
      background: rgba(255, 255, 255, 0.04);
      color: var(--text);
      font: inherit;
      padding: 12px 14px;
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
    .result-box {
      padding: 14px;
      border-radius: 16px;
      background: rgba(255, 255, 255, 0.03);
      border: 1px solid rgba(255, 255, 255, 0.05);
      color: var(--muted);
      line-height: 1.6;
      min-height: 52px;
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
      background: var(--accent);
    }
    button.secondary {
      color: var(--text);
      background: rgba(255, 255, 255, 0.08);
      border: 1px solid rgba(255, 255, 255, 0.08);
    }
    code {
      font-family: ui-monospace, "Cascadia Code", monospace;
      color: #f8d67c;
      font-size: 12px;
    }
    @media (max-width: 1080px) {
      .hero, .layout { grid-template-columns: 1fr; }
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
          Operational view across inbound chat traffic, A2A tasks, node trust state, AP2 settlements, and agent-card activity.
        </div>
        <div class="hero-meta">
          <span class="pill">A2A-native routing</span>
          <span class="pill">AP2-aware payments</span>
          <span class="pill">Node attestation</span>
          <span class="pill">China connector path</span>
        </div>
      </div>
      <div class="hero-card">
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
            <h2>Agent Cards</h2>
            <span class="tiny">Registry footprint</span>
          </div>
          <table>
            <thead><tr><th>Card</th><th>Hosted</th><th>Signals</th></tr></thead>
            <tbody id="card-rows"></tbody>
          </table>
        </section>
      </div>
    </section>
  </div>

  <script>
    const fmt = (value) => value ?? "—";
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
    const ellipsis = (value, max = 66) => {
      if (!value) return "—";
      return value.length > max ? `${value.slice(0, max)}…` : value;
    };
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
      applyCommandTemplate(false);
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
        document.getElementById("command-status").innerHTML =
          `Queued <code>${response.command.commandType}</code> for <code>${response.command.nodeId}</code> with delivery <strong>${response.delivery}</strong>.`;
        await refresh(nodeId);
      } catch (error) {
        document.getElementById("command-status").textContent = error.message;
      }
    }
    async function decideApproval(approvalId, kind, decision) {
      try {
        if (kind === "payment" && decision === "approve") {
          const mcuPublicDid = window.prompt("MCU public DID");
          if (!mcuPublicDid) return;
          const mcuSignature = window.prompt("MCU signature hex");
          if (!mcuSignature) return;
          await postJson(`/api/gateway/approvals/${approvalId}/decision`, {
            actor: "console-operator",
            decision,
            mcuPublicDid,
            mcuSignature
          });
        } else {
          const reason = window.prompt(
            decision === "approve" ? "Approval reason" : "Rejection reason",
            decision === "approve" ? "approved via control center" : "rejected via control center"
          );
          if (!reason) return;
          await postJson(`/api/gateway/approvals/${approvalId}/decision`, {
            actor: "console-operator",
            decision,
            reason
          });
        }
        await refresh();
      } catch (error) {
        window.alert(error.message);
      }
    }
    async function refresh(preferredNodeId) {
      const [tasks, nodes, settlements, cards, ingress, approvals] = await Promise.all([
        fetchJson("/api/a2a/tasks"),
        fetchJson("/api/gateway/control-plane/nodes"),
        fetchJson("/api/gateway/agent-cards/settlements"),
        fetchJson("/api/gateway/agent-cards/"),
        fetchJson("/api/gateway/ingress/events?limit=8"),
        fetchJson("/api/gateway/approvals?status=pending")
      ]);
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
        ["Approvals", approvals.length]
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
        <article class="feed-item">
          <strong>${approval.title}</strong>
          <p>${ellipsis(approval.summary, 120)}</p>
          <p><code>${approval.kind}</code> · ${approval.referenceId}</p>
          <div class="approval-actions">
            <button type="button" onclick="decideApproval('${approval.approvalId}', '${approval.kind}', 'approve')">Approve</button>
            <button type="button" class="secondary" onclick="decideApproval('${approval.approvalId}', '${approval.kind}', 'reject')">Reject</button>
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

      syncNodeCommandForm(nodes, selectedNodeId);

      document.getElementById("command-rows").innerHTML = commands.slice(0, 8).map((command) => `
        <tr>
          <td>
            <strong>${command.commandType}</strong><br />
            <code>${command.commandId}</code>
          </td>
          <td>${badge(command.status)}</td>
          <td><code>${ellipsis(JSON.stringify(command.result || command.payload || {}), 96)}</code></td>
        </tr>`).join("") || `<tr><td colspan="3" class="tiny">No commands for the selected node.</td></tr>`;

      document.getElementById("settlement-rows").innerHTML = settlements.slice(0, 8).map((settlement) => `
        <tr>
          <td>${ellipsis(settlement.cardId, 26)}</td>
          <td>${badge(settlement.status)}</td>
          <td>${settlement.amount}</td>
        </tr>`).join("");

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
    });
    refresh();
    setInterval(refresh, 5000);
  </script>
</body>
</html>"#,
    )
}
