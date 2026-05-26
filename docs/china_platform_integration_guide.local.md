# Dawn 中国平台接入教程

本文件是本地使用的接入教程，不计划提交到 GitHub。

适用范围：

- 飞书
- 钉钉
- 微信公众号
- QQ Bot

本文面向两类人：

- 平台接入负责人：负责在飞书、钉钉、微信、QQ 平台后台配置回调和机器人
- Dawn 操作员：负责把环境变量注入 Dawn，启动网关，并验证链路

## 1. 先理解 Dawn 里的链路

在 Dawn 里，中国平台接入通常分成两段：

1. 入站链路
   用户在平台里发消息，平台把事件回调到 Dawn Gateway。
2. 出站链路
   Dawn 把执行结果回发到平台。

四个平台在 Dawn 里的实现方式并不完全一样：

| 平台 | 入站 | 出站 | 当前实现特点 |
| --- | --- | --- | --- |
| 飞书 | 回调事件 | Webhook | 入站支持 challenge 与文本消息 |
| 钉钉 | 回调事件 | Webhook | 入站会校验 callback token |
| 微信公众号 | Token 验证 + XML 消息 | 官方客服消息接口 | 这里的“微信”指公众号，不是个人微信 |
| QQ Bot | 回调事件 | QQ OpenAPI | 当前 challenge 可回显，签名校验尚未完全落地 |

## 2. 通用准备

### 2.1 需要一个 Dawn Gateway

本机先能启动 Dawn：

```powershell
.\dawn.ps1
```

默认网关地址：

- `http://127.0.0.1:8000`

先确认本机服务活着：

```powershell
Invoke-RestMethod http://127.0.0.1:8000/health
```

### 2.2 需要一个公网 HTTPS 地址

中国平台的回调一般都要求平台能访问到你的 Gateway，所以你至少需要：

- 一个公网域名，例如 `https://dawn.example.cn`
- 反向代理到本机 `127.0.0.1:8000`
- HTTPS 证书

如果 Dawn 只跑在本机局域网且没有公网暴露，平台回调就打不到 Gateway。

### 2.3 环境变量必须对 Gateway 进程可见

这些平台接入依赖环境变量。最简单的做法是先在当前 PowerShell 会话里设置，再启动 Dawn：

```powershell
$env:FEISHU_BOT_WEBHOOK_URL = "https://open.feishu.cn/open-apis/bot/v2/hook/xxxx"
$env:DINGTALK_BOT_WEBHOOK_URL = "https://oapi.dingtalk.com/robot/send?access_token=xxxx"
$env:DAWN_DINGTALK_CALLBACK_TOKEN = "your-dingtalk-callback-token"
$env:DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN = "your-wechat-token"
$env:WECHAT_OFFICIAL_ACCOUNT_APP_ID = "wx1234567890"
$env:WECHAT_OFFICIAL_ACCOUNT_APP_SECRET = "xxxxxxxx"
$env:QQ_BOT_APP_ID = "xxxxxxxx"
$env:QQ_BOT_CLIENT_SECRET = "xxxxxxxx"
$env:DAWN_QQ_BOT_CALLBACK_SECRET = "xxxxxxxx"

.\dawn.ps1
```

如果你想持久化到当前 Windows 用户环境，也可以用：

```powershell
[Environment]::SetEnvironmentVariable("FEISHU_BOT_WEBHOOK_URL", "your-value", "User")
```

然后重开 PowerShell，再启动 Dawn。

### 2.4 用户在平台里怎么用 Dawn

这些平台接进来以后，用户不是在调用某个单独接口，而是在聊天窗口里直接使用 Dawn。

建议先教用户这些命令：

- `帮助`
- `状态`
- `技能`
- `/help`
- `/status`
- `/skills`
- `/task 帮我整理今天的工作`
- `#observe`
- `#assist`

在飞书、钉钉、QQ、企微这一类平台里，也支持：

