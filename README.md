# DawnSys A2A Application

一个面向本地节点、自动化编排、多模型连接器和多通道消息接入的 Rust A2A 网关系统。

当前仓库的运行时主线是 `dawn_core + dawn_node`。旧的 Django / Vue 启动说明已经不再代表项目现状。

## 项目定位

DawnSys 用于把以下能力放到一个统一的自动化系统里：

- A2A 任务接入、编排与执行
- 本地节点注册、心跳、命令下发与结果回传
- 多模型连接器统一调用
- 多聊天平台的入站事件接收与出站消息分发
- Agent Card 发布、导入、发现与远程调用
- Wasm 技能注册、签名分发与激活
- AP2 支付授权与审批流
- 面向操作员和终端用户的 Web 控制界面

## 当前能力

- Rust 网关服务基于 Axum，状态持久化使用 SQLx + SQLite。
- `dawn_node` 可作为本地桌面节点接入网关，通过 WebSocket 接收命令并回传执行结果。
- 模型连接器同时覆盖云端与本地路径，支持 `OpenAI`、`Anthropic`、`Google`、`DeepSeek`、`Qwen`、`Zhipu`、`Moonshot`、`Doubao`、`Ollama` 等。
- 聊天连接器和入站接入覆盖 `Telegram`、`Slack`、`Discord`、`Signal`、`Feishu`、`DingTalk`、`WeCom`、`微信公众号`、`QQ Bot` 等。
- 系统提供 `/.well-known/agent-card.json`、Agent Card 注册表、Marketplace、Approval Center、Control Center 和终端用户工作台。
- 本地模型可以通过 Ollama 直接挂入自动化流程，Gemma4 已支持作为默认本地模型接入。

## 主要目录

- `dawn_core/`
  Rust 网关服务，包含 A2A、AP2、连接器、控制平面、审批中心、市场、Agent Card 与技能注册。
- `dawn_node/`
  本地节点 CLI 与节点运行时。
- `workflow/native_skills/`
  工作区内置技能包。
- `docs/`
  实现说明、接入说明和专题文档。
- `templates/`
  运行期相关模板资源。

## 快速启动

### 依赖

- Rust stable
- Cargo
- Windows + PowerShell

### 直接启动 Dawn

在仓库根目录执行：

```powershell
.\dawn.ps1
```

这条命令会走 `dawn-node start --app` 路径，自动拉起网关、节点预检查并打开工作台。

默认入口：

- 终端用户工作台: [http://127.0.0.1:8000/app](http://127.0.0.1:8000/app)
- 操作员控制台: [http://127.0.0.1:8000/console](http://127.0.0.1:8000/console)
- 健康检查: [http://127.0.0.1:8000/health](http://127.0.0.1:8000/health)
- Agent Card: [http://127.0.0.1:8000/.well-known/agent-card.json](http://127.0.0.1:8000/.well-known/agent-card.json)

### 常用命令

```powershell
.\dawn.ps1 status
.\dawn.ps1 connectors status
.\dawn.ps1 gateway start
.\dawn.ps1 models test ollama --input "Respond with exactly: OK"
```

## 本地 Gemma4 接入

你现在这套仓库已经支持通过现有 `ollama` connector 直接使用本地 Gemma4。

### 一键接入

```powershell
.\Start-DawnGemma4.ps1
```

这个脚本会：

- 持久化 `OLLAMA_BASE_URL`
- 持久化 `OLLAMA_DEFAULT_MODEL=gemma4-e2b-local`
- 启动 `D:\Gemma 4\start-ollama.ps1`
- 启动 Dawn 并打开 `/app`

### 手动接入

```powershell
.\dawn.ps1 secrets set OLLAMA_BASE_URL http://127.0.0.1:11434
.\dawn.ps1 secrets set OLLAMA_DEFAULT_MODEL gemma4-e2b-local
.\dawn.ps1 models add ollama
```

测试链路：

```powershell
.\dawn.ps1 models test ollama --input "Respond with exactly: GEMMA4_OK"
```

## 工作流接入方式

在编排步骤里继续使用现有 `model_connector` 即可：

```json
{
  "kind": "model_connector",
  "provider": "ollama",
  "input": "Summarize {{task.name}}"
}
```

如果已经设置了 `OLLAMA_DEFAULT_MODEL`，系统会默认调用 `gemma4-e2b-local`。如果某个流程想强制指定模型，也可以在步骤里显式写 `model` 字段。

## 文档

- [Rust 网关实现说明](docs/dawn_rust_gateway_implementation.md)
- [Gemma4 本地接入说明](docs/gemma4_ollama_integration.md)
- [AP2 串口签名协议](docs/ap2_serial_signer_protocol.md)
- [API 参考](docs/api_reference.md)

## 许可证

本项目采用 [MIT License](LICENSE)。
