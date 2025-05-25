<template>
  <div class="workflow-list-container">
    <div class="page-header">
      <h1 class="app-page-title">工作流列表</h1>
      <div class="header-actions">
        <el-button type="primary" @click="$router.push('/workflow/create')">
          <el-icon><plus /></el-icon> 创建新工作流
        </el-button>
        <el-button type="success" @click="showTemplates">
          <el-icon><files /></el-icon> 从模板创建
        </el-button>
      </div>
    </div>

    <el-card class="workflow-filters app-card">
      <el-form :model="filters" :disabled="isLoading">
        <el-row :gutter="20">
          <el-col :xs="24" :sm="12" :md="8" :lg="6">
        <el-form-item label="搜索工作流">
          <el-input v-model="filters.search" placeholder="输入名称或描述搜索" clearable />
        </el-form-item>
          </el-col>
        
          <el-col :xs="24" :sm="12" :md="8" :lg="6">
        <el-form-item label="类型">
              <el-select v-model="filters.workflow_type" placeholder="全部类型" clearable class="full-width">
            <el-option label="企业工作流" value="enterprise" />
            <el-option label="主播工作流" value="streamer" />
            <el-option label="通用工作流" value="general" />
          </el-select>
        </el-form-item>
          </el-col>
        
          <el-col :xs="24" :sm="12" :md="8" :lg="6">
        <el-form-item label="标签">
              <el-select v-model="filters.tags" placeholder="全部标签" multiple clearable class="full-width">
            <el-option 
              v-for="tag in availableTags" 
              :key="tag" 
              :label="tag" 
              :value="tag" 
            />
          </el-select>
        </el-form-item>
          </el-col>
        
          <el-col :xs="24" :sm="12" :md="8" :lg="6">
            <el-form-item label="">
          <el-checkbox v-model="filters.show_templates">显示模板</el-checkbox>
        </el-form-item>
          </el-col>
        
          <el-col :xs="24" :sm="12" :md="8" :lg="6">
            <el-form-item class="filter-buttons">
          <el-button type="primary" @click="fetchWorkflows">
            <el-icon><search /></el-icon> 搜索
          </el-button>
          <el-button @click="resetFilters">
            <el-icon><refresh /></el-icon> 重置
          </el-button>
        </el-form-item>
          </el-col>
        </el-row>
      </el-form>
    </el-card>

    <el-card v-loading="isLoading" class="app-card">
      <template #header>
        <div class="card-header">
          <h3 class="app-card-title">工作流列表</h3>
          <el-button 
            link 
            type="primary" 
            @click="fetchWorkflows">
            <el-icon><refresh /></el-icon> 刷新
          </el-button>
        </div>
      </template>

      <el-alert
        v-if="error"
        :title="error"
        type="error"
        :closable="false"
        show-icon
      />

      <!-- 工作流列表 -->
      <div class="workflow-list">
        <template v-if="paginatedWorkflows.length > 0">
          <el-row :gutter="20">
            <el-col :xs="24" :sm="12" :md="8" :lg="6" v-for="workflow in paginatedWorkflows" :key="workflow.id">
              <el-card 
                class="workflow-card clickable-card app-card" 
                shadow="hover" 
                @click="navigateToDetail(workflow.id)"
              >
                <template #header>
                  <div class="workflow-header">
                    <span class="workflow-name">{{ workflow.name }}</span>
                    <el-tag size="small" :type="getWorkflowTypeTag(workflow.workflow_type)">
                      {{ getWorkflowTypeLabel(workflow.workflow_type) }}
                    </el-tag>
                  </div>
                </template>
                
                <div class="workflow-content">
                  <p class="workflow-description">{{ workflow.description || '无描述' }}</p>
                  
                  <div class="workflow-meta">
                    <div class="meta-item">
                      <el-icon><calendar /></el-icon>
                      <span>{{ formatDate(workflow.created_at) }}</span>
                    </div>
                    
                    <div class="meta-item" v-if="workflow.last_run_at">
                      <el-icon><timer /></el-icon>
                      <span>最近运行: {{ formatDate(workflow.last_run_at) }}</span>
                    </div>
                    
                    <div class="meta-item">
                      <el-icon><user /></el-icon>
                      <span>{{ workflow.user_name || '未知用户' }}</span>
                    </div>
                  </div>
                  
                  <div class="workflow-actions" @click.stop>
                    <el-button-group>
                      <el-button
                        size="small"
                        type="primary"
                        @click="navigateToEdit(workflow.id)"
                        v-if="workflow.can_edit"
                      >
                        <el-icon><edit /></el-icon>
                        编辑
                      </el-button>
                      
                      <el-button
                        size="small"
                        type="success"
                        @click="executeWorkflow(workflow)"
                      >
                        <el-icon><video-play /></el-icon>
                        运行
                      </el-button>
                      
                      <el-button
                        size="small"
                        type="danger"
                        @click="confirmDeleteWorkflow(workflow)"
                        v-if="workflow.can_delete"
                      >
                        <el-icon><delete /></el-icon>
                      </el-button>
                    </el-button-group>
                  </div>
                </div>
              </el-card>
            </el-col>
          </el-row>
        </template>
        <el-empty 
          v-else-if="!isLoading && paginatedWorkflows.length === 0" 
          :description="emptyListDescription" 
        />
      </div>

      <!-- 分页 -->
      <div class="pagination-container" v-if="Array.isArray(fullyFilteredWorkflows) && fullyFilteredWorkflows.length > 0">
        <el-pagination
          v-model:current-page="currentPage"
          v-model:page-size="pageSize"
          :page-sizes="[12, 24, 36, 48]"
          layout="total, sizes, prev, pager, next, jumper"
          :total="fullyFilteredWorkflows.length"
          @size-change="handleSizeChange"
          @current-change="handleCurrentChange"
        />
      </div>
    </el-card>

    <!-- 删除确认对话框 -->
    <el-dialog
      v-model="deleteDialogVisible"
      title="确认删除"
      width="30%"
    >
      <span>确定要删除工作流 "{{ workflowToDelete?.name }}" 吗？此操作不可撤销。</span>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="deleteDialogVisible = false">取消</el-button>
          <el-button 
            type="danger" 
            @click="deleteWorkflow" 
            :loading="isDeleting"
          >
            确认删除
          </el-button>
        </span>
      </template>
    </el-dialog>

    <!-- 工作流执行对话框 -->
    <el-dialog
      v-model="executeDialogVisible"
      title="执行工作流"
      width="50%"
    >
      <template v-if="selectedWorkflow">
        <p><strong>工作流名称:</strong> {{ selectedWorkflow.name }}</p>
        <p><strong>描述:</strong> {{ selectedWorkflow.description }}</p>
        
        <el-divider content-position="left">输入参数</el-divider>
        
        <el-form 
          ref="executeForm" 
          :model="executeParams" 
          :rules="executeRules"
          label-position="top"
        >
          <el-form-item 
            v-for="(param, index) in workflowParams" 
            :key="index"
            :label="param.name || `参数 ${index + 1}`"
            :prop="`params.${param.key}`"
          >
            <div class="param-description" v-if="param.description">
              {{ param.description }}
            </div>
            <el-input 
              v-if="param.type === 'string'" 
              v-model="executeParams.params[param.key]" 
              :placeholder="param.placeholder || '请输入参数值'"
            />
            <el-input-number 
              v-else-if="param.type === 'number'" 
              v-model="executeParams.params[param.key]" 
              :placeholder="param.placeholder || '请输入数值'"
              class="full-width"
            />
            <el-switch 
              v-else-if="param.type === 'boolean'" 
              v-model="executeParams.params[param.key]" 
              :active-text="param.trueLabel || '是'"
              :inactive-text="param.falseLabel || '否'"
            />
            <el-input 
              v-else
              v-model="executeParams.params[param.key]" 
              :placeholder="param.placeholder || '请输入参数值'"
            />
          </el-form-item>
        </el-form>
      </template>
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="executeDialogVisible = false">取消</el-button>
          <el-button type="primary" @click="runWorkflow" :loading="isExecuting">执行</el-button>
        </span>
      </template>
    </el-dialog>
    
    <!-- 模板对话框 -->
    <el-dialog
      v-model="templateDialogVisible"
      title="工作流模板库"
      width="70%"
    >
      <div class="template-filters">
        <el-input 
          v-model="templateFilters.search" 
          placeholder="搜索模板" 
          prefix-icon="Search"
          clearable
          @input="filterTemplates"
        />
        <el-select
          v-model="templateFilters.tag"
          placeholder="按标签筛选"
          clearable
          @change="filterTemplates"
        >
          <el-option
            v-for="tag in availableTags"
            :key="tag"
            :label="tag"
            :value="tag"
          />
        </el-select>
      </div>
      
      <div class="templates-wrapper">
        <el-empty v-if="filteredTemplates.length === 0" description="没有找到匹配的模板" />
        
        <div v-else class="templates-grid">
          <el-card 
            v-for="template in filteredTemplates" 
            :key="template.id"
            class="template-card"
            :body-style="{ padding: '0px' }"
            shadow="hover"
          >
            <div class="template-header">
              <h4>{{ template.name }}</h4>
              <div class="template-tags">
                <el-tag 
                  v-for="tag in template.tags" 
                  :key="tag" 
                  size="small" 
                  effect="plain"
                >
                  {{ tag }}
                </el-tag>
              </div>
            </div>
            <div class="template-body">
              <p class="template-description">{{ template.description || '无描述' }}</p>
              <div class="template-info">
                <span>步骤数: {{ getTemplateStepsCount(template) }}</span>
                <span>类型: {{ getWorkflowTypeLabel(template.workflow_type) }}</span>
              </div>
            </div>
            <div class="template-footer">
              <el-button type="primary" @click="createFromTemplate(template)">
                使用此模板
              </el-button>
              <el-button type="info" plain @click="viewWorkflow(template)">
                查看详情
              </el-button>
            </div>
          </el-card>
        </div>
      </div>
    </el-dialog>
    
    <!-- 保存为模板对话框 -->
    <el-dialog
      v-model="saveTemplateDialogVisible"
      title="保存为模板"
      width="50%"
    >
      <el-form ref="templateForm" :model="templateForm" :rules="templateRules" label-position="top">
        <el-form-item label="模板名称" prop="name">
          <el-input v-model="templateForm.name" placeholder="请输入模板名称" />
        </el-form-item>
        
        <el-form-item label="描述" prop="description">
          <el-input 
            v-model="templateForm.description" 
            type="textarea" 
            :rows="3" 
            placeholder="请输入模板描述"
          />
        </el-form-item>
        
        <el-form-item label="标签" prop="tags">
          <el-select
            v-model="templateForm.tags"
            multiple
            filterable
            allow-create
            default-first-option
            placeholder="请选择或创建标签"
            class="full-width"
          >
            <el-option
              v-for="tag in availableTags"
              :key="tag"
              :label="tag"
              :value="tag"
            />
          </el-select>
        </el-form-item>
      </el-form>
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="saveTemplateDialogVisible = false">取消</el-button>
          <el-button type="primary" @click="saveTemplate">保存</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, reactive, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useWorkflowStore } from '@/store/workflow'
