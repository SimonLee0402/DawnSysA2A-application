# Agent-to-Agent (A2A) 工作流平台 - API参考

本文档详细说明 Agent-to-Agent (A2A) 工作流平台提供的所有 API 接口。这些 API 围绕 Agent 之间的互操作性和工作流编排构建，遵循 RESTful 风格。

## API 概述

A2A 工作流平台 API 设计用于支持智能体注册、能力发现、工作流编排与执行以及用户管理。API 使用 HTTP 标准方法，请求和响应均为 JSON 格式，使用标准 HTTP 状态码，并提供详细错误信息。

## 认证与权限

### 认证方式

系统支持以下认证方式：

1. **会话认证（Cookie）**：适用于浏览器环境
2. **Token认证**：适用于API集成和第三方应用
3. **基本认证**：用户名密码认证，主要用于调试

#### 获取认证Token

```
POST /api/users/login/
```

请求体：

```json
{
  "username": "your_username",
  "password": "your_password"
}
```

响应：

```json
{
  "token": "9944b09199c62bcf9418ad846dd0e4bbdfc6ee4b",
  "user_id": 1,
  "user_type": "individual"
}
```

### 使用Token认证

在API请求中，在HTTP头部添加：

```
Authorization: Token 9944b09199c62bcf9418ad846dd0e4bbdfc6ee4b
```

## Agent (智能体) API

本节介绍用于管理平台中 Agent (智能体) 的 API 接口，包括 Agent 的注册、查看、编辑和删除，以及与 Agent A2A 能力相关的功能。

### 获取 Agent 列表

```
GET /api/agents/
```

查询参数：

| 参数 | 类型 | 必须 | 描述 |
|------|------|------|------|
| page | int | 否 | 分页页码 |
| page_size | int | 否 | 每页项目数 |
| search | string | 否 | 搜索关键词 |
| agent_type | string | 否 | 按类型筛选 |
| is_active | bool | 否 | 是否激活 |

响应：

```json
{
  "count": 8,
  "next": null,
  "previous": null,
  "results": [
    {
      "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
      "name": "数据分析助手",
      "description": "专业数据分析AI助手",
      "agent_type": "gpt-4",
      "model_name": "gpt-4-1106-preview",
      "is_active": true,
      "owner": {
        "id": 1,
        "username": "current_user"
      },
      "created_at": "2025-04-01T10:30:00Z",
      "updated_at": "2025-04-01T10:30:00Z"
    },
    // ...更多智能体
  ]
}
```

### 创建智能体

```
POST /api/agents/
```

请求体：

```json
{
  "name": "新智能体",
  "description": "智能体描述",
  "agent_type": "claude-3",
  "model_name": "claude-3-opus-20240229",
  "is_active": true,
  "api_key": "your_api_key_here",
  "api_endpoint": "https://api.example.com/v1",
  "additional_params": {
    "temperature": 0.7,
    "max_tokens": 2000
  }
}
```

响应：

```json
{
  "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "name": "新智能体",
  "description": "智能体描述",
  "agent_type": "claude-3",
  "model_name": "claude-3-opus-20240229",
  "is_active": true,
  "owner": {
    "id": 1,
    "username": "current_user"
  },
  "created_at": "2025-05-01T11:00:00Z",
  "updated_at": "2025-05-01T11:00:00Z",
  "credential": {
    "id": "4fa85f64-5717-4562-b3fc-2c963f66afa7",
    "api_endpoint": "https://api.example.com/v1",
    "additional_params": {
      "temperature": 0.7,
      "max_tokens": 2000
    },
    "created_at": "2025-05-01T11:00:00Z",
    "updated_at": "2025-05-01T11:00:00Z"
  }
}
```

### 获取单个智能体

```
GET /api/agents/{id}/
```

响应：与创建智能体响应相同，但不包含API密钥。

### 更新智能体

```
PUT /api/agents/{id}/
```

请求体：与创建智能体请求相同

响应：与创建智能体响应相同

### 删除智能体

```
DELETE /api/agents/{id}/
```

响应：HTTP 204 No Content

### 测试智能体连接

```
POST /api/agents/{id}/test_connection/
```

请求体：

```json
{
  "message": "你好，这是测试消息",
  "timeout": 10
}
```

响应：

```json
{
  "success": true,
  "response": "你好！我是Claude智能助手。我已收到你的测试消息，连接正常。有什么我可以帮助你的吗？",
  "response_time": 1.24,
  "details": {
    "model": "claude-3-opus-20240229",
    "usage": {
      "input_tokens": 8,
      "output_tokens": 35
    }
  }
}
```

### 发送消息到智能体

```
POST /api/agents/{id}/send_message/
```

请求体：

```json
{
  "message": "请分析以下数据：[数据内容]",
  "session_id": "optional-session-id"
}
```

响应：

```json
{
  "task_id": "5fa85f64-5717-4562-b3fc-2c963f66afa8",
  "response": "根据提供的数据分析，我发现以下几个关键点：\n\n1. 销售趋势呈现季节性波动...",
  "created_at": "2025-05-01T13:30:00Z",
  "session_id": "generated-or-provided-session-id"
}
```

