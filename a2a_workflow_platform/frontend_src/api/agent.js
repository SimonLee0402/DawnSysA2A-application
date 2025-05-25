import { apiClient } from './api-bridge.js'; // 直接导入 apiClient 对象
import { useStore as useNotificationStore } from "@/store/notification.js"
import { useAuthStore } from "@/store/auth.js"

// 获取通知存储和认证存储
const notificationStore = useNotificationStore()
const authStore = useAuthStore()

// const apiClient = getApiClient(); // Remove this line

const API_URL_PREFIX = 'agents'; // REMOVED /api/ - Matches backend agents.api.urls

// 智能体相关API
export const agentApi = {
  // Fetch all agents (uses AgentCardSerializer)
  getAgents(params = {}) {
    // params can include 'my_agents=true'
    return apiClient.get(`${API_URL_PREFIX}/`, { params });
  },

  // Fetch a single agent's details (uses AgentCardSerializer)
  getAgentById(agentId) {
    return apiClient.get(`${API_URL_PREFIX}/${agentId}/`);
  },

  // Create a new agent (uses AgentSerializer)
  createAgent(agentData) {
    // agentData should match AgentSerializer fields for writing
    return apiClient.post(`${API_URL_PREFIX}/`, agentData);
  },

  // Update an existing agent (uses AgentSerializer)
  updateAgent(agentId, agentData) {
    return apiClient.put(`${API_URL_PREFIX}/${agentId}/`, agentData);
  },

  // Partially update an existing agent (uses AgentSerializer)
  patchAgent(agentId, agentData) {
    return apiClient.patch(`${API_URL_PREFIX}/${agentId}/`, agentData);
  },

  // Delete an agent
  deleteAgent(agentId) {
    return apiClient.delete(`${API_URL_PREFIX}/${agentId}/`);
  },

  // Link a knowledge base to an agent
  linkKnowledgeBase(agentId, knowledgeBaseId) {
    return apiClient.post(`${API_URL_PREFIX}/${agentId}/link-knowledgebase/`, {
      knowledge_base_id: knowledgeBaseId,
    });
  },

  // Unlink a knowledge base from an agent
  unlinkKnowledgeBase(agentId, knowledgeBaseId) {
    return apiClient.post(`${API_URL_PREFIX}/${agentId}/unlink-knowledgebase/`, {
      knowledge_base_id: knowledgeBaseId,
    });
  },

  // Get knowledge bases available for linking (for the current user)
  getAvailableKnowledgeBasesForLinking() {
    // Corresponds to 'available-knowledgebases-for-linking' action in AgentViewSet
    return apiClient.get(`${API_URL_PREFIX}/available-knowledgebases-for-linking/`);
  },
  
  // Get workflows related to an agent
  getAgentWorkflows(agentId) {
    return apiClient.get(`${API_URL_PREFIX}/${agentId}/workflows/`);
  },

  // List all available tools for agents
  listAvailableTools() {
    // Corresponds to /api/agents/tools/available/
    return apiClient.get(`${API_URL_PREFIX}/tools/available/`); // This will become 'agents/tools/available/'
  },

  // 测试智能体连接
  testAgentConnection: async (id) => {
    try {
      // 注意：如果后端的 test_connection 是嵌套路由，路径可能需要是 /agents/${id}/test_connection/
      // 根据 a2a_client/urls.py, AgentViewSet 的 action test_connection 默认会是 /agents/${id}/test_connection/
      const response = await apiClient.post(`${API_URL_PREFIX}/${id}/test_connection/`)
      notificationStore.success('测试成功', '智能体连接测试成功')
      return response.data
    } catch (error) {
      console.error('测试智能体连接失败', error)
      notificationStore.error('测试失败', '智能体连接测试失败')
      throw error
    }
  },

  // 获取智能体凭证
  getAgentCredentials: async (id) => {
    try {
      // 假设凭证API是 /api/agents/${id}/credentials/
      // 这需要后端 AgentViewSet 有一个 'credentials' 的 action 或嵌套路由
      const response = await apiClient.get(`${API_URL_PREFIX}/${id}/credentials/`)
      return response.data
    } catch (error) {
      console.error('获取智能体凭证失败', error)
      notificationStore.error('获取失败', '无法获取智能体凭证')
      throw error
    }
  },

  // 更新智能体凭证
  updateAgentCredentials: async (id, credentials) => {
    try {
      const response = await apiClient.put(`${API_URL_PREFIX}/${id}/credentials/`, credentials)
      notificationStore.success('更新成功', '智能体凭证已成功更新')
      return response.data
    } catch (error) {
      console.error('更新智能体凭证失败', error)
      notificationStore.error('更新失败', '无法更新智能体凭证')
      throw error
    }
  }
}

// export default agentApi // Removed default export 