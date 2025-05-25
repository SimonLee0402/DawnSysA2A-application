<template>
  <el-dialog
    v-model="dialogVisible"
    :title="getTitle"
    width="60%"
    @close="handleClose"
  >
    <!-- 主节点编辑表单 -->
    <el-form
      v-if="!isEditingSubNode"
      ref="nodeForm"
      :model="editingNode"
      :rules="formRules"
      label-position="top"
    >
      <el-form-item label="步骤名称" prop="name">
        <el-input v-model="editingNode.name" placeholder="请输入步骤名称" />
      </el-form-item>
      
      <!-- A2A客户端步骤配置 -->
      <template v-if="editingNode.type === 'a2a_client'">
        <el-form-item label="选择智能体" prop="config.agent_id">
          <el-select 
            v-model="editingNode.config.agent_id" 
            placeholder="请选择智能体"
            class="full-width"
            filterable
          >
            <el-option
              v-for="agent in agents"
              :key="agent.id"
              :label="agent.name"
              :value="agent.id"
            />
          </el-select>
        </el-form-item>
        
        <el-form-item label="超时时间 (秒)" prop="config.timeout">
          <el-input-number 
            v-model="editingNode.config.timeout" 
            :min="1" 
            :max="3600"
            class="full-width"
          />
        </el-form-item>
        
        <el-divider>输入映射</el-divider>
        <p class="help-text">定义从工作流变量到智能体输入参数的映射</p>
        
        <div class="mapping-container">
          <div v-for="(_, index) in inputMappings" :key="index" class="mapping-row">
            <el-input
              v-model="inputKeys[index]"
              placeholder="目标参数名"
              class="mapping-key"
            />
            <span class="mapping-equals">=</span>
            <codemirror
              v-model="inputValues[index]"
              placeholder="工作流变量或表达式 (e.g., context.var1)"
              :style="{ flex: 1, height: '32px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
              :extensions="cmInlineJsExtensions"
              class="mapping-value-cm"
            />
            <el-button 
              type="danger" 
              circle 
              size="small"
              @click="removeInputMapping(index)"
            >
              <el-icon><delete /></el-icon>
            </el-button>
          </div>
          
          <el-button 
            type="primary" 
            plain 
            size="small"
            @click="addInputMapping"
          >
            <el-icon><plus /></el-icon> 添加输入映射
          </el-button>
        </div>
        
        <el-divider>输出映射</el-divider>
        <p class="help-text">定义从智能体输出结果到工作流变量的映射</p>
        
        <div class="mapping-container">
          <div v-for="(_, index) in outputMappings" :key="index" class="mapping-row">
            <el-input
              v-model="outputKeys[index]"
              placeholder="工作流变量名"
              class="mapping-key"
            />
            <span class="mapping-equals">=</span>
            <codemirror
              v-model="outputValues[index]"
              placeholder="智能体输出字段或表达式 (e.g., output.text)"
              :style="{ flex: 1, height: '32px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
              :extensions="cmInlineJsExtensions"
              class="mapping-value-cm"
            />
            <el-button 
              type="danger" 
              circle 
              size="small"
              @click="removeOutputMapping(index)"
            >
              <el-icon><delete /></el-icon>
            </el-button>
          </div>
          
          <el-button 
            type="primary" 
            plain 
            size="small"
            @click="addOutputMapping"
          >
            <el-icon><plus /></el-icon> 添加输出映射
          </el-button>
        </div>
      </template>
      
      <!-- 条件步骤配置 -->
      <template v-else-if="editingNode.type === 'condition'">
        <el-form-item label="条件表达式" prop="config.condition">
          <codemirror
            v-model="editingNode.config.condition" 
            placeholder="e.g. context.result > 10" 
            :style="{ height: '100px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
            :extensions="cmJsExtensions"
            class="expression-editor-cm"
          />
          <div class="help-text">
            输入一个JavaScript条件表达式，可以引用工作流变量，如：context.temperature > 30
          </div>
        </el-form-item>
        
        <el-divider>分支配置</el-divider>
        
        <el-tabs type="border-card">
          <el-tab-pane label="True分支">
            <div class="branch-editor">
              <el-alert type="info" :closable="false" show-icon style="margin-bottom: 10px;">
                编辑内嵌步骤的详细配置（如A2A参数、条件表达式等），请关闭此对话框并在主流程编辑器中操作。此处的"编辑"仅可修改步骤名称。
              </el-alert>
              <div class="branch-actions">
                <el-button type="success" size="small" @click="addBranchStep('true')">
                  <el-icon><plus /></el-icon> 添加步骤
                </el-button>
              </div>
              
              <el-empty v-if="!editingNode.config.true_branch || editingNode.config.true_branch.length === 0"
                description="此分支内尚无步骤">
              </el-empty>
              
              <div v-else class="branch-steps">
                <div v-for="(step, index) in editingNode.config.true_branch" :key="step.id" class="branch-step">
                  <div class="step-info">
                    <el-tag size="small" :type="getNodeTypeTag(step.type)" class="step-type-tag">
                      {{ getNodeTypeLabel(step.type) }}
                    </el-tag>
                    <span class="step-name">{{ step.name }}</span>
                  </div>
                  <div class="step-actions">
                    <el-button-group>
                      <el-button size="small" circle @click="editBranchStep('true', step)"
                        title="编辑步骤">
                        <el-icon><edit /></el-icon>
                      </el-button>
                      <el-button size="small" circle @click="moveBranchStepUp('true', index)"
                        :disabled="index === 0" title="上移">
                        <el-icon><top /></el-icon>
                      </el-button>
                      <el-button size="small" circle @click="moveBranchStepDown('true', index)"
                        :disabled="index === editingNode.config.true_branch.length - 1" title="下移">
                        <el-icon><bottom /></el-icon>
                      </el-button>
                      <el-button size="small" type="danger" circle @click="removeBranchStep('true', index)"
                        title="删除步骤">
                        <el-icon><delete /></el-icon>
                      </el-button>
                    </el-button-group>
                  </div>
                </div>
              </div>
            </div>
          </el-tab-pane>
          
          <el-tab-pane label="False分支">
            <div class="branch-editor">
              <el-alert type="info" :closable="false" show-icon style="margin-bottom: 10px;">
                编辑内嵌步骤的详细配置（如A2A参数、条件表达式等），请关闭此对话框并在主流程编辑器中操作。此处的"编辑"仅可修改步骤名称。
              </el-alert>
              <div class="branch-actions">
                <el-button type="danger" size="small" @click="addBranchStep('false')">
                  <el-icon><plus /></el-icon> 添加步骤
                </el-button>
              </div>
              
              <el-empty v-if="!editingNode.config.false_branch || editingNode.config.false_branch.length === 0"
                description="此分支内尚无步骤">
              </el-empty>
              
              <div v-else class="branch-steps">
                <div v-for="(step, index) in editingNode.config.false_branch" :key="step.id" class="branch-step">
                  <div class="step-info">
                    <el-tag size="small" :type="getNodeTypeTag(step.type)" class="step-type-tag">
                      {{ getNodeTypeLabel(step.type) }}
                    </el-tag>
                    <span class="step-name">{{ step.name }}</span>
                  </div>
                  <div class="step-actions">
                    <el-button-group>
                      <el-button size="small" circle @click="editBranchStep('false', step)"
                        title="编辑步骤">
                        <el-icon><edit /></el-icon>
                      </el-button>
                      <el-button size="small" circle @click="moveBranchStepUp('false', index)"
                        :disabled="index === 0" title="上移">
                        <el-icon><top /></el-icon>
                      </el-button>
                      <el-button size="small" circle @click="moveBranchStepDown('false', index)"
                        :disabled="index === editingNode.config.false_branch.length - 1" title="下移">
                        <el-icon><bottom /></el-icon>
                      </el-button>
                      <el-button size="small" type="danger" circle @click="removeBranchStep('false', index)"
                        title="删除步骤">
                        <el-icon><delete /></el-icon>
                      </el-button>
                    </el-button-group>
                  </div>
                </div>
              </div>
            </div>
          </el-tab-pane>
        </el-tabs>
      </template>
      
      <!-- 循环步骤配置 -->
      <template v-else-if="editingNode.type === 'loop'">
        <el-form-item label="循环条件" prop="config.condition">
          <codemirror
            v-model="editingNode.config.condition" 
            placeholder="e.g. i < context.items.length" 
            :style="{ height: '100px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
            :extensions="cmJsExtensions"
            class="expression-editor-cm"
          />
          <div class="help-text">
            输入一个JavaScript条件表达式，可以引用工作流变量和索引变量i，如：i < context.items.length
          </div>
        </el-form-item>
        
        <el-form-item label="最大迭代次数" prop="config.max_iterations">
          <el-input-number 
            v-model="editingNode.config.max_iterations" 
            :min="1" 
            :max="1000"
            class="full-width"
          />
          <div class="help-text">
            防止无限循环的安全措施，超过此次数将自动终止循环
          </div>
        </el-form-item>
        
        <el-divider>循环体配置</el-divider>
        
        <div class="branch-editor">
          <el-alert type="info" :closable="false" show-icon style="margin-bottom: 10px;">
            编辑内嵌步骤的详细配置（如A2A参数、条件表达式等），请关闭此对话框并在主流程编辑器中操作。此处的"编辑"仅可修改步骤名称。
          </el-alert>
          <div class="branch-actions">
            <el-button type="warning" size="small" @click="addLoopStep">
              <el-icon><plus /></el-icon> 添加循环体步骤
            </el-button>
          </div>
          
          <el-empty v-if="!editingNode.config.body || editingNode.config.body.length === 0"
            description="循环体内尚无步骤">
          </el-empty>
          
          <div v-else class="branch-steps">
            <div v-for="(step, index) in editingNode.config.body" :key="step.id" class="branch-step">
              <div class="step-info">
                <el-tag size="small" :type="getNodeTypeTag(step.type)" class="step-type-tag">
                  {{ getNodeTypeLabel(step.type) }}
                </el-tag>
                <span class="step-name">{{ step.name }}</span>
              </div>
              <div class="step-actions">
                <el-button-group>
                  <el-button size="small" circle @click="editLoopStep(step)"
                    title="编辑步骤">
                    <el-icon><edit /></el-icon>
                  </el-button>
                  <el-button size="small" circle @click="moveLoopStepUp(index)"
                    :disabled="index === 0" title="上移">
                    <el-icon><top /></el-icon>
                  </el-button>
                  <el-button size="small" circle @click="moveLoopStepDown(index)"
                    :disabled="index === editingNode.config.body.length - 1" title="下移">
                    <el-icon><bottom /></el-icon>
                  </el-button>
                  <el-button size="small" type="danger" circle @click="removeLoopStep(index)"
                    title="删除步骤">
                    <el-icon><delete /></el-icon>
                  </el-button>
                </el-button-group>
              </div>
            </div>
          </div>
        </div>
      </template>
      
      <!-- 转换步骤配置 -->
      <template v-else-if="editingNode.type === 'transform'">
        <el-form-item label="转换表达式" prop="config.expression">
          <codemirror
            v-model="editingNode.config.expression" 
            placeholder="e.g. context.value * 2" 
            :style="{ height: '150px', border: '1px solid #dcdfe6', borderRadius: '4px' }"
            :extensions="cmJsExtensions"
            class="expression-editor-cm"
          />
          <div class="help-text">
            输入一个JavaScript表达式，可以引用工作流变量，如：context.temperature * 1.8 + 32
          </div>
        </el-form-item>
        
        <el-form-item label="输出变量名" prop="config.output_var">
          <el-input 
            v-model="editingNode.config.output_var" 
            placeholder="e.g. result" 
          />
          <div class="help-text">
            表达式的结果将被存储到此工作流变量中
          </div>
        </el-form-item>
      </template>
    </el-form>
    
    <!-- 子步骤编辑组件 -->
    <workflow-node-edit
      v-if="isEditingSubNode"
      v-model:visible="isEditingSubNode"
      :node="subNodeToEdit"
      @save="handleSubNodeSave"
      :isSubNode="true"
      :parentType="editingNode.type"
    />
    
    <template #footer>
      <span class="dialog-footer">
        <!-- 在编辑子步骤时隐藏主对话框的保存/取消按钮 -->
        <template v-if="!isEditingSubNode">
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" @click="saveNode">保存</el-button>
        </template>
      </span>
    </template>
  </el-dialog>