## Workflow (工作流) API

本节介绍用于创建、管理和执行自动化工作流的 API 接口。工作流允许用户编排 Agent 的能力以完成复杂任务。

### 获取工作流列表

```
GET /api/workflows/
```

查询参数：

| 参数 | 类型 | 必须 | 描述 |
|------|------|------|------|
| page | int | 否 | 分页页码 |
| page_size | int | 否 | 每页项目数 |
| search | string | 否 | 搜索关键词 |
| is_public | bool | 否 | 筛选公开工作流 |
| workflow_type | string | 否 | 按类型筛选 |

响应：

```json
{
  "count": 12,
  "next": "http://example.com/api/workflows/?page=2",
  "previous": null,
  "results": [
    {
      "id": 1,
      "name": "数据分析工作流",
      "description": "自动数据分析和生成报告",
      "workflow_type": "data_analysis",
      "is_public": true,
      "tags": ["数据", "分析", "报告"],
      "created_by": {
        "id": 3,
        "username": "data_expert"
      },
      "created_at": "2025-04-15T09:30:00Z",
      "updated_at": "2025-04-16T14:20:00Z",
      "version": 2
    },
    // ...更多工作流
  ]
}
```

### 创建工作流

```
POST /api/workflows/
```

请求体：

```json
{
  "name": "新工作流",
  "description": "工作流描述",
  "workflow_type": "standard",
  "is_public": false,
  "tags": ["标签1", "标签2"],
  "definition": {
    "steps": [
      {
        "id": "step_1",
        "name": "第一步",
        "type": "a2a_client",
        "parameters": {
          "agent_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
          "message": "执行分析"
        }
      }
    ],
    "output": {
      "result": "${step_1.output}"
    }
  }
}
```

响应：

```json
{
  "id": 13,
  "name": "新工作流",
  "description": "工作流描述",
  "workflow_type": "standard",
  "is_public": false,
  "tags": ["标签1", "标签2"],
  "created_by": {
    "id": 1,
    "username": "current_user"
  },
  "created_at": "2025-05-01T10:15:30Z",
  "updated_at": "2025-05-01T10:15:30Z",
  "version": 1,
  "definition": {
    "steps": [
      {
        "id": "step_1",
        "name": "第一步",
        "type": "a2a_client",
        "parameters": {
          "agent_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
          "message": "执行分析"
        }
      }
    ],
    "output": {
      "result": "${step_1.output}"
    }
  }
}
```

### 获取单个工作流

```
GET /api/workflows/{id}/
```

响应：与创建工作流响应相同

### 更新工作流

```
PUT /api/workflows/{id}/
```

请求体：与创建工作流请求相同

响应：与创建工作流响应相同

### 删除工作流

```
DELETE /api/workflows/{id}/
```

响应：HTTP 204 No Content

### 执行工作流

```
POST /api/workflows/{id}/execute/
```

请求体：

```json
{
  "name": "执行实例名称",
  "initial_context": {
    "input_data": "初始输入数据",
    "parameters": {
      "param1": "value1"
    }
  }
}
```

响应：

```json
{
  "instance_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "workflow": 1,
  "name": "执行实例名称",
  "started_by": {
    "id": 1,
    "username": "current_user"
  },
  "status": "created",
  "created_at": "2025-05-01T11:20:00Z"
}
```

## 工作流实例 (Workflow Instance) API

本节介绍用于监控和管理工作流执行实例的 API 接口。

### 获取工作流实例列表

```
GET /api/instances/
```

查询参数：

| 参数 | 类型 | 必须 | 描述 |
|------|------|------|------|
| page | int | 否 | 分页页码 |
| page_size | int | 否 | 每页项目数 |
| workflow | int | 否 | 按工作流ID筛选 |
| status | string | 否 | 按状态筛选 |

响应：

```json
{
  "count": 25,
  "next": "http://example.com/api/instances/?page=2",
  "previous": null,
  "results": [
    {
      "instance_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
      "workflow": {
        "id": 1,
        "name": "数据分析工作流"
      },
      "name": "月度数据分析",
      "started_by": {
        "id": 1,
        "username": "current_user"
      },
      "status": "running",
      "current_step_index": 2,
      "created_at": "2025-05-01T08:00:00Z",
      "started_at": "2025-05-01T08:00:05Z",
      "updated_at": "2025-05-01T08:02:30Z",
      "completed_at": null
    },
    // ...更多实例
  ]
}
```

### 获取工作流实例详情

```
GET /api/instances/{instance_id}/
```

响应：

