<template>
  <el-dialog
    :model-value="visible"
    :title="isEditMode ? '编辑智能体' : '新建智能体'"
    width="60%"
    @close="handleClose"
    :close-on-click-modal="false"
  >
    <el-form
      ref="agentFormRef"
      :model="formState"
      :rules="rules"
      label-width="120px"
      v-loading="store.loading.createAgent || store.loading.updateAgent"
    >
      <el-tabs v-model="activeTab">
        <el-tab-pane label="基本信息" name="basic">
          <el-form-item label="名称" prop="name">
            <el-input v-model="formState.name" placeholder="请输入智能体名称" />
          </el-form-item>
          <el-form-item label="描述" prop="description">
            <el-input type="textarea" :rows="3" v-model="formState.description" placeholder="请输入智能体描述" />
          </el-form-item>
          <el-row :gutter="20">
            <el-col :span="12">
              <el-form-item label="类型" prop="agent_type">
                <el-select v-model="formState.agent_type" placeholder="请选择类型" style="width: 100%;" @change="handleAgentTypeChange">
                  <el-option label="GPT 系列 (例如: OpenAI)" value="gpt"></el-option>
                  <el-option label="Claude 系列 (例如: Anthropic)" value="claude"></el-option>
                  <el-option label="Gemini 系列 (例如: Google)" value="gemini"></el-option>
                  <el-option label="Llama 系列 (例如: Meta)" value="llama"></el-option>
                  <el-option label="文心千帆 (例如: Baidu)" value="ernie"></el-option>
                  <el-option label="通义千问 (例如: Alibaba)" value="qwen"></el-option>
                  <el-option label="自定义" value="custom"></el-option>
                </el-select>
                <el-input
                  v-if="formState.agent_type === 'custom'"
                  v-model="formState.custom_agent_type_name"
                  placeholder="请输入自定义类型名称"
                  style="margin-top: 10px;"
                  clearable
                />
              </el-form-item>
            </el-col>
            <el-col :span="12">
              <el-form-item label="模型名称" prop="model_name">
                <el-input v-model="formState.model_name" placeholder="例如: gpt-4, claude-3-opus" />
              </el-form-item>
            </el-col>
          </el-row>
          <el-form-item label="服务URL" prop="service_url">
            <el-input v-model="formState.service_url" placeholder="智能体A2A服务终结点URL" />
          </el-form-item>
          <el-form-item label="A2A版本" prop="a2a_version">
            <el-input v-model="formState.a2a_version" placeholder="例如: 1.0" />
          </el-form-item>
          <el-form-item label="激活状态" prop="is_active">
            <el-switch v-model="formState.is_active" />
          </el-form-item>
          <el-form-item label="A2A兼容" prop="is_a2a_compliant">
            <el-switch v-model="formState.is_a2a_compliant" />
          </el-form-item>
        </el-tab-pane>

        <el-tab-pane label="提供商与能力" name="provider_capabilities">
          <el-form-item label="提供商组织" prop="provider_info_organization">
            <el-input v-model="formState.provider_info_organization" placeholder="例如: OpenAI, Google" />
          </el-form-item>
          <el-form-item label="提供商URL" prop="provider_info_url">
            <el-input v-model="formState.provider_info_url" placeholder="例如: https://openai.com" />
            <small>提供商的官方网站或相关链接。</small>
          </el-form-item>
          
          <el-form-item label="流式响应" prop="capability_streaming">
            <el-checkbox v-model="formState.capability_streaming">启用流式响应</el-checkbox>
            <small>智能体是否支持逐步返回结果。</small>
          </el-form-item>
          <el-form-item label="允许工具使用" prop="capability_tool_usage">
            <el-checkbox v-model="formState.capability_tool_usage">启用工具调用</el-checkbox>
            <small>智能体是否能够使用或被外部工具调用。</small>
          </el-form-item>
        </el-tab-pane>

        <el-tab-pane label="认证与工具" name="auth_tools">
          <el-form-item label="认证方式">
            <el-checkbox v-model="formState.auth_use_custom_json">使用自定义JSON配置认证方案</el-checkbox>
          </el-form-item>

          <template v-if="formState.auth_use_custom_json">
            <el-form-item label="认证方案" prop="authentication_schemes">
              <VueJsonEditor v-model="formState.authentication_schemes" :show-btns="false" :expanded-on-start="true" mode="tree" style="width: 100%;" height="200px" />
              <small>JSON数组, 例如: [{"scheme": "bearer", "token": "your_token"}]</small>
            </el-form-item>
          </template>
          <template v-else>
            <el-form-item label="Bearer Token" prop="auth_bearer_token">
              <el-input v-model="formState.auth_bearer_token" placeholder="请输入Bearer Token" clearable />
              <small>如果您的智能体使用Bearer Token进行认证，请在此处填写。</small>
            </el-form-item>
          </template>

          <el-form-item label="可用工具" prop="available_tools">
             <el-select
                v-model="formState.available_tools"
                multiple
                filterable
                allow-create
                default-first-option
                placeholder="选择或输入工具名称"
                style="width: 100%;"
              >
                <el-option
                  v-for="tool in availableToolsForSelect"
                  :key="tool.name"
                  :label="`${tool.name_zh || tool.name}${tool.description_zh || tool.description ? ' (' + (tool.description_zh || tool.description) + ')' : ''}`"
                  :value="tool.name"
                />
              </el-select>
            <small>工具名称列表, 例如: ["calculator", "web_search"]</small>
          </el-form-item>
        </el-tab-pane>
      </el-tabs>
    </el-form>

    <template #footer>
      <span class="dialog-footer">
        <el-button @click="handleClose">取消</el-button>
        <el-button 
          type="primary" 
          @click="handleSubmit"
          :loading="store.loading.createAgent || store.loading.updateAgent"
        >
          {{ isEditMode ? '保存更改' : '创建智能体' }}
        </el-button>
      </span>
    </template>
  </el-dialog>
