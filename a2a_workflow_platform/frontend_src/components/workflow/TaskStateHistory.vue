<template>
  <div class="task-state-history">
    <el-card>
      <template #header>
        <div class="card-header">
          <h3>任务状态历史</h3>
          <div class="header-actions">
            <el-button 
              size="small" 
              type="primary" 
              @click="loadStateHistory" 
              :loading="loading"
              :disabled="loading"
            >
              <el-icon><refresh /></el-icon> 刷新
        </el-button>
            <el-radio-group v-model="sortOrder" size="small" class="ml-2">
          <el-radio-button label="desc">最新在前</el-radio-button>
          <el-radio-button label="asc">最早在前</el-radio-button>
        </el-radio-group>
      </div>
        </div>
      </template>

      <div v-if="loading && !stateHistory.length" class="loading-container">
        <el-skeleton :rows="3" animated />
      </div>
      
      <el-alert
        v-else-if="error"
        :title="error"
        type="error"
        :closable="false"
        show-icon
      />
      
      <el-empty v-else-if="!stateHistory || stateHistory.length === 0" description="无状态历史记录" />
      
      <div v-else>
      <!-- 状态变更统计 -->
        <div class="state-stats" v-if="stateTransitions.length > 0">
          <el-divider content-position="left">状态转换时间</el-divider>
        <el-table :data="stateTransitions" size="small" border stripe>
          <el-table-column prop="from" label="从状态" width="120">
            <template #default="scope">
              <el-tag size="small" :type="getStateTagType(scope.row.from)">
                {{ getStateDisplayName(scope.row.from) }}
              </el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="to" label="到状态" width="120">
            <template #default="scope">
              <el-tag size="small" :type="getStateTagType(scope.row.to)">
                {{ getStateDisplayName(scope.row.to) }}
              </el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="duration" label="持续时间">
            <template #default="scope">
              {{ scope.row.duration }}
            </template>
          </el-table-column>
        </el-table>
      </div>
    
        <el-divider content-position="left">状态历史记录</el-divider>
      <div class="state-timeline">
        <div v-for="(record, index) in sortedStateHistory" :key="index" class="state-entry">
          <div class="state-badge" :class="getStateBadgeClass(record.state)">
              <el-tag :type="getStateTagType(record.state)">
            {{ getStateDisplayName(record.state) }}
              </el-tag>
          </div>
          <div class="state-details">
            <div class="state-time">
              {{ formatTime(record.timestamp) }}
            </div>
            <div v-if="record.reason" class="state-reason">
              <strong>原因:</strong> {{ record.reason }}
            </div>
            <div v-if="record.metadata" class="state-metadata">
              <el-collapse>
                <el-collapse-item title="详细信息">
                    <codemirror
                      :model-value="JSON.stringify(record.metadata, null, 2)"
                      disabled
                      :style="{ height: 'auto', maxHeight: '300px' }"
                      :extensions="cmJsonExtensions"
                    />
                </el-collapse-item>
              </el-collapse>
            </div>
              <div v-if="index < sortedStateHistory.length - 1 && sortOrder === 'asc'" class="state-duration">
              <el-tag size="small" type="info">
                持续时间: {{ calculateDuration(record.timestamp, sortedStateHistory[index + 1].timestamp) }}
              </el-tag>
              </div>
              <div v-if="index > 0 && sortOrder === 'desc'" class="state-duration">
                <el-tag size="small" type="info">
                  持续时间: {{ calculateDuration(record.timestamp, sortedStateHistory[index - 1].timestamp) }}
                </el-tag>
              </div>
            </div>
          </div>
        </div>
      </div>
    </el-card>
  </div>
</template>

<script>
import { ref, defineComponent, onMounted, computed, shallowRef } from 'vue'
import { getTaskStateHistory } from '@/api/a2a'
import { Refresh } from '@element-plus/icons-vue'
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'

