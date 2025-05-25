<template>
  <div class="workflow-error-handler">
    <el-collapse v-if="errorData" v-model="activeNames">
      <el-collapse-item name="basic">
        <template #title>
          <div class="error-title">
            <el-icon class="error-icon"><warning /></el-icon>
            <span>{{ errorData.message || '工作流执行错误' }}</span>
          </div>
        </template>
        
        <div class="error-details">
          <el-descriptions :column="1" border>
            <el-descriptions-item label="错误类型">{{ errorData.type || '未知类型' }}</el-descriptions-item>
            <el-descriptions-item label="步骤名称" v-if="errorData.stepName">{{ errorData.stepName }}</el-descriptions-item>
            <el-descriptions-item label="错误时间" v-if="errorData.timestamp">{{ formatDate(errorData.timestamp) }}</el-descriptions-item>
          </el-descriptions>
          
          <div class="error-message" v-if="errorData.details">
            <h4>详细信息</h4>
            <codemirror
              :model-value="errorData.details"
              disabled
              :style="{ height: 'auto', maxHeight: '300px' }"
              :extensions="cmJsonExtensions"
            />
          </div>
        </div>
      </el-collapse-item>
      
      <el-collapse-item name="troubleshooting" v-if="showTroubleshooting">
        <template #title>
          <div class="error-title">
            <el-icon class="solution-icon"><magic-stick /></el-icon>
            <span>故障排除建议</span>
          </div>
        </template>
        
        <div class="troubleshooting-tips">
          <el-alert
            v-for="(tip, index) in troubleshootingTips"
            :key="index"
            :title="tip.title"
            :description="tip.description"
            type="info"
            show-icon
            :closable="false"
            class="tip-item"
          />
        </div>
      </el-collapse-item>
      
      <el-collapse-item name="actions">
        <template #title>
          <div class="error-title">
            <el-icon class="action-icon"><operation /></el-icon>
            <span>可能的操作</span>
          </div>
        </template>
        
        <div class="error-actions">
          <el-button 
            v-for="action in availableActions" 
            :key="action.key"
            :type="action.type || 'primary'"
            size="small"
            @click="handleAction(action.key)"
            :loading="isLoading[action.key]"
          >
            <el-icon v-if="action.icon"><component :is="action.icon" /></el-icon>
            {{ action.text }}
          </el-button>
        </div>
      </el-collapse-item>
    </el-collapse>
    
    <el-empty v-else description="暂无错误信息" />
  </div>
</template>

<script>
import { ref, computed, onMounted, watch, reactive, shallowRef } from 'vue'
import { Warning, MagicStick, Operation, RefreshRight, View, DocumentCopy } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'

// Codemirror imports
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'

