<template>
  <div class="task-detail">
    <el-page-header @back="goBack" :content="`任务详情: ${taskId ? formatTaskId(taskId) : '加载中...'}`" />
    
    <el-card v-if="loading" class="loading-card">
      <el-skeleton :rows="6" animated />
    </el-card>
    
    <el-alert
      v-if="error"
      :title="error"
      type="error"
      description="获取任务信息失败"
      show-icon
      :closable="false"
      class="mt-3"
    />
    
    <div v-if="task" class="task-content">
      <!-- 任务基本信息 -->
      <el-card class="mt-3">
        <template #header>
          <div class="d-flex justify-content-between align-items-center">
            <span>基本信息</span>
            <div>
              <el-button size="small" type="primary" icon="el-icon-refresh" @click="loadTask" :loading="refreshLoading">刷新</el-button>
              <el-tag :type="getStatusTagType(task.status.state)">{{ getStateDisplayName(task.status.state) }}</el-tag>
            </div>
          </div>
        </template>
        <div class="task-info">
          <el-descriptions :column="2" border>
            <el-descriptions-item label="任务ID">{{ task.id }}</el-descriptions-item>
            <el-descriptions-item label="状态">{{ getStateDisplayName(task.status.state) }}</el-descriptions-item>
            <el-descriptions-item label="智能体">{{ task.agent ? task.agent.name : 'N/A' }}</el-descriptions-item>
            <el-descriptions-item label="会话">{{ task.sessionId || 'N/A' }}</el-descriptions-item>
            <el-descriptions-item label="创建时间">{{ formatTime(task.status.startTime) }}</el-descriptions-item>
            <el-descriptions-item label="完成时间">{{ task.status.endTime ? formatTime(task.status.endTime) : 'N/A' }}</el-descriptions-item>
            <el-descriptions-item label="执行时长">{{ getExecutionTime(task) }}</el-descriptions-item>
            <el-descriptions-item label="优先级">{{ task.priority || '普通' }}</el-descriptions-item>
          </el-descriptions>
        </div>
      </el-card>
      
      <!-- 任务消息 -->
      <el-card class="mt-3">
        <template #header>
          <div class="card-header">
            <span>任务消息</span>
          </div>
        </template>
        <div v-if="task.messages && task.messages.length > 0" class="messages-container">
          <div v-for="(message, index) in task.messages" :key="index" class="message"
               :class="{ 'user-message': message.role === 'user', 'agent-message': message.role === 'agent' }">
            <div class="message-header">
              <el-avatar :size="small" :icon="message.role === 'user' ? 'el-icon-user' : 'el-icon-s-custom'"
                        :class="{ 'user-avatar': message.role === 'user', 'agent-avatar': message.role === 'agent' }">
                {{ message.role === 'user' ? '用户' : 'AI' }}
              </el-avatar>
              <span class="message-role">{{ message.role === 'user' ? '用户' : '智能体' }}</span>
              <span class="message-time">{{ formatTime(message.createdAt) }}</span>
            </div>
            <div class="message-content">
              <div v-for="(part, pIndex) in message.parts" :key="pIndex" class="message-part">
                <!-- 文本内容 -->
                <div v-if="part.text" class="text-content">
                  <codemirror
                    v-if="isJson(part.text)"
                    :model-value="formatJson(part.text)"
                    disabled
                    :style="{ height: 'auto', minHeight: '100px', maxHeight: '400px' }"
                    :extensions="cmJsonExtensions"
                  />
                  <pre v-else>{{ part.text }}</pre>
                </div>
                <!-- 结构化数据 -->
                <div v-if="part.data" class="data-content">
                  <el-collapse>
                    <el-collapse-item title="查看数据">
                      <codemirror
                        :model-value="JSON.stringify(part.data, null, 2)"
                        disabled
                        :style="{ height: 'auto', minHeight: '100px', maxHeight: '400px' }"
                        :extensions="cmJsonExtensions"
                      />
                    </el-collapse-item>
                  </el-collapse>
                </div>
                <!-- 文件 -->
                <div v-if="part.fileInfo" class="file-content">
                  <el-link :href="part.fileInfo.url" target="_blank" type="primary">
                    <i class="el-icon-download"></i> {{ part.fileInfo.filename }}
                  </el-link>
                </div>
              </div>
            </div>
          </div>
        </div>
        <el-empty v-else description="暂无消息记录" />
      </el-card>
      
      <!-- 任务控制 -->
      <el-card v-if="isTaskActive" class="mt-3">
        <template #header>
          <div class="card-header">
            <span>任务控制</span>
          </div>
        </template>
        <div class="task-controls">
          <el-button v-if="task.status.state === 'input-required'" type="primary" @click="showInputDialog" icon="el-icon-message">
            提供输入
          </el-button>
          <el-button v-if="['submitted', 'working', 'input-required'].includes(task.status.state)" 
                    type="danger" @click="confirmCancelTask" icon="el-icon-close">
            取消任务
          </el-button>
        </div>
      </el-card>
      
      <!-- 任务树 -->
      <task-tree :task-id="taskId" class="mt-3" />
      
      <!-- 任务状态历史 -->
      <task-state-history :task-id="taskId" class="mt-3" />
    </div>
    
    <!-- 输入对话框 -->
    <el-dialog
      v-model="inputDialogVisible"
      title="提供任务输入"
      width="50%"
    >
      <el-form :model="inputForm" label-width="80px">
        <el-form-item label="消息">
          <el-input type="textarea" v-model="inputForm.message" :rows="4" placeholder="请输入消息内容" />
        </el-form-item>
        <el-form-item label="数据">
          <el-input type="textarea" v-model="inputForm.jsonData" :rows="4" placeholder="可选: 输入JSON格式数据" />
        </el-form-item>
        <el-form-item label="文件">
          <el-upload
            action="#"
            :auto-upload="false"
            :on-change="handleFileChange"
            :limit="1"
          >
            <el-button type="primary">选择文件</el-button>
          </el-upload>
        </el-form-item>
      </el-form>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="inputDialogVisible = false">取消</el-button>
          <el-button type="primary" @click="submitInput" :loading="submitting">提交</el-button>
        </span>
      </template>
    </el-dialog>
    
    <!-- 确认取消任务对话框 -->
    <el-dialog
      v-model="cancelDialogVisible"
      title="确认取消任务"
      width="30%"
    >
      <p>您确定要取消这个任务吗？此操作不可撤销。</p>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="cancelDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="doCancelTask" :loading="cancelling">确认取消</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, defineComponent, onMounted, computed, shallowRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getTask, cancelTask, sendTaskInput } from '@/api/a2a' // Updated path
