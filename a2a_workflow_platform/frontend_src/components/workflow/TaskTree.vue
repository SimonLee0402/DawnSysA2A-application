<template>
  <div class="task-tree">
    <el-card>
      <template #header>
        <div class="card-header">
          <h3>任务树</h3>
          <div class="header-actions">
            <el-button 
              size="small" 
              type="primary" 
              @click="loadTaskTree" 
              :loading="loading"
              :disabled="loading"
            >
              <el-icon><refresh /></el-icon> 刷新
        </el-button>
        <el-switch
          v-model="autoRefresh"
          active-text="自动刷新"
          @change="toggleAutoRefresh"
              class="ml-2"
              :disabled="loading"
        />
        <span v-if="autoRefresh" class="auto-refresh-info ml-2">
              {{ refreshInterval / 1000 }}秒
        </span>
          </div>
        </div>
      </template>
      
      <div v-if="loading && !taskTree" class="loading-container">
        <el-skeleton :rows="3" animated />
      </div>
      
      <el-alert
        v-else-if="error"
        :title="error"
        type="error"
        :closable="false"
        show-icon
      />
      
      <el-empty v-else-if="!taskTree" description="无任务树数据" />
      
      <div v-else class="tree-container">
      <!-- 任务树可视化 -->
      <div class="tree-node main-node">
        <div class="node-content" :class="getNodeClass(taskTree.taskId)" @click="navigateToTask(taskTree.taskId)">
          <div class="node-title">当前任务</div>
          <div class="node-id">{{ formatTaskId(taskTree.taskId) }}</div>
          <div v-if="taskStatusMap[taskTree.taskId]" class="node-status">
              <el-tag size="small" :type="getNodeTagType(taskTree.taskId)">
            {{ getStateDisplayName(taskStatusMap[taskTree.taskId].state) }}
              </el-tag>
          </div>
        </div>
        
        <!-- 父任务连接线 -->
        <div v-if="taskTree.parentTaskId" class="parent-connection">
          <div class="connection-line"></div>
          <div class="parent-node">
            <div class="node-content" :class="getNodeClass(taskTree.parentTaskId)" @click="navigateToTask(taskTree.parentTaskId)">
              <div class="node-title">父任务</div>
              <div class="node-id">{{ formatTaskId(taskTree.parentTaskId) }}</div>
              <div v-if="taskStatusMap[taskTree.parentTaskId]" class="node-status">
                  <el-tag size="small" :type="getNodeTagType(taskTree.parentTaskId)">
                {{ getStateDisplayName(taskStatusMap[taskTree.parentTaskId].state) }}
                  </el-tag>
              </div>
            </div>
          </div>
        </div>
        
        <!-- 子任务连接线和节点 -->
        <div v-if="taskTree.childTaskIds && taskTree.childTaskIds.length > 0" class="children-container">
          <div class="children-connection">
            <div class="connection-line"></div>
            <div class="connection-branches"></div>
          </div>
          <div class="children-nodes">
            <div v-for="(childId, index) in taskTree.childTaskIds" :key="index" class="child-node">
              <div class="node-content" :class="getNodeClass(childId)" @click="navigateToTask(childId)">
                <div class="node-title">子任务 {{ index + 1 }}</div>
                <div class="node-id">{{ formatTaskId(childId) }}</div>
                <div v-if="taskStatusMap[childId]" class="node-status">
                    <el-tag size="small" :type="getNodeTagType(childId)">
                  {{ getStateDisplayName(taskStatusMap[childId].state) }}
                    </el-tag>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </el-card>
  </div>
</template>

<script>
import { ref, defineComponent, onMounted, computed, onBeforeUnmount } from 'vue'
import { getTaskTree, getTask } from '@/api/a2a'
import { useRouter } from 'vue-router'
import { Refresh } from '@element-plus/icons-vue'

