<template>
  <div class="notification-container">
    <!-- 系统通知列表 -->
    <transition-group name="notification-list" tag="div">
      <div 
        v-for="notification in notifications" 
        :key="notification.id"
        class="notification-item"
        :class="[`notification-${notification.type}`, { 'notification-with-actions': notification.actions }]"
      >
        <div class="notification-icon">
          <el-icon v-if="notification.type === 'success'"><circle-check-filled /></el-icon>
          <el-icon v-else-if="notification.type === 'warning'"><warning-filled /></el-icon>
          <el-icon v-else-if="notification.type === 'error'"><circle-close-filled /></el-icon>
          <el-icon v-else-if="notification.type === 'info'"><info-filled /></el-icon>
        </div>
        <div class="notification-content">
          <div class="notification-title">{{ notification.title }}</div>
          <div v-if="notification.message" class="notification-message">{{ notification.message }}</div>
          <div v-if="notification.actions" class="notification-actions">
            <el-button 
              v-for="action in notification.actions" 
              :key="action.name"
              :type="action.type || 'default'"
              size="small"
              @click="handleAction(notification, action)"
            >
              {{ action.label }}
            </el-button>
          </div>
        </div>
        <div class="notification-close">
          <el-icon @click="closeNotification(notification.id)"><close /></el-icon>
        </div>
      </div>
    </transition-group>
  </div>
</template>

<script>
import { computed, onMounted, onUnmounted } from 'vue'
import { useStore } from '@/store/notification'
import { 
  CircleCheckFilled, 
  WarningFilled, 
  CircleCloseFilled, 
  InfoFilled, 
  Close 
} from '@element-plus/icons-vue'

export default {
  name: 'AppNotification',
  components: {
    CircleCheckFilled,
    WarningFilled,
    CircleCloseFilled,
    InfoFilled,
    Close
  },
  setup() {
    const store = useStore()
    
    // 根据添加时间进行排序，最新的通知显示在顶部
    const notifications = computed(() => {
      return [...store.notifications].sort((a, b) => b.timestamp - a.timestamp)
    })
    
    // 关闭指定通知
    const closeNotification = (id) => {
      store.removeNotification(id)
    }
    
    // 处理通知操作
    const handleAction = (notification, action) => {
      if (action.handler && typeof action.handler === 'function') {
        action.handler(notification)
      }
      
      // 如果设置了自动关闭，执行操作后关闭通知
      if (action.closeOnClick) {
        closeNotification(notification.id)
      }
    }
    
    // 自动清理超时的通知
    let cleanupInterval = null
    
    const setupAutoCleanup = () => {
      cleanupInterval = setInterval(() => {
        const now = Date.now()
        store.notifications.forEach(notification => {
          if (notification.autoClose && notification.timestamp + notification.duration < now) {
            closeNotification(notification.id)
          }
        })
      }, 1000)
    }
    
    onMounted(() => {
      setupAutoCleanup()
    })
    
    onUnmounted(() => {
      if (cleanupInterval) {
        clearInterval(cleanupInterval)
      }
    })
    
    return {
      notifications,
      closeNotification,
      handleAction
    }
  }
}
</script>

<style scoped>
.notification-container {
  position: fixed;
  right: 20px;
  top: 20px;
  z-index: 9999;
  width: 350px;
  max-width: 100%;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.notification-item {
  display: flex;
  padding: 12px 15px;
  border-radius: 4px;
  box-shadow: 0 2px 12px 0 rgba(0, 0, 0, 0.1);
  background-color: #ffffff;
  transition: all 0.3s ease;
  opacity: 1;
  transform: translateX(0);
}

.notification-success {
  border-left: 4px solid #67c23a;
}

.notification-warning {
  border-left: 4px solid #e6a23c;
}

.notification-error {
  border-left: 4px solid #f56c6c;
}

.notification-info {
  border-left: 4px solid #909399;
}

.notification-icon {
  margin-right: 12px;
  font-size: 20px;
  display: flex;
  align-items: flex-start;
}

.notification-success .notification-icon {
  color: #67c23a;
}

.notification-warning .notification-icon {
  color: #e6a23c;
}

.notification-error .notification-icon {
  color: #f56c6c;
}

.notification-info .notification-icon {
  color: #909399;
}

.notification-content {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.notification-title {
  font-size: 14px;
  font-weight: bold;
  margin-bottom: 5px;
}

.notification-message {
  font-size: 13px;
  color: #606266;
  margin-bottom: 5px;
}

.notification-actions {
  display: flex;
  gap: 5px;
  margin-top: 5px;
}

.notification-close {
  margin-left: 12px;
  cursor: pointer;
  font-size: 16px;
  color: #909399;
  display: flex;
  align-items: flex-start;
}

.notification-close:hover {
  color: #606266;
}

.notification-with-actions {
  padding-bottom: 8px;
}

/* 过渡动画 */
.notification-list-enter-active,
.notification-list-leave-active {
  transition: all 0.3s ease;
}

.notification-list-enter-from {
  opacity: 0;
  transform: translateX(30px);
}

.notification-list-leave-to {
  opacity: 0;
  transform: translateY(-30px);
}
</style> 