- `@机器人 /help`
- `@机器人 /skills`

在微信公众号里，一般直接发送纯文本即可，不需要 `@机器人`。

## 3. 飞书接入

### 3.1 Dawn 侧需要什么

出站发送需要：

- `FEISHU_BOT_WEBHOOK_URL`

入站回调地址：

- `https://你的域名/api/gateway/ingress/feishu/events`

飞书在 Dawn 里的特点：

- 当前实现支持 challenge 回包
- 当前实现支持把文本消息转成 Dawn 任务
- 当前实现没有额外要求飞书 ingress secret，走的是基础 challenge 模式

### 3.2 飞书后台怎么配

在飞书开放平台或机器人配置界面：

1. 创建应用或机器人
2. 打开事件订阅
3. 把请求地址配置成：

```text
https://你的域名/api/gateway/ingress/feishu/events
```

4. 开启你需要的消息事件
5. 如果需要 Dawn 主动回发到群，配置并复制 Bot Webhook URL

### 3.3 Dawn 里如何主动发飞书消息

飞书发送接口是：

- `POST /api/gateway/connectors/chat/feishu/send`

请求体：

```json
{
  "text": "Dawn 飞书链路测试"
}
```

PowerShell 示例：

```powershell
Invoke-RestMethod `
  -Method Post `
  -Uri http://127.0.0.1:8000/api/gateway/connectors/chat/feishu/send `
  -ContentType "application/json" `
  -Body '{"text":"Dawn 飞书链路测试"}'
```

注意：

- 飞书出站当前是 Webhook 模式
- Webhook 已经绑定到固定机器人或固定群，不需要再传 `chatId`

### 3.4 飞书用户如何使用 Dawn

在飞书群聊或机器人会话里，用户可以直接发送：

- `帮助`
- `状态`
- `技能`
- `@机器人 /help`
- `@机器人 /task 帮我总结今天待办`
- `#observe`

### 3.5 飞书验证顺序

建议这样测：

1. 先启动 Dawn
2. 在飞书后台保存回调地址，确认 challenge 成功
3. 用上面的 `/chat/feishu/send` 接口做一次主动发送测试
4. 在飞书里手动发 `帮助`
5. 确认 Dawn 创建了 ingress 事件，并且机器人能回消息

## 4. 钉钉接入

### 4.1 Dawn 侧需要什么

出站发送需要：

- `DINGTALK_BOT_WEBHOOK_URL`

入站回调需要：

- `DAWN_DINGTALK_CALLBACK_TOKEN`

入站回调地址：

- `https://你的域名/api/gateway/ingress/dingtalk/events`

### 4.2 钉钉后台怎么配

在钉钉机器人或事件订阅后台：

1. 创建机器人
2. 配置消息回调地址：

```text
https://你的域名/api/gateway/ingress/dingtalk/events
```

3. 回调 token 要和 `DAWN_DINGTALK_CALLBACK_TOKEN` 一致
4. 配置你需要的消息事件
5. 如果要让 Dawn 主动发钉钉消息，复制机器人 webhook

### 4.3 Dawn 里如何主动发钉钉消息

钉钉发送接口是：

- `POST /api/gateway/connectors/chat/dingtalk/send`

请求体：

```json
{
  "text": "Dawn 钉钉链路测试"
}
```

PowerShell 示例：

```powershell
Invoke-RestMethod `
  -Method Post `
  -Uri http://127.0.0.1:8000/api/gateway/connectors/chat/dingtalk/send `
  -ContentType "application/json" `
  -Body '{"text":"Dawn 钉钉链路测试"}'
```

注意：

- 钉钉出站当前也是 Webhook 模式
- 不需要额外传 `chatId`

### 4.4 钉钉用户如何使用 Dawn

用户在钉钉里可以发送：

- `帮助`
- `状态`
- `技能`
- `@机器人 /help`
- `@机器人 /task 帮我生成日报`
- `#assist`