```json
{
  "instance_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "workflow": {
    "id": 1,
    "name": "数据分析工作流"
  },
  "name": "月度数据分析",
  "started_by": {
    "id": 1,
    "username": "current_user"
  },
  "status": "running",
  "current_step_index": 2,
  "context": {
    "input_data": "初始输入数据",
    "step_1_result": "第一步结果",
    "current_value": 42
  },
  "output": null,
  "error": null,
  "created_at": "2025-05-01T08:00:00Z",
  "started_at": "2025-05-01T08:00:05Z",
  "updated_at": "2025-05-01T08:02:30Z",
  "completed_at": null,
  "steps": [
    {
      "id": 101,
      "step_index": 0,
      "step_id": "step_1",
      "step_name": "数据准备",
      "step_type": "a2a_client",
      "status": "completed",
      "started_at": "2025-05-01T08:00:10Z",
      "completed_at": "2025-05-01T08:00:55Z",
      "input_data": {
        "agent_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
        "message": "准备分析数据: 初始输入数据"
      },
      "output_data": {
        "response": "数据已准备完成，可以进行分析"
      }
    },
    {
      "id": 102,
      "step_index": 1,
      "step_id": "step_2",
      "step_name": "数据分析",
      "step_type": "a2a_client",
      "status": "completed",
      "started_at": "2025-05-01T08:01:00Z",
      "completed_at": "2025-05-01T08:02:00Z",
      "input_data": {
        "agent_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
        "message": "分析处理后的数据"
      },
      "output_data": {
        "response": "分析结果显示...",
        "metrics": {
          "accuracy": 0.95,
          "confidence": 0.89
        }
      }
    },
    {
      "id": 103,
      "step_index": 2,
      "step_id": "step_3",
      "step_name": "生成报告",
      "step_type": "a2a_client",
      "status": "running",
      "started_at": "2025-05-01T08:02:10Z",
      "completed_at": null,
      "input_data": {
        "agent_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
        "message": "根据分析结果生成详细报告"
      },
      "output_data": null
    }
  ]
}
```

### 取消工作流实例

```
POST /api/instances/{instance_id}/cancel/
```

响应：

```json
{
  "instance_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "status": "canceled",
  "updated_at": "2025-05-01T08:05:00Z"
}
```

### 暂停工作流实例

```
POST /api/instances/{instance_id}/pause/
```

响应：

```json
{
  "instance_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "status": "paused",
  "updated_at": "2025-05-01T08:05:00Z"
}
```

### 恢复工作流实例

```
POST /api/instances/{instance_id}/resume/
```

响应：

```json
{
  "instance_id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
  "status": "running",
  "updated_at": "2025-05-01T08:10:00Z"
}
```

## A2A协议API

### 获取Agent Card

```
GET /.well-known/agent.json
```

响应：

```json
{
  "name": "A2A工作流平台",
  "description": "高效的工作流自动化系统，支持多智能体协作",
  "url": "https://example.com/api/a2a/tasks",
  "version": "1.0.0",
  "capabilities": {
    "streaming": true,
    "pushNotifications": false,
    "stateTransitionHistory": true
  },
  "authentication": {
    "schemes": ["apiKey"]
  },
  "defaultInputModes": ["text"],
  "defaultOutputModes": ["text"],
  "skills": [
    {
      "id": "workflow_execution",
      "name": "工作流执行",
      "description": "执行预定义的工作流",
      "inputModes": ["text"],
      "outputModes": ["text"],
      "examples": [
        "请执行数据分析工作流",
        "启动内容生成流程，主题：人工智能"
      ]
    }
  ]
}
```

### 创建A2A任务

```
POST /api/a2a/tasks/
```

请求体：

```json
{
  "jsonrpc": "2.0",
  "method": "createTask",
  "params": {
    "title": "数据分析任务"
  },
  "id": "client-request-id"
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "task": {
      "id": "5fa85f64-5717-4562-b3fc-2c963f66afa8",
      "state": "created",
      "title": "数据分析任务",
      "created_at": "2025-05-01T14:00:00Z"
    }
  },
  "id": "client-request-id"
}
```

### 获取任务状态

```
GET /api/a2a/tasks/{task_id}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "task": {
      "id": "5fa85f64-5717-4562-b3fc-2c963f66afa8",
      "state": "running",
      "title": "数据分析任务",
      "created_at": "2025-05-01T14:00:00Z",
      "updated_at": "2025-05-01T14:01:30Z"
    }
  },
  "id": null
}
```

### 发送消息

```
POST /api/a2a/tasks/{task_id}/messages
```

请求体：

```json
{
  "jsonrpc": "2.0",
  "method": "addMessage",
  "params": {
    "role": "user",
    "parts": [
      {
        "text": "请帮我分析这个销售数据"
      },
      {
        "fileUri": "https://example.com/sales_data.csv",
        "contentType": "text/csv"
      }
    ]
  },
  "id": "client-request-id"
}
```

响应：

```json
{
  "jsonrpc": "2.0",
  "result": {
    "message": {
      "id": "6fa85f64-5717-4562-b3fc-2c963f66afa9",
      "role": "user",
      "created_at": "2025-05-01T14:05:00Z"
    }
  },
  "id": "client-request-id"
}
```

### 获取消息历史

```
GET /api/a2a/tasks/{task_id}/messages
```