<template>
  <div class="workflow-realtime-monitor">
    <el-card class="monitor-card" v-loading="isLoading">
      <template #header>
        <div class="card-header">
          <h3>{{ title }}</h3>
          <div class="header-controls">
            <el-switch
              v-model="autoRefresh"
              active-text="实时更新"
              inactive-text="手动刷新"
            />
            <el-select 
              v-model="refreshInterval" 
              placeholder="刷新间隔" 
              size="small"
              :disabled="!autoRefresh"
            >
              <el-option label="1秒" :value="1000" />
              <el-option label="3秒" :value="3000" />
              <el-option label="5秒" :value="5000" />
              <el-option label="10秒" :value="10000" />
            </el-select>
            <el-button 
              type="primary" 
              size="small"
              @click="refreshMetrics" 
              :disabled="autoRefresh">
              <el-icon><refresh /></el-icon> 刷新
            </el-button>
          </div>
        </div>
      </template>
      
      <div v-if="!instanceId" class="no-instance">
        <el-empty description="未选择工作流实例" />
      </div>
      
      <div v-else>
        <!-- 实时状态和性能指标 -->
        <div class="metrics-grid">
          <el-card shadow="hover" class="metric-card">
            <template #header>
              <div class="metric-header">
                <el-icon><timer /></el-icon>
                运行时间
              </div>
            </template>
            <div class="metric-value">{{ metrics.runningTime || '00:00:00' }}</div>
          </el-card>
          
          <el-card shadow="hover" class="metric-card">
            <template #header>
              <div class="metric-header">
                <el-icon><cpu /></el-icon>
                CPU使用率
              </div>
            </template>
            <div class="metric-value">
              <el-progress 
                type="dashboard" 
                :percentage="metrics.cpuUsage || 0" 
                :color="getResourceColor(metrics.cpuUsage)" 
              />
            </div>
          </el-card>
          
          <el-card shadow="hover" class="metric-card">
            <template #header>
              <div class="metric-header">
                <el-icon><connection /></el-icon>
                内存使用
              </div>
            </template>
            <div class="metric-value">
              <el-progress 
                type="dashboard" 
                :percentage="metrics.memoryUsagePercent || 0" 
                :color="getResourceColor(metrics.memoryUsagePercent)" 
              />
              <div class="metric-detail">{{ formatMemory(metrics.memoryUsage) }}</div>
            </div>
          </el-card>
          
          <el-card shadow="hover" class="metric-card">
            <template #header>
              <div class="metric-header">
                <el-icon><document /></el-icon>
                API调用次数
              </div>
            </template>
            <div class="metric-value">{{ metrics.apiCalls || 0 }}</div>
          </el-card>
        </div>
        
        <!-- 步骤执行统计 -->
        <el-divider>步骤执行统计</el-divider>
        <el-table :data="stepMetrics" stripe style="width: 100%" size="small">
          <el-table-column prop="stepName" label="步骤名称" min-width="150" />
          <el-table-column prop="executions" label="执行次数" width="100" align="center" />
          <el-table-column prop="avgDuration" label="平均耗时" width="120" align="center">
            <template #default="{ row }">
              {{ formatDuration(row.avgDuration) }}
            </template>
          </el-table-column>
          <el-table-column prop="status" label="状态" width="100" align="center">
            <template #default="{ row }">
              <el-tag :type="getStatusType(row.status)">{{ getStatusText(row.status) }}</el-tag>
            </template>
          </el-table-column>
          <el-table-column label="耗时分布" min-width="200">
            <template #default="{ row }">
              <div class="duration-chart">
                <div 
                  v-for="(segment, i) in row.durationSegments" 
                  :key="i"
                  class="duration-segment"
                  :style="{ 
                    width: `${segment.percentage}%`, 
                    backgroundColor: getDurationColor(segment.level)
                  }"
                />
              </div>
            </template>
          </el-table-column>
        </el-table>
        
        <!-- 关键事件时间线 -->
        <el-divider>关键事件</el-divider>
        <el-timeline>
          <el-timeline-item
            v-for="event in recentEvents"
            :key="event.id"
            :type="getEventIcon(event.type)"
            :color="getEventColor(event.type)"
            :timestamp="formatDate(event.timestamp)"
            :hollow="event.type === 'info'"
          >
            <div class="event-content">
              <h4>{{ event.title }}</h4>
              <p>{{ event.message }}</p>
              <div v-if="event.details" class="event-details">
                <el-button type="primary" link @click="toggleEventDetails(event.id)">
                  {{ expandedEvents.includes(event.id) ? '收起详情' : '查看详情' }}
                </el-button>
                <div v-if="expandedEvents.includes(event.id)" class="details-content">
                  <pre>{{ event.details }}</pre>
                </div>
              </div>
            </div>
          </el-timeline-item>
        </el-timeline>
      </div>
    </el-card>
  </div>
