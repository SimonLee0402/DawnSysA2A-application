/**
 * API配置文件
 * 提供基本配置参数
 */

// 基础URL配置
export const baseURL = '';

// API版本
export const apiVersion = 'v1';

// 超时配置（毫秒）
export const timeout = 30000;

// 默认请求头
export const defaultHeaders = {
  'Content-Type': 'application/json'
};

// 导出默认配置
export default {
  baseURL,
  apiVersion,
  timeout,
  defaultHeaders
}; 