export default {
  name: 'WorkflowErrorHandler',
  components: {
    Warning,
    MagicStick,
    Operation,
    RefreshRight,
    View,
    DocumentCopy,
    Codemirror
  },
  props: {
    error: {
      type: [Object, String],
      default: null
    },
    stepType: {
      type: String,
      default: ''
    },
    stepName: {
      type: String,
      default: ''
    },
    workflowId: {
      type: [String, Number],
      default: null
    },
    instanceId: {
      type: [String, Number],
      default: null
    }
  },
  emits: ['retry', 'view-logs', 'skip', 'debug'],
  setup(props, { emit }) {
    const activeNames = ref(['basic', 'troubleshooting', 'actions'])
    const showTroubleshooting = ref(true)
    // 加载状态管理
    const isLoading = reactive({
      retry: false,
      'view-logs': false,
      skip: false,
      debug: false,
      copy: false
    })

    // Codemirror JSON Extensions
    const cmJsonExtensions = shallowRef([json(), oneDark, EditorView.lineWrapping, EditorView.editable.of(false)]);
    
    // 处理错误数据格式化
    const errorData = computed(() => {
      if (!props.error) return null
      
      // 如果错误是字符串
      if (typeof props.error === 'string') {
        return {
          message: props.error,
          type: '运行时错误',
          stepName: props.stepName,
          timestamp: new Date().toISOString(),
          details: null
        }
      }
      
      // 如果错误是对象
      return {
        message: props.error.message || '未知错误',
        type: props.error.type || getErrorTypeFromMessage(props.error.message) || '运行时错误',
        stepName: props.stepName,
        timestamp: props.error.timestamp || new Date().toISOString(),
        details: props.error.details || props.error.stack || formatErrorDetails(props.error)
      }
    })
    
    // 根据错误消息猜测错误类型
    const getErrorTypeFromMessage = (message) => {
      if (!message) return null
      
      const lowerMessage = message.toLowerCase()
      if (lowerMessage.includes('timeout') || lowerMessage.includes('超时')) {
        return '超时错误'
      } else if (lowerMessage.includes('permission') || lowerMessage.includes('权限')) {
        return '权限错误'
      } else if (lowerMessage.includes('network') || lowerMessage.includes('网络')) {
        return '网络错误'
      } else if (lowerMessage.includes('syntax') || lowerMessage.includes('语法')) {
        return '语法错误'
      } else if (lowerMessage.includes('validation') || lowerMessage.includes('验证')) {
        return '验证错误'
      }
      
      return null
    }
    
    // 格式化错误详情
    const formatErrorDetails = (error) => {
      if (!error) return ''
      
      try {
        if (typeof error === 'string') {
          return error
        } else if (error instanceof Error) {
          return error.stack || error.message
        } else {
          return JSON.stringify(error, null, 2)
        }
      } catch (e) {
        return String(error)
      }
    }
    
    // 根据错误类型和步骤类型生成故障排除建议
    const troubleshootingTips = computed(() => {
      if (!errorData.value) return []
      
      const tips = []
      const errorType = errorData.value.type
      const stepType = props.stepType
      const errorMsg = errorData.value.message.toLowerCase()
      
      // 通用提示
      tips.push({
        title: '检查工作流定义',
        description: '确保工作流定义中的步骤配置正确，参数格式符合要求。'
      })
      
      // 根据错误类型添加特定提示
      if (errorType.includes('超时') || errorMsg.includes('timeout')) {
        tips.push({
          title: '检查超时设置',
          description: '当前操作可能需要更长的执行时间，请考虑增加步骤的超时设置。'
        })
      }
      
      if (errorType.includes('网络') || errorMsg.includes('network') || errorMsg.includes('connection')) {
        tips.push({
          title: '检查网络连接',
          description: '确保系统可以访问所需的网络资源，检查网络连接和代理设置。'
        })
      }
      
      // 根据步骤类型添加特定提示
      if (stepType === 'a2a_client') {
        tips.push({
          title: '检查智能体配置',
          description: '确保智能体存在且配置正确，检查API密钥和访问权限。'
        })
        tips.push({
          title: '查看智能体日志',
          description: '检查智能体的执行日志，可能包含详细的错误信息。'
        })
      } else if (stepType === 'condition') {
        tips.push({
          title: '检查条件表达式',
          description: '确保条件表达式语法正确，变量名称拼写正确，且所有引用的变量都存在于上下文中。'
        })
      } else if (stepType === 'loop') {
        tips.push({
          title: '检查循环条件',
          description: '确保循环条件表达式正确，并设置了恰当的最大迭代次数，避免无限循环。'
        })
      } else if (stepType === 'transform') {
        tips.push({
          title: '检查转换表达式',
          description: '确保转换表达式语法正确，并且所有引用的变量都存在于上下文中。'
        })
      }
      
      return tips
    })
    
    // 可用操作
    const availableActions = computed(() => {
      const actions = [
        { key: 'retry', text: '重试步骤', type: 'primary', icon: 'RefreshRight' },
        { key: 'view-logs', text: '查看完整日志', type: 'info', icon: 'View' },
        { key: 'copy', text: '复制错误信息', type: 'default', icon: 'DocumentCopy' }
      ]
      
      // 如果是循环或条件步骤，添加跳过选项
      if (['condition', 'loop'].includes(props.stepType)) {
        actions.push({ key: 'skip', text: '跳过此步骤', type: 'warning', icon: 'Right' })
      }
      
      // 如果有工作流ID，添加调试选项
      if (props.workflowId) {
        actions.push({ key: 'debug', text: '调试工作流', type: 'success', icon: 'MagicStick' })
      }
      
      return actions
    })
    
    // 处理操作点击
    const handleAction = (actionKey) => {
      isLoading[actionKey] = true
      
      switch (actionKey) {
        case 'retry':
          emit('retry')
          // 让父组件处理加载状态的重置，5秒后自动重置以防止UI卡住
          setTimeout(() => { isLoading.retry = false }, 5000)
          break
        case 'view-logs':
          emit('view-logs')
          isLoading['view-logs'] = false
          break
        case 'skip':
          emit('skip')
          setTimeout(() => { isLoading.skip = false }, 5000)
          break
        case 'debug':
          emit('debug')
          setTimeout(() => { isLoading.debug = false }, 5000)
          break
        case 'copy':
          copyErrorToClipboard()
          break
      }
    }
    
    // 复制错误信息到剪贴板
    const copyErrorToClipboard = () => {
      if (!errorData.value) {
        isLoading.copy = false
        return
      }
      
      const errorText = `错误: ${errorData.value.message}\n` +
        `类型: ${errorData.value.type}\n` +
        `步骤: ${errorData.value.stepName || '未知'}\n` +
        `时间: ${errorData.value.timestamp}\n\n` +
        `详情: ${errorData.value.details || '无详细信息'}`
      
      try {
        navigator.clipboard.writeText(errorText).then(() => {
          ElMessage.success('错误信息已复制到剪贴板')
          isLoading.copy = false
        }).catch(e => {
          console.error('无法复制到剪贴板', e)
          ElMessage.error('复制失败，请手动复制')
          isLoading.copy = false
        })
      } catch (e) {
        console.error('无法复制到剪贴板', e)
        ElMessage.error('复制失败，请手动复制')
        isLoading.copy = false
      }
    }
    
    // 格式化日期
    const formatDate = (dateString) => {
      if (!dateString) return ''
      
      try {
        const date = new Date(dateString)
        return date.toLocaleString('zh-CN', { 
          year: 'numeric',
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit'
        })
      } catch (e) {
        return dateString
      }
    }
    
    // 监听错误变化
    watch(() => props.error, (newValue) => {
      if (newValue) {
        // 当有新错误时，自动展开所有面板
        activeNames.value = ['basic', 'troubleshooting', 'actions']
      }
    })
    
    return {
      activeNames,
      errorData,
      troubleshootingTips,
      showTroubleshooting,
      availableActions,
      handleAction,
      formatDate,
      isLoading,
      cmJsonExtensions
    }
  }
}
</script>

<style scoped>
.workflow-error-handler {
  margin-bottom: 20px;
}

.error-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.error-icon {
  color: #f56c6c;
}

.solution-icon {
  color: #409eff;
}

.action-icon {
  color: #67c23a;
}

.error-details {
  padding: 10px 0;
}

.error-message {
  margin-top: 15px;
}

.error-code {
  background-color: #f8f8f8;
  padding: 12px;
  border-radius: 4px;
  border: 1px solid #dcdfe6;
  font-family: monospace;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 300px;
  overflow: auto;
}

/* Codemirror styles */
.error-message :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
  background-color: #f8f8f8;
  border: 1px solid #dcdfe6;
}

.error-message :deep(.cm-scroller) {
  padding: 8px;
  overflow: auto; /* Ensure scrollbar appears when maxHeight is reached */
  max-height: 300px;
}

.troubleshooting-tips {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.error-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 10px;
}

.tip-item {
  margin-bottom: 10px;
}
</style> 