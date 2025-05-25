<template>
  <div class="auth-page-container">
    <el-card class="login-card auth-card app-card">
      <h2 class="app-page-title">登录 A2A智能体协作平台</h2>
      
      <el-alert
        v-if="error"
        :title="error"
        type="error"
        :closable="false"
        show-icon
        class="login-alert"
      />
      
      <el-form 
        ref="loginForm" 
        :model="formData" 
        :rules="rules" 
        label-position="top"
        @submit.native.prevent="handleLogin"
        class="login-form"
      >
        <input type="hidden" name="csrfmiddlewaretoken" :value="csrfToken">
        
        <el-form-item label="用户名" prop="username">
          <el-input 
            v-model="formData.username" 
            name="username"
            placeholder="请输入用户名"
            prefix-icon="User"
            size="large"
          />
        </el-form-item>
        
        <el-form-item label="密码" prop="password">
          <el-input 
            v-model="formData.password" 
            name="password"
            type="password" 
            placeholder="请输入密码"
            prefix-icon="Lock"
            show-password
            size="large"
          />
        </el-form-item>
        
        <div class="form-actions">
          <el-button 
            type="primary" 
            @click="handleLogin"
            :loading="isLoading" 
            class="login-button"
            size="large"
            native-type="submit"
          >
            登录
          </el-button>
        </div>
        
        <div class="login-footer">
          <span>还没有账号？<router-link to="/register">立即注册</router-link></span>
          <a href="#" @click.prevent="runDiagnostics" class="login-help-link">登录遇到问题？</a>
        </div>
      </el-form>
    </el-card>
    
    <!-- 诊断对话框 -->
    <el-dialog v-model="diagnosticVisible" title="登录诊断" width="500px">
      <div class="diagnostic-results">
        <div v-if="isDiagnosticRunning">
          <p>正在运行诊断...</p>
          <el-progress :percentage="diagnosticProgress" :format="progressFormat" />
        </div>
        <div v-else>
          <h4>诊断结果</h4>
          <el-descriptions :column="1" border>
            <el-descriptions-item label="网络连接">
              <el-tag :type="diagnosticResults.network ? 'success' : 'danger'">
                {{ diagnosticResults.network ? '正常' : '异常' }}
              </el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="CSRF令牌">
              <el-tag :type="diagnosticResults.csrf ? 'success' : 'danger'">
                {{ diagnosticResults.csrf ? '已获取' : '获取失败' }}
              </el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="API服务">
              <el-tag :type="diagnosticResults.api ? 'success' : 'danger'">
                {{ diagnosticResults.api ? '可访问' : '不可访问' }}
              </el-tag>
            </el-descriptions-item>
            <el-descriptions-item label="本地存储">
              <el-tag :type="diagnosticResults.localStorage ? 'success' : 'danger'">
                {{ diagnosticResults.localStorage ? '正常' : '异常' }}
              </el-tag>
            </el-descriptions-item>
          </el-descriptions>
          
          <div class="diagnostic-actions">
            <div v-if="!allDiagnosticsPassed">
              <h4>修复建议</h4>
              <ul>
                <li v-if="!diagnosticResults.network">请检查网络连接后重试</li>
                <li v-if="!diagnosticResults.csrf">尝试清除浏览器缓存后重试</li>
                <li v-if="!diagnosticResults.api">后端服务可能暂时不可用，请稍后重试</li>
                <li v-if="!diagnosticResults.localStorage">
                  <a href="#" @click.prevent="clearLocalStorage">点击清除本地存储并刷新页面</a>
                </li>
              </ul>
            </div>
            
            <div v-else>
              <p class="success-message">所有检查都已通过！请尝试再次登录。</p>
            </div>
            
            <div class="diagnostic-buttons">
              <el-button type="primary" @click="runDiagnostics" :disabled="isDiagnosticRunning">
                重新诊断
              </el-button>
              <el-button @click="diagnosticVisible = false">关闭</el-button>
            </div>
          </div>
        </div>
      </div>
    </el-dialog>
  </div>
</template>

<script>
import { ref, reactive, computed, onMounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useAuthStore } from '@/store/auth'
import { User, Lock } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import axios from 'axios'
import { repairAuthentication } from '@/api/auth-repair'

