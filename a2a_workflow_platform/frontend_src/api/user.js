import axios from 'axios'

// 用户相关API
const userApi = {
  // 获取当前用户信息
  getCurrentUser: async () => {
    try {
      const response = await axios.get('/api/users/current/')
      return response.data
    } catch (error) {
      console.error('获取当前用户信息失败', error)
      throw error
    }
  },

  // 用户登录
  login: async (credentials) => {
    try {
      const response = await axios.post('/api/users/login/', credentials)
      return response.data
    } catch (error) {
      console.error('用户登录失败', error)
      throw error
    }
  },

  // 用户登出
  logout: async () => {
    try {
      await axios.post('/api/users/logout/')
      return true
    } catch (error) {
      console.error('用户登出失败', error)
      throw error
    }
  },

  // 用户注册
  register: async (userData) => {
    try {
      const response = await axios.post('/api/users/register/', userData)
      return response.data
    } catch (error) {
      console.error('用户注册失败', error)
      throw error
    }
  },

  // 更新用户个人资料
  updateProfile: async (userData) => {
    try {
      const response = await axios.patch('/api/users/profile/', userData)
      return response.data
    } catch (error) {
      console.error('更新用户个人资料失败', error)
      throw error
    }
  },

  // 修改密码
  changePassword: async (passwordData) => {
    try {
      await axios.post('/api/users/change-password/', passwordData)
      return true
    } catch (error) {
      console.error('修改密码失败', error)
      throw error
    }
  },

  // 获取用户活动历史
  getUserActivity: async (params = {}) => {
    try {
      const response = await axios.get('/api/users/activity/', { params })
      return response.data
    } catch (error) {
      console.error('获取用户活动历史失败', error)
      throw error
    }
  }
}

export default userApi 