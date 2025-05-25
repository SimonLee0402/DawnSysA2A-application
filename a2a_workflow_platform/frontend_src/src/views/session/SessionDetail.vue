<template>
  <div class="session-detail-container" v-loading="isLoading">
    <div class="page-header">
      <div class="title-section">
        <h1>{{ session?.name || '会话详情' }}</h1>
        <el-tag v-if="session" size="large" type="primary">
          {{ session.task_count || 0 }} 个任务
        </el-tag>
      </div>
      <div class="action-buttons">
        <el-button-group>
          <el-button type="primary" @click="$router.push(`/sessions/${sessionId}/edit`)">
            <el-icon><edit /></el-icon> 编辑
          </el-button>
          <el-button type="success" @click="showCreateTaskDialog">
            <el-icon><plus /></el-icon> 创建任务
          </el-button>
          <el-button @click="$router.push('/sessions')">
            <el-icon><back /></el-icon> 返回列表
          </el-button>
        </el-button-group>
      </div>
    </div>

    <el-alert
      v-if="error"
      :title="error"
      type="error"
      :closable="false"
      show-icon
      class="mb-3"
    />

    <template v-if="session">
      <el-tabs>
        <el-tab-pane label="基本信息">
          <el-card class="info-card">
            <el-descriptions :column="2" border>
              <el-descriptions-item label="会话名称">{{ session.name || '无名称' }}</el-descriptions-item>
              <el-descriptions-item label="ID">{{ sessionId }}</el-descriptions-item>
              <el-descriptions-item label="智能体">
                <el-link 
                  v-if="session.agent"
                  type="primary"
                  @click="$router.push(`/agents/${session.agent.id}`)"
                >
                  {{ session.agent.name }}
                </el-link>
                <span v-else>未指定</span>
              </el-descriptions-item>
              <el-descriptions-item label="任务数量">{{ session.task_count || 0 }}</el-descriptions-item>
              <el-descriptions-item label="创建时间">{{ formatDateTime(session.created_at) }}</el-descriptions-item>
              <el-descriptions-item label="最近活动">{{ formatDateTime(session.updated_at) }}</el-descriptions-item>
              <el-descriptions-item label="元数据" :span="2">
                <pre v-if="session.metadata && Object.keys(session.metadata).length" class="metadata-display">{{ formatJSON(session.metadata) }}</pre>
                <span v-else>无元数据</span>
              </el-descriptions-item>
            </el-descriptions>
          </el-card>
        </el-tab-pane>

        <el-tab-pane label="任务列表">
          <el-card class="task-card">
            <div class="task-header">
              <h3>会话任务</h3>
              <el-button type="primary" size="small" @click="showCreateTaskDialog">
                <el-icon><plus /></el-icon> 创建新任务
              </el-button>
            </div>
            
            <el-empty v-if="sessionTasks.length === 0" description="该会话还没有任务" />
            
            <el-table v-else :data="sessionTasks" style="width: 100%" row-key="id" border>
              <el-table-column prop="id" label="任务ID" width="280">
                <template #default="scope">
                  <el-link type="primary" @click="$router.push(`/tasks/${scope.row.id}`)">
                    {{ scope.row.id }}
                  </el-link>
                </template>
              </el-table-column>
              
              <el-table-column prop="state" label="状态" width="120">
                <template #default="scope">
                  <el-tag :type="getStateTagType(scope.row.state)">
                    {{ getStateDisplay(scope.row.state) }}
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
              
              <el-table-column label="操作" width="160">
                <template #default="scope">
                  <el-button-group>
                    <el-button size="small" @click="$router.push(`/tasks/${scope.row.id}`)">
                      <el-icon><view /></el-icon> 查看
                    </el-button>
                    <el-button 
                      size="small" 
                      type="danger" 
                      @click="handleCancelTask(scope.row)"
                      v-if="isTaskActive(scope.row.state)"
                    >
                      <el-icon><close /></el-icon> 取消
                    </el-button>
                  </el-button-group>
                </template>
              </el-table-column>
            </el-table>
          </el-card>
        </el-tab-pane>

        <el-tab-pane label="会话历史">
          <el-card class="history-card">
            <el-empty v-if="sessionHistory.length === 0" description="没有会话历史记录" />
            
            <div v-else class="message-container">
              <div
                v-for="(message, index) in sessionHistory"
                :key="index"
                :class="['message', message.role === 'user' ? 'user-message' : 'agent-message']"
              >
                <div class="message-header">
                  <strong>{{ message.role === 'user' ? '用户' : '智能体' }}</strong>
                  <span class="message-time">{{ formatDateTime(message.created_at) }}</span>
                </div>
                <div class="message-content" v-html="formatMessageContent(message.content)"></div>
              </div>
            </div>
          </el-card>
        </el-tab-pane>
      </el-tabs>
    </template>
    
    <!-- 创建任务对话框 -->
    <el-dialog
      v-model="createTaskDialogVisible"
      title="创建新任务"
      width="50%"
    >
      <el-form :model="newTaskForm" :rules="taskRules" ref="taskFormRef" label-position="top">
        <el-form-item label="任务内容" prop="content">
          <el-input 
            v-model="newTaskForm.content" 
            type="textarea" 
            :rows="5" 
            placeholder="请输入任务内容"
          />
        </el-form-item>
        
        <el-form-item label="元数据 (可选)">
          <el-input 
            v-model="newTaskForm.metadata" 
            type="textarea" 
            :rows="3" 
            placeholder="请输入元数据 (JSON格式)"
          />
        </el-form-item>
      </el-form>
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="createTaskDialogVisible = false">取消</el-button>
          <el-button type="primary" @click="createTask" :loading="isLoading">创建任务</el-button>
        </span>
      </template>
    </el-dialog>
    
    <!-- 取消任务确认对话框 -->
    <el-dialog
      v-model="cancelTaskDialogVisible"
      title="确认取消任务"
      width="30%"
    >
      <p>您确定要取消任务吗？</p>
      <p>请输入取消原因（可选）：</p>
      <el-input v-model="cancelReason" type="textarea" :rows="3" placeholder="请输入取消原因（可选）" />
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="cancelTaskDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="confirmCancelTask" :loading="isLoading">确认取消</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSessionStore } from '@/store/session'
import { useTaskStore } from '@/store/task'
import { ElMessage } from 'element-plus'
import {
  Edit,
  Plus,
  Back,
  View,
  Close
} from '@element-plus/icons-vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'

