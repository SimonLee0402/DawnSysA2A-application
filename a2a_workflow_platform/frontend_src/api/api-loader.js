/**
 * API加载器模块
 * 用于确保所有API相关依赖正确加载
 */

import { getAxios } from './use-axios';
import { getApiClient } from './api-bridge';

// 延迟加载的依赖项
let _dependencies = null;

/**
 * 加载所有API依赖项
 * 返回axios和apiClient实例
 */
export async function loadApiDependencies() {
  if (_dependencies) {
    return _dependencies;
  }
  
  try {
    console.log('正在加载API依赖项...');
    
    // 同时加载axios和apiClient
    const [axios, apiClient] = await Promise.all([
      getAxios(),
      getApiClient()
    ]);
    
    _dependencies = { axios, apiClient };
    console.log('API依赖项加载成功');
    return _dependencies;
  } catch (error) {
    console.error('API依赖项加载失败:', error);
    // 返回空对象而不是null，避免解构时出错
    return { axios: null, apiClient: null };
  }
}

// 预加载
loadApiDependencies();

export default {
  loadApiDependencies
}; 