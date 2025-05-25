<template>
  <div class="workflow-instance-detail-container">
    <el-card class="workflow-instance-detail-card" v-loading="loading">
      <template #header>
        <div class="card-header">
          <div class="header-left">
            <h2>{{ instance.name || '工作流实例详情' }}</h2>
            <el-tag :type="getStatusType(instance.status)" class="status-tag">
              {{ getStatusText(instance.status) }}
            </el-tag>
          </div>
          <div class="header-actions">
            <el-button-group>
              <el-button 
                v-if="instance.status === 'created'" 
                type="primary" 
                @click="startInstance"
              >
                启动
              </el-button>
              <el-button 
                v-if="instance.status === 'running'" 
                type="warning" 
                @click="pauseInstance"
              >
                暂停
              </el-button>
              <el-button 
                v-if="instance.status === 'paused'" 
                type="success" 
                @click="resumeInstance"
              >
                恢复
              </el-button>
              <el-button 
                v-if="['running', 'paused'].includes(instance.status)" 
                type="danger" 
                @click="cancelInstance"
              >
                取消
              </el-button>
              <el-button 
                type="info" 
                @click="cloneInstance"
              >
                克隆
              </el-button>
              <el-button 
                type="primary" 
                plain
                @click="goToInstanceList"
              >
                返回列表
              </el-button>
            </el-button-group>
          </div>
        </div>
      </template>
      
      <!-- 实例基本信息 -->
      <el-descriptions title="基本信息" :column="3" border>
        <el-descriptions-item label="实例ID">{{ instance.instance_id || '-' }}</el-descriptions-item>
        <el-descriptions-item label="所属工作流">{{ instance.workflow?.name || '-' }}</el-descriptions-item>
        <el-descriptions-item label="创建者">{{ instance.created_by?.username || '-' }}</el-descriptions-item>
        <el-descriptions-item label="创建时间">{{ formatDateTime(instance.created_at) }}</el-descriptions-item>
        <el-descriptions-item label="开始时间">{{ formatDateTime(instance.started_at) }}</el-descriptions-item>
        <el-descriptions-item label="结束时间">{{ formatDateTime(instance.finished_at) }}</el-descriptions-item>
      </el-descriptions>
      
      <!-- 步骤列表 -->
      <div class="steps-container">
        <h3>执行步骤</h3>
        <el-timeline>
          <el-timeline-item
            v-for="step in steps"
            :key="step.id"
            :type="getStepStatusType(step.status)"
            :timestamp="formatDateTime(step.started_at)"
            :hollow="step.status === 'pending'"
          >
            <el-card class="step-card">
              <div class="step-header">
                <span class="step-name">{{ step.name }}</span>
                <el-tag :type="getStepStatusType(step.status)" size="small">{{ getStepStatusText(step.status) }}</el-tag>
              </div>
              
              <div class="step-info">
                <p v-if="step.agent_name">执行智能体: {{ step.agent_name }}</p>
                <p v-if="step.description">{{ step.description }}</p>
                <p v-if="step.error_message" class="error-message">错误信息: {{ step.error_message }}</p>
              </div>
              
              <div class="step-actions" v-if="step.status === 'failed'">
                <el-button type="warning" size="small" @click="retryStep(step)">重试</el-button>
              </div>
            </el-card>
          </el-timeline-item>
        </el-timeline>
      </div>
      
      <!-- 工作流上下文 -->
      <div class="context-container" v-if="showContext">
        <h3>工作流上下文</h3>
        <el-card class="code-card">
          <pre class="context-json">{{ formattedContext }}</pre>
        </el-card>
      </div>
    </el-card>
  </div>
</template>

<script>
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import axios from 'axios'