import { useAuthStore } from '@/store/auth'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Plus,
  Search,
  Refresh,
  View,
  Edit,
  Delete,
  VideoPlay,
  Calendar,
  Timer,
  User,
  Star,
  DocumentCopy,
  Files
} from '@element-plus/icons-vue'
import { formatDistanceToNow } from 'date-fns'
import { zhCN } from 'date-fns/locale'

export default {
  name: 'WorkflowList',
  components: {
    Plus,
    Search,
    Refresh,
    View,
    Edit,
    Delete,
    VideoPlay,
    Calendar,
    Timer,
    User,
    Star,
    DocumentCopy,
    Files
  },
  setup() {
    const router = useRouter()
    const workflowStore = useWorkflowStore()
    const authStore = useAuthStore()
    
    // 状态
    const filters = reactive({
      search: '',
      workflow_type: '',
      show_templates: false,
      tags: []
    })
    
    const executeDialogVisible = ref(false)
    const deleteDialogVisible = ref(false)
    const templateDialogVisible = ref(false)
    const saveTemplateDialogVisible = ref(false)
    
    const selectedWorkflow = ref(null)
    const executeParams = reactive({
      params: {}
    })
    
    const isExecuting = ref(false)
    const isDeleting = ref(false)
    const workflowToDelete = ref(null)
    
    // 模板状态
    const templateFilters = reactive({
      search: '',
      tag: ''
    })
    
    const templateForm = reactive({
      id: null,
      name: '',
      description: '',
      tags: []
    })
    
    const templateRules = {
      name: [
        { required: true, message: '请输入模板名称', trigger: 'blur' }
      ]
    }
    
    // 状态数据
    const searchQuery = ref('')
    const typeFilter = ref('')
    const onlyShowMine = ref(false)
    
    // 分页状态
    const currentPage = ref(1)
    const pageSize = ref(12) // Default page size
    
    // 计算属性
    const isLoading = computed(() => workflowStore.isLoading)
    const error = computed(() => workflowStore.error)
    const workflows = computed(() => workflowStore.workflows)
    const currentWorkflow = computed(() => workflowStore.currentWorkflow)
    
    const filteredWorkflows = computed(() => {
      if (!workflowStore.workflows || !Array.isArray(workflowStore.workflows)) {
        console.warn('[WorkflowList] workflowStore.workflows is not an array or is undefined during filteredWorkflows computation. Returning empty array.');
        return [];
      }

      const searchTerm = filters.search ? filters.search.toLowerCase() : '';
      const typeTerm = filters.workflow_type;
      const tagsToFilter = Array.isArray(filters.tags) ? filters.tags : [];
      const showTemplatesFilter = filters.show_templates;

      return workflowStore.workflows.filter(workflow => {
        let matchesSearch = true;
        if (searchTerm) {
          const nameMatch = workflow.name && workflow.name.toLowerCase().includes(searchTerm);
          const descMatch = workflow.description && workflow.description.toLowerCase().includes(searchTerm);
          matchesSearch = nameMatch || descMatch;
        }

        let matchesType = true;
        if (typeTerm) {
          matchesType = workflow.workflow_type === typeTerm;
        }
        
        let matchesTags = true;
        if (tagsToFilter.length > 0) {
          if (Array.isArray(workflow.tags) && workflow.tags.length > 0) {
            matchesTags = tagsToFilter.some(filterTag => workflow.tags.includes(filterTag));
          } else {
            matchesTags = false;
          }
        }
        
        let matchesTemplateFilter = true;
        // Ensure is_template exists or provide a fallback
        const isTemplate = typeof workflow.is_template === 'boolean' ? workflow.is_template : false;

        if (showTemplatesFilter !== undefined) {
            if (showTemplatesFilter) {
                // If show_templates is true, we only want templates
                matchesTemplateFilter = isTemplate === true;
            } else {
                 matchesTemplateFilter = isTemplate !== true; 
            }
        } else {
            // Default behavior if show_templates is not set in filters (e.g. initially)
             matchesTemplateFilter = isTemplate !== true; 
        }

        return matchesSearch && matchesType && matchesTags && matchesTemplateFilter;
      });
    })
    
    const paginatedWorkflows = computed(() => {
      const start = (currentPage.value - 1) * pageSize.value
      const end = start + pageSize.value
      if (filteredWorkflows.value && typeof filteredWorkflows.value.slice === 'function') {
        return filteredWorkflows.value.slice(start, end);
      }
      return [];
    })
    
    const templates = computed(() => {
      return workflows.value.filter(w => w.is_template)
    })
    
    const filteredTemplates = computed(() => {
      let result = [...templates.value]
      
      // 搜索过滤
      if (templateFilters.search) {
        const searchLower = templateFilters.search.toLowerCase()
        result = result.filter(t => 
          (t.name && t.name.toLowerCase().includes(searchLower)) || 
          (t.description && t.description.toLowerCase().includes(searchLower))
        )
      }
      
      // 标签过滤
      if (templateFilters.tag) {
        result = result.filter(t => 
          t.tags && t.tags.includes(templateFilters.tag)
        )
      }
      
      return result
    })
    
    // 获取所有可用标签
    const availableTags = computed(() => {
      const allTags = new Set()
      if (Array.isArray(workflowStore.workflows)) {
        workflowStore.workflows.forEach(wf => {
          if (Array.isArray(wf.tags)) {
            wf.tags.forEach(tag => allTags.add(tag))
          }
        })
      }
      return Array.from(allTags)
    })
    
    // 工作流参数 - 从工作流定义中提取
    const workflowParams = computed(() => {
      if (!selectedWorkflow.value || !selectedWorkflow.value.definition) {
        return []
      }
      
      // 尝试提取工作流参数定义
      try {
        let definition = selectedWorkflow.value.definition
        if (typeof definition === 'string') {
          definition = JSON.parse(definition)
        }
        
        return definition.parameters || []
      } catch (err) {
        console.error('解析工作流参数失败', err)
        return []
      }
    })
    
    // 执行参数验证规则 - 动态生成
    const executeRules = computed(() => {
      const rules = {};
      workflowParams.value.forEach(param => {
        if (param.key && param.required) {
          rules[`params.${param.key}`] = [
            { required: true, message: `${param.name || '此参数'}不能为空`, trigger: 'blur' }
          ];
        }
        // 可以根据 param.type 添加其他验证规则，例如数字范围、字符串长度等
      });
      return rules;
    });
    
    // 获取模板步骤数
    const getTemplateStepsCount = (template) => {
      if (!template.definition) return 0
      
      try {
        let definition = template.definition
        if (typeof definition === 'string') {
          definition = JSON.parse(definition)
        }
        
        return definition.steps ? definition.steps.length : 0
      } catch (err) {
        return 0
      }
    }
    
    // 方法
    const fetchWorkflows = async () => {
      await workflowStore.fetchWorkflows()
    }
    
    const confirmDeleteWorkflow = (workflow) => {
      workflowToDelete.value = workflow
      deleteDialogVisible.value = true
    }
    
    const deleteWorkflow = async () => {
      if (!workflowToDelete.value) return
      isDeleting.value = true
      
      try {
        const success = await workflowStore.deleteWorkflow(workflowToDelete.value.id)
        if (success) {
          ElMessage.success('工作流删除成功')
          deleteDialogVisible.value = false
          workflowToDelete.value = null
        }
      } catch (error) {
        ElMessage.error('删除工作流失败')
      } finally {
        isDeleting.value = false
      }
    }
    
    // 执行工作流
    const executeWorkflow = (workflow) => {
      selectedWorkflow.value = workflow
      executeParams.params = {}
      
      // 初始化默认参数值
      workflowParams.value.forEach(param => {
        if (param.key && param.defaultValue !== undefined) {
          executeParams.params[param.key] = param.defaultValue
        } else if (param.key) {
          // 根据类型设置默认空值
          if (param.type === 'number') {
            executeParams.params[param.key] = 0
          } else if (param.type === 'boolean') {
            executeParams.params[param.key] = false
          } else {
            executeParams.params[param.key] = ''
          }
        }
      })
      
      executeDialogVisible.value = true
    }
    
    // 运行工作流
    const runWorkflow = async () => {
      if (!selectedWorkflow.value) return
      
      // 新增：执行前验证表单
      const executeFormRef = executeForm.value;
      if (!executeFormRef) return;
      
      try {
        await executeFormRef.validate();
        console.log('Execute form validation passed');
      } catch (err) {
        console.warn('Execute form validation failed:', err);
        return; // 验证失败则不继续执行
      }
      
      isExecuting.value = true
      
      try {
        const instance = await workflowStore.startWorkflowInstance(
          selectedWorkflow.value.id, 
          executeParams.params
        )
        
        ElMessage.success('工作流启动成功')
        executeDialogVisible.value = false
        
        // 跳转到实例详情页
        if (instance && instance.instance_id) {
          router.push(`/workflow/instances/${instance.instance_id}`)
        }
      } catch (error) {
        ElMessage.error('工作流启动失败: ' + (error.message || '未知错误'))
      } finally {
        isExecuting.value = false
      }
    }
    
    // 显示模板库
    const showTemplates = () => {
      templateFilters.search = ''
      templateFilters.tag = ''
      templateDialogVisible.value = true
    }
    
    // 筛选模板
    const filterTemplates = () => {
      // 由计算属性自动处理
    }
    
    // 从模板创建
    const createFromTemplate = (template) => {
      if (!template) return
      
      // 准备克隆数据
      const clonedWorkflow = {
        name: `基于 ${template.name} 的工作流`,
        description: template.description,
        workflow_type: template.workflow_type,
        is_public: false,
        definition: template.definition,
        tags: [...(template.tags || [])],
        is_template: false,
        template_id: template.id
      }
      
      // 创建新工作流
      workflowStore.saveWorkflow(clonedWorkflow).then((newWorkflow) => {
        if (newWorkflow && newWorkflow.id) {
          ElMessage.success('成功从模板创建工作流')
          templateDialogVisible.value = false
          router.push(`/workflow/${newWorkflow.id}/edit`)
        }
      }).catch((error) => {
        ElMessage.error('从模板创建工作流失败: ' + (error.message || '未知错误'))
      })
    }
    
    const viewWorkflow = (template) => {
      if (template && template.id) {
        // 在新标签页中打开工作流详情
        const url = router.resolve({ path: `/workflow/${template.id}` }).href;
        window.open(url, '_blank');
      } else {
        console.error('viewWorkflow: template or template.id is undefined', template);
        ElMessage.error('无法查看模板详情，缺少ID');
      }
    };
    
    // 保存为模板
    const saveTemplate = async () => {
      if (!templateForm.id) return
      
      try {
        // 获取原始工作流
        await workflowStore.fetchWorkflow(templateForm.id)
        const originalWorkflow = workflowStore.currentWorkflow
        
        if (!originalWorkflow) {
          ElMessage.error('获取原始工作流失败')
          return
        }
        
        // 创建模板
        const templateData = {
          name: templateForm.name,
          description: templateForm.description,
          workflow_type: originalWorkflow.workflow_type,
          is_public: true,
          definition: originalWorkflow.definition,
          tags: templateForm.tags,
          is_template: true
        }
        
        await workflowStore.saveWorkflow(templateData)
        ElMessage.success('工作流已保存为模板')
        saveTemplateDialogVisible.value = false
        fetchWorkflows()
      } catch (error) {
        ElMessage.error('保存模板失败: ' + (error.message || '未知错误'))
      }
    }
    
    // 工具函数
    const formatDate = (dateString) => {
      if (!dateString) return 'N/A'
      // Ensure dateString is valid before parsing
      try {
        return formatDistanceToNow(new Date(dateString), { addSuffix: true, locale: zhCN })
      } catch (e) {
        console.error("Error formatting date:", dateString, e);
        return 'Invalid Date';
      }
    }
    
    const getWorkflowTypeLabel = (type) => {
      switch (type) {
        case 'enterprise': return '企业';
        case 'streamer': return '主播';
        case 'general': return '通用';
        default: return '未知';
      }
    }
    
    const getWorkflowTypeTag = (type) => {
      switch (type) {
        case 'enterprise': return 'primary';
        case 'streamer': return 'success';
        case 'general': return 'info';
        default: return 'info';
      }
    }
    
    // Navigation functions
    const navigateToDetail = (id) => {
      router.push(`/workflow/${id}`);
    };

    const navigateToEdit = (id) => {
      router.push(`/workflow/${id}/edit`);
    };
    
    // 重置过滤条件
    const resetFilters = () => {
      filters.search = ''
      filters.workflow_type = ''
      filters.tags = []
      filters.show_templates = false // Reset template filter as well
      currentPage.value = 1 // Reset to first page on filter reset
      // No need to call fetchWorkflows() here as computed properties will update automatically
    }
    
    // 分页事件处理
    const handleSizeChange = (val) => {
      pageSize.value = val
      // currentPage.value = 1; // Optionally reset to first page
    }

    const handleCurrentChange = (val) => {
      currentPage.value = val
    }
    
    // 生命周期钩子
    onMounted(() => {
      fetchWorkflows()
      // console.log("Auth User:", authStore.user); // For debugging
    })
    
    const emptyListDescription = computed(() => {
      // This computed property is used when !isLoading and paginatedWorkflows.value.length === 0.
      // This means filteredWorkflows.value.length (the source for paginatedWorkflows) is also 0.

      const hasActiveTextSearch = !!filters.search;
      const hasActiveTypeFilter = !!filters.workflow_type;
      const hasActiveTagFilter = Array.isArray(filters.tags) && filters.tags.length > 0;

      // filters.show_templates === true IS an active filter choice (user wants to see templates).
      // filters.show_templates === false is the default view (user wants to see non-template workflows).
      const isFilteredView = hasActiveTextSearch || hasActiveTypeFilter || hasActiveTagFilter || filters.show_templates === true;

      if (isFilteredView) {
        // User has applied some filters (or is looking specifically at templates) and found nothing.
        if (filters.show_templates === true) {
          // If filters.show_templates is true, it means the user is trying to view templates, and none match other criteria.
          return '没有符合筛选条件的模板。请尝试调整或清除筛选器。';
        } else {
          // If filters.show_templates is false, it means the user is trying to view workflows, and none match other criteria.
          return '没有符合筛选条件的工作流。请尝试调整或清除筛选器。';
        }
      } else {
        // No filters applied by user (search, type, tags are empty), 
        // AND filters.show_templates is false (default view for workflows), 
        // yet the list of (non-template) workflows is empty.
        return '当前还没有任何工作流。您可以点击页面顶部的"创建新工作流"或"从模板创建"开始。';
      }
    });
    
    return {
      filters,
      executeDialogVisible,
      deleteDialogVisible,
      templateDialogVisible,
      saveTemplateDialogVisible,
      selectedWorkflow,
      workflowToDelete,
      executeParams,
      isExecuting,
      isDeleting,
      templateFilters,
      templateForm,
      templateRules,
      executeRules,
      isLoading,
      error,
      currentWorkflow,
      filteredWorkflows,
      paginatedWorkflows,
      emptyListDescription,
      availableTags,
      workflowParams,
      getTemplateStepsCount,
      fetchWorkflows,
      confirmDeleteWorkflow,
      deleteWorkflow,
      resetFilters,
      executeWorkflow,
      runWorkflow,
      showTemplates,
      filterTemplates,
      createFromTemplate,
      viewWorkflow,
      saveTemplate,
      getWorkflowTypeLabel,
      getWorkflowTypeTag,
      formatDate,
      navigateToDetail,
      navigateToEdit,
      currentPage,
      pageSize,
      handleSizeChange,
      handleCurrentChange,
    }
  }
}
</script>

