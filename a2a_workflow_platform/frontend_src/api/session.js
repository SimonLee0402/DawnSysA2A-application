import axios from 'axios'
import { useStore as useNotificationStore } from '../store/notification'

// 获取通知存储
const notificationStore = useNotificationStore()

// 会话相关API
const sessionApi = {
  // 获取会话列表
  getSessions: async (params = {}) => {
    try {
      const response = await axios.get('/api/sessions/', { params })
      return response.data
    } catch (error) {
      console.error('获取会话列表失败', error)
      notificationStore.error('获取失败', '无法获取会话列表')
      throw error
    }
  },

  // 获取单个会话详情
  getSession: async (id) => {
    try {
      const response = await axios.get(`/api/sessions/${id}/`)
      return response.data
    } catch (error) {
      console.error('获取会话详情失败', error)
      notificationStore.error('获取失败', '无法获取会话详情')
      throw error
    }
  },

  // 创建会话
  createSession: async (sessionData) => {
    try {
      const response = await axios.post('/api/sessions/', sessionData)
      notificationStore.success('创建成功', '会话已成功创建')
      return response.data
    } catch (error) {
      console.error('创建会话失败', error)
      notificationStore.error('创建失败', '无法创建新的会话')
      throw error
    }
  },

  // 更新会话
  updateSession: async (id, sessionData) => {
    try {
      const response = await axios.put(`/api/sessions/${id}/`, sessionData)
      notificationStore.success('更新成功', '会话已成功更新')
      return response.data
    } catch (error) {
      console.error('更新会话失败', error)
      notificationStore.error('更新失败', '无法更新会话')
      throw error
    }
  },

  // 删除会话
  deleteSession: async (id) => {
    try {
      await axios.delete(`/api/sessions/${id}/`)
      notificationStore.success('删除成功', '会话已成功删除')
      return true
    } catch (error) {
      console.error('删除会话失败', error)
      notificationStore.error('删除失败', '无法删除会话')
      throw error
    }
  },

  // 获取会话中的任务列表
  getSessionTasks: async (id, params = {}) => {
    try {
      const response = await axios.get(`/api/sessions/${id}/tasks/`, { params })
      return response.data
    } catch (error) {
      console.error('获取会话任务列表失败', error)
      notificationStore.error('获取失败', '无法获取会话任务列表')
      throw error
    }
  },

  // 向会话中添加任务
  addSessionTask: async (id, taskData) => {
    try {
      const response = await axios.post(`/api/sessions/${id}/tasks/`, taskData)
      notificationStore.success('添加成功', '任务已添加到会话')
      return response.data
    } catch (error) {
      console.error('向会话添加任务失败', error)
      notificationStore.error('添加失败', '无法向会话添加任务')
      throw error
    }
  },

  // 获取会话聊天历史
  getSessionHistory: async (id, params = {}) => {
    try {
      const response = await axios.get(`/api/sessions/${id}/history/`, { params })
      return response.data
    } catch (error) {
      console.error('获取会话历史失败', error)
      notificationStore.error('获取失败', '无法获取会话历史记录')
      throw error
    }
  }
}

export default sessionApi 