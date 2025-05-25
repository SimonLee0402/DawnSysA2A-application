import { createApp } from 'vue'
import App from './App.vue'
import router from './router'
import { createPinia } from 'pinia'
import { useAuthStore } from './store/auth'
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
import axios from 'axios'

// 设置axios默认配置
axios.defaults.xsrfCookieName = 'csrftoken'
axios.defaults.xsrfHeaderName = 'X-CSRFToken'
axios.defaults.withCredentials = true
// 在开发环境中不设置baseURL，使用相对路径
// axios.defaults.baseURL = 'http://localhost:8000'

// 添加拦截器，确保所有请求都包含CSRF令牌
axios.interceptors.request.use(
  config => {
    // 从cookie中获取CSRF令牌
    const csrfToken = document.cookie
      .split('; ')
      .find(row => row.startsWith('csrftoken='))
      ?.split('=')[1];
    
    if (csrfToken) {
      config.headers['X-CSRFToken'] = csrfToken;
    }
    
    // 从localStorage获取认证token
    const token = localStorage.getItem('token');
    if (token) {
      config.headers['Authorization'] = `Token ${token}`;
    }
    
    return config;
  },
  error => {
    return Promise.reject(error);
  }
);

// 添加响应拦截器
axios.interceptors.response.use(
  response => response,
  error => {
    if (error.response) {
      // 服务器返回错误
      console.error('API错误:', error.response.data);
      
      // 未认证时自动跳转到登录页
      if (error.response.status === 401 || error.response.status === 403) {
        router.push('/login');
      }
    }
    return Promise.reject(error);
  }
);

// API直接请求处理：拦截对JS文件的直接请求
console.log('设置API拦截器，处理对JS文件的直接请求...');
const originalFetch = window.fetch;
window.fetch = function(input, init) {
  if (typeof input === 'string' && input.match(/\/api\/.*\.js$/)) {
    console.warn(`拦截到对API JS文件的fetch请求: ${input}`);
    // 返回空对象
    return Promise.resolve(new Response(
      '{}',
      {
        status: 200,
        headers: { 'Content-Type': 'application/javascript' }
      }
    ));
  }
  return originalFetch.apply(this, arguments);
};

// 拦截XMLHttpRequest
const originalOpen = XMLHttpRequest.prototype.open;
XMLHttpRequest.prototype.open = function(method, url, async, user, password) {
  if (typeof url === 'string' && url.match(/\/api\/.*\.js$/)) {
    console.warn(`拦截到对API JS文件的XMLHttpRequest请求: ${url}`);
    this.__intercepted = true;
    url = '/api/__fake_js_file__';
  }
  return originalOpen.call(this, method, url, async, user, password);
};

const originalSend = XMLHttpRequest.prototype.send;
XMLHttpRequest.prototype.send = function() {
  if (this.__intercepted) {
    setTimeout(() => {
      Object.defineProperty(this, 'status', { value: 200 });
      Object.defineProperty(this, 'responseText', { value: '{}' });
      Object.defineProperty(this, 'response', { value: '{}' });
      Object.defineProperty(this, 'readyState', { value: 4 });
      
      if (this.onreadystatechange) {
        this.onreadystatechange(new Event('readystatechange'));
      }
      
      if (this.onload) {
        this.onload(new Event('load'));
      }
    }, 10);
    return;
  }
  return originalSend.apply(this, arguments);
};

// 创建pinia状态管理实例
const pinia = createPinia()

// 创建Vue应用实例
const app = createApp(App)

// 挂载插件
app.use(router)
app.use(pinia)
app.use(ElementPlus)

// 全局挂载axios
app.config.globalProperties.$axios = axios

// 在应用启动前检查认证状态
const authStore = useAuthStore(pinia)
authStore.checkAuth().then(() => {
  console.log('应用初始化 - 认证状态检查:', { 
    storeAuth: authStore.isAuthenticated, 
    localAuth: localStorage.getItem('authenticated') === 'true'
  })
  
  // 确保localStorage和store状态一致
  if (localStorage.getItem('authenticated') === 'true' && !authStore.isAuthenticated) {
    console.log('本地存储显示已登录但store未同步，强制更新store状态')
    authStore.isAuthenticated = true
  }
  
  // 挂载应用
  app.mount('#app')
}).catch(error => {
  console.error('应用初始化过程中发生错误:', error)
  // 即使有错误也要挂载应用
  app.mount('#app')
}) 