export default {
  name: 'LoginView',
  components: {
    User,
    Lock
  },
  setup() {
    const router = useRouter()
    const route = useRoute()
    const authStore = useAuthStore()
    const loginForm = ref(null)
    const csrfToken = ref('')
    
    // 诊断相关状态
    const diagnosticVisible = ref(false)
    const isDiagnosticRunning = ref(false)
    const diagnosticProgress = ref(0)
    const diagnosticResults = reactive({
      network: false,
      csrf: false,
      api: false,
      localStorage: false
    })
    
    // 表单数据
    const formData = reactive({
      username: '',
      password: ''
    })
    
    // 表单校验规则
    const rules = {
      username: [
        { required: true, message: '请输入用户名', trigger: 'blur' }
      ],
      password: [
        { required: true, message: '请输入密码', trigger: 'blur' },
        { min: 6, message: '密码长度不能小于6个字符', trigger: 'blur' }
      ]
    }
    
    // 计算属性
    const isLoading = computed(() => authStore.isLoading)
    const error = computed(() => authStore.error)
    const allDiagnosticsPassed = computed(() => {
      return diagnosticResults.network &&
             diagnosticResults.csrf &&
             diagnosticResults.api &&
             diagnosticResults.localStorage
    })
    
    // 进度条格式化
    const progressFormat = (percentage) => {
      return `${percentage}%`
    }
    
    // 获取CSRF Token
    const getCsrfToken = async () => {
      try {
        // 使用专门的CSRF端点
        await axios.get('/api/csrf/')
        const token = document.cookie
          .split('; ')
          .find(row => row.startsWith('csrftoken='))
          ?.split('=')[1]
        
        if (token) {
          csrfToken.value = token
          console.log('已获取CSRF令牌')
          return token
        } else {
          console.warn('未能从cookie中获取csrftoken')
          return null
        }
      } catch (error) {
        console.error('获取CSRF令牌失败', error)
        return null
      }
    }
    
    // 诊断函数
    const runDiagnostics = async () => {
      diagnosticVisible.value = true
      isDiagnosticRunning.value = true
      diagnosticProgress.value = 0
      
      // 重置诊断结果
      Object.keys(diagnosticResults).forEach(key => {
        diagnosticResults[key] = false
      })
      
      // 检查网络连接
      try {
        await axios.get('/')
        diagnosticResults.network = true
      } catch (error) {
        if (error.response) {
          // 有响应意味着网络连接是正常的
          diagnosticResults.network = true
        } else {
          diagnosticResults.network = false
        }
      }
      diagnosticProgress.value = 25
      
      // 检查CSRF令牌
      const token = await getCsrfToken()
      diagnosticResults.csrf = !!token
      diagnosticProgress.value = 50
      
      // 检查API服务
      try {
        await axios.get('/api/users/current/')
        diagnosticResults.api = true
      } catch (error) {
        if (error.response) {
          // 有响应意味着API服务是可用的
          diagnosticResults.api = true
        } else {
          diagnosticResults.api = false
        }
      }
      diagnosticProgress.value = 75
      
      // 检查本地存储
      try {
        // 尝试写入和读取一个测试值
        localStorage.setItem('_test_key', 'test_value')
        const testValue = localStorage.getItem('_test_key')
        localStorage.removeItem('_test_key')
        
        diagnosticResults.localStorage = testValue === 'test_value'
      } catch (error) {
        diagnosticResults.localStorage = false
      }
      diagnosticProgress.value = 100
      
      // 完成诊断
      setTimeout(() => {
        isDiagnosticRunning.value = false
      }, 500)
    }
    
    // 清除本地存储
    const clearLocalStorage = () => {
      try {
        localStorage.clear()
        ElMessage.success('本地存储已清除，页面将在2秒后刷新')
        setTimeout(() => {
          window.location.reload()
        }, 2000)
      } catch (error) {
        ElMessage.error('清除本地存储失败')
      }
    }
    
    // 处理登录
    const handleLogin = async () => {
      if (!loginForm.value) return; // 确保表单已挂载
      
      try {
        // 1. 使用 Element Plus 表单验证
        await loginForm.value.validate();
        console.log('表单验证通过');
        
        // 2. 表单验证通过后，执行登录逻辑
        try {
        // 设置CSRF令牌
        if (csrfToken.value) {
          axios.defaults.headers.common['X-CSRFToken'] = csrfToken.value
        }
        
        console.log('登录请求URL:', '/api/users/login/')
          console.log('向API发送登录请求', { username: formData.username, password: '***' }); // Mask password in log
        
          authStore.clearError(); // Clear previous store errors
          localStorage.removeItem('loginError') // Clear previous manual errors
        
          // 尝试使用 store 登录
          const success = await authStore.login({
            username: formData.username,
            password: formData.password
          });
          
          console.log('Store 登录尝试结果:', success);
          console.log('当前认证状态:', authStore.isAuthenticated);
          
          if (success) {
            await repairAuthentication(); // 修复认证状态
            ElMessage.success('登录成功');
            const redirectPath = route.query.redirect || '/';
            router.push(redirectPath); 
            // 可以移除强制跳转逻辑，如果路由问题已解决
          } else {
            // 登录失败，错误信息应该在 authStore.error 中
            // ElMessage.error(authStore.error || '登录失败，请检查用户名或密码');
            // 不需要在这里重复显示 ElMessage，authStore.error 会被 error computed 属性捕获并显示在模板的 el-alert 中
            console.error("Login failed via store, error:", authStore.error);
          }
          
        } catch (loginLogicError) {
          // 捕获登录逻辑中的其他未知错误 (非验证、非 store/API 调用失败)
          console.error('登录逻辑执行错误', loginLogicError);
          ElMessage.error('登录过程中发生意外错误，请稍后再试');
        }
        
      } catch (validationError) {
        // 3. 表单验证失败
        console.warn('表单验证失败');
        // ElMessage.error('请检查用户名和密码是否按要求填写。');
        // 验证失败时，Element Plus 会自动显示字段错误，无需额外提示
      }
    }
    
    // 生命周期钩子
    onMounted(async () => {
      // 获取CSRF令牌
      await getCsrfToken()
      
      // 修复可能存在的认证问题
      try {
        await repairAuthentication()
        console.log('登录页面认证状态修复完成')
        
        // 如果已登录，直接跳转到首页
        if (authStore.isAuthenticated) {
          console.log('用户已登录，重定向到首页')
          router.push('/')
        }
      } catch (error) {
        console.error('登录页面认证状态修复失败', error)
      }
    })
    
    return {
      loginForm,
      formData,
      rules,
      isLoading,
      error,
      handleLogin,
      csrfToken,
      // 诊断相关
      diagnosticVisible,
      isDiagnosticRunning,
      diagnosticProgress,
      diagnosticResults,
      allDiagnosticsPassed,
      progressFormat,
      runDiagnostics,
      clearLocalStorage
    }
  }
}
</script>

