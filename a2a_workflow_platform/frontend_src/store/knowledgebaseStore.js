import { defineStore } from 'pinia';
import knowledgeBaseApi from '../api/knowledgebase.js'; // 路径相对于 store 目录
import { ElMessage, ElMessageBox } from 'element-plus';

export const useKnowledgeBaseStore = defineStore('knowledgebase', {
  state: () => ({
    knowledgeBases: [],
    currentKnowledgeBase: null,
    documents: [],
    loading: {
      list: false,
      detail: false,
      documents: false,
      upload: false,
      deleteKb: false,
      deleteDoc: false,
      saveKb: false,
      searching: false,
    },
    error: {
      list: null,
      detail: null,
      documents: null,
      upload: null,
      search: null,
    },
    searchResults: [],
  }),
  actions: {
    async fetchKnowledgeBases() {
      this.loading.list = true;
      this.error.list = null;
      try {
        const data = await knowledgeBaseApi.getKnowledgeBases();
        this.knowledgeBases = data.results || data;
      } catch (err) {
        this.error.list = err;
        ElMessage.error('加载知识库列表失败: ' + (err.message || '请稍后再试'));
        console.error('Pinia: Failed to fetch knowledge bases', err);
      } finally {
        this.loading.list = false;
      }
    },

    async fetchKnowledgeBaseDetail(id) {
      this.loading.detail = true;
      this.error.detail = null;
      this.currentKnowledgeBase = null;
      try {
        const data = await knowledgeBaseApi.getKnowledgeBaseDetail(id);
        this.currentKnowledgeBase = data;
      } catch (err) {
        this.error.detail = err;
        ElMessage.error('加载知识库详情失败: ' + (err.response?.data?.detail || err.message));
        console.error('Pinia: Failed to fetch knowledge base detail', err);
      } finally {
        this.loading.detail = false;
      }
    },

    async createKnowledgeBase(kbData) {
      this.loading.saveKb = true;
      try {
        const newKb = await knowledgeBaseApi.createKnowledgeBase(kbData);
        ElMessage.success('知识库创建成功！');
        await this.fetchKnowledgeBases(); // 重新获取列表以包含新创建的
        return newKb;
      } catch (err) {
        ElMessage.error('创建知识库失败: ' + (err.response?.data?.detail || err.message || '操作失败'));
        console.error('Pinia: Failed to create knowledge base', err);
        throw err; // 重新抛出错误，让组件可以捕获
      } finally {
        this.loading.saveKb = false;
      }
    },

    async updateKnowledgeBase(id, kbData) {
      this.loading.saveKb = true;
      try {
        const updatedKb = await knowledgeBaseApi.updateKnowledgeBase(id, kbData);
        ElMessage.success('知识库更新成功！');
        await this.fetchKnowledgeBases(); // 更新列表中的对应项，或者直接重新获取
        if (this.currentKnowledgeBase && this.currentKnowledgeBase.id === id) {
          this.currentKnowledgeBase = updatedKb;
        }
        this.error.documents = null;
        return updatedKb;
      } catch (err) {
        ElMessage.error('更新知识库失败: ' + (err.response?.data?.detail || err.message || '操作失败'));
        console.error('Pinia: Failed to update knowledge base', err);
        throw err;
      } finally {
        this.loading.saveKb = false;
      }
    },

    async deleteKnowledgeBase(id) {
      try {
        await ElMessageBox.confirm(
          '确定要删除这个知识库吗？其下的所有文档也将被删除，此操作不可恢复。',
          '确认删除',
          { confirmButtonText: '确定删除', cancelButtonText: '取消', type: 'warning' }
        );
        this.loading.deleteKb = true;
        await knowledgeBaseApi.deleteKnowledgeBase(id);
        ElMessage.success('知识库删除成功！');
        await this.fetchKnowledgeBases();
        if (this.currentKnowledgeBase && this.currentKnowledgeBase.id === id) {
          this.currentKnowledgeBase = null; // 如果删除的是当前查看的，清空它
          this.documents = []; // 清空文档列表
        }
        this.error.documents = null;
      } catch (actionOrError) {
        if (typeof actionOrError === 'string' && actionOrError === 'cancel') {
          ElMessage.info('已取消删除操作');
        } else {
          ElMessage.error('删除知识库失败: ' + (actionOrError.response?.data?.detail || actionOrError.message || '操作失败'));
          console.error('Pinia: Failed to delete knowledge base', actionOrError);
          throw actionOrError;
        }
      } finally {
        this.loading.deleteKb = false;
      }
    },

    async fetchDocuments(knowledgeBaseId) {
      if (!knowledgeBaseId) {
        this.documents = [];
        return;
      }
      this.loading.documents = true;
      this.error.documents = null;
      try {
        const data = await knowledgeBaseApi.getDocuments(knowledgeBaseId);
        this.documents = data.results || data;
      } catch (err) {
        this.error.documents = err;
        ElMessage.error('加载文档列表失败: ' + (err.response?.data?.detail || err.message));
        console.error('Pinia: Failed to fetch documents', err);
      } finally {
        this.loading.documents = false;
      }
    },

    async uploadDocument(knowledgeBaseId, file) { // 修改为一次上传一个文件，方便细粒度控制
      this.loading.upload = true;
      this.error.upload = null;
      try {
        const formData = new FormData();
        formData.append('file', file);
        const newDoc = await knowledgeBaseApi.uploadDocument(knowledgeBaseId, formData);
        // ElMessage.success(`文档 "${file.name}" 上传成功！`); // 由组件处理批量后的总消息
        await this.fetchDocuments(knowledgeBaseId); // 上传成功后刷新文档列表
        return newDoc;
      } catch (err) {
        this.error.upload = err;
        // ElMessage.error(`文档 "${file.name}" 上传失败: ` + (err.response?.data?.detail || err.message));
        console.error('Pinia: Failed to upload document', err);
        throw err; // 允许组件捕获并处理
      } finally {
        this.loading.upload = false;
      }
    },

    async deleteDocument(knowledgeBaseId, documentId) {
      try {
        await ElMessageBox.confirm(
          '确定要删除这个文档吗？此操作不可恢复。',
          '确认删除文档',
          { confirmButtonText: '确定删除', cancelButtonText: '取消', type: 'warning' }
        );
        this.loading.deleteDoc = true;
        await knowledgeBaseApi.deleteDocument(knowledgeBaseId, documentId);
        ElMessage.success('文档删除成功！');
        await this.fetchDocuments(knowledgeBaseId);
      } catch (actionOrError) {
        if (typeof actionOrError === 'string' && actionOrError === 'cancel') {
          ElMessage.info('已取消删除操作');
        } else {
          ElMessage.error('删除文档失败: ' + (actionOrError.response?.data?.detail || actionOrError.message));
          console.error('Pinia: Failed to delete document', actionOrError);
          throw actionOrError;
        }
      } finally {
        this.loading.deleteDoc = false;
      }
    },

    clearCurrentKnowledgeBase() {
      this.currentKnowledgeBase = null;
      this.documents = [];
      this.searchResults = [];
      this.error.detail = null;
      this.error.documents = null;
      this.error.search = null;
    },

    async performSearch(knowledgeBaseId, query) {
      if (!knowledgeBaseId || !query.trim()) {
        this.searchResults = [];
        return;
      }
      this.loading.searching = true;
      this.error.search = null;
      try {
        const results = await knowledgeBaseApi.searchDocumentsInKnowledgeBase(knowledgeBaseId, query.trim());
        this.searchResults = results.results || results;
        if (!this.searchResults.length) {
          ElMessage.info('没有找到匹配的文档。');
        }
      } catch (err) {
        this.error.search = err;
        ElMessage.error('搜索失败: ' + (err.response?.data?.detail || err.message));
        console.error('Pinia: Failed to search documents', err);
        this.searchResults = [];
      } finally {
        this.loading.searching = false;
      }
    },

    clearSearchResults() {
      this.searchResults = [];
    }
  },
  getters: {
    getKnowledgeBaseById: (state) => (id) => {
      return state.knowledgeBases.find(kb => kb.id === id);
    },
    // 可以添加更多getters，例如 isLoading, hasError等组合状态
    isListLoading: (state) => state.loading.list,
    isDetailLoading: (state) => state.loading.detail,
    isDocumentsLoading: (state) => state.loading.documents,
  }
}); 