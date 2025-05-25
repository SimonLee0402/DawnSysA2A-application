/**
 * 身份验证修复工具
 * 用于修复可能存在的令牌和权限问题
 */

import { useAuthStore } from '../store/auth'
import router from '../router'
import { getApiClient, apiClient } from './api-bridge'

// 动态导入axios
let axios = null;
async function getAxios() {
  if (!axios) {
    try {
      const axiosModule = await import('axios');
      axios = axiosModule.default;
    } catch (error) {
      console.error('无法导入axios:', error);
    }
  }
  return axios;
}

/**
 * 执行一次性修复操作
 * 1. 检查并修复localStorage和sessionStorage中的令牌
 * 2. 确保axios默认头部包含正确的Authorization
 * 3. 重新验证用户会话
 * 4. 修复路由冲突
 */
export async function repairAuthentication() {
  console.log('开始修复身份验证...')
  const authStore = useAuthStore()
  
  // 获取axios实例
  const axios = await getAxios();
  const api = await getApiClient();
  
  // 检查当前URL，如果是问题路径则修正
  fixRoutingIssues()
  
  // 确保token存在
  const token = localStorage.getItem('token')
  if (token && axios) {
    console.log('发现已存储的令牌，设置默认Authorization头...')
    axios.defaults.headers.common['Authorization'] = `Token ${token}`
    // apiClient已经在api-bridge中处理了token设置
  }
  
  // 检查认证状态
  const isAuthenticated = localStorage.getItem('authenticated') === 'true'
  if (isAuthenticated && !authStore.isAuthenticated) {
    console.log('本地存储显示已登录但store未同步，更新store状态...')
    authStore.isAuthenticated = true
  }
  
  // 如果本地存储显示已登录，但没有token，尝试重新获取
  if (isAuthenticated && !token) {
    console.log('发现异常状态：已登录但无令牌，尝试重新验证...')
    try {
      // 尝试获取当前用户信息
      await authStore.checkAuth()
    } catch (error) {
      console.error('重新验证失败，清除认证状态:', error)
      authStore.clearAuth()
    }
  }
  
  // 确保CSRF令牌存在
  try {
    await ensureCsrfToken()
  } catch (e) {
    console.error('CSRF令牌获取失败', e)
  }
  
  // 记录当前身份验证状态
  console.log('身份验证修复完成，当前状态:', {
    isAuthenticated: authStore.isAuthenticated,
    hasToken: !!token,
    user: authStore.user
  })
  
  return {
    success: true,
    isAuthenticated: authStore.isAuthenticated,
    hasToken: !!token
  }
}

/**
 * 确保CSRF令牌已设置
 */
async function ensureCsrfToken() {
  try {
    // 先检查cookie是否已存在
    const hasCsrfCookie = document.cookie.includes('csrftoken=')
    
    if (!hasCsrfCookie) {
      console.log('未检测到CSRF令牌，尝试获取...')
      // 调用CSRF视图获取令牌
      await apiClient.get('/csrf/')
      console.log('CSRF令牌已获取')
    } else {
      console.log('CSRF令牌已存在')
    }
    
    // 从cookie获取令牌并设置到axios默认头部
    const csrfToken = getCookie('csrftoken')
    if (csrfToken) {
      const axios = await getAxios();
      if (axios) {
        axios.defaults.headers.common['X-CSRFToken'] = csrfToken
      }
      return true
    }
    return false
  } catch (error) {
    console.error('CSRF令牌获取失败', error)
    return false
  }
}

/**
 * 从cookie中获取指定名称的值
 */
function getCookie(name) {
  const value = `; ${document.cookie}`
  const parts = value.split(`; ${name}=`)
  if (parts.length === 2) return parts.pop().split(';').shift()
  return null
}

/**
 * 修复路由冲突问题
 */
function fixRoutingIssues() {
  // 检查当前URL中的问题
  const path = window.location.pathname
  const query = window.location.search
  
  // 处理登录页面路由问题
  if (path === '/login/' || path === '/accounts/login/') {
    console.log('检测到登录路径问题，修正到Vue路由')
    router.push('/login' + query)
    return true
  }
  
  // 处理注册页面路由问题
  if (path === '/register/' || path === '/accounts/register/') {
    console.log('检测到注册路径问题，修正到Vue路由')
    router.push('/register' + query)
    return true
  }
  
  return false
}

/**
 * 清理所有身份验证数据
 * 用于彻底重置状态
 */
export function clearAllAuthData() {
  console.log('清理所有身份验证数据...')
  
  // 清除localStorage
  localStorage.removeItem('token')
  localStorage.removeItem('authenticated')
  localStorage.removeItem('user')
  
  // 清除sessionStorage
  sessionStorage.removeItem('token')
  sessionStorage.removeItem('authenticated')
  sessionStorage.removeItem('user')
  
  // 清除axios默认头部
  delete axios.defaults.headers.common['Authorization']
  delete apiClient.defaults.headers.common['Authorization']
  
  // 清除store状态
  try {
    const authStore = useAuthStore()
    authStore.clearAuth()
  } catch (e) {
    console.error('清除Auth Store失败', e)
  }
  
  console.log('所有身份验证数据已清理')
  
  return {
    success: true
  }
}

// 立即执行一次修复，确保页面加载时认证状态正确
setTimeout(() => {
  repairAuthentication().catch(e => console.error('自动认证修复失败', e))
}, 100)

export default {
  repairAuthentication,
  clearAllAuthData,
  fixRoutingIssues
} 