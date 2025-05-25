<template>
  <el-dialog
    :model-value="props.visible"
    title="链接外部智能体"
    width="600px"
    @close="handleInternalClose"
    :close-on-click-modal="false"
    :destroy-on-close="true"
  >
    <el-form @submit.prevent="submitLinkAgent" label-position="top">
      <el-form-item label="链接方式">
        <el-radio-group v-model="linkType">
          <el-radio-button label="url">通过URL链接</el-radio-button>
          <el-radio-button label="json">粘贴JSON内容链接</el-radio-button>
        </el-radio-group>
      </el-form-item>

      <el-form-item label="Agent Card URL" v-if="linkType === 'url'">
        <el-input
          v-model="agentCardUrl"
          placeholder="请输入外部 Agent Card 的URL"
          clearable
        />
      </el-form-item>

      <el-form-item label="Agent Card JSON 内容" v-if="linkType === 'json'">
        <el-input
          v-model="agentCardJsonContent"
          type="textarea"
          :rows="10"
          placeholder="请在此处粘贴外部 Agent Card 的JSON内容"
          clearable
        />
      </el-form-item>
    </el-form>

    <template #footer>
      <span class="dialog-footer">
        <el-button @click="handleCloseDialog">取消</el-button>
        <el-button
          type="primary"
          @click="submitLinkAgent"
          :loading="isLoading"
          :disabled="isSubmitDisabled"
        >
          链接智能体
        </el-button>
      </span>
    </template>

    <el-alert
      v-if="message"
      :title="messageTitle"
      :type="messageType"
      show-icon
      @close="message = ''"
      style="margin-top: 20px;"
    />
  </el-dialog>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import axios from 'axios';
import { ElMessage } from 'element-plus';

interface Props {
  visible: boolean;
}

const props = defineProps<Props>();
const emit = defineEmits(['close', 'link-success', 'update:visible']);

const linkType = ref<'url' | 'json'>('url');
const agentCardUrl = ref('');
const agentCardJsonContent = ref('');
const isLoading = ref(false);
const message = ref('');
const messageType = ref<'success' | 'error'>('success');
const messageTitle = ref('');

// CSRF token helper (same as before)
function getCookie(name: string): string | null {
  let cookieValue: string | null = null;
  if (document.cookie && document.cookie !== '') {
    const cookies = document.cookie.split(';');
    for (let i = 0; i < cookies.length; i++) {
      const cookie = cookies[i].trim();
      if (cookie.substring(0, name.length + 1) === name + '=') {
        cookieValue = decodeURIComponent(cookie.substring(name.length + 1));
        break;
      }
    }
  }
  return cookieValue;
}

const isSubmitDisabled = computed(() => {
  if (linkType.value === 'url') {
    return !agentCardUrl.value.trim();
  }
  if (linkType.value === 'json') {
    return !agentCardJsonContent.value.trim();
  }
  return true;
});

const handleInternalClose = () => {
  emit('update:visible', false);
  // emit('close'); // Keep emitting 'close' if parent component relies on it for other logic besides visibility
};

const handleCloseDialog = () => {
  // Reset form state if dialog is closed
  linkType.value = 'url';
  agentCardUrl.value = '';
  agentCardJsonContent.value = '';
  message.value = '';
  isLoading.value = false;
  handleInternalClose(); // Use the new internal close handler
  emit('close'); // Also explicitly emit 'close' for compatibility if needed
};

watch(() => props.visible, (newVal) => {
  if (newVal) {
    // Reset state when dialog becomes visible, if not already reset by destroy-on-close
    linkType.value = 'url';
    agentCardUrl.value = '';
    agentCardJsonContent.value = '';
    message.value = '';
    isLoading.value = false;
  }
});

const submitLinkAgent = async () => {
  if (isLoading.value) {
    return; // 如果已经在处理中，则阻止重复提交
  }
  isLoading.value = true;
  message.value = '';
  const csrfToken = getCookie('csrftoken');

  let payload: { card_url?: string; card_content?: string } = {};
  if (linkType.value === 'url') {
    if (!agentCardUrl.value.trim()) {
      messageTitle.value = '链接失败';
      message.value = '请输入 Agent Card URL。';
      messageType.value = 'error';
      isLoading.value = false;
      return;
    }
    payload = { card_url: agentCardUrl.value.trim() };
  } else {
    if (!agentCardJsonContent.value.trim()) {
      messageTitle.value = '链接失败';
      message.value = '请输入 Agent Card JSON 内容。';
      messageType.value = 'error';
      isLoading.value = false;
      return;
    }
    payload = { card_content: agentCardJsonContent.value.trim() };
  }

  try {
    const response = await axios.post('/api/agents/external/link/', payload, {
      headers: {
        'X-CSRFToken': csrfToken,
        'Content-Type': 'application/json',
      },
    });

    if (response.status === 201) {
      messageTitle.value = '链接成功';
      message.value = `已成功链接外部智能体: ${response.data.name}`;
      messageType.value = 'success';
      ElMessage({
        message: `外部智能体 "${response.data.name}" 链接成功!`,
        type: 'success',
        duration: 3000
      });
      emit('link-success', response.data); // Pass linked agent data to parent
      handleCloseDialog(); // Close dialog on success
    } else {
      // Should not happen if server follows REST principles (201 for create)
      // but handle defensively.
      messageTitle.value = '链接失败';
      message.value = `发生意外的响应状态: ${response.status}`;
      messageType.value = 'error';
    }
  } catch (error: any) {
    messageTitle.value = '链接失败';
    if (axios.isAxiosError(error) && error.response) {
      console.error('Link agent error response:', error.response);
      if (error.response.status === 409) { // Conflict
         message.value = error.response.data.error || '您已链接过具有相同服务URL的智能体。';
      } else {
        message.value = error.response.data.error || '未能链接外部智能体。请检查URL或JSON内容以及网络连接。';
        if (error.response.data.details) {
          message.value += ` 详情: ${JSON.stringify(error.response.data.details)}`;
        }
      }
    } else {
      console.error('Link agent error:', error);
      message.value = '发生未知错误，请稍后重试。';
    }
    messageType.value = 'error';
  } finally {
    isLoading.value = false;
  }
};

</script>

<style scoped>
.dialog-footer {
  display: flex;
  justify-content: flex-end;
}
</style> 