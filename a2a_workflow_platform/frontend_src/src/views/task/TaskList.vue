<template>
  <div class="task-list-container">
    <div class="page-header">
      <h1 class="app-page-title">任务列表</h1>
    </div>

    <el-card v-loading="isLoading" class="app-card">
      <!-- 筛选工具栏 -->
      <div class="filter-toolbar">
        <el-form :inline="true" :model="filterForm" class="filter-form">
          <el-form-item label="智能体">
            <el-select v-model="filterForm.agent" placeholder="选择智能体" clearable>
              <el-option label="所有智能体" value="" />
              <el-option 
                v-for="agent in agents" 
                :key="agent.id" 
                :label="agent.name" 
                :value="agent.id" 
              />
            </el-select>
          </el-form-item>
          
          <el-form-item label="状态">
            <el-select v-model="filterForm.state" placeholder="选择状态" clearable>
              <el-option label="所有状态" value="" />
              <el-option label="已提交" value="submitted" />
              <el-option label="处理中" value="working" />
              <el-option label="需要输入" value="input-required" />
              <el-option label="已完成" value="completed" />
              <el-option label="失败" value="failed" />
              <el-option label="已取消" value="canceled" />
            </el-select>
          </el-form-item>
          
          <el-form-item label="创建时间">
            <el-date-picker
              v-model="filterForm.dateRange"
              type="daterange"
              range-separator="至"
              start-placeholder="开始日期"
              end-placeholder="结束日期"
              format="YYYY-MM-DD"
              clearable
            />
          </el-form-item>
          
          <el-form-item>
            <el-button type="primary" @click="handleFilterChange">
              <el-icon><search /></el-icon> 搜索
            </el-button>
            <el-button @click="resetFilter">
              <el-icon><refresh /></el-icon> 重置
            </el-button>
          </el-form-item>
        </el-form>
      </div>

      <el-alert
        v-if="error"
        :title="error"
        type="error"
        :closable="false"
        show-icon
        class="mb-3"
      />

      <!-- 任务数据表格 -->
      <el-table 
        :data="tasks" 
        style="width: 100%" 
        row-key="id" 
        border 
        @row-click="(row) => $router.push(`/tasks/${row.id}`)"
        class="hover-pointer"
      >
        <el-table-column prop="id" label="任务ID" width="300">
          <template #default="scope">
            <el-link 
              type="primary" 
              @click.stop="$router.push(`/tasks/${scope.row.id}`)"
            >
              {{ formatTaskId(scope.row.id) }}
            </el-link>
          </template>
        </el-table-column>
        
        <el-table-column prop="agent" label="智能体" width="200">
          <template #default="scope">
            <el-link
              v-if="scope.row.agent"
              type="info"
              @click="$router.push(`/agents/${scope.row.agent.id}`)"
            >
              {{ scope.row.agent.name }}
            </el-link>
            <span v-else>-</span>
          </template>
        </el-table-column>
        
        <el-table-column prop="state" label="状态" width="120">
          <template #default="scope">
            <el-tag :type="getStatusTagType(scope.row.state)">
              {{ getStateDisplayName(scope.row.state) }}
            </el-tag>
          </template>
        </el-table-column>
        
        <el-table-column prop="created_at" label="创建时间" width="180">
          <template #default="scope">
            {{ formatDateTime(scope.row.created_at) }}
          </template>
        </el-table-column>
        
        <el-table-column prop="completed_at" label="完成时间" width="180">
          <template #default="scope">
            {{ scope.row.completed_at ? formatDateTime(scope.row.completed_at) : '-' }}
          </template>
        </el-table-column>
        
        <el-table-column label="操作" width="200">
          <template #default="scope">
            <el-button-group>
              <el-button 
                size="small" 
                @click.stop="$router.push(`/tasks/${scope.row.id}`)"
              >
                <el-icon><view /></el-icon>
                查看
              </el-button>
              
              <el-button 
                size="small" 
                type="danger" 
                @click.stop="handleCancelTask(scope.row)"
                :loading="isCancelling[scope.row.id]"
                v-if="isTaskActive(scope.row.state)"
              >
                <el-icon><close /></el-icon>
                取消
              </el-button>
            </el-button-group>
          </template>
        </el-table-column>
      </el-table>
      
      <!-- 分页 -->
      <div class="pagination-container">
        <el-pagination
          v-model:current-page="pagination.current"
          v-model:page-size="pagination.pageSize"
          :page-sizes="[10, 20, 50, 100]"
          layout="total, sizes, prev, pager, next, jumper"
          :total="pagination.total"
          @size-change="handleSizeChange"
          @current-change="handleCurrentChange"
        />
      </div>
    </el-card>
    
    <!-- 取消任务确认对话框 -->
    <el-dialog
      v-model="cancelDialogVisible"
      title="确认取消任务"
      width="30%"
    >
      <p>您确定要取消任务 \"{{ formatTaskId(taskToCancel?.id) }}\" 吗？</p>
      <p>请输入取消原因（可选）：</p>
      <el-input v-model="cancelReason" type="textarea" :rows="3" placeholder="请输入取消原因（可选）" />
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="cancelDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="confirmCancelTask" :loading="isCancelling[taskToCancel?.id]">确认取消</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch, reactive } from 'vue'
import { useRouter } from 'vue-router'
import { useTaskStore } from '@/store/task'
import { useAgentStore } from '@/store/agent'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Search,
  Refresh,
  View,
  Close
} from '@element-plus/icons-vue'