import TaskTree from '@/components/workflow/TaskTree.vue' // Updated path
import TaskStateHistory from '@/components/workflow/TaskStateHistory.vue' // Updated path
import { ElMessage } from 'element-plus'
import axios from 'axios'

// Codemirror imports
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'

export default defineComponent({
  name: 'TaskDetail',
  components: {
    TaskTree,
    TaskStateHistory,
    Codemirror
  },
  setup() {
    const route = useRoute()
    const router = useRouter()
    const taskId = ref(route.params.id)
    const task = ref(null)
    const loading = ref(true)
    const refreshLoading = ref(false)
    const error = ref(null)
    const inputDialogVisible = ref(false)
    const cancelDialogVisible = ref(false)
    const submitting = ref(false)
    const cancelling = ref(false)
    const inputForm = ref({
      message: '',
      jsonData: '',
      file: null
    })

    // Codemirror JSON Extensions
    const cmJsonExtensions = shallowRef([
      json(),
      oneDark,
      EditorView.lineWrapping,
      EditorView.editable.of(false)
    ])

    // 加载任务详情
    const loadTask = async () => {
      if (refreshLoading.value) return;
      
      if (!loading.value) {
        refreshLoading.value = true;
      } else {
        loading.value = true;
      }
      error.value = null;

      try {
        const response = await getTask(taskId.value)
        if (response.data.result && response.data.result.task) {
          task.value = response.data.result.task
        } else {
          error.value = '无法获取任务信息'
        }
      } catch (err) {
        console.error('加载任务失败', err)
        error.value = '加载任务失败: ' + (err.response?.data?.error?.message || err.message)
      } finally {
        loading.value = false;
        refreshLoading.value = false;
      }
    }

    // 检查文本是否是JSON
    const isJson = (text) => {
      if (!text) return false;
      try {
        const parsed = JSON.parse(text);
        return typeof parsed === 'object' && parsed !== null;
      } catch (e) {
        return false;
      }
    }

    // 格式化JSON文本
    const formatJson = (text) => {
      try {
        const parsed = JSON.parse(text);
        return JSON.stringify(parsed, null, 2);
      } catch (e) {
        return text;
      }
    }

    // 是否为活动任务（可以进行操作）
    const isTaskActive = computed(() => {
      return task.value && ['submitted', 'working', 'input-required'].includes(task.value.status.state)
    })

    // 格式化时间
    const formatTime = (timestamp) => {
      if (!timestamp) return ''
      const date = new Date(timestamp)
      return date.toLocaleString('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      })
    }

    // 格式化任务ID
    const formatTaskId = (id) => {
      if (!id) return ''
      if (id.length > 12) {
        return id.substring(0, 8) + '...'
      }
      return id
    }

    // 获取状态的显示名称
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

    // 获取执行时间
    const getExecutionTime = (task) => {
      if (!task || !task.status.startTime) return 'N/A'
      
      const startTime = new Date(task.status.startTime)
      const endTime = task.status.endTime ? new Date(task.status.endTime) : new Date()
      
      const diffMs = endTime - startTime
      if (diffMs < 1000) return `${diffMs}毫秒`
      
      const diffSec = Math.floor(diffMs / 1000)
      if (diffSec < 60) return `${diffSec}秒`
      
      const diffMin = Math.floor(diffSec / 60)
      const remainingSec = diffSec % 60
      if (diffMin < 60) return `${diffMin}分${remainingSec}秒`
      
      const diffHour = Math.floor(diffMin / 60)
      const remainingMin = diffMin % 60
      return `${diffHour}小时${remainingMin}分${remainingSec}秒`
    }

    // 返回上一页
    const goBack = () => {
      router.push({ name: 'task-list' }) // Assuming you have a named route for task list
    }

    // 确认取消任务
    const confirmCancelTask = () => {
      cancelDialogVisible.value = true
    }
    
    // 执行取消任务
    const doCancelTask = async () => {
      cancelling.value = true
      try {
        await cancelTask(taskId.value)
        ElMessage.success('任务已取消')
        cancelDialogVisible.value = false
        loadTask() // 重新加载任务状态
      } catch (err) {
        ElMessage.error('取消任务失败: ' + (err.response?.data?.error?.message || err.message))
      } finally {
        cancelling.value = false
      }
    }

    // 显示输入对话框
    const showInputDialog = () => {
      inputForm.value = {
        message: '',
        jsonData: '',
        file: null
      }
      inputDialogVisible.value = true
    }

    // 处理文件选择
    const handleFileChange = (file) => {
      inputForm.value.file = file.raw
    }

    // 提交用户输入
    const submitInput = async () => {
      submitting.value = true
      
      try {
        let inputData = {
          taskId: taskId.value,
          input: {}
        }
        
        // 添加消息内容
        if (inputForm.value.message) {
          inputData.input.text = inputForm.value.message
        }
        
        // 添加JSON数据
        if (inputForm.value.jsonData) {
          try {
            inputData.input.data = JSON.parse(inputForm.value.jsonData)
          } catch (error) {
            ElMessage.error('JSON数据格式不正确')
            submitting.value = false
            return
          }
        }
        
        // 添加文件（如果有）
        if (inputForm.value.file) {
          const formData = new FormData()
          formData.append('file', inputForm.value.file)
          
          // 先上传文件
          try {
            // Assuming the API endpoint for file upload is /api/a2a/files/upload
            // This might need adjustment based on your actual API
            const uploadResponse = await axios.post('/api/a2a/files/upload', formData) 
            if (uploadResponse.data && uploadResponse.data.fileId) {
              inputData.input.fileId = uploadResponse.data.fileId
            }
          } catch (err) {
            ElMessage.error('文件上传失败: ' + (err.response?.data?.error?.message || err.message))
            submitting.value = false
            return
          }
        }
        
        // 发送输入
        await sendTaskInput(inputData)
        
        ElMessage.success('输入已提交')
        inputDialogVisible.value = false
        loadTask() // 重新加载任务状态
      } catch (err) {
        ElMessage.error('提交输入失败: ' + (err.response?.data?.error?.message || err.message))
      } finally {
        submitting.value = false
      }
    }

    onMounted(() => {
      if (taskId.value) {
        loadTask()
      } else {
        error.value = '未提供任务ID'
      }
    })

    return {
      taskId,
      task,
      loading,
      refreshLoading,
      error,
      isTaskActive,
      inputDialogVisible,
      cancelDialogVisible,
      submitting,
      cancelling,
      inputForm,
      formatTime,
      formatTaskId,
      getStateDisplayName,
      getStatusTagType,
      getExecutionTime,
      goBack,
      confirmCancelTask,
      doCancelTask,
      showInputDialog,
      handleFileChange,
      submitInput,
      loadTask,
      cmJsonExtensions,
      isJson,
      formatJson
    }
  }
})
</script>

<style scoped>
.task-detail {
  padding: 20px;
}

.loading-card {
  margin-top: 20px;
}

.task-content {
  margin-top: 20px;
}

.mt-3 {
  margin-top: 1rem;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.messages-container {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.message {
  margin-bottom: 1rem;
}

.user-message {
  align-self: flex-end;
  background-color: #e1f3ff;
  padding: 0.8rem 1rem;
  border-radius: 0.5rem 0.5rem 0 0.5rem;
  margin-left: auto;
}

.agent-message {
  align-self: flex-start;
  background-color: #f5f5f5;
  padding: 0.8rem 1rem;
  border-radius: 0.5rem 0.5rem 0.5rem 0;
  margin-right: auto;
}

.message-header {
  display: flex;
  align-items: center;
  margin-bottom: 0.5rem;
}

.message-role {
  font-weight: bold;
  margin-left: 0.5rem;
}

.message-time {
  font-size: 0.8rem;
  color: #999;
  margin-left: 1rem;
}

.message-content {
  margin-top: 0;
  padding: 0;
  background-color: transparent;
  border-radius: 0;
}

.message-part {
  margin-bottom: 15px;
}

.message-part:last-child {
  margin-bottom: 0;
}

.text-content {
  margin-bottom: 10px;
}

.text-content pre {
  white-space: pre-wrap;
  overflow-wrap: break-word;
  margin: 0;
  padding: 10px;
  background-color: #f5f5f5;
  border-radius: 4px;
  font-family: monospace;
}

/* Codemirror styles */
.text-content :deep(.cm-editor),
.data-content :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
  background-color: #f8f8f8;
  border: 1px solid #dcdfe6;
}

.text-content :deep(.cm-scroller),
.data-content :deep(.cm-scroller) {
  padding: 8px;
  overflow: auto; /* Ensure scrollbar appears when maxHeight is reached */
}

.data-content {
  margin-top: 10px;
}

.data-content pre {
  margin: 0;
  padding: 10px;
  background-color: #f5f5f5;
  border-radius: 4px;
  white-space: pre-wrap;
  font-family: monospace;
}

.file-content {
  margin-top: 10px;
  padding: 10px;
  background-color: #f5f5f5;
  border-radius: 4px;
}

.user-avatar {
  background-color: #67c23a;
}

.agent-avatar {
  background-color: #409eff;
}

.task-controls {
  display: flex;
  gap: 1rem;
}
</style> 