</template>

<script setup>
import { ref, watch, computed, onMounted } from 'vue';
import { useAgentStore } from '@/store/agentStore';
import { ElMessage } from 'element-plus';
import VueJsonEditor from 'vue3-ts-jsoneditor'; // Using a JSON editor component

const props = defineProps({
  visible: Boolean,
  agentData: Object, // For editing, null for creation
});

const emit = defineEmits(['close']);

const store = useAgentStore();
const agentFormRef = ref(null);
const activeTab = ref('basic');

// MODIFICATION START: Define known types for easier checking globally in script setup
const PRESET_AGENT_TYPES = Object.freeze(['gpt', 'claude', 'gemini', 'llama', 'ernie', 'qwen']);
// MODIFICATION END

const initialFormState = {
  name: '',
  description: '',
  agent_type: 'gpt', // MODIFICATION: Default to a common type
  custom_agent_type_name: '', // MODIFICATION: Added for custom agent type
  model_name: '',
  is_active: true,
  // MODIFICATION: Replace provider_info with structured fields
  provider_info_organization: '',
  provider_info_url: '',
  service_url: '',
  is_a2a_compliant: true,
  // MODIFICATION: Replace capabilities with structured fields
  capability_streaming: true, 
  capability_tool_usage: false,
  authentication_schemes: [],
  auth_bearer_token: '',        // Added for simple Bearer token input
  auth_use_custom_json: false, // Added to toggle custom JSON editor
  a2a_version: '1.0',
  available_tools: [],
};

const formState = ref({ ...initialFormState });

const isEditMode = computed(() => !!props.agentData && !!props.agentData.id);

const rules = {
  name: [{ required: true, message: '请输入智能体名称', trigger: 'blur' }],
  service_url: [
    { required: true, message: '请输入服务URL', trigger: 'blur' },
    { type: 'url', message: '请输入有效的URL', trigger: ['blur', 'change'] },
  ],
  agent_type: [{ required: true, message: '请选择或输入智能体类型', trigger: 'change' }], 
  a2a_version: [{ required: true, message: '请输入A2A版本', trigger: 'blur' }],
  // MODIFICATION: Add rules for new provider fields if necessary, e.g., URL validation for provider_info_url
  provider_info_url: [
    { type: 'url', message: '请输入有效的提供商URL', trigger: ['blur', 'change'] },
  ],
};

