<template>
  <el-dialog
    v-model="dialogVisible"
    title="导入Agent Card"
    width="600px" 
    @close="handleClose"
    :close-on-click-modal="false"
  >
    <el-form @submit.prevent="handleImport" label-position="top">
      <el-form-item label="导入方式">
        <el-radio-group v-model="importType">
          <el-radio-button label="url">通过URL导入</el-radio-button>
          <el-radio-button label="json">粘贴JSON内容</el-radio-button>
        </el-radio-group>
      </el-form-item>

      <el-form-item label="Agent Card URL" v-if="importType === 'url'">
        <el-input
          v-model="agentCardUrl"
          placeholder="请输入 Agent Card 的URL"
          clearable
        />
      </el-form-item>

      <el-form-item label="Agent Card JSON 内容" v-if="importType === 'json'">
        <el-input
          v-model="agentCardJsonContent"
          type="textarea"
          :rows="10"
          placeholder="请在此处粘贴 Agent Card 的JSON内容"
        />
      </el-form-item>
    </el-form>
    <template #footer>
      <span class="dialog-footer">
        <el-button @click="handleClose">取消</el-button>
        <el-button 
          type="primary" 
          @click="handleImport" 
          :loading="isLoading"
          :disabled="(importType === 'url' && !agentCardUrl.trim()) || (importType === 'json' && !agentCardJsonContent.trim())"
        >
          导入
        </el-button>
      </span>
    </template>
    <el-alert
      v-if="message"
      :title="messageTitle"
      :type="messageType"
      show-icon
      @close="message = ''"
      style="margin-top: 15px;"
    />
  </el-dialog>
</template>

<script setup>
import { ref, defineProps, defineEmits, watch } from 'vue';
import axios from 'axios';
import { ElMessage } from 'element-plus';

const props = defineProps({
  visible: {
    type: Boolean,
    default: false,
  },
});

const emits = defineEmits(['close', 'import-success']);

const dialogVisible = ref(props.visible);
const importType = ref('url'); // 'url' or 'json'
const agentCardUrl = ref('');
const agentCardJsonContent = ref('');
const isLoading = ref(false);
const message = ref('');
const messageType = ref('info');
const messageTitle = ref('');

watch(() => props.visible, (newVal) => {
  dialogVisible.value = newVal;
  if (newVal) {
    // Reset state when dialog opens
    importType.value = 'url';
    agentCardUrl.value = '';
    agentCardJsonContent.value = '';
    message.value = '';
    isLoading.value = false;
  }
});

function getCookie(name) {
  let cookieValue = null;
  if (document.cookie && document.cookie !== '') {
    const cookies = document.cookie.split(';');
    for (let i = 0; i < cookies.length; i++) {
      const cookie = cookies[i].trim();
      if (cookie.substring(0, name.length + 1) === (name + '=')) {
        cookieValue = decodeURIComponent(cookie.substring(name.length + 1));
        break;
      }
    }
  }
  return cookieValue;
}

const handleImport = async () => {
  isLoading.value = true;
  message.value = '';
  let payload = {};

  if (importType.value === 'url') {
    if (!agentCardUrl.value.trim()) {
      ElMessage.warning('请输入Agent Card的URL。');
      isLoading.value = false;
      return;
    }
    payload = { card_url: agentCardUrl.value };
  } else if (importType.value === 'json') {
    if (!agentCardJsonContent.value.trim()) {
      ElMessage.warning('请粘贴Agent Card的JSON内容。');
      isLoading.value = false;
      return;
    }
    try {
      // Validate if the content is valid JSON before sending
      JSON.parse(agentCardJsonContent.value);
      payload = { card_content: agentCardJsonContent.value }; 
    } catch (e) {
      ElMessage.error('粘贴的内容不是有效的JSON格式。');
      isLoading.value = false;
      return;
    }
  }

  try {
    const csrftoken = getCookie('csrftoken');
    const response = await axios.post(
      '/api/agents/import/', 
      payload,
      {
        headers: {
          'Content-Type': 'application/json',
          'X-CSRFToken': csrftoken,
        },
      }
    );

    if (response.data && response.data.id) {
      messageTitle.value = '导入成功';
      message.value = `智能体 "${response.data.name}" 已成功导入。`;
      messageType.value = 'success';
      ElMessage.success(message.value);
      emits('import-success');
      // handleClose(); // Keep dialog open to show message
    } else {
      // Prefer error message from response if available
      const errorMsg = response.data?.error || response.data?.detail || '导入失败，响应中未包含智能体ID或有效错误信息。';
      throw new Error(errorMsg);
    }
  } catch (error) {
    console.error('Error importing agent card:', error);
    messageTitle.value = '导入失败';
    let errorMessage = '导入Agent Card时发生错误。';
    if (error.response && error.response.data) {
        errorMessage = error.response.data.error || error.response.data.detail || (typeof error.response.data === 'string' ? error.response.data : errorMessage);
    } else if (error.message) {
      errorMessage = error.message;
    }
    message.value = errorMessage;
    messageType.value = 'error';
    ElMessage.error(errorMessage);
  } finally {
    isLoading.value = false;
  }
};

const handleClose = () => {
  emits('close');
};
</script>

<style scoped>
.dialog-footer {
  text-align: right;
}
</style> 