### 4.5 钉钉验证顺序

建议这样测：

1. 先配 `DINGTALK_BOT_WEBHOOK_URL`
2. 再配 `DAWN_DINGTALK_CALLBACK_TOKEN`
3. 启动 Dawn
4. 先用 `/chat/dingtalk/send` 做主动发送测试
5. 再在钉钉里发 `帮助`

当前代码侧的入站测试已经通过，你可以把它理解为“逻辑链路已打通，差的只是平台后台配置和公网入口”。

## 5. 微信公众号接入

### 5.1 先明确边界

这里接入的是：

- 微信公众号

不是：

- 个人微信
- 企业微信客服
- 小程序消息

### 5.2 Dawn 侧需要什么

入站验证与消息接收需要：

- `DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN`

出站发送二选一：

- `WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN`
- 或 `WECHAT_OFFICIAL_ACCOUNT_APP_ID + WECHAT_OFFICIAL_ACCOUNT_APP_SECRET`

入站地址：

- `GET/POST https://你的域名/api/gateway/ingress/wechat-official-account/events`

### 5.3 微信公众号后台怎么配

在公众号开发配置里：

1. 把服务器地址配置成：

```text
https://你的域名/api/gateway/ingress/wechat-official-account/events
```

2. Token 配成和 `DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN` 一样
3. 完成平台的 URL 验证
4. 开启消息接收

### 5.4 Dawn 里如何主动发公众号消息

微信公众号发送接口是：

- `POST /api/gateway/connectors/chat/wechat-official-account/send`

请求体：

```json
{
  "openId": "用户-open-id",
  "text": "Dawn 公众号链路测试"
}
```

PowerShell 示例：

```powershell
Invoke-RestMethod `
  -Method Post `
  -Uri http://127.0.0.1:8000/api/gateway/connectors/chat/wechat-official-account/send `
  -ContentType "application/json" `
  -Body '{"openId":"user-open-id","text":"Dawn 公众号链路测试"}'
```

注意：

- 公众号出站不是 webhook，而是公众号官方客服消息接口
- 所以必须能拿到 `access_token`
- Dawn 当前支持两种方式：
  - 你自己提前提供 `WECHAT_OFFICIAL_ACCOUNT_ACCESS_TOKEN`
  - 或者提供 `WECHAT_OFFICIAL_ACCOUNT_APP_ID + WECHAT_OFFICIAL_ACCOUNT_APP_SECRET` 让 Dawn 去换 token

### 5.5 微信用户如何使用 Dawn

在公众号对话框里，建议直接发纯文本：

- `帮助`
- `状态`
- `技能`
- `/task 帮我总结今天的安排`
- `#observe`

公众号场景通常不需要 `@机器人`。

### 5.6 微信验证顺序

建议这样测：

1. 先配 `DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN`
2. 在公众号后台完成 URL 验证
3. 再配发送侧凭据
4. 用 `/chat/wechat-official-account/send` 做一次主动发送测试
5. 在公众号里手动发 `帮助`

当前代码侧的验证回包测试和 XML 入站测试都已经通过。

## 6. QQ Bot 接入

### 6.1 Dawn 侧需要什么

出站发送需要：

- `QQ_BOT_APP_ID`
- `QQ_BOT_CLIENT_SECRET`

入站回调需要：

- `DAWN_QQ_BOT_CALLBACK_SECRET`

入站回调地址：

- `https://你的域名/api/gateway/ingress/qq/events`

### 6.2 QQ 平台后台怎么配

在 QQ Bot 开发平台：

1. 创建 Bot 应用
2. 配置回调地址：

```text
https://你的域名/api/gateway/ingress/qq/events
```

3. 配置 callback secret，并与 `DAWN_QQ_BOT_CALLBACK_SECRET` 保持一致
4. 拿到 `QQ_BOT_APP_ID` 和 `QQ_BOT_CLIENT_SECRET`

### 6.3 Dawn 里如何主动发 QQ 消息