export default defineComponent({
  name: 'TaskTree',
  components: {
    Refresh
  },
  props: {
    taskId: {
      type: String,
      required: true
    }
  },
  setup(props) {
    const router = useRouter()
    const taskTree = ref(null)
    const loading = ref(false)
    const error = ref(null)
    const taskStatusMap = ref({})
    const autoRefresh = ref(false)
    const refreshInterval = ref(10000) // 10秒刷新一次
    let refreshTimer = null

    // 加载任务树
    const loadTaskTree = async () => {
      loading.value = true
      error.value = null

      try {
        const response = await getTaskTree(props.taskId)
        if (response.data.result && response.data.result.taskTree) {
          taskTree.value = response.data.result.taskTree
          
          // 获取所有相关任务的状态
          await loadTasksStatus()
        } else {
          error.value = '无法获取任务树'
        }
      } catch (err) {
        console.error('加载任务树失败', err)
        error.value = '加载任务树失败: ' + (err.response?.data?.error?.message || err.message)
      } finally {
        loading.value = false
      }
    }
    
    // 加载所有相关任务的状态
    const loadTasksStatus = async () => {
      if (!taskTree.value) return
      
      // 收集所有需要获取状态的任务ID
      const taskIds = [taskTree.value.taskId]
      if (taskTree.value.parentTaskId) {
        taskIds.push(taskTree.value.parentTaskId)
      }
      if (taskTree.value.childTaskIds && taskTree.value.childTaskIds.length > 0) {
        taskIds.push(...taskTree.value.childTaskIds)
      }
      
      // 为每个任务ID获取状态
      const promises = taskIds.map(async (id) => {
        try {
          const response = await getTask(id)
          if (response.data.result && response.data.result.task) {
            const task = response.data.result.task
            taskStatusMap.value[id] = task.status
          }
        } catch (error) {
          console.error(`获取任务 ${id} 状态失败:`, error)
        }
      })
      
      await Promise.all(promises)
    }

    // 格式化任务ID显示
    const formatTaskId = (id) => {
      if (!id) return 'N/A'
      // 只显示前8个字符
      return id.substring(0, 8) + '...'
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
    
    // 根据任务状态获取节点样式类
    const getNodeClass = (taskId) => {
      if (!taskStatusMap.value[taskId]) return 'status-unknown'
      
      const state = taskStatusMap.value[taskId].state
      const classMap = {
        'submitted': 'status-submitted',
        'working': 'status-working',
        'input-required': 'status-input-required',
        'completed': 'status-completed',
        'failed': 'status-failed',
        'canceled': 'status-canceled'
      }
      
      return classMap[state] || 'status-unknown'
    }

    // 根据任务状态获取标签类型
    const getNodeTagType = (taskId) => {
      if (!taskStatusMap.value[taskId]) return '';
      
      const state = taskStatusMap.value[taskId].state;
      const typeMap = {
        'submitted': 'info',
        'working': 'primary',
        'input-required': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'canceled': 'info'
      };
      
      return typeMap[state] || '';
    }

    // 切换自动刷新
    const toggleAutoRefresh = (value) => {
      if (value) {
        // 启动定时器
        refreshTimer = setInterval(() => {
          loadTaskTree()
        }, refreshInterval.value)
      } else {
        // 清除定时器
        clearInterval(refreshTimer)
      }
    }
    
    // 导航到指定任务
    const navigateToTask = (taskId) => {
      if (taskId === props.taskId) return // 当前任务不跳转
      
      router.push({
        name: 'task-detail',
        params: { id: taskId }
      })
    }

    onMounted(() => {
      if (props.taskId) {
        loadTaskTree()
      }
    })
    
    // 组件卸载前清除定时器
    onBeforeUnmount(() => {
      if (refreshTimer) {
        clearInterval(refreshTimer)
      }
    })

    return {
      taskTree,
      loading,
      error,
      taskStatusMap,
      autoRefresh,
      refreshInterval,
      formatTaskId,
      getStateDisplayName,
      getNodeClass,
      getNodeTagType,
      loadTaskTree,
      toggleAutoRefresh,
      navigateToTask
    }
  }
})
</script>

<style scoped>
.task-tree {
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

.auto-refresh-info {
  color: #909399;
  font-size: 12px;
}

.loading-container {
  padding: 20px 0;
}

.tree-container {
  padding: 20px 0;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.tree-node {
  display: flex;
  flex-direction: column;
  align-items: center;
  margin-bottom: 20px;
}

.node-content {
  width: 200px;
  padding: 15px;
  border-radius: 4px;
  background-color: #f5f7fa;
  cursor: pointer;
  transition: all 0.3s;
  border: 1px solid #dcdfe6;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.05);
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.node-content:hover {
  transform: translateY(-2px);
  box-shadow: 0 4px 8px rgba(0, 0, 0, 0.1);
}

.node-title {
  font-weight: bold;
  margin-bottom: 8px;
}

.node-id {
  font-family: monospace;
  margin-bottom: 8px;
  word-break: break-all;
}

.node-status {
  margin-top: 5px;
}

.parent-connection, .children-connection {
  width: 2px;
  height: 40px;
  background-color: #dcdfe6;
  margin: 10px 0;
}

.connection-branches {
  position: relative;
  height: 2px;
  background-color: #dcdfe6;
  width: 100%;
  max-width: 300px;
}

.children-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 100%;
}

.children-nodes {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 20px;
  max-width: 800px;
  margin-top: 10px;
}

.child-node {
  margin-bottom: 10px;
}

/* Status styling */
.status-submitted {
  border-left: 4px solid #909399;
}

.status-working {
  border-left: 4px solid #409EFF;
}

.status-input-required {
  border-left: 4px solid #E6A23C;
}

.status-completed {
  border-left: 4px solid #67C23A;
}

.status-failed {
  border-left: 4px solid #F56C6C;
}

.status-canceled {
  border-left: 4px solid #909399;
}

.status-unknown {
  border-left: 4px solid #DCDFE6;
}
</style> 