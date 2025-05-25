/**
 * API导出集中器
 * 该文件集中导出所有API相关的函数，便于其他模块导入
 */

// 从各模块导入
import { getAxios, createApiClient } from './use-axios';
import { getApiClient, apiClient, getApiUrl, getResourceUrl } from './api-bridge';
import { loadApiDependencies } from './api-loader';

// 统一导出
export {
  // use-axios模块
  getAxios,
  createApiClient,
  
  // api-bridge模块
  getApiClient,
  apiClient,
  getApiUrl,
  getResourceUrl,
  
  // api-loader模块
  loadApiDependencies
};

// 默认导出所有API
export default {
  getAxios,
  createApiClient,
  getApiClient,
  apiClient,
  getApiUrl,
  getResourceUrl,
  loadApiDependencies
}; 