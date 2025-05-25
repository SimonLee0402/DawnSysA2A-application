<template>
  <el-header class="app-header">
    <div class="toolbar-container">
      <div class="menu-button-mobile">
        <el-button type="text" @click="showMobileMenu = true">
          <el-icon size="24"><Menu /></el-icon>
        </el-button>
      </div>
      
      <div class="logo-container">
        <router-link to="/" class="logo">A2A工作流平台</router-link>
      </div>
      
      <div class="toolbar-spacer"></div>
      
      <div class="user-menu">
        <template v-if="isAuthenticated">
          <el-dropdown trigger="click" @command="handleCommand">
            <span class="user-dropdown-link">
              <el-avatar icon="UserFilled" size="small" style="margin-right: 8px;" />
              <span class="username-text">{{ username }}</span> <el-icon class="el-icon--right"><arrow-down /></el-icon>
            </span>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="profile">
                  <el-icon><User /></el-icon>个人资料
                </el-dropdown-item>
                <el-dropdown-item divided command="logout">
                  <el-icon><SwitchButton /></el-icon>退出登录
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </template>
        <template v-else>
          <el-button type="text" @click="$router.push('/login')">登录</el-button>
          <el-button type="text" @click="$router.push('/register')">注册</el-button>
        </template>
      </div>
    </div>
    
    <!-- 移动端菜单 -->
    <mobile-menu v-model="showMobileMenu" />
  </el-header>
</template>

<script>
import { computed, ref } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useAuthStore } from '@/store/auth'
import { ArrowDown, User, SwitchButton, Menu } from '@element-plus/icons-vue'
import MobileMenu from './MobileMenu.vue'

export default {
  name: 'AppHeader',
  components: {
    ArrowDown,
    User,
    SwitchButton,
    Menu,
    MobileMenu
  },
  setup() {
    const authStore = useAuthStore()
    const router = useRouter()
    const route = useRoute()
    
    // 移动菜单状态
    const showMobileMenu = ref(false)
    
    // 计算属性
    const isAuthenticated = computed(() => authStore.isAuthenticated)
    const username = computed(() => authStore.user?.username || '')
    const activeRoute = computed(() => route.path)
    
    // 处理下拉菜单命令
    const handleCommand = (command) => {
      if (command === 'logout') {
        authStore.logout()
        router.push('/login')
      } else if (command === 'profile') {
        router.push('/profile')
      }
    }
    
    return {
      isAuthenticated,
      username,
      activeRoute,
      handleCommand,
      showMobileMenu
    }
  }
}
</script>

<style scoped>
.app-header {
  padding: 0;
  box-shadow: 0 1px 4px rgba(0,21,41,.08);
  background-color: #fff;
  height: 60px;
}

.toolbar-container {
  display: flex;
  align-items: center;
  height: 100%;
  padding: 0 20px;
}

.logo-container {
  margin-right: 20px;
  display: flex;
  align-items: center;
}

.logo {
  font-size: 20px;
  font-weight: 600;
  color: #303133;
  text-decoration: none;
}

.toolbar-spacer {
  flex-grow: 1;
}

.user-menu {
  margin-left: auto;
}

.user-dropdown-link {
  color: #606266;
  cursor: pointer;
  display: flex;
  align-items: center;
}

.user-dropdown-link .el-icon {
  margin-left: 5px;
}

.user-dropdown-link .el-icon--right {
  margin-left: 5px;
}

.user-dropdown-link:hover {
  color: var(--el-color-primary);
}

.user-menu .el-button {
  color: #606266;
}

.user-menu .el-button:hover {
  color: var(--el-color-primary);
}

/* 移动端适配 */
.menu-button-mobile {
  display: none;
}

@media (max-width: 768px) {
  .menu-button-mobile {
    display: block;
    margin-right: 10px;
  }
  
  .logo {
    font-size: 16px;
  }
  
  .username-text {
    display: none;
  }
}
</style> 