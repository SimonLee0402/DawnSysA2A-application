<template>
  <div class="agent-detail-view" v-if="store.currentAgent">
    <el-page-header @back="goBack" :icon="ArrowLeft">
      <template #content>
        <span class="text-large font-600 mr-3"> 智能体详情: {{ store.currentAgent.name }} </span>
      </template>
      <template #extra>
        <div class="page-header-extra">
          <el-button @click="refreshData" :icon="RefreshRight" :loading="store.loading.fetchAgentDetail">刷新</el-button>
          <el-button type="primary" @click="openEditModal" :icon="Edit" :disabled="!canModifyCurrentAgent">编辑智能体</el-button>
        </div>
      </template>
    </el-page-header>

    <el-skeleton :rows="10" animated v-if="store.loading.fetchAgentDetail && !store.currentAgent" />
    
    <el-card class="box-card agent-main-info-card" v-if="store.currentAgent">
        <el-descriptions title="基本信息" :column="2" border>
            <el-descriptions-item label="ID">{{ store.currentAgent.id }}</el-descriptions-item>
            <el-descriptions-item label="名称">{{ store.currentAgent.name }}</el-descriptions-item>
            <el-descriptions-item label="类型">{{ store.currentAgent.agent_type }}</el-descriptions-item>
            <el-descriptions-item label="模型">{{ store.currentAgent.model_name || 'N/A' }}</el-descriptions-item>
            <el-descriptions-item label="状态">
                <el-tag :type="store.currentAgent.is_active ? 'success' : 'info'">
                    {{ store.currentAgent.is_active ? '活跃' : '离线' }}
                </el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="A2A兼容">
                <el-tag :type="store.currentAgent.is_a2a_compliant ? 'success' : 'danger'">
                    {{ store.currentAgent.is_a2a_compliant ? '是' : '否' }}
                </el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="服务URL" :span="2">{{ store.currentAgent.url }}</el-descriptions-item>
            <el-descriptions-item label="A2A 版本">{{ store.currentAgent.a2aVersion }}</el-descriptions-item>
            <el-descriptions-item label="创建者">{{ store.currentAgent.owner_username || 'N/A' }}</el-descriptions-item>
            <el-descriptions-item label="创建时间">{{ formatDateTime(store.currentAgent.created_at) }}</el-descriptions-item>
            <el-descriptions-item label="最后更新">{{ formatDateTime(store.currentAgent.updated_at) }}</el-descriptions-item>
            <el-descriptions-item label="描述" :span="2">{{ store.currentAgent.description || '暂无描述' }}</el-descriptions-item>
            
            <el-descriptions-item label="会话数">
                <el-tag type="info" effect="plain">{{ store.currentAgent.session_count !== undefined ? store.currentAgent.session_count : 'N/A' }}</el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="任务数">
                 <el-tag type="info" effect="plain">{{ store.currentAgent.task_count !== undefined ? store.currentAgent.task_count : 'N/A' }}</el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="工作流实例数" :span="2">
                 <el-tag type="info" effect="plain">{{ store.currentAgent.workflow_instance_count !== undefined ? store.currentAgent.workflow_instance_count : 'N/A' }}</el-tag>
            </el-descriptions-item>
        </el-descriptions>
    </el-card>

    <el-card class="box-card info-row" v-if="store.currentAgent && store.currentAgent.skills && store.currentAgent.skills.length > 0">
        <template #header>
            <div class="card-header">
                <span>智能体技能</span>
            </div>
        </template>
        <el-collapse accordion>
            <el-collapse-item v-for="skill in store.currentAgent.skills" :key="skill.id" :title="skill.name">
                <el-descriptions :column="1" border size="small">
                    <el-descriptions-item label="技能ID">{{ skill.id }}</el-descriptions-item>
                    <el-descriptions-item label="描述">{{ skill.description || 'N/A' }}</el-descriptions-item>
                    <el-descriptions-item label="输入模式">
                        <el-tag v-for="mode in skill.inputModes" :key="mode" size="small" style="margin-right: 5px;">{{ mode }}</el-tag>
                        <span v-if="!skill.inputModes || skill.inputModes.length === 0">N/A</span>
                    </el-descriptions-item>
                    <el-descriptions-item label="输出模式">
                        <el-tag v-for="mode in skill.outputModes" :key="mode" size="small" style="margin-right: 5px;">{{ mode }}</el-tag>
                        <span v-if="!skill.outputModes || skill.outputModes.length === 0">N/A</span>
                    </el-descriptions-item>
                    <el-descriptions-item label="示例" v-if="skill.examples && skill.examples.length > 0">
                        <VueJsonEditor :modelValue="skill.examples" :show-btns="false" :expanded-on-start="false" mode="preview" />
                    </el-descriptions-item>
                </el-descriptions>
            </el-collapse-item>
        </el-collapse>
    </el-card>
    <el-alert title="无技能信息" type="info" description="此智能体未定义任何技能。" show-icon v-else-if="store.currentAgent && (!store.currentAgent.skills || store.currentAgent.skills.length === 0)" class="info-row" />

    <el-row :gutter="20" class="info-row">
      <el-col :span="12">
        <el-card class="box-card">
          <template #header>
            <div class="card-header">
              <span>提供商信息</span>
            </div>
          </template>
          <VueJsonEditor 
            :modelValue="store.currentAgent.agentProvider"
            :show-btns="false" 
            :expanded-on-start="true" 
            mode="preview" 
            style="width: 100%;"
            height="150px"
          />
        </el-card>
      </el-col>
      <el-col :span="12">
        <el-card class="box-card">
          <template #header>
            <div class="card-header">
              <span>能力</span>
            </div>
          </template>
          <VueJsonEditor 
            :modelValue="store.currentAgent.capabilities"
            :show-btns="false" 
            :expanded-on-start="true" 
            mode="preview" 
            style="width: 100%;"
            height="150px"
           />
        </el-card>
      </el-col>
    </el-row>

    <el-row :gutter="20" class="info-row">
        <el-col :span="12">
            <el-card class="box-card">
                <template #header>
                    <div class="card-header">
                        <span>认证方案</span>
                    </div>
                </template>
                <VueJsonEditor 
                    :modelValue="store.currentAgent.authentication"
                    :show-btns="false" 
                    :expanded-on-start="true" 
                    mode="preview" 
                    style="width: 100%;"
                    height="200px"
                />
            </el-card>
        </el-col>
        <el-col :span="12">
            <el-card class="box-card">
                <template #header>
                    <div class="card-header">
                        <span>可用工具</span>
                         <el-tooltip content="刷新工具列表" placement="top">
                            <el-button circle :icon="Refresh" @click="fetchAgentTools" :loading="store.loading.fetchAvailableTools" size="small"></el-button>
                        </el-tooltip>
                    </div>
                </template>
                <div v-if="store.currentAgent.available_tools && store.currentAgent.available_tools.length > 0">
                    <el-tag v-for="tool in store.currentAgent.available_tools" :key="tool" style="margin-right: 5px; margin-bottom: 5px;">
                        {{ tool }}
                    </el-tag>
                </div>
                <el-empty description="此Agent未使用任何工具" v-else />
                 <div style="margin-top: 10px; font-size: 0.9em; color: #606266;">
                    <strong>平台可用工具:</strong>
                    <div v-if="store.availableTools.length > 0">
                        <el-tag 
                            v-for="tool in store.availableTools" 
                            :key="tool.name" 
                            type="info" 
                            effect="plain"
                            style="margin-right: 5px; margin-bottom: 5px; cursor: help;"
                            :title="tool.description"
                        >
                            {{ tool.name }}
                        </el-tag>
                    </div>
                    <p v-else>暂无平台级可用工具或未加载。</p>
                </div>
            </el-card>
        </el-col>
    </el-row>

    <el-card class="box-card info-row">
        <template #header>
            <div class="card-header">
                <span>关联的知识库</span>
                <el-button type="primary" @click="showLinkKBModal = true" :icon="Connection" :disabled="!canModifyCurrentAgent">关联知识库</el-button>
            </div>
        </template>
        <el-table :data="store.currentAgentLinkedKBs" style="width: 100%" v-if="store.currentAgentLinkedKBs && store.currentAgentLinkedKBs.length > 0">
            <el-table-column prop="id" label="ID" width="300"></el-table-column>
            <el-table-column prop="name" label="名称"></el-table-column>
            <el-table-column prop="visibility" label="可见性" width="100">
                 <template #default="{ row }">
                    <el-tag :type="row.visibility === 'PUBLIC' ? 'success' : 'info'">{{ row.visibility }}</el-tag>
                </template>
            </el-table-column>
            <el-table-column label="操作" width="120">
                <template #default="{ row }">
                    <el-button size="small" type="danger" @click="handleUnlinkKB(row.id)" :icon="Scissor" :disabled="!canModifyCurrentAgent">解绑</el-button>
                </template>
            </el-table-column>
        </el-table>
        <el-empty description="此Agent未关联任何知识库" v-else />
    </el-card>

    <AgentFormModal 
      v-if="isEditModalVisible"
      :visible="isEditModalVisible" 
      :agent-data="store.currentAgent" 
      @close="closeEditModal" 
    />

    <el-dialog v-model="showLinkKBModal" title="关联知识库到智能体" width="50%">
      <el-select 
        v-model="selectedKBToLink"
        placeholder="选择一个知识库进行关联"
        filterable
        style="width: 100%; margin-bottom: 20px;"
        :loading="store.loading.fetchAvailableKBs"
      >
        <el-option
          v-for="kb in unlinkedAvailableKBs"
          :key="kb.id"
          :label="kb.name + (kb.visibility === 'PUBLIC' ? ' (公开)' : ' (私有)')"
          :value="kb.id"
        />
      </el-select>
      <div v-if="store.loading.fetchAvailableKBs">正在加载可用知识库...</div>
      <div v-if="!store.loading.fetchAvailableKBs && unlinkedAvailableKBs.length === 0">没有可关联的知识库 (您拥有或公开的)。</div>
      <template #footer>
        <el-button @click="showLinkKBModal = false">取消</el-button>
        <el-button type="primary" @click="handleLinkKB" :loading="store.loading.linkKB" :disabled="!selectedKBToLink">确认关联</el-button>
      </template>
    </el-dialog>

  </div>
  <el-empty description="未找到智能体或正在加载..." v-else-if="!store.loading.fetchAgentDetail && !store.currentAgent" />
