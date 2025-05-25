import { defineStore } from 'pinia'

export const useGlobalStore = defineStore('global', {
  state: () => ({
    loading: false,
    loadingMessage: '加载中...',
    sidebarCollapsed: false,
    darkMode: false,
    initialized: false
  }),
  
  actions: {
    /**
     * 开始全局加载
     * @param {string} message 加载提示信息
     */
    startLoading(message = '加载中...') {
      this.loading = true
      this.loadingMessage = message
    },
    
    /**
     * 结束全局加载
     */
    endLoading() {
      this.loading = false
    },
    
    /**
     * 切换侧边栏折叠状态
     */
    toggleSidebar() {
      this.sidebarCollapsed = !this.sidebarCollapsed
    },
    
    /**
     * 切换暗黑模式
     */
    toggleDarkMode() {
      this.darkMode = !this.darkMode
      
      // 存储到本地存储
      localStorage.setItem('darkMode', this.darkMode.toString())
      
      // 更新DOM
      if (this.darkMode) {
        document.documentElement.classList.add('dark')
      } else {
        document.documentElement.classList.remove('dark')
      }
    },
    
    /**
     * 初始化全局状态
     */
    initialize() {
      if (this.initialized) return
      
      // 从本地存储读取暗黑模式状态
      const darkMode = localStorage.getItem('darkMode')
      if (darkMode !== null) {
        this.darkMode = darkMode === 'true'
        
        // 更新DOM
        if (this.darkMode) {
          document.documentElement.classList.add('dark')
        }
      }
      
      this.initialized = true
    }
  }
})

// 创建一个单例实例，方便全局直接访问
let globalStore = null

export const useStore = () => {
  if (!globalStore) {
    globalStore = useGlobalStore()
    if (!globalStore.initialized) {
      globalStore.initialize()
    }
  }
  return globalStore
} 