import { defineStore } from 'pinia'
import { useStore as useNotificationStore } from './notification'
import { loadApiDependencies } from '../api/api-loader'

// 预加载依赖
loadApiDependencies();

export const useAuthStore = defineStore('auth', {
  state: () => ({
    user: null,
    isAuthenticated: false,
    isLoading: false,
    error: null,
    token: localStorage.getItem('token') || null,
    lastAuthCheck: null // 最后一次认证检查时间
  }),

  getters: {
    getUser: (state) => state.user,
    getIsAuthenticated: (state) => state.isAuthenticated,
    getIsLoading: (state) => state.isLoading,
    getError: (state) => state.error,
    getToken: (state) => state.token
  },

  actions: {
    // 清除错误状态
    clearError() {
      this.error = null;
    },

    // 设置认证token
    async setToken(token) {
      this.token = token;
      
      // const { axios, apiClient } = await loadApiDependencies(); // apiClientFromDeps 可能不是我们最终要修改的
      // 修改为从各自的源头获取，并确保操作的是实际的实例
      const { getAxios: getLocalAxios } = await import('../api/use-axios');
      const { getApiClient: getActualApiClient } = await import('../api/api-bridge');

      const localAxios = await getLocalAxios();
      const actualApiClient = await getActualApiClient();
      
      if (token) {
        localStorage.setItem('token', token);
        if (localAxios && localAxios.defaults && localAxios.defaults.headers && localAxios.defaults.headers.common) {
          localAxios.defaults.headers.common['Authorization'] = `Token ${token}`;
        }
        if (actualApiClient && actualApiClient.defaults && actualApiClient.defaults.headers && actualApiClient.defaults.headers.common) {
          actualApiClient.defaults.headers.common['Authorization'] = `Token ${token}`;
          console.log('[AuthStore] setToken - Successfully set Authorization header on actualApiClient');
        } else {
          console.warn('[AuthStore] setToken - actualApiClient or its defaults/headers/common is not defined. Token:', token, actualApiClient);
        }
      } else {
        localStorage.removeItem('token');
        if (localAxios && localAxios.defaults && localAxios.defaults.headers && localAxios.defaults.headers.common) {
          delete localAxios.defaults.headers.common['Authorization'];
        }
        if (actualApiClient && actualApiClient.defaults && actualApiClient.defaults.headers && actualApiClient.defaults.headers.common) {
          delete actualApiClient.defaults.headers.common['Authorization'];
          console.log('[AuthStore] setToken - Successfully cleared Authorization header on actualApiClient');
        }
      }
    },

    // 检查当前用户认证状态
    async checkAuth() {
      // 防止短时间内多次调用
      const now = Date.now()
      if (this.lastAuthCheck && (now - this.lastAuthCheck < 5000)) {
        return this.isAuthenticated
      }
      
      this.isLoading = true
      this.error = null
      this.lastAuthCheck = now
      
      try {
        // 确保依赖已加载
        const { apiClient } = await loadApiDependencies();
        if (!apiClient) {
          console.error('API客户端未加载，认证检查失败');
          return false;
        }
        
        // 先检查是否有token
        const token = localStorage.getItem('token')
        if (token) {
          await this.setToken(token)
        }
        
        // 添加请求重试逻辑
        let retryCount = 0
        const maxRetries = 2
        
        while (retryCount <= maxRetries) {
          try {
            // 使用apiClient来调用API
            const response = await apiClient.get('/users/current/')
            
            // ---- 添加详细日志 ----
            console.log('[AuthStore] checkAuth - API 响应状态:', response.status);
            console.log('[AuthStore] checkAuth - API 响应数据:', JSON.stringify(response.data, null, 2));
            // ---- 结束添加日志 ----

            // 检查响应中是否有authenticated字段，该字段为false表示未登录
            if (response.data.authenticated === false) {
              console.log('[AuthStore] checkAuth - 后端返回未认证');
              await this.clearAuth(); // 确保异步清除
              return false
            }
            
            // ---- 添加状态更新日志 ----
            console.log('[AuthStore] checkAuth - 准备更新用户信息:', response.data);
            this.user = response.data
            this.isAuthenticated = true
            localStorage.setItem('authenticated', 'true')
            console.log('[AuthStore] checkAuth - 用户状态已更新:', JSON.stringify(this.user));
            // ---- 结束添加日志 ----
            return true
          } catch (reqError) {
            retryCount++
            if (retryCount > maxRetries) {
              throw reqError
            }
            // 延迟500ms后重试
            await new Promise(resolve => setTimeout(resolve, 500))
          }
        }
      } catch (error) {
        console.error('获取用户信息失败:', error)
        
        if (error.response) {
          // 只有401/403才清除认证状态
          if (error.response.status === 401 || error.response.status === 403) {
            this.clearAuth()
          } else {
            // 其他错误可能是临时网络问题，不清除认证状态
            console.error('获取用户信息失败，但不清除认证状态:', error.response.status)
          }
        } else {
          // 网络错误等情况，也不立即清除认证状态
          console.error('获取用户信息时出现网络错误:', error)
        }
        return false
      } finally {
        this.isLoading = false
      }
    },

    // 清除认证状态
    async clearAuth() {
      this.user = null
      this.isAuthenticated = false
      await this.setToken(null)
      localStorage.removeItem('authenticated')
    },

    // 用户登录
    async login(credentials) {
      this.isLoading = true
      this.error = null
      let success = false; // Track success
      
      try {
        const { apiClient } = await loadApiDependencies();
        if (!apiClient) {
          throw new Error('API客户端未加载，无法登录');
        }
        
        const loginData = {
          username: credentials.username,
          password: credentials.password
        }
        
        const response = await apiClient.post('/users/login/', loginData)
        
        let token = null;
        if (response.data && response.data.token) {
          token = response.data.token;
        } else if (response.data && response.data.key) {
          token = response.data.key;
        }
        
        if (token) {
          await this.setToken(token);
          this.isAuthenticated = true;
          localStorage.setItem('authenticated', 'true');
          await this.checkAuth(); // Fetch user info
          success = true; // Mark as successful
          console.log('登录流程完成，认证状态:', this.isAuthenticated);
        } else {
          this.error = '认证服务器返回了无效响应，未找到Token或Key。';
          console.error('登录响应中没有找到token或key:', response.data);
          success = false;
        }
        
      } catch (error) {
        console.error('登录失败:', error);
        if (error.response && error.response.data) {
          // Try to extract specific error messages from backend
          const data = error.response.data;
          if (data.non_field_errors) {
            this.error = data.non_field_errors.join(' ');
          } else if (data.detail) {
            this.error = data.detail;
          } else if (typeof data === 'string') {
            this.error = data;
          } else {
            // Generic error if no specific message found
            this.error = `登录失败 (状态码: ${error.response.status})`
          }
        } else if (error.message === 'API客户端未加载，无法登录') {
             this.error = error.message;
        } else {
          this.error = '登录请求失败，请检查网络连接或联系管理员。';
        }
        success = false; // Mark as failed
      } finally {
        this.isLoading = false
      }
      return success; // Return status
    },

    // 用户登出
    async logout() {
      this.isLoading = true
      
      try {
        // 确保依赖已加载
        const { apiClient } = await loadApiDependencies();
        if (apiClient) {
          await apiClient.post('/users/logout/')
        }
      } catch (error) {
        console.error('登出请求失败', error)
      } finally {
        await this.clearAuth()
        this.isLoading = false
        
        // 显示登出成功通知
        const notificationStore = useNotificationStore()
        notificationStore.info('已登出', '您已成功退出登录')
      }
    },

    // 用户注册
    async register(userData) {
      this.isLoading = true
      this.error = null
      
      try {
        // 确保依赖已加载
        const { apiClient } = await loadApiDependencies();
        if (!apiClient) {
          throw new Error('API客户端未加载，无法注册');
        }
        
        // 转换字段名以匹配后端API期望的字段
        const apiData = {
          username: userData.username,
          email: userData.email,
          password: userData.password1,
          password_confirm: userData.password2
        }
        
        console.log('开始注册请求', apiData)
        
        const response = await apiClient.post('/users/register/', apiData)
        console.log('注册API响应:', response.data)
        
        // 显示注册成功通知
        const notificationStore = useNotificationStore()
        notificationStore.success('注册成功', '您已成功注册账号')
        
        // 如果注册成功，自动登录
        if (response.data && response.data.success) {
          console.log('注册成功，尝试自动登录')
          await this.login({
            username: userData.username,
            password: userData.password1
          })
        }
        return true
      } catch (error) {
        console.error('注册失败:', error)
        
        if (error.response && error.response.data) {
          // 处理常见的注册错误情况
          if (error.response.data.username) {
            this.error = `用户名错误: ${error.response.data.username[0]}`
          } else if (error.response.data.email) {
            this.error = `邮箱错误: ${error.response.data.email[0]}`
          } else if (error.response.data.password) {
            this.error = `密码错误: ${error.response.data.password[0]}`
          } else if (error.response.data.password_confirm) {
            this.error = `确认密码错误: ${error.response.data.password_confirm[0]}`
          } else if (error.response.data.non_field_errors) {
            this.error = error.response.data.non_field_errors[0]
          } else {
            this.error = '注册失败，请检查表单信息'
          }
        } else if (error.request) {
          this.error = '网络连接失败，请检查您的网络'
        } else {
          this.error = '注册请求失败，请稍后再试'
        }
        
        throw error; // Re-throw the error
      } finally {
        this.isLoading = false
      }
    },
    
    // 检查用户是否拥有指定权限
    hasPermission(permission) {
      if (!this.user || !this.user.permissions) {
        return false
      }
      return this.user.permissions.includes(permission)
    },

    // 清除所有认证信息，包括存储和HTTP头部
    async clearAllAuthData() {
      this.user = null
      this.isAuthenticated = false
      this.token = null
      
      // 清除localStorage
      localStorage.removeItem('token')
      localStorage.removeItem('authenticated')
      
      // 获取axios和apiClient实例
      const { axios, apiClient } = await loadApiDependencies();
      
      // 清除HTTP头部
      if (axios) {
        delete axios.defaults.headers.common['Authorization']
      }
      
      if (apiClient) {
        delete apiClient.defaults.headers.common['Authorization']
      }
      
      console.log('所有认证数据已清除')
    }
  }
}) 