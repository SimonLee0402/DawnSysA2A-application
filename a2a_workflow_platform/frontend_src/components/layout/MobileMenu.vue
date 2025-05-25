<template>
  <div class="mobile-menu">
    <el-drawer
      v-model="isOpen"
      :direction="direction"
      :size="drawerSize"
      :before-close="handleClose"
      title="主菜单"
    >
      <el-menu
        class="mobile-nav"
        :router="true"
        :default-active="activeIndex"
        @select="handleSelect"
      >
        <el-menu-item v-for="item in menuItems" :key="item.path" :index="item.path">
          <el-icon><component :is="item.icon" /></el-icon>
          <template #title>{{ item.title }}</template>
        </el-menu-item>
      </el-menu>
      
      <div class="mobile-menu-footer" v-if="isAuthenticated">
        <el-divider />
        <el-dropdown trigger="click" @command="handleCommand">
          <div class="user-info">
            <el-avatar :size="32" icon="UserFilled" />
            <span class="username">{{ username }}</span>
          </div>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="profile">
                <el-icon><User /></el-icon>个人资料
              </el-dropdown-item>
              <el-dropdown-item command="logout">
                <el-icon><SwitchButton /></el-icon>退出登录
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
    </el-drawer>
  </div>
</template>

<script>
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/store/auth'
import { User, SwitchButton } from '@element-plus/icons-vue'

export default {
  name: 'MobileMenu',
  components: {
    User,
    SwitchButton
  },
  props: {
    modelValue: {
      type: Boolean,
      default: false
    }
  },
  emits: ['update:modelValue'],
  setup(props, { emit }) {
    const router = useRouter()
    const authStore = useAuthStore()
    
    const isOpen = computed({
      get: () => props.modelValue,
      set: (value) => emit('update:modelValue', value)
    })
    
    const direction = ref('ltr')
    const drawerSize = ref('70%')
    
    // 登录状态和用户信息
    const isAuthenticated = computed(() => authStore.isAuthenticated)
    const username = computed(() => authStore.currentUser?.username || '用户')
    
    // 菜单项配置
    const menuItems = [
      { path: '/', title: '首页', icon: 'House' },
      { path: '/workflows', title: '工作流', icon: 'Connection' },
      { path: '/agents', title: '智能体', icon: 'Avatar' },
      { path: '/sessions', title: '会话', icon: 'ChatDotRound' },
      { path: '/tasks', title: '任务', icon: 'List' },
      { path: '/a2a/test', title: 'A2A测试', icon: 'Share' }
    ]
    
    // 计算当前激活的菜单项
    const activeIndex = computed(() => {
      const path = router.currentRoute.value.path
      // 找到匹配的菜单路径（前缀匹配）
      const match = menuItems.find(item => path === item.path || path.startsWith(`${item.path}/`))
      return match ? match.path : '/'
    })
    
    // 关闭抽屉
    const handleClose = () => {
      isOpen.value = false
    }
    
    // 菜单项选择
    const handleSelect = (index) => {
      router.push(index)
      // 在移动端选择菜单项后自动关闭菜单
      setTimeout(() => {
        isOpen.value = false
      }, 300)
    }
    
    // 处理用户菜单命令
    const handleCommand = async (command) => {
      if (command === 'profile') {
        router.push('/profile')
        isOpen.value = false
      } else if (command === 'logout') {
        try {
          await authStore.logout()
          router.push('/login')
          isOpen.value = false
        } catch (error) {
          console.error('退出登录失败', error)
        }
      }
    }
    
    return {
      isOpen,
      direction,
      drawerSize,
      isAuthenticated,
      username,
      menuItems,
      activeIndex,
      handleClose,
      handleSelect,
      handleCommand
    }
  }
}
</script>

<style scoped>
.mobile-nav {
  border-right: none;
}

.mobile-menu-footer {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 15px;
}

.user-info {
  display: flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
  padding: 5px 10px;
  border-radius: 4px;
  transition: background-color 0.3s;
}

.user-info:hover {
  background-color: #f5f7fa;
}

.username {
  font-size: 14px;
  color: #303133;
}
</style> 