export default defineComponent({
  name: 'TaskStateHistory',
  components: {
    Refresh,
    Codemirror
  },
  props: {
    taskId: {
      type: String,
      required: true
    }
  },
  setup(props) {
    const stateHistory = ref([])
    const loading = ref(false)
    const error = ref(null)
    const sortOrder = ref('desc') // 默认最新的在前面

    // Codemirror JSON Extensions
    const cmJsonExtensions = shallowRef([
      json(),
      oneDark,
      EditorView.lineWrapping,
      EditorView.editable.of(false)
    ])

    // 加载状态历史
    const loadStateHistory = async () => {
      loading.value = true
      error.value = null

      try {
        const response = await getTaskStateHistory(props.taskId)
        if (response.data.result && response.data.result.stateHistory) {
          // 按时间排序，最新的在前面
          stateHistory.value = response.data.result.stateHistory.sort(
            (a, b) => new Date(b.timestamp) - new Date(a.timestamp)
          )
        } else {
          error.value = '无法获取状态历史'
        }
      } catch (err) {
        console.error('加载状态历史失败', err)
        error.value = '加载状态历史失败: ' + (err.response?.data?.error?.message || err.message)
      } finally {
        loading.value = false
      }
    }

    // 格式化时间戳
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

    // 获取状态的样式类名
    const getStateBadgeClass = (state) => {
      const classMap = {
        'submitted': 'badge-info',
        'working': 'badge-primary',
        'input-required': 'badge-warning',
        'completed': 'badge-success',
        'failed': 'badge-danger',
        'canceled': 'badge-secondary'
      }
      return classMap[state] || 'badge-light'
    }

    // 计算状态持续时间
    const calculateDuration = (start, end) => {
      if (!start || !end) return 'N/A'
      
      const startTime = new Date(start)
      const endTime = new Date(end)
      
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
    
    // 计算状态转换信息
    const stateTransitions = computed(() => {
      if (!stateHistory.value || stateHistory.value.length <= 1) return []
      
      // 按时间从早到晚排序状态历史
      const sortedHistory = [...stateHistory.value].sort(
        (a, b) => new Date(a.timestamp) - new Date(b.timestamp)
      )
      
      const transitions = []
      
      for (let i = 0; i < sortedHistory.length - 1; i++) {
        transitions.push({
          from: sortedHistory[i].state,
          to: sortedHistory[i + 1].state,
          fromTime: sortedHistory[i].timestamp,
          toTime: sortedHistory[i + 1].timestamp,
          duration: calculateDuration(sortedHistory[i].timestamp, sortedHistory[i + 1].timestamp)
        })
      }
      
      return transitions
    })
    
    // 根据排序方式返回排序后的状态历史
    const sortedStateHistory = computed(() => {
      if (!stateHistory.value) return []
      
      const sorted = [...stateHistory.value]
      
      if (sortOrder.value === 'asc') {
        // 按时间从早到晚排序
        return sorted.sort((a, b) => new Date(a.timestamp) - new Date(b.timestamp))
      } else {
        // 按时间从晚到早排序
        return sorted.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp))
      }
    })
    
    // 获取状态标签类型
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

    onMounted(() => {
      if (props.taskId) {
        loadStateHistory()
      }
    })

    return {
      stateHistory,
      sortedStateHistory,
      stateTransitions,
      loading,
      error,
      sortOrder,
      formatTime,
      getStateDisplayName,
      getStateBadgeClass,
      getStateTagType,
      calculateDuration,
      loadStateHistory,
      cmJsonExtensions
    }
  }
})
</script>

<style scoped>
.task-state-history {
  margin-top: 20px;
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

.ml-2 {
  margin-left: 8px;
}

.loading-container {
  padding: 20px 0;
}

.state-stats {
  margin-bottom: 20px;
}

.state-timeline {
  display: flex;
  flex-direction: column;
  gap: 15px;
}

.state-entry {
  display: flex;
  gap: 15px;
  position: relative;
  padding-bottom: 15px;
}

.state-entry:not(:last-child)::after {
  content: '';
  position: absolute;
  bottom: 0;
  left: 35px;
  height: 15px;
  width: 2px;
  background-color: #e9e9eb;
}

.state-badge {
  min-width: 70px;
  text-align: center;
}

.state-details {
  flex: 1;
  background-color: #f5f7fa;
  border-radius: 4px;
  padding: 12px;
  border-left: 4px solid #dcdfe6;
  transition: all 0.3s;
}

.state-time {
  font-weight: 500;
  color: #606266;
  margin-bottom: 8px;
  font-size: 14px;
}

.state-reason {
  margin-top: 8px;
  font-size: 14px;
}

.state-metadata {
  margin-top: 12px;
}

.state-metadata :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
  background-color: #f8f8f8;
  border: 1px solid #dcdfe6;
}

.state-metadata :deep(.cm-scroller) {
  padding: 8px;
  overflow: auto;
}

.state-duration {
  margin-top: 10px;
  display: flex;
  justify-content: flex-end;
}

/* Customize entry border based on state */
.badge-submitted + .state-details {
  border-left-color: #909399;
}

.badge-working + .state-details {
  border-left-color: #409EFF;
}

.badge-input-required + .state-details {
  border-left-color: #E6A23C;
}

.badge-completed + .state-details {
  border-left-color: #67C23A;
}

.badge-failed + .state-details {
  border-left-color: #F56C6C;
}

.badge-canceled + .state-details {
  border-left-color: #909399;
}

.badge-light + .state-details {
  border-left-color: #DCDFE6;
}
</style> 