<template>
  <div class="workflow-editor-container">
    <div class="page-header">
      <h1 class="app-page-title">{{ getPageTitle }}</h1>
      <div class="header-actions">
        <el-button @click="$router.push('/workflow')">
          <el-icon><back /></el-icon> 返回
        </el-button>
        <el-button type="primary" @click="saveWorkflow" :loading="isLoading">
          <el-icon><circle-check /></el-icon> 保存工作流
        </el-button>
      </div>
    </div>
    
    <el-card class="editor-card app-card" v-loading="isLoading">
      <el-tabs v-model="activeTab" class="editor-tabs">
        <el-tab-pane :label="isDesignerMode ? '可视化设计器' : '可视化编辑器'" name="editor">
          <!-- 如果是设计器模式，显示额外提示 -->
          <div v-if="isDesignerMode" class="designer-info">
            <el-alert
              title="设计器模式"
              type="info"
              :closable="false"
              show-icon
            >
              <p>在设计器模式中，您可以使用高级功能来设计复杂的工作流。所有更改将作为新工作流保存。</p>
            </el-alert>
          </div>
          
          <div class="step-toolbar">
            <el-button-group>
              <el-button type="success" @click="addNode('a2a_client')">
                <el-icon><connection /></el-icon> 添加A2A步骤
              </el-button>
              <el-button type="primary" @click="addNode('condition')">
                <el-icon><switch /></el-icon> 添加条件步骤
              </el-button>
              <el-button type="warning" @click="addNode('loop')">
                <el-icon><refresh-right /></el-icon> 添加循环步骤
              </el-button>
              <el-button type="danger" @click="addNode('transform')">
                <el-icon><refresh /></el-icon> 添加转换步骤
              </el-button>
            </el-button-group>
          </div>
          
          <div class="workflow-canvas">
            <div v-if="workflow.steps.length === 0" class="canvas-empty">
              <el-empty description="点击上方按钮添加工作流步骤">
                <el-button type="primary" @click="addNode('a2a_client')">
                  添加第一个步骤
                </el-button>
              </el-empty>
            </div>
            
            <div v-else class="workflow-nodes">
              <workflow-node
                v-for="(step, index) in workflow.steps"
                :key="step.id"
                :step="step"
                :index="index"
                @edit="editNode"
                @remove="confirmRemoveNode"
                @move-up="moveNodeUp(index)"
                @move-down="moveNodeDown(index)"
              />
            </div>
          </div>
        </el-tab-pane>
        
        <el-tab-pane label="JSON编辑器" name="json">
          <p class="text-muted">您可以直接编辑工作流定义的JSON结构。注意：修改此处将覆盖可视化编辑器中的内容。</p>
          <codemirror
            v-model="jsonDefinition"
            placeholder="工作流定义 (JSON格式)..."
            :style="{ height: '400px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
            :autofocus="true"
            :indent-with-tab="true"
            :tab-size="2"
            :extensions="cmExtensions"
            @ready="handleCmReady"
            class="json-editor-cm"
          />
          <div class="json-actions">
            <el-button type="primary" @click="updateFromJson">
              应用JSON更改
            </el-button>
            <el-button type="warning" @click="formatJson">
              格式化JSON
            </el-button>
          </div>
          
          <!-- JSON 错误信息显示区域 -->
          <el-alert
            v-if="jsonError"
            :title="jsonError"
            type="error"
            show-icon
            :closable="false"
            class="json-error-alert mt-3"
          />
        </el-tab-pane>
        
        <el-tab-pane label="工作流设置" name="settings">
          <el-form 
            ref="settingsForm" 
            :model="workflow" 
            :rules="settingsRules" 
            label-position="top"
          >
            <el-form-item label="工作流名称" prop="name">
              <el-input v-model="workflow.name" placeholder="请输入工作流名称" />
            </el-form-item>
            
            <el-form-item label="描述" prop="description">
              <el-input 
                v-model="workflow.description" 
                type="textarea" 
                :rows="3" 
                placeholder="请输入工作流描述"
              />
            </el-form-item>
            
            <el-form-item label="工作流类型" prop="workflow_type">
              <el-select v-model="workflow.workflow_type" placeholder="请选择工作流类型" class="full-width">
                <el-option label="企业工作流" value="enterprise" />
                <el-option label="主播工作流" value="streamer" />
                <el-option label="通用工作流" value="general" />
              </el-select>
            </el-form-item>
            
            <el-form-item label="标签" prop="tags">
              <el-select
                v-model="workflow.tags"
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
              <div class="help-text">标签可用于分类和筛选工作流</div>
            </el-form-item>
            
            <el-form-item>
              <el-checkbox v-model="workflow.is_public">公开此工作流（其他用户可见并使用）</el-checkbox>
            </el-form-item>
          </el-form>
        </el-tab-pane>
        
        <el-tab-pane label="参数定义" name="parameters">
          <p class="text-muted">定义执行此工作流时需要的输入参数</p>
          
          <div class="parameters-header">
            <el-button type="primary" size="small" @click="addParameter">
              <el-icon><plus /></el-icon> 添加参数
            </el-button>
          </div>
          
          <el-empty v-if="!workflowParameters.length" description="暂无参数定义">
            <el-button type="primary" @click="addParameter">添加第一个参数</el-button>
          </el-empty>
          
          <el-table v-else :data="workflowParameters" border stripe style="width: 100%">
            <el-table-column label="参数名称" prop="name" width="180">
              <template #default="scope">
                <el-input v-model="scope.row.name" placeholder="参数名称" size="small" />
              </template>
            </el-table-column>
            
            <el-table-column label="参数键" prop="key" width="150">
              <template #default="scope">
                <el-input v-model="scope.row.key" placeholder="参数键 (变量名)" size="small" />
              </template>
            </el-table-column>
            
            <el-table-column label="类型" prop="type" width="120">
              <template #default="scope">
                <el-select v-model="scope.row.type" placeholder="参数类型" class="full-width" size="small">
                  <el-option label="字符串" value="string" />
                  <el-option label="数字" value="number" />
                  <el-option label="布尔值" value="boolean" />
                  <el-option label="JSON对象" value="object" />
                </el-select>
              </template>
            </el-table-column>
            
            <el-table-column label="必填" prop="required" width="80" align="center">
              <template #default="scope">
                <el-checkbox v-model="scope.row.required" />
              </template>
            </el-table-column>
            
            <el-table-column label="默认值" prop="defaultValue">
              <template #default="scope">
                <el-input 
                  v-if="scope.row.type === 'string'" 
                  v-model="scope.row.defaultValue" 
                  placeholder="默认值" 
                  size="small"
                />
                <el-input-number 
                  v-else-if="scope.row.type === 'number'" 
                  v-model="scope.row.defaultValue" 
                  placeholder="0" 
                  class="full-width"
                  size="small"
                  controls-position="right"
                />
                <el-switch 
                  v-else-if="scope.row.type === 'boolean'" 
                  v-model="scope.row.defaultValue"
                />
                <codemirror
                  v-else-if="scope.row.type === 'object'" 
                  v-model="scope.row.defaultValueStr" 
                  placeholder='输入JSON对象, e.g., {} 或 {"key": "value"}'
                  :style="{ height: '80px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
                  :indent-with-tab="true"
                  :tab-size="2"
                  :extensions="cmExtensions"
                  @change="() => handleParamJsonChange(scope.row)" 
                  class="param-json-editor-cm"
                />
              </template>
            </el-table-column>
            
            <el-table-column label="描述" prop="description">
              <template #default="scope">
                <el-input v-model="scope.row.description" placeholder="参数描述" size="small" />
              </template>
            </el-table-column>
            
            <el-table-column label="操作" width="100" fixed="right">
              <template #default="scope">
                <el-button 
                  size="small" 
                  type="danger" 
                  circle
                  icon="el-icon-delete"
                  @click="confirmRemoveParameter(scope.$index)"
                />
              </template>
            </el-table-column>
          </el-table>
        </el-tab-pane>
      </el-tabs>
    </el-card>
    
    <!-- 步骤编辑对话框 -->
    <workflow-node-edit
      v-model:visible="nodeEditVisible"
      :node="currentNode"
      @save="saveNode"
    />
    
    <!-- 删除确认对话框 -->
    <el-dialog
      v-model="removeDialogVisible"
      title="确认删除"
      width="30%"
    >
      <span>确定要删除此步骤吗？此操作不可撤销。</span>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="removeDialogVisible = false">取消</el-button>
          <el-button type="danger" @click="removeNode">确认删除</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, reactive, computed, watch, onMounted, shallowRef } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useWorkflowStore } from '@/store/workflow'
