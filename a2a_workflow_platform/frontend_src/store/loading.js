import { defineStore } from 'pinia'
import axios from 'axios'
import { v4 as uuidv4 } from 'uuid'

export const useLoadingStore = defineStore('loading', {
  state: () => ({
    isLoading: false,
    message: '',
    subMessage: '',
    withBlur: true,
    progress: 0,
    status: '',
    isCancellable: false,
    operationName: '',
    operationId: '',
    errorMessage: '',
    // 存储取消标记（用于Axios取消请求）
    cancelTokens: {}
  }),
  
  actions: {
    /**
     * 显示加载状态
     * @param {Object} options 加载选项
     * @param {string} options.message 主消息
     * @param {string} options.subMessage 次要消息
     * @param {boolean} options.withBlur 是否使用背景模糊
     * @param {boolean} options.isCancellable 是否可取消
     * @param {string} options.operationName 操作名称
     */
    showLoading(options = {}) {
      this.isLoading = true
      this.message = options.message || '加载中...'
      this.subMessage = options.subMessage || ''
      this.withBlur = options.withBlur !== false
      this.progress = options.progress || 0
      this.status = options.status || ''
      this.isCancellable = options.isCancellable === true
      this.operationName = options.operationName || ''
      this.errorMessage = ''
      
      // 生成操作ID
      this.operationId = options.operationId || uuidv4()
      
      // 如果是可取消的操作，创建取消标记
      if (this.isCancellable) {
        this.cancelTokens[this.operationId] = axios.CancelToken.source()
      }
      
      return this.operationId
    },
    
    /**
     * 隐藏加载状态
     */
    hideLoading() {
      this.isLoading = false
      this.message = ''
      this.subMessage = ''
      this.progress = 0
      this.status = ''
      this.isCancellable = false
      this.operationName = ''
      this.operationId = ''
      this.errorMessage = ''
    },
    
    /**
     * 更新加载消息
     * @param {string} message 新的消息
     * @param {string} subMessage 新的次要消息
     */
    updateMessage(message, subMessage = null) {
      this.message = message
      if (subMessage !== null) {
        this.subMessage = subMessage
      }
    },
    
    /**
     * 设置加载进度
     * @param {number} progress 进度百分比（0-100）
     * @param {string} status 进度状态
     */
    setProgress(progress, status = '') {
      this.progress = Math.min(100, Math.max(0, progress))
      if (status) {
        this.status = status
      }
      
      // 如果进度达到100%，自动隐藏加载状态
      if (this.progress >= 100) {
        setTimeout(() => {
          if (this.progress >= 100) {
            this.hideLoading()
          }
        }, 500)
      }
    },
    
    /**
     * 设置错误信息
     * @param {string} errorMessage 错误信息
     */
    setError(errorMessage) {
      this.errorMessage = errorMessage
      this.status = 'exception'
    },
    
    /**
     * 取消当前操作
     * @param {string} operationId 操作ID
     */
    cancelOperation(operationId = null) {
      const id = operationId || this.operationId
      
      if (id && this.cancelTokens[id]) {
        // 取消请求
        this.cancelTokens[id].cancel('操作已被用户取消')
        delete this.cancelTokens[id]
      }
      
      // 更新状态
      this.status = 'exception'
      this.errorMessage = '操作已被用户取消'
      
      // 短暂延迟后隐藏加载状态
      setTimeout(() => {
        this.hideLoading()
      }, 1500)
    },
    
    /**
     * 获取当前操作的取消标记
     * @param {string} operationId 操作ID
     * @returns {CancelToken} Axios取消标记
     */
    getCancelToken(operationId) {
      if (!operationId || !this.cancelTokens[operationId]) {
        return null
      }
      
      return this.cancelTokens[operationId].token
    }
  }
})

// 为了全局访问方便，创建一个单例
let store = null

export const useStore = () => {
  if (!store) {
    store = useLoadingStore()
  }
  return store
} 