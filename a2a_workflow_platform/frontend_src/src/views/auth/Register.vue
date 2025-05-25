<template>
  <div class="auth-page-container">
    <el-card class="register-card auth-card app-card">
      <h2 class="app-page-title">注册新账户</h2>
      
      <el-alert
        v-if="authStore.error"
        :title="authStore.error"
        type="error"
        :closable="false"
        show-icon
        class="register-alert"
        @close="authStore.clearError"
      />
      
      <el-form 
        ref="registerForm" 
        :model="formData" 
        :rules="rules" 
        label-position="top"
        @submit.native.prevent="handleRegister"
        class="register-form"
      >
        <el-form-item label="用户名" prop="username">
          <el-input 
            v-model="formData.username" 
            name="username"
            placeholder="请输入用户名"
            prefix-icon="User"
            size="large"
          />
        </el-form-item>
        
        <el-form-item label="电子邮箱" prop="email">
          <el-input 
            v-model="formData.email" 
            name="email"
            placeholder="请输入电子邮箱"
            type="email"
            prefix-icon="Message"
            size="large"
          />
        </el-form-item>
        
        <el-form-item label="密码" prop="password1">
          <el-input 
            v-model="formData.password1" 
            name="password1"
            type="password" 
            placeholder="请输入密码 (至少6位)"
            prefix-icon="Lock"
            show-password
            size="large"
          />
        </el-form-item>
        
        <el-form-item label="确认密码" prop="password2">
          <el-input 
            v-model="formData.password2" 
            name="password2"
            type="password" 
            placeholder="请再次输入密码"
            prefix-icon="Lock"
            show-password
            size="large"
          />
        </el-form-item>
        
        <div class="form-actions">
          <el-button 
            type="primary" 
            @click="handleRegister"
            :loading="authStore.isLoading" 
            class="register-button"
            size="large"
            native-type="submit"
          >
            注册
          </el-button>
        </div>
        
        <div class="register-footer">
          <span>已有账号？<router-link to="/login">立即登录</router-link></span>
        </div>
      </el-form>
    </el-card>
  </div>
</template>

<script>
import { ref, reactive, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/store/auth' 
import { ElMessage } from 'element-plus'
import axios from 'axios'
import { User, Lock, Message } from '@element-plus/icons-vue'

export default {
  name: 'RegisterView',
  components: {
    User,
    Lock,
    Message
  },
  setup() {
    const router = useRouter()
    const authStore = useAuthStore()
    const registerForm = ref(null)
    
    // 表单数据
    const formData = reactive({
      username: '',
      email: '',
      password1: '',
      password2: ''
    })
    
    // 验证两次密码是否一致
    const validatePassConfirm = (rule, value, callback) => {
      if (value !== formData.password1) {
        callback(new Error('两次输入的密码不一致'))
      } else {
        callback()
      }
    }
    
    // 表单校验规则
    const rules = {
      username: [
        { required: true, message: '请输入用户名', trigger: 'blur' },
        { min: 3, max: 30, message: '用户名长度应在3-30个字符之间', trigger: 'blur' }
      ],
      email: [
        { required: true, message: '请输入电子邮箱', trigger: 'blur' },
        { type: 'email', message: '请输入正确的电子邮箱格式', trigger: 'blur' }
      ],
      password1: [
        { required: true, message: '请输入密码', trigger: 'blur' },
        { min: 6, message: '密码长度不能小于6个字符', trigger: 'blur' }
      ],
      password2: [
        { required: true, message: '请再次输入密码', trigger: 'blur' },
        { validator: validatePassConfirm, trigger: 'blur' }
      ]
    }
    
    // 计算属性
    const isLoading = computed(() => authStore.isLoading)
    const error = computed(() => authStore.error)
    
    // 获取CSRF Token (Ensures cookie is set for Axios)
    const getCsrfToken = async () => {
      try {
        // 使用专门的CSRF端点来确保cookie被设置
        await axios.get('/api/csrf/') // Use the configured axios instance
        console.log('CSRF cookie should be set by the response.')
        // No need to read or store the token here, Axios handles it.
      } catch (error) {
        console.error('请求CSRF令牌端点失败', error)
        // Potentially inform the user or block the form?
        authStore.error = '无法初始化注册表单，请刷新页面。'; 
      }
    }
    
    // 处理注册
    const handleRegister = async () => {
      authStore.clearError();
      if (!registerForm.value) return;

        try {
        await registerForm.value.validate();
        console.log('Register form validation passed');

        await authStore.register(formData);
        console.log('Registration successful via store, redirecting...');
        router.push({ name: 'Login' });
      } catch (err) {
        if (err && err.fields) {
          console.warn('Register form validation failed:', err.fields);
            } else {
          console.error('Registration failed in component catch block:', err);
        }
      }
    }
    
    // 生命周期钩子
    onMounted(async () => {
      // 获取CSRF令牌
      await getCsrfToken()
      
      // 如果已登录，直接跳转到首页
      if (authStore.isAuthenticated) {
        router.push('/')
      }
    })
    
    return {
      registerForm,
      formData,
      rules,
      isLoading,
      error,
      handleRegister,
      authStore
    }
  }
}
</script>

<style scoped>
/* .modern-register-container and .modern-register-card removed */

/* 使用全局app-page-title样式，只添加注册页特有的样式 */
.app-page-title {
  text-align: center;
  margin-bottom: 25px;
}

.register-form .el-form-item {
  margin-bottom: 20px;
}

.register-form .el-form-item :deep(.el-form-item__label) {
  padding-bottom: 4px;
  line-height: 1.2;
  font-size: 14px;
}

.register-form .el-input :deep(.el-input__inner) {
  border-radius: 4px;
}

.form-actions {
  margin-top: 25px;
}

.register-button {
  width: 100%;
  border-radius: 4px;
  font-weight: 500;
}

.register-footer {
  margin-top: 20px;
  text-align: center;
  font-size: 14px;
  color: #606266;
}

.register-footer a {
  color: #409EFF;
  text-decoration: none;
}

.register-footer a:hover {
  text-decoration: underline;
}

.register-alert {
  margin-bottom: 20px;
}
</style> 