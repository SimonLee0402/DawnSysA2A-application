# A2A工作流平台 - Vue迁移

## 项目概述
A2A工作流平台已完成从传统的Django模板视图到现代的Vue.js单页应用架构的迁移。Vue.js现在是平台的唯一前端技术。

## 技术栈
- 后端: Django, Django REST Framework
- 前端: Vue 3, Pinia, Vue Router, Element Plus, Axios
- 构建工具: Vite

## 目录结构
```
a2a_workflow_platform/
├── a2a_platform/        # Django项目配置
├── frontend_src/        # Vue 3前端源代码
│   ├── api/             # API服务
│   ├── components/      # Vue组件
│   ├── router/          # 路由配置
│   ├── store/           # Pinia状态管理
│   ├── views/           # 页面视图
│   ├── App.vue          # 根组件
│   └── main.js          # 入口文件
├── templates/           # Django模板（只保留Vue入口模板）
└── static/              # 静态资源
    └── vue/             # Vue构建输出目录
```

## 开发环境设置
1. 安装Node.js和npm
2. 安装依赖:
   ```bash
   cd a2a_workflow_platform
   npm install
   ```
3. 启动开发服务器:
   ```bash
   npm run dev
   ```
4. 同时启动Django服务器:
   ```bash
   python manage.py runserver
   ```

## 构建生产版本
```bash
npm run build
```
构建输出将放置在`a2a_workflow_platform/static/vue/`目录中。

## 路由说明
- Vue应用: `/`（主入口）和 `/app/`（可选）

## 迁移计划
1. ✅ 搭建Vue项目结构
2. ✅ 实现核心组件和视图
3. ✅ 集成API服务
4. ✅ 清理不需要的旧HTML模板
5. ✅ 将Vue应用设为默认前端
6. ⬜ 完成所有功能页面开发
7. ⬜ 测试和修复问题

## 注意事项
- Vue应用现在是唯一的前端入口
- 所有Django路由都重定向到Vue应用
- API端点继续服务于前端

## 最近更新
- 2025-05-07: 完成前端迁移，Vue.js成为唯一前端技术
- 2025-05-07: 修复了依赖问题，清理了旧HTML模板文件，优化了工作区结构
- 2025-05-05: 创建了核心组件和API服务
- 2025-05-01: 设置Vue基础架构

## 开发团队
- 前端开发: [前端团队成员]
- 后端开发: [后端团队成员]
- 项目管理: [项目经理] 