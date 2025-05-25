import axios from 'axios'; // Or your preferred HTTP client

// You might have a base URL configured elsewhere, e.g., in an env file or a global config
// For now, we'll use relative paths assuming the Vue dev server proxies to the Django backend.
const API_BASE_URL = '/api/'; // Ensure API_BASE_URL ends with a slash

// Utility to get the token, e.g., from localStorage or a store
// This is a placeholder and needs to be adapted to your auth mechanism
const getAuthHeaders = () => {
  const token = localStorage.getItem('authToken'); // Example: adjust to your token storage
  if (token) {
    return { Authorization: `Token ${token}` }; // Example: adjust to your auth scheme (Bearer, Token, etc.)
  }
  return {};
};

const apiClient = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
    // Add other default headers if needed
  },
});

// Add a request interceptor to include the auth token dynamically
apiClient.interceptors.request.use(config => {
  const authHeaders = getAuthHeaders();
  config.headers = { ...config.headers, ...authHeaders };
  return config;
}, error => {
  return Promise.reject(error);
});


export default {
  // Workflow Instances
  getWorkflowInstances(params) {
    // Directly use axios with the full path and manual header management
    return axios.get('/api/workflow-instance/', { // Full path with trailing slash, changed to singular
      headers: getAuthHeaders(), // Manually add auth headers
      params: params
    });
  },
  getWorkflowInstance(id) {
    // For consistency, let's also change this one if the above works, or revert if not needed
    // return apiClient.get(`/workflows/instances/${id}/`);
    // Also changing this to singular for consistency with the hypothesis
    return axios.get(`/api/workflow-instance/${id}/`, { 
        headers: getAuthHeaders()
    });
  },
  // Add other API functions as needed for workflows, agents, etc.

  // Example: Workflow Definitions
  getWorkflows(params) {
    return apiClient.get('/workflows/', { params });
  },
  getWorkflow(id) {
    return apiClient.get(`/workflows/${id}/`);
  },
  createWorkflow(data) {
    return apiClient.post('/workflows/', data);
  },
  updateWorkflow(id, data) {
    return apiClient.put(`/workflows/${id}/`, data);
  },
  deleteWorkflow(id) {
    return apiClient.delete(`/workflows/${id}/`);
  },
  executeWorkflow(id, parameters) {
    return apiClient.post(`/workflows/${id}/execute/`, { parameters });
  },
  // ... other api calls

  // Tasks
  getTasks(params) {
    return apiClient.get('/tasks/', { params });
  },
  getTask(id) {
    return apiClient.get(`/tasks/${id}/`);
  },

  // Sessions
  getSessions(params) {
    return apiClient.get('/sessions/', { params });
  },
  getSession(id) {
    return apiClient.get(`/sessions/${id}/`);
  },

  // Agents (AgentCards)
  getAgents(params) {
    return apiClient.get('/agents/', { params }); // Corresponds to AgentCardViewSet list
  },
  getAgent(id) {
    return apiClient.get(`/agents/${id}/`); // Corresponds to AgentCardViewSet retrieve
  },
  createAgent(data) { // Assuming you might have added this for the form to work
    return apiClient.post('/agents/', data);
  },
  updateAgent(id, data) {
    return apiClient.put(`/agents/${id}/`, data);
  },
}; 