<style scoped>
/* .modern-login-container and .modern-login-card removed */

/* 使用全局app-page-title样式，只添加登录页特有的样式 */
.app-page-title {
  text-align: center;
  margin-bottom: 25px;
}

.login-form .el-form-item {
  margin-bottom: 20px;
}

.login-form .el-form-item :deep(.el-form-item__label) {
  padding-bottom: 4px;
  line-height: 1.2;
  font-size: 14px;
}

.login-form .el-input :deep(.el-input__inner) {
  border-radius: 4px;
}

.form-actions {
  margin-top: 25px;
}

.login-button {
  width: 100%;
  border-radius: 4px;
  font-weight: 500;
}

.login-footer {
  margin-top: 20px;
  text-align: center;
  font-size: 14px;
  color: #606266;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.login-footer a {
  color: #409EFF;
  text-decoration: none;
}

.login-footer a:hover {
  text-decoration: underline;
}

.login-help-link {
  color: #909399;
}

.login-alert {
  margin-bottom: 20px;
}

.diagnostic-results {
  padding: 10px;
}

.diagnostic-actions {
  margin-top: 20px;
}

.diagnostic-buttons {
  margin-top: 15px;
  text-align: right;
}

.success-message {
  color: #67C23A;
  font-weight: bold;
  margin-bottom: 10px;
}
</style> 