</template>

<script>
import { ref, reactive, computed, watch, nextTick, shallowRef } from 'vue'
import { useAgentStore } from '@/store/agent'
import { Delete, Plus, Edit, Top, Bottom, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { v4 as uuidv4 } from 'uuid'

// Codemirror imports
import { Codemirror } from 'vue-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'

export default {
  name: 'WorkflowNodeEdit',
  components: {
    Delete,
    Plus,
    Edit,
    Top,
    Bottom,
    Refresh,
    Codemirror
  },
  props: {
    visible: {
      type: Boolean,
      default: false
    },
    node: {
      type: Object,
      default: null
    }
  },
  emits: ['update:visible', 'save'],
  setup(props, { emit }) {
    const agentStore = useAgentStore()
    const nodeForm = ref(null)
    
    // Codemirror setup
    const cmJsExtensions = shallowRef([javascript(), oneDark, EditorView.lineWrapping]);
    // Extensions for single-line or small JS expressions (e.g., no line wrapping by default unless content is long)
    const cmInlineJsExtensions = shallowRef([
      javascript(), 
      oneDark, 
      EditorView.lineWrapping // Keep line wrapping for consistency, height will constrain it
    ]);
    
    // 表单数据
    const editingNode = reactive({
      id: '',
      type: '',
      name: '',
      config: {}
    })
    
    // 映射数据
    const inputKeys = ref([])
    const inputValues = ref([])
    const outputKeys = ref([])
    const outputValues = ref([])
    
    // 子步骤编辑相关状态
    const isEditingSubNode = ref(false);
    const subNodeToEdit = ref(null);
    const currentSubNodeBranchOrBody = ref(null);
    const currentSubNodeIndex = ref(null);
    
    // 计算属性
    const dialogVisible = computed({
      get: () => props.visible,
      set: (value) => emit('update:visible', value)
    })
    
    const getTitle = computed(() => {
      const nodeType = editingNode.type
      const typeNames = {
        'a2a_client': 'A2A步骤',
        'condition': '条件步骤',
        'loop': '循环步骤',
        'transform': '转换步骤'
      }
      const typeName = typeNames[nodeType] || '步骤'
      
      // 如果正在编辑子步骤，显示更详细的标题
      if (props.node && props.node.isSubNode) { // 假设我们通过 props 传递一个标记来指示是否为子节点
         const parentType = typeNames[props.node.parentType] || '步骤';
         return `编辑${typeName} (${parentType}) - ${editingNode.name}`;
      }

      return `编辑${typeName} - ${editingNode.name}`
    })
    
    const agents = computed(() => agentStore.agents)
    
    const inputMappings = computed(() => {
      return inputKeys.value.map((key, index) => {
        return { key, value: inputValues.value[index] }
      })
    })
    
    const outputMappings = computed(() => {
      return outputKeys.value.map((key, index) => {
        return { key, value: outputValues.value[index] }
      })
    })
    
    // 获取节点类型标签
    const getNodeTypeLabel = (type) => {
      const types = {
        'a2a_client': 'A2A',
        'condition': '条件',
        'loop': '循环',
        'transform': '转换'
      }
      return types[type] || '未知'
    }
    
    // 获取节点类型样式
    const getNodeTypeTag = (type) => {
      const types = {
        'a2a_client': 'success',
        'condition': 'primary',
        'loop': 'warning',
        'transform': 'danger'
      }
      return types[type] || 'info'
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
    
    // 表单验证规则
    const formRules = {
      name: [
        { required: true, message: '请输入步骤名称', trigger: 'blur' }
      ],
      'config.agent_id': [
        { required: true, message: '请选择智能体', trigger: 'change' }
      ],
      'config.condition': [
        { required: true, message: '请输入条件表达式', trigger: 'blur' }
      ],
      'config.expression': [
        { required: true, message: '请输入转换表达式', trigger: 'blur' }
      ],
      'config.output_var': [
        { required: true, message: '请输入输出变量名', trigger: 'blur' }
      ]
    }
    
    // 方法
    const initNode = () => {
      if (!props.node) return
      
      // 复制节点数据
      editingNode.id = props.node.id
      editingNode.type = props.node.type
      editingNode.name = props.node.name
      
      // 深拷贝配置对象
      editingNode.config = JSON.parse(JSON.stringify(props.node.config || {}))
      
      // 初始化映射数组
      if (editingNode.type === 'a2a_client') {
        // 确保配置对象具有必要的属性
        if (!editingNode.config.input_mapping) {
          editingNode.config.input_mapping = {}
        }
        if (!editingNode.config.output_mapping) {
          editingNode.config.output_mapping = {}
        }
        if (!editingNode.config.timeout) {
          editingNode.config.timeout = 300
        }
        
        // 输入映射
        inputKeys.value = []
        inputValues.value = []
        for (const [key, value] of Object.entries(editingNode.config.input_mapping)) {
          inputKeys.value.push(key)
          inputValues.value.push(value)
        }
        
        // 输出映射
        outputKeys.value = []
        outputValues.value = []
        for (const [key, value] of Object.entries(editingNode.config.output_mapping)) {
          outputKeys.value.push(key)
          outputValues.value.push(value)
        }
      }
    }
    
    // 添加输入映射
    const addInputMapping = () => {
      inputKeys.value.push('')
      inputValues.value.push('')
    }
    
    // 删除输入映射
    const removeInputMapping = (index) => {
      inputKeys.value.splice(index, 1)
      inputValues.value.splice(index, 1)
    }
    
    // 添加输出映射
    const addOutputMapping = () => {
      outputKeys.value.push('')
      outputValues.value.push('')
    }
    
    // 删除输出映射
    const removeOutputMapping = (index) => {
      outputKeys.value.splice(index, 1)
      outputValues.value.splice(index, 1)
    }
    
    // 更新映射对象
    const updateMappings = () => {
      if (editingNode.type !== 'a2a_client') return
      
      // 更新输入映射
      const inputMapping = {}
      for (let i = 0; i < inputKeys.value.length; i++) {
        const key = inputKeys.value[i]
        const value = inputValues.value[i]
        if (key && value) {
          inputMapping[key] = value
        }
      }
      editingNode.config.input_mapping = inputMapping
      
      // 更新输出映射
      const outputMapping = {}
      for (let i = 0; i < outputKeys.value.length; i++) {
        const key = outputKeys.value[i]
        const value = outputValues.value[i]
        if (key && value) {
          outputMapping[key] = value
        }
      }
      editingNode.config.output_mapping = outputMapping
    }
    
    // 保存节点
    const saveNode = async () => {
      if (!nodeForm.value) return
      
      try {
        await nodeForm.value.validate()
        
        // 更新映射对象
        updateMappings()
        
        // 创建要保存的节点对象
        const nodeToSave = {
          id: editingNode.id,
          type: editingNode.type,
          name: editingNode.name,
          config: { ...editingNode.config }
        }
        
        emit('save', nodeToSave)
      } catch (error) {
        console.error('表单验证失败', error)
      }
    }
    
    // 关闭对话框时重置表单
    const handleClose = () => {
      if (nodeForm.value) {
        nodeForm.value.resetFields()
      }
    }
    
    // 监听node prop变化
    watch(() => props.node, () => {
      if (props.node) {
        nextTick(() => {
          initNode()
        })
      }
    }, { immediate: true })
    
    // 监听visible prop变化
    watch(() => props.visible, async (visible) => {
      if (visible) {
        // 确保agents数据已加载
        if (agents.value.length === 0) {
          await agentStore.fetchAgents()
        }
      }
    })
    
    // 子步骤编辑相关方法
    const startEditSubNode = (branchOrBody, step, index) => {
      // Deep copy the sub-node to avoid direct modification
      subNodeToEdit.value = JSON.parse(JSON.stringify(step));
      currentSubNodeBranchOrBody.value = branchOrBody;
      currentSubNodeIndex.value = index;
      isEditingSubNode.value = true; // This will hide the main dialog
    };

    const handleSubNodeSave = (savedSubNode) => {
      if (currentSubNodeBranchOrBody.value && currentSubNodeIndex.value !== null) {
        // Update the corresponding sub-node in the parent node's config
        const branchOrBodyArray = editingNode.config[currentSubNodeBranchOrBody.value];
        if (branchOrBodyArray && branchOrBodyArray[currentSubNodeIndex.value]) {
          branchOrBodyArray[currentSubNodeIndex.value] = savedSubNode;
        }
      }
      // Close the sub-node editor and reset state
      handleSubNodeClose();
    };

    const handleSubNodeClose = () => {
      isEditingSubNode.value = false;
      subNodeToEdit.value = null;
      currentSubNodeBranchOrBody.value = null;
      currentSubNodeIndex.value = null;
    };
    
    // 对分支步骤的操作
    // 添加分支步骤
    const addBranchStep = (branch) => {
      ElMessageBox.prompt('请选择步骤类型', '添加步骤', {
        confirmButtonText: '确定',
        cancelButtonText: '取消',
        inputType: 'select',
        inputPlaceholder: '请选择步骤类型',
        inputValue: 'a2a_client',
        inputValidator: (value) => !!value,
        inputErrorMessage: '请选择步骤类型',
        inputOptions: [
          { label: 'A2A步骤', value: 'a2a_client' },
          { label: '条件步骤', value: 'condition' },
          { label: '循环步骤', value: 'loop' },
          { label: '转换步骤', value: 'transform' }
        ]
      }).then(({ value: stepType }) => {
        const newStep = {
          id: uuidv4(),
          type: stepType,
          name: getDefaultNodeName(stepType),
          config: getDefaultNodeConfig(stepType)
        }
        
        if (!editingNode.config[`${branch}_branch`]) {
          editingNode.config[`${branch}_branch`] = []
        }
        
        editingNode.config[`${branch}_branch`].push(newStep)
        editBranchStep(branch, newStep)
      }).catch(() => {})
    }
    
    // 编辑分支步骤
    const editBranchStep = (branch, step) => {
      // 调用新的方法开始编辑子步骤
      const index = editingNode.config[`${branch}_branch`].findIndex(s => s.id === step.id);
        if (index !== -1) {
        startEditSubNode(`${branch}_branch`, step, index);
        }
    }
    
    // 上移分支步骤
    const moveBranchStepUp = (branch, index) => {
      if (index <= 0) return
      
      const steps = editingNode.config[`${branch}_branch`]
      const temp = steps[index]
      steps[index] = steps[index - 1]
      steps[index - 1] = temp
    }
    
    // 下移分支步骤
    const moveBranchStepDown = (branch, index) => {
      const steps = editingNode.config[`${branch}_branch`]
      if (index >= steps.length - 1) return
      
      const temp = steps[index]
      steps[index] = steps[index + 1]
      steps[index + 1] = temp
    }
    
    // 删除分支步骤
    const removeBranchStep = (branch, index) => {
      ElMessageBox.confirm('确定要删除此步骤吗？此操作不可撤销。', '删除步骤', {
        confirmButtonText: '确定',
        cancelButtonText: '取消',
        type: 'warning'
      }).then(() => {
        editingNode.config[`${branch}_branch`].splice(index, 1)
      }).catch(() => {})
    }
    
    // 对循环步骤的操作
    // 添加循环步骤
    const addLoopStep = () => {
      ElMessageBox.prompt('请选择步骤类型', '添加步骤', {
        confirmButtonText: '确定',
        cancelButtonText: '取消',
        inputType: 'select',
        inputPlaceholder: '请选择步骤类型',
        inputValue: 'a2a_client',
        inputValidator: (value) => !!value,
        inputErrorMessage: '请选择步骤类型',
        inputOptions: [
          { label: 'A2A步骤', value: 'a2a_client' },
          { label: '条件步骤', value: 'condition' },
          { label: '循环步骤', value: 'loop' },
          { label: '转换步骤', value: 'transform' }
        ]
      }).then(({ value: stepType }) => {
        const newStep = {
          id: uuidv4(),
          type: stepType,
          name: getDefaultNodeName(stepType),
          config: getDefaultNodeConfig(stepType)
        }
        
        if (!editingNode.config.body) {
          editingNode.config.body = []
        }
        
        editingNode.config.body.push(newStep)
        editLoopStep(newStep)
      }).catch(() => {})
    }
    
    // 编辑循环步骤
    const editLoopStep = (step) => {
      // 调用新的方法开始编辑子步骤
      const index = editingNode.config.body.findIndex(s => s.id === step.id);
        if (index !== -1) {
        startEditSubNode('body', step, index);
        }
    }
    
    // 上移循环步骤
    const moveLoopStepUp = (index) => {
      if (index <= 0) return
      
      const steps = editingNode.config.body
      const temp = steps[index]
      steps[index] = steps[index - 1]
      steps[index - 1] = temp
    }
    
    // 下移循环步骤
    const moveLoopStepDown = (index) => {
      const steps = editingNode.config.body
      if (index >= steps.length - 1) return
      
      const temp = steps[index]
      steps[index] = steps[index + 1]
      steps[index + 1] = temp
    }
    
    // 删除循环步骤
    const removeLoopStep = (index) => {
      ElMessageBox.confirm('确定要删除此步骤吗？此操作不可撤销。', '删除步骤', {
        confirmButtonText: '确定',
        cancelButtonText: '取消',
        type: 'warning'
      }).then(() => {
        editingNode.config.body.splice(index, 1)
      }).catch(() => {})
    }
    
    return {
      dialogVisible,
      editingNode,
      nodeForm,
      formRules,
      inputKeys,
      inputValues,
      outputKeys,
      outputValues,
      inputMappings,
      outputMappings,
      agents,
      getTitle,
      getNodeTypeLabel,
      getNodeTypeTag,
      saveNode,
      handleClose,
      addInputMapping,
      removeInputMapping,
      addOutputMapping,
      removeOutputMapping,
      updateMappings,
      // 分支和循环步骤操作
      addBranchStep,
      editBranchStep,
      moveBranchStepUp,
      moveBranchStepDown,
      removeBranchStep,
      addLoopStep,
      editLoopStep,
      moveLoopStepUp,
      moveLoopStepDown,
      removeLoopStep,
      // Codemirror related
      cmJsExtensions,
      cmInlineJsExtensions,
      // 子步骤编辑相关方法
      startEditSubNode,
      handleSubNodeSave,
      handleSubNodeClose
    }
  }
}
</script>

<style scoped>
.full-width {
  width: 100%;
}

.help-text {
  font-size: 0.85em;
  color: #909399;
  margin-top: 4px;
  line-height: 1.4;
}

.mapping-container {
  margin-bottom: 20px;
}

.mapping-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}

.mapping-key,
.mapping-value {
  flex: 1;
}

.mapping-equals {
  font-weight: bold;
}

.branch-editor {
  padding: 10px;
  background-color: #f9fafc;
  border-radius: 4px;
  margin-top: 10px;
}

.branch-actions {
  margin-bottom: 10px;
}

.branch-steps {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.branch-step {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px;
  border: 1px solid #eee;
  border-radius: 4px;
  background-color: #fff;
}

.step-info {
  display: flex;
  align-items: center;
  gap: 8px;
}

.step-name {
  font-size: 0.9em;
}

.expression-editor-cm :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
}

.mapping-value-cm :deep(.cm-editor) {
  outline: none;
  border-radius: 4px;
  /* height: 32px; */ /* Height is controlled by inline style on component for flexibility */
}

.mapping-value-cm :deep(.cm-content) {
  padding-top: 4px; /* Adjust padding to align text better in small height */
  padding-bottom: 4px;
}

.mapping-value-cm :deep(.cm-gutters) {
  display: none; /* Hide line numbers for inline-like editors */
}
</style> 