<template>
  <div class="session-list-container">
    <div class="page-header">
      <h1 class="app-page-title">会话列表</h1>
      <el-button type="primary" @click="$router.push('/sessions/create')">
        <el-icon><plus /></el-icon> 创建新会话
      </el-button>
    </div>

    <el-card v-loading="isLoading" class="app-card">
      <!-- 筛选工具栏 -->
      <div class="filter-toolbar">
        <el-form :inline="true" :model="filterForm" class="filter-form">
          <el-form-item label="智能体">
            <!-- Simplified el-select for debugging -->
            <el-select v-model="filterForm.agent">
              <el-option
                v-for="agent in agents"
                :key="agent.id"
                :label="agent.name || `智能体ID: ${agent.id}`"
                :value="String(agent.id)"
              />
            </el-select>
          </el-form-item>
          
          <el-form-item label="状态">
            <el-select v-model="filterForm.status" placeholder="选择状态" clearable>
              <el-option label="所有状态" value="" />
              <el-option label="活跃" value="active" />
              <el-option label="已完成" value="completed" />
              <el-option label="空会话" value="empty" />
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

      <!-- 会话数据表格 -->
      <el-table :data="sessions" style="width: 100%" row-key="id" border>
        <el-table-column prop="name" label="会话名称" min-width="200">
          <template #default="scope">
            <el-link 
              type="primary" 
              @click="$router.push(`/sessions/${scope.row.id}`)"
            >
              {{ scope.row.name || `会话 ${formatSessionId(scope.row.id)}` }}
            </el-link>
          </template>
        </el-table-column>
        
        <el-table-column prop="agent" label="智能体" width="180">
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
        
        <el-table-column prop="task_count" label="任务数量" width="100" align="center">
          <template #default="scope">
            <el-tag type="info">{{ scope.row.task_count || 0 }}</el-tag>
          </template>
        </el-table-column>
        
        <el-table-column prop="last_activity" label="最近活动" width="180">
          <template #default="scope">
            {{ scope.row.updated_at ? formatDateTime(scope.row.updated_at) : '-' }}
          </template>
        </el-table-column>
        
        <el-table-column prop="created_at" label="创建时间" width="180">
          <template #default="scope">
            {{ formatDateTime(scope.row.created_at) }}
          </template>
        </el-table-column>
        
        <el-table-column label="操作" width="220">
          <template #default="scope">
            <el-button-group>
              <el-button 
                size="small" 
                @click="$router.push(`/sessions/${scope.row.id}`)"
              >
                <el-icon><view /></el-icon>
                查看
              </el-button>
              
              <el-button 
                size="small" 
                type="primary"
                @click="$router.push(`/sessions/${scope.row.id}/edit`)"
              >
                <el-icon><edit /></el-icon>
                编辑
              </el-button>
              
              <el-button 
                size="small" 
                type="danger" 
                @click="handleDeleteSession(scope.row)"
              >
                <el-icon><delete /></el-icon>
                删除
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
    
    <!-- 删除会话确认对话框 -->
    <el-dialog
      v-model="deleteDialogVisible"
      title="确认删除会话"
      width="30%"
    >
      <p>您确定要删除会话 \"{{ sessionToDelete?.name || `会话 ${formatSessionId(sessionToDelete?.id)}` }}\" 吗？</p>
      <p>此操作将删除会话及其相关数据，且不可恢复。</p>
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="deleteDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="confirmDeleteSession" :loading="isLoading">确认删除</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useSessionStore } from '@/store/session'
import { useAgentStore } from '@/store/agent'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Plus,
  Search,
  Refresh,
  View,
  Edit,
  Delete
} from '@element-plus/icons-vue'

export default {
  name: 'SessionList',
  components: {
    Plus,
    Search,
    Refresh,
    View,
    Edit,
    Delete
  },
  setup() {
    const router = useRouter()
    const sessionStore = useSessionStore()
    const agentStore = useAgentStore()
    
    // 筛选表单
    const filterForm = ref({
      agent: '',
      status: '',
      dateRange: []
    })
    
    // 删除会话相关
    const deleteDialogVisible = ref(false)
    const sessionToDelete = ref(null)
    
    // 计算属性
    const sessions = computed(() => sessionStore.sessions)
    const agents = computed(() => agentStore.agents)
    const isLoading = computed(() => sessionStore.isLoading)
    const error = computed(() => sessionStore.error)
    const pagination = computed(() => sessionStore.pagination)
    
    // 筛选会话 (由搜索按钮触发)
    const handleFilterChange = () => {
      // 构造后端筛选参数
      const filters = {}
      
      if (filterForm.value.agent) {
        filters.agent = filterForm.value.agent
      }
      
      if (filterForm.value.status) {
        filters.status = filterForm.value.status
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
      sessionStore.setFilters(filters)
      sessionStore.setPagination({ current: 1 })
      loadSessions()
    }
    
    // 重置筛选
    const resetFilter = () => {
      filterForm.value = {
        agent: '',
        status: '',
        dateRange: []
      };
      handleFilterChange(); // 重置后也需要重新获取数据
    }
    
    // 翻页和每页数量变化处理
    const handleSizeChange = (size) => {
      sessionStore.setPagination({ pageSize: size })
      loadSessions()
    }
    
    const handleCurrentChange = (page) => {
      sessionStore.setPagination({ current: page })
      loadSessions()
    }
    
    // 加载会话列表
    const loadSessions = async () => {
      await sessionStore.fetchSessions()
    }
    
    // 初始化加载代理列表
    const loadAgents = async () => {
      if (agents.value.length === 0) {
        await agentStore.fetchAgents()
      }
    }
    
    // 处理删除会话
    const handleDeleteSession = (session) => {
      sessionToDelete.value = session
      deleteDialogVisible.value = true
    }
    
    // 确认删除会话
    const confirmDeleteSession = async () => {
      if (!sessionToDelete.value) return
      
      try {
        await sessionStore.deleteSession(sessionToDelete.value.id)
        deleteDialogVisible.value = false
        sessionToDelete.value = null
      } catch (err) {
        console.error('删除会话失败', err)
      }
    }
    
    // 格式化会话ID
    const formatSessionId = (id) => {
      if (!id) return ''
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
    
    // 生命周期钩子
    onMounted(async () => {
      await loadAgents()
      await loadSessions()
    })
    
    return {
      sessions,
      agents,
      isLoading,
      error,
      pagination,
      filterForm,
      deleteDialogVisible,
      sessionToDelete,
      handleFilterChange,
      resetFilter,
      handleSizeChange,
      handleCurrentChange,
      handleDeleteSession,
      confirmDeleteSession,
      formatSessionId,
      formatDateTime
    }
  }
}
</script>

<style scoped>
.session-list-container {
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
</style> 