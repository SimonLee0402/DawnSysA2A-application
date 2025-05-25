<template>
  <div class="not-found-container">
    <el-card class="not-found-card">
      <div class="not-found-icon">
        <el-icon :size="64"><WarningFilled /></el-icon>
      </div>
      <h1>404</h1>
      <h2>页面未找到</h2>
      <p>抱歉，您请求的页面不存在或已被移除。</p>
      <div class="actions">
        <el-button type="primary" @click="goHome">返回首页</el-button>
        <el-button @click="goBack">返回上一页</el-button>
      </div>
    </el-card>
  </div>
</template>

<script>
import { useRouter } from 'vue-router'
import { WarningFilled } from '@element-plus/icons-vue'

export default {
  name: 'NotFound',
  components: {
    WarningFilled
  },
  setup() {
    const router = useRouter()
    
    // 返回首页
    const goHome = () => {
      router.push('/')
    }
    
    // 返回上一页
    const goBack = () => {
      router.back()
    }
    
    return {
      goHome,
      goBack
    }
  }
}
</script>

<style scoped>
.not-found-container {
  display: flex;
  justify-content: center;
  align-items: center;
  height: calc(100vh - 200px);
  padding: 20px;
}

.not-found-card {
  max-width: 500px;
  width: 100%;
  text-align: center;
  padding: 20px;
}

.not-found-icon {
  display: flex;
  justify-content: center;
  margin-bottom: 20px;
  color: #f56c6c;
}

h1 {
  font-size: 72px;
  margin: 0;
  color: #f56c6c;
}

h2 {
  font-size: 24px;
  margin: 10px 0;
  color: #303133;
}

p {
  color: #606266;
  margin-bottom: 30px;
}

.actions {
  display: flex;
  justify-content: center;
  gap: 15px;
}
</style> 