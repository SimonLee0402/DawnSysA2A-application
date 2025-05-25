<template>
  <el-row :gutter="20" class="profile-page-container">
    <el-col :span="24">
      <h1 class="app-page-title">个人资料</h1>
    </el-col>
  </el-row>
  
  <el-row :gutter="20">
    <el-col :xs="24" :md="16">
      <el-card class="profile-card app-card" v-loading="isLoading">
        <template #header>
          <h3 class="app-card-title">基本信息</h3>
        </template>
        
        <el-form 
          ref="profileForm" 
          :model="formData" 
          :rules="rules" 
          label-position="top"
          @submit.prevent="handleSaveProfile"
          class="profile-form"
        >
          <el-alert
            v-if="error"
            :title="error"
            type="error"
            :closable="false"
            show-icon
            class="profile-alert"
          />
          
          <el-form-item label="用户名" prop="username">
            <el-input 
              v-model="formData.username" 
              placeholder="用户名"
              disabled
              size="large"
            />
          </el-form-item>
          
          <el-form-item label="电子邮箱" prop="email">
            <el-input 
              v-model="formData.email" 
              placeholder="电子邮箱"
              type="email"
              size="large"
            />
          </el-form-item>
          
          <el-form-item label="名字" prop="first_name">
            <el-input 
              v-model="formData.first_name" 
              placeholder="名字"
              size="large"
            />
          </el-form-item>
          
          <el-form-item label="姓氏" prop="last_name">
            <el-input 
              v-model="formData.last_name" 
              placeholder="姓氏"
              size="large"
            />
          </el-form-item>
          
          <el-form-item class="form-actions">
            <el-button 
              type="primary" 
              native-type="submit" 
              :loading="isLoading"
              size="large"
              class="save-button"
            >
              保存更改
            </el-button>
          </el-form-item>
        </el-form>
      </el-card>
    </el-col>
    
    <el-col :xs="24" :md="8">
      <el-card class="security-card app-card">
        <template #header>
          <h3 class="app-card-title">安全设置</h3>
        </template>
        
        <div class="security-item">
          <h4>修改密码</h4>
          <p>定期更改密码可以提高账户安全性</p>
          <el-button 
            type="primary" 
            plain
            @click="showPasswordDialog = true"
            class="security-button"
          >
            修改密码
          </el-button>
        </div>
        
        <el-divider />
        
        <div class="security-item">
          <h4>账户活动</h4>
          <p>查看您的账户登录记录和操作历史</p>
          <el-button 
            type="info" 
            plain
            @click="$router.push('/profile/activity')"
            class="security-button"
          >
            查看活动
          </el-button>
        </div>
      </el-card>
    </el-col>
  </el-row>
  
  <!-- 修改密码对话框 -->
  <el-dialog
    v-model="showPasswordDialog"
    title="修改密码"
    width="400px"
    append-to-body
    class="password-dialog"
  >
    <el-form 
      ref="passwordForm" 
      :model="passwordData" 
      :rules="passwordRules" 
      label-position="top"
      class="password-change-form"
    >
      <el-alert
        v-if="passwordError"
        :title="passwordError"
        type="error"
        show-icon
        :closable="false"
        style="margin-bottom: 15px;"
      />
      
      <el-form-item label="当前密码" prop="old_password">
        <el-input 
          v-model="passwordData.old_password" 
          type="password" 
          placeholder="请输入当前密码"
          show-password
          size="large"
        />
      </el-form-item>
      
      <el-form-item label="新密码" prop="new_password1">
        <el-input 
          v-model="passwordData.new_password1" 
          type="password" 
          placeholder="请输入新密码 (至少6位)"
          show-password
          size="large"
        />
      </el-form-item>
      
      <el-form-item label="确认新密码" prop="new_password2">
        <el-input 
          v-model="passwordData.new_password2" 
          type="password" 
          placeholder="请再次输入新密码"
          show-password
          size="large"
        />
      </el-form-item>
    </el-form>
    
    <template #footer>
      <span class="dialog-footer">
        <el-button @click="showPasswordDialog = false" size="large">取消</el-button>
        <el-button 
          type="primary" 
          @click="handleChangePassword"
          :loading="passwordLoading"
          size="large"
        >
          确认修改
        </el-button>
      </span>
    </template>
  </el-dialog>
</template>

<script>
import { ref, reactive, computed, onMounted } from 'vue'
import { useAuthStore } from '@/store/auth' 
import { ElMessage } from 'element-plus'
import axios from 'axios'

