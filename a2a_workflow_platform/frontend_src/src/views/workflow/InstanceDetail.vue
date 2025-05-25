<template>
  <div class="instance-detail" v-loading="isLoading">
    <div class="page-header">
      <div class="title-area">
        <h1>{{ instance?.name || '工作流实例详情' }}</h1>
        <el-tag :type="getStatusType(instance?.status)">{{ getStatusText(instance?.status) }}</el-tag>
      </div>
      <div class="actions">
        <el-button @click="$router.push('/workflow/instances')" type="default">
          <el-icon><back /></el-icon> 返回列表
        </el-button>
        <el-button 
          v-if="instance?.status === 'running'"
          @click="pauseInstance"
          type="warning"
          :loading="isPausing"
        >
          <el-icon><video-pause /></el-icon> 暂停
        </el-button>
        <el-button 
          v-if="instance?.status === 'paused'"
          @click="resumeInstance"
          type="success"
          :loading="isResuming"
        >
          <el-icon><video-play /></el-icon> 继续
        </el-button>
        <el-button 
          v-if="['running', 'paused'].includes(instance?.status)"
          @click="confirmCancel"
          type="danger"
          :loading="isCancelling"
        >
          <el-icon><circle-close /></el-icon> 取消
        </el-button>
      </div>
    </div>

    <el-card v-if="instance" class="instance-info">
      <template #header>
        <div class="card-header">
          <h3>实例信息</h3>
          <el-button 
            link 
            type="primary" 
            @click="refreshInstance"
            :loading="isRefreshingInstance"
          >
            <el-icon><refresh /></el-icon> 刷新
          </el-button>
        </div>
      </template>
      <el-descriptions :column="2" border>
        <el-descriptions-item label="实例ID">{{ instance.instance_id }}</el-descriptions-item>
        <el-descriptions-item label="状态">
          <el-tag :type="getStatusType(instance.status)">{{ getStatusText(instance.status) }}</el-tag>
        </el-descriptions-item>
        <el-descriptions-item label="工作流名称">{{ instance.workflow?.name || '未知工作流' }}</el-descriptions-item>
        <el-descriptions-item label="创建者">{{ instance.created_by?.username || '系统' }}</el-descriptions-item>
        <el-descriptions-item label="创建时间">{{ formatDate(instance.created_at) }}</el-descriptions-item>
        <el-descriptions-item label="更新时间">{{ formatDate(instance.updated_at) }}</el-descriptions-item>
        <el-descriptions-item label="当前步骤" :span="2">
          {{ instance.current_step_index }} / {{ stepsCount }}
        </el-descriptions-item>
        <el-descriptions-item label="开始时间" v-if="instance.started_at">
          {{ formatDate(instance.started_at) }}
        </el-descriptions-item>
        <el-descriptions-item label="完成时间" v-if="instance.completed_at">
          {{ formatDate(instance.completed_at) }}
        </el-descriptions-item>
      </el-descriptions>
    </el-card>

    <!-- 工作流监控组件 -->
    <workflow-monitor 
      :instance="instance" 
      title="工作流监控" 
      @refresh="refreshInstance"
      @retry-step="retryStep"
      :parent-loading="isRefreshingInstance"
    />

    <el-tabs v-model="activeTab" class="detail-tabs">
      <el-tab-pane label="步骤详情" name="steps">
        <el-card v-if="instance && instance.steps && instance.steps.length > 0" class="steps-card">
          <!-- 错误处理组件 - 如果存在失败的步骤就显示 -->
          <workflow-error-handler
            v-if="failedStep"
            :error="failedStep.error"
            :step-type="failedStep.step_type"
            :step-name="failedStep.step_name"
            :workflow-id="instance.workflow?.id"
            :instance-id="instance.instance_id"
            @retry="retryStep(failedStep.id)"
            @view-logs="activeTab = 'logs'"
            @skip="skipFailedStep"
            @debug="goToWorkflowDebug"
          />
          
          <el-timeline>
            <el-timeline-item
              v-for="step in instance.steps"
              :key="step.id"
              :type="getStepIconType(step.status)"
              :color="getStepIconColor(step.status)"
              :timestamp="step.started_at ? formatDate(step.started_at) : '未开始'"
            >
              <el-card class="step-card">
                <template #header>
                  <div class="step-header">
                    <span class="step-name">{{ step.step_name || '未命名步骤' }}</span>
                    <el-tag size="small" :type="getStepStatusType(step.status)">{{ getStepStatusText(step.status) }}</el-tag>
                  </div>
                </template>
                <div class="step-content">
                  <p><strong>步骤类型:</strong> {{ getStepTypeText(step.step_type) }}</p>
                  <p v-if="step.started_at"><strong>开始时间:</strong> {{ formatDate(step.started_at) }}</p>
                  <p v-if="step.completed_at"><strong>完成时间:</strong> {{ formatDate(step.completed_at) }}</p>
                  
                  <div v-if="step.parameters" class="step-params">
                    <p><strong>参数:</strong></p>
                    <codemirror
                      :model-value="JSON.stringify(step.parameters, null, 2)"
                      disabled
                      :style="{ height: 'auto', maxHeight: '300px' }"
                      :extensions="cmJsonExtensions"
                    />
                  </div>
                  
                  <div v-if="step.output_data" class="step-output">
                    <p><strong>输出:</strong></p>
                    <codemirror
                      :model-value="JSON.stringify(step.output_data, null, 2)"
                      disabled
                      :style="{ height: 'auto', maxHeight: '300px' }"
                      :extensions="cmJsonExtensions"
                    />
                  </div>
                  
                  <div v-if="step.error" class="step-error">
                    <p><strong>错误信息:</strong></p>
                    <el-alert type="error" :closable="false">
                      {{ step.error }}
                    </el-alert>
                  </div>
                  
                  <div v-if="step.status === 'failed'" class="step-actions">
                    <el-button 
                      type="primary" 
                      size="small" 
                      @click="retryStep(step.id)"
                      :loading="isRetrying[step.id]"
                    >
                      <el-icon><refresh-right /></el-icon> 重试此步骤
                    </el-button>
                  </div>
                </div>
              </el-card>
            </el-timeline-item>
          </el-timeline>
        </el-card>
      </el-tab-pane>
      
      <!-- 新增实时监控标签页 -->
      <el-tab-pane label="实时监控" name="realtime" v-if="isActiveInstance">
        <workflow-real-time-monitor
          :instance-id="instance.instance_id"
          title="工作流实时监控"
          @metrics-updated="handleMetricsUpdate"
        />
      </el-tab-pane>
      
      <el-tab-pane label="上下文数据" name="context">
        <el-card v-if="instance && instance.context" class="context-card">
          <div class="code-container">
            <codemirror
              :model-value="JSON.stringify(instance.context, null, 2)"
              disabled
              :style="{ height: 'auto', maxHeight: '600px' }"
              :extensions="cmJsonExtensions"
            />
          </div>
        </el-card>
        <el-empty v-else description="暂无上下文数据" />
      </el-tab-pane>
      
      <el-tab-pane label="输出数据" name="output">
        <el-card v-if="instance && instance.output" class="output-card">
          <div class="code-container">
            <codemirror
              :model-value="JSON.stringify(instance.output, null, 2)"
              disabled
              :style="{ height: 'auto', maxHeight: '600px' }"
              :extensions="cmJsonExtensions"
            />
          </div>
        </el-card>
        <el-empty v-else description="暂无输出数据" />
      </el-tab-pane>
      
      <!-- 新增日志标签页 -->
      <el-tab-pane label="完整日志" name="logs">
        <el-card class="logs-card">
          <div class="logs-header">
            <h3>系统日志</h3>
            <div class="logs-actions">
              <el-select v-model="logLevel" placeholder="日志级别" size="small">
                <el-option label="全部" value="all" />
                <el-option label="信息" value="info" />
                <el-option label="警告" value="warning" />
                <el-option label="错误" value="error" />
              </el-select>
              <el-button 
                type="primary" 
                size="small" 
                @click="refreshLogs"
                :loading="isRefreshingLogs"
              >
                <el-icon><refresh /></el-icon> 刷新
              </el-button>
              <el-button type="default" size="small" @click="downloadLogs">
                <el-icon><download /></el-icon> 下载
              </el-button>
            </div>
          </div>
          
          <div v-if="logs.length === 0" class="no-logs">
            <el-empty description="暂无日志记录" />
          </div>
          
          <div v-else class="logs-content">
            <div 
              v-for="(log, index) in filteredLogs" 
              :key="index"
              class="log-entry"
              :class="`log-${log.level}`"
            >
              <div class="log-timestamp">{{ formatDate(log.timestamp) }}</div>
              <div class="log-level">
                <el-tag size="small" :type="getLogLevelType(log.level)">{{ log.level }}</el-tag>
              </div>
              <div class="log-message">{{ log.message }}</div>
            </div>
          </div>
        </el-card>
      </el-tab-pane>
    </el-tabs>

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
          <el-button type="danger" @click="cancelInstance" :loading="isCancelling">确认</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, computed, onMounted, onUnmounted, watch, reactive, shallowRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useWorkflowStore } from '@/store/workflow'