import { useAgentStore } from '@/store/agent'
import { ElMessage, ElMessageBox } from 'element-plus'
import { v4 as uuidv4 } from 'uuid'

// 实例化路由和仓库
const route = useRoute();
const router = useRouter();
const workflowStore = useWorkflowStore();
const agentStore = useAgentStore(); // 如果有用到 agentStore，也实例化

// UI状态和表单引用
const isLoading = ref(false);
const settingsForm = ref(null);

import {
  Back,
  CircleCheck,
  Connection,
  Switch,
  RefreshRight,
  Refresh,
  Plus,
  Delete
} from '@element-plus/icons-vue'
import WorkflowNode from '@/components/workflow/WorkflowNode.vue'
import WorkflowNodeEdit from '@/components/workflow/WorkflowNodeEdit.vue'

// Codemirror imports
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark' // Optional: choose a theme

// 状态
const activeTab = ref('editor')
const jsonDefinition = ref('{}')
const nodeEditVisible = ref(false)
const currentNode = ref(null)
const removeDialogVisible = ref(false)
const nodeToRemove = ref(null)

// JSON 校验错误状态
const jsonError = ref(null);

// 工作流数据
const workflow = reactive({
  id: null,
  name: '',
  description: '',
  workflow_type: 'general',
  is_public: false,
  tags: [],
  steps: [],
  definition: {}
})

