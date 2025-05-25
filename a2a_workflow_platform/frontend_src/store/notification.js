import { defineStore } from 'pinia'
import { v4 as uuidv4 } from 'uuid'

export const useNotificationStore = defineStore('notification', {
  state: () => ({
    notifications: []
  }),
  
  actions: {
    /**
     * 添加通知
     * @param {Object} notification 通知对象
     * @param {string} notification.title 通知标题
     * @param {string} notification.message 通知消息内容
     * @param {string} notification.type 通知类型 (success, warning, error, info)
     * @param {boolean} notification.autoClose 是否自动关闭
     * @param {number} notification.duration 显示时长(毫秒)
     * @param {Array} notification.actions 操作按钮
     * @returns {string} 通知ID
     */
    addNotification(notification) {
      // 设置默认值
      const defaultNotification = {
        id: uuidv4(),
        title: '',
        message: '',
        type: 'info',
        autoClose: true,
        duration: 5000,  // 默认显示5秒
        timestamp: Date.now(),
        actions: null
      }
      
      // 合并通知对象
      const newNotification = { ...defaultNotification, ...notification }
      
      // 添加到通知列表
      this.notifications.push(newNotification)
      
      return newNotification.id
    },
    
    /**
     * 移除通知
     * @param {string} id 通知ID
     */
    removeNotification(id) {
      const index = this.notifications.findIndex(notification => notification.id === id)
      if (index !== -1) {
        this.notifications.splice(index, 1)
      }
    },
    
    /**
     * 清空所有通知
     */
    clearAllNotifications() {
      this.notifications = []
    },
    
    /**
     * 添加成功通知
     * @param {string} title 通知标题
     * @param {string} message 通知消息
     * @param {Object} options 其他选项
     */
    success(title, message = '', options = {}) {
      return this.addNotification({
        title,
        message,
        type: 'success',
        ...options
      })
    },
    
    /**
     * 添加警告通知
     * @param {string} title 通知标题
     * @param {string} message 通知消息
     * @param {Object} options 其他选项
     */
    warning(title, message = '', options = {}) {
      return this.addNotification({
        title,
        message,
        type: 'warning',
        ...options
      })
    },
    
    /**
     * 添加错误通知
     * @param {string} title 通知标题
     * @param {string} message 通知消息
     * @param {Object} options 其他选项
     */
    error(title, message = '', options = {}) {
      return this.addNotification({
        title,
        message,
        type: 'error',
        autoClose: false,  // 错误通知默认不自动关闭
        ...options
      })
    },
    
    /**
     * 添加信息通知
     * @param {string} title 通知标题
     * @param {string} message 通知消息
     * @param {Object} options 其他选项
     */
    info(title, message = '', options = {}) {
      return this.addNotification({
        title,
        message,
        type: 'info',
        ...options
      })
    }
  }
})

// 为了全局访问方便，创建一个单例
let store = null

export const useStore = () => {
  if (!store) {
    store = useNotificationStore()
  }
  return store
} 