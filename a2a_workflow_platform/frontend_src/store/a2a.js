import { defineStore } from 'pinia'
import { 
  getAgentCard, 
  sendTask, 
  getTask, 
  cancelTask, 
  getTaskTree,
  getTaskStateHistory,
  runInteroperabilityTest
} from '../api/a2a'

export const useA2AStore = defineStore('a2a', {
  state: () => ({
    currentTask: null,
    taskHistory: [],
    taskResults: {},
    agentCard: null,
    isLoading: false,
    error: null,
    interopTestResults: null
  }),

  getters: {
    getCurrentTask: (state) => state.currentTask,
    getTaskHistory: (state) => state.taskHistory,
    getTaskResults: (state) => state.taskResults,
    getAgentCard: (state) => state.agentCard,
    getInteropTestResults: (state) => state.interopTestResults,
    isTaskInProgress: (state) => state.currentTask && ['submitted', 'working', 'input-required'].includes(state.currentTask.status?.state)
  },

  actions: {
    // 获取代理卡片
    async fetchAgentCard(agentId = null) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await getAgentCard(agentId)
        this.agentCard = response.data
        return this.agentCard
      } catch (error) {
        this.error = '获取Agent Card失败: ' + (error.response?.data?.detail || error.message)
        console.error('获取Agent Card失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 发送任务
    async sendTask(params) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await sendTask(params)
        const result = response.data
        
        if (result.result && result.result.task) {
          this.currentTask = result.result.task
          
          // 将任务添加到历史记录中
          if (!this.taskHistory.some(task => task.taskId === this.currentTask.taskId)) {
            this.taskHistory.unshift(this.currentTask)
            
            // 保持历史记录不超过20条
            if (this.taskHistory.length > 20) {
              this.taskHistory.pop()
            }
          }
          
          return this.currentTask
        } else {
          throw new Error('无效的任务响应')
        }
      } catch (error) {
        this.error = '发送任务失败: ' + (error.response?.data?.error?.message || error.message)
        console.error('发送任务失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 获取任务状态
    async getTaskStatus(taskId) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await getTask(taskId)
        const result = response.data
        
        if (result.result && result.result.task) {
          // 更新当前任务
          if (this.currentTask && this.currentTask.taskId === taskId) {
            this.currentTask = result.result.task
          }
          
          // 更新历史记录中的任务
          const taskIndex = this.taskHistory.findIndex(task => task.taskId === taskId)
          if (taskIndex !== -1) {
            this.taskHistory[taskIndex] = result.result.task
          }
          
          return result.result.task
        } else {
          throw new Error('无效的任务响应')
        }
      } catch (error) {
        this.error = '获取任务状态失败: ' + (error.response?.data?.error?.message || error.message)
        console.error('获取任务状态失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 取消任务
    async cancelTask(taskId, reason = '用户取消') {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await cancelTask(taskId, reason)
        const result = response.data
        
        if (result.result && result.result.task) {
          // 更新当前任务
          if (this.currentTask && this.currentTask.taskId === taskId) {
            this.currentTask = result.result.task
          }
          
          // 更新历史记录中的任务
          const taskIndex = this.taskHistory.findIndex(task => task.taskId === taskId)
          if (taskIndex !== -1) {
            this.taskHistory[taskIndex] = result.result.task
          }
          
          return result.result.task
        } else {
          throw new Error('无效的任务响应')
        }
      } catch (error) {
        this.error = '取消任务失败: ' + (error.response?.data?.error?.message || error.message)
        console.error('取消任务失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 获取任务树
    async fetchTaskTree(taskId) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await getTaskTree(taskId)
        const result = response.data
        
        if (result.result && result.result.taskTree) {
          return result.result.taskTree
        } else {
          throw new Error('无效的任务树响应')
        }
      } catch (error) {
        this.error = '获取任务树失败: ' + (error.response?.data?.error?.message || error.message)
        console.error('获取任务树失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 获取任务状态历史
    async fetchTaskStateHistory(taskId) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await getTaskStateHistory(taskId)
        const result = response.data
        
        if (result.result && result.result.stateHistory) {
          return result.result.stateHistory
        } else {
          throw new Error('无效的状态历史响应')
        }
      } catch (error) {
        this.error = '获取状态历史失败: ' + (error.response?.data?.error?.message || error.message)
        console.error('获取状态历史失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 运行互操作性测试
    async runInteroperabilityTest(params) {
      this.isLoading = true
      this.error = null
      this.interopTestResults = null
      
      try {
        const response = await runInteroperabilityTest(params)
        this.interopTestResults = response.data
        return this.interopTestResults
      } catch (error) {
        this.error = '互操作性测试失败: ' + (error.response?.data?.error || error.message)
        console.error('互操作性测试失败:', error)
        return null
      } finally {
        this.isLoading = false
      }
    },
    
    // 设置互操作性测试结果（用于从历史记录加载）
    setInteropTestResults(results) {
      this.interopTestResults = results
    },
    
    // 清除当前任务
    clearCurrentTask() {
      this.currentTask = null
    },
    
    // 清除错误信息
    clearError() {
      this.error = null
    }
  }
}) 