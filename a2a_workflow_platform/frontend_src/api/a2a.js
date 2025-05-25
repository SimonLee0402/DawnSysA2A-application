import axios from 'axios'
import { useStore as useNotificationStore } from '../store/notification'

// 获取通知存储
const notificationStore = useNotificationStore()

// A2A协议相关API
const a2aApi = {
  // 获取A2A协议支持的能力列表
  getCapabilities: async () => {
    try {
      const response = await axios.get('/api/a2a/capabilities/')
      return response.data
    } catch (error) {
      console.error('获取A2A能力列表失败', error)
      notificationStore.error('获取失败', '无法获取A2A协议能力列表')
      throw error
    }
  },

  // 获取A2A协议版本信息
  getVersion: async () => {
    try {
      const response = await axios.get('/api/a2a/version/')
      return response.data
    } catch (error) {
      console.error('获取A2A版本信息失败', error)
      notificationStore.error('获取失败', '无法获取A2A协议版本信息')
      throw error
    }
  },

  // 验证A2A消息格式
  validateMessage: async (message) => {
    try {
      const response = await axios.post('/api/a2a/validate/', { message })
      return response.data
    } catch (error) {
      console.error('验证A2A消息失败', error)
      notificationStore.error('验证失败', '无法验证A2A消息格式')
      throw error
    }
  },

  // 发送A2A消息
  sendMessage: async (agentId, message) => {
    try {
      const response = await axios.post(`/api/a2a/agents/${agentId}/send/`, { message })
      return response.data
    } catch (error) {
      console.error('发送A2A消息失败', error)
      notificationStore.error('发送失败', '无法发送A2A消息')
      throw error
    }
  },

  // 获取A2A协议规范
  getSpecification: async () => {
    try {
      const response = await axios.get('/api/a2a/specification/')
      return response.data
    } catch (error) {
      console.error('获取A2A规范失败', error)
      notificationStore.error('获取失败', '无法获取A2A协议规范')
      throw error
    }
  }
}

export default a2aApi

/**
 * 获取Agent Card
 * @param {string} agentId - 可选的特定Agent ID
 * @returns {Promise} - 返回Agent Card
 */
export async function getAgentCard(agentId = null) {
  let url = '/.well-known/agent.json'
  if (agentId) {
    url = `/api/a2a/agents/${agentId}/.well-known/agent.json`
  }
  
  return axios.get(url)
}

/**
 * 发送A2A任务
 * @param {Object} params - 任务参数
 * @returns {Promise} - 返回任务结果
 */
export async function sendTask(params) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/send',
    params,
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/send', data)
}

/**
 * 获取A2A任务状态
 * @param {string} taskId - 任务ID
 * @returns {Promise} - 返回任务状态
 */
export async function getTask(taskId) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/get',
    params: { taskId },
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/get', data)
}

/**
 * 取消A2A任务
 * @param {string} taskId - 任务ID
 * @param {string} reason - 取消原因
 * @returns {Promise} - 返回取消结果
 */
export async function cancelTask(taskId, reason = '用户取消') {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/cancel',
    params: { 
      taskId,
      reason
    },
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/cancel', data)
}

/**
 * 使用SSE流式发送任务
 * @param {Object} params - 任务参数
 * @returns {EventSource} - 返回EventSource对象
 */
export function sendTaskWithStreaming(params) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/sendSubscribe',
    params,
    id: String(Date.now())
  }
  
  // 使用EventSource而不是axios
  const evtSource = new EventSource(`/api/a2a/tasks/sendSubscribe?data=${encodeURIComponent(JSON.stringify(data))}`)
  return evtSource
}

/**
 * 获取任务树
 * @param {string} taskId - 任务ID
 * @returns {Promise} - 返回任务树数据
 */
export async function getTaskTree(taskId) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/tree',
    params: {
      taskId,
      operation: 'get'
    },
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/tree', data)
}

/**
 * 更新任务树
 * @param {string} taskId - 任务ID
 * @param {Object} taskTree - 任务树数据
 * @returns {Promise} - 返回更新结果
 */
export async function updateTaskTree(taskId, taskTree) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/tree',
    params: {
      taskId,
      operation: 'update',
      taskTree
    },
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/tree', data)
}

/**
 * 获取任务状态历史
 * @param {string} taskId - 任务ID
 * @returns {Promise} - 返回任务状态历史
 */
export async function getTaskStateHistory(taskId) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/stateHistory',
    params: {
      taskId
    },
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/stateHistory', data)
}

/**
 * 执行A2A互操作性测试
 * @param {Object} params - 测试参数
 * @param {string} params.agent_id - 本地Agent ID
 * @param {string} params.target_url - 目标A2A系统URL
 * @param {string} params.test_type - 测试类型
 * @param {Object} params.options - 可选的测试选项
 * @returns {Promise} - 返回测试结果
 */
export async function runInteroperabilityTest(params) {
  return axios.post('/api/a2a/test/interoperability', params)
}

/**
 * 发送任务输入
 * @param {Object} params - 包含taskId和input的参数对象
 * @returns {Promise} - 返回发送输入结果
 */
export async function sendTaskInput(params) {
  const data = {
    jsonrpc: '2.0',
    method: 'tasks/input',
    params: {
      taskId: params.taskId,
      input: params.input
    },
    id: String(Date.now())
  }
  
  return axios.post('/api/a2a/tasks/input', data)
} 