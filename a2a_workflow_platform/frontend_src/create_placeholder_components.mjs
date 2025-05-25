/**
 * 为缺少的视图组件创建占位符
 * 运行方式: node create_placeholder_components.mjs
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

// 获取当前目录
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// 需要创建的所有组件列表
const components = [
  // Agent相关组件
  { path: 'views/agent/AgentForm.vue', name: 'AgentForm', title: '智能体表单' },
  { path: 'views/agent/AgentTest.vue', name: 'AgentTest', title: '智能体测试' },
  
  // 认证相关组件
  { path: 'views/auth/Login.vue', name: 'Login', title: '登录' },
  { path: 'views/auth/Register.vue', name: 'Register', title: '注册' },
  { path: 'views/auth/Profile.vue', name: 'Profile', title: '个人资料' },
  
  // 任务相关组件
  { path: 'views/task/TaskList.vue', name: 'TaskList', title: '任务列表' },
  { path: 'views/task/TaskDetail.vue', name: 'TaskDetail', title: '任务详情' },
  { path: 'views/task/TaskCreate.vue', name: 'TaskCreate', title: '创建任务' },
  
  // 会话相关组件
  { path: 'views/session/SessionList.vue', name: 'SessionList', title: '会话列表' },
  { path: 'views/session/SessionDetail.vue', name: 'SessionDetail', title: '会话详情' },
  { path: 'views/session/SessionForm.vue', name: 'SessionForm', title: '会话表单' },
];

// 占位符组件模板
function generatePlaceholderComponent(name, title) {
  const className = name.replace(/([A-Z])/g, '-$1').toLowerCase().replace(/^-/, '');
  
  return `<template>
  <div class="${className}">
    <h1>${title}</h1>
    <el-alert
      title="页面开发中"
      type="info"
      description="此页面正在开发中，敬请期待。"
      show-icon
      :closable="false"
    />
  </div>
</template>

<script>
export default {
  name: '${name}'
}
</script>

<style scoped>
.${className} {
  padding: 20px;
}
</style>
`;
}

// 确保目录存在
function ensureDirectoryExists(filePath) {
  const dirname = path.dirname(filePath);
  if (fs.existsSync(dirname)) {
    return true;
  }
  ensureDirectoryExists(dirname);
  fs.mkdirSync(dirname);
}

// 创建组件
for (const component of components) {
  const filePath = path.resolve(__dirname, component.path);
  
  // 如果文件已存在，跳过
  if (fs.existsSync(filePath)) {
    console.log(`组件 ${component.path} 已存在，跳过`);
    continue;
  }
  
  // 确保目录存在
  ensureDirectoryExists(filePath);
  
  // 生成并写入组件内容
  const content = generatePlaceholderComponent(component.name, component.title);
  fs.writeFileSync(filePath, content, 'utf8');
  
  console.log(`已创建组件: ${component.path}`);
}

console.log('占位符组件创建完成！'); 