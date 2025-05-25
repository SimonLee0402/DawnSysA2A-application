import { defineStore } from 'pinia';
import { agentApi } from '../api/agent.js';
import { ElMessage, ElMessageBox } from 'element-plus';

export const useAgentStore = defineStore('agent', {
  state: () => ({
    agents: [],
    agentPagination: {
      count: 0,
      next: null,
      previous: null,
    },
    currentAgent: null,
    currentAgentSkills: [],
    currentAgentWorkflows: [],
    currentAgentLinkedKBs: [], // Store summarized KBs linked to currentAgent
    availableKBsForLinking: [], // KBs user can link
    availableTools: [], // Tools agents can use

    loading: {
      fetchAgents: false,
      fetchAgentDetail: false,
      createAgent: false,
      updateAgent: false,
      deleteAgent: false,
      linkKB: false,
      unlinkKB: false,
      fetchAvailableKBs: false,
      fetchAgentSubresources: false, // Generic for skills, workflows etc.
      fetchAvailableTools: false,
    },
    error: {
      fetchAgents: null,
      fetchAgentDetail: null,
      createAgent: null,
      updateAgent: null,
      deleteAgent: null,
      linkKB: null,
      unlinkKB: null,
      fetchAvailableKBs: null,
      fetchAgentSubresources: null,
      fetchAvailableTools: null,
    },
  }),

  actions: {
    async fetchAgents(params = {}) {
      this.loading.fetchAgents = true;
      this.error.fetchAgents = null;
      try {
        const response = await agentApi.getAgents(params);
        if (response.data && typeof response.data === 'object' && 'results' in response.data) {
          this.agents = response.data.results;
          this.agentPagination.count = response.data.count;
          this.agentPagination.next = response.data.next;
          this.agentPagination.previous = response.data.previous;
        } else if (Array.isArray(response.data)) {
          this.agents = response.data;
          this.agentPagination = { count: response.data.length, next: null, previous: null };
        } else {
          this.agents = [];
          this.agentPagination = { count: 0, next: null, previous: null };
          console.warn('Unexpected response format for fetchAgents:', response.data);
          this.error.fetchAgents = 'Failed to fetch agents: Unexpected response format';
          ElMessage.error(this.error.fetchAgents);
        }
      } catch (err) {
        this.agents = [];
        this.agentPagination = { count: 0, next: null, previous: null };
        this.error.fetchAgents = 'Failed to fetch agents';
        console.error(err);
        ElMessage.error(this.error.fetchAgents);
      } finally {
        this.loading.fetchAgents = false;
      }
    },

    async fetchAgentDetail(agentId) {
      this.loading.fetchAgentDetail = true;
      this.error.fetchAgentDetail = null;
      this.currentAgent = null;
      this.currentAgentSkills = [];
      this.currentAgentWorkflows = [];
      this.currentAgentLinkedKBs = [];
      try {
        const response = await agentApi.getAgentById(agentId);
        this.currentAgent = response.data;
        // AgentCardSerializer nests skills and linked_knowledge_bases summary
        this.currentAgentSkills = response.data.skills || [];
        this.currentAgentLinkedKBs = response.data.linked_knowledge_bases || [];
        // TODO: Fetch workflows separately if not included or if full detail is needed
        // await this.fetchAgentWorkflows(agentId); // Example
      } catch (err) {
        this.error.fetchAgentDetail = 'Failed to fetch agent details';
        console.error(err);
        ElMessage.error(this.error.fetchAgentDetail);
      } finally {
        this.loading.fetchAgentDetail = false;
      }
    },

    async createAgent(agentData) {
      this.loading.createAgent = true;
      this.error.createAgent = null;
      try {
        const response = await agentApi.createAgent(agentData);
        // Optionally re-fetch list or add to current list
        await this.fetchAgents(); 
        ElMessage.success('Agent created successfully');
        return response.data; // Return created agent
      } catch (err) {
        this.error.createAgent = 'Failed to create agent';
        console.error(err);
        ElMessage.error(this.error.createAgent + (err.response?.data?.detail ? `: ${err.response.data.detail}` : ''));
        return null;
      } finally {
        this.loading.createAgent = false;
      }
    },

    async updateAgent(agentId, agentData) {
      this.loading.updateAgent = true;
      this.error.updateAgent = null;
      try {
        const response = await agentApi.updateAgent(agentId, agentData);
        // Update in local list and currentAgent if it matches
        const index = this.agents.findIndex(a => a.id === agentId);
        if (index !== -1) {
          this.agents[index] = { ...this.agents[index], ...response.data };
        }
        if (this.currentAgent && this.currentAgent.id === agentId) {
          this.currentAgent = { ...this.currentAgent, ...response.data };
        }
        ElMessage.success('Agent updated successfully');
        return response.data;
      } catch (err) {
        this.error.updateAgent = 'Failed to update agent';
        console.error(err);
        ElMessage.error(this.error.updateAgent + (err.response?.data?.detail ? `: ${err.response.data.detail}` : ''));
        return null;
      } finally {
        this.loading.updateAgent = false;
      }
    },

    async deleteAgent(agentId) {
      try {
        await ElMessageBox.confirm(
          'Are you sure you want to delete this agent? This action cannot be undone.',
          'Confirm Deletion',
          { type: 'warning' }
        );
        this.loading.deleteAgent = true;
        this.error.deleteAgent = null;
        await agentApi.deleteAgent(agentId);
        this.agents = this.agents.filter(a => a.id !== agentId);
        if (this.currentAgent && this.currentAgent.id === agentId) {
          this.currentAgent = null;
        }
        ElMessage.success('Agent deleted successfully');
        return true;
      } catch (err) {
        if (err !== 'cancel') { // Don't show error if user cancelled confirm dialog
          this.error.deleteAgent = 'Failed to delete agent';
          console.error(err);
          ElMessage.error(this.error.deleteAgent);
        }
        return false;
      } finally {
        this.loading.deleteAgent = false;
      }
    },

    async linkKnowledgeBaseToAgent(agentId, knowledgeBaseId) {
      this.loading.linkKB = true;
      this.error.linkKB = null;
      try {
        await agentApi.linkKnowledgeBase(agentId, knowledgeBaseId);
        ElMessage.success('Knowledge base linked successfully');
        // Refresh current agent details to show the new link
        if (this.currentAgent && this.currentAgent.id === agentId) {
          await this.fetchAgentDetail(agentId);
        }
        return true;
      } catch (err) {
        this.error.linkKB = 'Failed to link knowledge base';
        console.error(err);
        ElMessage.error(this.error.linkKB + (err.response?.data?.detail ? `: ${err.response.data.detail}` : ''));
        return false;
      } finally {
        this.loading.linkKB = false;
      }
    },

    async unlinkKnowledgeBaseFromAgent(agentId, knowledgeBaseId) {
      this.loading.unlinkKB = true;
      this.error.unlinkKB = null;
      try {
        await agentApi.unlinkKnowledgeBase(agentId, knowledgeBaseId);
        ElMessage.success('Knowledge base unlinked successfully');
        if (this.currentAgent && this.currentAgent.id === agentId) {
          await this.fetchAgentDetail(agentId); // Re-fetch to update linked KBs list
        }
        return true;
      } catch (err) {
        this.error.unlinkKB = 'Failed to unlink knowledge base';
        console.error(err);
        ElMessage.error(this.error.unlinkKB + (err.response?.data?.detail ? `: ${err.response.data.detail}` : ''));
        return false;
      } finally {
        this.loading.unlinkKB = false;
      }
    },

    async fetchAvailableKBsForLinking() {
      this.loading.fetchAvailableKBs = true;
      this.error.fetchAvailableKBs = null;
      try {
        const response = await agentApi.getAvailableKnowledgeBasesForLinking();
        this.availableKBsForLinking = response.data;
      } catch (err) {
        this.error.fetchAvailableKBs = 'Failed to fetch available knowledge bases';
        console.error(err);
        ElMessage.error(this.error.fetchAvailableKBs);
      } finally {
        this.loading.fetchAvailableKBs = false;
      }
    },

    async fetchAgentWorkflows(agentId) {
      this.loading.fetchAgentSubresources = true;
      this.error.fetchAgentSubresources = null;
      try {
        const response = await agentApi.getAgentWorkflows(agentId);
        this.currentAgentWorkflows = response.data;
      } catch (err) {
        this.error.fetchAgentSubresources = `Failed to fetch workflows for agent ${agentId}`;
        console.error(err);
        ElMessage.error(this.error.fetchAgentSubresources);
      } finally {
        this.loading.fetchAgentSubresources = false;
      }
    },

    async fetchAvailableTools() {
      this.loading.fetchAvailableTools = true;
      this.error.fetchAvailableTools = null;
      try {
        const response = await agentApi.listAvailableTools();
        this.availableTools = response.data;
      } catch (err) {
        this.error.fetchAvailableTools = 'Failed to fetch available tools';
        console.error(err);
        ElMessage.error(this.error.fetchAvailableTools);
      } finally {
        this.loading.fetchAvailableTools = false;
      }
    },

    clearCurrentAgent() {
      this.currentAgent = null;
      this.currentAgentSkills = [];
      this.currentAgentWorkflows = [];
      this.currentAgentLinkedKBs = [];
    },
  },

  getters: {
    getAgentById: (state) => (id) => {
      return state.agents.find(agent => agent.id === id);
    },
    // Add more specific loading/error getters if needed
    isListLoading: (state) => state.loading.fetchAgents,
    isDetailLoading: (state) => state.loading.fetchAgentDetail,
    // ... other getters for specific loading states
  },
}); 