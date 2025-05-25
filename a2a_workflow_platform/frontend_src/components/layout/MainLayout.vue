<template>
  <el-container class="main-layout">
    <el-header class="layout-header" height="60px">
      <app-header />
    </el-header>
    <el-container class="main-container">
      <app-sidebar class="desktop-sidebar" />
    <el-main class="app-content">
      <router-view v-slot="{ Component, route }">
        <transition name="fade" mode="out-in">
          <keep-alive>
            <div :key="route.path" class="page-wrapper">
              <component :is="Component" />
            </div>
          </keep-alive>
        </transition>
      </router-view>
    </el-main>
    </el-container>
    <el-footer class="layout-footer" height="auto">
      <app-footer />
    </el-footer>
  </el-container>
</template>

<script setup>
import AppHeader from './AppHeader.vue';
import AppSidebar from './AppSidebar.vue';
import AppFooter from './AppFooter.vue';

// Footer display logic (if needed later) can be uncommented and adapted
// import { computed } from 'vue';
// import { useRoute } from 'vue-router';
// const route = useRoute();
// const showFooter = computed(() => {
//   return route.meta.showFooter !== false;
// });

</script>

<style scoped>
.main-layout {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.layout-header {
  padding: 0;
  border-bottom: 1px solid #e6e6e6;
  background-color: #fff;
  height: 60px;
}

.main-container {
  display: flex;
  flex-direction: row;
  flex-grow: 1;
  overflow: hidden;
}

.app-content {
  flex: 1;
  padding: 20px;
  background-color: #f4f6f8;
  overflow-y: auto;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.layout-footer {
  padding: 0;
  background-color: #f4f6f8;
  border-top: 1px solid #e6e6e6;
}

/* 响应式布局调整 */
@media (max-width: 768px) {
  .desktop-sidebar {
    display: none;
  }
  
  .app-content {
    padding: 15px;
  }
}
</style> 