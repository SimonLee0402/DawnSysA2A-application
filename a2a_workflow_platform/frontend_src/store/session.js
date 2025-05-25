import { defineStore } from 'pinia'
import sessionApi from '@/api/session'
import { ElMessage } from 'element-plus'

export const useSessionStore = defineStore('session', {
  state: () => ({
    sessions: [],
    currentSession: null,
    sessionTasks: [],
    sessionHistory: [],
    pagination: {
      total: 0,
      current: 1,
      pageSize: 10
    },
    filters: {
      agent: '',
      status: '',
      dateRange: []
    },
    isLoading: false,
    error: null
  }),

  getters: {
    getSessions: (state) => state.sessions,
    getCurrentSession: (state) => state.currentSession,
    getSessionTasks: (state) => state.sessionTasks,
    getSessionHistory: (state) => state.sessionHistory,
    getPagination: (state) => state.pagination,
    getFilters: (state) => state.filters,
    getIsLoading: (state) => state.isLoading,
    getError: (state) => state.error
  },

  actions: {
    // 设置筛选条件
    setFilters(filters) {
      this.filters = { ...this.filters, ...filters }
    },

    // 设置分页
    setPagination(pagination) {
      this.pagination = { ...this.pagination, ...pagination }
    },

    // 获取会话列表
    async fetchSessions() {
      this.isLoading = true
      this.error = null
      
      try {
        const params = {
          page: this.pagination.current,
          page_size: this.pagination.pageSize,
          ...this.filters
        }
        
        const response = await sessionApi.getSessions(params)
        this.sessions = response.results || response
        
        if (response.count !== undefined) {
          this.pagination.total = response.count
        }
      } catch (error) {
        this.error = '获取会话列表失败'
        console.error(error)
      } finally {
        this.isLoading = false
      }
    },

    // 获取单个会话详情
    async fetchSession(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sessionApi.getSession(id)
        this.currentSession = response
        return response
      } catch (error) {
        this.error = '获取会话详情失败'
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 创建会话
    async createSession(sessionData) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sessionApi.createSession(sessionData)
        
        // 将新会话添加到会话列表
        this.sessions.unshift(response)
        
        ElMessage.success('会话创建成功')
        return response
      } catch (error) {
        this.error = '创建会话失败'
        ElMessage.error(this.error)
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 更新会话
    async updateSession(id, sessionData) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sessionApi.updateSession(id, sessionData)
        
        // 更新会话列表中的会话
        const index = this.sessions.findIndex(session => session.id === id)
        if (index !== -1) {
          this.sessions[index] = response
        }
        
        // 如果正在查看的就是被更新的会话，也更新它
        if (this.currentSession && this.currentSession.id === id) {
          this.currentSession = response
        }
        
        ElMessage.success('会话更新成功')
        return response
      } catch (error) {
        this.error = '更新会话失败'
        ElMessage.error(this.error)
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 删除会话
    async deleteSession(id) {
      this.isLoading = true
      this.error = null
      
      try {
        await sessionApi.deleteSession(id)
        
        // 从会话列表中删除
        this.sessions = this.sessions.filter(session => session.id !== id)
        
        // 如果当前查看的就是被删除的会话，清空它
        if (this.currentSession && this.currentSession.id === id) {
          this.currentSession = null
        }
        
        ElMessage.success('会话已成功删除')
        return true
      } catch (error) {
        this.error = '删除会话失败'
        ElMessage.error(this.error)
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 获取会话中的任务列表
    async fetchSessionTasks(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sessionApi.getSessionTasks(id)
        this.sessionTasks = response.results || response
        return response
      } catch (error) {
        this.error = '获取会话任务列表失败'
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 向会话中添加任务
    async addSessionTask(id, taskData) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sessionApi.addSessionTask(id, taskData)
        
        // 将新任务添加到任务列表
        this.sessionTasks.unshift(response)
        
        ElMessage.success('任务已添加到会话')
        return response
      } catch (error) {
        this.error = '向会话添加任务失败'
        ElMessage.error(this.error)
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 获取会话聊天历史
    async fetchSessionHistory(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sessionApi.getSessionHistory(id)
        this.sessionHistory = response.results || response
        return response
      } catch (error) {
        this.error = '获取会话历史失败'
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 清空状态
    clearState() {
      this.sessions = []
      this.currentSession = null
      this.sessionTasks = []
      this.sessionHistory = []
      this.error = null
    }
  }
}) 