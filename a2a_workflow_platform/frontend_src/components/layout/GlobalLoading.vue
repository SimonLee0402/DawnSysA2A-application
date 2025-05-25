<template>
  <div v-if="isVisible" class="global-loading-container">
    <div class="loading-overlay" :class="{ 'with-blur': withBlur }"></div>
    <div class="loading-content">
      <div class="loading-spinner">
        <el-progress 
          type="dashboard" 
          :percentage="progressPercentage" 
          :status="loadingStatus"
          :stroke-width="8"
          :width="120"
        />
      </div>
      <div class="loading-message">
        <h3>{{ currentMessage }}</h3>
        <p v-if="subMessage">{{ subMessage }}</p>
        <div v-if="showOperationDetails" class="operation-details">
          <div v-if="operationName" class="operation-name">
            <el-tag size="small">{{ operationName }}</el-tag>
          </div>
          <div v-if="errorMessage" class="error-message">
            {{ errorMessage }}
          </div>
        </div>
      </div>
      <div v-if="isCancellable" class="loading-actions">
        <el-button type="danger" size="small" @click="handleCancel">
          <el-icon><close /></el-icon> 取消操作
        </el-button>
      </div>
    </div>
  </div>
</template>

<script>
import { computed, ref, watch } from 'vue'
import { useStore } from '@/store/loading'
import { Close } from '@element-plus/icons-vue'
import { ElMessageBox } from 'element-plus'

export default {
  name: 'GlobalLoading',
  components: {
    Close
  },
  setup() {
    // 获取加载状态存储
    const store = useStore()
    
    // 属性
    const isVisible = computed(() => store.isLoading)
    const currentMessage = computed(() => store.message || '加载中...')
    const subMessage = computed(() => store.subMessage || '')
    const withBlur = computed(() => store.withBlur !== false)
    const isCancellable = computed(() => store.isCancellable === true)
    const operationName = computed(() => store.operationName || '')
    const operationId = computed(() => store.operationId || '')
    const errorMessage = computed(() => store.errorMessage || '')
    const progressPercentage = computed(() => store.progress || 0)
    const loadingStatus = computed(() => store.status || '')
    const showOperationDetails = computed(() => operationName.value || errorMessage.value)
    
    // 计时器
    let loadingTimer = null
    let dotsTimer = null
    
    // 动态消息处理
    const processingDots = ref('...')
    
    // 监听加载状态变化
    watch(isVisible, (newValue) => {
      if (newValue) {
        startTimers()
      } else {
        stopTimers()
      }
    })
    
    // 开始计时器
    const startTimers = () => {
      stopTimers()
      
      // 消息动态省略号
      dotsTimer = setInterval(() => {
        processingDots.value = processingDots.value.length >= 3 
          ? '.' 
          : processingDots.value + '.'
      }, 500)
      
      // 自动更新进度的逻辑（仅对未指定具体进度的情况）
      if (!store.progress) {
        let progress = 0
        loadingTimer = setInterval(() => {
          // 模拟进度：快速到达80%，然后缓慢增加
          if (progress < 80) {
            progress += Math.random() * 5
          } else if (progress < 95) {
            progress += Math.random() * 0.5
          }
          
          if (progress > 95) progress = 95
          
          // 更新进度
          store.setProgress(Math.round(progress))
        }, 800)
      }
    }
    
    // 停止计时器
    const stopTimers = () => {
      if (dotsTimer) {
        clearInterval(dotsTimer)
        dotsTimer = null
      }
      
      if (loadingTimer) {
        clearInterval(loadingTimer)
        loadingTimer = null
      }
    }
    
    // 处理取消操作
    const handleCancel = async () => {
      try {
        // 显示确认对话框
        await ElMessageBox.confirm(
          '确定要取消当前操作吗？这可能会导致数据不完整或操作失败。',
          '取消操作',
          {
            confirmButtonText: '确定',
            cancelButtonText: '继续等待',
            type: 'warning'
          }
        )
        
        // 调用取消操作
        store.cancelOperation(operationId.value)
      } catch (e) {
        // 用户取消了操作
        console.log('用户继续等待操作完成')
      }
    }
    
    return {
      isVisible,
      currentMessage,
      subMessage,
      withBlur,
      isCancellable,
      operationName,
      errorMessage,
      progressPercentage,
      loadingStatus,
      showOperationDetails,
      handleCancel
    }
  }
}
</script>

<style scoped>
.global-loading-container {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  display: flex;
  justify-content: center;
  align-items: center;
  z-index: 9999;
}

.loading-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: rgba(0, 0, 0, 0.5);
  z-index: -1;
}

.loading-overlay.with-blur {
  backdrop-filter: blur(4px);
}

.loading-content {
  background-color: white;
  padding: 30px;
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.1);
  display: flex;
  flex-direction: column;
  align-items: center;
  max-width: 90%;
  width: 400px;
}

.loading-spinner {
  margin-bottom: 20px;
}

.loading-message {
  text-align: center;
  margin-bottom: 20px;
}

.loading-message h3 {
  margin: 0 0 10px 0;
  font-size: 18px;
  color: #303133;
}

.loading-message p {
  margin: 0;
  color: #606266;
  font-size: 14px;
}

.loading-actions {
  margin-top: 20px;
}

.operation-details {
  margin-top: 15px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}

.operation-name {
  font-size: 14px;
}

.error-message {
  color: #f56c6c;
  font-size: 14px;
  max-width: 100%;
  overflow-wrap: break-word;
}
</style> 