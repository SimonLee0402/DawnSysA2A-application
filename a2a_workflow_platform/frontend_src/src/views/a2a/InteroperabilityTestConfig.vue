<template>
  <div class="test-config-manager">
    <el-collapse accordion>
      <el-collapse-item title="保存/加载测试配置" name="config-manager">
        <div class="config-manager-container">
          <div class="save-config">
            <h4 class="app-card-title">保存当前配置</h4>
            <el-form :model="saveForm" :rules="saveRules" ref="saveFormRef" label-position="top">
              <el-form-item label="配置名称" prop="name">
                <el-input v-model="saveForm.name" placeholder="请输入配置名称"></el-input>
              </el-form-item>
              <el-form-item label="描述">
                <el-input v-model="saveForm.description" type="textarea" :rows="2" placeholder="请输入配置描述"></el-input>
              </el-form-item>
              <el-form-item>
                <el-button type="primary" @click="saveConfig" size="small">保存配置</el-button>
              </el-form-item>
            </el-form>
          </div>
          
          <el-divider direction="vertical"></el-divider>
          
          <div class="load-config">
            <h4 class="app-card-title">加载保存的配置</h4>
            <el-empty v-if="!savedConfigs || savedConfigs.length === 0" description="暂无保存的配置" :image-size="100"></el-empty>
            <div v-else class="config-list">
              <el-card v-for="config in savedConfigs" :key="config.id" class="config-card app-card">
                <template #header>
                  <div class="config-card-header">
                    <span>{{ config.name }}</span>
                    <div class="config-actions">
                      <el-button type="primary" size="mini" @click="loadConfig(config)" icon="el-icon-download">
                        加载
                      </el-button>
                      <el-button type="danger" size="mini" @click="deleteConfig(config)" icon="el-icon-delete">
                        删除
                      </el-button>
                    </div>
                  </div>
                </template>
                <div class="config-info">
                  <div v-if="config.description" class="config-description">{{ config.description }}</div>
                  <div class="config-details">
                    <span class="config-target">目标: {{ config.targetUrl }}</span>
                    <span class="config-type">类型: {{ getTestTypeDisplayName(config.testType) }}</span>
                    <span class="config-date">保存时间: {{ formatDateTime(config.savedAt) }}</span>
                  </div>
                </div>
              </el-card>
            </div>
          </div>
        </div>
      </el-collapse-item>
    </el-collapse>
  </div>
</template>

<script>
import { ref, defineComponent, computed, onMounted } from 'vue'
import { ElMessage } from 'element-plus'

export default {
  name: 'InteroperabilityTestConfig',
  
  props: {
    currentConfig: {
      type: Object,
      required: true
    }
  },
  
  emits: ['load-config'],
  
  setup(props, { emit }) {
    const saveFormRef = ref(null)
    const saveForm = ref({
      name: '',
      description: ''
    })
    
    const savedConfigs = ref([])
    
    const saveRules = {
      name: [
        { required: true, message: '请输入配置名称', trigger: 'blur' },
        { min: 2, max: 50, message: '配置名称长度需在2-50个字符之间', trigger: 'blur' }
      ]
    }
    
    // 从本地存储获取保存的配置
    const loadSavedConfigs = () => {
      try {
        const configsStr = localStorage.getItem('a2a_interop_test_configs')
        if (configsStr) {
          savedConfigs.value = JSON.parse(configsStr)
        }
      } catch (error) {
        console.error('加载测试配置失败:', error)
        ElMessage.error('加载测试配置失败')
      }
    }
    
    // 保存配置到本地存储
    const saveToLocalStorage = () => {
      try {
        localStorage.setItem('a2a_interop_test_configs', JSON.stringify(savedConfigs.value))
      } catch (error) {
        console.error('保存测试配置失败:', error)
        ElMessage.error('保存测试配置失败')
      }
    }
    
    // 保存当前配置
    const saveConfig = () => {
      saveFormRef.value.validate((valid) => {
        if (valid) {
          const configToSave = {
            id: Date.now().toString(),
            name: saveForm.value.name,
            description: saveForm.value.description,
            agentId: props.currentConfig.agentId,
            targetUrl: props.currentConfig.targetUrl,
            testType: props.currentConfig.testType,
            taskContent: props.currentConfig.taskContent,
            timeout: props.currentConfig.timeout,
            retries: props.currentConfig.retries,
            verifySSL: props.currentConfig.verifySSL,
            testCases: [...props.currentConfig.testCases],
            savedAt: new Date().toISOString()
          }
          
          savedConfigs.value.push(configToSave)
          saveToLocalStorage()
          
          ElMessage.success('配置保存成功')
          saveForm.value.name = ''
          saveForm.value.description = ''
        }
      })
    }
    
    // 加载保存的配置
    const loadConfig = (config) => {
      emit('load-config', config)
      ElMessage.success(`配置 "${config.name}" 已加载`)
    }
    
    // 删除保存的配置
    const deleteConfig = (config) => {
      const index = savedConfigs.value.findIndex(c => c.id === config.id)
      if (index !== -1) {
        savedConfigs.value.splice(index, 1)
        saveToLocalStorage()
        ElMessage.success(`配置 "${config.name}" 已删除`)
      }
    }
    
    // 辅助函数 - 格式化日期时间
    const formatDateTime = (timestamp) => {
      if (!timestamp) return '-'
      return new Date(timestamp).toLocaleString()
    }
    
    // 辅助函数 - 获取测试类型显示名称
    const getTestTypeDisplayName = (type) => {
      const typeMap = {
        'basic': '基本测试',
        'streaming': '流式响应',
        'push_notification': '推送通知',
        'full': '完整测试'
      }
      
      return typeMap[type] || type
    }
    
    // 初始化时加载保存的配置
    onMounted(() => {
      loadSavedConfigs()
    })
    
    return {
      saveFormRef,
      saveForm,
      saveRules,
      savedConfigs,
      formatDateTime,
      getTestTypeDisplayName,
      saveConfig,
      loadConfig,
      deleteConfig
    }
  }
}
</script>

<style scoped>
.test-config-manager {
  margin-bottom: 20px;
}

.config-manager-container {
  display: flex;
  justify-content: space-between;
  gap: 20px;
}

.save-config,
.load-config {
  flex: 1;
}

.config-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-height: 300px;
  overflow-y: auto;
}

.config-card {
  margin-bottom: 10px;
}

.config-card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.config-actions {
  display: flex;
  gap: 10px;
}

.config-info {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.config-description {
  font-size: 14px;
  color: #606266;
  margin-bottom: 8px;
}

.config-details {
  display: flex;
  flex-direction: column;
  font-size: 13px;
  color: #909399;
  line-height: 1.4;
}

/* 自定义卡片样式 */
.app-card {
  border-radius: 8px;
  border: 1px solid #ebeef5;
  box-shadow: 0 2px 12px 0 rgba(0,0,0,.05);
}

/* 自定义卡片标题样式 */
.app-card-title {
  font-size: 16px;
  font-weight: 600;
  color: #303133;
  margin: 0 0 15px 0;
}
</style> 