import { 
  Back, 
  VideoPause, 
  VideoPlay, 
  CircleClose, 
  Refresh,
  RefreshRight,
  Download
} from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import WorkflowMonitor from '@/components/workflow/WorkflowMonitor.vue'
import WorkflowErrorHandler from '@/components/workflow/WorkflowErrorHandler.vue'
import WorkflowRealTimeMonitor from '@/components/workflow/WorkflowRealTimeMonitor.vue'

// Codemirror imports
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'

export default {
  name: 'InstanceDetail',
  components: {
    Back,
    VideoPause,
    VideoPlay,
    CircleClose,
    Refresh,
    RefreshRight,
    Download,
    WorkflowMonitor,
    WorkflowErrorHandler,
    WorkflowRealTimeMonitor,
    Codemirror // Register Codemirror component
  },
  setup() {
    const route = useRoute()
    const router = useRouter()
    const workflowStore = useWorkflowStore()
    
    const instance = computed(() => workflowStore.currentInstance)
    const isLoading = computed(() => workflowStore.isLoading)
    const stepsCount = computed(() => instance.value?.steps?.length || 0)
    
    const cancelDialogVisible = ref(false)
    const activeTab = ref('steps')
    const logLevel = ref('all')
    const logs = ref([])
    const isPausing = ref(false)
    const isResuming = ref(false)
    const isCancelling = ref(false)
    const isRetrying = reactive({})
    const isRefreshingInstance = ref(false)
    const isRefreshingLogs = ref(false)
    
    // Codemirror JSON Extensions
    const cmJsonExtensions = shallowRef([json(), oneDark, EditorView.lineWrapping, EditorView.editable.of(false)]);
    
    // 计算属性: 活跃实例（运行中或已暂停）
    const isActiveInstance = computed(() => {
      return instance.value && ['running', 'paused'].includes(instance.value.status)
    })
    
    // 计算属性: 失败的步骤
    const failedStep = computed(() => {
      if (!instance.value || !instance.value.steps) return null
      return instance.value.steps.find(step => step.status === 'failed')
    })
    
    // 计算属性: 过滤后的日志
    const filteredLogs = computed(() => {
      if (logLevel.value === 'all') return logs.value
      return logs.value.filter(log => log.level === logLevel.value)
    })
    
    const fetchInstance = async () => {
      isRefreshingInstance.value = true // Use dedicated loading state
      try {
        const id = route.params.id
        if (!id) {
          ElMessage.error('未找到实例ID')
          router.push('/workflow/instances')
          return
        }
        
        await workflowStore.fetchWorkflowInstance(id)
        
        if (!workflowStore.currentInstance) {
          ElMessage.error('未找到工作流实例')
          router.push('/workflow/instances')
        }
      } catch (error) {
        console.error('获取工作流实例失败', error)
        ElMessage.error(error.message || '获取工作流实例失败')
      } finally {
        isRefreshingInstance.value = false
      }
    }
    
    const refreshInstance = () => {
      fetchInstance()
    }
    
    const pauseInstance = async () => {
      isPausing.value = true
      try {
        const result = await workflowStore.pauseWorkflowInstance(instance.value.instance_id)
        if (result) {
          ElMessage.success('工作流实例已暂停')
          refreshInstance() // fetchInstance handles its own loading state
        }
      } catch (error) {
        ElMessage.error(error.message || '暂停工作流实例失败')
      } finally {
        isPausing.value = false
      }
    }
    
    const resumeInstance = async () => {
      isResuming.value = true
      try {
        const result = await workflowStore.resumeWorkflowInstance(instance.value.instance_id)
        if (result) {
          ElMessage.success('工作流实例已恢复')
          refreshInstance()
        }
      } catch (error) {
        ElMessage.error(error.message || '恢复工作流实例失败')
      } finally {
        isResuming.value = false
      }
    }
    
    const confirmCancel = () => {
      cancelDialogVisible.value = true
    }
    
    const cancelInstance = async () => {
      isCancelling.value = true
      try {
        const result = await workflowStore.cancelWorkflowInstance(instance.value.instance_id)
        if (result) {
          ElMessage.success('工作流实例已取消')
          cancelDialogVisible.value = false
          refreshInstance()
        }
      } catch (error) {
        ElMessage.error(error.message || '取消工作流实例失败')
      } finally {
        isCancelling.value = false
      }
    }
    
    const retryStep = async (stepId) => {
      isRetrying[stepId] = true
      try {
        const result = await workflowStore.retryWorkflowStep(instance.value.instance_id, stepId)
        if (result) {
          ElMessage.success('步骤重试已触发')
          refreshInstance()
        }
      } catch (error) {
        ElMessage.error(error.message || '重试步骤失败')
      } finally {
        isRetrying[stepId] = false
      }
    }
    
    // 跳过失败步骤
    const skipFailedStep = async () => {
      if (!failedStep.value || !failedStep.value.id) return;
      
      try {
        const stepId = failedStep.value.id;
        isRetrying[stepId] = true; // 重用isRetrying状态保持UI一致性
        
        // 模拟API调用
        await new Promise(resolve => setTimeout(resolve, 1000));
        
        ElMessage.warning('步骤已跳过（功能待实现）');
        refreshInstance();
      } catch (error) {
        ElMessage.error('跳过步骤失败');
      } finally {
        if (failedStep.value && failedStep.value.id) {
          isRetrying[failedStep.value.id] = false;
        }
      }
    }
    
    const formatDate = (dateString) => {
      if (!dateString) return '未设置'
      const date = new Date(dateString)
      return new Intl.DateTimeFormat('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      }).format(date)
    }
    
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
    
    const getStepStatusType = (status) => {
      const statusMap = {
        'pending': 'info',
        'running': 'primary',
        'completed': 'success',
        'failed': 'danger',
        'skipped': 'warning'
      }
      return statusMap[status] || 'info'
    }
    
    const getStepStatusText = (status) => {
      const statusMap = {
        'pending': '待执行',
        'running': '执行中',
        'completed': '已完成',
        'failed': '失败',
        'skipped': '已跳过'
      }
      return statusMap[status] || '未知状态'
    }
    
    const getStepTypeText = (type) => {
      const typeMap = {
        'a2a_client': 'A2A智能体',
        'condition': '条件判断',
        'loop': '循环',
        'transform': '数据转换'
      }
      return typeMap[type] || type
    }
    
    const getStepIconType = (status) => {
      if (status === 'failed') return 'danger'
      if (status === 'completed') return 'success'
      if (status === 'running') return 'primary'
      return ''
    }
    
    const getStepIconColor = (status) => {
      if (status === 'failed') return '#F56C6C'
      if (status === 'completed') return '#67C23A'
      if (status === 'running') return '#409EFF'
      if (status === 'pending') return '#909399'
      return '#E6A23C'
    }
    
    // 获取并刷新日志
    const refreshLogs = async () => {
      if (!instance.value) return
      isRefreshingLogs.value = true
      try {
        // 实际项目中这里应该调用API获取日志
        // 目前使用模拟数据
        console.log(`模拟获取日志: instance=${instance.value.instance_id}, level=${logLevel.value}`)
        await new Promise(resolve => setTimeout(resolve, 500)) // Simulate API delay
        logs.value = generateMockLogs()
        ElMessage.success('日志刷新成功（模拟）')
      } catch (error) {
        console.error('获取日志失败', error)
        ElMessage.error(error.message || '获取日志失败')
      } finally {
        isRefreshingLogs.value = false
      }
    }
    
    // 下载日志
    const downloadLogs = () => {
      if (!logs.value || logs.value.length === 0) {
        ElMessage.warning('没有可下载的日志')
        return
      }
      
      try {
        // 格式化日志内容
        const logContent = logs.value.map(log => {
          return `[${log.timestamp}] [${log.level.toUpperCase()}] ${log.message}`
        }).join('\n')
        
        // 创建Blob对象
        const blob = new Blob([logContent], { type: 'text/plain' })
        
        // 创建下载链接
        const url = URL.createObjectURL(blob)
        const link = document.createElement('a')
        link.href = url
        link.download = `workflow-${instance.value.instance_id}-logs.txt`
        
        // 模拟点击下载
        document.body.appendChild(link)
        link.click()
        
        // 清理
        URL.revokeObjectURL(url)
        document.body.removeChild(link)
        
        ElMessage.success('日志下载成功')
      } catch (error) {
        console.error('下载日志失败', error)
        ElMessage.error('下载日志失败')
      }
    }
    
    // 生成模拟日志数据
    const generateMockLogs = () => {
      const mockLogs = []
      const logCount = 20 + Math.floor(Math.random() * 30)
      const levels = ['info', 'info', 'info', 'warning', 'error']
      const timeBase = new Date(instance.value.created_at || new Date())
      
      for (let i = 0; i < logCount; i++) {
        const level = levels[Math.floor(Math.random() * levels.length)]
        const timeOffset = i * (1000 + Math.random() * 5000)
        const timestamp = new Date(timeBase.getTime() + timeOffset)
        
        mockLogs.push({
          level,
          timestamp: timestamp.toISOString(),
          message: getRandomLogMessage(level, i)
        })
      }
      
      return mockLogs.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp))
    }
    
    // 获取随机日志消息
    const getRandomLogMessage = (level, index) => {
      const messages = {
        info: [
          `工作流实例 ${instance.value?.instance_id} 启动`,
          `步骤 ${index} 开始执行`,
          `加载配置文件成功`,
          `工作流环境初始化完成`,
          `步骤 ${index} 执行完成，耗时: ${Math.round(Math.random() * 1000)}ms`
        ],
        warning: [
          `步骤 ${index} 执行时间超过预期`,
          `系统资源使用率高`,
          `API响应时间较长`,
          `缓存命中率低`,
          `步骤 ${index} 重试执行`
        ],
        error: [
          `步骤 ${index} 执行失败: 连接超时`,
          `无法访问外部服务`,
          `数据验证失败: 格式错误`,
          `表达式解析错误: 语法无效`,
          `步骤 ${index} 抛出异常: 未处理的错误`
        ]
      }
      
      const options = messages[level] || messages.info
      return options[Math.floor(Math.random() * options.length)]
    }
    
    // 获取日志级别颜色类型
    const getLogLevelType = (level) => {
      const types = {
        info: 'info',
        warning: 'warning',
        error: 'danger',
        debug: '',
        trace: ''
      }
      return types[level] || ''
    }
    
    // 前往工作流调试页面
    const goToWorkflowDebug = () => {
      if (instance.value && instance.value.workflow && instance.value.workflow.id) {
        router.push(`/workflow/${instance.value.workflow.id}/debug`)
      }
    }
    
    // 处理指标更新
    const handleMetricsUpdate = (data) => {
      console.log('指标更新:', data)
      // 这里可以处理来自实时监控组件的数据
    }
    
    // 监听实例变化，更新日志
    watch(() => instance.value, (newInstance) => {
      if (newInstance) {
        refreshLogs()
      }
    })
    
    onMounted(() => {
      fetchInstance()
    })
    
    return {
      instance,
      isLoading,
      stepsCount,
      cancelDialogVisible,
      activeTab,
      logLevel,
      logs,
      isActiveInstance,
      failedStep,
      filteredLogs,
      isPausing,
      isResuming,
      isCancelling,
      isRetrying,
      isRefreshingInstance,
      isRefreshingLogs,
      refreshInstance,
      pauseInstance,
      resumeInstance,
      confirmCancel,
      cancelInstance,
      retryStep,
      skipFailedStep,
      formatDate,
      getStatusType,
      getStatusText,
      getStepStatusType,
      getStepStatusText,
      getStepTypeText,
      getStepIconType,
      getStepIconColor,
      refreshLogs,
      downloadLogs,
      getLogLevelType,
      goToWorkflowDebug,
      handleMetricsUpdate,
      cmJsonExtensions // Expose extensions to template
    }
  }
}
</script>

