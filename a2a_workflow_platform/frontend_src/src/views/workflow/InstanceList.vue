<template>
  <div class="instance-list">
    <div class="page-header">
      <h1>工作流实例</h1>
      <el-button @click="refreshInstances" type="primary">
        <el-icon><refresh /></el-icon> 刷新
      </el-button>
    </div>

    <el-card class="filter-card">
      <el-form :model="filterForm" class="filter-form" label-position="top">
        <el-row :gutter="20">
          <el-col :span="24" :md="8" :lg="6">
            <el-form-item label="状态">
              <el-select v-model="filterForm.status" placeholder="实例状态" clearable class="full-width">
                <el-option label="已创建" value="created" />
                <el-option label="运行中" value="running" />
                <el-option label="已暂停" value="paused" />
                <el-option label="已完成" value="completed" />
                <el-option label="失败" value="failed" />
                <el-option label="已取消" value="canceled" />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :span="24" :md="8" :lg="6">
            <el-form-item label="工作流">
              <el-select 
                v-model="filterForm.workflow_id" 
                placeholder="选择工作流" 
                clearable
                filterable
                class="full-width"
              >
                <el-option 
                  v-for="workflow in workflows" 
                  :key="workflow.id" 
                  :label="workflow.name" 
                  :value="workflow.id" 
                />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :span="24" :md="8" :lg="8">
            <el-form-item label="时间范围">
              <el-date-picker
                v-model="filterForm.date_range"
                type="daterange"
                format="YYYY/MM/DD"
                value-format="YYYY-MM-DD"
                range-separator="至"
                start-placeholder="开始日期"
                end-placeholder="结束日期"
                class="full-width"
              />
            </el-form-item>
          </el-col>
          <el-col :span="24" :md="24" :lg="4" class="filter-buttons">
            <el-form-item>
              <el-button type="primary" @click="handleFilter">筛选</el-button>
              <el-button @click="resetFilter">重置</el-button>
            </el-form-item>
          </el-col>
        </el-row>
      </el-form>
    </el-card>

    <el-card v-loading="isLoading" class="table-card">
      <el-empty v-if="!isLoading && instances.length === 0" description="暂无工作流实例" />
      
      <el-table v-else :data="instances" style="width: 100%" border>
        <el-table-column prop="name" label="实例名称" min-width="180">
          <template #default="{ row }">
            <router-link :to="`/workflow/instances/${row.instance_id}`" class="instance-link">
              {{ row.name || `${row.workflow.name} #${row.instance_id.substring(0, 8)}` }}
            </router-link>
          </template>
        </el-table-column>
        
        <el-table-column prop="workflow.name" label="工作流" min-width="150">
          <template #default="{ row }">
            {{ row.workflow?.name || '未知工作流' }}
          </template>
        </el-table-column>
        
        <el-table-column prop="status" label="状态" width="120">
          <template #default="{ row }">
            <el-tag :type="getStatusType(row.status)">
              {{ getStatusText(row.status) }}
            </el-tag>
          </template>
        </el-table-column>
        
        <el-table-column prop="current_step_index" label="进度" width="100">
          <template #default="{ row }">
            <el-progress 
              :percentage="calculateProgress(row)" 
              :status="getProgressStatus(row.status)"
            />
          </template>
        </el-table-column>
        
        <el-table-column prop="created_by.username" label="创建者" width="120">
          <template #default="{ row }">
            {{ row.created_by?.username || '系统' }}
          </template>
        </el-table-column>
        
        <el-table-column prop="created_at" label="创建时间" width="180">
          <template #default="{ row }">
            {{ formatDate(row.created_at) }}
          </template>
        </el-table-column>
        
        <el-table-column label="操作" width="200" fixed="right">
          <template #default="{ row }">
            <el-button-group>
              <!-- <el-button 
                size="small" 
                type="primary" 
                @click="viewInstance(row)"
                title="查看详情"
              >
                <el-icon><view /></el-icon>
              </el-button> -->
              
              <el-button 
                v-if="row.status === 'running'"
                size="small" 
                type="warning" 
                @click="pauseInstance(row)"
                title="暂停"
                :loading="isPausing[row.instance_id]"
              >
                <el-icon><video-pause /></el-icon>
              </el-button>
              
              <el-button 
                v-if="row.status === 'paused'"
                size="small" 
                type="success" 
                @click="resumeInstance(row)"
                title="继续"
                :loading="isResuming[row.instance_id]"
              >
                <el-icon><video-play /></el-icon>
              </el-button>
              
              <el-button 
                v-if="['running', 'paused'].includes(row.status)"
                size="small" 
                type="danger" 
                @click="confirmCancel(row)"
                title="取消"
                :loading="isCancelling[row.instance_id]"
              >
                <el-icon><circle-close /></el-icon>
              </el-button>
              
              <el-button 
                size="small" 
                type="info" 
                @click="confirmDelete(row)"
                title="删除"
                v-if="['completed', 'failed', 'canceled'].includes(row.status)"
                :loading="isDeleting[row.instance_id]"
              >
                <el-icon><delete /></el-icon>
              </el-button>
            </el-button-group>
          </template>
        </el-table-column>
      </el-table>
      
      <div class="pagination-container">
        <el-pagination
          v-model:current-page="pagination.page"
          v-model:page-size="pagination.pageSize"
          :page-sizes="[10, 20, 50, 100]"
          background
          layout="total, sizes, prev, pager, next, jumper"
          :total="pagination.total"
          @size-change="handleSizeChange"
          @current-change="handleCurrentChange"
        />
      </div>
    </el-card>
    
    <!-- 取消确认对话框 -->
    <el-dialog
      v-model="cancelDialogVisible"
      title="确认取消"
      width="30%"
    >
      <span>确定要取消此工作流实例吗？此操作不可撤销。</span>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="cancelDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="cancelInstance" :loading="isCancelling[currentInstance?.instance_id]">确认</el-button>
        </span>
      </template>
    </el-dialog>
    
    <!-- 删除确认对话框 -->
    <el-dialog
      v-model="deleteDialogVisible"
      title="确认删除"
      width="30%"
    >
      <span>确定要删除此工作流实例吗？此操作不可撤销，所有相关数据将被永久删除。</span>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="deleteDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="deleteInstance" :loading="isDeleting[currentInstance?.instance_id]">确认删除</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, reactive, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useWorkflowStore } from '@/store/workflow'
