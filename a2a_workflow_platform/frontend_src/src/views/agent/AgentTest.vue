<template>
  <div class="agent-test-container" v-loading="isLoading">
    <div class="page-header">
      <div class="title-section">
        <el-icon class="agent-icon"><avatar /></el-icon>
        <div class="title-text">
          <h1 class="page-title">测试智能体: {{ agent?.name || '加载中...' }}</h1>
          <el-tag v-if="agent" size="small" :type="getAgentTypeTag(agent.agent_type)" class="title-tag">
            {{ getAgentTypeLabel(agent.agent_type) }}
          </el-tag>
        </div>
      </div>
      <div class="action-buttons">
        <el-button @click="$router.push(`/agents/${agentId}`)">
          <el-icon><back /></el-icon> 返回详情
        </el-button>
        <el-button @click="$router.push('/agents')">
          <el-icon><grid /></el-icon> 返回列表
        </el-button>
      </div>
    </div>

    <el-alert
      v-if="error"
      :title="error"
      type="error"
      :closable="false"
      show-icon
      class="alert-margin"
    />

    <el-row :gutter="20">
      <el-col :md="24" :lg="8">
        <el-card class="test-card" v-if="agent">
          <template #header>
            <div class="card-header">
              <h2>智能体信息</h2>
            </div>
          </template>
          
          <el-descriptions direction="vertical" :column="1" border>
            <el-descriptions-item label="ID">{{ agentId }}</el-descriptions-item>
            <el-descriptions-item label="名称">{{ agent.name }}</el-descriptions-item>
            <el-descriptions-item label="类型">{{ getAgentTypeLabel(agent.agent_type) }}</el-descriptions-item>
            <el-descriptions-item label="模型">{{ agent.model_name }}</el-descriptions-item>
            <el-descriptions-item label="状态">
              <el-tag :type="agent.is_active ? 'success' : 'danger'">
                {{ agent.is_active ? '激活' : '未激活' }}
              </el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="A2A协议">
              <el-tag :type="agent.is_a2a_compliant ? 'success' : 'info'">
                {{ agent.is_a2a_compliant ? 'A2A兼容' : '非A2A兼容' }}
              </el-tag>
            </el-descriptions-item>
          </el-descriptions>
          
          <div class="test-options">
            <h3>测试选项</h3>
            <el-form :model="testOptions" label-position="top">
              <el-form-item label="测试模式">
                <el-radio-group v-model="testOptions.mode">
                  <el-radio-button label="chat">对话测试</el-radio-button>
                  <el-radio-button label="a2a" :disabled="!agent.is_a2a_compliant">A2A任务测试</el-radio-button>
                </el-radio-group>
              </el-form-item>
              
              <el-form-item label="测试参数" v-if="testOptions.mode === 'a2a'">
                <el-input
                  v-model="testOptions.a2aParams"
                  type="textarea"
                  :rows="5"
                  placeholder="请输入A2A任务参数（JSON格式）"
                />
              </el-form-item>
              
              <el-form-item label="系统提示" v-if="testOptions.mode === 'chat'">
                <el-input
                  v-model="testOptions.systemPrompt"
                  type="textarea"
                  :rows="3"
                  placeholder="可选的系统提示"
                />
              </el-form-item>
              
              <el-form-item>
                <el-button type="primary" @click="clearConversation">
                  <el-icon><delete /></el-icon> 清除对话
                </el-button>
              </el-form-item>
            </el-form>
          </div>
        </el-card>
      </el-col>
      
      <el-col :md="24" :lg="16">
        <el-card class="message-card">
          <template #header>
            <div class="card-header">
              <h2>测试会话</h2>
              <el-tag>{{ testOptions.mode === 'chat' ? '对话模式' : 'A2A任务模式' }}</el-tag>
            </div>
          </template>
          
          <div class="message-container" ref="messageContainer">
            <div v-if="messages.length === 0" class="empty-messages">
              <el-empty description="暂无消息，开始您的对话吧" />
            </div>
            
            <div v-else class="messages">
              <div
                v-for="(message, index) in messages"
                :key="index"
                :class="['message', message.role === 'user' ? 'user-message' : 'agent-message']"
              >
                <div class="message-header">
                  <strong>{{ message.role === 'user' ? '用户' : '智能体' }}</strong>
                  <span class="message-time">{{ formatTime(message.timestamp) }}</span>
                </div>
                <div class="message-content" v-html="formatMessageContent(message.content)"></div>
              </div>
            </div>
            
            <div v-if="isResponding" class="typing-indicator">
              <div class="dot"></div>
              <div class="dot"></div>
              <div class="dot"></div>
            </div>
          </div>
          
          <div class="message-input">
            <el-input
              v-model="userMessage"
              type="textarea"
              :rows="3"
              placeholder="输入消息..."
              :disabled="isResponding"
              @keydown.enter.ctrl="sendMessage"
            />
            <div class="input-actions">
              <span class="input-hint">按 Ctrl+Enter 发送</span>
              <el-button
                type="primary"
                @click="sendMessage"
                :disabled="isResponding || !userMessage.trim()"
                :loading="isResponding"
              >
                <el-icon><chat-line-round /></el-icon> 发送
              </el-button>
            </div>
          </div>
        </el-card>
      </el-col>
    </el-row>
    
    <el-card class="tasks-card" v-if="testOptions.mode === 'a2a' && tasks.length > 0">
      <template #header>
        <div class="card-header">
          <h2>A2A任务历史</h2>
        </div>
      </template>
      
      <el-table :data="tasks" style="width: 100%">
        <el-table-column prop="id" label="任务ID" width="280" />
        <el-table-column prop="title" label="标题" />
        <el-table-column prop="state" label="状态" width="120">
          <template #default="scope">
            <el-tag :type="getTaskStateTag(scope.row.state)">{{ scope.row.state }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="created_at" label="创建时间" width="180">
          <template #default="scope">
            {{ formatDate(scope.row.created_at) }}
          </template>
        </el-table-column>
        <el-table-column label="操作" width="150">
          <template #default="scope">
            <el-button size="small" @click="viewTaskDetails(scope.row)">查看详情</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-card>
    
    <!-- 任务详情对话框 -->
    <el-dialog
      v-model="taskDialogVisible"
      :title="`任务详情 - ${currentTask?.id || ''}`"
      width="70%"
    >
      <div v-if="currentTask" class="task-detail">
        <el-descriptions :column="2" border>
          <el-descriptions-item label="任务ID">{{ currentTask.id }}</el-descriptions-item>
          <el-descriptions-item label="状态">
            <el-tag :type="getTaskStateTag(currentTask.state)">{{ currentTask.state }}</el-tag>
          </el-descriptions-item>
          <el-descriptions-item label="标题">{{ currentTask.title || '无标题' }}</el-descriptions-item>
          <el-descriptions-item label="创建时间">{{ formatDate(currentTask.created_at) }}</el-descriptions-item>
        </el-descriptions>
        
        <div class="section-title">
          <h3>元数据</h3>
        </div>
        <pre v-if="currentTask.metadata" class="json-display">{{ formatJSON(currentTask.metadata) }}</pre>
        <el-empty v-else description="无元数据" />
        
        <div class="section-title">
          <h3>消息历史</h3>
        </div>
        <div v-if="currentTask.messages && currentTask.messages.length" class="task-messages">
          <div v-for="(msg, index) in currentTask.messages" :key="index" class="task-message">
            <div class="task-message-header">
              <strong>{{ msg.role }}</strong>
              <span>{{ formatDate(msg.created_at) }}</span>
            </div>
            <div v-for="(part, partIndex) in msg.parts" :key="partIndex" class="task-message-part">
              <div class="part-header">
                <span>{{ part.content_type }}</span>
              </div>
              <div v-if="part.text_content" class="part-content">
                <pre>{{ part.text_content }}</pre>
              </div>
              <div v-else-if="part.data_content" class="part-content">
                <pre>{{ formatJSON(part.data_content) }}</pre>
              </div>
              <div v-else class="part-content">
                <em>非文本内容</em>
              </div>
            </div>
          </div>
        </div>
        <el-empty v-else description="无消息历史" />
      </div>
    </el-dialog>
  </div>
</template>

<script>
import { ref, computed, onMounted, nextTick, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAgentStore } from '@/store/agent'
import { ElMessage } from 'element-plus'
import axios from 'axios'
import {
  Back,
  Grid,
  Delete,
  ChatLineRound,
  Avatar,
  Promotion,
  View
} from '@element-plus/icons-vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'

export default {
  name: 'AgentTest',
  components: {
    Back,
    Grid,
    Delete,
    ChatLineRound,
    Avatar,
    Promotion,
    View
  },
  setup() {
    const route = useRoute()
    const router = useRouter()
    const agentStore = useAgentStore()
    const messageContainer = ref(null)
    
    const agentId = ref(route.params.id)
    
    // 从store获取状态
    const agent = computed(() => agentStore.currentAgent)
    const isLoading = computed(() => agentStore.isLoading)
    const error = computed(() => agentStore.error)
    
    // 测试相关状态
    const messages = ref([])
    const userMessage = ref('')
    const isResponding = ref(false)
    const tasks = ref([])
    const currentTask = ref(null)
    const taskDialogVisible = ref(false)
    
    // 测试选项
    const testOptions = ref({
      mode: 'chat', // chat 或 a2a
      systemPrompt: '',
      a2aParams: JSON.stringify({
        title: "测试任务",
        metadata: {
          importance: "high"
        }
      }, null, 2)
    })
    
    // 获取智能体详情
    const fetchAgentDetail = async () => {
      await agentStore.fetchAgent(agentId.value)
    }
    
    // 发送消息
    const sendMessage = async () => {
      if (!userMessage.value.trim() || isResponding.value) return
      
      const msgContent = userMessage.value.trim()
      userMessage.value = ''
      
      // 添加用户消息到列表
      messages.value.push({
        role: 'user',
        content: msgContent,
        timestamp: new Date()
      })
      
      // 滚动到底部
      await scrollToBottom()
      
      isResponding.value = true
      
      try {
        if (testOptions.value.mode === 'chat') {
          await sendChatMessage(msgContent)
        } else {
          await sendA2ATask(msgContent)
        }
      } catch (err) {
        console.error('发送消息失败', err)
        
        // 添加错误提示
        messages.value.push({
          role: 'assistant',
          content: '发送消息时发生错误，请重试。',
          timestamp: new Date()
        })
      } finally {
        isResponding.value = false
        await scrollToBottom()
      }
    }
    
    // 发送常规聊天消息
    const sendChatMessage = async (content) => {
      const response = await agentStore.sendChatMessage(
        agentId.value,
        content,
        testOptions.value.systemPrompt || undefined
      )
      
      // 添加回复到消息列表
      if (response && response.reply) {
        messages.value.push({
          role: 'assistant',
          content: response.reply,
          timestamp: new Date()
        })
      }
    }
    
    // 发送A2A任务
    const sendA2ATask = async (content) => {
      // 解析A2A参数
      let taskParams = {}
      try {
        taskParams = JSON.parse(testOptions.value.a2aParams)
      } catch (e) {
        ElMessage.warning('A2A参数格式不正确，使用默认参数')
        taskParams = { title: "测试任务" }
      }
      
      // 构建任务数据
      const taskData = {
        ...taskParams,
        input: {
          role: "user",
          content: [
            {
              type: "text",
              text: content
            }
          ]
        }
      }
      
      // 使用store发送A2A任务
      const response = await agentStore.createA2ATask(agentId.value, taskData)
      
      if (response && response.id) {
        // 记录任务
        tasks.value.unshift(response)
        
        // 获取任务结果
        await fetchTaskResult(response.id)
      }
    }
    
    // 获取任务结果
    const fetchTaskResult = async (taskId) => {
      // 模拟轮询获取任务状态
      let attempts = 0
      const maxAttempts = 30
      const pollInterval = 1000
      
      while (attempts < maxAttempts) {
        try {
          const response = await agentStore.fetchTaskDetail(agentId.value, taskId)
          
          // 更新任务列表中的任务状态
          const taskIndex = tasks.value.findIndex(t => t.id === taskId)
          if (taskIndex >= 0) {
            tasks.value[taskIndex] = response
          }
          
          // 如果任务已完成，提取回复
          if (response.state === 'completed') {
            extractTaskReply(response)
            break
          }
          
          // 如果任务失败，显示错误
          if (['failed', 'canceled', 'expired'].includes(response.state)) {
            messages.value.push({
              role: 'assistant',
              content: `任务执行失败，状态: ${response.state}`,
              timestamp: new Date()
            })
            break
          }
          
          // 继续轮询
          attempts++
          await new Promise(resolve => setTimeout(resolve, pollInterval))
        } catch (err) {
          console.error('获取任务结果失败', err)
          messages.value.push({
            role: 'assistant',
            content: '获取任务结果失败，请重试。',
            timestamp: new Date()
          })
          break
        }
      }
      
      if (attempts >= maxAttempts) {
        messages.value.push({
          role: 'assistant',
          content: '任务执行超时，请稍后查看任务状态。',
          timestamp: new Date()
        })
      }
    }
    
    // 从任务中提取回复
    const extractTaskReply = (task) => {
      if (!task.messages || task.messages.length === 0) {
        messages.value.push({
          role: 'assistant',
          content: '任务完成，但没有收到回复。',
          timestamp: new Date()
        })
        return
      }
      
      // 找出助手回复
      const assistantMessages = task.messages.filter(m => m.role === 'assistant')
      if (assistantMessages.length === 0) {
        messages.value.push({
          role: 'assistant',
          content: '任务完成，但没有收到助手回复。',
          timestamp: new Date()
        })
        return
      }
      
      // 使用最新的回复
      const latestMessage = assistantMessages[assistantMessages.length - 1]
      
      // 提取文本内容
      let replyContent = ''
      if (latestMessage.parts && latestMessage.parts.length > 0) {
        for (const part of latestMessage.parts) {
          if (part.text_content) {
            replyContent += part.text_content + '\n\n'
          } else if (part.data_content) {
            replyContent += '```json\n' + JSON.stringify(part.data_content, null, 2) + '\n```\n\n'
          }
        }
      }
      
      if (!replyContent) {
        replyContent = '任务完成，但回复内容无法解析。'
      }
      
      // 添加到消息列表
      messages.value.push({
        role: 'assistant',
        content: replyContent.trim(),
        timestamp: new Date()
      })
    }
    
    // 查看任务详情
    const viewTaskDetails = async (task) => {
      try {
        const response = await agentStore.fetchTaskDetail(agentId.value, task.id)
        currentTask.value = response
        taskDialogVisible.value = true
      } catch (err) {
        console.error('获取任务详情失败', err)
        ElMessage.error('获取任务详情失败')
      }
    }
    
    // 清除对话
    const clearConversation = () => {
      messages.value = []
      ElMessage.success('对话已清除')
    }
    
    // 滚动到对话底部
    const scrollToBottom = async () => {
      await nextTick()
      if (messageContainer.value) {
        messageContainer.value.scrollTop = messageContainer.value.scrollHeight
      }
    }
    
    // 格式化时间
    const formatTime = (date) => {
      if (!date) return ''
      const d = new Date(date)
      return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
    }
    
    // 格式化日期
    const formatDate = (dateString) => {
      if (!dateString) return '未知日期'
      const date = new Date(dateString)
      return date.toLocaleString('zh-CN', { 
        year: 'numeric', 
        month: '2-digit', 
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      })
    }
    
    // 格式化消息内容（支持Markdown）
    const formatMessageContent = (content) => {
      if (!content) return ''
      try {
        const html = marked.parse(content)
        return DOMPurify.sanitize(html)
      } catch (e) {
        return content
      }
    }
    
    // 格式化JSON
    const formatJSON = (data) => {
      return JSON.stringify(data, null, 2)
    }
    
    // 获取智能体类型的标签类型
    const getAgentTypeTag = (type) => {
      const typeMap = {
        'gpt-3.5': 'info',
        'gpt-4': 'success',
        'claude-3': 'warning',
        'gemini': 'danger',
        'custom': 'primary',
        'a2a': ''
      }
      return typeMap[type] || ''
    }
    
    // 获取智能体类型的显示标签
    const getAgentTypeLabel = (type) => {
      const labelMap = {
        'gpt-3.5': 'GPT-3.5',
        'gpt-4': 'GPT-4',
        'claude-3': 'Claude 3',
        'gemini': 'Gemini',
        'custom': '自定义',
        'a2a': 'A2A兼容'
      }
      return labelMap[type] || type
    }
    
    // 获取任务状态的标签类型
    const getTaskStateTag = (state) => {
      const stateMap = {
        'pending': 'info',
        'running': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'canceled': 'info',
        'expired': 'info'
      }
      return stateMap[state] || ''
    }
    
    // 生命周期钩子
    onMounted(() => {
      fetchAgentDetail()
    })
    
    return {
      agent,
      agentId,
      isLoading,
      error,
      messages,
      userMessage,
      isResponding,
      tasks,
      testOptions,
      messageContainer,
      currentTask,
      taskDialogVisible,
      sendMessage,
      clearConversation,
      viewTaskDetails,
      formatTime,
      formatDate,
      formatMessageContent,
      formatJSON,
      getAgentTypeTag,
      getAgentTypeLabel,
      getTaskStateTag
    }
  }
}
</script>

<style scoped>
.agent-test-container {
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

.alert-margin {
  margin-bottom: 20px;
}

.test-card, .message-card, .tasks-card {
  margin-bottom: 20px;
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.card-header h2 {
  margin: 0;
  font-size: 18px;
}

.test-options {
  margin-top: 20px;
}

.test-options h3 {
  margin-top: 0;
  margin-bottom: 15px;
  font-size: 16px;
  padding-bottom: 8px;
  border-bottom: 1px solid #EBEEF5;
}

.message-container {
  height: 400px;
  overflow-y: auto;
  padding: 10px;
  background-color: #f9f9f9;
  border-radius: 4px;
  margin-bottom: 15px;
}

.empty-messages {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.message {
  margin-bottom: 15px;
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

.message-input {
  margin-top: 15px;
}

.input-actions {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 8px;
}

.input-hint {
  color: #909399;
  font-size: 12px;
}

.typing-indicator {
  display: flex;
  padding: 10px;
  width: 70px;
  border-radius: 8px;
  background-color: #ffffff;
  border: 1px solid #e8e8e8;
}

.dot {
  height: 10px;
  width: 10px;
  margin: 0 5px;
  background-color: #bbb;
  border-radius: 50%;
  animation: pulse 1.5s infinite;
}

.dot:nth-child(2) {
  animation-delay: 0.3s;
}

.dot:nth-child(3) {
  animation-delay: 0.6s;
}

@keyframes pulse {
  0% {
    transform: scale(0.8);
    opacity: 0.5;
  }
  50% {
    transform: scale(1);
    opacity: 1;
  }
  100% {
    transform: scale(0.8);
    opacity: 0.5;
  }
}

.section-title {
  margin: 20px 0 10px;
  padding-bottom: 5px;
  border-bottom: 1px solid #EBEEF5;
}

.json-display {
  background-color: #f7f7f7;
  padding: 10px;
  border-radius: 4px;
  white-space: pre-wrap;
  font-family: monospace;
  overflow-x: auto;
}

.task-messages {
  max-height: 400px;
  overflow-y: auto;
}

.task-message {
  margin-bottom: 15px;
  border: 1px solid #e8e8e8;
  border-radius: 4px;
}

.task-message-header {
  display: flex;
  justify-content: space-between;
  padding: 10px;
  background-color: #f5f7fa;
  border-bottom: 1px solid #e8e8e8;
}

.task-message-part {
  padding: 10px;
  border-bottom: 1px solid #ebeef5;
}

.task-message-part:last-child {
  border-bottom: none;
}

.part-header {
  font-size: 12px;
  color: #909399;
  margin-bottom: 5px;
}

.part-content {
  white-space: pre-wrap;
  word-break: break-word;
}

.part-content pre {
  margin: 0;
  padding: 10px;
  background-color: #f7f7f7;
  border-radius: 4px;
  overflow-x: auto;
}
</style>