<style scoped>
.instance-detail {
  padding: 20px;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.title-area {
  display: flex;
  align-items: center;
}

.title-area h1 {
  margin-right: 15px;
  margin-bottom: 0;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.card-header h3 {
  margin: 0;
}

.instance-info, .steps-card, .context-card, .output-card {
  margin-bottom: 20px;
}

.detail-tabs {
  margin-top: 20px;
}

.step-card {
  margin-bottom: 10px;
}

.step-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.step-name {
  font-weight: bold;
}

.step-content {
  padding: 10px 0;
}

.step-params, .step-output, .step-error {
  margin-top: 15px;
}

.step-actions {
  margin-top: 15px;
  display: flex;
  justify-content: flex-end;
}

.code-container {
  background-color: #f5f7fa;
  border-radius: 4px;
  padding: 10px;
  overflow-x: auto;
}

.code-container pre {
  margin: 0;
  white-space: pre-wrap;
}

/* Style Codemirror container */
.code-container :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
}

.code-container :deep(.cm-scroller) {
  overflow: auto; /* Ensure scrollbar appears when maxHeight is reached */
}

/* Remove default pre styling if needed */
.code-container pre {
  display: none; /* Hide original pre if replaced by Codemirror */
}

.step-error .el-alert {
  margin-top: 5px;
}

/* 让卡片内部的描述列表填充完整 */
:deep(.el-descriptions) {
  width: 100%;
}

.logs-card {
  margin-bottom: 20px;
}

.logs-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 15px;
}

.logs-header h3 {
  margin: 0;
}

.logs-actions {
  display: flex;
  gap: 10px;
  align-items: center;
}

.logs-content {
  max-height: 500px;
  overflow-y: auto;
  border: 1px solid #dcdfe6;
  border-radius: 4px;
  background-color: #f8f8f8;
  font-family: monospace;
  font-size: 12px;
}

.log-entry {
  display: flex;
  padding: 10px 12px;
  border-bottom: 1px solid #ebeef5;
  border-left: 4px solid transparent; /* Add space for color bar */
}

.log-entry:last-child {
  border-bottom: none;
}

.log-timestamp {
  width: 160px;
  color: #909399;
  flex-shrink: 0;
  margin-right: 10px; /* Add space after timestamp */
}

.log-level {
  width: 80px;
  flex-shrink: 0;
  text-align: center;
  margin-right: 10px; /* Add space after level tag */
}

.log-message {
  flex: 1;
  word-break: break-word;
}

.log-info {
  border-left-color: #909399; /* Grey color for info */
}

.log-warning {
  border-left-color: #E6A23C; /* Yellow color for warning */
}

.log-error {
  border-left-color: #F56C6C; /* Red color for error */
}

.no-logs {
  padding: 40px 0;
}
</style> 