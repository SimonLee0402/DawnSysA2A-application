/**
 * 集中的Axios实例配置
 * 用于解决模块导入问题并统一axios配置
 */

// 使用动态导入，确保在浏览器环境下正确加载
let axios = null;

// 异步加载axios
async function loadAxios() {
  if (axios === null) {
    try {
      const axiosModule = await import('axios');
      axios = axiosModule.default;
      
      // 配置全局默认值
      axios.defaults.xsrfCookieName = 'csrftoken';
      axios.defaults.xsrfHeaderName = 'X-CSRFToken';
      axios.defaults.withCredentials = true;
      
      // 从localStorage获取token并设置
      const token = localStorage.getItem('token');
      if (token) {
        axios.defaults.headers.common['Authorization'] = `Token ${token}`;
      }
      
      console.log('Axios模块加载成功');
    } catch (error) {
      console.error('Axios模块加载失败:', error);
      // 创建一个简单的模拟对象，避免完全崩溃
      axios = {
        get: () => Promise.reject(new Error('Axios加载失败')),
        post: () => Promise.reject(new Error('Axios加载失败')),
        put: () => Promise.reject(new Error('Axios加载失败')),
        delete: () => Promise.reject(new Error('Axios加载失败')),
        create: () => ({ 
          get: () => Promise.reject(new Error('Axios加载失败')),
          post: () => Promise.reject(new Error('Axios加载失败')),
          put: () => Promise.reject(new Error('Axios加载失败')),
          delete: () => Promise.reject(new Error('Axios加载失败'))
        })
      };
    }
  }
  return axios;
}

// 预加载axios
loadAxios();

// 导出axios对象和创建客户端的函数
export async function getAxios() {
  return await loadAxios();
}

export async function createApiClient(baseURL = '') {
  const axiosInstance = await loadAxios();
  return axiosInstance.create({
    baseURL,
    timeout: 30000,
    withCredentials: true
  });
}

export default {
  getAxios,
  createApiClient
}; 