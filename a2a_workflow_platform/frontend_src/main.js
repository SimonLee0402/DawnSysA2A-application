import { createApp } from 'vue'
import App from './App.vue'
import router from './router'
import { createPinia } from 'pinia'
import { useAuthStore } from './store/auth'
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
// 导入全局样式
import './assets/styles/global.css'
import { getAxios } from './api/use-axios'
import { getApiClient } from './api/api-bridge'

// 创建Vue应用实例
const app = createApp(App)

// 创建pinia状态管理实例
const pinia = createPinia()

// 添加错误处理
app.config.errorHandler = (err, instance, info) => {
  console.error('Vue全局错误:', err)
  console.error('错误信息:', info)
}

// 挂载插件
app.use(router)
app.use(pinia)
app.use(ElementPlus)

// 异步加载和初始化应用
async function initApp() {
  try {
    console.log('正在初始化应用...')
    
    // 获取axios和apiClient实例
    const axios = await getAxios()
    const apiClientInstance = await getApiClient()
    
    // 全局挂载axios和apiClient
    app.config.globalProperties.$axios = axios
    app.config.globalProperties.$api = apiClientInstance
    
    // 动态导入API中间件
    const apiMiddlewareModule = await import('./api/api-middleware')
    const apiMiddleware = apiMiddlewareModule.default
    
    // 初始化API中间件，现在是异步的
    try {
      console.log('正在初始化API中间件(main.js)...')
      await apiMiddleware.initApiMiddleware()
      console.log('API中间件初始化成功(main.js)')
    } catch (error) {
      console.error('API中间件初始化失败(main.js):', error)
    }
    
    // 检查认证状态
    const authStore = useAuthStore(pinia)
    try {
      await authStore.checkAuth()
      // 确保localStorage和store状态一致
      if (localStorage.getItem('authenticated') === 'true' && !authStore.isAuthenticated) {
        console.log('本地存储显示已登录但store未同步，强制更新store状态')
        authStore.isAuthenticated = true
      }
    } catch (error) {
      console.error('认证状态检查失败(main.js):', error)
    }
    
    // 挂载应用
    console.log('准备挂载Vue应用到#app元素(main.js)')
    app.mount('#app')
    console.log('Vue应用挂载完成(main.js)')
    
  } catch (error) {
    console.error('应用初始化失败(main.js):', error)
    // 即使有错误也要挂载应用，避免白屏
    if (!app || !app._container) {
      const newApp = createApp(App)
      newApp.use(router).use(pinia).use(ElementPlus).mount('#app')
    } else if (app._container.innerHTML.trim() === '') {
      app.mount('#app')
    }
  }
}

// 运行初始化过程
initApp() 