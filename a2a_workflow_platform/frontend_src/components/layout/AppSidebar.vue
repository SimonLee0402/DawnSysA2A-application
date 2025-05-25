<template>
  <el-aside :width="asideWidth" class="app-sidebar">
    <el-scrollbar>
      <el-menu
        :default-active="activeMenu"
        router
        class="sidebar-menu"
        :collapse="isCollapsed"
      >
        <el-menu-item index="/">
          <el-icon><HomeFilled /></el-icon>
          <template #title>首页</template>
        </el-menu-item>
        <el-sub-menu index="/agents">
          <template #title>
            <el-icon><Avatar /></el-icon>
            <span>智能体管理</span>
          </template>
          <el-menu-item index="/agents">列表</el-menu-item>
          <!-- <el-menu-item index="/agents/create">创建</el-menu-item> -->
          <!-- Add other agent related routes if needed -->
        </el-sub-menu>
        <el-sub-menu index="/workflow">
           <template #title>
            <el-icon><Share /></el-icon>
            <span>工作流管理</span>
          </template>
          <el-menu-item index="/workflow">工作流列表</el-menu-item>
          <el-menu-item index="/workflow/create">创建工作流</el-menu-item>
          <el-menu-item index="/workflow/designer">设计器</el-menu-item>
          <!-- Add workflow related routes -->
        </el-sub-menu>
        <!-- Add other main menu items like Task, Session etc. -->
         <el-menu-item index="/tasks">
          <el-icon><List /></el-icon>
          <template #title>任务中心</template>
        </el-menu-item>
        <el-menu-item index="/sessions">
          <el-icon><ChatDotRound /></el-icon>
          <template #title>会话历史</template>
        </el-menu-item>
         <el-menu-item index="/a2a/interop-test">
          <el-icon><Share /></el-icon>
          <template #title>A2A测试</template>
        </el-menu-item>
        <el-menu-item index="/knowledgebases">
          <el-icon><Collection /></el-icon>
          <template #title>知识库管理</template>
        </el-menu-item>
      </el-menu>
    </el-scrollbar>
    
    <!-- Collapse Button -->
    <div class="collapse-button-container">
      <el-button 
        text 
        :icon="isCollapsed ? Expand : Fold"
        @click="toggleCollapse"
        class="collapse-button"
      >
        <span v-if="!isCollapsed">收起侧边栏</span>
      </el-button>
    </div>

  </el-aside>
</template>

<script setup>
import { ref, computed } from 'vue';
import { useRoute } from 'vue-router';
import {
  HomeFilled,
  Avatar,
  Share,
  List,
  ChatDotRound,
  Fold,
  Expand,
  Collection
} from '@element-plus/icons-vue';

const route = useRoute();

// 侧边栏折叠状态
const isCollapsed = ref(false);

// 计算侧边栏宽度
const asideWidth = computed(() => isCollapsed.value ? '64px' : '200px');

// 切换折叠状态
const toggleCollapse = () => {
  isCollapsed.value = !isCollapsed.value;
};

// 计算当前激活菜单项，用于高亮显示
const activeMenu = computed(() => {
  const { path } = route;
  // 如果是子路由，可能需要高亮父菜单，这里简单处理，直接用路径
   // 检查path是否以任何菜单项的path开头
  const menuPaths = [
    '/',
    '/agents',
    '/workflow',
    '/tasks',
    '/sessions',
    '/a2a/interop-test',
    '/knowledgebases'
  ];
  const matchedPath = menuPaths.find(menuPath => 
    path === menuPath || (path.startsWith(menuPath + '/') && menuPath !== '/')
  );
  return matchedPath || path; // If no specific match, just return the path
});

</script>

<style scoped>
.app-sidebar {
  background-color: #fff; /* Or your desired sidebar background */
  border-right: 1px solid #e6e6e6;
  height: 100%; /* Use 100% to fill parent, parent (main-container) will handle height */
  display: flex;
  flex-direction: column;
  transition: width 0.3s ease; /* Add transition for smooth resizing */
  flex-shrink: 0; /* Prevent sidebar from shrinking below its width */
}

.el-scrollbar {
  flex-grow: 1;
}

.sidebar-menu {
  border-right: none; /* Remove default border of el-menu */
  height: 100%;
}

.sidebar-menu:not(.el-menu--collapse) {
  width: 200px;
}

.collapse-button-container {
  padding: 10px;
  border-top: 1px solid #e6e6e6;
  text-align: center; /* Center the button */
}

.collapse-button {
  width: 100%; /* Make button full width */
}

/* Adjust menu item padding when collapsed */
.sidebar-menu.el-menu--collapse :deep(.el-tooltip) {
  padding: 0 !important; /* Remove tooltip padding */
}

.sidebar-menu.el-menu--collapse :deep(.el-menu-item) {
   padding: 0 20px !important; /* Adjust padding for collapsed items */
}

.sidebar-menu.el-menu--collapse :deep(.el-sub-menu__title) {
   padding: 0 20px !important; /* Adjust padding for collapsed items */
}

/* Add more styles as needed for hover effects, active state, etc. */

</style> 