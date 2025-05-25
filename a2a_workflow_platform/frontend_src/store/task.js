import { defineStore } from 'pinia'
import taskApi from '@/api/task'
import { ElMessage } from 'element-plus'

export const useTaskStore = defineStore('task', {
  state: () => ({
    tasks: [],
    currentTask: null,
    taskTree: null,
    taskStateHistory: [],
    pagination: {
      total: 0,
      current: 1,
      pageSize: 10
    },
    filters: {
      agent: '',
      state: '',
      dateRange: []
    },
    isLoading: false,
    error: null
  }),

  getters: {
    getTasks: (state) => state.tasks,
    getCurrentTask: (state) => state.currentTask,
    getTaskTree: (state) => state.taskTree,
    getTaskStateHistory: (state) => state.taskStateHistory,
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

    // 获取任务列表
    async fetchTasks() {
      this.isLoading = true
      this.error = null
      
      try {
        const params = {
          page: this.pagination.current,
          page_size: this.pagination.pageSize,
          ...this.filters
        }
        
        const response = await taskApi.getTasks(params)
        this.tasks = response.results || response
        
        if (response.count !== undefined) {
          this.pagination.total = response.count
        }
      } catch (error) {
        this.error = '获取任务列表失败'
        console.error(error)
      } finally {
        this.isLoading = false
      }
    },

    // 获取单个任务详情
    async fetchTask(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await taskApi.getTask(id)
        this.currentTask = response
        return response
      } catch (error) {
        this.error = '获取任务详情失败'
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 获取特定智能体的任务列表
    async fetchAgentTasks(agentId) {
      this.isLoading = true
      this.error = null
      
      try {
        const params = {
          page: this.pagination.current,
          page_size: this.pagination.pageSize,
          ...this.filters
        }
        
        const response = await taskApi.getAgentTasks(agentId, params)
        this.tasks = response.results || response
        
        if (response.count !== undefined) {
          this.pagination.total = response.count
        }
      } catch (error) {
        this.error = '获取智能体任务列表失败'
        console.error(error)
      } finally {
        this.isLoading = false
      }
    },

    // 取消任务
    async cancelTask(id, reason = '用户取消') {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await taskApi.cancelTask(id, reason)
        
        // 更新任务列表中的状态
        const index = this.tasks.findIndex(task => task.id === id)
        if (index !== -1) {
          this.tasks[index].state = 'canceled'
        }
        
        // 如果当前任务是被取消的任务，也更新它的状态
        if (this.currentTask && this.currentTask.id === id) {
          this.currentTask.state = 'canceled'
        }
        
        ElMessage.success('任务已成功取消')
        return response
      } catch (error) {
        this.error = '取消任务失败'
        ElMessage.error(this.error)
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 提交用户输入
    async submitTaskInput(id, input) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await taskApi.submitTaskInput(id, input)
        ElMessage.success('输入已成功提交')
        return response
      } catch (error) {
        this.error = '提交任务输入失败'
        ElMessage.error(this.error)
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 获取任务状态历史
    async fetchTaskStateHistory(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await taskApi.getTaskStateHistory(id)
        this.taskStateHistory = response
        return response
      } catch (error) {
        this.error = '获取任务状态历史失败'
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 获取任务关系树
    async fetchTaskTree(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await taskApi.getTaskTree(id)
        this.taskTree = response
        return response
      } catch (error) {
        this.error = '获取任务树失败'
        console.error(error)
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 清空状态
    clearState() {
      this.tasks = []
      this.currentTask = null
      this.taskTree = null
      this.taskStateHistory = []
      this.error = null
    }
  }
}) 