// 工作流参数
const workflowParameters = ref([])

// 表单校验规则
const settingsRules = {
  name: [
    { required: true, message: '请输入工作流名称', trigger: 'blur' }
  ],
  workflow_type: [
    { required: true, message: '请选择工作流类型', trigger: 'change' }
  ]
}

// 是否是编辑模式
const isEdit = computed(() => {
  return route.name === 'WorkflowEdit'
})

// 检查是否是设计器模式
const isDesignerMode = computed(() => {
  return route.name === 'WorkflowDesigner'
})

// 获取所有可用标签
const availableTags = ref(['重要', '测试', '生产', '实验性'])

// 从JSON更新可视化状态 (简化版，实际需要更复杂的逻辑)
const updateVisualFromDefinition = () => {
  try {
    const def = JSON.parse(jsonDefinition.value)
    workflow.name = def.name || workflow.name
    workflow.description = def.description || workflow.description
    workflow.workflow_type = def.workflow_type || workflow.workflow_type
    workflow.tags = def.tags || workflow.tags
    workflow.is_public = typeof def.is_public === 'boolean' ? def.is_public : workflow.is_public
    
    workflow.steps = def.steps || [] 
    
    // Ensure workflowParameters are properly initialized for reactivity, especially defaultValueStr
    workflowParameters.value = (def.parameters || []).map(p => ({
      ...p,
      defaultValueStr: typeof p.defaultValue === 'object' ? JSON.stringify(p.defaultValue, null, 2) : 
                       typeof p.defaultValue === 'string' && p.type === 'object' ? p.defaultValue : // if it's already a string (e.g. "{}")
                       p.type === 'object' ? '{}' : // default for new object params
                       p.defaultValue // for other types
    }))
    
    workflow.definition = def 

  } catch (error) {
    ElMessage.error('JSON格式无效，无法更新可视化编辑器')
    console.error("Error parsing JSON for visual update:", error)
  }
}

// 从可视化状态更新JSON (简化版)
const updateJsonFromVisual = () => {
  try {
    // Ensure defaultValue is correctly set from defaultValueStr for object types
    const processedParameters = workflowParameters.value.map(p => {
      if (p.type === 'object') {
        try {
          return { ...p, defaultValue: JSON.parse(p.defaultValueStr || '{}') }
        } catch (e) {
          console.warn(`Invalid JSON for parameter ${p.key}: ${p.defaultValueStr}. Using empty object.`)
          return { ...p, defaultValue: {} }
        }
      }
      return p
    })

    const definitionForBackend = {
      name: workflow.name,
      description: workflow.description,
      workflow_type: workflow.workflow_type,
      tags: workflow.tags,
      is_public: workflow.is_public,
      steps: workflow.steps,
      parameters: processedParameters, // Use processed parameters
    }
    jsonDefinition.value = JSON.stringify(definitionForBackend, null, 2)
    workflow.definition = definitionForBackend
  } catch (error) {
    ElMessage.error('无法序列化工作流到JSON')
    console.error("Error serializing workflow to JSON:", error)
  }
}