import { ElMessage, ElMessageBox } from 'element-plus'
import { 
  Refresh, 
  View, 
  VideoPause, 
  VideoPlay, 
  CircleClose, 
  Delete
} from '@element-plus/icons-vue'
import workflowApi from '@/api/workflow'

export default {
  name: 'InstanceList',
  components: {
    Refresh,
    View,
    VideoPause,
    VideoPlay,
    CircleClose,
    Delete
  },
  setup() {
    const router = useRouter()
    const workflowStore = useWorkflowStore()
    
    const instances = ref([])
    const workflows = ref([])
    const isLoading = ref(false)
    const isPausing = reactive({})
    const isResuming = reactive({})
    const isCancelling = reactive({})
    const isDeleting = reactive({})
    
    // 筛选表单
    const filterForm = reactive({
      status: '',
      workflow_id: '',
      date_range: []
    })
    
    // 分页
    const pagination = reactive({
      page: 1,
      pageSize: 10,
      total: 0
    })
    
    // 删除和取消对话框
    const cancelDialogVisible = ref(false)
    const deleteDialogVisible = ref(false)
    const currentInstance = ref(null)
    
    // 获取工作流实例列表
    const fetchInstances = async () => {
      isLoading.value = true
      
      try {
        const params = {
          page: pagination.page,
          page_size: pagination.pageSize
        }
        
        // 添加筛选条件
        if (filterForm.status) {
          params.status = filterForm.status
        }
        
        if (filterForm.workflow_id) {
          params.workflow_id = filterForm.workflow_id
        }
        
        if (filterForm.date_range && filterForm.date_range.length === 2) {
          params.start_date = filterForm.date_range[0]
          params.end_date = filterForm.date_range[1]
        }
        
        const response = await workflowApi.getWorkflowInstances(params)
        
        instances.value = response.results || []
        pagination.total = response.count || instances.value.length
      } catch (error) {
        console.error('获取工作流实例列表失败', error)
        ElMessage.error('获取工作流实例列表失败')
      } finally {
        isLoading.value = false
      }
    }
    
    // 获取工作流列表（用于筛选）
    const fetchWorkflows = async () => {
      try {
        const response = await workflowApi.getWorkflows()
        workflows.value = response.results || response
      } catch (error) {
        console.error('获取工作流列表失败', error)
        ElMessage.error('获取工作流列表（用于筛选）失败，请稍后重试或联系管理员。')
      }
    }
    
    // 查看实例详情
    const viewInstance = (instance) => {
      router.push(`/workflow/instances/${instance.instance_id}`)
    }
    
    // 暂停实例
    const pauseInstance = async (instance) => {
      isPausing[instance.instance_id] = true
      try {
        const result = await workflowStore.pauseWorkflowInstance(instance.instance_id)
        if (result) {
          ElMessage.success('工作流实例已暂停')
          fetchInstances()
        }
      } catch (error) {
        ElMessage.error(error.message || '暂停工作流实例失败')
      } finally {
        isPausing[instance.instance_id] = false
      }
    }
    
    // 恢复实例
    const resumeInstance = async (instance) => {
      isResuming[instance.instance_id] = true
      try {
        const result = await workflowStore.resumeWorkflowInstance(instance.instance_id)
        if (result) {
          ElMessage.success('工作流实例已恢复')
          fetchInstances()
        }
      } catch (error) {
        ElMessage.error(error.message || '恢复工作流实例失败')
      } finally {
        isResuming[instance.instance_id] = false
      }
    }
    
    // 确认取消
    const confirmCancel = (instance) => {
      currentInstance.value = instance
      cancelDialogVisible.value = true
    }
    
    // 取消实例
    const cancelInstance = async () => {
      if (!currentInstance.value) return
      isCancelling[currentInstance.value.instance_id] = true
      try {
        const result = await workflowStore.cancelWorkflowInstance(currentInstance.value.instance_id)
        if (result) {
          ElMessage.success('工作流实例已取消')
          cancelDialogVisible.value = false
          fetchInstances()
        }
      } catch (error) {
        ElMessage.error(error.message || '取消工作流实例失败')
      } finally {
        isCancelling[currentInstance.value.instance_id] = false
      }
    }
    
    // 确认删除
    const confirmDelete = (instance) => {
      currentInstance.value = instance
      deleteDialogVisible.value = true
    }
    
    // 删除实例
    const deleteInstance = async () => {
      if (!currentInstance.value) return
      isDeleting[currentInstance.value.instance_id] = true
      try {
        // 假设API已经提供了删除实例的功能
        await workflowApi.deleteWorkflowInstance(currentInstance.value.instance_id)
        ElMessage.success('工作流实例已删除')
        deleteDialogVisible.value = false
        fetchInstances()
      } catch (error) {
        ElMessage.error(error.message || '删除工作流实例失败')
      } finally {
        isDeleting[currentInstance.value.instance_id] = false
      }
    }
    
    // 刷新实例列表
    const refreshInstances = () => {
      fetchInstances()
    }
    
    // 处理筛选
    const handleFilter = () => {
      pagination.page = 1
      fetchInstances()
    }
    
    // 重置筛选
    const resetFilter = () => {
      filterForm.status = ''
      filterForm.workflow_id = ''
      filterForm.date_range = []
      pagination.page = 1
      fetchInstances()
    }
    
    // 处理页面大小变化
    const handleSizeChange = (newSize) => {
      pagination.pageSize = newSize
      fetchInstances()
    }
    
    // 处理页面变化
    const handleCurrentChange = (newPage) => {
      pagination.page = newPage
      fetchInstances()
    }
    
    // 格式化日期
    const formatDate = (dateString) => {
      if (!dateString) return '未设置'
      const date = new Date(dateString)
      return new Intl.DateTimeFormat('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit'
      }).format(date)
    }
    
    // 获取状态类型
    const getStatusType = (status) => {
      const statusMap = {
        'created': 'info',
        'running': 'primary',
        'paused': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'canceled': 'info'
      }
      return statusMap[status] || 'info'
    }
    
    // 获取状态文本
    const getStatusText = (status) => {
      const statusMap = {
        'created': '已创建',
        'running': '运行中',
        'paused': '已暂停',
        'completed': '已完成',
        'failed': '失败',
        'canceled': '已取消'
      }
      return statusMap[status] || '未知状态'
    }
    
    // 计算进度百分比
    const calculateProgress = (instance) => {
      if (!instance.steps || instance.steps.length === 0) {
        return 0
      }
      
      const totalSteps = instance.steps.length
      const completedSteps = instance.steps.filter(s => 
        ['completed', 'skipped'].includes(s.status)
      ).length
      
      if (instance.status === 'completed') {
        return 100
      }
      
      if (instance.status === 'failed') {
        // 对于失败的实例，显示已完成步骤的百分比
        return Math.round((completedSteps / totalSteps) * 100)
      }
      
      // 对于进行中的实例，当前步骤计算为半个步骤的进度
      const currentStepValue = instance.status === 'running' ? 0.5 : 0
      return Math.round(((completedSteps + currentStepValue) / totalSteps) * 100)
    }
    
    // 获取进度条状态
    const getProgressStatus = (status) => {
      if (status === 'completed') return 'success'
      if (status === 'failed') return 'exception'
      if (status === 'paused') return 'warning'
      return ''
    }
    
    onMounted(() => {
      fetchInstances()
      fetchWorkflows()
    })
    
    return {
      instances,
      workflows,
      isLoading,
      filterForm,
      pagination,
      cancelDialogVisible,
      deleteDialogVisible,
      currentInstance,
      isPausing,
      isResuming,
      isCancelling,
      isDeleting,
      viewInstance,
      pauseInstance,
      resumeInstance,
      confirmCancel,
      cancelInstance,
      confirmDelete,
      deleteInstance,
      refreshInstances,
      handleFilter,
      resetFilter,
      handleSizeChange,
      handleCurrentChange,
      formatDate,
      getStatusType,
      getStatusText,
      calculateProgress,
      getProgressStatus
    }
  }
}
</script>

<style scoped>
.instance-list {
  padding: 20px;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.filter-card {
  margin-bottom: 20px;
}

.filter-form {
  display: flex;
  flex-wrap: wrap;
}

.table-card {
  margin-bottom: 20px;
}

.pagination-container {
  margin-top: 20px;
  display: flex;
  justify-content: flex-end;
}

.instance-link {
  color: #409EFF;
  text-decoration: none;
  font-weight: bold;
}

.instance-link:hover {
  text-decoration: underline;
}

/* 响应式调整 */
@media (max-width: 768px) {
  .filter-form {
    flex-direction: column;
  }
  
  .filter-form .el-form-item {
    margin-right: 0;
    width: 100%;
  }
}
</style> 