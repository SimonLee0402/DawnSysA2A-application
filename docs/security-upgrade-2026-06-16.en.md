# Dawn Security Upgrade Notes

Date: 2026-06-16

## Overview

This security upgrade hardens the gateway exposure model, chat platform webhooks, remote resource fetching, skill package paths, payment authorization, and desktop-control command gates. The upgrade preserves existing QQ, Telegram, WeChat, and other chat-driven workflows while making production ingress fail closed and enforcing platform-level signature validation.

## Completed Work

### Gateway And Control Plane

- The gateway now defaults to `127.0.0.1:8000`, so a fresh install does not expose privileged APIs publicly by default.
- Sensitive gateway surfaces now require loopback access or an administrator token. This includes control-plane APIs, connectors, Marketplace, skills, Agent Cards, A2A, and AP2 payment/task APIs.
- Administrator tokens are accepted through `Authorization: Bearer`, `x-dawn-admin-token`, or `x-dawn-operator-token`.

### Chat Platform Webhook Verification

- Telegram: keeps the legacy path secret and also accepts Telegram's official `X-Telegram-Bot-Api-Secret-Token` header.
- Feishu: verifies `X-Lark-Request-Timestamp`, `X-Lark-Request-Nonce`, and `X-Lark-Signature`, with encrypted event body decryption support.
- DingTalk: verifies callback token plus `signature/timestamp/nonce`, decrypts EncodingAESKey callbacks, and returns encrypted `success` for encrypted callbacks.
- WeCom: verifies `msg_signature/timestamp/nonce`, decrypts URL verification `echostr`, and decrypts message-body `Encrypt` payloads.
- WeChat Official Account: keeps plaintext `signature/timestamp/nonce` validation and adds safe-mode `msg_signature + Encrypt + EncodingAESKey` support.
- QQ: verifies `X-Signature-Ed25519` and `X-Signature-Timestamp`, and returns signed `plain_token` URL validation responses.
- Signal and BlueBubbles: continue to use local bridge callback path secrets.

### Remote Fetching And Skill Package Safety

- Remote Marketplace, remote Agent Card, remote quote, and skill package downloads now default to public `http/https` URLs only.
- Localhost, private networks, link-local addresses, multicast, unspecified addresses, and cloud metadata hostnames are rejected by default to reduce SSRF risk.
- Skill artifact paths now use canonicalization and directory containment checks, rejecting unsafe path segments such as `.` and `..`.

### AP2, Quote Signing, And Desktop Control

- AP2 hardware-signature authorization now checks the expected DID against the pending payment record.
- Local quote signing requires a production signing seed outside development mode.
- High-risk node commands such as file reads, directory enumeration, and process snapshots require approval by default.

### Configuration And Observability

- The README now documents production-safe startup and webhook settings.
- Console and onboarding readiness checks now reflect the new Feishu, DingTalk, WeCom, and QQ signature requirements.
- Chat ingress status now reports Feishu signature configuration, DingTalk/WeCom encryption configuration, and WeChat safe-mode encryption configuration.

## Production Configuration

```powershell
$env:DAWN_GATEWAY_ADMIN_TOKEN = "<admin-token>"
$env:DAWN_TELEGRAM_WEBHOOK_SECRET = "<telegram-secret-token>"
$env:FEISHU_EVENT_ENCRYPT_KEY = "<feishu-event-encrypt-key>"
$env:DAWN_DINGTALK_CALLBACK_TOKEN = "<dingtalk-token>"
$env:DAWN_DINGTALK_ENCODING_AES_KEY = "<dingtalk-encoding-aes-key>"
$env:DAWN_WECOM_CALLBACK_TOKEN = "<wecom-token>"
$env:DAWN_WECOM_ENCODING_AES_KEY = "<wecom-encoding-aes-key>"
$env:DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN = "<wechat-token>"
$env:WECHAT_OFFICIAL_ACCOUNT_ENCODING_AES_KEY = "<optional-wechat-safe-mode-key>"
$env:DAWN_QQ_BOT_CALLBACK_SECRET = "<qq-bot-secret>"
```

`DAWN_ALLOW_UNAUTHENTICATED_INGRESS=1` is only for local development or controlled intranet testing. It should not be enabled in production.

## Verification

Full smoke test executed:

```powershell
cargo test --manifest-path dawn_core/Cargo.toml -- --test-threads=1
```

Result: `192 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.

Additional checks executed:

```powershell
cargo check --manifest-path dawn_core/Cargo.toml
cargo test --manifest-path dawn_core/Cargo.toml chat_ingress
cargo test --manifest-path dawn_core/Cargo.toml identity
cargo test --manifest-path dawn_core/Cargo.toml -- --skip qgis
```

All checks passed.
