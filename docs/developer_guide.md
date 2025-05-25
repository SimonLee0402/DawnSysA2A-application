# Agent-to-Agent (A2A) 工作流平台 - 开发者指南

本文档提供 Agent-to-Agent (A2A) 工作流平台的架构设计、代码结构和开发指南。平台核心在于促进智能体之间的互操作性（基于 Google A2A 协议），并通过工作流对这些智能体能力进行编排和应用。本文旨在帮助开发者理解系统实现原理和进行二次开发。

## 目录

1. [系统架构](#系统架构)
2. [核心模块](#核心模块)
3. [数据模型](#数据模型)
4. [API接口](#api接口)
5. [前端架构](#前端架构)
6. [工作流引擎](#工作流引擎)
7. [扩展开发](#扩展开发)
8. [测试指南](#测试指南)
9. [部署方案](#部署方案)

## 系统架构

Agent-to-Agent (A2A) 工作流平台采用标准的 Django 框架构建后端，Vue.js 构建前端。系统架构设计围绕 A2A 智能体的集成、管理和工作流编排展开，主要组件如下：

```
+-----------------------+    +-----------------------+    +-----------------------+
|     前端 (Frontend)    |<-->|      API (Views)      |<-->|     模型 (Models)     |
+-----------------------+    +-----------------------+    +-----------------------+
          ^                          ^                          ^
          |                          |                          |
+-----------------------+    +-----------------------+    +-----------------------+
| 工作流引擎 (Workflow)  |<-->| A2A 客户端 (a2a_client) |<-->|    外部 A2A Agent    |
+-----------------------+    +-----------------------+    +-----------------------+
          ^                          ^
          |                          |
+-----------------------+    +-----------------------+
|      用户 (Users)      |    |       数据库 (DB)      |
+-----------------------+    +-----------------------+
```

系统主要由以下几个应用组成，核心在于 `a2a_client` 和 `agents` 模块，它们提供了与 A2A 智能体交互的基础能力，而 `workflow` 模块在此基础上进行编排：

*   **a2a_platform**: 核心配置和URL路由。
*   **a2a_client**: **A2A 客户端实现，负责与外部 A2A 兼容智能体进行通信。**
*   **agents**: **平台内部 Agent 的管理和配置，存储 A2A 智能体信息。**
*   **workflow**: **工作流定义和执行引擎，用于编排和调用 Agent 的 A2A 能力。**
*   **frontend**: 用户界面，提供智能体管理、工作流编辑器等。
*   **users**: 用户认证和权限管理。

## 核心模块

### a2a_platform

主应用配置模块，包含：

- **settings.py**: 系统配置，包括数据库、中间件、静态资源等
- **urls.py**: 主URL路由配置
- **wsgi.py/asgi.py**: Web服务网关接口

### a2a_client 模块

**与外部 A2A 智能体进行标准协议交互的客户端模块。**

*   **models.py**: A2A 任务和消息相关数据模型。
*   **views_a2a.py**: **实现符合 A2A 协议的对外接口，供外部系统调用 (如果平台也作为 A2A 提供者)。**
*   **client.py**: **A2A 客户端实现，用于调用外部 A2体提供的 A2A 接口。**

#### Agent Card 的处理

Agent Card 是 Google A2A 协议中用于描述 Agent 自身信息、能力（技能）和交互方式的核心元数据。它通常是一个 JSON 文件，供其他 A2A 客户端发现和理解 Agent 的功能。

**1. 系统 Agent Card 的生成与暴露**

我们的 Agent-to-Agent 工作流平台自身也可以被视为一个 Agent，并暴露其能力（例如执行工作流的能力）供其他 A2A 客户端调用。系统的 Agent Card 是根据平台自身的配置和能力动态生成的，并通常通过一个标准化的 HTTP 端点暴露，例如 `/.well-known/agent.json`。

这个 Agent Card 包含了平台的名称、描述、支持的协议版本、认证方式以及提供的技能列表（如"工作流执行"技能）。

**2. 接收和处理外部 Agent Card**

平台作为 A2A 客户端，需要能够接收和理解其他 A2A 兼容智能体提供的 Agent Card。这通常发生在用户在平台中添加一个新的外部 Agent 时。

用户在添加外部 Agent 时，可能需要提供 Agent Card 的 URL 或者直接输入 Agent Card 的内容。平台会读取并解析这些 Agent Card，从中提取关键信息，包括：

*   Agent 的基本信息（名称、描述）
*   Agent 提供的技能列表及其输入输出模式
*   Agent 支持的认证方式
*   Agent 的 A2A 端点 URL (用于后续的任务创建和消息交互)

这些信息会被存储在平台的数据库中 (`agents` 模块的相关模型)，供工作流编排和 Agent 调用时使用。平台会根据 Agent Card 中描述的技能信息，在工作流编辑器中提供相应的选项，让用户能够方便地调用外部 Agent 的特定能力。

开发者在实现新的 Agent 集成时，需要关注如何正确地解析和使用外部 Agent Card 中提供的信息。

### agents 模块

**平台内部管理和配置智能体信息的模块，特别关注 A2A 兼容智能体的注册和元数据。**

*   **models.py**: Agent 定义和相关数据模型（如 Agent 的 A2A 配置、技能等）。
*   **views.py**: Agent 的管理 API 接口（创建、读取、更新、删除）。
*   **serializers.py**: Agent 数据序列化器。

### workflow 模块

**负责定义、存储和执行自动化工作流的模块。工作流的核心在于编排和调用由 `agents` 模块管理的 Agent 的 A2A 能力。**

*   **models.py**: 工作流定义、实例和步骤相关数据模型。
*   **views.py**: 工作流管理和执行 API 接口。
*   **engine.py**: 工作流执行引擎，负责解析工作流定义并调度 Agent 调用。

### frontend 模块

用户界面模块：

- **views.py**: 前端视图函数
- **urls.py**: 前端URL路由
- **templates/**: HTML模板文件
- **static/**: CSS、JS等静态资源

### users 模块

用户认证和权限管理模块：

- **models.py**: 用户模型扩展
- **views.py**: 用户管理API接口
- **serializers.py**: 用户数据序列化器
- **permissions.py**: 用户权限控制

## 数据模型

数据模型主要围绕 A2A 智能体及其在工作流中的应用展开，包括：

### Agent (智能体) 相关模型

#### Agent 模型

```python
class Agent(models.Model):
    """AI Agent定义"""
    id = models.UUIDField(primary_key=True, default=uuid.uuid4)
    name = models.CharField(max_length=100)
    description = models.TextField(blank=True)
    agent_type = models.CharField(max_length=50, choices=AGENT_TYPES)
    model_name = models.CharField(max_length=100)
    is_active = models.BooleanField(default=True)
    owner = models.ForeignKey(User, on_delete=models.CASCADE)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
```

### 工作流相关模型

#### Workflow模型

```python
class Workflow(models.Model):
    """工作流定义模板"""
    id = models.AutoField(primary_key=True)
    name = models.CharField(max_length=255)
    description = models.TextField(blank=True, null=True)
    definition = models.JSONField()  # 工作流定义JSON
    workflow_type = models.CharField(max_length=50, default='standard')
    is_public = models.BooleanField(default=False)
    tags = models.JSONField(default=list, blank=True)
    created_by = models.ForeignKey(User, on_delete=models.CASCADE)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
    version = models.IntegerField(default=1)
```

#### WorkflowInstance模型

```python
class WorkflowInstance(models.Model):
    """工作流执行实例"""
    instance_id = models.UUIDField(primary_key=True, default=uuid.uuid4)
    workflow = models.ForeignKey(Workflow, on_delete=models.CASCADE)
    name = models.CharField(max_length=255, blank=True, null=True)
    started_by = models.ForeignKey(User, on_delete=models.SET_NULL, null=True)
    status = models.CharField(max_length=20, choices=STATUS_CHOICES, default='created')
    current_step_index = models.IntegerField(default=0)
    context = models.JSONField(blank=True, null=True)  # 执行上下文数据
    output = models.JSONField(blank=True, null=True)  # 输出结果
    error = models.TextField(blank=True, null=True)  # 错误信息
    created_at = models.DateTimeField(auto_now_add=True)
    started_at = models.DateTimeField(null=True, blank=True)
    completed_at = models.DateTimeField(null=True, blank=True)
```

#### WorkflowStep模型

```python
class WorkflowStep(models.Model):
    """工作流步骤实例"""
    id = models.AutoField(primary_key=True)
    instance = models.ForeignKey(WorkflowInstance, on_delete=models.CASCADE)
    step_index = models.IntegerField()  # 步骤在工作流中的索引
    step_id = models.CharField(max_length=100)  # 步骤唯一标识
    step_name = models.CharField(max_length=255)
    step_type = models.CharField(max_length=50)
    status = models.CharField(max_length=20, choices=STATUS_CHOICES)
    parameters = models.JSONField(default=dict)  # 步骤参数
    input_data = models.JSONField(blank=True, null=True)  # 输入数据
    output_data = models.JSONField(blank=True, null=True)  # 输出数据
    a2a_task_id = models.UUIDField(null=True, blank=True)  # 关联的A2A任务ID
    started_at = models.DateTimeField(null=True, blank=True)
    completed_at = models.DateTimeField(null=True, blank=True)
```

## API接口

系统提供 RESTful API 接口，用于支持 A2A 智能体管理、工作流编排与执行、用户管理等。使用 Django REST Framework 实现。主要API如下：

### 工作流API

| 端点 | 方法 | 描述 |
|------|------|------|
| /api/workflows/ | GET | 获取工作流列表 |
| /api/workflows/ | POST | 创建新工作流 |
| /api/workflows/{id}/ | GET | 获取单个工作流详情 |
| /api/workflows/{id}/ | PUT | 更新工作流 |
| /api/workflows/{id}/ | DELETE | 删除工作流 |
| /api/workflows/{id}/execute/ | POST | 执行工作流 |
| /api/instances/ | GET | 获取工作流实例列表 |
| /api/instances/{id}/ | GET | 获取单个实例详情 |
| /api/instances/{id}/cancel/ | POST | 取消工作流实例 |

### A2A客户端API

| 端点 | 方法 | 描述 |
|------|------|------|
| /api/agents/ | GET | 获取智能体列表 |
| /api/agents/ | POST | 创建新智能体 |
| /api/agents/{id}/ | GET | 获取单个智能体详情 |
| /api/agents/{id}/ | PUT | 更新智能体 |
| /api/agents/{id}/ | DELETE | 删除智能体 |
| /api/agents/{id}/test_connection/ | POST | 测试智能体连接 |
| /api/tasks/ | GET | 获取任务列表 |
| /api/tasks/{id}/ | GET | 获取单个任务详情 |

### A2A协议API

| 端点 | 方法 | 描述 |
|------|------|------|
| /.well-known/agent.json | GET | 获取Agent Card |
| /api/a2a/tasks/ | POST | 创建新任务 |
| /api/a2a/tasks/{id} | GET | 获取任务状态 |
| /api/a2a/tasks/{id}/messages | POST | 发送消息 |
| /api/a2a/tasks/{id}/messages | GET | 获取消息历史 |

## 前端架构

前端采用Django模板系统和Bootstrap框架，结合jQuery和现代JavaScript实现交互功能。

### 模板结构

```
templates/
├── base.html                # 基本布局模板
├── home.html                # 首页模板
├── registration/            # 用户认证相关模板
└── frontend/                # 前端应用模板
    ├── workflow_list.html   # 工作流列表
    ├── workflow_detail.html # 工作流详情
    ├── workflow_edit.html   # 工作流编辑
    ├── workflow_editor.html # 可视化工作流编辑器
    ├── instance_detail.html # 工作流实例详情
    └── agent_list.html      # 智能体列表
```

### 静态资源

```
static/
├── css/                     # 样式文件
├── js/                      # JavaScript文件
│   ├── workflow-editor.js   # 工作流编辑器逻辑
│   └── workflow-monitor.js  # 工作流监控逻辑
└── img/                     # 图片资源
```

## 工作流引擎

工作流引擎(`workflow/engine.py`)是系统的核心组件，负责解析和执行工作流定义。

### 引擎架构

```
WorkflowEngine
├── 初始化 (__init__)          # 加载工作流实例和定义
├── 启动 (start)               # 开始执行工作流
├── 执行 (_execute_workflow)   # 执行工作流主循环
├── 步骤执行 (_execute_step)   # 根据步骤类型执行不同操作
│   ├── a2a_client 步骤       # 调用A2A智能体
│   ├── condition 步骤        # 执行条件判断
│   ├── loop 步骤             # 执行循环逻辑
│   └── transform 步骤        # 执行数据转换
├── 参数解析 (_resolve_parameters) # 处理变量引用和表达式
├── 条件评估 (_evaluate_condition) # 评估条件表达式
└── 恢复执行 (resume)          # 恢复异步步骤的执行
```

### 工作流定义格式

工作流使用JSON格式定义，基本结构如下：

```json
{
  "steps": [
    {
      "id": "step_1",
      "name": "调用智能体",
      "type": "a2a_client",
      "parameters": {
        "agent_id": "uuid-of-agent",
        "message": "你好，请分析这个数据: ${input_data}"
      }
    },
    {
      "id": "step_2",
      "name": "条件判断",
      "type": "condition",
      "parameters": {
        "condition": {
          "operator": "==",
          "left": "${status}",
          "right": "success"
        },
        "then": "step_3",
        "else": "step_4"
      }
    }
  ],
  "output": {
    "result": "${analysis_result}",
    "status": "${status}"
  }
}
```

### 执行流程

1. 创建工作流实例
2. 初始化工作流上下文
3. 按顺序执行步骤，或根据条件/循环规则调整执行路径
4. 对于异步步骤(如A2A调用)，暂停执行并等待回调
5. 收到回调后，更新上下文并继续执行
6. 所有步骤执行完成后，生成输出结果

## 扩展开发

### 添加新步骤类型

1. 在`workflow/engine.py`的`_execute_step`方法中添加新类型的处理逻辑
2. 实现相应的执行方法
3. 在前端工作流编辑器(`templates/frontend/workflow_editor.html`)添加新步骤的UI组件
4. 更新工作流编辑器JS代码，支持新步骤的编辑和保存

### 集成新的A2A智能体

1. 在`Agent`模型中添加新的智能体类型
2. 在`a2a_client/views.py`的`test_connection`和`send_message`方法中添加新智能体的处理逻辑
3. 在智能体创建表单中添加新类型的选项

### 自定义权限控制

1. 在`permissions.py`中定义新的权限类
2. 在相应的视图中应用权限类
3. 更新前端模板，根据权限显示或隐藏特定功能

## 测试指南

### 单元测试

单元测试主要针对核心逻辑和功能点：

```
tests/
├── test_workflow_engine.py  # 工作流引擎测试
├── test_a2a_client.py       # A2A客户端测试
└── test_permissions.py      # 权限系统测试
```

运行测试：

```bash
python manage.py test
```

### 集成测试

测试完整工作流从创建到执行的端到端流程。

### 性能测试

主要针对工作流引擎和API接口进行负载测试，确保系统在高并发下的稳定性。

## 部署方案

### 开发环境

- Django内建开发服务器
- SQLite数据库
- 日志级别：DEBUG

### 测试环境

- Gunicorn + Nginx
- PostgreSQL数据库
- Redis缓存
- 日志级别：INFO

### 生产环境

- Gunicorn + Nginx
- PostgreSQL数据库 (主从复制)
- Redis集群 (缓存 + 会话存储)
- Celery工作队列 (异步任务处理)
- ElasticSearch (日志和搜索)
- 日志级别：WARNING

### Docker部署

系统提供Docker和Docker Compose配置，支持容器化部署：

```yaml
# docker-compose.yml
version: '3'

services:
  db:
    image: postgres:13
    volumes:
      - postgres_data:/var/lib/postgresql/data/
    env_file:
      - ./.env.prod

  redis:
    image: redis:6
    volumes:
      - redis_data:/data

  web:
    build: .
    restart: always
    volumes:
      - static_volume:/app/static
      - media_volume:/app/media
    depends_on:
      - db
      - redis
    env_file:
      - ./.env.prod

  nginx:
    image: nginx:1.21
    ports:
      - 80:80
      - 443:443
    volumes:
      - static_volume:/app/static
      - media_volume:/app/media
      - ./nginx/conf.d:/etc/nginx/conf.d
      - ./certbot/www:/var/www/certbot
      - ./certbot/conf:/etc/nginx/ssl
    depends_on:
      - web

volumes:
  postgres_data:
  redis_data:
  static_volume:
  media_volume:
```

### CI/CD流程

使用GitHub Actions或Jenkins实现持续集成和部署：

1. 代码提交触发测试
2. 测试通过后构建Docker镜像
3. 将镜像推送至容器仓库
4. 自动部署到测试环境
5. 手动确认后部署到生产环境 