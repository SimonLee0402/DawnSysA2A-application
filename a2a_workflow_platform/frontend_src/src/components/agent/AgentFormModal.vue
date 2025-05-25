<template>
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
      <!-- Existing code -->
    </el-col>
  </el-row>
</template>

<script setup>
import { ref, watch, computed, onMounted } from 'vue';
import { ElMessage } from 'element-plus';
import { useStore } from 'vuex';

const props = defineProps({
  agentData: {
    type: Object,
    default: () => ({})
  }
});

const store = useStore();
const agentFormRef = ref(null);
const formState = ref({
  name: '',
  description: '',
  agent_type: 'gpt', // Default to a common type
  custom_agent_type_name: '', // Added for custom agent type
  model_name: '',
  is_active: true,
});
const activeTab = ref('basic');

const initialFormState = {
  name: '',
  description: '',
  agent_type: 'gpt', // Default to a common type
  custom_agent_type_name: '', // Added for custom agent type
  model_name: '',
  is_active: true,
};

const isEditMode = computed(() => !!props.agentData && !!props.agentData.id);

// Define known types for easier checking globally in script setup
const PRESET_AGENT_TYPES = Object.freeze(['gpt', 'claude', 'gemini', 'llama', 'ernie', 'qwen']);

const rules = {
  name: [{ required: true, message: '请输入智能体名称', trigger: 'blur' }],
  service_url: [
    { required: true, message: '请输入服务URL', trigger: 'blur' },
    { type: 'url', message: '请输入有效的URL', trigger: ['blur', 'change'] },
  ],
  // agent_type rule might need adjustment if custom name is validated separately
  // For now, the main select prop is agent_type
  agent_type: [{ required: true, message: '请选择或输入智能体类型', trigger: 'change' }], 
  a2a_version: [{ required: true, message: '请输入A2A版本', trigger: 'blur' }],
  // Add more rules as needed for other fields
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
        // It's not in known types, so it must be a custom one
        displayAgentType = 'custom';
        customName = newData.agent_type;
      }
    } else {
      // If newData.agent_type is empty or null, use default
      displayAgentType = initialFormState.agent_type; 
    }

    formState.value = {
      name: newData.name || '',
      description: newData.description || '',
      agent_type: displayAgentType,
      custom_agent_type_name: customName,
      model_name: newData.model_name || '',
      is_active: newData.is_active === undefined ? true : newData.is_active,
      provider_info: newData.provider_info ? JSON.parse(JSON.stringify(newData.provider_info)) : {},
      service_url: newData.service_url || '',
      is_a2a_compliant: newData.is_a2a_compliant === undefined ? true : newData.is_a2a_compliant,
      capabilities: newData.capabilities ? JSON.parse(JSON.stringify(newData.capabilities)) : {},
      authentication_schemes: newData.authentication_schemes ? JSON.parse(JSON.stringify(newData.authentication_schemes)) : [],
      a2a_version: newData.a2a_version || '1.0',
      available_tools: newData.available_tools ? [...newData.available_tools] : [],
    };
  } else {
    formState.value = { ...initialFormState };
  }
  activeTab.value = 'basic'; // Reset to first tab
}, { immediate: true, deep: true }); // Added deep:true just in case, though not strictly necessary for top-level prop change

const availableToolsForSelect = computed(() => store.availableTools);

// Function to handle agent_type change, if needed for reactivity (e.g., clearing custom_agent_type_name)
const handleAgentTypeChange = (newType) => {
  if (newType !== 'custom') {
    formState.value.custom_agent_type_name = '';
  }
};

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
    // Trigger validation for all fields, including potentially custom_agent_type_name if visible
    const valid = await agentFormRef.value.validate(); 
    if (valid) {
      let success = false;
      const payload = { ...formState.value };

      if (payload.agent_type === 'custom') {
        if (!payload.custom_agent_type_name || payload.custom_agent_type_name.trim() === '') {
          // Manually trigger validation error display for custom_agent_type_name if possible or just show message
          ElMessage.error('选择了自定义类型，请输入自定义类型名称。');
          // Optionally, find a way to target the custom_agent_type_name input for error display
          // agentFormRef.value.validateField('custom_agent_type_name_if_prop_exists') 
          return; 
        }
        payload.agent_type = payload.custom_agent_type_name.trim();
      }
      delete payload.custom_agent_type_name; // Not part of the backend model

      // Ensure JSON fields are objects/arrays, not strings if user edited them that way somehow
      // ... existing code ...
    }
  } catch (error) {
    console.error('Error submitting form:', error);
  }
};
</script> 