// 添加节点
const addNode = (type) => {
  const newNode = {
    id: uuidv4(),
    type: type,
    name: getDefaultNodeName(type),
    config: getDefaultNodeConfig(type)
  }
  
  workflow.steps.push(newNode)
  editNode(newNode)
}

// 编辑节点
const editNode = (node) => {
  currentNode.value = node
  nodeEditVisible.value = true
}

// 保存节点
const saveNode = (node) => {
  const index = workflow.steps.findIndex(step => step.id === node.id)
  if (index !== -1) {
    workflow.steps[index] = { ...node }
  }
  nodeEditVisible.value = false
  
  // 更新JSON
  updateJsonFromVisual()
}

// 确认删除节点
const confirmRemoveNode = (node) => {
  nodeToRemove.value = node
  removeDialogVisible.value = true
}

// 删除节点
const removeNode = () => {
  if (!nodeToRemove.value) return
  
  const index = workflow.steps.findIndex(step => step.id === nodeToRemove.value.id)
  if (index !== -1) {
    workflow.steps.splice(index, 1)
  }
  
  removeDialogVisible.value = false
  nodeToRemove.value = null
  
  // 更新JSON
  updateJsonFromVisual()
}

// 上移节点
const moveNodeUp = (index) => {
  if (index <= 0) return
  
  const temp = workflow.steps[index]
  workflow.steps[index] = workflow.steps[index - 1]
  workflow.steps[index - 1] = temp
  
  // 更新JSON
  updateJsonFromVisual()
}

// 下移节点
const moveNodeDown = (index) => {
  if (index >= workflow.steps.length - 1) return
  
  const temp = workflow.steps[index]
  workflow.steps[index] = workflow.steps[index + 1]
  workflow.steps[index + 1] = temp
  
  // 更新JSON
  updateJsonFromVisual()
}

// 添加参数
const addParameter = () => {
  const newParameter = {
    name: '新参数',
    key: 'param' + (workflowParameters.value.length + 1),
    type: 'string',
    required: false,
    description: '',
    defaultValueStr: ''
  }
  
  if (newParameter.type === 'object') {
    newParameter.defaultValue = {}
    newParameter.defaultValueStr = '{}'
  } else if (newParameter.type === 'number') {
    newParameter.defaultValue = 0
  } else if (newParameter.type === 'boolean') {
    newParameter.defaultValue = false
  } else { // string and others
    newParameter.defaultValue = ''
  }
  
  workflowParameters.value.push(newParameter)
  updateJsonFromVisual()
}

// 删除参数
const removeParameter = (index) => {
  workflowParameters.value.splice(index, 1)
  
  // 更新JSON
  updateJsonFromVisual()
}

// 保存工作流
const saveWorkflow = async () => {
  if (!settingsForm.value) return

  isLoading.value = true; // 添加：开始保存时设置加载状态
  
  try {
    await settingsForm.value.validate()
    
    // 确保在保存前，最新的可视化/参数状态已同步到 workflow.definition
    updateJsonFromVisual()
    
    // 准备提交给后端的数据，确保 definition 是一个对象
    let definitionToSave
    try {
      definitionToSave = JSON.parse(jsonDefinition.value)
    } catch (e) {
      ElMessage.error('工作流定义 (JSON) 格式无效，无法保存。请在JSON编辑器中修正。')
      activeTab.value = 'json'
      return
    }
    
    const payload = {
      id: workflow.id,
      name: workflow.name,
      description: workflow.description,
      workflow_type: workflow.workflow_type,
      tags: workflow.tags,
      is_public: workflow.is_public,
      definition: definitionToSave
    }
    
    await workflowStore.saveWorkflow(payload)
    ElMessage.success('工作流保存成功')
    
    router.push('/workflow')
  } catch (error) {
    console.error('Failed to save workflow:', error)
    // 可以在这里添加通用的错误提示
    ElMessage.error('保存工作流失败。'); // 添加：保存失败时显示错误提示
  } finally {
    isLoading.value = false; // 添加：保存完成后无论成功或失败都设置加载状态为 false
  }
}