export default {
  name: 'TaskList',
  components: {
    Search,
    Refresh,
    View,
    Close
  },
  setup() {
    const router = useRouter()
    const taskStore = useTaskStore()
    const agentStore = useAgentStore()
    
    // 筛选表单
    const filterForm = ref({
      agent: '',
      state: '',
      dateRange: []
    })
    
    // 取消任务相关
    const cancelDialogVisible = ref(false)
    const taskToCancel = ref(null)
    const cancelReason = ref('')
    const isCancelling = reactive({})
    
    // 计算属性
    const tasks = computed(() => taskStore.tasks)
    const agents = computed(() => agentStore.agents)
    const isLoading = computed(() => taskStore.isLoading)
    const error = computed(() => taskStore.error)
    const pagination = computed(() => taskStore.pagination)
    
    // 监听筛选条件变化
    watch(() => filterForm.value, (newVal, oldVal) => {
      // 当filterForm变化但不是通过resetFilter重置时，自动应用筛选
      if (JSON.stringify(newVal) !== JSON.stringify(oldVal) && 
          JSON.stringify(newVal) !== JSON.stringify({agent: '', state: '', dateRange: []})) {
        handleFilterChange()
      }
    }, { deep: true })
    
    // 筛选任务
    const handleFilterChange = () => {
      // 构造后端筛选参数
      const filters = {}
      
      if (filterForm.value.agent) {
        filters.agent = filterForm.value.agent
      }
      
      if (filterForm.value.state) {
        filters.state = filterForm.value.state
      }
      
      // 处理日期范围
      if (filterForm.value.dateRange && filterForm.value.dateRange.length === 2) {
        const [startDate, endDate] = filterForm.value.dateRange
        if (startDate && endDate) {
          filters.created_after = formatDateToISO(startDate)
          filters.created_before = formatDateToISO(endDate, true) // 添加23:59:59
        }
      }
      
      // 设置筛选条件并重置分页到第一页
      taskStore.setFilters(filters)
      taskStore.setPagination({ current: 1 })
      loadTasks()
    }
    
    // 重置筛选条件
    const resetFilter = () => {
      filterForm.value = {
        agent: '',
        state: '',
        dateRange: []
      }
      taskStore.setFilters({})
      loadTasks()
    }
    
    // 翻页和每页数量变化处理
    const handleSizeChange = (size) => {
      taskStore.setPagination({ pageSize: size })
      loadTasks()
    }
    
    const handleCurrentChange = (page) => {
      taskStore.setPagination({ current: page })
      loadTasks()
    }
    
    // 加载任务列表
    const loadTasks = async () => {
      await taskStore.fetchTasks()
    }
    
    // 初始化加载代理列表
    const loadAgents = async () => {
      if (agents.value.length === 0) {
        await agentStore.fetchAgents()
      }
    }
    
    // 处理取消任务
    const handleCancelTask = (task) => {
      taskToCancel.value = task
      cancelReason.value = ''
      cancelDialogVisible.value = true
    }
    
    // 确认取消任务
    const confirmCancelTask = async () => {
      if (!taskToCancel.value) return
      
      try {
        const taskId = taskToCancel.value.id
        isCancelling[taskId] = true
        await taskStore.cancelTask(taskId, cancelReason.value || '用户取消')
        cancelDialogVisible.value = false
        taskToCancel.value = null
      } catch (err) {
        console.error('取消任务失败', err)
      } finally {
        if (taskToCancel.value) {
          isCancelling[taskToCancel.value.id] = false
        }
      }
    }
    
    // 判断任务是否处于可操作状态
    const isTaskActive = (state) => {
      return ['submitted', 'working', 'input-required'].includes(state)
    }
    
    // 格式化任务ID（太长时截断）
    const formatTaskId = (id) => {
      if (!id) return ''
      // 只显示前8个字符
      if (id.length > 8) {
        return id.substring(0, 8) + '...'
      }
      return id
    }
    
    // 格式化日期时间
    const formatDateTime = (dateTime) => {
      if (!dateTime) return '-'
      const date = new Date(dateTime)
      return date.toLocaleString('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      })
    }
    
    // 格式化日期为ISO字符串（用于API请求）
    const formatDateToISO = (date, isEndOfDay = false) => {
      if (!date) return null
      
      const d = new Date(date)
      if (isEndOfDay) {
        d.setHours(23, 59, 59, 999)
      }
      return d.toISOString()
    }
    
    // 获取状态显示名称
    const getStateDisplayName = (state) => {
      const stateMap = {
        'submitted': '已提交',
        'working': '处理中',
        'input-required': '需要输入',
        'completed': '已完成',
        'failed': '失败',
        'canceled': '已取消'
      }
      return stateMap[state] || state
    }
    
    // 获取状态标签类型
    const getStatusTagType = (state) => {
      const typeMap = {
        'submitted': 'info',
        'working': 'primary',
        'input-required': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'canceled': 'info'
      }
      return typeMap[state] || ''
    }
    
    // 生命周期钩子
    onMounted(async () => {
      await loadAgents()
      await loadTasks()
    })
    
    return {
      tasks,
      agents,
      isLoading,
      error,
      pagination,
      filterForm,
      cancelDialogVisible,
      taskToCancel,
      cancelReason,
      isCancelling,
      handleFilterChange,
      resetFilter,
      handleSizeChange,
      handleCurrentChange,
      handleCancelTask,
      confirmCancelTask,
      isTaskActive,
      formatTaskId,
      formatDateTime,
      getStateDisplayName,
      getStatusTagType
    }
  }
}
</script>

<style scoped>
.task-list-container {
  max-width: 1200px;
  margin: 0 auto;
  padding: 20px;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

/* 页面标题样式 */
.app-page-title {
  font-size: 24px;
  font-weight: 600;
  color: #303133;
  margin-bottom: 0; /* page-header has margin-bottom */
}

/* 卡片样式 */
.app-card {
  border-radius: 8px;
  border: 1px solid #ebeef5;
  box-shadow: 0 2px 12px 0 rgba(0,0,0,.05);
}

.filter-toolbar {
  margin-bottom: 20px;
}

.filter-form {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.mb-3 {
  margin-bottom: 15px;
}

.pagination-container {
  margin-top: 20px;
  display: flex;
  justify-content: flex-end;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

/* 表格样式 */
.hover-pointer tbody tr {
  cursor: pointer;
}
</style> 