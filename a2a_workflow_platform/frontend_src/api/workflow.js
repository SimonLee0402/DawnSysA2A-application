import axios from 'axios'
import { useStore as useNotificationStore } from '../store/notification'

// 获取通知存储
const notificationStore = useNotificationStore()

// 配置常量
const API_RETRY_ATTEMPTS = 2  // 重试次数
const API_RETRY_DELAY = 1000  // 重试延迟（毫秒）

// 错误处理函数
const handleApiError = async (error, message, options = {}) => {
  console.error(message, error)
  
  let errorMessage = '操作失败'
  let errorType = '未知错误'
  let errorDetails = null
  let shouldRetry = options.retry !== false && options.retryCount < API_RETRY_ATTEMPTS
  
  // 尝试从响应中获取更详细的错误信息
  if (error.response) {
    // 服务器返回了错误状态码
    const { status, data } = error.response
    
    // 获取详细错误信息
    if (data) {
      if (data.detail) {
        errorMessage = data.detail
      } else if (data.message) {
        errorMessage = data.message
      } else if (data.error) {
        errorMessage = data.error
      }
      
      // 尝试获取更多错误详情
      errorDetails = data.details || data.trace || JSON.stringify(data)
    }
    
    // 根据状态码设置错误类型和消息
    if (status === 400) {
      errorType = '请求错误'
      errorMessage = errorMessage || '请求参数无效'
      shouldRetry = false  // 不重试客户端错误
    } else if (status === 401) {
      errorType = '认证错误'
      errorMessage = '未授权，请登录后重试'
      shouldRetry = false  // 不重试认证错误
      // 可以在这里添加重定向到登录页面的逻辑
    } else if (status === 403) {
      errorType = '权限错误'
      errorMessage = '您没有权限执行此操作'
      shouldRetry = false  // 不重试权限错误
    } else if (status === 404) {
      errorType = '资源不存在'
      errorMessage = errorMessage || '请求的资源不存在'
      shouldRetry = false  // 不重试资源不存在的错误
    } else if (status === 408 || status === 429) {
      errorType = '超时或限流'
      errorMessage = errorMessage || '请求超时或被限流，请稍后再试'
      shouldRetry = true  // 这些错误可以重试
    } else if (status >= 500) {
      errorType = '服务器错误'
      errorMessage = errorMessage || '服务器错误，请稍后重试'
      shouldRetry = true  // 服务器错误可以重试
    }
  } else if (error.request) {
    // 请求已发送但没有收到响应
    errorType = '网络错误'
    errorMessage = '无法连接服务器，请检查网络连接'
    shouldRetry = true  // 网络错误可以重试
  } else {
    // 设置请求时出错
    errorType = '请求配置错误'
    errorMessage = error.message
    shouldRetry = false  // 请求配置错误通常不重试
  }
  
  // 处理超时错误
  if (error.code === 'ECONNABORTED' || (error.message && error.message.includes('timeout'))) {
    errorType = '请求超时'
    errorMessage = '请求超时，服务器响应时间过长'
    shouldRetry = true  // 超时错误可以重试
  }
  
  // 检查是否应该重试请求
  if (shouldRetry && (!options.retryCount || options.retryCount < API_RETRY_ATTEMPTS)) {
    const retryCount = (options.retryCount || 0) + 1
    console.log(`尝试第 ${retryCount} 次重试...`)
    
    // 添加重试通知
    if (retryCount === 1) {  // 仅在第一次重试时显示通知
      notificationStore.warning(
        '请求失败，正在重试',
        `${message}，正在尝试重新连接 (${retryCount}/${API_RETRY_ATTEMPTS})`,
        { duration: 3000 }
      )
    }
    
    // 等待一段时间后重试
    await new Promise(resolve => setTimeout(resolve, API_RETRY_DELAY * retryCount))
    
    // 尝试重新调用原始函数
    if (options.retryFn && typeof options.retryFn === 'function') {
      try {
        return await options.retryFn(retryCount)
      } catch (retryError) {
        // 如果重试失败，则以最新的错误继续处理
        return handleApiError(retryError, message, { 
          ...options, 
          retryCount, 
          // 如果已达到最大重试次数，则不再重试
          retry: retryCount < API_RETRY_ATTEMPTS 
        })
      }
    }
  }
  
  // 构建错误对象
  const enhancedError = {
    ...error,
    message: errorMessage,
    type: errorType,
    details: errorDetails,
    originalError: error
  }
  
  // 显示通知
  notificationStore.error(message, errorMessage, {
    // 添加错误诊断操作
    actions: [
      {
        label: '查看详情',
        handler: () => {
          console.log('完整错误信息:', enhancedError)
          // 可以在这里添加显示详细错误的弹窗逻辑
        }
      }
    ]
  })
  
  throw enhancedError
}