export default {
  name: 'WorkflowInstanceDetail',
  setup() {
    const route = useRoute()
    const router = useRouter()
    const loading = ref(false)
    const instance = ref({})
    const steps = ref([])
    const showContext = ref(true)  // 是否显示上下文
    
    // 获取实例ID
    const instanceId = computed(() => route.params.id)
    
    // 格式化的上下文
    const formattedContext = computed(() => {
      try {
        return JSON.stringify(instance.value.context || {}, null, 2)
      } catch (e) {
        return '{}'
      }
    })
    
    // 获取工作流实例详情
    const fetchInstanceDetail = async () => {
      loading.value = true
      try {
        const response = await axios.get(`/api/workflows/instances/${instanceId.value}/`)
        instance.value = response.data
        steps.value = instance.value.steps || []
      } catch (error) {
        console.error('获取工作流实例详情失败', error)
        ElMessage.error('获取工作流实例详情失败')
      } finally {
        loading.value = false
      }
    }
    
    // 格式化日期时间
    const formatDateTime = (datetime) => {
      if (!datetime) return '-'
      const date = new Date(datetime)
      return date.toLocaleString('zh-CN')
    }
    
    // 获取状态对应的类型
    const getStatusType = (status) => {
      const types = {
        'created': 'info',
        'running': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'cancelled': 'info',
        'paused': 'info'
      }
      return types[status] || 'info'
    }
    
    // 获取状态对应的文本
    const getStatusText = (status) => {
      const texts = {
        'created': '已创建',
        'running': '运行中',
        'completed': '已完成',
        'failed': '失败',
        'cancelled': '已取消',
        'paused': '已暂停'
      }
      return texts[status] || status
    }
    
    // 获取步骤状态对应的类型
    const getStepStatusType = (status) => {
      const types = {
        'pending': 'info',
        'running': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'skipped': 'info'
      }
      return types[status] || 'info'
    }
    
    // 获取步骤状态对应的文本
    const getStepStatusText = (status) => {
      const texts = {
        'pending': '等待中',
        'running': '执行中',
        'completed': '已完成',
        'failed': '失败',
        'skipped': '已跳过'
      }
      return texts[status] || status
    }
    
    // 启动实例
    const startInstance = async () => {
      try {
        await axios.post(`/api/workflows/instances/${instanceId.value}/start/`)
        ElMessage.success('工作流实例已启动')
        fetchInstanceDetail()
      } catch (error) {
        console.error('启动工作流实例失败', error)
        ElMessage.error('启动工作流实例失败')
      }
    }
    
    // 暂停实例
    const pauseInstance = async () => {
      try {
        await axios.post(`/api/workflows/instances/${instanceId.value}/pause/`)
        ElMessage.success('工作流实例已暂停')
        fetchInstanceDetail()
      } catch (error) {
        console.error('暂停工作流实例失败', error)
        ElMessage.error('暂停工作流实例失败')
      }
    }
    
    // 恢复实例
    const resumeInstance = async () => {
      try {
        await axios.post(`/api/workflows/instances/${instanceId.value}/resume/`)
        ElMessage.success('工作流实例已恢复')
        fetchInstanceDetail()
      } catch (error) {
        console.error('恢复工作流实例失败', error)
        ElMessage.error('恢复工作流实例失败')
      }
    }
    
    // 取消实例
    const cancelInstance = async () => {
      ElMessageBox.confirm(
        '确定要取消此工作流实例吗？这个操作不可逆。',
        '确认取消',
        {
          confirmButtonText: '确定',
          cancelButtonText: '取消',
          type: 'warning'
        }
      ).then(async () => {
        try {
          await axios.post(`/api/workflows/instances/${instanceId.value}/cancel/`)
          ElMessage.success('工作流实例已取消')
          fetchInstanceDetail()
        } catch (error) {
          console.error('取消工作流实例失败', error)
          ElMessage.error('取消工作流实例失败')
        }
      }).catch(() => {
        // 用户取消操作
      })
    }
    
    // 克隆实例
    const cloneInstance = async () => {
      try {
        const response = await axios.post(`/api/workflows/instances/${instanceId.value}/clone/`)
        ElMessage.success('已成功克隆工作流实例')
        // 跳转到新实例
        if (response.data && response.data.instance_id) {
          router.push(`/workflow/instance/${response.data.instance_id}`)
        }
      } catch (error) {
        console.error('克隆工作流实例失败', error)
        ElMessage.error('克隆工作流实例失败')
      }
    }
    
    // 重试步骤
    const retryStep = async (step) => {
      try {
        await axios.post(`/api/workflows/instances/${instanceId.value}/retry-step/${step.id}/`)
        ElMessage.success('步骤已重试')
        fetchInstanceDetail()
      } catch (error) {
        console.error('重试步骤失败', error)
        ElMessage.error('重试步骤失败')
      }
    }
    
    // 返回列表
    const goToInstanceList = () => {
      router.push('/workflow/instance')
    }
    
    onMounted(() => {
      fetchInstanceDetail()
    })
    
    return {
      loading,
      instance,
      steps,
      showContext,
      formattedContext,
      formatDateTime,
      getStatusType,
      getStatusText,
      getStepStatusType,
      getStepStatusText,
      startInstance,
      pauseInstance,
      resumeInstance,
      cancelInstance,
      cloneInstance,
      retryStep,
      goToInstanceList
    }
  }
}
</script>

<style scoped>
.workflow-instance-detail-container {
  padding: 20px;
}

.workflow-instance-detail-card {
  width: 100%;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.header-left {
  display: flex;
  align-items: center;
}

.header-left h2 {
  margin: 0;
  margin-right: 15px;
}

.status-tag {
  margin-left: 10px;
}

.steps-container,
.context-container {
  margin-top: 20px;
}

.step-card {
  margin-bottom: 10px;
}

.step-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}

.step-name {
  font-weight: bold;
}

.step-info {
  color: #606266;
}

.step-info p {
  margin: 5px 0;
}

.error-message {
  color: #f56c6c;
}

.context-json {
  background: #f8f8f8;
  padding: 10px;
  border-radius: 4px;
  overflow: auto;
  max-height: 300px;
}

.step-actions {
  margin-top: 10px;
}
</style> 