// Watch for agentData prop changes to populate form for editing
watch(() => props.agentData, (newData) => {
  if (newData && newData.id) {
    let displayAgentType = initialFormState.agent_type;
    let customName = '';

    if (newData.agent_type) { 
      if (PRESET_AGENT_TYPES.includes(newData.agent_type)) {
        displayAgentType = newData.agent_type;
      } else {
        displayAgentType = 'custom';
        customName = newData.agent_type;
      }
    }
    
    // MODIFICATION START: Populate new provider_info fields
    let org = '';
    let url = '';
    if (newData.provider_info && typeof newData.provider_info === 'object') {
      org = newData.provider_info.organization || '';
      url = newData.provider_info.url || '';
    }
    // MODIFICATION END

    // MODIFICATION START: Populate new capability fields
    let streaming = initialFormState.capability_streaming; // Default to initial
    let toolUsage = initialFormState.capability_tool_usage; // Default to initial
    if (newData.capabilities && typeof newData.capabilities === 'object') {
      if (typeof newData.capabilities.streaming === 'boolean') {
        streaming = newData.capabilities.streaming;
      }
      if (typeof newData.capabilities.tool_usage === 'boolean') {
        toolUsage = newData.capabilities.tool_usage;
      }
    }
    // MODIFICATION END

    // MODIFICATION START: Populate auth_bearer_token or authentication_schemes for JSON editor
    let bearerToken = '';
    let useCustomJsonAuth = false;
    let authSchemesForEditor = [];

    if (Array.isArray(newData.authentication_schemes) && newData.authentication_schemes.length === 1) {
      const firstScheme = newData.authentication_schemes[0];
      if (typeof firstScheme === 'object' && firstScheme.scheme === 'bearer' && typeof firstScheme.token === 'string') {
        bearerToken = firstScheme.token;
        useCustomJsonAuth = false;
        // Keep authSchemesForEditor as initial empty or full data if user toggles back
        authSchemesForEditor = newData.authentication_schemes ? JSON.parse(JSON.stringify(newData.authentication_schemes)) : [];
      } else {
        // It's an array but not a simple bearer token, or malformed
        authSchemesForEditor = newData.authentication_schemes ? JSON.parse(JSON.stringify(newData.authentication_schemes)) : [];
        useCustomJsonAuth = true;
      }
    } else if (newData.authentication_schemes && (!Array.isArray(newData.authentication_schemes) || newData.authentication_schemes.length > 1)) {
      // Not an array or an array with more than one scheme, or other complex cases
      authSchemesForEditor = newData.authentication_schemes ? JSON.parse(JSON.stringify(newData.authentication_schemes)) : [];
      useCustomJsonAuth = true;
    } else {
      // Null, undefined, or empty array - default to simple bearer token input mode
      useCustomJsonAuth = false;
      authSchemesForEditor = []; // Ensure it's an array for the editor if toggled
    }
    // MODIFICATION END

    formState.value = {
      name: newData.name || '',
      description: newData.description || '',
      agent_type: displayAgentType,
      custom_agent_type_name: customName,
      model_name: newData.model_name || '',
      is_active: newData.is_active === undefined ? true : newData.is_active,
      // MODIFICATION START: Use new provider_info fields
      provider_info_organization: org,
      provider_info_url: url,
      service_url: newData.service_url || '',
      is_a2a_compliant: newData.is_a2a_compliant === undefined ? true : newData.is_a2a_compliant,
      // MODIFICATION START: Use new capability fields
      capability_streaming: streaming,
      capability_tool_usage: toolUsage,
      // MODIFICATION END
      authentication_schemes: authSchemesForEditor, // For JSON editor
      auth_bearer_token: bearerToken,              // For simple input
      auth_use_custom_json: useCustomJsonAuth,    // Toggle state
      a2a_version: newData.a2a_version || '1.0',
      available_tools: newData.available_tools ? [...newData.available_tools] : [],
    };
  } else {
    formState.value = { ...initialFormState };
  }
  activeTab.value = 'basic'; // Reset to first tab
}, { immediate: true });

// MODIFICATION START: Add handler for agent_type select change
// Function to handle agent_type change, to clear custom_agent_type_name if a preset is chosen
const handleAgentTypeChange = (newType) => {
  if (newType !== 'custom') {
    formState.value.custom_agent_type_name = '';
  }
};
// MODIFICATION END

const availableToolsForSelect = computed(() => store.availableTools);

onMounted(() => {
  if (store.availableTools.length === 0) {
    store.fetchAvailableTools();
  }
});

