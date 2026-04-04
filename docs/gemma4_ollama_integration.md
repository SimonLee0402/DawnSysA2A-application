# Gemma4 本地接入 Dawn 自动化系统

当前工作区的实际模型接入点不是新增一个 provider，而是复用已经存在的 `ollama` connector。

你本机 `D:\Gemma 4` 目录里已经具备以下条件：

- `start-ollama.ps1` 可以后台启动本地 Ollama 服务
- `run-gemma4-e2b.ps1` 会以模型名 `gemma4-e2b-local` 运行
- `Gemma4-E2B-local.Modelfile` 基于 `FROM gemma4:e2b`

## 一次性持久化到 Dawn

在工作区根目录执行：

```powershell
.\dawn.ps1 secrets set OLLAMA_BASE_URL http://127.0.0.1:11434
.\dawn.ps1 secrets set OLLAMA_DEFAULT_MODEL gemma4-e2b-local
```

这样后续通过 `dawn-node gateway start` 或 `dawn.ps1 start --app` 启动时，DawnCore 会自动注入这两个环境变量。

如果你的工作区默认模型 provider 里还没有 `ollama`，再执行一次：

```powershell
.\dawn.ps1 models add ollama
```

## 连通性测试

先启动本地 Ollama：

```powershell
& 'D:\Gemma 4\start-ollama.ps1'
```

然后测试 Dawn 到 Gemma4 的链路：

```powershell
.\dawn.ps1 models test ollama --model gemma4-e2b-local --input "Respond with exactly: GEMMA4_OK"
```

如果你已经设置了 `OLLAMA_DEFAULT_MODEL`，也可以不显式传 `--model`：

```powershell
.\dawn.ps1 models test ollama --input "Respond with exactly: GEMMA4_OK"
```

## 工作流里如何使用

在 A2A 编排里继续使用现有的 `model_connector`：

```json
{
  "kind": "model_connector",
  "provider": "ollama",
  "input": "Summarize {{task.name}}"
}
```

如果已经设置 `OLLAMA_DEFAULT_MODEL=gemma4-e2b-local`，上面这段会默认调用 Gemma4。

如果某个流程想强制指定模型，也可以直接写：

```json
{
  "kind": "model_connector",
  "provider": "ollama",
  "model": "gemma4-e2b-local",
  "input": "Summarize {{task.name}}"
}
```

## 一键启动

仓库根目录新增了 `Start-DawnGemma4.ps1`，它会：

- 持久化 `OLLAMA_BASE_URL`
- 持久化 `OLLAMA_DEFAULT_MODEL`
- 启动 `D:\Gemma 4\start-ollama.ps1`
- 启动 Dawn 桌面链路并打开 `/app`

默认用法：

```powershell
.\Start-DawnGemma4.ps1
```
