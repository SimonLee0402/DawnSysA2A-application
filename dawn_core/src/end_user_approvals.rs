use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::error;
use uuid::Uuid;

use crate::{
    ap2::{self, PaymentRequest, PaymentResponse},
    app_state::{
        AppState, ApprovalRequestKind, ApprovalRequestRecord, ApprovalRequestStatus,
        ChatIngressEventRecord, EndUserApprovalSessionRecord, EndUserApprovalStatus, PaymentRecord,
        PaymentStatus, StoredTask, unix_timestamp_ms,
    },
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EndUserApprovalDetailResponse {
    session: EndUserApprovalSessionRecord,
    approval: ApprovalRequestRecord,
    payment: Option<PaymentRecord>,
    task: Option<StoredTask>,
    ingress: Option<ChatIngressEventRecord>,
    approval_url: String,
    signature_payload: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndUserApprovalDecisionRequest {
    actor: Option<String>,
    decision: String,
    reason: Option<String>,
    mcu_public_did: Option<String>,
    mcu_signature: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EndUserApprovalDecisionResponse {
    session: EndUserApprovalSessionRecord,
    approval: ApprovalRequestRecord,
    payment: Option<PaymentRecord>,
    payment_response: Option<PaymentResponse>,
    task: Option<StoredTask>,
}

struct SessionContext {
    session: EndUserApprovalSessionRecord,
    approval: ApprovalRequestRecord,
    payment: Option<PaymentRecord>,
    task: Option<StoredTask>,
    ingress: Option<ChatIngressEventRecord>,
}

pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/approvals/:approval_token", get(get_end_user_approval))
        .route(
            "/approvals/:approval_token/decision",
            post(decide_end_user_approval),
        )
}

pub fn page_router() -> Router<Arc<AppState>> {
    Router::new().route("/approvals/:approval_token", get(end_user_approval_page))
}

pub async fn issue_payment_approval_session(
    state: &AppState,
    approval: &ApprovalRequestRecord,
    payment: &PaymentRecord,
) -> anyhow::Result<Option<String>> {
    if approval.kind != ApprovalRequestKind::Payment
        || approval.status != ApprovalRequestStatus::Pending
        || payment.status != PaymentStatus::PendingPhysicalAuth
    {
        return Ok(None);
    }
    let Some(task_id) = approval.task_id.or(payment.task_id) else {
        return Ok(None);
    };
    let Some(mut ingress) = state.latest_chat_ingress_event_for_task(task_id).await? else {
        return Ok(None);
    };

    let now = unix_timestamp_ms();
    let approval_token = generate_approval_token();
    let approval_url = public_end_user_approval_url(&approval_token);
    let token_hint = token_hint(&approval_token);
    let approval_token_hash = hash_approval_token(&approval_token);
    let expires_at_unix_ms = Some(now + end_user_approval_ttl_ms());

    let session = if let Some(mut existing) = state
        .get_pending_end_user_approval_session_by_approval(approval.approval_id)
        .await?
    {
        existing.task_id = Some(task_id);
        existing.transaction_id = Some(payment.transaction_id);
        existing.platform = Some(ingress.platform.clone());
        existing.chat_id = ingress.chat_id.clone();
        existing.sender_id = ingress.sender_id.clone();
        existing.sender_display = ingress.sender_display.clone();
        existing.approval_token_hash = approval_token_hash;
        existing.token_hint = token_hint;
        existing.status = EndUserApprovalStatus::Pending;
        existing.expires_at_unix_ms = expires_at_unix_ms;
        existing.decided_at_unix_ms = None;
        existing.updated_at_unix_ms = now;
        state.upsert_end_user_approval_session(existing).await?
    } else {
        state
            .upsert_end_user_approval_session(EndUserApprovalSessionRecord {
                session_id: Uuid::new_v4(),
                approval_id: approval.approval_id,
                approval_kind: approval.kind,
                task_id: Some(task_id),
                transaction_id: Some(payment.transaction_id),
                platform: Some(ingress.platform.clone()),
                chat_id: ingress.chat_id.clone(),
                sender_id: ingress.sender_id.clone(),
                sender_display: ingress.sender_display.clone(),
                token_hint,
                status: EndUserApprovalStatus::Pending,
                expires_at_unix_ms,
                decided_at_unix_ms: None,
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
                approval_token_hash,
            })
            .await?
    };

    let existing_reply = ingress.reply_text.unwrap_or_default();
    let approval_message = format!(
        "Payment approval required for {:.2} {}. Open {} to review the charge and approve or reject it from your device.",
        payment.amount, payment.description, approval_url
    );
    let reply_text = if existing_reply.trim().is_empty() {
        approval_message.clone()
    } else if existing_reply.contains(&approval_url) {
        existing_reply
    } else {
        format!("{existing_reply}\n\n{approval_message}")
    };
    ingress.reply_text = Some(reply_text);
    ingress.updated_at_unix_ms = unix_timestamp_ms();
    state.upsert_chat_ingress_event(ingress).await?;
    state
        .record_task_event(
            task_id,
            "end_user_approval_session_issued",
            format!(
                "end-user approval session issued for {} on {}",
                session
                    .sender_display
                    .clone()
                    .or(session.sender_id.clone())
                    .unwrap_or_else(|| "unknown user".to_string()),
                session
                    .platform
                    .clone()
                    .unwrap_or_else(|| "web".to_string()),
            ),
        )
        .await?;

    let _ = session;
    Ok(Some(approval_url))
}

async fn get_end_user_approval(
    State(state): State<Arc<AppState>>,
    Path(approval_token): Path<String>,
) -> Result<Json<EndUserApprovalDetailResponse>, (StatusCode, Json<Value>)> {
    let context = load_session_context(&state, &approval_token).await?;
    let signature_payload = context
        .payment
        .as_ref()
        .filter(|_| context.approval.kind == ApprovalRequestKind::Payment)
        .map(|payment| {
            ap2::signature_payload(
                payment.transaction_id,
                payment.mandate_id,
                payment.amount,
                &payment.description,
            )
        });
    Ok(Json(EndUserApprovalDetailResponse {
        session: context.session,
        approval: context.approval,
        payment: context.payment,
        task: context.task,
        ingress: context.ingress,
        approval_url: public_end_user_approval_url(&approval_token),
        signature_payload,
    }))
}

async fn decide_end_user_approval(
    State(state): State<Arc<AppState>>,
    Path(approval_token): Path<String>,
    Json(request): Json<EndUserApprovalDecisionRequest>,
) -> Result<Json<EndUserApprovalDecisionResponse>, (StatusCode, Json<Value>)> {
    let mut context = load_session_context(&state, &approval_token).await?;
    if context.session.status != EndUserApprovalStatus::Pending {
        return Err(session_state_error(&context.session));
    }
    if context.approval.status != ApprovalRequestStatus::Pending {
        return Err(conflict("approval request is already resolved"));
    }

    let normalized_decision = request.decision.to_ascii_lowercase();
    let actor = request
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| default_end_user_actor(&context.session));
    let payment = context.payment.clone().ok_or_else(|| {
        bad_request(anyhow::anyhow!(
            "payment record is missing for this approval"
        ))
    })?;
    let payment_response = match normalized_decision.as_str() {
        "approve" => ap2::submit_signed_payment_authorization(
            &state,
            PaymentRequest {
                transaction_id: Some(payment.transaction_id),
                task_id: context.approval.task_id.or(payment.task_id),
                mandate_id: payment.mandate_id,
                amount: payment.amount,
                description: payment.description.clone(),
                mcu_public_did: request.mcu_public_did.clone(),
                mcu_signature: request.mcu_signature.clone(),
            },
        )
        .await
        .map_err(service_error)?,
        "reject" => ap2::reject_payment_authorization(
            &state,
            payment.transaction_id,
            &actor,
            request.reason.as_deref().unwrap_or("rejected by end user"),
        )
        .await
        .map_err(service_error)?,
        _ => {
            return Err(bad_request(anyhow::anyhow!(
                "decision must be approve or reject"
            )));
        }
    };

    context.approval = state
        .get_approval_request(context.approval.approval_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("approval request not found after update"))?;
    context.payment = state
        .get_payment(payment.transaction_id)
        .await
        .map_err(internal_error)?;
    context.task = match context.approval.task_id.or(payment.task_id) {
        Some(task_id) => state.get_task(task_id).await.map_err(internal_error)?,
        None => None,
    };

    context.session.status = match payment_response.status {
        PaymentStatus::Authorized => EndUserApprovalStatus::Approved,
        PaymentStatus::Rejected => EndUserApprovalStatus::Rejected,
        PaymentStatus::PendingPhysicalAuth => EndUserApprovalStatus::Pending,
    };
    if context.session.status != EndUserApprovalStatus::Pending {
        context.session.decided_at_unix_ms = Some(unix_timestamp_ms());
    }
    context.session.updated_at_unix_ms = unix_timestamp_ms();
    context.session = state
        .upsert_end_user_approval_session(context.session)
        .await
        .map_err(internal_error)?;

    Ok(Json(EndUserApprovalDecisionResponse {
        session: context.session,
        approval: context.approval,
        payment: context.payment,
        payment_response: Some(payment_response),
        task: context.task,
    }))
}

async fn load_session_context(
    state: &Arc<AppState>,
    approval_token: &str,
) -> Result<SessionContext, (StatusCode, Json<Value>)> {
    let mut session = state
        .get_end_user_approval_session_by_token(approval_token)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("approval session not found"))?;
    let approval = state
        .get_approval_request(session.approval_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("approval request not found"))?;
    session = sync_session_status(state, session, &approval)
        .await
        .map_err(internal_error)?;
    let payment = match approval.kind {
        ApprovalRequestKind::Payment => {
            let transaction_id = session
                .transaction_id
                .or_else(|| Uuid::parse_str(&approval.reference_id).ok())
                .ok_or_else(|| bad_request(anyhow::anyhow!("invalid payment transaction id")))?;
            state
                .get_payment(transaction_id)
                .await
                .map_err(internal_error)?
        }
        ApprovalRequestKind::NodeCommand => None,
    };
    let task_id = session.task_id.or(approval.task_id);
    let task = match task_id {
        Some(task_id) => state.get_task(task_id).await.map_err(internal_error)?,
        None => None,
    };
    let ingress = match task_id {
        Some(task_id) => state
            .latest_chat_ingress_event_for_task(task_id)
            .await
            .map_err(internal_error)?,
        None => None,
    };
    Ok(SessionContext {
        session,
        approval,
        payment,
        task,
        ingress,
    })
}

