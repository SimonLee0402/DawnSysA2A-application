# Dawn 安全升级说明

日期：2026-06-16

## 概述

本次安全升级完成了网关暴露面、聊天平台回调、远程资源访问、技能包路径、支付授权和桌面控制命令的安全加固。升级目标是在不破坏现有 QQ、Telegram、微信等聊天入口调用能力的前提下，让生产环境默认失败关闭，并补齐平台级签名校验。

## 已完成内容

### 网关和控制面

- 网关默认绑定到 `127.0.0.1:8000`，避免安装后默认暴露到公网。
- 控制面、连接器、Marketplace、skills、Agent Cards、A2A/AP2 等敏感接口增加本机或管理员 token 校验。
- 管理员 token 支持 `Authorization: Bearer`、`x-dawn-admin-token` 和 `x-dawn-operator-token`。

### 聊天平台回调验签

- Telegram：保留旧的路径 secret，同时支持官方 `X-Telegram-Bot-Api-Secret-Token` header。
- Feishu：新增 `X-Lark-Request-Timestamp`、`X-Lark-Request-Nonce`、`X-Lark-Signature` 校验，并支持加密事件体解密。
- DingTalk：新增 callback token、`signature/timestamp/nonce` 校验和 EncodingAESKey 解密；密文回调返回加密 `success`。
- WeCom：新增 `msg_signature/timestamp/nonce` 校验，支持 URL 验证 `echostr` 解密和消息体 `Encrypt` 解密。
- WeChat Official Account：保留明文模式 `signature/timestamp/nonce` 校验，并新增安全模式 `msg_signature + Encrypt + EncodingAESKey` 支持。
- QQ：新增 `X-Signature-Ed25519` 和 `X-Signature-Timestamp` 校验，URL validation 返回签名后的 `plain_token`。
- Signal 和 BlueBubbles：继续通过本地桥接回调路径 secret 保护。

### 远程访问和技能包安全

- 远程 Marketplace、远程 Agent Card、远程报价和技能包下载默认只允许公网 `http/https` URL。
- 默认拒绝 localhost、私网、链路本地、组播、未指定地址和云 metadata 地址，降低 SSRF 风险。
- 技能包 artifact 路径增加 canonicalize 和目录包含校验，拒绝 `.`、`..` 等不安全路径段。

### AP2、报价和桌面控制

- AP2 硬件签名授权增加 DID 匹配校验，防止授权来源和待支付记录不一致。
- 本地报价签名要求配置正式签名 seed，避免生产环境使用不安全开发密钥。
- 文件读取、目录枚举、进程快照等高风险 node command 默认需要审批。

### 配置和可观测性

- README 已更新生产安全启动配置。
- 控制台和 onboarding readiness 已同步新的 Feishu、DingTalk、WeCom、QQ 验签配置要求。
- 聊天入口状态增加 Feishu 签名、DingTalk/WeCom 加密、WeChat 安全模式配置状态。

## 生产环境关键配置

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

`DAWN_ALLOW_UNAUTHENTICATED_INGRESS=1` 只允许用于本地开发或受控内网测试，不应在生产环境开启。

## 验证结果

已执行全量烟测：

```powershell
cargo test --manifest-path dawn_core/Cargo.toml -- --test-threads=1
```

结果：`192 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`。

另外执行过：

```powershell
cargo check --manifest-path dawn_core/Cargo.toml
cargo test --manifest-path dawn_core/Cargo.toml chat_ingress
cargo test --manifest-path dawn_core/Cargo.toml identity
cargo test --manifest-path dawn_core/Cargo.toml -- --skip qgis
```

上述检查均已通过。
