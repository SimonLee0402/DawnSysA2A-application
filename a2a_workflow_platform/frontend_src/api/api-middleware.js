/**
 * API中间件
 * 用于配置Axios、处理请求/响应拦截和路由冲突
 */

import { useAuthStore } from '../store/auth'
import { useNotificationStore } from '../store/notification'
import router from '../router'
import { loadApiDependencies } from './api-loader' // 引入统一的依赖加载器

// 配置Axios默认值
const configureAxios = async () => {
  const { axios, apiClient } = await loadApiDependencies();
  if (!axios || !apiClient) {
    console.error('API中间件: Axios或ApiClient未加载，无法配置');
    return;
  }

  // 设置请求超时
  axios.defaults.timeout = 30000
  
  // 设置CSRF头部
  const getCookie = (name) => {
    const value = `; ${document.cookie}`
    const parts = value.split(`; ${name}=`)
    if (parts.length === 2) return parts.pop().split(';').shift()
    return null
  }
  
  const csrftoken = getCookie('csrftoken')
  if (csrftoken) {
    axios.defaults.headers.common['X-CSRFToken'] = csrftoken
    if (apiClient.defaults && apiClient.defaults.headers && apiClient.defaults.headers.common) {
        apiClient.defaults.headers.common['X-CSRFToken'] = csrftoken
    } else {
        console.warn('apiClient.defaults.headers.common 未定义，无法设置CSRF Token');
    }
  }
  
  // 从localStorage检查Token并设置
  const token = localStorage.getItem('token')
  if (token) {
    axios.defaults.headers.common['Authorization'] = `Token ${token}`
    if (apiClient.defaults && apiClient.defaults.headers && apiClient.defaults.headers.common) {
        apiClient.defaults.headers.common['Authorization'] = `Token ${token}`
    } else {
        console.warn('apiClient.defaults.headers.common 未定义，无法设置Authorization Token');
    }
  }
}

// 创建请求拦截器
const setupRequestInterceptor = async () => {
  const { axios, apiClient } = await loadApiDependencies();
  if (!axios || !apiClient) {
    console.error('API中间件: Axios或ApiClient未加载，无法设置请求拦截器');
    return;
  }

  const setCommonHeaders = (config) => {
    // 设置 CSRF Token
    const getCookie = (name) => {
      const value = `; ${document.cookie}`;
      const parts = value.split(`; ${name}=`);
      if (parts.length === 2) return parts.pop().split(';').shift();
      return null;
    };
    const csrftoken = getCookie('csrftoken');
    if (csrftoken) {
      config.headers['X-CSRFToken'] = csrftoken;
    }

    // 设置 Authorization Token
    const token = localStorage.getItem('token'); // 从 localStorage 获取
    if (token) {
      config.headers['Authorization'] = `Token ${token}`;
      // 添加日志确认Token被设置到请求头
      console.log(`[API Middleware] Request Interceptor: Set Authorization header for ${config.url} with token ${token.substring(0, 10)}...`);
    } else {
      // 如果需要，可以移除 Authorization 头，以防旧的被缓存
      delete config.headers['Authorization'];
      console.log(`[API Middleware] Request Interceptor: No token found, ensured Authorization header is not set for ${config.url}`);
    }
    return config;
  };

  // 设置全局axios拦截器
  axios.interceptors.request.use(setCommonHeaders, 
    error => {
      console.error('API中间件 - axios请求拦截器错误:', error);
      return Promise.reject(error);
    }
  );
  
  // 设置apiClient拦截器
  if (apiClient.interceptors) { // 确保 interceptors 存在
    apiClient.interceptors.request.use(setCommonHeaders, 
      error => {
        console.error('API中间件 - apiClient请求拦截器错误:', error);
        return Promise.reject(error);
      }
    );
  } else {
    console.warn('apiClient.interceptors 未定义，无法设置请求拦截器');
  }
};

// 创建响应拦截器
const setupResponseInterceptor = async () => {
  const { axios, apiClient } = await loadApiDependencies();
  if (!axios || !apiClient) {
    console.error('API中间件: Axios或ApiClient未加载，无法设置响应拦截器');
    return;
  }

  // 全局axios响应拦截器
  axios.interceptors.response.use(
    response => {
      return response
    },
    error => {
      const authStore = useAuthStore()
      const notificationStore = useNotificationStore()
      
      // 处理401未授权错误
      if (error.response && error.response.status === 401) {
        console.warn('API中间件 - axios检测到401未授权，清除认证')
        authStore.clearAuth()
        
        // 重定向到登录页面
        if (router.currentRoute.value.name !== 'Login') {
          router.push({
            name: 'Login',
            query: { redirect: router.currentRoute.value.fullPath }
          })
        }
      }
      
      return Promise.reject(error)
    }
  )
  
  // apiClient响应拦截器
  if (apiClient.interceptors) { // 确保 interceptors 存在
    apiClient.interceptors.response.use(
      response => {
        return response
      },
      error => {
        const authStore = useAuthStore()
        
        // 处理401未授权错误
        if (error.response && error.response.status === 401) {
          console.warn('API中间件 - apiClient检测到401未授权，清除认证')
          authStore.clearAuth()
          
          // 重定向到登录页面
          if (router.currentRoute.value.name !== 'Login') {
            router.push({
              name: 'Login',
              query: { redirect: router.currentRoute.value.fullPath }
            })
          }
        }
        
        return Promise.reject(error)
      }
    )
  } else {
    console.warn('apiClient.interceptors 未定义，无法设置响应拦截器');
  }
}

// 修复前端路由与后端路由冲突
const repairRouteConflicts = () => {
  // 监听路由变化
  router.beforeEach((to, from, next) => {
    console.log('路由拦截:', to.path)
    
    // 对于登录和注册页面，确保在URL中不包含额外的斜杠
    if (to.path === '/login/' || to.path === '/register/') {
      // 重定向到没有尾部斜杠的路径
      next({ path: to.path.replace(/\/$/, ''), query: to.query, replace: true })
      return
    }
    
    // 处理默认情况
    next()
  })
  
  // 监听浏览器的导航错误
  router.onError((error) => {
    console.error('路由错误:', error)
    
    // 如果是导航到错误路径，重定向到首页
    if (error.name === 'NavigationDuplicated') {
      console.log('路由重复导航，尝试修复')
      router.push('/')
    }
  })
}

// 初始化API中间件，现在是异步的
export const initApiMiddleware = async () => {
  console.log('开始异步初始化API中间件...');
  await configureAxios();
  await setupRequestInterceptor();
  await setupResponseInterceptor();
  repairRouteConflicts(); // 这个可以同步执行
  console.log('API中间件异步初始化完成。');
}

export default {
  initApiMiddleware
} 