</template>

<script>
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { 
  Refresh, 
  Timer, 
  Cpu, 
  Connection, 
  Document
} from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import axios from 'axios'

export default {
  name: 'WorkflowRealTimeMonitor',
  components: {
    Refresh,
    Timer,
    Cpu,
    Connection,
    Document
  },
  props: {
    instanceId: {
      type: String,
      default: ''
    },
    title: {
      type: String,
      default: '工作流实时监控'
    },
    autoRefreshDefault: {
      type: Boolean,
      default: true
    },
    defaultRefreshInterval: {
      type: Number,
      default: 5000 // 默认5秒
    }
  },
  emits: ['metrics-updated'],
  setup(props, { emit }) {
    const isLoading = ref(false)
    const autoRefresh = ref(props.autoRefreshDefault)
    const refreshInterval = ref(props.defaultRefreshInterval)
    let refreshTimer = null
    
    // 监控数据
    const metrics = ref({
      runningTime: '00:00:00',
      cpuUsage: 0,
      memoryUsage: 0,
      memoryUsagePercent: 0,
      apiCalls: 0,
      startTime: null,
      lastUpdateTime: null
    })
    
    // 步骤指标数据
    const stepMetrics = ref([])
    
    // 事件数据
    const recentEvents = ref([])
    const expandedEvents = ref([])
    
    // 获取指标数据
    const refreshMetrics = async () => {
      if (!props.instanceId) return
      
      isLoading.value = true
      
      try {
        // 实际中这里应该调用API获取真实数据
        // 目前使用模拟数据用于演示
        await getSimulatedMetrics()
        
        // 发出指标更新事件
        emit('metrics-updated', {
          metrics: metrics.value,
          stepMetrics: stepMetrics.value,
          events: recentEvents.value
        })
      } catch (error) {
        console.error('获取监控指标失败', error)
        ElMessage.error('获取监控数据失败')
      } finally {
        isLoading.value = false
      }
    }
    
    // 模拟获取监控数据（开发阶段使用）
    const getSimulatedMetrics = async () => {
      // 模拟网络延迟
      await new Promise(resolve => setTimeout(resolve, 300))
      
      // 如果是第一次获取数据，初始化开始时间
      if (!metrics.value.startTime) {
        metrics.value.startTime = new Date()
        metrics.value.lastUpdateTime = new Date()
      }
      
      // 更新运行时间
      const now = new Date()
      const diffMs = now - new Date(metrics.value.startTime)
      const hours = Math.floor(diffMs / 3600000)
      const minutes = Math.floor((diffMs % 3600000) / 60000)
      const seconds = Math.floor((diffMs % 60000) / 1000)
      metrics.value.runningTime = `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`
      
      // 更新资源使用情况（随机值，用于演示）
      metrics.value.cpuUsage = Math.min(100, Math.max(0, metrics.value.cpuUsage + (Math.random() * 10 - 5)))
      
      // 内存使用 (MB)
      const prevMemory = metrics.value.memoryUsage || 100
      metrics.value.memoryUsage = Math.max(50, Math.min(1024, prevMemory + (Math.random() * 50 - 10)))
      metrics.value.memoryUsagePercent = Math.round((metrics.value.memoryUsage / 1024) * 100)
      
      // API调用次数（累加）
      metrics.value.apiCalls = (metrics.value.apiCalls || 0) + Math.floor(Math.random() * 3)
      
      // 步骤指标
      if (stepMetrics.value.length === 0) {
        // 首次加载，创建模拟步骤数据
        stepMetrics.value = [
          {
            stepName: '初始化',
            executions: 1,
            avgDuration: 1200,
            status: 'completed',
            durationSegments: [
              { level: 'fast', percentage: 100 }
            ]
          },
          {
            stepName: '数据加载',
            executions: 1,
            avgDuration: 3500,
            status: 'completed',
            durationSegments: [
              { level: 'medium', percentage: 100 }
            ]
          },
          {
            stepName: '智能体处理',
            executions: 5,
            avgDuration: 5200,
            status: 'running',
            durationSegments: [
              { level: 'fast', percentage: 20 },
              { level: 'medium', percentage: 50 },
              { level: 'slow', percentage: 30 }
            ]
          },
          {
            stepName: '结果验证',
            executions: 0,
            avgDuration: 0,
            status: 'pending',
            durationSegments: [
              { level: 'none', percentage: 100 }
            ]
          }
        ]
      } else {
        // 更新现有步骤数据
        stepMetrics.value.forEach(step => {
          if (step.status === 'running') {
            step.executions += Math.random() > 0.7 ? 1 : 0
            step.avgDuration = Math.max(100, Math.min(10000, step.avgDuration + (Math.random() * 500 - 250)))
            
            // 随机更新耗时分布
            if (Math.random() > 0.7) {
              const total = step.durationSegments.reduce((sum, segment) => sum + segment.percentage, 0)
              if (total === 100) {
                // 随机调整两个段的比例
                const idx1 = Math.floor(Math.random() * step.durationSegments.length)
                let idx2 = Math.floor(Math.random() * step.durationSegments.length)
                while (idx2 === idx1) {
                  idx2 = Math.floor(Math.random() * step.durationSegments.length)
                }
                
                const change = Math.min(5, Math.min(step.durationSegments[idx1].percentage, 100 - step.durationSegments[idx2].percentage))
                step.durationSegments[idx1].percentage -= change
                step.durationSegments[idx2].percentage += change
              }
            }
          }
        })
      }
      
      // 事件数据
      if (Math.random() > 0.7 || recentEvents.value.length === 0) {
        // 随机添加新事件
        const eventTypes = ['info', 'warning', 'error', 'success']
        const eventType = eventTypes[Math.floor(Math.random() * eventTypes.length)]
        
        const newEvent = {
          id: Date.now().toString(),
          type: eventType,
          timestamp: new Date().toISOString(),
          title: getRandomEventTitle(eventType),
          message: getRandomEventMessage(eventType),
          details: Math.random() > 0.5 ? getRandomEventDetails(eventType) : null
        }
        
        recentEvents.value.unshift(newEvent)
        
        // 限制事件数量
        if (recentEvents.value.length > 10) {
          recentEvents.value = recentEvents.value.slice(0, 10)
        }
      }
      
      metrics.value.lastUpdateTime = now
    }
    
    // 随机事件标题
    const getRandomEventTitle = (type) => {
      const titles = {
        info: ['步骤开始执行', '配置加载完成', '资源分配成功'],
        warning: ['性能降低', '资源使用率高', '操作耗时过长'],
        error: ['步骤执行失败', 'API调用错误', '资源分配失败'],
        success: ['步骤执行成功', '任务完成', '检查点已保存']
      }
      
      const typeOptions = titles[type] || titles.info
      return typeOptions[Math.floor(Math.random() * typeOptions.length)]
    }
    
    // 随机事件消息
    const getRandomEventMessage = (type) => {
      const messages = {
        info: ['系统正在处理数据', '已成功连接到外部服务', '缓存已更新'],
        warning: ['CPU使用率超过80%', '内存使用接近上限', '操作响应时间增加'],
        error: ['无法连接到API服务', '表达式解析错误', '步骤超时'],
        success: ['数据处理完成，生成结果', '验证通过', '成功保存到数据库']
      }
      
      const typeOptions = messages[type] || messages.info
      return typeOptions[Math.floor(Math.random() * typeOptions.length)]
    }
    
    // 随机事件详情
    const getRandomEventDetails = (type) => {
      if (type === 'error') {
        return `错误代码: ${Math.floor(Math.random() * 1000)}\n堆栈跟踪:\nError: 操作失败\n  at processStep (workflow.js:120)\n  at executeWorkflow (engine.js:85)\n  at async runInstance (controller.js:42)`
      } else if (type === 'warning') {
        return `警告: 资源使用率高\n详情: CPU: ${Math.floor(Math.random() * 100)}%, 内存: ${Math.floor(Math.random() * 100)}%\n建议: 考虑优化当前操作或增加资源配置`
      } else {
        return `{ "status": "success", "timestamp": "${new Date().toISOString()}", "data": { "id": "${Math.random().toString(36).substring(2, 10)}" } }`
      }
    }
    
    // 格式化内存使用
    const formatMemory = (memoryMB) => {
      if (!memoryMB) return '0 MB'
      
      if (memoryMB < 1024) {
        return `${Math.round(memoryMB)} MB`
      } else {
        return `${(memoryMB / 1024).toFixed(2)} GB`
      }
    }
    
    // 格式化持续时间
    const formatDuration = (ms) => {
      if (!ms) return '0 ms'
      
      if (ms < 1000) {
        return `${Math.round(ms)} ms`
      } else if (ms < 60000) {
        return `${(ms / 1000).toFixed(2)} 秒`
      } else {
        const minutes = Math.floor(ms / 60000)
        const seconds = Math.floor((ms % 60000) / 1000)
        return `${minutes}分${seconds}秒`
      }
    }
    
    // 格式化日期
    const formatDate = (dateString) => {
      if (!dateString) return ''
      
      try {
        const date = new Date(dateString)
        return date.toLocaleTimeString('zh-CN', {
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit'
        })
      } catch (e) {
        return dateString
      }
    }
    
    // 获取资源使用颜色
    const getResourceColor = (percentage) => {
      if (percentage <= 60) return '#67c23a'
      if (percentage <= 80) return '#e6a23c'
      return '#f56c6c'
    }
    
    // 获取持续时间颜色
    const getDurationColor = (level) => {
      const colors = {
        none: '#dcdfe6',
        fast: '#67c23a',
        medium: '#e6a23c',
        slow: '#f56c6c'
      }
      return colors[level] || colors.none
    }
    
    // 获取事件图标类型
    const getEventIcon = (type) => {
      const icons = {
        info: 'info',
        warning: 'warning',
        error: 'error',
        success: 'success'
      }
      return icons[type] || 'info'
    }
    
    // 获取事件颜色
    const getEventColor = (type) => {
      const colors = {
        info: '#909399',
        warning: '#e6a23c',
        error: '#f56c6c',
        success: '#67c23a'
      }
      return colors[type] || colors.info
    }
    
    // 切换事件详情显示
    const toggleEventDetails = (eventId) => {
      const index = expandedEvents.value.indexOf(eventId)
      if (index === -1) {
        expandedEvents.value.push(eventId)
      } else {
        expandedEvents.value.splice(index, 1)
      }
    }
    
    // 获取状态类型
    const getStatusType = (status) => {
      const types = {
        pending: 'info',
        running: 'primary',
        completed: 'success',
        failed: 'danger',
        paused: 'warning'
      }
      return types[status] || 'info'
    }
    
    // 获取状态文本
    const getStatusText = (status) => {
      const texts = {
        pending: '等待中',
        running: '运行中',
        completed: '已完成',
        failed: '失败',
        paused: '已暂停'
      }
      return texts[status] || '未知'
    }
    
    // 开始自动刷新
    const startAutoRefresh = () => {
      stopAutoRefresh()
      
      if (autoRefresh.value) {
        refreshMetrics()
        refreshTimer = setInterval(refreshMetrics, refreshInterval.value)
      }
    }
    
    // 停止自动刷新
    const stopAutoRefresh = () => {
      if (refreshTimer) {
        clearInterval(refreshTimer)
        refreshTimer = null
      }
    }
    
    // 监听自动刷新设置变化
    watch(autoRefresh, (newValue) => {
      if (newValue) {
        startAutoRefresh()
      } else {
        stopAutoRefresh()
      }
    })
    
    // 监听刷新间隔变化
    watch(refreshInterval, () => {
      if (autoRefresh.value) {
        startAutoRefresh()
      }
    })
    
    // 监听实例ID变化
    watch(() => props.instanceId, (newInstanceId) => {
      if (newInstanceId) {
        // 重置数据
        metrics.value = {
          runningTime: '00:00:00',
          cpuUsage: 0,
          memoryUsage: 0,
          memoryUsagePercent: 0,
          apiCalls: 0,
          startTime: null,
          lastUpdateTime: null
        }
        stepMetrics.value = []
        recentEvents.value = []
        expandedEvents.value = []
        
        // 获取初始数据
        refreshMetrics()
        
        // 如果启用了自动刷新，开始定时刷新
        if (autoRefresh.value) {
          startAutoRefresh()
        }
      } else {
        stopAutoRefresh()
      }
    })
    
    // 生命周期钩子
    onMounted(() => {
      if (props.instanceId) {
        refreshMetrics()
        if (autoRefresh.value) {
          startAutoRefresh()
        }
      }
    })
    
    onBeforeUnmount(() => {
      stopAutoRefresh()
    })
    
    return {
      isLoading,
      autoRefresh,
      refreshInterval,
      metrics,
      stepMetrics,
      recentEvents,
      expandedEvents,
      refreshMetrics,
      formatMemory,
      formatDuration,
      formatDate,
      getResourceColor,
      getDurationColor,
      getEventIcon,
      getEventColor,
      toggleEventDetails,
      getStatusType,
      getStatusText
    }
  }
}
</script>

