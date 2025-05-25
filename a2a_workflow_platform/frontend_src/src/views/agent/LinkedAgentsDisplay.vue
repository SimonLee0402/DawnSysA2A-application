<template>
    <div class="linked-agents-display">
      <el-card class="box-card">
        <template #header>
          <div class="card-header">
            <span>已链接的外部智能体</span>
            <!-- Optional: Button to navigate to the link new agent dialog -->
            <!-- <el-button type="primary" @click="navigateAndOpenLinkDialog">链接新的外部智能体</el-button> -->
          </div>
        </template>
  
        <el-skeleton :rows="5" animated v-if="isLoading" />
  
        <el-table :data="linkedAgents" style="width: 100%" v-if="!isLoading && linkedAgents.length > 0">
          <el-table-column prop="name" label="名称" sortable>
            <template #default="{ row }">
              {{ row.name }} <el-tag type="info" size="small" style="margin-left: 5px;">外部</el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="service_url" label="服务URL" show-overflow-tooltip />
          <el-table-column label="状态" width="100">
            <template #default="{ row }">
              <el-tag :type="row.is_enabled ? 'success' : 'info'">{{ row.is_enabled ? '已启用' : '已禁用' }}</el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="a2a_version" label="A2A版本" width="120" />
          <el-table-column label="创建日期" width="180">
            <template #default="{ row }">
              {{ formatDateTime(row.created_at) }}
            </template>
          </el-table-column>
          <el-table-column label="操作" width="200" fixed="right">
            <template #default="{ row }">
              <el-button size="small" type="info" :icon="View" @click="handleViewDetails(row)">查看Card</el-button>
              <el-button size="small" type="danger" :icon="Delete" @click="handleUnlinkAgent(row)">取消链接</el-button>
            </template>
          </el-table-column>
        </el-table>
  
        <el-empty description="暂无已链接的外部智能体" v-if="!isLoading && linkedAgents.length === 0 && !fetchError" />
        
        <el-alert
          title="加载失败"
          type="error"
          :description="fetchError || '获取已链接的外部智能体列表时发生错误，请稍后重试。'"
          show-icon
          v-if="fetchError"
          style="margin-top: 20px;"
        />
      </el-card>
  
      <!-- Dialog to view Agent Card JSON -->
      <el-dialog v-model="detailsDialogVisible" title="外部智能体Card详情" width="60%" top="5vh">
        <div v-if="selectedAgentCardContent" class="agent-card-content">
          <pre>{{ formattedAgentCard }}</pre>
        </div>
        <div v-else>
          <p>Agent Card 内容为空或加载失败。</p>
        </div>
        <template #footer>
          <span class="dialog-footer">
            <el-button @click="detailsDialogVisible = false">关闭</el-button>
          </span>
        </template>
      </el-dialog>
    </div>
  </template>
  
  <script setup lang="ts">
  import { ref, onMounted, computed } from 'vue';
  import axios from 'axios';
  import { ElMessage, ElMessageBox } from 'element-plus';
  import { View, Delete } from '@element-plus/icons-vue';
  import { formatDateTime } from '@/utils/formatters'; // Corrected import path
  // import { useRouter } from 'vue-router'; // If needed for navigation
  
  interface LinkedAgent {
    id: string;
    name: string;
    service_url: string;
    is_enabled: boolean;
    a2a_version?: string;
    created_at: string;
    card_content?: object; // The full agent card for viewing details
    // Add other fields as necessary from LinkedExternalAgentSerializer
  }
  
  const isLoading = ref(true);
  const linkedAgents = ref<LinkedAgent[]>([]);
  const fetchError = ref<string | null>(null);
  
  const detailsDialogVisible = ref(false);
  const selectedAgentCardContent = ref<object | null>(null);
  
  // const router = useRouter(); // If navigation is added
  
  // CSRF token helper
  function getCookie(name: string): string | null {
    let cookieValue = null;
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
  
  const fetchLinkedAgents = async () => {
    isLoading.value = true;
    fetchError.value = null;
    const csrfToken = getCookie('csrftoken');
    try {
      const response = await axios.get('/api/agents/external/manage/', {
        headers: { 'X-CSRFToken': csrfToken }, // GET might not strictly need CSRF, but good practice if session auth is used
      });
      linkedAgents.value = response.data;
    } catch (error: any) {
      console.error('Failed to fetch linked agents:', error);
      if (axios.isAxiosError(error) && error.response) {
        fetchError.value = error.response.data.detail || '获取列表失败，请检查网络连接或稍后再试。';
      } else {
        fetchError.value = '获取列表时发生未知错误。';
      }
    } finally {
      isLoading.value = false;
    }
  };
  
  const formattedAgentCard = computed(() => {
    if (selectedAgentCardContent.value) {
      try {
        return JSON.stringify(selectedAgentCardContent.value, null, 2);
      } catch (e) {
        console.error("Error stringifying agent card content:", e);
        return "错误: 无法格式化Agent Card内容。";
      }
    }
    return '';
  });
  
  const handleViewDetails = (agent: LinkedAgent) => {
    if (agent.card_content) {
      selectedAgentCardContent.value = agent.card_content;
      detailsDialogVisible.value = true;
    } else {
      ElMessage.warning('此链接的智能体没有可供查看的Card JSON内容。');
    }
  };
  
  const handleUnlinkAgent = async (agent: LinkedAgent) => {
    try {
      await ElMessageBox.confirm(
        `确定要取消与外部智能体 "${agent.name}" 的链接吗？此操作无法撤销。`,
        '确认取消链接',
        {
          confirmButtonText: '取消链接',
          cancelButtonText: '保留',
          type: 'warning',
        }
      );
      const csrfToken = getCookie('csrftoken');
      await axios.delete(`/api/agents/external/manage/${agent.id}/`, {
        headers: { 'X-CSRFToken': csrfToken },
      });
      ElMessage.success(`与 "${agent.name}" 的链接已取消。`);
      fetchLinkedAgents(); // Refresh the list
    } catch (error: any) {
      if (error !== 'cancel') {
        console.error('Failed to unlink agent:', error);
        if (axios.isAxiosError(error) && error.response) {
          ElMessage.error(error.response.data.detail || '取消链接失败。');
        } else {
          ElMessage.error('取消链接时发生未知错误。');
        }
      } else {
        ElMessage.info('取消链接操作已取消。');
      }
    }
  };
  
  /* Optional: Navigation to the main linking dialog (if this view is separate)
  const navigateAndOpenLinkDialog = () => {
    // This would require AgentListView to listen for a query param or event to open the dialog
    router.push({ name: 'AgentList', query: { openLinkDialog: 'true' } }); 
  };
  */
  
  onMounted(() => {
    fetchLinkedAgents();
  });
  
  </script>
  
  <style scoped>
  .linked-agents-display {
    padding: 20px;
  }
  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .agent-card-content pre {
    white-space: pre-wrap; /* CSS3 */
    white-space: -moz-pre-wrap; /* Mozilla, since 1999 */
    white-space: -pre-wrap; /* Opera 4-6 */
    white-space: -o-pre-wrap; /* Opera 7 */
    word-wrap: break-word; /* Internet Explorer 5.5+ */
    background-color: #f5f5f5;
    padding: 15px;
    border-radius: 4px;
    max-height: 60vh;
    overflow-y: auto;
  }
  .dialog-footer {
    text-align: right;
  }
  </style>
  