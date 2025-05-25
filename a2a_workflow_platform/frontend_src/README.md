# A2A工作流平台前端

这是A2A工作流平台的前端项目，基于Vue 3 + Vite构建。

## 目录结构

```
frontend_src/
  ├── api/                 # API调用接口
  ├── components/          # 可复用组件
  │   ├── layout/          # 布局组件
  │   └── workflow/        # 工作流相关组件
  ├── router/              # 路由配置
  ├── store/               # Pinia状态管理
  ├── views/               # 页面组件
  │   ├── a2a/             # A2A协议相关页面
  │   ├── auth/            # 认证相关页面
  │   └── workflow/        # 工作流相关页面
  ├── App.vue              # 根组件
  ├── main.js              # 入口文件
  └── index.html           # HTML模板
```

## 开发环境设置

### 安装依赖

```bash
cd a2a_workflow_platform
npm run setup   # 安装前端依赖
```

### 开发模式

两种方式运行开发环境：

1. 分别启动前端和后端：

```bash
# 启动后端服务器
npm run backend:run

# 另一个终端中启动前端开发服务器
npm run frontend:dev
```

2. 使用concurrently同时启动前后端：

```bash
npm run dev
```

开发服务器会运行在 http://localhost:3000，并自动代理API请求到Django后端。

## 构建与部署

### 构建前端

```bash
# 只构建前端
npm run frontend:build

# 构建前端并收集静态文件
npm run frontend:deploy
```

### 监视模式

在开发时实时构建前端代码到Django静态目录：

```bash
npm run frontend:watch
```

## 前端与后端集成

前端构建后的文件会输出到 `static/vue` 目录，Django通过`collectstatic`命令收集到`static_collected`目录提供服务。

在模板 `templates/frontend/vue_app.html` 中，系统会根据DEBUG环境决定：
- 开发环境：从Vite开发服务器加载资源
- 生产环境：从Django静态文件加载资源

## 故障排除

如果遇到静态文件问题，请尝试：

1. 清理构建文件：`npm run frontend:clean`
2. 重新构建：`npm run frontend:build`
3. 收集静态文件：`npm run backend:collectstatic`

如果前端路由不正常，请确保Django的URL配置将所有前端路径定向到`VueAppView`。 