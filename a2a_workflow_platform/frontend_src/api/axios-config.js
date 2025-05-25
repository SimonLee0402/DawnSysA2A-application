import axios from 'axios'
import router from '../router'
import { useStore as useNotificationStore } from '../store/notification'
import { useStore as useAuthStore } from '../store/auth'

// 创建Axios实例
const axiosInstance = axios.create({
  baseURL: '',  // 使用相对路径，便于开发和生产环境
  timeout: 30000,  // 请求超时时间
  headers: {
    'Content-Type': 'application/json'
  }
})

// 请求拦截器
axiosInstance.interceptors.request.use(
  config => {
    // 从localStorage获取token
    const token = localStorage.getItem('token')
    
    // 如果有token，添加到请求头
    if (token) {
      config.headers['Authorization'] = `Token ${token}`
    }
    
    return config
  },
  error => {
    console.error('请求拦截器错误', error)
    return Promise.reject(error)
  }
)

// 响应拦截器
axiosInstance.interceptors.response.use(
  response => {
    return response
  },
  error => {
    // 获取通知存储
    const notificationStore = useNotificationStore()
    const authStore = useAuthStore()
    
    // 处理错误
    if (error.response) {
      // 服务器响应错误
      const { status } = error.response
      
      // 401错误 - 未授权
      if (status === 401) {
        // 清除用户信息
        authStore.clearAuth()
        
        // 如果不在登录页面，重定向到登录页面
        const currentRoute = router.currentRoute.value
        if (currentRoute.name !== 'Login') {
          notificationStore.error('认证失败', '您的登录已过期，请重新登录')
          router.push({
            name: 'Login',
            query: { redirect: currentRoute.fullPath }
          })
        }
      }
      
      // 403错误 - 权限不足
      else if (status === 403) {
        notificationStore.error('权限错误', '您没有权限执行此操作')
      }
      
      // 404错误 - 资源不存在
      else if (status === 404) {
        // 检查是否是直接请求JS文件
        const url = error.config.url
        if (url && (url.endsWith('.js') || url.includes('/api/') && !url.includes('/api/v'))) {
          console.error('直接请求JS文件的错误', url)
          // 这种错误不显示给用户
        } else {
          notificationStore.error('资源不存在', '请求的资源不存在')
        }
      }
      
      // 429错误 - 请求过多
      else if (status === 429) {
        notificationStore.error('请求限制', '请求过于频繁，请稍后再试')
      }
      
      // 500错误 - 服务器错误
      else if (status >= 500) {
        notificationStore.error('服务器错误', '服务器发生错误，请联系管理员')
      }
    } else if (error.request) {
      // 请求发送了但没有收到响应
      notificationStore.error('网络错误', '无法连接到服务器，请检查您的网络连接')
    } else {
      // 设置请求时发生的错误
      notificationStore.error('请求错误', error.message || '发送请求时出错')
    }
    
    return Promise.reject(error)
  }
)

export default axiosInstance 