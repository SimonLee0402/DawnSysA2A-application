# Agent-to-Agent (A2A) 工作流平台

一个基于Google Agent-to-Agent (A2A) 协议构建的平台，旨在促进不同AI智能体之间的互操作性，并提供强大的工作流编排能力。

## 快速启动

### 方法1：使用批处理文件（Windows）

双击以下文件之一启动服务：

- `start-all.bat` - 同时启动前端和后端服务器
- `start-backend.bat` - 仅启动Django后端服务器
- `start-frontend.bat` - 仅启动Vue前端开发服务器

### 方法2：使用PowerShell脚本（Windows）

右键点击 `Start-A2APlatform.ps1` 并选择"使用PowerShell运行"。

### 方法3：使用VSCode任务（推荐开发者使用）

1. 在VSCode中打开项目
2. 按 `Ctrl+Shift+P` 打开命令面板
3. 输入 "Tasks: Run Task" 并选择
4. 选择 "启动完整平台" 任务

## 访问应用

启动服务后，请在浏览器中访问：

- 前端应用：http://localhost:3000
- 后端API：http://127.0.0.1:8000/api

## 项目结构

- `a2a_workflow_platform/` - Django后端项目
  - `frontend_src/` - Vue前端项目

## 开发指南

### 前端开发

前端代码位于 `a2a_workflow_platform/frontend_src/` 目录。修改代码后，Vue开发服务器会自动热重载。

### 后端开发

后端代码位于 `a2a_workflow_platform/` 目录。修改Python代码后，Django开发服务器通常会自动重载。

## 项目概述

Agent-to-Agent (A2A) 工作流平台的核心功能围绕智能体之间的交互和协作展开，主要包括：

1.  **A2A 智能体管理与集成**：支持注册、配置和管理兼容 A2A 协议的智能体，并提供与各种 AI 智能体的无缝集成能力。
2.  **工作流编排与执行**：提供直观的可视化编辑器用于编排智能体协作流程，以及高效的执行引擎来驱动这些工作流。
3.  **智能体能力发现与调用**：平台能够发现智能体的A2A能力（如技能），并在工作流中进行调用。
4.  **工作流监控与智能体交互跟踪**：实时监控工作流执行状态，并跟踪智能体之间的详细交互过程。
5.  **用户与权限管理**：细粒度的权限控制，支持多用户协作管理智能体和工作流。

## 系统架构

A2A 工作流平台采用 Django 框架构建后端，Vue.js 构建前端。系统主要由以下核心模块组成：

*   **A2A 客户端模块 (`a2a_client/`)：** 负责实现 Google A2A 协议的客户端逻辑，处理平台与外部 A2A 兼容智能体之间的通信和交互。
*   **Agent 模块 (`agents/`)：** 管理平台内的智能体配置信息，包括 A2A 兼容智能体的元数据和凭证。
*   **Workflow 模块 (`workflow/`)：** 负责工作流的定义、存储和执行。工作流步骤可以调用 Agent 模块管理的智能体能力。
*   **User 模块 (`users/`)：** 提供用户认证、授权和管理功能。
*   **Frontend 模块 (`frontend_src/`)：** 提供用户界面，包括智能体管理、工作流编辑器、监控等。
*   **a2a_platform 模块 (`a2a_platform/`)：** 项目的核心配置和通用功能。

### 数据模型

主要数据模型围绕 A2A 智能体和工作流展开，包括：

*   **A2A 智能体 (Agent):** 存储智能体的基本信息、A2A 相关配置和能力（技能）。
*   **工作流 (Workflow):** 存储工作流的定义，描述智能体之间的协作流程。
*   **工作流实例 (WorkflowInstance):** 存储工作流的每次执行状态和结果。
*   **工作流步骤 (WorkflowStep):** 存储工作流中的单个执行步骤，通常对应对一个智能体能力的调用。
*   **用户 (User):** 系统用户。

## 安装指南

### 系统要求

- Python 3.8+
- Django 5.2+
- PostgreSQL或SQLite数据库
- Node.js 14+ (用于前端资源构建)

### 本地开发环境设置

1. **克隆仓库**

```bash
git clone <仓库地址>
cd a2a_workflow_platform
```

2. **创建并激活虚拟环境**

```bash
python -m venv venv
source venv/bin/activate  # Linux/Mac
venv\Scripts\activate  # Windows
```

3. **安装依赖**

```bash
pip install -r requirements.txt
```

4. **配置环境变量**

创建`.env`文件并配置必要的环境变量：

```
DEBUG=True
SECRET_KEY=your_secret_key
DATABASE_URL=sqlite:///db.sqlite3
AGENT_CREDENTIALS_SECRET=your_encryption_key
```

5. **数据库初始化**

```bash
python manage.py migrate
```

6. **创建管理员用户**

```bash
python manage.py createsuperuser
```

7. **启动开发服务器**

```bash
python manage.py runserver
```

访问 http://127.0.0.1:8000/ 进入系统。

## 项目结构

```
a2a_workflow_platform/
├── a2a_client/            # A2A客户端应用
├── a2a_platform/          # 项目核心配置
├── frontend/              # 前端界面应用
├── static/                # 静态资源
├── templates/             # HTML模板
├── users/                 # 用户认证和权限管理
├── workflow/              # 工作流定义和执行模块
├── manage.py              # Django管理脚本
└── requirements.txt       # 项目依赖
```

## 贡献指南

1. Fork项目仓库
2. 创建您的特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交您的修改 (`git commit -m 'Add some amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 打开Pull Request

## 许可证

本项目采用[MIT许可证](LICENSE)。

## 联系方式

如有任何问题或建议，请联系项目维护者。 