export default {
  name: 'ProfileView',
  setup() {
    const authStore = useAuthStore()
    const profileForm = ref(null)
    const passwordForm = ref(null)
    
    // 表单数据
    const formData = reactive({
      username: '',
      email: '',
      first_name: '',
      last_name: ''
    })
    
    // 密码表单数据
    const passwordData = reactive({
      old_password: '',
      new_password1: '',
      new_password2: ''
    })
    
    // 状态
    const showPasswordDialog = ref(false)
    const passwordLoading = ref(false)
    const passwordError = ref('')
    
    // 验证两次密码是否一致
    const validatePassConfirm = (rule, value, callback) => {
      if (value !== passwordData.new_password1) {
        callback(new Error('两次输入的密码不一致'))
      } else {
        callback()
      }
    }
    
    // 表单校验规则
    const rules = {
      email: [
        { required: true, message: '请输入电子邮箱', trigger: 'blur' },
        { type: 'email', message: '请输入正确的电子邮箱格式', trigger: 'blur' }
      ]
    }
    
    // 密码校验规则
    const passwordRules = {
      old_password: [
        { required: true, message: '请输入当前密码', trigger: 'blur' }
      ],
      new_password1: [
        { required: true, message: '请输入新密码', trigger: 'blur' },
        { min: 6, message: '密码长度不能小于6个字符', trigger: 'blur' }
      ],
      new_password2: [
        { required: true, message: '请再次输入新密码', trigger: 'blur' },
        { validator: validatePassConfirm, trigger: 'blur' }
      ]
    }
    
    // 计算属性
    const isLoading = computed(() => authStore.isLoading)
    const error = computed(() => authStore.error)
    
    // 获取用户个人资料
    const fetchUserProfile = async () => {
      if (!authStore.user) {
        await authStore.checkAuth()
      }
      
      if (authStore.user) {
        formData.username = authStore.user.username || ''
        formData.email = authStore.user.email || ''
        formData.first_name = authStore.user.first_name || ''
        formData.last_name = authStore.user.last_name || ''
      }
    }
    
    // 保存个人资料
    const handleSaveProfile = async () => {
      if (!profileForm.value) return
      authStore.clearError() // 清除之前的错误
      
      try {
        await profileForm.value.validate()
        authStore.setLoading(true) // 手动设置加载状态
        
        const success = await authStore.updateUserProfile({
          email: formData.email,
          first_name: formData.first_name,
          last_name: formData.last_name
        })
        
        if (success) {
        ElMessage.success('个人资料更新成功')
        } else {
          // 错误信息应该由 store 的 error 状态处理
        }
        
      } catch (error) {
        if (error && error.fields) {
          console.warn('Profile form validation failed:', error.fields)
        } else {
          console.error('Error saving profile:', error)
          authStore.setError('保存个人资料时发生意外错误。')
        }
      } finally {
         authStore.setLoading(false) // 确保加载状态被重置
      }
    }
    
    // 修改密码
    const handleChangePassword = async () => {
      if (!passwordForm.value) return
      passwordError.value = ''
      
      try {
        await passwordForm.value.validate()
        passwordLoading.value = true
        
        await axios.post('/api/users/change-password/', passwordData)
        ElMessage.success('密码修改成功')
        
        // 重置表单并关闭对话框
        passwordData.old_password = ''
        passwordData.new_password1 = ''
        passwordData.new_password2 = ''
        showPasswordDialog.value = false
        passwordError.value = ''
      } catch (error) {
        passwordError.value = '修改密码失败：' + (error.response?.data?.detail || error.response?.data?.message || error.message || '未知错误')
      } finally {
        passwordLoading.value = false
      }
    }
    
    // 生命周期钩子
    onMounted(() => {
      fetchUserProfile()
    })
    
    return {
      profileForm,
      passwordForm,
      formData,
      passwordData,
      rules,
      passwordRules,
      isLoading,
      error,
      showPasswordDialog,
      passwordLoading,
      passwordError,
      handleSaveProfile,
      handleChangePassword
    }
  }
}
</script>

<style scoped>
/* Page Title */
/* .app-page-title {...} removed */

/* Modern Card Style */
/* .app-card {...} removed */

/* .app-card-title {...} removed */

/* Profile Form Styles */
.profile-form .el-form-item {
  margin-bottom: 20px;
}

.profile-form .el-form-item :deep(.el-form-item__label) {
  padding-bottom: 4px;
  line-height: 1.2;
  font-size: 14px;
}

.profile-form .el-input :deep(.el-input__inner) {
  border-radius: 4px;
}

.profile-form .el-input.is-disabled :deep(.el-input__inner) {
  background-color: #f5f7fa;
  color: #a8abb2;
}

.form-actions {
  margin-top: 25px;
  text-align: left; /* Align save button to left */
}

.save-button {
  min-width: 120px; /* Give button some width */
  font-weight: 500;
}

/* Security Card Styles */
.security-item {
  margin-bottom: 20px;
}

.security-item h4 {
  font-size: 16px;
  font-weight: 500;
  margin-bottom: 8px;
  color: #303133;
}

.security-item p {
  font-size: 14px;
  color: #606266;
  margin-bottom: 12px;
  line-height: 1.5;
}

.security-button {
  width: 100%; /* Make buttons full width */
  margin-top: 5px;
}

.el-divider {
  margin: 25px 0; /* More spacing for divider */
}

/* Password Dialog Styles */
.password-dialog :deep(.el-dialog__header) {
  border-bottom: 1px solid #e4e7ed;
  margin-right: 0;
}

.password-dialog :deep(.el-dialog__body) {
  padding: 25px 25px 10px 25px; /* Adjust padding */
}

.password-change-form .el-form-item {
  margin-bottom: 20px;
}

.password-change-form .el-form-item :deep(.el-form-item__label) {
  padding-bottom: 4px;
  line-height: 1.2;
  font-size: 14px;
}

.password-change-form .el-input :deep(.el-input__inner) {
  border-radius: 4px;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.profile-alert {
  margin-bottom: 20px;
}
</style> 