QQ 发送接口是：

- `POST /api/gateway/connectors/chat/qq/send`

请求体：

```json
{
  "recipientId": "group-openid-or-user-openid",
  "text": "Dawn QQ 链路测试",
  "targetType": "group"
}
```

PowerShell 示例：

```powershell
Invoke-RestMethod `
  -Method Post `
  -Uri http://127.0.0.1:8000/api/gateway/connectors/chat/qq/send `
  -ContentType "application/json" `
  -Body '{"recipientId":"group-openid","text":"Dawn QQ 链路测试","targetType":"group"}'
```

### 6.4 QQ 用户如何使用 Dawn

在 QQ 里可以发送：

- `帮助`
- `状态`
- `技能`
- `@机器人 /help`
- `@机器人 /task 帮我汇总今天的任务`
- `#assist`

### 6.5 当前限制

QQ 当前实现可以：

- 处理 challenge 回显
- 提取文本消息
- 进入 Dawn 任务链路
- 通过 QQ OpenAPI 发消息

但要注意：

- 当前代码里 challenge 回显已经支持
- callback 签名校验尚未完全强制执行

所以如果你要把 QQ 用在正式生产环境，建议把这部分补完，再开放公网入口。

## 7. 推荐接入顺序

如果你是第一次把中国平台接到 Dawn，建议按这个顺序来：

1. 飞书
   原因：最轻，回调和出站都简单，适合先跑通。
2. 钉钉
   原因：比飞书多一个 callback token，但仍然是 webhook 型发送。
3. 微信公众号
   原因：要处理 token 验证、XML 消息和 access token。
4. QQ Bot
   原因：当前功能有基础链路，但安全校验还没完全收紧。

## 8. 一套通用验证办法

每接一个平台，都按下面顺序做：

1. 启动 Dawn

```powershell
.\dawn.ps1
```

2. 先看本机健康检查

```powershell
Invoke-RestMethod http://127.0.0.1:8000/health
```

3. 再做 Dawn 主动发送测试

- 飞书：`/chat/feishu/send`
- 钉钉：`/chat/dingtalk/send`
- 微信公众号：`/chat/wechat-official-account/send`
- QQ：`/chat/qq/send`

4. 再去平台里人工发一条：

- `帮助`

5. 确认两件事：

- Dawn 收到了 ingress 事件
- 平台里看到了 Dawn 回复

## 9. 当前已知实现差异

- 飞书 ingress 当前是基础 challenge 模式，没有额外 secret 校验
- 钉钉 ingress 会校验 `DAWN_DINGTALK_CALLBACK_TOKEN`
- 微信公众号 ingress 会校验 `DAWN_WECHAT_OFFICIAL_ACCOUNT_TOKEN`
- 微信公众号发送依赖 `access_token`
- QQ callback secret 当前在接入说明层面存在，但网关还没有把签名校验彻底强制化
- 你这台机器目前这些平台的环境变量都还没配，所以要先补环境，再谈 live 联通

## 10. 接入完成后的用户培训话术

给最终用户最简单的说明可以是：

1. 在飞书、钉钉、公众号或 QQ 里直接找到 Dawn 机器人
2. 先发 `帮助`
3. 再发 `状态`
4. 需要提交任务时，用 `/task 你的需求`
5. 不确定怎么表达时，直接用自然语言发需求也可以
6. 如果机器人没有回应，先让管理员检查平台回调、Webhook 和 Dawn 网关状态

## 11. 最后建议

如果你要先做一个最稳的中国区入口，我建议优先用：

- 飞书作为第一接入平台

原因是：

- 上手最快
- 出站简单
- 入站 challenge 容易验证
- 最适合作为 Dawn 在中国平台侧的第一条正式演示链路

如果你要做企业内协同，再补：

- 钉钉
- 微信公众号

如果你要做社区、年轻用户或泛用户入口，再考虑：

- QQ Bot