// 获取默认节点名称
const getDefaultNodeName = (type) => {
  const typeNames = {
    'a2a_client': 'A2A步骤',
    'condition': '条件步骤',
    'loop': '循环步骤',
    'transform': '转换步骤'
  }
  return typeNames[type] || '新步骤'
}

// 获取默认节点配置
const getDefaultNodeConfig = (type) => {
  switch (type) {
    case 'a2a_client':
      return {
        agent_id: '',
        input_mapping: {},
        output_mapping: {},
        timeout: 300
      }
    case 'condition':
      return {
        condition: '',
        true_branch: [],
        false_branch: []
      }
    case 'loop':
      return {
        condition: '',
        max_iterations: 10,
        body: []
      }
    case 'transform':
      return {
        expression: '',
        output_var: 'result'
      }
    default:
      return {}
  }
}

// 更新对象类型默认值
const updateObjectDefault = (parameter) => {
  try {
    parameter.defaultValue = JSON.parse(parameter.defaultValueStr)
  } catch (e) {
    ElMessage.error('参数默认值 (JSON对象) 格式无效')
    parameter.defaultValue = {} // 重置为有效JSON
  }
}

// 格式化JSON
const formatJson = () => {
  try {
    const parsed = JSON.parse(jsonDefinition.value)
    jsonDefinition.value = JSON.stringify(parsed, null, 2)
  } catch (e) {
    ElMessage.error('JSON格式无效，无法格式化。')
  }
}

// 更新JSON
const updateFromJson = () => {
  updateVisualFromDefinition()
}

// 处理参数JSON变化的函数
const handleParamJsonChange = (paramRow) => {
  if (paramRow.type === 'object') {
    try {
      paramRow.defaultValue = JSON.parse(paramRow.defaultValueStr || '{}')
    } catch (e) {
      // Optionally show an error or revert to a safe default
      console.warn(`Invalid JSON in parameter ${paramRow.key}: ${paramRow.defaultValueStr}`)
      paramRow.defaultValue = {} // Fallback to empty object
      // Potentially update defaultValueStr to '{}' to reflect the parse failure in the editor
      // paramRow.defaultValueStr = '{}'; 
    }
  }
  updateJsonFromVisual() // Update main JSON definition after any param change
}

// 添加确认删除参数的函数，原表格中的删除按钮直接调用了 removeParameter
const confirmRemoveParameter = (index) => {
   ElMessageBox.confirm('确定要删除此参数吗？此操作不可撤销。', '确认删除参数', {
      confirmButtonText: '确定',
      cancelButtonText: '取消',
      type: 'warning',
   }).then(() => {
      removeParameter(index);
      ElMessage.success('参数已删除');
   }).catch(() => {
      // 用户取消操作
   });
};

// 监视活动标签页的变化，并在切换到 JSON 或从 JSON 切换时同步
watch(activeTab, (newTab, oldTab) => {
  if (newTab === 'json' && oldTab !== 'json') {
    updateJsonFromVisual()
    // 当切换到JSON编辑器时，也立即进行一次校验
    validateJson();
  } else if (newTab !== 'json' && oldTab === 'json') {
    updateVisualFromDefinition()
  }
})

// 监视 workflow.steps 和 workflowParameters 的变化，自动更新 JSON (如果不在 JSON 编辑器 tab)
watch([() => workflow.steps, workflowParameters], () => {
  if (activeTab.value !== 'json') {
    updateJsonFromVisual()
  }
}, { deep: true })

// 监视 jsonDefinition 的变化，进行实时 JSON 校验
watch(jsonDefinition, (newValue) => {
  if (activeTab.value === 'json') { // 只在 JSON 编辑器标签页时进行实时校验
    validateJson(newValue);
  }
});

// JSON 校验函数
const validateJson = (jsonString) => {
    try {
        JSON.parse(jsonString);
        jsonError.value = null; // Valid JSON
    } catch (e) {
        // Attempt to extract more specific error message and position if available
        let errorMessage = 'JSON格式无效: ' + e.message;
        
        // Basic attempt to get line number (may not work for all JSON parse errors)
        const match = e.message.match(/at position (\d+)/);
        if (match && cmView.value) {
            const position = parseInt(match[1], 10);
            const { line, ch } = cmView.value.state.doc.lineAt(position);
            errorMessage += ` (位于行 ${line}, 列 ${ch})`;
        }
        
        jsonError.value = errorMessage;
        console.error("JSON validation error:", e);
    }
};