<style scoped>
.workflow-list-container {
  max-width: 1200px;
  margin: 0 auto;
  padding: 20px;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.header-actions {
  display: flex;
  gap: 10px;
}

.workflow-filters {
  margin-bottom: 20px;
}

.filters-form {
  display: flex;
  flex-wrap: wrap;
  gap: 15px;
  align-items: flex-end;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.param-description {
  font-size: 12px;
  color: #606266;
  margin-bottom: 5px;
}

.full-width {
  width: 100%;
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.workflow-tag {
  margin-right: 5px;
}

.no-tags {
  color: #909399;
  font-size: 12px;
}

/* 模板对话框样式 */
.template-filters {
  display: flex;
  gap: 15px;
  margin-bottom: 20px;
  flex-wrap: wrap; /* Allow wrapping on small screens */
  align-items: center; /* Vertically align items */
}

.template-filters .el-input {
  max-width: 300px; /* Limit width of search input */
}

.templates-wrapper {
  max-height: 60vh;
  overflow-y: auto;
  padding: 10px 0;
}

.templates-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 20px;
}

.template-card {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.template-header {
  padding: 15px 20px; /* Add horizontal padding */
  border-bottom: 1px solid #ebeef5;
}

.template-header h4 {
  margin: 0 0 8px 0; /* Adjust bottom margin */
  font-size: 16px;
}

.template-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin-bottom: 10px; /* Add space below tags */
}

.template-body {
  padding: 15px 20px; /* Add horizontal padding */
  flex-grow: 1;
}

.template-description {
  margin-top: 0; /* Keep top margin 0 */
  color: #606266;
  min-height: 40px; /* Ensure minimum height */
  max-height: 60px; /* Optional: add max height to prevent overly long descriptions */
  line-height: 1.4;
  display: -webkit-box;
  -line-clamp: 2; /* Limit to 2 lines */
  -webkit-box-orient: vertical;
  overflow: hidden;
  text-overflow: ellipsis;
}

.template-info {
  display: flex;
  justify-content: space-between;
  color: #909399;
  font-size: 13px; /* Slightly increase font size */
  margin-top: 10px;
}

.template-footer {
  padding: 10px 20px; /* Add horizontal padding */
  border-top: 1px solid #ebeef5;
  display: flex;
  justify-content: space-between;
}

/* Add style for clickable card */
.clickable-card {
  cursor: pointer;
  transition: all 0.2s ease-in-out; /* Add transition for hover effects */
}

.clickable-card:hover {
  transform: scale(1.02); /* Add scale effect on hover */
  box-shadow: 0 6px 16px rgba(0, 0, 0, 0.12); /* Slightly stronger shadow on hover */
}

.workflow-list {
  margin-top: 20px;
}

.workflow-card {
  margin-bottom: 20px;
}

.workflow-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

/* Add padding to card header */
.workflow-card :deep(.el-card__header) {
  padding: 15px 20px; /* Match padding of workflow-content */
}

.workflow-name {
  font-weight: bold;
  font-size: 1.1em; /* Increase font size */
  color: #303133; /* Ensure good contrast */
}

.workflow-content {
  /* Add styles if needed */
  padding: 15px 20px; /* Adjust padding */
}

.workflow-description {
  font-size: 14px;
  color: #606266;
  margin-bottom: 15px;
  min-height: 40px; /* Ensure minimum height */
  line-height: 1.4;
  display: -webkit-box;
  -line-clamp: 2; /* Limit to 2 lines */
  -webkit-box-orient: vertical;
  overflow: hidden;
  text-overflow: ellipsis;
}

.workflow-meta {
  font-size: 12px;
  color: #909399;
  margin-bottom: 15px;
  display: flex; /* Use flexbox for alignment */
  flex-wrap: wrap; /* Allow items to wrap */
  gap: 10px; /* Add gap between wrapped items */
}

/* Style for individual meta items */
.workflow-meta .meta-item {
  display: flex;
  align-items: center; /* Vertically align icon and text */
  gap: 5px; /* Space between icon and text */
}

/* Pagination container styles */
.pagination-container {
  margin-top: 20px;
  display: flex;
  justify-content: flex-end; /* Align pagination to the right */
}

/* Style for filter buttons container */
.filters-form .filter-buttons {
  width: 100%; /* Ensure it takes full width in its column */
  display: flex;
  justify-content: flex-end; /* Align buttons to the right */
}

/* All app-page-title, app-card, app-card-title definitions are removed from here */

.workflow-list {
  margin-top: 20px;
}
</style> 