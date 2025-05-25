<template>
  <div class="workflow-instance-list-container">
    <el-card class="workflow-instance-list-card">
      <template #header>
        <div class="card-header">
          <h2>工作流实例列表</h2>
          <el-button type="primary" @click="goToCreate">创建工作流实例</el-button>
        </div>
      </template>
      
      <el-table
        v-loading="loading"
        :data="instances"
        style="width: 100%"
        border
      >
        <el-table-column prop="name" label="名称" min-width="180" />
        <el-table-column prop="workflow.name" label="工作流" min-width="180" />
        <el-table-column prop="status" label="状态" width="120">
          <template #default="scope">
            <el-tag :type="getStatusType(scope.row.status)">
              {{ getStatusText(scope.row.status) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="started_at" label="开始时间" width="180">
          <template #default="scope">
            {{ formatDateTime(scope.row.started_at) }}
          </template>
        </el-table-column>
        <el-table-column prop="finished_at" label="结束时间" width="180">
          <template #default="scope">
            {{ formatDateTime(scope.row.finished_at) }}
          </template>
        </el-table-column>
        <el-table-column label="操作" width="200" fixed="right">
          <template #default="scope">
            <el-button 
              @click="viewDetail(scope.row)" 
              type="primary" 
              size="small"
              plain
            >
              详情
            </el-button>
            <el-button 
              v-if="canCancel(scope.row)" 
              @click="cancelInstance(scope.row)" 
              type="danger" 
              size="small"
              plain
            >
              取消
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      
      <div class="pagination-container">
        <el-pagination
          v-model:current-page="currentPage"
          v-model:page-size="pageSize"
          :page-sizes="[10, 20, 50, 100]"
          layout="total, sizes, prev, pager, next, jumper"
          :total="total"
          @size-change="handleSizeChange"
          @current-change="handleCurrentChange"
        />
      </div>
    </el-card>
  </div>
</template>

<script>
import { ref, onMounted, computed } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import axios from 'axios'

export default {
  name: 'WorkflowInstanceList',
  setup() {
    const router = useRouter()
    const loading = ref(false)
    const instances = ref([])
    const total = ref(0)
    const currentPage = ref(1)
    const pageSize = ref(20)
    
    // 获取工作流实例列表
    const fetchInstances = async () => {
      loading.value = true
      try {
        const params = {
          page: currentPage.value,
          page_size: pageSize.value
        }
        
        const response = await axios.get('/api/workflows/instances/', { params })
        instances.value = response.data.results || []
        total.value = response.data.count || 0
      } catch (error) {
        console.error('获取工作流实例列表失败', error)
        ElMessage.error('获取工作流实例列表失败')
      } finally {
        loading.value = false
      }
    }
    
    // 格式化日期时间
    const formatDateTime = (datetime) => {
      if (!datetime) return '-'
      const date = new Date(datetime)
      return date.toLocaleString('zh-CN')
    }
    
    // 获取状态对应的类型
    const getStatusType = (status) => {
      const types = {
        'created': 'info',
        'running': 'warning',
        'completed': 'success',
        'failed': 'danger',
        'cancelled': 'info',
        'paused': 'info'
      }
      return types[status] || 'info'
    }
    
    // 获取状态对应的文本
    const getStatusText = (status) => {
      const texts = {
        'created': '已创建',
        'running': '运行中',
        'completed': '已完成',
        'failed': '失败',
        'cancelled': '已取消',
        'paused': '已暂停'
      }
      return texts[status] || status
    }
    
    // 是否可以取消实例
    const canCancel = (instance) => {
      return ['running', 'paused'].includes(instance.status)
    }
    
    // 查看详情
    const viewDetail = (instance) => {
      router.push(`/workflow/instance/${instance.instance_id}`)
    }
    
    // 取消实例
    const cancelInstance = (instance) => {
      ElMessageBox.confirm(
        '确定要取消此工作流实例吗？这个操作不可逆。',
        '确认取消',
        {
          confirmButtonText: '确定',
          cancelButtonText: '取消',
          type: 'warning'
        }
      ).then(async () => {
        try {
          await axios.post(`/api/workflows/instances/${instance.instance_id}/cancel/`)
          ElMessage.success('工作流实例已取消')
          fetchInstances()
        } catch (error) {
          console.error('取消工作流实例失败', error)
          ElMessage.error('取消工作流实例失败')
        }
      }).catch(() => {
        // 用户取消操作
      })
    }
    
    // 前往创建页面
    const goToCreate = () => {
      router.push('/workflow')
    }
    
    // 处理页码变化
    const handleCurrentChange = (page) => {
      currentPage.value = page
      fetchInstances()
    }
    
    // 处理每页数量变化
    const handleSizeChange = (size) => {
      pageSize.value = size
      fetchInstances()
    }
    
    onMounted(() => {
      fetchInstances()
    })
    
    return {
      loading,
      instances,
      total,
      currentPage,
      pageSize,
      formatDateTime,
      getStatusType,
      getStatusText,
      canCancel,
      viewDetail,
      cancelInstance,
      goToCreate,
      handleCurrentChange,
      handleSizeChange
    }
  }
}
</script>

<style scoped>
.workflow-instance-list-container {
  padding: 20px;
}

.workflow-instance-list-card {
  width: 100%;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.card-header h2 {
  margin: 0;
}

.pagination-container {
  margin-top: 20px;
  display: flex;
  justify-content: center;
}
</style> 