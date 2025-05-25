/**
 * Axios实例获取和配置模块
 * 解决模块导入和配置问题
 */

// 为了解决直接import的问题，我们使用CDN URL作为备选方案
const AXIOS_CDN_URL = 'https://cdn.jsdelivr.net/npm/axios/dist/axios.min.js';

// 用于存储axios实例
let axiosInstance = null;

/**
 * 获取Axios实例
 * 首先尝试使用ESM导入，如果失败则从CDN加载
 */
export async function getAxios() {
  if (axiosInstance) {
    return axiosInstance;
  }
  
  try {
    // 尝试ESM导入
    const module = await import('axios');
    axiosInstance = module.default;
    console.log('成功从ESM导入axios');
  } catch (error) {
    console.error('ESM导入axios失败，尝试从CDN加载', error);
    
    // 从CDN加载
    try {
      await loadScript(AXIOS_CDN_URL);
      // 全局axios对象现在应该可用
      axiosInstance = window.axios;
      console.log('成功从CDN加载axios');
    } catch (cdnError) {
      console.error('从CDN加载axios失败', cdnError);
      // 创建一个简单的模拟实现
      axiosInstance = createMockAxios();
    }
  }
  
  // 配置axios实例
  configureAxios(axiosInstance);
  
  return axiosInstance;
}

/**
 * 配置axios实例
 */
function configureAxios(axios) {
  if (!axios) return;
  
  // 设置默认值
  axios.defaults.xsrfCookieName = 'csrftoken';
  axios.defaults.xsrfHeaderName = 'X-CSRFToken';
  axios.defaults.withCredentials = true;
  
  // 从localStorage获取token并设置
  const token = localStorage.getItem('token');
  if (token) {
    axios.defaults.headers.common['Authorization'] = `Token ${token}`;
  }
  
  // 可以添加全局拦截器等
}

/**
 * 创建API客户端
 */
export async function createApiClient(baseURL = '') {
  const axios = await getAxios();
  return axios.create({
    baseURL,
    timeout: 30000,
    withCredentials: true
  });
}

/**
 * 加载外部脚本
 */
function loadScript(url) {
  return new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = url;
    script.onload = () => resolve();
    script.onerror = (e) => reject(new Error(`加载脚本失败: ${url}`));
    document.head.appendChild(script);
  });
}

/**
 * 创建模拟的axios实现
 */
function createMockAxios() {
  const mock = {
    get: () => Promise.reject(new Error('Axios未加载')),
    post: () => Promise.reject(new Error('Axios未加载')),
    put: () => Promise.reject(new Error('Axios未加载')),
    delete: () => Promise.reject(new Error('Axios未加载')),
    patch: () => Promise.reject(new Error('Axios未加载')),
    create: () => mock,
    defaults: {
      headers: {
        common: {}
      }
    }
  };
  return mock;
}

// 预加载axios
getAxios().catch(e => console.error('预加载axios失败', e));

export default {
  getAxios,
  createApiClient
}; 