// 包装API调用方法，添加重试逻辑
const withRetry = (apiCall) => {
  return async (...args) => {
    try {
      return await apiCall(...args)
    } catch (error) {
      // 传递重试函数
      return handleApiError(error, error.message || '请求失败', {
        retryCount: 0,
        retryFn: async (retryCount) => {
          // 重新调用原始函数
          return apiCall(...args)
        }
      })
    }
  }
}

// 工作流相关API
const workflowApi = {
  // 获取工作流列表
  getWorkflows: async (params = {}) => {
    try {
      const response = await axios.get('/api/workflows/', { params })
      return response.data
    } catch (error) {
      return handleApiError(error, '获取工作流列表失败', {
        retryCount: 0,
        retryFn: async () => {
          const response = await axios.get('/api/workflows/', { params })
          return response.data
        }
      })
    }
  },

  // 获取单个工作流详情
  getWorkflow: async (id) => {
    try {
      const response = await axios.get(`/api/workflows/${id}/`)
      return response.data
    } catch (error) {
      return handleApiError(error, '获取工作流详情失败')
    }
  },

  // 创建工作流
  createWorkflow: async (workflowData) => {
    try {
      const response = await axios.post('/api/workflows/', workflowData)
      notificationStore.success('创建成功', '工作流已成功创建')
      return response.data
    } catch (error) {
      return handleApiError(error, '创建工作流失败')
    }
  },

  // 更新工作流
  updateWorkflow: async (id, workflowData) => {
    try {
      const response = await axios.put(`/api/workflows/${id}/`, workflowData)
      notificationStore.success('更新成功', '工作流已成功更新')
      return response.data
    } catch (error) {
      return handleApiError(error, '更新工作流失败')
    }
  },

  // 删除工作流
  deleteWorkflow: async (id) => {
    try {
      await axios.delete(`/api/workflows/${id}/`)
      notificationStore.success('删除成功', '工作流已成功删除')
      return true
    } catch (error) {
      return handleApiError(error, '删除工作流失败')
    }
  },

  // 获取工作流实例列表
  getWorkflowInstances: async (params = {}) => {
    try {
      const response = await axios.get('/api/workflows/instances/', { params })
      return response.data
    } catch (error) {
      return handleApiError(error, '获取工作流实例列表失败')
    }
  },

  // 获取单个工作流实例详情
  getWorkflowInstance: async (id) => {
    try {
      const response = await axios.get(`/api/workflows/instances/${id}/`)
      return response.data
    } catch (error) {
      return handleApiError(error, '获取工作流实例详情失败')
    }
  },

  // 启动工作流实例
  startWorkflowInstance: async (workflowId, parameters = {}) => {
    try {
      const response = await axios.post(`/api/workflows/${workflowId}/execute/`, { parameters })
      notificationStore.success('启动成功', '工作流实例已成功启动')
      return response.data
    } catch (error) {
      return handleApiError(error, '启动工作流实例失败')
    }
  },

  // 取消工作流实例
  cancelWorkflowInstance: async (instanceId) => {
    try {
      await axios.post(`/api/workflows/instances/${instanceId}/cancel/`)
      notificationStore.success('取消成功', '工作流实例已取消')
      return true
    } catch (error) {
      return handleApiError(error, '取消工作流实例失败')
    }
  },

  // 暂停工作流实例
  pauseWorkflowInstance: async (instanceId) => {
    try {
      await axios.post(`/api/workflows/instances/${instanceId}/pause/`)
      notificationStore.success('暂停成功', '工作流实例已暂停')
      return true
    } catch (error) {
      return handleApiError(error, '暂停工作流实例失败')
    }
  },

  // 恢复工作流实例
  resumeWorkflowInstance: async (instanceId) => {
    try {
      await axios.post(`/api/workflows/instances/${instanceId}/resume/`)
      notificationStore.success('恢复成功', '工作流实例已恢复')
      return true
    } catch (error) {
      return handleApiError(error, '恢复工作流实例失败')
    }
  },

  // 重试工作流步骤
  retryWorkflowStep: async (instanceId, stepId) => {
    try {
      await axios.post(`/api/workflows/instances/${instanceId}/steps/${stepId}/retry/`)
      notificationStore.success('重试成功', '步骤重试已触发')
      return true
    } catch (error) {
      return handleApiError(error, '重试工作流步骤失败')
    }
  },
  
  // 删除工作流实例
  deleteWorkflowInstance: async (instanceId) => {
    try {
      await axios.delete(`/api/workflows/instances/${instanceId}/`)
      notificationStore.success('删除成功', '工作流实例已删除')
      return true
    } catch (error) {
      return handleApiError(error, '删除工作流实例失败')
    }
  }
}

export default workflowApi 