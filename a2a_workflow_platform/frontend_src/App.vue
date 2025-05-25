<template>
  <div class="app-container">
    <el-config-provider :locale="zhCn">
      <!-- AppHeader 和 AppFooter 将由 MainLayout.vue 管理 -->
      <!-- <app-header /> --> 
      
      <div class="main-content-wrapper"> <!-- 可以用一个包装器代替之前的 main-content class -->
        <app-notification />
        <router-view />
      </div>
      
      <!-- <app-footer /> -->
    </el-config-provider>
  </div>
</template>

<script>
import { defineComponent, onMounted, onErrorCaptured } from 'vue'
// AppHeader 和 AppFooter不再需要在App.vue中直接导入和注册，除非有其他特殊用途
// import AppHeader from './components/layout/AppHeader.vue' 
// import AppFooter from './components/layout/AppFooter.vue'
import AppNotification from './components/layout/AppNotification.vue'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import { useStore as useGlobalStore } from './store/global'

export default defineComponent({
  name: 'App',
  components: {
    // AppHeader, // 移除
    // AppFooter, // 移除
    AppNotification
  },
  setup() {
    console.log('App组件已加载')
    const globalStore = useGlobalStore()
    
    onMounted(() => {
      // 初始化全局状态
      try {
        console.log('初始化全局状态...')
        globalStore.initialize()
        console.log('全局状态初始化完成')
      } catch (error) {
        console.error('全局状态初始化失败:', error)
      }
    })
    
    // 添加错误捕获
    onErrorCaptured((err, instance, info) => {
      console.error('App组件错误:', err)
      console.error('错误实例:', instance)
      console.error('错误详情:', info)
      return false // 阻止错误继续传播
    })
    
    return {
      zhCn
    }
  }
})
</script>

<style>
html, body {
  margin: 0;
  padding: 0;
  height: 100%;
  font-family: 'PingFang SC', 'Microsoft YaHei', sans-serif;
}

.app-container {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}

/* .main-content class 相关的样式可以考虑移到 MainLayout.vue 或者根据需要调整 */
/* 例如，如果 AppNotification 需要特定布局，可以保留 main-content-wrapper */
.main-content-wrapper {
  flex: 1;
  display: flex; /* 如果 AppNotification 和 router-view 需要并排或其他复杂布局 */
  flex-direction: column; /* 假设通知在router-view之上 */
}

/* 如果 main-content 的 padding 是全局的，可以保留，否则移到 MainLayout */
/* .main-content {
  flex: 1;
  padding: 20px;
} */

h1 {
  /* 全局 h1 样式可以保留，或者也移到更具体的地方 */
  color: #409EFF;
}
</style> 