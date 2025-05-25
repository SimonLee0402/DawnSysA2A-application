<template>
  <div class="test-logs">
    <div class="logs-header">
      <h3>测试日志</h3>
      <div class="logs-actions">
        <el-switch
          v-model="autoScroll"
          active-text="自动滚动"
          inactive-text=""
        ></el-switch>
        <el-button size="small" @click="clearLogs" type="danger" plain>
          清除日志
        </el-button>
      </div>
    </div>
    
    <div class="logs-content" ref="logsContainer">
      <el-empty v-if="logs.length === 0" description="暂无日志"></el-empty>
      <div v-else>
        <div v-for="(log, index) in logs" :key="index" class="log-entry" :class="getLogClass(log.level)">
          <span class="log-time">{{ formatLogTime(log.timestamp) }}</span>
          <span class="log-level" :class="getLogClass(log.level)">{{ log.level }}</span>
          <span class="log-message">{{ log.message }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, defineComponent, onMounted, watch, nextTick } from 'vue'

export default {
  name: 'InteroperabilityTestLogs',
  
  props: {
    testId: {
      type: String,
      default: null
    }
  },
  
  emits: ['log'],
  
  setup(props, { emit }) {
    const logs = ref([])
    const autoScroll = ref(true)
    const logsContainer = ref(null)
    
    // 添加日志
    const addLog = (level, message) => {
      const log = {
        timestamp: new Date(),
        level,
        message
      }
      
      logs.value.push(log)
      emit('log', log)
      
      // 最多保留1000条日志
      if (logs.value.length > 1000) {
        logs.value.shift()
      }
      
      // 自动滚动到底部
      if (autoScroll.value) {
        nextTick(() => {
          if (logsContainer.value) {
            logsContainer.value.scrollTop = logsContainer.value.scrollHeight
          }
        })
      }
    }
    
    // 清除日志
    const clearLogs = () => {
      logs.value = []
    }
    
    // 格式化日志时间
    const formatLogTime = (timestamp) => {
      if (!timestamp) return ''
      
      const date = new Date(timestamp)
      const hours = date.getHours().toString().padStart(2, '0')
      const minutes = date.getMinutes().toString().padStart(2, '0')
      const seconds = date.getSeconds().toString().padStart(2, '0')
      const milliseconds = date.getMilliseconds().toString().padStart(3, '0')
      
      return `${hours}:${minutes}:${seconds}.${milliseconds}`
    }
    
    // 获取日志级别对应的样式类
    const getLogClass = (level) => {
      switch (level.toLowerCase()) {
        case 'info':
          return 'log-info'
        case 'warn':
          return 'log-warn'
        case 'error':
          return 'log-error'
        case 'success':
          return 'log-success'
        default:
          return ''
      }
    }
    
    // 提供对外暴露的方法
    const log = (message) => {
      addLog('info', message)
    }
    
    const warn = (message) => {
      addLog('warn', message)
    }
    
    const error = (message) => {
      addLog('error', message)
    }
    
    const success = (message) => {
      addLog('success', message)
    }
    
    // 测试ID变化时清除日志
    watch(() => props.testId, (newId) => {
      if (newId) {
        clearLogs()
        log(`开始测试 ID: ${newId}`)
      }
    })
    
    return {
      logs,
      autoScroll,
      logsContainer,
      addLog,
      clearLogs,
      formatLogTime,
      getLogClass,
      log,
      warn,
      error,
      success
    }
  }
}
</script>

<style scoped>
.test-logs {
  width: 100%;
  display: flex;
  flex-direction: column;
  border: 1px solid #dcdfe6;
  border-radius: 4px;
  overflow: hidden;
}

.logs-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.5rem 1rem;
  background-color: #f5f7fa;
  border-bottom: 1px solid #dcdfe6;
}

.logs-header h3 {
  margin: 0;
  font-size: 1rem;
}

.logs-actions {
  display: flex;
  gap: 1rem;
  align-items: center;
}

.logs-content {
  height: 300px;
  overflow-y: auto;
  padding: 0.5rem 1rem;
  background-color: #1e1e1e;
  color: #d4d4d4;
  font-family: monospace;
  font-size: 0.9rem;
}

.log-entry {
  padding: 0.25rem 0;
  white-space: pre-wrap;
  word-break: break-word;
}

.log-time {
  color: #858585;
  margin-right: 0.5rem;
}

.log-level {
  font-weight: bold;
  padding: 0.15rem 0.3rem;
  border-radius: 2px;
  margin-right: 0.5rem;
  text-transform: uppercase;
  font-size: 0.7rem;
}

.log-info {
  color: #42a5f5;
}

.log-warn {
  color: #ffb74d;
}

.log-error {
  color: #ef5350;
}

.log-success {
  color: #66bb6a;
}
</style> 