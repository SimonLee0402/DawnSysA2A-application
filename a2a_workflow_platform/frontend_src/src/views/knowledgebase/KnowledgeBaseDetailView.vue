<template>
  <div class="p-4">
    <div class="bg-white rounded-lg shadow p-6">
      <div v-if="kbStore.isDetailLoading || kbStore.isDocumentsLoading">
        <el-skeleton animated>
          <template #template>
            <div class="flex justify-between items-center mb-6">
              <div>
                <el-skeleton-item variant="h1" style="width: 300px; height: 36px; margin-bottom: 8px;" />
                <el-skeleton-item variant="text" style="width: 400px;" />
              </div>
              <el-skeleton-item variant="button" style="width: 100px; height: 32px;" />
            </div>

            <div class="documents-section mt-8">
              <el-skeleton-item variant="h2" style="width: 200px; height: 28px; margin-bottom: 16px;" />
              
              <div class="mb-6 p-4 border rounded-lg bg-gray-50">
                <el-skeleton-item variant="h3" style="width: 150px; height: 24px; margin-bottom: 12px;" />
                <div class="flex items-center">
                  <el-skeleton-item variant="button" style="width: 100px; height: 32px;" />
                  <el-skeleton-item variant="button" style="width: 120px; height: 32px; margin-left: 12px;" />
                </div>
                <el-skeleton-item variant="text" style="margin-top: 8px; width: 250px;" />
              </div>

              <ul class="space-y-3">
                <li v-for="i in 3" :key="i" class="bg-white shadow p-4 rounded-lg flex justify-between items-center">
                  <div>
                    <el-skeleton-item variant="text" style="width: 200px; margin-bottom: 6px;"/>
                    <el-skeleton-item variant="text" style="width: 100px; margin-bottom: 4px;" />
                    <el-skeleton-item variant="text" style="width: 150px;" />
                  </div>
                  <el-skeleton-item variant="button" style="width: 60px; height: 28px;" />
                </li>
              </ul>
            </div>
          </template>
        </el-skeleton>
      </div>
      <div v-else-if="kbStore.error.detail || kbStore.error.documents" class="text-center py-10">
        <el-empty v-if="kbStore.error.detail" description="加载知识库详情失败" :image-size="100">
          <p class="text-red-500 mb-4">{{ kbStore.error.detail.message }}</p>
          <el-button type="primary" @click="retryFetchDetail" :loading="kbStore.isDetailLoading">重试加载详情</el-button>
          <router-link to="/knowledgebases" class="ml-2 el-button el-button--default">返回列表</router-link>
        </el-empty>
        <el-empty v-else-if="kbStore.error.documents" description="加载文档列表失败" :image-size="100">
          <p class="text-red-500 mb-4">{{ kbStore.error.documents.message }}</p>
          <el-button type="primary" @click="retryFetchDocuments" :loading="kbStore.isDocumentsLoading">重试加载文档</el-button>
          <router-link to="/knowledgebases" class="ml-2 el-button el-button--default">返回列表</router-link>
        </el-empty>
      </div>

      <div v-else-if="kbStore.currentKnowledgeBase">
        <div class="flex justify-between items-center mb-6">
          <div>
            <h1 class="text-3xl font-semibold">{{ kbStore.currentKnowledgeBase.name }}</h1>
            <p class="text-gray-600 mt-1">{{ kbStore.currentKnowledgeBase.description || '暂无描述' }}</p>
          </div>
          <el-button :icon="ArrowLeft" @click="() => router.push('/knowledgebases')">返回列表</el-button>
        </div>

        <!-- Search Section -->
        <div class="search-section my-8 p-4 border rounded-lg bg-gray-50">
          <h2 class="text-xl font-semibold mb-3">在知识库中搜索</h2>
          <el-input 
            v-model="searchQuery"
            placeholder="输入关键词搜索文档内容..."
            clearable
            @keyup.enter="handleSearch"
            class="mb-3"
          >
            <template #append>
              <el-button @click="handleSearch" :loading="kbStore.loading.searching">搜索</el-button>
            </template>
          </el-input>
          <div v-if="kbStore.error.search" class="text-red-500 text-sm">
            搜索时发生错误: {{ kbStore.error.search.message || '请稍后再试' }}
          </div>
        </div>

        <!-- Search Results Section -->
        <div v-if="searchQuery && !kbStore.loading.searching" class="search-results-section mb-8">
          <h3 class="text-xl font-semibold mb-3">搜索结果 ({{ kbStore.searchResults.length }})</h3>
          <div v-if="kbStore.searchResults.length === 0" class="text-center py-6">
            <el-empty description="没有找到与您的查询匹配的文档。" :image-size="80">
              <p class="text-gray-500">请尝试其他关键词。</p>
            </el-empty>
          </div>
          <ul v-else class="space-y-3">
            <li 
              v-for="doc in kbStore.searchResults"
              :key="doc.id + '-search'" 
              class="bg-blue-50 shadow p-4 rounded-lg border border-blue-200 hover:shadow-md transition-shadow"
            >
              <p class="font-medium text-blue-800">{{ doc.name || doc.file_name || '未命名文档' }}</p>
              <p class="text-sm text-gray-600 mt-1">类型: {{ doc.file_type }}, 状态: {{ doc.status }}</p>
              <p class="text-sm text-gray-500">上传于: {{ new Date(doc.uploaded_at).toLocaleString() }}</p>
              <p v-if="doc.extracted_text" class="text-xs text-gray-700 mt-2 p-2 bg-gray-100 rounded">
                <span class="font-semibold">匹配内容片段 (模拟):</span> 
                {{ snippet(doc.extracted_text, searchQuery, 150) }}
              </p>
            </li>
          </ul>
        </div>

        <div class="documents-section mt-8">
          <h2 class="text-2xl font-semibold mb-4">文档列表</h2>
          <div class="mb-6 p-4 border rounded-lg bg-gray-50">
            <h3 class="text-lg font-medium mb-3">上传新文档</h3>
            <el-upload
              ref="uploadRef"
              v-model:file-list="filesToUploadForElUpload"
              action="#"
              :auto-upload="false"
              :multiple="true"
              :on-change="handleElUploadChange"
              accept=".txt,.md,.pdf,.doc,.docx"
              class="mb-3"
            >
              <template #trigger>
                <el-button type="primary">选择文件</el-button>
              </template>
              <el-button class="ml-3" type="success" @click="submitUpload" :disabled="filesToUpload.length === 0 || kbStore.loading.upload">
                <span v-if="kbStore.loading.upload">上传中... ({{ uploadingProgress }})</span>
                <span v-else>上传到服务器</span>
              </el-button>
              <template #tip>
                <div class="el-upload__tip">可以将文件拖到此处，或点击"选择文件"按钮进行上传。</div>
              </template>
            </el-upload>
            <p v-if="uploadComponentError" class="text-red-500 mt-2">{{ uploadComponentError }}</p>
          </div>

          <div v-if="kbStore.documents.length === 0 && !kbStore.isDocumentsLoading && !kbStore.error.documents" class="text-center py-6">
            <el-empty description="此知识库中暂无文档" :image-size="80">
              <p class="text-gray-500">尝试上传一些文件来填充它吧！</p>
            </el-empty>
          </div>

          <ul v-else-if="kbStore.documents.length > 0" class="space-y-3">
            <li 
              v-for="doc in kbStore.documents"
              :key="doc.id"
              class="bg-white shadow p-4 rounded-lg hover:shadow-md transition-shadow border border-gray-200"
            >
              <div class="flex-grow">
                <p class="font-medium text-gray-800">{{ doc.name || doc.file_name || '未命名文档' }}</p>
                <p class="text-sm text-gray-500">
                  状态: <el-tag :type="statusTagType(doc.status)" size="small">{{ doc.status }}</el-tag>
                </p>
                <p class="text-sm text-gray-500">上传于: {{ new Date(doc.uploaded_at).toLocaleString() }}</p>
                <p v-if="doc.error_message" class="text-xs text-red-500 mt-1">
                  <span class="font-semibold">处理错误:</span> {{ doc.error_message }}
                </p>
                <p v-if="doc.status === 'COMPLETED' && doc.extracted_text" class="text-xs text-gray-600 mt-1 italic bg-gray-50 p-1 rounded">
                  <span class="font-semibold">提取内容预览:</span> {{ doc.extracted_text.substring(0, 100) }}{{ doc.extracted_text.length > 100 ? '...' : '' }}
                </p>
                <p v-if="doc.status === 'COMPLETED' && !doc.extracted_text && !doc.error_message" class="text-xs text-gray-500 mt-1 italic">
                  提取内容为空。
                </p>
              </div>
              <el-button 
                type="danger" 
                :icon="DeleteIcon" 
                size="small" 
                @click="handleDeleteDocument(doc.id)" 
                :loading="kbStore.loading.deleteDoc && deletingDocId === doc.id" 
                title="删除文档"
                class="ml-4 flex-shrink-0"
              ></el-button>
            </li>
          </ul>
        </div>
      </div>
      <div v-else class="text-center text-xl text-gray-500 py-10">
        <el-empty description="未找到知识库或加载失败" :image-size="100">
          <p class="text-gray-500 mb-4">请检查知识库ID是否正确或稍后再试。</p>
          <router-link to="/knowledgebases" class="el-button el-button--primary">返回知识库列表</router-link>
        </el-empty>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, computed, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useKnowledgeBaseStore } from '@/store/knowledgebaseStore';
