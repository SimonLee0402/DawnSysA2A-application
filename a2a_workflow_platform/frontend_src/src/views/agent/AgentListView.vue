<template>
  <div class="agent-list-view">
    <el-card class="box-card">
      <template #header>
        <div class="card-header">
          <span>智能体管理</span>
          <div>
            <el-button type="success" @click="openLinkAgentDialog" :icon="UploadFilled" style="margin-right: 10px;">链接外部智能体</el-button>
            <el-button type="primary" @click="handleCreateAgent" :icon="Plus">新建智能体</el-button>
          </div>
        </div>
      </template>

      <el-skeleton :rows="5" animated v-if="store.loading.fetchAgents" />

      <el-table :data="store.agents" style="width: 100%" v-if="!store.loading.fetchAgents && store.agents.length > 0">
        <el-table-column prop="name" label="名称" sortable />
        <el-table-column prop="agent_type" label="类型" width="120" />
        <el-table-column prop="model_name" label="模型" width="180" />
        <el-table-column label="状态" width="100">
          <template #default="{ row }">
            <el-tag :type="row.is_active ? 'success' : 'info'">{{ row.is_active ? '活跃' : '离线' }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="description" label="描述" show-overflow-tooltip />
        <el-table-column prop="owner_username" label="创建者" width="120" />
         <el-table-column prop="updated_at" label="更新时间" width="180">
          <template #default="{ row }">
            {{ formatDateTime(row.updated_at) }}
          </template>
        </el-table-column>
        <el-table-column label="操作" width="280" fixed="right">
          <template #default="{ row }">
            <el-button size="small" type="primary" :icon="View" @click="handleViewAgent(row.id)">详情</el-button>
            <el-button size="small" type="info" :icon="DocumentCopy" @click="handleViewAgentCard(row)">查看Card</el-button>
            <el-button size="small" :icon="Edit" @click="handleEditAgent(row)" :disabled="!canModify(row)">编辑</el-button>
            <el-button size="small" type="danger" :icon="Delete" @click="handleDeleteAgent(row.id)" :disabled="!canModify(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>

      <el-empty description="暂无智能体数据" v-if="!store.loading.fetchAgents && store.agents.length === 0 && !store.error.fetchAgents" />
      <el-alert
        title="加载失败"
        type="error"
        :description="store.error.fetchAgents || '获取智能体列表时发生错误，请稍后重试。'"
        show-icon
        v-if="store.error.fetchAgents"
        style="margin-top: 20px;"
      />
    </el-card>

    <AgentFormModal 
      v-if="isModalVisible"
      :visible="isModalVisible" 
      :agent-data="selectedAgentForEdit" 
      @close="closeModal" 
    />

    <LinkExternalAgentDialog
      :visible="isLinkAgentDialogVisible"
      @close="closeLinkAgentDialog"
      @link-success="handleLinkAgentSuccess"
    />

    <el-dialog
      v-model="agentCardModalVisible"
      title="Agent Card"
      width="60%"
      @close="closeAgentCardModal"
      top="5vh" 
      append-to-body
    >
      <div v-if="currentAgentCardContent" class="agent-card-content">
        <pre>{{ formattedAgentCard }}</pre>
      </div>
      <div v-else>
        <p>Agent Card 内容为空或加载失败。</p>
        <p>请确保智能体数据中包含 'a2a_card_content' 字段。</p>
      </div>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="copyAgentCardToClipboard" :disabled="!currentAgentCardContent" :icon="CopyDocument">复制JSON</el-button>
          <el-button type="primary" @click="downloadAgentCard" :disabled="!currentAgentCardContent" :icon="Download">下载 .json</el-button>
          <el-button @click="closeAgentCardModal">关闭</el-button>
        </span>
      </template>
    </el-dialog>

  </div>
</template>

<script setup>
import { ref, onMounted, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useAgentStore } from '@/store/agentStore';
import { useAuthStore } from '@/store/auth';
import AgentFormModal from '@/components/agent/AgentFormModal.vue';
import LinkExternalAgentDialog from '@/components/agent/LinkExternalAgentDialog.vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import { Plus, Edit, Delete, View, DocumentCopy, Download, CopyDocument, UploadFilled } from '@element-plus/icons-vue';
import { formatDateTime } from '@/src/utils/formatters';

const store = useAgentStore();
const authStore = useAuthStore();
const router = useRouter();

const isModalVisible = ref(false);
const selectedAgentForEdit = ref(null);

const isLinkAgentDialogVisible = ref(false);

const agentCardModalVisible = ref(false);
const currentAgentCardContent = ref(null);
const currentAgentNameForDownload = ref('');

onMounted(() => {
  store.fetchAgents();
});

const formattedAgentCard = computed(() => {
  if (currentAgentCardContent.value) {
    try {
      return JSON.stringify(currentAgentCardContent.value, null, 2);
    } catch (e) {
      console.error("Error stringifying agent card content:", e);
      return "Error: Could not format Agent Card content.";
    }
  }
  return '';
});

const canModify = (agent) => {
  return authStore.user && authStore.user.id === agent.owner_id;
};

const handleCreateAgent = () => {
  selectedAgentForEdit.value = null;
  isModalVisible.value = true;
};

const handleEditAgent = (agent) => {
  if (!canModify(agent)) {
    ElMessage.error('您没有权限编辑此智能体。');
    return;
  }
  selectedAgentForEdit.value = { ...agent };
  isModalVisible.value = true;
};

const handleDeleteAgent = async (agentId) => {
  const agentToDelete = store.agents.find(a => a.id === agentId);
  if (!canModify(agentToDelete)) {
    ElMessage.error('您没有权限删除此智能体。');
    return;
  }
  try {
    await ElMessageBox.confirm(
      `确定要删除智能体 "${agentToDelete.name}" 吗？此操作无法撤销。`,
      '确认删除',
      {
        confirmButtonText: '删除',
        cancelButtonText: '取消',
        type: 'warning',
      }
    );
    await store.deleteAgent(agentId);
    ElMessage.success('智能体删除成功。');
  } catch (error) {
    if (error !== 'cancel') {
    } else {
      ElMessage.info('删除操作已取消。');
    }
  }
};

const handleViewAgent = (agentId) => {
  router.push({ name: 'AgentDetail', params: { id: agentId } });
};

const closeModal = () => {
  isModalVisible.value = false;
  selectedAgentForEdit.value = null;
};

const handleViewAgentCard = (agentRow) => {
  if (agentRow && agentRow.a2a_card_content) {
    currentAgentCardContent.value = agentRow.a2a_card_content;
    currentAgentNameForDownload.value = agentRow.name || 'agent_card';
    agentCardModalVisible.value = true;
  } else {
    ElMessage.error('未能获取Agent Card内容。请确保数据已正确加载。');
    console.warn('Agent row or a2a_card_content is missing:', agentRow);
  }
};

const closeAgentCardModal = () => {
  agentCardModalVisible.value = false;
  currentAgentCardContent.value = null;
  currentAgentNameForDownload.value = '';
};

const copyAgentCardToClipboard = async () => {
  if (!formattedAgentCard.value) {
    ElMessage.warning('没有内容可复制。');
    return;
  }
  try {
    await navigator.clipboard.writeText(formattedAgentCard.value);
    ElMessage.success('Agent Card JSON已复制到剪贴板！');
  } catch (err) {
    console.error('无法复制文本: ', err);
    ElMessage.error('复制失败，请检查浏览器权限或手动复制。');
  }
};

const downloadAgentCard = () => {
  if (!formattedAgentCard.value) {
    ElMessage.warning('没有内容可下载。');
    return;
  }
  const blob = new Blob([formattedAgentCard.value], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  const filenameSafeName = currentAgentNameForDownload.value.replace(/[^a-z0-9_\-\.]/gi, '_');
  link.download = `${filenameSafeName}_card.json`;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
  ElMessage.success('Agent Card已开始下载！');
};

const openLinkAgentDialog = () => {
  isLinkAgentDialogVisible.value = true;
};

const closeLinkAgentDialog = () => {
  isLinkAgentDialogVisible.value = false;
};

const handleLinkAgentSuccess = (linkedAgentData) => {
  ElMessage.info(`已成功与外部智能体 "${linkedAgentData.name}" 建立链接。`);
  closeLinkAgentDialog();
};

</script>

<style scoped>
.agent-list-view {
  padding: 20px;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.agent-card-content pre {
  white-space: pre-wrap;
  white-space: -moz-pre-wrap;
  white-space: -pre-wrap;
  white-space: -o-pre-wrap;
  word-wrap: break-word;
  background-color: #f5f5f5;
  padding: 15px;
  border-radius: 4px;
  max-height: 60vh;
  overflow-y: auto;
}
.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style> 