async fn sync_session_status(
    state: &Arc<AppState>,
    mut session: EndUserApprovalSessionRecord,
    approval: &ApprovalRequestRecord,
) -> anyhow::Result<EndUserApprovalSessionRecord> {
    let now = unix_timestamp_ms();
    let mut changed = false;
    if session.status == EndUserApprovalStatus::Pending {
        let next_status = match approval.status {
            ApprovalRequestStatus::Approved => Some(EndUserApprovalStatus::Approved),
            ApprovalRequestStatus::Rejected => Some(EndUserApprovalStatus::Rejected),
            ApprovalRequestStatus::Pending => session
                .expires_at_unix_ms
                .filter(|expires_at| now > *expires_at)
                .map(|_| EndUserApprovalStatus::Expired),
        };
        if let Some(next_status) = next_status {
            session.status = next_status;
            session.decided_at_unix_ms = Some(now);
            session.updated_at_unix_ms = now;
            changed = true;
        }
    }
    if changed {
        session = state.upsert_end_user_approval_session(session).await?;
    }
    Ok(session)
}

async fn end_user_approval_page(Path(approval_token): Path<String>) -> Html<String> {
    Html(render_end_user_approval_page(&approval_token))
}

fn render_end_user_approval_page(approval_token: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Dawn 终端用户审批页</title>
  <style>
    :root {{
      --bg: #081018;
      --panel: rgba(17, 27, 39, 0.82);
      --panel-2: rgba(12, 21, 31, 0.94);
      --border: rgba(140, 196, 225, 0.18);
      --text: #f1f6fb;
      --muted: #97adbf;
      --accent: #6bd1ff;
      --accent-2: #ffd46f;
      --success: #6ad3a6;
      --danger: #ff8d74;
      --shadow: 0 28px 90px rgba(0, 0, 0, 0.35);
      --font: "Aptos", "Segoe UI Variable Text", "Microsoft YaHei UI", sans-serif;
      --display: "Bahnschrift", "Aptos Display", sans-serif;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      font-family: var(--font);
      color: var(--text);
      background:
        radial-gradient(circle at top left, rgba(107, 209, 255, 0.18), transparent 28%),
        radial-gradient(circle at bottom right, rgba(255, 212, 111, 0.14), transparent 26%),
        linear-gradient(160deg, #061019 0%, #0a1622 44%, #08111a 100%);
      padding: 24px;
    }}
    .shell {{
      max-width: 920px;
      margin: 0 auto;
      display: grid;
      gap: 18px;
    }}
    .hero, .panel {{
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 24px;
      box-shadow: var(--shadow);
      backdrop-filter: blur(18px);
      padding: 24px;
    }}
    h1 {{
      margin: 0 0 8px;
      font-family: var(--display);
      font-size: clamp(28px, 4vw, 42px);
      letter-spacing: 0.02em;
    }}
    p {{ margin: 0; color: var(--muted); line-height: 1.6; }}
    .grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
      gap: 14px;
    }}
    .metric {{
      background: var(--panel-2);
      border: 1px solid rgba(255,255,255,0.06);
      border-radius: 18px;
      padding: 14px 16px;
    }}
    .metric span {{
      display: block;
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }}
    .metric strong {{
      display: block;
      margin-top: 6px;
      font-size: 16px;
    }}
    .pill {{
      display: inline-flex;
      align-items: center;
      border-radius: 999px;
      padding: 6px 10px;
      background: rgba(107, 209, 255, 0.12);
      border: 1px solid rgba(107, 209, 255, 0.2);
      color: var(--accent);
      font-size: 12px;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }}
    .field {{
      display: grid;
      gap: 8px;
      margin-bottom: 14px;
    }}
    label {{
      color: var(--muted);
      font-size: 13px;
    }}
    input, textarea {{
      width: 100%;
      border: 1px solid rgba(255,255,255,0.09);
      border-radius: 16px;
      background: rgba(4, 10, 15, 0.5);
      color: var(--text);
      padding: 14px 16px;
      font: inherit;
    }}
    textarea {{
      min-height: 120px;
      resize: vertical;
    }}
    pre {{
      margin: 0;
      white-space: pre-wrap;
      word-break: break-word;
      background: rgba(4, 10, 15, 0.65);
      border: 1px solid rgba(255,255,255,0.06);
      border-radius: 18px;
      padding: 16px;
      color: #dceeff;
      font-size: 13px;
      line-height: 1.6;
    }}
    .actions {{
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      margin-top: 18px;
    }}
    button {{
      border: 0;
      border-radius: 999px;
      padding: 12px 18px;
      font: inherit;
      font-weight: 600;
      cursor: pointer;
      color: #07111a;
      background: linear-gradient(135deg, var(--accent), #90e3ff);
    }}
    button.secondary {{
      color: var(--text);
      background: rgba(255,255,255,0.06);
      border: 1px solid rgba(255,255,255,0.08);
    }}
    button.reject {{
      color: var(--text);
      background: linear-gradient(135deg, #ec8468, var(--danger));
    }}
    button:disabled {{
      opacity: 0.45;
      cursor: not-allowed;
    }}
    .status {{
      border-radius: 18px;
      padding: 14px 16px;
      background: rgba(255,255,255,0.04);
      border: 1px solid rgba(255,255,255,0.06);
      color: var(--muted);
    }}
    .status.error {{
      color: #ffd6cf;
      border-color: rgba(255, 141, 116, 0.3);
      background: rgba(255, 141, 116, 0.08);
    }}
  </style>
</head>
<body>
  <main class="shell">
    <section class="hero">
      <div style="display:flex;justify-content:space-between;gap:12px;align-items:flex-start;flex-wrap:wrap;">
        <div>
          <span class="pill" id="approval-pill">Dawn 终端用户审批</span>
          <h1 id="approval-title">请审核并授权这笔支付</h1>
        </div>
        <button type="button" class="secondary" id="lang-toggle" onclick="toggleLanguage()">EN</button>
      </div>
      <p id="hero-copy">正在加载审批详情…</p>
    </section>
    <section class="panel">
      <div class="grid" id="metrics"></div>
    </section>
    <section class="panel">
      <div class="field">
        <label id="signature-payload-label">请用 DawnMCU 设备签名的载荷</label>
        <pre id="signature-payload">加载中…</pre>
      </div>
      <div class="actions">
        <button type="button" class="secondary" id="copy-payload-button" onclick="copyPayload()">复制载荷</button>
      </div>
    </section>
    <section class="panel">
      <div class="field">
        <label for="actor" id="actor-label">显示名称</label>
        <input id="actor" type="text" placeholder="你的姓名" />
      </div>
      <div class="field">
        <label for="reason" id="reason-label">原因说明</label>
        <textarea id="reason" placeholder="可选的批准或拒绝说明"></textarea>
      </div>
      <div class="field">
        <label for="mcu-public-did" id="mcu-public-did-label">MCU 公钥 DID</label>
        <input id="mcu-public-did" type="text" placeholder="did:dawn:mcu:..." />
      </div>
      <div class="field">
        <label for="mcu-signature" id="mcu-signature-label">MCU 签名 Hex</label>
        <textarea id="mcu-signature" placeholder="粘贴设备生成的签名"></textarea>
      </div>
      <div class="actions">
        <button id="approve-button" type="button" onclick="submitDecision('approve')">批准支付</button>
        <button id="reject-button" type="button" class="reject" onclick="submitDecision('reject')">拒绝支付</button>
        <button type="button" class="secondary" id="refresh-approval-button" onclick="loadApproval()">刷新</button>
      </div>
    </section>
    <section class="status" id="status">等待审批数据…</section>
  </main>
  <script>
    const approvalToken = {approval_token:?};
    let approvalData = null;
    const localeKey = "dawnUiLanguage";
    const translations = {{
      zh: {{
        title: "Dawn 终端用户审批页",
        pill: "Dawn 终端用户审批",
        heroTitle: "请审核并授权这笔支付",
        heroLoading: "正在加载审批详情…",
        signatureLabel: "请用 DawnMCU 设备签名的载荷",
        copyPayload: "复制载荷",
        actorLabel: "显示名称",
        actorPlaceholder: "你的姓名",
        reasonLabel: "原因说明",
        reasonPlaceholder: "可选的批准或拒绝说明",
        publicDidLabel: "MCU 公钥 DID",
        signatureLabelShort: "MCU 签名 Hex",
        signaturePlaceholder: "粘贴设备生成的签名",
        approve: "批准支付",
        reject: "拒绝支付",
        refresh: "刷新",
        waitingStatus: "等待审批数据…",
        metrics: ["审批状态", "支付状态", "金额", "说明", "用户", "渠道", "令牌提示", "过期时间"],
        unknown: "未知",
        noPayload: "当前审批没有可签名的载荷。",
        paymentPending: (amount) => `一笔金额为 $${{amount}} 的支付正在等待你的硬件签名授权。`,
        actionPending: "这条审批正在等待你的处理。",
        reviewHint: "请先审阅载荷并在设备上完成签名，再执行批准；如果不希望继续扣款，请直接拒绝。",
        payloadMissing: "当前没有可复制的签名载荷。",
        payloadCopied: "签名载荷已复制到剪贴板。",
        payloadCopyFailed: "写入剪贴板失败，请直接从页面复制载荷。",
        dataNotReady: "审批数据尚未加载完成。",
        didRequired: "批准前必须填写 MCU 公钥 DID。",
        sigRequired: "批准前必须填写 MCU 签名 Hex。",
        submitting: (decision) => `正在提交 ${{decision}}…`,
        submitFailed: "提交决策失败",
        loadFailed: "加载审批失败",
        decisionRecorded: (status) => `决策已记录。支付当前状态为 ${{status}}。`
      }},
      en: {{
        title: "Dawn Approval Portal",
        pill: "Dawn End-User Approval",
        heroTitle: "Review and authorize this payment",
        heroLoading: "Loading approval details…",
        signatureLabel: "Payload to sign with your DawnMCU device",
        copyPayload: "Copy Payload",
        actorLabel: "Display name",
        actorPlaceholder: "Your name",
        reasonLabel: "Reason",
        reasonPlaceholder: "Optional approval or rejection note",
        publicDidLabel: "MCU public DID",
        signatureLabelShort: "MCU signature hex",
        signaturePlaceholder: "Paste the signature produced by your device",
        approve: "Approve Payment",
        reject: "Reject Payment",
        refresh: "Refresh",
        waitingStatus: "Waiting for approval data…",
        metrics: ["Approval status", "Payment state", "Amount", "Description", "User", "Channel", "Token hint", "Expires"],
        unknown: "Unknown",
        noPayload: "No signing payload is available for this approval.",
        paymentPending: (amount) => `A payment for $${{amount}} is waiting for your hardware-backed authorization.`,
        actionPending: "This approval is waiting for your action.",
        reviewHint: "Review the payload, sign it on your device, then approve. If you do not want this charge to proceed, reject it.",
        payloadMissing: "No payload is available to copy.",
        payloadCopied: "Signing payload copied to clipboard.",
        payloadCopyFailed: "Clipboard write failed. Copy the payload directly from the panel.",
        dataNotReady: "Approval data has not loaded yet.",
        didRequired: "MCU public DID is required before approval.",
        sigRequired: "MCU signature hex is required before approval.",
        submitting: (decision) => `Submitting ${{decision}}…`,
        submitFailed: "Failed to submit decision",
        loadFailed: "Failed to load approval",
        decisionRecorded: (status) => `Decision recorded. Payment is now ${{status}}.`
      }}
    }};
    let currentLanguage = window.localStorage?.getItem(localeKey) || "zh";
    function t(key) {{
      return translations[currentLanguage]?.[key] || translations.zh[key] || key;
    }}
    function applyLanguage() {{
      document.documentElement.lang = currentLanguage === "zh" ? "zh-CN" : "en";
      document.title = t("title");
      document.getElementById("approval-pill").textContent = t("pill");
      document.getElementById("approval-title").textContent = t("heroTitle");
      document.getElementById("signature-payload-label").textContent = t("signatureLabel");
      document.getElementById("copy-payload-button").textContent = t("copyPayload");
      document.getElementById("actor-label").textContent = t("actorLabel");
      document.getElementById("actor").placeholder = t("actorPlaceholder");
      document.getElementById("reason-label").textContent = t("reasonLabel");
      document.getElementById("reason").placeholder = t("reasonPlaceholder");
      document.getElementById("mcu-public-did-label").textContent = t("publicDidLabel");
      document.getElementById("mcu-signature-label").textContent = t("signatureLabelShort");
      document.getElementById("mcu-signature").placeholder = t("signaturePlaceholder");
      document.getElementById("approve-button").textContent = t("approve");
      document.getElementById("reject-button").textContent = t("reject");
      document.getElementById("refresh-approval-button").textContent = t("refresh");
      document.getElementById("lang-toggle").textContent = currentLanguage === "zh" ? "EN" : "中文";
      if (!approvalData) {{
        document.getElementById("hero-copy").textContent = t("heroLoading");
        document.getElementById("signature-payload").textContent = "Loading…";
        document.getElementById("status").textContent = t("waitingStatus");
      }} else {{
        renderApproval(approvalData);
      }}
    }}
    function toggleLanguage() {{
      currentLanguage = currentLanguage === "zh" ? "en" : "zh";
      try {{
        window.localStorage?.setItem(localeKey, currentLanguage);
      }} catch (_error) {{}}
      applyLanguage();
    }}

    function escapeHtml(value) {{
      return String(value ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;");
    }}

    function setStatus(message, isError = false) {{
      const element = document.getElementById("status");
      element.className = isError ? "status error" : "status";
      element.innerHTML = message;
    }}

    function renderMetrics(data) {{
      const metrics = [
        [t("metrics")[0], data.session.status],
        [t("metrics")[1], data.payment?.status || "pending"],
        [t("metrics")[2], data.payment ? data.payment.amount.toFixed(2) : "n/a"],
        [t("metrics")[3], data.payment?.description || data.approval.summary],
        [t("metrics")[4], data.session.senderDisplay || data.session.senderId || t("unknown")],
        [t("metrics")[5], data.session.platform || "web"],
        [t("metrics")[6], data.session.tokenHint],
        [t("metrics")[7], data.session.expiresAtUnixMs ? new Date(Number(data.session.expiresAtUnixMs)).toLocaleString() : "n/a"]
      ];
      document.getElementById("metrics").innerHTML = metrics.map(([label, value]) => `
        <div class="metric">
          <span>${{escapeHtml(label)}}</span>
          <strong>${{escapeHtml(value)}}</strong>
        </div>
      `).join("");
    }}

    function renderApproval(data) {{
      approvalData = data;
      document.getElementById("hero-copy").textContent =
        data.payment
          ? t("paymentPending")(Number(data.payment.amount).toFixed(2))
          : t("actionPending");
      document.getElementById("signature-payload").textContent = data.signaturePayload || t("noPayload");
      document.getElementById("actor").value = data.session.senderDisplay || data.session.senderId || "";
      renderMetrics(data);
      const pending = data.session.status === "pending";
      document.getElementById("approve-button").disabled = !pending;
      document.getElementById("reject-button").disabled = !pending;
      setStatus(
        pending
          ? t("reviewHint")
          : `This approval is already ${{data.session.status}}.`
      );
    }}

    async function loadApproval() {{
      try {{
        const response = await fetch(`/api/gateway/end-user/approvals/${{approvalToken}}`);
        const payload = await response.json();
        if (!response.ok) throw new Error(payload.error || t("loadFailed"));
        renderApproval(payload);
      }} catch (error) {{
        setStatus(escapeHtml(error.message), true);
      }}
    }}

    async function copyPayload() {{
      const payload = approvalData?.signaturePayload;
      if (!payload) {{
          setStatus(t("payloadMissing"), true);
        return;
      }}
      try {{
        await navigator.clipboard.writeText(payload);
        setStatus(t("payloadCopied"));
      }} catch (_error) {{
        setStatus(t("payloadCopyFailed"), true);
      }}
    }}

    async function submitDecision(decision) {{
      if (!approvalData) {{
        setStatus(t("dataNotReady"), true);
        return;
      }}
      const actor = document.getElementById("actor").value.trim();
      const reason = document.getElementById("reason").value.trim();
      const payload = {{ actor, decision, reason }};
      if (decision === "approve") {{
        const mcuPublicDid = document.getElementById("mcu-public-did").value.trim();
        const mcuSignature = document.getElementById("mcu-signature").value.trim();
        if (!mcuPublicDid) {{
          setStatus(t("didRequired"), true);
          return;
        }}
        if (!mcuSignature) {{
          setStatus(t("sigRequired"), true);
          return;
        }}
        payload.mcuPublicDid = mcuPublicDid;
        payload.mcuSignature = mcuSignature;
      }}
      setStatus(t("submitting")(decision));
      try {{
        const response = await fetch(`/api/gateway/end-user/approvals/${{approvalToken}}/decision`, {{
          method: "POST",
          headers: {{ "Content-Type": "application/json" }},
          body: JSON.stringify(payload)
        }});
        const result = await response.json();
        if (!response.ok) throw new Error(result.error || t("submitFailed"));
        setStatus(t("decisionRecorded")(result.payment?.status || result.session.status));
        await loadApproval();
      }} catch (error) {{
        setStatus(escapeHtml(error.message), true);
      }}
    }}

    applyLanguage();
    loadApproval();
  </script>
</body>
</html>"#
    )
}

fn default_end_user_actor(session: &EndUserApprovalSessionRecord) -> String {
    session
        .sender_display
        .clone()
        .or(session.sender_id.clone())
        .unwrap_or_else(|| "end-user".to_string())
}

fn session_state_error(session: &EndUserApprovalSessionRecord) -> (StatusCode, Json<Value>) {
    match session.status {
        EndUserApprovalStatus::Expired => gone("approval session has expired"),
        EndUserApprovalStatus::Approved | EndUserApprovalStatus::Rejected => {
            conflict("approval session is already resolved")
        }
        EndUserApprovalStatus::Pending => {
            bad_request(anyhow::anyhow!("approval session is not available"))
        }
    }
}

fn generate_approval_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn token_hint(token: &str) -> String {
    token
        .chars()
        .rev()
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn hash_approval_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn end_user_approval_ttl_ms() -> u128 {
    std::env::var("DAWN_END_USER_APPROVAL_TTL_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(u128::from)
        .unwrap_or(72 * 60 * 60)
        * 1000
}

pub fn public_end_user_approval_url(approval_token: &str) -> String {
    let path = format!("/end-user/approvals/{approval_token}");
    if let Ok(value) = std::env::var("DAWN_PUBLIC_BASE_URL") {
        let trimmed = value.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            return format!("{trimmed}{path}");
        }
    }
    path
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
}

fn conflict(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::CONFLICT, Json(json!({ "error": message })))
}

fn gone(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::GONE, Json(json!({ "error": message })))
}

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": error.to_string() })),
    )
}

fn service_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    let status = if message.contains("unknown transactionId") {
        StatusCode::NOT_FOUND
    } else if message.contains("required")
        || message.contains("invalid")
        || message.contains("must be")
        || message.contains("already resolved")
        || message.contains("expired")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        error!(?error, "End-user approval service failure");
    }
    (status, Json(json!({ "error": message })))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    error!(?error, "End-user approval persistence failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal persistence error"
        })),
    )
}
