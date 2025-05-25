<template>
  <div class="workflow-detail-container">
    <el-page-header @back="goBack" :content="workflow ? workflow.name : '加载中...'" class="page-header" />

    <el-card v-if="isLoading" v-loading="isLoading" element-loading-text="加载中...">
      <div style="height: 200px;"></div>
    </el-card>

    <el-alert
      v-if="error"
      :title="error.title || '获取工作流详情失败'"
      type="error"
      :description="error.message || '请检查网络连接或稍后重试。'"
      show-icon
      :closable="false"
      class="error-alert"
    />

    <el-card v-if="workflow && !isLoading && !error" class="workflow-card">
      <template #header>
        <div class="card-header">
          <h2>{{ workflow.name }}</h2>
          <div class="actions">
            <el-button 
              v-if="workflow.can_edit" 
              type="primary" 
              icon="EditPen" 
              @click="editWorkflow">
              编辑
            </el-button>
            <!-- <el-button type="success" icon="VideoPlay" @click="runWorkflow">运行</el-button> -->
            <!-- <el-button v-if="workflow.can_delete" type="danger" icon="Delete" @click="confirmDelete">删除</el-button> -->
          </div>
        </div>
      </template>

      <el-descriptions :column="2" border>
        <el-descriptions-item label="ID">{{ workflow.id }}</el-descriptions-item>
        <el-descriptions-item label="类型">{{ workflow.workflow_type }}</el-descriptions-item>
        <el-descriptions-item label="创建者">{{ workflow.user_name || 'N/A' }}</el-descriptions-item>
        <el-descriptions-item label="版本">{{ workflow.version }}</el-descriptions-item>
        <el-descriptions-item label="是否公开">
          <el-tag :type="workflow.is_public ? 'success' : 'info'">
            {{ workflow.is_public ? '是' : '否' }}
          </el-tag>
        </el-descriptions-item>
        <el-descriptions-item label="创建时间">{{ formatDate(workflow.created_at) }}</el-descriptions-item>
        <el-descriptions-item label="最后更新">{{ formatDate(workflow.updated_at) }}</el-descriptions-item>
        
        <el-descriptions-item label="标签" :span="2" v-if="workflow.tags && workflow.tags.length">
           <el-tag v-for="tag in workflow.tags" :key="tag" class="tag-item">{{ tag }}</el-tag>
        </el-descriptions-item>
        <el-descriptions-item label="标签" :span="2" v-else>无</el-descriptions-item>
        
        <el-descriptions-item label="描述" :span="2">
          {{ workflow.description || '无描述' }}
        </el-descriptions-item>
      </el-descriptions>

      <el-divider content-position="left">工作流定义 (JSON)</el-divider>
      <div class="definition-json">
        <codemirror
          v-if="workflow && workflow.definition"
          :model-value="workflowDefinitionString"
          placeholder="加载工作流定义中..."
          :style="{ height: 'auto', maxHeight: '600px' }" 
          :autofocus="false"
          :indent-with-tab="true"
          :tab-size="2"
          :extensions="cmExtensions"
          :disabled="true" 
          class="readonly-codemirror"
        />
        <el-empty v-else description="无工作流定义" />
      </div>
    </el-card>
  </div>
</template>

<script setup>
import { ref, onMounted, computed, shallowRef } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElCard, ElPageHeader, ElDescriptions, ElDescriptionsItem, ElTag, ElButton, ElAlert, ElDivider, ElMessage, ElEmpty } from 'element-plus';
import { EditPen, VideoPlay, Delete } from '@element-plus/icons-vue';
import api from '@/src/api';

// Codemirror imports
import { Codemirror } from 'vue-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark'; // Optional: choose a theme
import { EditorView } from '@codemirror/view'; // For readonly theme

const route = useRoute();
const router = useRouter();

const workflow = ref(null);
const isLoading = ref(false);
const error = ref(null); // Can be an object { title: '', message: '' }

// Codemirror setup for readonly instance
const cmExtensions = shallowRef([
  json(),
  oneDark, // Use your preferred theme
  EditorView.lineWrapping, // Enable line wrapping
  EditorView.editable.of(false) // Make editor readonly
]);

const workflowId = computed(() => route.params.id);

// Computed property to safely stringify the definition for Codemirror
const workflowDefinitionString = computed(() => {
  if (workflow.value && workflow.value.definition) {
    try {
      return JSON.stringify(workflow.value.definition, null, 2);
    } catch (e) {
      console.error("Error stringifying workflow definition:", e);
      return "// 无法解析工作流定义";
    }
  } 
  return ''; // Return empty string if no definition
});

const fetchWorkflowDetail = async () => {
  if (!workflowId.value) {
    error.value = { title: '错误', message: '未提供工作流ID。' };
    return;
  }
  isLoading.value = true;
  error.value = null;
  try {
    const response = await api.getWorkflow(workflowId.value);
    workflow.value = response.data;
  } catch (err) {
    console.error("Error fetching workflow detail:", err);
    if (err.response && err.response.status === 404) {
      error.value = { title: '未找到', message: `ID为 ${workflowId.value} 的工作流不存在。` };
    } else {
      error.value = { title: '加载失败', message: '获取工作流详情时发生错误，请检查网络或稍后重试。' };
    }
  }
  finally {
    isLoading.value = false;
  }
};

const formatDate = (dateString) => {
  if (!dateString) return 'N/A';
  try {
    const date = new Date(dateString);
    return date.toLocaleString('zh-CN', { hour12: false });
  } catch (e) {
    return dateString;
  }
};

const goBack = () => {
  router.push('/workflow'); // Navigate back to the workflow list
};

const editWorkflow = () => {
  if (workflow.value && workflow.value.id) {
    router.push(`/workflow/${workflow.value.id}/edit`);
  }
};

// Placeholder for future actions
// const runWorkflow = () => { ElMessage.info('运行功能待实现'); };
// const confirmDelete = () => { ElMessage.info('删除功能待实现'); };

onMounted(() => {
  fetchWorkflowDetail();
});

</script>

<style scoped>
.workflow-detail-container {
  padding: 20px;
}
.page-header {
  margin-bottom: 20px;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.actions .el-button {
  margin-left: 10px;
}
.error-alert {
  margin-bottom: 20px;
}
.workflow-card {
  margin-top: 20px;
}
.definition-json {
  margin-top: 15px;
}

/* Remove default pre styles if they conflict */
/* .definition-json pre { ... } */ 

.readonly-codemirror :deep(.cm-editor) {
  border: 1px solid #e9e9eb;
  border-radius: 4px;
  background-color: #f4f4f5; /* Match pre background */
}

.readonly-codemirror :deep(.cm-scroller) {
  font-family: Menlo, Monaco, Consolas, "Courier New", monospace;
  font-size: 13px;
}

.tag-item {
  margin-right: 5px;
  margin-bottom: 5px;
}
</style> 