// 初始化数据
const initializeWorkflow = async () => {
  isLoading.value = true
  const id = route.params.id
  if (id) {
    await workflowStore.fetchWorkflow(id)
      const fetchedWorkflow = workflowStore.currentWorkflow
    if (fetchedWorkflow) {
      workflow.id = fetchedWorkflow.id
      workflow.name = fetchedWorkflow.name || ''
      workflow.description = fetchedWorkflow.description || ''
      workflow.workflow_type = fetchedWorkflow.workflow_type || 'general'
      workflow.tags = fetchedWorkflow.tags || []
      workflow.is_public = fetchedWorkflow.is_public || false
      
      let definition = {}
      if (typeof fetchedWorkflow.definition === 'string') {
        try {
          definition = JSON.parse(fetchedWorkflow.definition)
        } catch (e) {
          console.error("Failed to parse fetched workflow definition:", e)
          ElMessage.error("加载工作流定义失败：JSON格式错误。")
          definition = { name: workflow.name, steps: [], parameters: [] } // Fallback
        }
      } else if (typeof fetchedWorkflow.definition === 'object' && fetchedWorkflow.definition !== null) {
        definition = fetchedWorkflow.definition
      }

      workflow.steps = definition.steps || []
      // Initialize workflowParameters with defaultValueStr
      workflowParameters.value = (definition.parameters || []).map(p => ({
        ...p,
        defaultValueStr: typeof p.defaultValue === 'object' ? 
                         JSON.stringify(p.defaultValue, null, 2) : 
                         (p.type === 'object' ? (p.defaultValue || '{}') : undefined) 
                         // Ensure defaultValueStr for object is string, even if defaultValue is string like "{}"
      }))
      
      // Set the main JSON editor content
      jsonDefinition.value = JSON.stringify(definition, null, 2)
      workflow.definition = definition // also store the parsed object

    } else {
      ElMessage.error('无法加载工作流数据')
      router.push('/workflow')
    }
  } else {
    // 新建工作流，确保 JSON 定义和可视化部分同步
    updateJsonFromVisual()
  }
  isLoading.value = false
}

onMounted(() => {
  initializeWorkflow()
})

// Codemirror an shallowRef for extensions to avoid deep reactivity
const cmView = shallowRef()
const handleCmReady = (payload) => {
  cmView.value = payload.view
}
const cmExtensions = [json(), oneDark] // Add more extensions if needed

// 计算页面标题
const getPageTitle = computed(() => {
  if (isEdit.value) {
    return `编辑工作流 - ${workflow.name}`
  }
  return isDesignerMode.value ? '工作流设计器' : '创建新工作流'
})
</script>

<style scoped>
.workflow-editor-container {
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

.editor-card {
  margin-bottom: 30px;
}

.editor-tabs {
  width: 100%;
}

.step-toolbar {
  margin-bottom: 20px;
  padding: 15px;
  background-color: #f5f7fa;
  border-radius: 4px;
}

.workflow-canvas {
  min-height: 400px;
  border: 1px dashed #ddd;
  padding: 20px;
  border-radius: 4px;
  background-color: #fafafa;
}

.canvas-empty {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 360px;
}

.workflow-nodes {
  display: flex;
  flex-direction: column;
  gap: 15px;
}

.json-editor-cm .cm-editor,
.param-json-editor-cm .cm-editor {
  outline: none; /* Remove Codemirror's default outline if desired */
}

.param-json-editor-cm .cm-editor {
  font-size: 0.9em; /* Slightly smaller font for nested editor */
}

.json-actions {
  margin-top: 15px;
  display: flex;
  gap: 10px;
}

.parameters-header {
  margin-bottom: 15px;
  display: flex;
  justify-content: flex-start;
}

.help-text {
  font-size: 12px;
  color: #909399;
  margin-top: 5px;
}

.text-muted {
  font-size: 14px;
  color: #606266;
  margin-bottom: 10px;
}

.full-width {
  width: 100%;
}

/* Styles for el-tabs to ensure content visibility */
.editor-tabs :deep(.el-tabs__content) {
  overflow: visible; 
}

.designer-info {
  margin-bottom: 20px;
}
</style> 