</template>

<script setup>
import { ref, onMounted, computed, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useAgentStore } from '@/store/agentStore';
import { useAuthStore } from '@/store/auth';
import AgentFormModal from '@/components/agent/AgentFormModal.vue';
import { formatDateTime } from '@/src/utils/formatters';
import { ArrowLeft, Edit, RefreshRight, Connection, Scissor, Refresh } from '@element-plus/icons-vue';
import VueJsonEditor from 'vue3-ts-jsoneditor';
import { ElMessage } from 'element-plus';

const route = useRoute();
const router = useRouter();
const store = useAgentStore();
const authStore = useAuthStore();

const agentId = ref(route.params.id);

const isEditModalVisible = ref(false);
const showLinkKBModal = ref(false);
const selectedKBToLink = ref(null);

const fetchData = () => {
  if (agentId.value) {
    store.fetchAgentDetail(agentId.value);
    // fetchAgentTools(); // Fetch system-wide tools when component mounts
  }
};

const fetchAgentTools = () => {
    if(store.availableTools.length === 0) { // Avoid re-fetching if already loaded
        store.fetchAvailableTools();
    }
};

onMounted(() => {
  fetchData();
  fetchAgentTools();
  store.fetchAvailableKBsForLinking(); // Fetch KBs user can link
});

