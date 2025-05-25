<template>
  <div class="agent-import-card">
    <h3>通过URL导入Agent Card</h3>
    <p>输入Agent Card的URL (通常以 /.well-known/agent.json 结尾) 来快速添加一个新的Agent。</p>
    
    <div class="import-controls">
      <input 
        type="url" 
        v-model="agentCardUrl" 
        placeholder="例如: https://example.com/.well-known/agent.json"
        :disabled="isLoading"
      />
      <button @click="handleImport" :disabled="isLoading || !agentCardUrl.trim()">
        {{ isLoading ? '导入中...' : '导入' }}
      </button>
    </div>
    
    <div v-if="message" :class="['message', messageType]">
      {{ message }}
    </div>
  </div>
</template>

<script lang="ts">
import { defineComponent, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useStore } from 'vuex'; // Or your preferred state management
import axios from 'axios'; // Import axios

interface AxiosErrorResponse {
  response?: {
    data?: {
      error?: string;
      detail?: string | Record<string, any>;
    };
    status?: number;
  };
  message?: string;
}

export default defineComponent({
  name: 'AgentImportCard',
  setup(_, { emit }) {
    const router = useRouter();
    const store = useStore(); // If needed for auth tokens or global state

    const agentCardUrl = ref('');
    const isLoading = ref(false);
    const message = ref('');
    const messageType = ref<'success' | 'error'>('error');

    // Helper function to get CSRF token if not using a global API setup
    const getCookie = (name: string) : string | null => {
        let cookieValue: string | null = null;
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
    };

    const handleImport = async () => {
      if (!agentCardUrl.value.trim()) {
        message.value = '请输入Agent Card的URL。';
        messageType.value = 'error';
        return;
      }

      isLoading.value = true;
      message.value = '';
      messageType.value = 'error';

      try {
        const csrfToken = getCookie('csrftoken');
        const headers: Record<string, string> = {
            'Content-Type': 'application/json',
        };
        if (csrfToken) {
            headers['X-CSRFToken'] = csrfToken;
        }
        // Add Authorization header if needed from store
        // if (store.state.user && store.state.user.token) {
        //     headers['Authorization'] = `Bearer ${store.state.user.token}`;
        // }

        const response = await axios.post('/api/agents/import/', 
            { agent_card_url: agentCardUrl.value },
            { headers: headers }
        );
        
        const newAgent = response.data;

        message.value = `Agent "${newAgent.name || '新Agent'}" 导入成功!`;
        messageType.value = 'success';
        agentCardUrl.value = ''; // Clear input

        emit('agent-imported', newAgent); 

      } catch (err) {
        const error = err as AxiosErrorResponse;
        console.error('导入Agent Card失败:', error);
        if (error.response && error.response.data) {
            const data = error.response.data;
            message.value = `导入失败: ${data.error || (typeof data.detail === 'string' ? data.detail : JSON.stringify(data.detail))}`;
        } else if (error.message) {
            message.value = `导入失败: ${error.message}`;
        } else {
            message.value = '导入失败: 发生未知错误，请检查URL或网络连接。';
        }
        messageType.value = 'error';
      } finally {
        isLoading.value = false;
      }
    };

    return {
      agentCardUrl,
      isLoading,
      message,
      messageType,
      handleImport,
    };
  },
});
</script>

<style scoped>
.agent-import-card {
  padding: 20px;
  border: 1px solid #ddd;
  border-radius: 8px;
  background-color: #f9f9f9;
  max-width: 600px;
  margin: 20px auto;
}

.agent-import-card h3 {
  margin-top: 0;
  color: #333;
}

.agent-import-card p {
  font-size: 0.9em;
  color: #666;
  margin-bottom: 15px;
}

.import-controls {
  display: flex;
  gap: 10px;
  margin-bottom: 15px;
}

.import-controls input[type="url"] {
  flex-grow: 1;
  padding: 10px;
  border: 1px solid #ccc;
  border-radius: 4px;
  font-size: 1em;
}

.import-controls button {
  padding: 10px 15px;
  font-size: 1em;
  color: white;
  background-color: #007bff;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: background-color 0.2s;
}

.import-controls button:hover {
  background-color: #0056b3;
}

.import-controls button:disabled {
  background-color: #ccc;
  cursor: not-allowed;
}

.message {
  padding: 10px;
  border-radius: 4px;
  margin-top: 15px;
  font-size: 0.9em;
}

.message.success {
  background-color: #d4edda;
  color: #155724;
  border: 1px solid #c3e6cb;
}

.message.error {
  background-color: #f8d7da;
  color: #721c24;
  border: 1px solid #f5c6cb;
}
</style> 