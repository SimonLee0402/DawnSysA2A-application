<template>
  <div class="workflow-monitor">
    <el-card class="monitor-card" v-loading="parentLoading">
      <template #header>
        <div class="card-header">
          <h3>{{ title }}</h3>
          <div class="header-actions">
            <el-switch
              v-model="autoRefresh"
              active-text="自动刷新"
              inactive-text="手动刷新"
              :disabled="parentLoading"
            />
            <el-button 
              type="primary" 
              link 
              @click="refreshData" 
              :disabled="autoRefresh || parentLoading"
              :loading="isManualRefreshing"
            >
              <el-icon><refresh /></el-icon> 刷新
            </el-button>
          </div>
        </div>
      </template>
      
      <div v-if="instance">
        <!-- 状态概览 -->
        <div class="status-overview">
          <div class="status-item">
            <div class="status-label">状态</div>
            <div class="status-value">
              <el-tag :type="getStatusType(instance.status)" size="large">
                {{ getStatusText(instance.status) }}
              </el-tag>
            </div>
          </div>
          
          <div class="status-item">
            <div class="status-label">进度</div>
            <div class="status-value progress-bar">
              <el-progress 
                :percentage="progress" 
                :status="getProgressStatus(instance.status)"
              />
            </div>
          </div>
          
          <div class="status-item">
            <div class="status-label">运行时间</div>
            <div class="status-value">{{ runningTime }}</div>
          </div>
          
          <div class="status-item">
            <div class="status-label">步骤</div>
            <div class="status-value">{{ instance.current_step_index }} / {{ totalSteps }}</div>
          </div>
        </div>
        
        <!-- 当前步骤卡片 -->
        <div v-if="currentStep" class="current-step-card">
          <h4>当前步骤: {{ currentStep.step_name }}</h4>
          <div class="step-info">
            <p><strong>类型:</strong> {{ getStepTypeText(currentStep.step_type) }}</p>
            <p v-if="currentStep.started_at"><strong>开始时间:</strong> {{ formatDate(currentStep.started_at) }}</p>
            <el-tag :type="getStepStatusType(currentStep.status)">
              {{ getStepStatusText(currentStep.status) }}
            </el-tag>
          </div>
        </div>
        
        <!-- 步骤流程可视化 -->
        <div class="steps-visualization">
          <h4>工作流步骤</h4>
          <div class="steps-flow">
            <div 
              v-for="(step, index) in instance.steps" 
              :key="step.id"
              class="step-node"
              :class="{
                'step-completed': step.status === 'completed',
                'step-running': step.status === 'running',
                'step-failed': step.status === 'failed',
                'step-pending': step.status === 'pending',
                'step-skipped': step.status === 'skipped',
                'step-current': index === instance.current_step_index
              }"
              @click="showStepDetails(step)"
            >
              <el-tooltip 
                :content="step.step_name" 
                placement="top" 
                :show-after="500"
              >
                <div class="step-node-inner">
                  <span class="step-index">{{ index + 1 }}</span>
                  <el-icon v-if="step.status === 'completed'"><check /></el-icon>
                  <el-icon v-else-if="step.status === 'running'"><loading /></el-icon>
                  <el-icon v-else-if="step.status === 'failed'"><close /></el-icon>
                  <el-icon v-else-if="step.status === 'skipped'"><right /></el-icon>
                </div>
              </el-tooltip>
              <div class="step-connector" v-if="index < instance.steps.length - 1"></div>
            </div>
          </div>
        </div>
        
        <!-- 最近日志 -->
        <div v-if="instance.logs && instance.logs.length > 0" class="recent-logs">
          <h4>最近日志</h4>
          <el-table :data="recentLogs" size="small" stripe>
            <el-table-column prop="timestamp" label="时间" width="180">
              <template #default="{ row }">
                {{ formatDate(row.timestamp) }}
              </template>
            </el-table-column>
            <el-table-column prop="level" label="级别" width="100">
              <template #default="{ row }">
                <el-tag 
                  :type="getLogLevelType(row.level)" 
                  size="small"
                >
                  {{ row.level }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column prop="message" label="消息" />
          </el-table>
        </div>
      </div>
      
      <el-empty v-else description="暂无监控数据" />
    </el-card>
    
    <!-- 步骤详情对话框 -->
    <el-dialog
      v-model="stepDetailsVisible"
      :title="selectedStep ? '步骤详情: ' + selectedStep.step_name : '步骤详情'"
      width="50%"
    >
      <div v-if="selectedStep" class="step-details">
        <el-descriptions :column="2" border>
          <el-descriptions-item label="步骤ID">{{ selectedStep.id }}</el-descriptions-item>
          <el-descriptions-item label="类型">{{ getStepTypeText(selectedStep.step_type) }}</el-descriptions-item>
          <el-descriptions-item label="状态">
            <el-tag :type="getStepStatusType(selectedStep.status)">
              {{ getStepStatusText(selectedStep.status) }}
            </el-tag>
          </el-descriptions-item>
          <el-descriptions-item label="索引">{{ selectedStep.step_index }}</el-descriptions-item>
          <el-descriptions-item label="开始时间">
            {{ selectedStep.started_at ? formatDate(selectedStep.started_at) : '未开始' }}
          </el-descriptions-item>
          <el-descriptions-item label="完成时间">
            {{ selectedStep.completed_at ? formatDate(selectedStep.completed_at) : '未完成' }}
          </el-descriptions-item>
        </el-descriptions>
        
        <div v-if="selectedStep.parameters" class="step-params">
          <h4>参数</h4>
          <codemirror
            :model-value="JSON.stringify(selectedStep.parameters, null, 2)"
            disabled
            :style="{ height: 'auto', maxHeight: '300px' }"
            :extensions="cmJsonExtensions"
          />
        </div>
        
        <div v-if="selectedStep.output_data" class="step-output">
          <h4>输出</h4>
          <codemirror
            :model-value="JSON.stringify(selectedStep.output_data, null, 2)"
            disabled
            :style="{ height: 'auto', maxHeight: '300px' }"
            :extensions="cmJsonExtensions"
          />
        </div>
        
        <div v-if="selectedStep.error" class="step-error">
          <h4>错误信息</h4>
          <el-alert type="error" :closable="false">
            {{ selectedStep.error }}
          </el-alert>
        </div>
        
        <div v-if="selectedStep.status === 'failed'" class="step-actions">
          <el-button 
            type="primary" 
            @click="retryStep"
            :loading="isRetryingStep"
          >
            <el-icon><refresh-right /></el-icon> 重试此步骤
          </el-button>
        </div>
      </div>
    </el-dialog>
  </div>
</template>

<script>
import { ref, computed, watch, onMounted, onUnmounted, shallowRef } from 'vue'
import { 
  Check, 
  Close, 
  Loading, 
  Right, 
  Refresh,
  RefreshRight 
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'

// Codemirror imports
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'

export default {
  name: 'WorkflowMonitor',
  components: {
    Check,
    Close,
    Loading,
    Right,
    Refresh,
    RefreshRight,
    Codemirror
  },
  props: {
    instance: {
      type: Object,
      required: false,
      default: null
    },
    title: {
      type: String,
      default: '工作流监控'
    },
    showLogs: {
      type: Boolean,
      default: true
    },
    autoRefreshDefault: {
      type: Boolean,
      default: true
    },
    refreshInterval: {
      type: Number,
      default: 5000  // 5秒
    },
    parentLoading: {
      type: Boolean,
      default: false
    }
  },
  emits: ['refresh', 'retry-step'],
  setup(props, { emit }) {
    const isManualRefreshing = ref(false)
    const isRetryingStep = ref(false)
    const autoRefresh = ref(props.autoRefreshDefault)
    let refreshTimer = null
    
    // 步骤详情对话框
    const stepDetailsVisible = ref(false)
    const selectedStep = ref(null)

    // Codemirror JSON Extensions
    const cmJsonExtensions = shallowRef([json(), oneDark, EditorView.lineWrapping, EditorView.editable.of(false)]);
    
    // 计算属性
    const totalSteps = computed(() => {
      return props.instance?.steps?.length || 0
    })
    
    const currentStep = computed(() => {
      if (!props.instance || !props.instance.steps) return null
      
      const index = props.instance.current_step_index
      return props.instance.steps.find(s => s.step_index === index) || null
    })
    
    const progress = computed(() => {
      if (!props.instance || !props.instance.steps || props.instance.steps.length === 0) {
        return 0
      }
      
      const totalStepsCount = props.instance.steps.length
      const completedSteps = props.instance.steps.filter(s => 
        ['completed', 'skipped'].includes(s.status)
      ).length
      
      if (props.instance.status === 'completed') {
        return 100
      }
      
      if (props.instance.status === 'failed') {
        return Math.round((completedSteps / totalStepsCount) * 100)
      }
      
      const currentStepValue = props.instance.status === 'running' ? 0.5 : 0
      return Math.round(((completedSteps + currentStepValue) / totalStepsCount) * 100)
    })
    
    const runningTime = computed(() => {
      if (!props.instance) return '00:00:00'
      
      const startTime = props.instance.started_at ? new Date(props.instance.started_at) : null
      const endTime = props.instance.completed_at ? new Date(props.instance.completed_at) : new Date()
      
      if (!startTime) return '00:00:00'
      
      const diffMs = endTime - startTime
      const diffSec = Math.floor(diffMs / 1000)
      const hours = Math.floor(diffSec / 3600)
      const minutes = Math.floor((diffSec % 3600) / 60)
      const seconds = diffSec % 60
      
      return [
        hours.toString().padStart(2, '0'),
        minutes.toString().padStart(2, '0'),
        seconds.toString().padStart(2, '0')
      ].join(':')
    })
    
    const recentLogs = computed(() => {
      if (!props.instance?.logs) return []
      
      // 获取最近10条日志，按时间倒序排列
      return [...props.instance.logs]
        .sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp))
        .slice(0, 10)
    })
    
    // 方法
    const refreshData = () => {
      if (props.parentLoading) return;
      isManualRefreshing.value = true;
      emit('refresh');
    }
    
    const setupAutoRefresh = () => {
      clearAutoRefresh()
      
      if (autoRefresh.value) {
        refreshTimer = setInterval(() => {
          // 如果实例处于终态，停止刷新
          if (props.instance && ['completed', 'failed', 'canceled'].includes(props.instance.status)) {
            clearAutoRefresh()
            autoRefresh.value = false
            return
          }
          
          refreshData()
        }, props.refreshInterval)
      }
    }
    
    const clearAutoRefresh = () => {
      if (refreshTimer) {
        clearInterval(refreshTimer)
        refreshTimer = null
      }
    }
    
    const showStepDetails = (step) => {
      selectedStep.value = step
      stepDetailsVisible.value = true
    }
    
    const retryStep = () => {
      if (!selectedStep.value) return
      
      isRetryingStep.value = true
      emit('retry-step', selectedStep.value.id)
      
      // 简单延时后关闭加载状态，实际中更好的方式应该是由父组件通知操作完成
      setTimeout(() => {
        isRetryingStep.value = false
      stepDetailsVisible.value = false
      }, 500)
    }
    
    // 格式化和显示辅助函数
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
    
    const getProgressStatus = (status) => {
      if (status === 'completed') return 'success'
      if (status === 'failed') return 'exception'
      if (status === 'paused') return 'warning'
      return ''
    }
    
    const getLogLevelType = (level) => {
      const levelMap = {
        'info': 'info',
        'warning': 'warning',
        'error': 'danger',
        'debug': ''
      }
      return levelMap[level] || ''
    }
    
    // 监听自动刷新开关变化
    watch(autoRefresh, (newValue) => {
      if (newValue) {
        setupAutoRefresh()
      } else {
        clearAutoRefresh()
      }
    })
    
    // 监听实例变化，如果状态为终态，关闭自动刷新
    watch(() => props.instance?.status, (newStatus) => {
      if (newStatus && ['completed', 'failed', 'canceled'].includes(newStatus)) {
        clearAutoRefresh()
        autoRefresh.value = false
      }
    })
    
    // Watch parentLoading to reset manual refresh button state
    watch(() => props.parentLoading, (newLoadingState) => {
      if (!newLoadingState) {
        isManualRefreshing.value = false;
      }
    });
    
    // 生命周期钩子
    onMounted(() => {
      if (autoRefresh.value) {
        setupAutoRefresh()
      }
    })
    
    onUnmounted(() => {
      clearAutoRefresh()
    })
    
    return {
      isManualRefreshing,
      isRetryingStep,
      autoRefresh,
      refreshData,
      totalSteps,
      currentStep,
      progress,
      runningTime,
      recentLogs,
      stepDetailsVisible,
      selectedStep,
      showStepDetails,
      retryStep,
      formatDate,
      getStatusType,
      getStatusText,
      getStepStatusType,
      getStepStatusText,
      getStepTypeText,
      getProgressStatus,
      getLogLevelType,
      cmJsonExtensions
    }
  }
}
</script>

