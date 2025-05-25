/**
 * API模块索引
 * 导出所有API，确保正确引用
 */

import agentApi from './agent'
import workflowApi from './workflow'
import sessionApi from './session'
import taskApi from './task'
import a2aApi from './a2a'
import userApi from './user'
import authApi from './auth-repair'

// 导出所有API
export {
  agentApi,
  workflowApi,
  sessionApi,
  taskApi,
  a2aApi,
  userApi,
  authApi as auth
}

// 默认导出所有API
export default {
  agent: agentApi,
  workflow: workflowApi,
  session: sessionApi,
  task: taskApi,
  a2a: a2aApi,
  user: userApi,
  auth: authApi
} 