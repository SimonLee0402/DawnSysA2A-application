/**
 * API修复脚本
 * 在浏览器控制台执行此脚本，修复直接请求API JS文件的问题
 */

(function() {
  console.log('开始执行API修复...');
  
  // API映射
  const apiModules = {
    'a2a.js': {},
    'task.js': {},
    'session.js': {},
    'agent.js': {},
    'workflow.js': {}
  };
  
  // 拦截XHR请求
  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url, async, user, password) {
    // 检查是否是对API JS文件的请求
    if (typeof url === 'string' && url.match(/\/api\/.*\.js$/)) {
      console.warn(`拦截到对API JS文件的请求: ${url}，修改为本地路径`);
      const parts = url.split('/');
      const filename = parts[parts.length - 1];
      
      // 修改为指向本地资源
      url = `/static/vue/assets/${filename}`;
    }
    
    // 调用原始方法
    return originalOpen.call(this, method, url, async, user, password);
  };
  
  // 拦截fetch请求
  const originalFetch = window.fetch;
  window.fetch = function(input, init) {
    if (typeof input === 'string' && input.match(/\/api\/.*\.js$/)) {
      console.warn(`拦截到对API JS文件的fetch请求: ${input}，修改为本地路径`);
      const parts = input.split('/');
      const filename = parts[parts.length - 1];
      
      // 修改为指向本地资源
      input = `/static/vue/assets/${filename}`;
    }
    
    return originalFetch.call(this, input, init);
  };
  
  // 处理令牌问题
  const token = localStorage.getItem('token');
  if (token) {
    console.log('设置Authorization头部...');
    if (!window.axios) {
      window.axios = {};
      window.axios.defaults = { headers: { common: {} } };
    }
    
    if (!window.axios.defaults) {
      window.axios.defaults = { headers: { common: {} } };
    }
    
    if (!window.axios.defaults.headers) {
      window.axios.defaults.headers = { common: {} };
    }
    
    if (!window.axios.defaults.headers.common) {
      window.axios.defaults.headers.common = {};
    }
    
    window.axios.defaults.headers.common['Authorization'] = `Token ${token}`;
  }
  
  // 修复认证状态
  if (localStorage.getItem('authenticated') === 'true') {
    console.log('确保认证状态一致...');
    if (window.app && window.app.$pinia) {
      const authStore = window.app.$pinia.state.value.auth;
      if (authStore && !authStore.isAuthenticated) {
        authStore.isAuthenticated = true;
        console.log('更新了store认证状态');
      }
    }
  }
  
  console.log('API修复完成！');
})(); 