// Watch for route changes if navigating between detail views directly
watch(() => route.params.id, (newId) => {
  if (newId) {
    agentId.value = newId;
    store.clearCurrentAgent(); // Clear previous agent data before fetching new
    fetchData();
  } else {
    // Handle case where ID might become undefined, perhaps navigate away
    router.push({ name: 'AgentList' }); 
  }
});

const canModifyCurrentAgent = computed(() => {
  return store.currentAgent && authStore.user && authStore.user.id === store.currentAgent.owner_id;
});

const unlinkedAvailableKBs = computed(() => {
  if (!store.currentAgent || !store.currentAgentLinkedKBs) return store.availableKBsForLinking;
  const linkedKbIds = new Set(store.currentAgentLinkedKBs.map(kb => kb.id));
  return store.availableKBsForLinking.filter(kb => !linkedKbIds.has(kb.id));
});

const goBack = () => router.push({ name: 'AgentList' });
const refreshData = () => fetchData();

const openEditModal = () => {
  if (!canModifyCurrentAgent.value) {
    ElMessage.error('您没有权限编辑此智能体。');
    return;
  }
  isEditModalVisible.value = true;
};
const closeEditModal = () => {
  isEditModalVisible.value = false;
  // Optionally refresh data if needed, though store update should reflect
  // store.fetchAgentDetail(agentId.value); 
};

const handleLinkKB = async () => {
  if (!selectedKBToLink.value || !store.currentAgent) return;
  const success = await store.linkKnowledgeBaseToAgent(store.currentAgent.id, selectedKBToLink.value);
  if (success) {
    selectedKBToLink.value = null;
    showLinkKBModal.value = false;
    // Data will be refreshed by the store action upon success
  }
};

const handleUnlinkKB = async (knowledgeBaseId) => {
  if (!store.currentAgent) return;
   const success = await store.unlinkKnowledgeBaseFromAgent(store.currentAgent.id, knowledgeBaseId);
   if (success) {
    // Data will be refreshed by the store action
   }
};

</script>

<style scoped>
.agent-detail-view {
  padding: 20px;
}
.el-page-header {
  margin-bottom: 20px;
}
.page-header-extra .el-button {
  margin-left: 10px;
}
.box-card {
  margin-bottom: 20px;
}
.info-row {
  margin-bottom: 20px; /* consistent spacing for rows of cards */
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

/* Ensure JSON editor preview looks good */
:deep(.jsoneditor-vue div.jsoneditor-tree) {
  min-height: 100px; /* Adjust as needed */
}
:deep(.jsoneditor-vue div.jsoneditor-mode-preview) {
    background-color: #f9f9f9;
    border: 1px solid #ebeef5;
}
</style> 