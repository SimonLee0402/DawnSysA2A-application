<template>
  <div class="p-4">
    <div class="bg-white rounded-lg shadow p-6">
      <div class="flex justify-between items-center mb-6">
        <h1 class="text-2xl font-semibold">知识库管理</h1>
        <el-button type="primary" @click="showCreateModal = true" :loading="kbStore.loading.saveKb" :icon="Plus">创建新知识库</el-button>
      </div>

      <div v-if="kbStore.isListLoading" class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div v-for="i in 3" :key="i" class="bg-white shadow-md rounded-lg p-4">
          <el-skeleton animated>
            <template #template>
              <el-skeleton-item variant="p" style="width: 60%; margin-bottom: 10px; height: 24px;" />
              <el-skeleton-item variant="text" style="margin-bottom: 6px;"/>
              <el-skeleton-item variant="text" style="width: 80%; margin-bottom: 20px;" />
              <div class="flex justify-start items-center mt-4 space-x-2">
                <el-skeleton-item variant="button" style="width: 32px; height: 28px;"/>
                <el-skeleton-item variant="button" style="width: 32px; height: 28px; "/>
                <el-skeleton-item variant="button" style="width: 32px; height: 28px; "/>
              </div>
            </template>
          </el-skeleton>
        </div>
      </div>

      <div v-else-if="kbStore.error.list" class="text-center py-10">
        <el-empty description="加载知识库列表失败" :image-size="100">
          <p class="text-red-500 mb-4">{{ kbStore.error.list.message || '未知错误' }}</p>
          <el-button type="primary" @click="kbStore.fetchKnowledgeBases()" :loading="kbStore.isListLoading">重试</el-button>
        </el-empty>
      </div>

      <div v-else-if="kbStore.knowledgeBases.length === 0" class="text-center py-10">
        <el-empty description="暂无知识库" :image-size="100">
          <p class="text-gray-500 mb-4">点击上方按钮创建一个新的知识库吧！</p>
        </el-empty>
      </div>

      <div v-else class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <div
          v-for="kb in kbStore.knowledgeBases"
          :key="kb.id"
          class="bg-white shadow-md rounded-lg p-4 hover:shadow-lg transition-shadow flex flex-col justify-between border border-gray-200"
        >
          <div>
            <div class="flex items-center mb-2">
              <div class="text-xl font-semibold mr-2">{{ kb.name }}</div>
              <el-tag v-if="kb.category && kb.category.name" type="info" size="small">{{ kb.category.name }}</el-tag>
              <el-tag v-else type="info" size="small" effect="plain">通用</el-tag>
            </div>
            <p class="text-gray-600 mb-3 text-sm min-h-10 overflow-hidden text-ellipsis">{{ kb.description || '暂无描述' }}</p>
          </div>
          <div>
            <div class="text-xs text-gray-400 mb-2 flex items-center flex-wrap">
              <span class="inline-flex items-center mr-3 mb-1">
                <el-icon :size="14" class="mr-1"><Calendar /></el-icon>
                <span>创建: {{ formatDate(kb.created_at) }}</span>
              </span>
              <span class="inline-flex items-center mr-3 mb-1">
                <el-icon :size="14" class="mr-1"><Calendar /></el-icon>
                <span>更新: {{ formatDate(kb.updated_at) }}</span>
              </span>
              <span class="inline-flex items-center mr-3 mb-1" :title="kb.is_public ? '公开知识库' : '私有知识库'">
                <el-icon :size="14" class="mr-1">
                  <View v-if="kb.is_public" />
                  <Lock v-else />
                </el-icon>
                <span>{{ kb.is_public ? '公开' : '私有' }}</span>
              </span>
            </div>
            <hr class="my-2 border-gray-200">
            <div class="flex justify-start space-x-2 mt-1">
              <el-button size="small" type="primary" link :icon="FolderOpened" @click="navigateToDetail(kb.id)" title="管理文档">管理</el-button>
              <el-button size="small" type="primary" link :icon="Edit" @click="openEditModal(kb)" :loading="kbStore.loading.saveKb && editingKnowledgeBase?.id === kb.id" title="编辑">编辑</el-button>
              <el-button size="small" type="danger" plain :icon="Delete" @click="handleDelete(kb.id)" :loading="kbStore.loading.deleteKb && deletingKbId === kb.id" title="删除"></el-button>
            </div>
          </div>
        </div>
      </div>

      <KnowledgeBaseFormModal
        :show="showCreateModal || !!editingKnowledgeBase"
        :knowledge-base="editingKnowledgeBase"
        @close="closeModal"
      />
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useKnowledgeBaseStore } from '@/store/knowledgebaseStore';
import KnowledgeBaseFormModal from '@/components/knowledgebase/KnowledgeBaseFormModal.vue';
import { ElButton, ElSkeleton, ElSkeletonItem, ElEmpty, ElIcon, ElTag } from 'element-plus';
import { FolderOpened, Edit, Delete, Calendar, Plus, Files, View, Lock } from '@element-plus/icons-vue';

const router = useRouter();
const kbStore = useKnowledgeBaseStore();

const showCreateModal = ref(false);
const editingKnowledgeBase = ref(null);
const deletingKbId = ref(null);

onMounted(() => {
  kbStore.fetchKnowledgeBases();
});

const navigateToDetail = (id) => {
  router.push({ name: 'KnowledgeBaseDetail', params: { id } });
};

const openEditModal = (kb) => {
  editingKnowledgeBase.value = { ...kb };
  showCreateModal.value = true;
};

const closeModal = () => {
  showCreateModal.value = false;
  editingKnowledgeBase.value = null;
};

const handleDelete = async (id) => {
  deletingKbId.value = id;
  try {
    await kbStore.deleteKnowledgeBase(id);
  } catch (err) {
    console.warn('Component: Delete knowledge base failed, error handled in store.', err);
  } finally {
    deletingKbId.value = null;
  }
};

const formatDate = (dateString) => {
  if (!dateString) return 'N/A';
  const date = new Date(dateString);
  const year = date.getFullYear();
  const month = (date.getMonth() + 1).toString().padStart(2, '0');
  const day = date.getDate().toString().padStart(2, '0');
  return `${year}-${month}-${day}`;
};
</script>

<style scoped>

.text-ellipsis {
  display: -webkit-box;
  -line-clamp: 2;
  -webkit-box-orient: vertical;  
  overflow: hidden;
  text-overflow: ellipsis;
}
/* Add any additional styles for the new panel structure if needed */
</style> 