<style scoped>
.workflow-monitor {
  margin-bottom: 20px;
}

.monitor-card {
  margin-bottom: 20px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.card-header h3 {
  margin: 0;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.status-overview {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 15px;
  margin-bottom: 20px;
}

.status-item {
  background-color: #f5f7fa;
  border-radius: 4px;
  padding: 15px;
}

.status-label {
  font-size: 14px;
  color: #909399;
  margin-bottom: 8px;
}

.status-value {
  font-size: 16px;
  font-weight: bold;
}

.progress-bar {
  padding-top: 10px;
}

.current-step-card {
  background-color: #ecf5ff;
  border-radius: 4px;
  padding: 15px;
  margin-bottom: 20px;
}

.current-step-card h4 {
  margin-top: 0;
  margin-bottom: 10px;
}

.step-info {
  display: flex;
  align-items: center;
  gap: 15px;
}

.step-info p {
  margin: 0;
}

.steps-visualization {
  margin-bottom: 20px;
}

.steps-visualization h4 {
  margin-bottom: 15px;
}

.steps-flow {
  display: flex;
  align-items: center;
  overflow-x: auto;
  padding: 10px 0;
}

.step-node {
  position: relative;
  display: flex;
  align-items: center;
}

.step-node-inner {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  background-color: #f5f7fa;
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
  cursor: pointer;
  transition: all 0.3s;
  z-index: 1;
}

.step-node-inner .step-index {
  position: absolute;
  top: -8px;
  right: -8px;
  background-color: #909399;
  color: #fff;
  border-radius: 50%;
  width: 18px;
  height: 18px;
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.step-connector {
  height: 2px;
  background-color: #e4e7ed;
  width: 40px;
}

.step-completed .step-node-inner {
  background-color: #67c23a;
  color: #fff;
}

.step-running .step-node-inner {
  background-color: #409eff;
  color: #fff;
  animation: pulse 1.5s infinite;
}

.step-failed .step-node-inner {
  background-color: #f56c6c;
  color: #fff;
}

.step-pending .step-node-inner {
  background-color: #e4e7ed;
  color: #909399;
}

.step-skipped .step-node-inner {
  background-color: #e6a23c;
  color: #fff;
}

.step-current .step-node-inner {
  border: 2px solid #409eff;
  transform: scale(1.1);
}

.recent-logs {
  margin-top: 20px;
}

.recent-logs h4 {
  margin-bottom: 10px;
}

.step-details {
  padding: 10px;
}

.step-params, .step-output, .step-error {
  margin-top: 20px;
}

.step-params h4, .step-output h4, .step-error h4 {
  margin-bottom: 10px;
}

/* Codemirror styles */
.step-params :deep(.cm-editor),
.step-output :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
  background-color: #f5f7fa;
}

.step-params :deep(.cm-scroller),
.step-output :deep(.cm-scroller) {
  overflow: auto; /* Ensure scrollbar appears when maxHeight is reached */
  padding: 10px;
}

/* Hide original pre when replaced by Codemirror */
.step-params pre, 
.step-output pre {
  display: none;
}

.step-actions {
  margin-top: 20px;
  display: flex;
  justify-content: flex-end;
}

@keyframes pulse {
  0% {
    box-shadow: 0 0 0 0 rgba(64, 158, 255, 0.7);
  }
  70% {
    box-shadow: 0 0 0 10px rgba(64, 158, 255, 0);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(64, 158, 255, 0);
  }
}
</style> 