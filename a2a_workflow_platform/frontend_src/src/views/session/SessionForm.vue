<template>
  <div class="session-form-container">
    <div class="page-header">
      <h1>{{ isEdit ? '编辑会话' : '创建会话' }}</h1>
      <el-button @click="$router.push(isEdit ? `/sessions/${sessionId}` : '/sessions')">
        <el-icon><back /></el-icon> {{ isEdit ? '返回详情' : '返回列表' }}
      </el-button>
    </div>

    <el-card class="form-card" v-loading="isLoading">
      <el-alert
        v-if="error"
        :title="error"
        type="error"
        :closable="false"
        show-icon
        class="mb-3"
      />

      <el-form 
        ref="formRef"
        :model="form"
        :rules="rules"
        label-position="top"
        @submit.prevent="submitForm"
      >
        <el-form-item label="会话名称" prop="name">
          <el-input v-model="form.name" placeholder="请输入会话名称" />
        </el-form-item>

        <el-form-item label="智能体" prop="agent_id">
          <el-select 
            v-model="form.agent_id" 
            placeholder="请选择智能体" 
            clearable
            style="width: 100%"
          >
            <el-option
              v-for="agent in agents"
              :key="agent.id"
              :label="agent.name"
              :value="agent.id"
            >
              <div class="agent-option">
                <span>{{ agent.name }}</span>
                <el-tag size="small" :type="getAgentTypeTag(agent.agent_type)">
                  {{ getAgentTypeLabel(agent.agent_type) }}
                </el-tag>
              </div>
            </el-option>
          </el-select>
          <div class="form-help-text">
            选择与此会话关联的智能体，可以留空
          </div>
        </el-form-item>

        <el-form-item label="元数据 (可选)" prop="metadata">
          <el-input
            v-model="form.metadata"
            type="textarea"
            :rows="5"
            placeholder="请输入元数据 (JSON格式)"
          />
          <div class="form-help-text">
            会话的附加元数据，使用JSON格式，例如：{\"tags\": [\"测试\", \"开发\"], \"priority\": \"high\"}
          </div>
        </el-form-item>

        <el-form-item label="描述 (可选)" prop="description">
          <el-input
            v-model="form.description"
            type="textarea"
            :rows="4"
            placeholder="请输入会话描述"
          />
        </el-form-item>

        <el-form-item>
          <el-button type="primary" native-type="submit" :loading="isSubmitting">
            {{ isEdit ? '更新会话' : '创建会话' }}
          </el-button>
          <el-button @click="$router.push(isEdit ? `/sessions/${sessionId}` : '/sessions')">
            取消
          </el-button>
        </el-form-item>
      </el-form>
    </el-card>
  </div>
</template>

<script>
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSessionStore } from '@/store/session'
import { useAgentStore } from '@/store/agent'
import { ElMessage } from 'element-plus'
import { Back } from '@element-plus/icons-vue'

export default {
  name: 'SessionForm',
  components: {
    Back
  },
  setup() {
    const route = useRoute()
    const router = useRouter()
    const sessionStore = useSessionStore()
    const agentStore = useAgentStore()
    const formRef = ref(null)
    
    // 判断是创建还是编辑模式
    const isEdit = computed(() => route.path.includes('/edit'))
    const sessionId = ref(route.params.id)
    
    // 表单数据
    const form = ref({
      name: '',
      agent_id: '',
      metadata: '{}',
      description: ''
    })
    
    // 表单验证规则
    const rules = {
      name: [
        { required: true, message: '请输入会话名称', trigger: 'blur' },
        { min: 2, max: 100, message: '长度在 2 到 100 个字符', trigger: 'blur' }
      ],
      metadata: [
        { validator: validateJSON, trigger: 'blur' }
      ]
    }
    
    // 验证JSON
    function validateJSON(rule, value, callback) {
      if (!value || value.trim() === '') {
        form.value.metadata = '{}'
        callback()
        return
      }
      
      try {
        JSON.parse(value)
        callback()
      } catch (e) {
        callback(new Error('元数据必须是有效的JSON格式'))
      }
    }
    
    // 状态
    const isLoading = computed(() => sessionStore.isLoading || agentStore.isLoading)
    const error = computed(() => sessionStore.error)
    const isSubmitting = ref(false)
    const agents = computed(() => agentStore.agents)
    
    // 初始化
    onMounted(async () => {
      // 加载代理列表
      if (agents.value.length === 0) {
        await agentStore.fetchAgents()
      }
      
      // 如果是编辑模式，获取会话详情
      if (isEdit.value) {
        try {
          const session = await sessionStore.fetchSession(sessionId.value)
          if (session) {
            form.value.name = session.name || ''
            form.value.agent_id = session.agent ? session.agent.id : ''
            form.value.description = session.description || ''
            form.value.metadata = session.metadata ? JSON.stringify(session.metadata, null, 2) : '{}'
          }
        } catch (err) {
          console.error('获取会话详情失败', err)
        }
      }
    })
    
    // 提交表单
    const submitForm = async () => {
      if (!formRef.value) return
      
      await formRef.value.validate(async (valid) => {
        if (!valid) return
        
        isSubmitting.value = true
        
        try {
          // 解析元数据
          let metadata = {}
          try {
            metadata = JSON.parse(form.value.metadata)
          } catch (e) {
            ElMessage.warning('元数据格式不正确，使用空对象')
          }
          
          // 准备提交数据
          const sessionData = {
            name: form.value.name,
            agent_id: form.value.agent_id || null,
            description: form.value.description,
            metadata
          }
          
          if (isEdit.value) {
            // 更新会话
            await sessionStore.updateSession(sessionId.value, sessionData)
            ElMessage.success('会话更新成功')
            router.push(`/sessions/${sessionId.value}`)
          } else {
            // 创建会话
            const newSession = await sessionStore.createSession(sessionData)
            ElMessage.success('会话创建成功')
            router.push(`/sessions/${newSession.id}`)
          }
        } catch (err) {
          console.error('提交会话表单失败', err)
        } finally {
          isSubmitting.value = false
        }
      })
    }
    
    // 获取智能体类型的标签类型
    const getAgentTypeTag = (type) => {
      const typeMap = {
        'gpt-3.5': 'info',
        'gpt-4': 'success',
        'claude-3': 'warning',
        'gemini': 'danger',
        'custom': 'primary',
        'a2a': ''
      }
      return typeMap[type] || ''
    }
    
    // 获取智能体类型的显示标签
    const getAgentTypeLabel = (type) => {
      const labelMap = {
        'gpt-3.5': 'GPT-3.5',
        'gpt-4': 'GPT-4',
        'claude-3': 'Claude 3',
        'gemini': 'Gemini',
        'custom': '自定义',
        'a2a': 'A2A兼容'
      }
      return labelMap[type] || type
    }
    
    return {
      isEdit,
      sessionId,
      form,
      rules,
      formRef,
      isLoading,
      error,
      isSubmitting,
      agents,
      submitForm,
      getAgentTypeTag,
      getAgentTypeLabel
    }
  }
}
</script>

<style scoped>
.session-form-container {
  max-width: 800px;
  margin: 0 auto;
  padding: 20px;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.page-header h1 {
  margin: 0;
}

.form-card {
  margin-bottom: 30px;
}

.mb-3 {
  margin-bottom: 15px;
}

.form-help-text {
  margin-top: 5px;
  font-size: 12px;
  color: #909399;
}

.agent-option {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
}
</style> 