export default {
  name: 'SessionDetail',
  components: {
    Edit,
    Plus,
    Back,
    View,
    Close
  },
  setup() {
    const route = useRoute()
    const router = useRouter()
    const sessionStore = useSessionStore()
    const taskStore = useTaskStore()
    const taskFormRef = ref(null)
    
    const sessionId = ref(route.params.id)
    
    // 创建任务相关
    const createTaskDialogVisible = ref(false)
    const newTaskForm = ref({
      content: '',
      metadata: '{}'
    })
    const taskRules = {
      content: [
        { required: true, message: '请输入任务内容', trigger: 'blur' }
      ]
    }
    
    // 取消任务相关
    const cancelTaskDialogVisible = ref(false)
    const taskToCancel = ref(null)
    const cancelReason = ref('')
    
    // 计算属性
    const session = computed(() => sessionStore.currentSession)
    const sessionTasks = computed(() => sessionStore.sessionTasks)
    const sessionHistory = computed(() => sessionStore.sessionHistory)
    const isLoading = computed(() => sessionStore.isLoading || taskStore.isLoading)
    const error = computed(() => sessionStore.error || taskStore.error)
    
    // 获取会话详情
    const fetchSessionDetail = async () => {
      try {
        await sessionStore.fetchSession(sessionId.value)
        await sessionStore.fetchSessionTasks(sessionId.value)
        await sessionStore.fetchSessionHistory(sessionId.value)
      } catch (err) {
        console.error('获取会话详情失败', err)
      }
    }
    
    // 显示创建任务对话框
    const showCreateTaskDialog = () => {
      newTaskForm.value = {
        content: '',
        metadata: '{}'
      }
      createTaskDialogVisible.value = true
    }
    
    // 创建任务
    const createTask = async () => {
      if (!taskFormRef.value) return
      
      await taskFormRef.value.validate(async (valid) => {
        if (!valid) return
        
        try {
          // 解析元数据
          let metadata = {}
          try {
            metadata = JSON.parse(newTaskForm.value.metadata)
          } catch (e) {
            ElMessage.warning('元数据格式不正确，使用空对象')
          }
          
          // 创建任务数据
          const taskData = {
            content: newTaskForm.value.content,
            metadata
          }
          
          // 添加任务到会话
          await sessionStore.addSessionTask(sessionId.value, taskData)
          
          // 关闭对话框
          createTaskDialogVisible.value = false
          ElMessage.success('任务创建成功')
        } catch (err) {
          console.error('创建任务失败', err)
        }
      })
    }
    
    // 处理取消任务
    const handleCancelTask = (task) => {
      taskToCancel.value = task
      cancelReason.value = ''
      cancelTaskDialogVisible.value = true
    }
    
    // 确认取消任务
    const confirmCancelTask = async () => {
      if (!taskToCancel.value) return
      
      try {
        await taskStore.cancelTask(taskToCancel.value.id, cancelReason.value)
        cancelTaskDialogVisible.value = false
        taskToCancel.value = null
        
        // 刷新任务列表
        await sessionStore.fetchSessionTasks(sessionId.value)
      } catch (err) {
        console.error('取消任务失败', err)
      }
    }
    
    // 判断任务是否处于活跃状态
    const isTaskActive = (state) => {
      return ['submitted', 'working', 'input-required'].includes(state)
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
    
    // 格式化JSON
    const formatJSON = (jsonObj) => {
      return JSON.stringify(jsonObj, null, 2)
    }
    
    // 格式化消息内容（支持Markdown）
    const formatMessageContent = (content) => {
      if (!content) return ''
      try {
        const html = marked(content)
        return DOMPurify.sanitize(html)
      } catch (e) {
        return content
      }
    }
    
    // 获取任务状态显示文本
    const getStateDisplay = (state) => {
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
    
    // 获取任务状态标签类型
    const getStateTagType = (state) => {
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
    onMounted(() => {
      fetchSessionDetail()
    })
    
    return {
      sessionId,
      session,
      sessionTasks,
      sessionHistory,
      isLoading,
      error,
      createTaskDialogVisible,
      newTaskForm,
      taskRules,
      taskFormRef,
      cancelTaskDialogVisible,
      taskToCancel,
      cancelReason,
      showCreateTaskDialog,
      createTask,
      handleCancelTask,
      confirmCancelTask,
      isTaskActive,
      formatDateTime,
      formatJSON,
      formatMessageContent,
      getStateDisplay,
      getStateTagType
    }
  }
}
</script>

<style scoped>
.session-detail-container {
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

.title-section {
  display: flex;
  align-items: center;
  gap: 10px;
}

.title-section h1 {
  margin: 0;
}

.mb-3 {
  margin-bottom: 15px;
}

.info-card, .task-card, .history-card {
  margin-bottom: 20px;
}

.task-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 15px;
}

.task-header h3 {
  margin: 0;
}

.metadata-display {
  background-color: #f7f7f7;
  padding: 10px;
  border-radius: 4px;
  white-space: pre-wrap;
  font-family: monospace;
  overflow-x: auto;
}

.message-container {
  display: flex;
  flex-direction: column;
  gap: 15px;
}

.message {
  padding: 10px;
  border-radius: 8px;
  max-width: 85%;
}

.user-message {
  background-color: #e6f7ff;
  margin-left: auto;
  border: 1px solid #91d5ff;
}

.agent-message {
  background-color: #ffffff;
  margin-right: auto;
  border: 1px solid #e8e8e8;
}

.message-header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 5px;
  font-size: 12px;
  color: #606266;
}

.message-content {
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.5;
}

.message-content :deep(pre) {
  background-color: #f0f0f0;
  padding: 10px;
  border-radius: 4px;
  overflow-x: auto;
}

.message-content :deep(code) {
  background-color: #f0f0f0;
  padding: 2px 4px;
  border-radius: 4px;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style> 