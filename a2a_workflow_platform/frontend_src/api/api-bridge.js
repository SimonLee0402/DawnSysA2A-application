/**
 * API桥接模块
 * 解决开发环境和生产环境中的API路径问题
 */

// 获取当前环境信息
const isDevelopment = import.meta.env.DEV
// 将开发环境下的apiBase指向后端实际运行的8000端口
const apiBase = isDevelopment ? '//localhost:8000/api' : '/api'

// 延迟初始化的apiClient
let _apiClient = null;

// 获取apiClient的异步函数
export async function getApiClient() {
  if (!_apiClient) {
    try {
      // 动态导入axios
      const axiosModule = await import('axios');
      const axios = axiosModule.default;
      
      // 创建预配置的axios实例
      _apiClient = axios.create({
        baseURL: apiBase,
        timeout: 30000,
        withCredentials: true
      });
      
      // 正确配置 Axios 实例以处理 Django CSRF token
      _apiClient.defaults.xsrfCookieName = 'csrftoken';
      _apiClient.defaults.xsrfHeaderName = 'X-CSRFToken';

      console.log('API客户端 (api-bridge.js): 新实例已创建。 Headers:', JSON.stringify(_apiClient.defaults.headers.common, null, 2));
    } catch (error) {
      console.error('API客户端 (api-bridge.js): 初始化失败:', error);
      throw error;
    }
  } else {
    console.log('API客户端 (api-bridge.js): 返回已缓存实例。 Headers:', JSON.stringify(_apiClient.defaults.headers.common, null, 2));
  }
  return _apiClient;
}

// 导出API路径函数，确保在不同环境中使用正确的URL
export function getApiUrl(path) {
  // 确保路径以/开头
  const normalizedPath = path.startsWith('/') ? path : `/${path}`
  return `${apiBase}${normalizedPath}`
}

// 导出资源URL函数，处理静态资源路径
export function getResourceUrl(path) {
  // 处理不同环境中的静态资源路径
  if (isDevelopment) {
    return `//localhost:3000${path}`
  } else {
    return path
  }
}

// 为了向后兼容，创建一个暂时的apiClient对象
// 这个对象在实际使用时会被真正的apiClient替换
export const apiClient = {
  get: async (...args) => (await getApiClient()).get(...args),
  post: async (...args) => (await getApiClient()).post(...args),
  put: async (...args) => (await getApiClient()).put(...args),
  delete: async (...args) => (await getApiClient()).delete(...args),
  patch: async (...args) => (await getApiClient()).patch(...args),
  defaults: {
    // 这些会在实际初始化时被替换
    xsrfCookieName: 'csrftoken',
    xsrfHeaderName: 'X-CSRFToken',
    withCredentials: true
  }
};

// 立即初始化
getApiClient().then(client => {
  // 更新apiClient.defaults的引用，使其指向真实客户端的defaults
  Object.defineProperty(apiClient, 'defaults', {
    get: () => client.defaults
  });
}).catch(e => console.error('预初始化API客户端失败:', e));

export default {
  apiClient,
  getApiClient,
  getApiUrl,
  getResourceUrl
} 