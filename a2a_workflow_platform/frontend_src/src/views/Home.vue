<template>
  <div class="home-container">
    <div class="welcome-card">
      <div class="welcome-header">
        <h1 v-if="isAuthenticated && username">欢迎回来, {{ username }}!</h1>
        <h1 v-else>欢迎使用A2A工作流平台</h1>
      </div>
      <div class="welcome-content">
        <p>A2A工作流平台是一个强大的工作流自动化系统，设计用于编排和管理A2A（Agent-to-Agent）任务，实现多智能体协作和自动化处理。</p>
      </div>
    </div>

    <el-row :gutter="20" class="feature-row" v-if="isAuthenticated">
      <el-col :xs="24" :sm="12" :md="8" :lg="6" v-for="(feature, index) in features" :key="index">
        <el-card class="feature-card">
          <div class="feature-icon">
            <el-icon :size="40"><component :is="feature.icon" /></el-icon>
          </div>
          <h3>{{ feature.title }}</h3>
          <p>{{ feature.description }}</p>
          <el-button type="primary" plain @click="$router.push(feature.link)">
            {{ feature.buttonText }}
          </el-button>
        </el-card>
      </el-col>
    </el-row>

    <el-row v-if="isAuthenticated && dashboardData">
      <el-col :span="24">
        <h2 class="dashboard-title">平台概览</h2>
      </el-col>
      <el-col :xs="24" :sm="12" :md="6">
        <el-card class="dashboard-card">
          <h4>工作流</h4>
          <div class="dashboard-number">{{ dashboardData.workflows || 0 }}</div>
        </el-card>
      </el-col>
      <el-col :xs="24" :sm="12" :md="6">
        <el-card class="dashboard-card">
          <h4>运行中实例</h4>
          <div class="dashboard-number">{{ dashboardData.runningInstances || 0 }}</div>
        </el-card>
      </el-col>
      <el-col :xs="24" :sm="12" :md="6">
        <el-card class="dashboard-card">
          <h4>智能体</h4>
          <div class="dashboard-number">{{ dashboardData.agents || 0 }}</div>
        </el-card>
      </el-col>
      <el-col :xs="24" :sm="12" :md="6">
        <el-card class="dashboard-card">
          <h4>任务</h4>
          <div class="dashboard-number">{{ dashboardData.tasks || 0 }}</div>
        </el-card>
      </el-col>
    </el-row>
  </div>
</template>

<script>
import { computed, onMounted, ref, onActivated } from 'vue'
// Path to auth store will need to be adjusted based on new location of Home.vue
// Assuming Home.vue moves to src/views/ and store is in src/store/
import { useAuthStore } from '@/store/auth' 
// Path to api-exports will also need to be adjusted
// Assuming Home.vue moves to src/views/ and api is in src/api/
import { apiClient } from '@/api/api-exports' 
import {
  Connection, 
  Document, 
  Setting, 
  Monitor
} from '@element-plus/icons-vue'

export default {
  name: 'HomeView',
  components: {
    Connection,
    Document,
    Setting,
    Monitor
  },
  setup() {
    const authStore = useAuthStore()
    
    const isAuthenticated = computed(() => authStore.isAuthenticated)
    const username = computed(() => authStore.user?.username || '用户')

    const dashboardData = ref(null)
    
    // 功能卡片
    const features = [
      {
        title: '工作流管理',
        description: '创建、编辑和管理工作流定义',
        icon: 'Document',
        link: '/workflow',
        buttonText: '查看工作流'
      },
      {
        title: '工作流实例',
        description: '监控和管理工作流执行实例',
        icon: 'Monitor',
        link: '/workflow/instance',
        buttonText: '查看实例'
      },
      {
        title: '智能体管理',
        description: '配置和测试A2A协议智能体',
        icon: 'Connection',
        link: '/agents',
        buttonText: '管理智能体'
      },
      {
        title: '系统设置',
        description: '管理个人资料和应用设置',
        icon: 'Setting',
        link: '/profile',
        buttonText: '查看设置'
      }
    ]
    
    // 获取仪表盘数据
    const fetchDashboardData = async () => {
      if (!isAuthenticated.value) {
        console.warn("HomeView: fetchDashboardData called when not authenticated. This shouldn't happen.");
        return;
      }
      
      try {
        const response = await apiClient.get('/dashboard/')
        dashboardData.value = response.data
      } catch (error) {
        console.error('获取仪表盘数据失败:', error.response ? error.response.data : error.message)
        dashboardData.value = {
          workflows: 'N/A',
          runningInstances: 'N/A',
          agents: 'N/A',
          tasks: 'N/A'
        }
      }
    }
    
    onMounted(() => {
      if (isAuthenticated.value) {
         fetchDashboardData()
      } else {
        console.warn("HomeView: Mounted, but user is not authenticated as expected by requiresAuth meta.");
      }
    })

    // 添加 onActivated 钩子
    onActivated(() => {
      if (isAuthenticated.value) {
        fetchDashboardData() // 当组件被激活时也获取数据
      }
    })
    
    return {
      isAuthenticated,
      username,
      features,
      dashboardData
    }
  }
}
</script>

<style scoped>
/* ... styles ... */
</style> 