const handleClose = () => {
  agentFormRef.value?.resetFields();
  formState.value = { ...initialFormState }; // Explicitly reset state
  // custom_agent_type_name is part of initialFormState, so it gets reset here
  activeTab.value = 'basic';
  emit('close');
};

const handleSubmit = async () => {
  if (!agentFormRef.value) return;
  try {
    const valid = await agentFormRef.value.validate();
    if (valid) {
      let success = false;
      const payload = { ...formState.value };

      if (payload.agent_type === 'custom') {
        if (!payload.custom_agent_type_name || payload.custom_agent_type_name.trim() === '') {
          ElMessage.error('选择了自定义类型，请输入自定义类型名称。');
          return; 
        }
        payload.agent_type = payload.custom_agent_type_name.trim();
      }
      delete payload.custom_agent_type_name;

      // MODIFICATION START: Assemble provider_info for payload
      const providerInfoPayload = {};
      if (payload.provider_info_organization && payload.provider_info_organization.trim() !== '') {
        providerInfoPayload.organization = payload.provider_info_organization.trim();
      }
      if (payload.provider_info_url && payload.provider_info_url.trim() !== '') {
        providerInfoPayload.url = payload.provider_info_url.trim();
      }

      if (Object.keys(providerInfoPayload).length > 0 && providerInfoPayload.organization) {
        // Only set provider_info if organization is present (A2A spec: if provider obj is present, organization is required)
        payload.provider_info = providerInfoPayload;
      } else if (Object.keys(providerInfoPayload).length > 0 && !providerInfoPayload.organization){
        ElMessage.error('提供了提供商URL但未提供组织名称。请填写组织名称或清空提供商信息。');
        return; 
      } else {
        payload.provider_info = null; // Or delete payload.provider_info; depending on backend
      }
      delete payload.provider_info_organization;
      delete payload.provider_info_url;
      // MODIFICATION END

      // MODIFICATION START: Assemble capabilities for payload
      payload.capabilities = {
        streaming: payload.capability_streaming,
        tool_usage: payload.capability_tool_usage,
      };
      delete payload.capability_streaming;
      delete payload.capability_tool_usage;
      // MODIFICATION END

      // MODIFICATION START: Assemble authentication_schemes from either bearer token or JSON editor
      if (payload.auth_use_custom_json) {
        // Ensure authentication_schemes is a valid JSON array if custom mode is used
        try {
          if (typeof payload.authentication_schemes === 'string') {
            payload.authentication_schemes = JSON.parse(payload.authentication_schemes || '[]');
          } else if (!Array.isArray(payload.authentication_schemes)) {
            // If it's some other non-array type from the editor somehow, default to empty.
            payload.authentication_schemes = [];
          }
        } catch (e) {
          ElMessage.error('认证方案 (自定义JSON) 包含无效的JSON格式。');
          activeTab.value = 'auth_tools';
          return;
        }
      } else {
        if (payload.auth_bearer_token && payload.auth_bearer_token.trim() !== '') {
          payload.authentication_schemes = [{ scheme: 'bearer', token: payload.auth_bearer_token.trim() }];
        } else {
          payload.authentication_schemes = [];
        }
      }
      delete payload.auth_bearer_token;
      delete payload.auth_use_custom_json;
      // MODIFICATION END

      // Ensure JSON fields are objects/arrays, not strings if user edited them that way somehow
      // VueJsonEditor should handle this, but a safeguard
      try {
        // This was moved to the custom JSON block above for authentication_schemes
        // if (typeof payload.authentication_schemes === 'string') payload.authentication_schemes = JSON.parse(payload.authentication_schemes || '[]');
      } catch (e) {
        ElMessage.error('高级字段 (认证方案) 包含无效的JSON格式。');
        return;
      }

      if (isEditMode.value) {
        const result = await store.updateAgent(props.agentData.id, payload);
        if (result) success = true;
      } else {
        const result = await store.createAgent(payload);
        if (result) success = true;
      }
      if (success) {
        handleClose();
      }
    } else {
      ElMessage.error('请检查表单输入。');
      return false;
    }
  } catch (error) {
    // Error already handled by store actions with ElMessage
    console.error('Form submission error:', error);
  }
};

</script>

<style scoped>
.dialog-footer {
  text-align: right;
}
/* Add styles for vue3-ts-jsoneditor if needed, or rely on its default styles */
</style> 