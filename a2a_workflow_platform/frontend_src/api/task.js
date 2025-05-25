import axios from 'axios'
import { useStore as useNotificationStore } from '../store/notification'

// 获取通知存储
const notificationStore = useNotificationStore()

// 任务相关API
const taskApi = {
  // 获取任务列表
  getTasks: async (params = {}) => {
    try {
      const response = await axios.get('/api/tasks/', { params })
      return response.data
    } catch (error) {
      console.error('获取任务列表失败', error)
      notificationStore.error('获取失败', '无法获取任务列表')
      throw error
    }
  },

  // 获取单个任务详情
  getTask: async (id) => {
    try {
      const response = await axios.get(`/api/tasks/${id}/`)
      return response.data
    } catch (error) {
      console.error('获取任务详情失败', error)
      notificationStore.error('获取失败', '无法获取任务详情')
      throw error
    }
  },

  // 获取特定智能体的任务列表
  getAgentTasks: async (agentId, params = {}) => {
    try {
      const response = await axios.get(`/api/agents/${agentId}/tasks/`, { params })
      return response.data
    } catch (error) {
      console.error('获取智能体任务列表失败', error)
      notificationStore.error('获取失败', '无法获取智能体任务列表')
      throw error
    }
  },

  // 取消任务
  cancelTask: async (id, reason = '用户取消') => {
    try {
      const response = await axios.post(`/api/tasks/${id}/cancel/`, { reason })
      notificationStore.success('取消成功', '任务已被取消')
      return response.data
    } catch (error) {
      console.error('取消任务失败', error)
      notificationStore.error('取消失败', '无法取消当前任务')
      throw error
    }
  },

  // 提交用户输入
  submitTaskInput: async (id, input) => {
    try {
      const response = await axios.post(`/api/tasks/${id}/input/`, input)
      return response.data
    } catch (error) {
      console.error('提交任务输入失败', error)
      notificationStore.error('提交失败', '无法提交用户输入')
      throw error
    }
  },

  // 获取任务状态历史
  getTaskStateHistory: async (id) => {
    try {
      const response = await axios.get(`/api/tasks/${id}/state-history/`)
      return response.data
    } catch (error) {
      console.error('获取任务状态历史失败', error)
      notificationStore.error('获取失败', '无法获取任务状态历史')
      throw error
    }
  },

  // 获取任务关系树
  getTaskTree: async (id) => {
    try {
      const response = await axios.get(`/api/tasks/${id}/tree/`)
      return response.data
    } catch (error) {
      console.error('获取任务树失败', error)
      notificationStore.error('获取失败', '无法获取任务关系树')
      throw error
    }
  }
}

export default taskApi 