<style scoped>
.workflow-realtime-monitor {
  margin-bottom: 20px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.header-controls {
  display: flex;
  gap: 12px;
  align-items: center;
}

.metrics-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 16px;
  margin-bottom: 20px;
}

.metric-card {
  text-align: center;
}

.metric-header {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  font-size: 14px;
}

.metric-value {
  font-size: 24px;
  font-weight: bold;
  padding: 10px 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

.metric-detail {
  font-size: 14px;
  font-weight: normal;
  margin-top: 5px;
  color: #606266;
}

.duration-chart {
  display: flex;
  height: 16px;
  width: 100%;
  border-radius: 4px;
  overflow: hidden;
}

.duration-segment {
  height: 100%;
  transition: width 0.3s ease;
}

.event-content h4 {
  margin: 0 0 5px 0;
  font-size: 14px;
}

.event-content p {
  margin: 0;
  font-size: 13px;
  color: #606266;
}

.event-details {
  margin-top: 8px;
}

.details-content {
  margin-top: 8px;
  background-color: #f8f8f8;
  padding: 10px;
  border-radius: 4px;
  border: 1px solid #ebeef5;
}

.details-content pre {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-all;
  font-family: monospace;
  font-size: 12px;
}

.no-instance {
  display: flex;
  justify-content: center;
  padding: 40px 0;
}
</style> 