import { ElUpload, ElButton, ElMessage, ElSkeleton, ElSkeletonItem, ElEmpty, ElIcon, ElTag } from 'element-plus'; // ElMessageBox is used in store actions
import { Delete as DeleteIcon, ArrowLeft } from '@element-plus/icons-vue'; // Import Delete icon
import _ from 'lodash'; // Import lodash for debounce

const route = useRoute();
const router = useRouter();
const kbStore = useKnowledgeBaseStore();

const uploadRef = ref(null);
const knowledgeBaseId = computed(() => route.params.id);

const filesToUpload = ref([]); 
const filesToUploadForElUpload = ref([]);
const uploadComponentError = ref(null); // Specific error for the upload component UI
const uploadingProgress = ref('');
const deletingDocId = ref(null);

const ALLOWED_FILE_TYPES = ['text/plain', 'text/markdown', 'application/pdf', 'application/msword', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'];
const MAX_FILE_SIZE_MB = 10;
const MAX_FILE_SIZE_BYTES = MAX_FILE_SIZE_MB * 1024 * 1024;

const searchQuery = ref('');

const debouncedSearch = _.debounce(async (newQuery) => {
  if (knowledgeBaseId.value) {
    if (newQuery && newQuery.trim() !== '') {
      await kbStore.performSearch(knowledgeBaseId.value, newQuery.trim());
    } else {
      kbStore.clearSearchResults(); // Assuming kbStore has a method to clear search results
                                  // or set kbStore.searchResults = []; directly if appropriate
    }
  }
}, 500); // 500ms debounce time

watch(searchQuery, (newQuery, oldQuery) => {
  if (newQuery !== oldQuery) {
    debouncedSearch(newQuery);
  }
});

const statusTagType = (status) => {
  switch (status) {
    case 'COMPLETED': return 'success';
    case 'PROCESSING': return 'primary';
    case 'PENDING': return 'info';
    case 'FAILED': return 'danger';
    default: return 'info';
  }
};

onMounted(() => {
  if (knowledgeBaseId.value) {
    kbStore.fetchKnowledgeBaseDetail(knowledgeBaseId.value);
    kbStore.fetchDocuments(knowledgeBaseId.value);
  } else {
    router.push('/knowledgebases'); // or a 404 page
    ElMessage.error('无效的知识库ID');
  }
});

watch(knowledgeBaseId, (newId, oldId) => {
  if (newId && newId !== oldId) {
    kbStore.clearCurrentKnowledgeBase(); // Clear old data before fetching new
    kbStore.fetchKnowledgeBaseDetail(newId);
    kbStore.fetchDocuments(newId);
    searchQuery.value = ''; // Reset search query when KB changes
  }
});

const handleElUploadChange = (uploadFile, currentUploadFiles) => {
  // filesToUploadForElUpload is automatically updated by v-model
  // We perform validation here and filter the list for our internal 'filesToUpload' ref
  const validFiles = [];
  const newRawFiles = [];
  
  for (const uFile of currentUploadFiles) {
    const rawFile = uFile.raw;
    if (rawFile instanceof File) { // Ensure it's a real file, not just a placeholder
      if (!ALLOWED_FILE_TYPES.includes(rawFile.type)) {
        ElMessage.warning(`文件 "${rawFile.name}" 类型 (${rawFile.type || '未知'}) 不受支持。仅支持 .txt, .md, .pdf, .doc, .docx。`);
        // Optionally remove from el-upload's list immediately if possible, or wait for user to remove
        // uploadRef.value?.handleRemove(uFile); // Might cause issues if done while iterating
        continue; // Skip this file
      }
      if (rawFile.size > MAX_FILE_SIZE_BYTES) {
        ElMessage.warning(`文件 "${rawFile.name}" 大小超过 ${MAX_FILE_SIZE_MB}MB 限制。`);
        // uploadRef.value?.handleRemove(uFile);
        continue; // Skip this file
      }
      validFiles.push(uFile); // Keep the ElUploadFile object
      newRawFiles.push(rawFile); // Keep the raw file for actual upload
    }
  }
  
  // Update the list used by el-upload to reflect only valid files if we removed some
  // This direct manipulation can be tricky with el-upload's internal state.
  // A safer way might be to let el-upload manage its list and we just show errors,
  // and our submitUpload function only processes files from filesToUpload.value.
  // For now, filesToUploadForElUpload is bound, so we just update our internal list.
  
  filesToUpload.value = newRawFiles;
  uploadComponentError.value = null;
  
  // If you want to strictly control el-upload's list:
  // filesToUploadForElUpload.value = validFiles;
  // This might be necessary if you want to prevent even showing invalid files in el-upload's list.
  // However, simple warnings are often sufficient, and the user can manually remove them.
};

const submitUpload = async () => {
  if (filesToUpload.value.length === 0) {
    // Check if filesToUploadForElUpload has files that were deemed invalid
    if (filesToUploadForElUpload.value.length > 0) {
        ElMessage.warning('请先移除无效文件或选择有效文件后再上传。');
    } else {
        ElMessage.warning('请先选择要上传的文件。');
    }
    return;
  }
  uploadComponentError.value = null;
  let successCount = 0;
  const totalFiles = filesToUpload.value.length;
  
  // Use a copy of the array for iteration, as filesToUpload might be cleared by clearFiles
  const filesToProcess = [...filesToUpload.value]; 

  for (let i = 0; i < filesToProcess.length; i++) {
    const file = filesToProcess[i];
    uploadingProgress.value = `${i + 1}/${totalFiles}`;
    try {
      await kbStore.uploadDocument(knowledgeBaseId.value, file);
      successCount++;
    } catch (err) {
      // Error for individual file already logged in store, 
      // but we might want a general message or to stop further uploads.
      uploadComponentError.value = `上传 "${file.name}" 失败: ${err.message || '未知错误'}`;
      ElMessage.error(`上传 "${file.name}" 失败。`);
      // Decide if you want to stop on first error or try all files
      // break; 
    }
  }
  uploadingProgress.value = '';

  if (uploadRef.value) {
    uploadRef.value.clearFiles();
  }
  filesToUpload.value = [];
  // filesToUploadForElUpload is bound to el-upload's list, clearFiles should handle it.

  if (successCount > 0) {
    ElMessage.success(`${successCount} / ${totalFiles} 个文件上传成功！`);
  }
  if (successCount < totalFiles && !uploadComponentError.value) { // Show general warning if no specific file error was set
    ElMessage.warning(`${totalFiles - successCount} 个文件上传遇到问题。`);
  }
  // fetchDocuments is called within the store's uploadDocument action upon success
};

const handleDeleteDocument = async (documentId) => {
  deletingDocId.value = documentId;
  try {
    await kbStore.deleteDocument(knowledgeBaseId.value, documentId);
  } catch (err) {
    // Error handled in store
    console.warn('Component: Delete document failed, error handled in store.', err);
  } finally {
    deletingDocId.value = null;
  }
};

const retryFetchDetail = () => {
  if (knowledgeBaseId.value) {
    kbStore.fetchKnowledgeBaseDetail(knowledgeBaseId.value);
    // Optionally, also retry documents if detail was the primary failure cause
    // kbStore.fetchDocuments(knowledgeBaseId.value); 
  }
};

const retryFetchDocuments = () => {
  if (knowledgeBaseId.value) {
    kbStore.fetchDocuments(knowledgeBaseId.value);
  }
};

const handleSearch = () => {
  // This can now be used to immediately trigger the debounced search if needed,
  // for example, if the user presses Enter or clicks a search button.
  debouncedSearch.flush(); 
  // Original logic for empty query can be kept or handled by the watcher/debouncedSearch
  // if (!searchQuery.value.trim()) {
  //   kbStore.searchResults = []; 
  // }
};

// Helper function to generate a snippet (can be improved)
const snippet = (text, query, maxLength = 150) => {
  if (!text) return '无内容可预览。';
  const queryLower = query.toLowerCase();
  const textLower = text.toLowerCase();
  let startIndex = textLower.indexOf(queryLower);
  
  if (startIndex === -1) {
    return text.length > maxLength ? text.substring(0, maxLength) + '...' : text;
  }
  
  let start = Math.max(0, startIndex - Math.floor((maxLength - query.length) / 2));
  let end = Math.min(text.length, start + maxLength);
  
  let result = text.substring(start, end);
  if (start > 0) result = '...' + result;
  if (end < text.length) result = result + '...';
  
  // Basic highlighting (can be replaced with a more robust solution or v-html with caution)
  // This is a simple text replacement, not DOM manipulation.
  const regex = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi'); // Escape regex special chars
  // return result.replace(regex, `<mark>${query}</mark>`); // Avoid v-html for now
  return result;
};

</script>

<style scoped>
/* max-width: 900px; Remove if using full-width panel
  margin: auto; */
.el-upload__tip {
  font-size: 12px;
  color: #606266;
  margin-top: 7px;
}
/* Add any additional styles for the new panel structure if needed */
.text-red-500.mb-4 + .el-button + .el-button--default {
    margin-left: 8px; /* Ensure spacing for router-link styled as button in error state */
}
</style> 