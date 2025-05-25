import { defineStore } from 'pinia'
// import axios from 'axios' // 移除 axios 直接导入
import { apiClient } from '../api/api-exports' // 导入 apiClient
import { ElMessage } from 'element-plus'; // 导入 ElMessage 以便在需要时使用

export const useWorkflowStore = defineStore('workflow', {
  state: () => ({
    workflows: [],
    currentWorkflow: null,
    workflowInstances: [],
    currentInstance: null,
    templates: [],
    isLoading: false,
    error: null
  }),

  getters: {
    getWorkflows: (state) => state.workflows,
    getCurrentWorkflow: (state) => state.currentWorkflow,
    getWorkflowInstances: (state) => state.workflowInstances,
    getCurrentInstance: (state) => state.currentInstance,
    getTemplates: (state) => state.templates,
    getIsLoading: (state) => state.isLoading,
    getError: (state) => state.error
  },

  actions: {
    // 获取工作流列表
    async fetchWorkflows() {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await apiClient.get('/workflows/') 
        const responseData = response.data;

        if (responseData && Array.isArray(responseData.results)) {
          this.workflows = responseData.results;
        } else if (Array.isArray(responseData)) {
          this.workflows = responseData;
        } else {
          console.warn('[WorkflowStore] fetchWorkflows: Unexpected response data format. Expected array or { results: [...] }.', responseData);
          this.workflows = [];
          // this.error = '获取工作流列表失败: 响应格式不正确'; // 可选：设置错误状态
        }

        // 确保标签字段存在并且是数组
        this.workflows.forEach(workflow => {
          if (!workflow.tags) {
            workflow.tags = []
          } else if (typeof workflow.tags === 'string') {
            try {
              workflow.tags = JSON.parse(workflow.tags)
            } catch (e) {
              const tagsArray = workflow.tags.split(',').map(tag => tag.trim()).filter(tag => tag); 
              workflow.tags = tagsArray.length > 0 ? tagsArray : [];
            }
          } else if (!Array.isArray(workflow.tags)) {
            console.warn(`[WorkflowStore] Workflow ID ${workflow.id} has non-array/non-string tags:`, workflow.tags);
            workflow.tags = [];
          }
        });

      } catch (error) {
        this.error = '获取工作流列表失败'
        this.workflows = []; // 确保在出错时 workflows 是一个空数组
        console.error('[WorkflowStore] fetchWorkflows error:', error.response ? error.response.data : error.message);
      } finally {
        this.isLoading = false
      }
    },

    // 获取单个工作流详情
    async fetchWorkflow(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await apiClient.get(`/workflows/${id}/`) // 使用 apiClient
        this.currentWorkflow = response.data
        // 确保标签字段存在并且是数组
        if (!this.currentWorkflow.tags) {
          this.currentWorkflow.tags = []
        } else if (typeof this.currentWorkflow.tags === 'string') {
          try {
            this.currentWorkflow.tags = JSON.parse(this.currentWorkflow.tags)
          } catch (e) {
            this.currentWorkflow.tags = this.currentWorkflow.tags.split(',').map(tag => tag.trim())
          }
        }
      } catch (error) {
        this.error = '获取工作流详情失败'
        console.error(error.response ? error.response.data : error.message);
      } finally {
        this.isLoading = false
      }
    },

    // 保存工作流
    async saveWorkflow(workflow) {
      this.isLoading = true
      this.error = null
      
      try {
        // 确保标签是有效的格式
        const workflowData = { ...workflow }
        if (workflowData.tags && Array.isArray(workflowData.tags)) {
          workflowData.tags = JSON.stringify(workflowData.tags)
        }
        
        let response
        if (workflow.id) {
          // 更新现有工作流
          response = await apiClient.put(`/workflows/${workflow.id}/`, workflowData) // 使用 apiClient
        } else {
          // 创建新工作流
          response = await apiClient.post('/workflows/', workflowData) // 使用 apiClient
        }
        
        const savedWorkflow = response.data
        
        // 确保标签字段是数组
        if (!savedWorkflow.tags) {
          savedWorkflow.tags = []
        } else if (typeof savedWorkflow.tags === 'string') {
          try {
            savedWorkflow.tags = JSON.parse(savedWorkflow.tags)
          } catch (e) {
            savedWorkflow.tags = savedWorkflow.tags.split(',').map(tag => tag.trim())
          }
        }
        
        this.currentWorkflow = savedWorkflow
        ElMessage.success(workflow.id ? '工作流已成功更新' : '工作流已成功创建');
        return savedWorkflow
      } catch (error) {
        this.error = '保存工作流失败'
        const errorMsg = error.response && error.response.data ? JSON.stringify(error.response.data) : error.message;
        console.error(errorMsg);
        ElMessage.error(`保存工作流失败: ${errorMsg}`);
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 删除工作流
    async deleteWorkflow(id) {
      this.isLoading = true
      this.error = null
      
      try {
        await apiClient.delete(`/workflows/${id}/`) // 使用 apiClient
        this.workflows = this.workflows.filter(w => w.id !== id)
        if (this.currentWorkflow && this.currentWorkflow.id === id) {
          this.currentWorkflow = null
        }
        ElMessage.success('工作流已成功删除');
        return true
      } catch (error) {
        this.error = '删除工作流失败'
        console.error(error.response ? error.response.data : error.message);
        ElMessage.error('删除工作流失败');
        return false
      } finally {
        this.isLoading = false
      }
    },

    // 获取工作流模板
    async fetchTemplates() {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await apiClient.get('/workflows/templates/') // 使用 apiClient
        this.templates = response.data.results || response.data
        // 确保标签字段存在并且是数组
        this.templates.forEach(template => {
          if (!template.tags) {
            template.tags = []
          } else if (typeof template.tags === 'string') {
            try {
              template.tags = JSON.parse(template.tags)
            } catch (e) {
              template.tags = template.tags.split(',').map(tag => tag.trim())
            }
          }
        })
      } catch (error) {
        this.error = '获取工作流模板失败'
        console.error(error.response ? error.response.data : error.message);
      } finally {
        this.isLoading = false
      }
    },

    // 获取工作流实例列表
    async fetchWorkflowInstances() {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await apiClient.get('/workflows/instances/') // 使用 apiClient
        this.workflowInstances = response.data.results || response.data
      } catch (error) {
        this.error = '获取工作流实例列表失败'
        console.error(error.response ? error.response.data : error.message);
      } finally {
        this.isLoading = false
      }
    },

    // 获取单个工作流实例详情
    async fetchWorkflowInstance(id) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await apiClient.get(`/workflows/instances/${id}/`) // 使用 apiClient
        this.currentInstance = response.data
      } catch (error) {
        this.error = '获取工作流实例详情失败'
        console.error(error.response ? error.response.data : error.message);
      } finally {
        this.isLoading = false
      }
    },

    // 启动工作流实例
    async startWorkflowInstance(workflowId, parameters) {
      this.isLoading = true
      this.error = null
      
      try {
        const response = await apiClient.post(`/workflows/${workflowId}/execute/`, { parameters }) // 使用 apiClient
        ElMessage.success('工作流实例已开始执行');
        return response.data
      } catch (error) {
        this.error = '启动工作流实例失败'
        const errorMsg = error.response && error.response.data ? JSON.stringify(error.response.data) : error.message;
        console.error(errorMsg);
        ElMessage.error(`启动工作流实例失败: ${errorMsg}`);
        throw error
      } finally {
        this.isLoading = false
      }
    },

    // 取消工作流实例
    async cancelWorkflowInstance(instanceId) {
      this.isLoading = true
      this.error = null
      
      try {
        await apiClient.post(`/workflows/instances/${instanceId}/cancel/`) // 使用 apiClient
        if (this.currentInstance && this.currentInstance.id === instanceId) {
          this.currentInstance.status = 'cancelled'
        }
        ElMessage.success('工作流实例已取消');
        return true
      } catch (error) {
        this.error = '取消工作流实例失败'
        console.error(error.response ? error.response.data : error.message);
        ElMessage.error('取消工作流实例失败');
        return false
      } finally {
        this.isLoading = false
      }
    },

    // 暂停工作流实例
    async pauseWorkflowInstance(instanceId) {
      this.isLoading = true
      this.error = null
      
      try {
        await apiClient.post(`/workflows/instances/${instanceId}/pause/`) // 使用 apiClient
        if (this.currentInstance && this.currentInstance.id === instanceId) {
          this.currentInstance.status = 'paused'
        }
        ElMessage.success('工作流实例已暂停');
        return true
      } catch (error) {
        this.error = '暂停工作流实例失败'
        console.error(error.response ? error.response.data : error.message);
        ElMessage.error('暂停工作流实例失败');
        return false
      } finally {
        this.isLoading = false
      }
    },
    
    // 恢复工作流实例
    async resumeWorkflowInstance(instanceId) {
      this.isLoading = true
      this.error = null
      
      try {
        await apiClient.post(`/workflows/instances/${instanceId}/resume/`) // 使用 apiClient
        if (this.currentInstance && this.currentInstance.id === instanceId) {
          this.currentInstance.status = 'running' // Or whatever status backend sets
        }
        ElMessage.success('工作流实例已恢复');
        return true
      } catch (error) {
        this.error = '恢复工作流实例失败'
        console.error(error.response ? error.response.data : error.message);
        ElMessage.error('恢复工作流实例失败');
        return false
      } finally {
        this.isLoading = false
      }
    },

    // 重试工作流步骤
    async retryWorkflowStep(instanceId, stepId) {
      this.isLoading = true
      this.error = null
      
      try {
        await apiClient.post(`/workflows/instances/${instanceId}/steps/${stepId}/retry/`) // 使用 apiClient
        ElMessage.success('工作流步骤已开始重试');
        return true
      } catch (error) {
        this.error = '重试工作流步骤失败'
        const errorMsg = error.response && error.response.data ? JSON.stringify(error.response.data) : error.message;
        console.error(errorMsg);
        ElMessage.error(`重试工作流步骤失败: ${errorMsg}`);
        return false
      } finally {
        this.isLoading = false
      }
    }
  }
}) 