import { getApiClient } from './api-bridge.js';

// 知识库相关API
const knowledgeBaseApi = {
  getKnowledgeBases: async (params = {}) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.get('/knowledgebase/knowledgebases/', { params });
      return response.data;
    } catch (error) {
      console.error('获取知识库列表失败', error);
      // 这里可以根据需要添加通知
      throw error;
    }
  },

  createKnowledgeBase: async (data) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.post('/knowledgebase/knowledgebases/', data);
      // 这里可以根据需要添加成功通知
      return response.data;
    } catch (error) {
      console.error('创建知识库失败', error);
      throw error;
    }
  },

  getKnowledgeBaseDetail: async (id) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.get(`/knowledgebase/knowledgebases/${id}/`);
      return response.data;
    } catch (error) {
      console.error(`获取知识库详情 (ID: ${id}) 失败`, error);
      throw error;
    }
  },

  updateKnowledgeBase: async (id, data) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.put(`/knowledgebase/knowledgebases/${id}/`, data);
      // 这里可以根据需要添加成功通知
      return response.data;
    } catch (error) {
      console.error(`更新知识库 (ID: ${id}) 失败`, error);
      throw error;
    }
  },

  deleteKnowledgeBase: async (id) => {
    const apiClient = await getApiClient();
    try {
      await apiClient.delete(`/knowledgebase/knowledgebases/${id}/`);
      // 这里可以根据需要添加成功通知
      return true;
    } catch (error) {
      console.error(`删除知识库 (ID: ${id}) 失败`, error);
      throw error;
    }
  },

  getDocuments: async (knowledgeBaseId, params = {}) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.get(`/knowledgebase/knowledgebases/${knowledgeBaseId}/documents/`, { params });
      return response.data;
    } catch (error) {
      console.error(`获取知识库 (ID: ${knowledgeBaseId}) 的文档列表失败`, error);
      throw error;
    }
  },

  uploadDocument: async (knowledgeBaseId, formData) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.post(`/knowledgebase/knowledgebases/${knowledgeBaseId}/upload_document/`, formData, {
        headers: {
          'Content-Type': 'multipart/form-data',
        },
      });
      // 这里可以根据需要添加成功通知
      return response.data;
    } catch (error) {
      console.error(`向知识库 (ID: ${knowledgeBaseId}) 上传文档失败`, error);
      throw error;
    }
  },

  deleteDocument: async (knowledgeBaseId, documentId) => {
    const apiClient = await getApiClient();
    try {
      await apiClient.delete(`/knowledgebase/knowledgebases/${knowledgeBaseId}/documents/${documentId}/`);
      // 这里可以根据需要添加成功通知
      return true;
    } catch (error) {
      console.error(`删除知识库 (ID: ${knowledgeBaseId}) 中的文档 (ID: ${documentId}) 失败`, error);
      throw error;
    }
  },

  searchDocumentsInKnowledgeBase: async (knowledgeBaseId, query) => {
    const apiClient = await getApiClient();
    try {
      const response = await apiClient.post(`/knowledgebase/knowledgebases/${knowledgeBaseId}/search/`, { query });
      return response.data;
    } catch (error) {
      console.error(`在知识库 (ID: ${knowledgeBaseId}) 中搜索文档失败，查询: "${query}"`, error);
      throw error;
    }
  }
};

export default knowledgeBaseApi; 