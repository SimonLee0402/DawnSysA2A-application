<template>
  <div class="workflow-debug-container">
    <div class="page-header">
      <h1>工作流调试 - {{ workflow?.name || '加载中...' }}</h1>
      <div class="header-actions">
        <el-button @click="$router.push(`/workflow/${workflowId}`)">
          <el-icon><back /></el-icon> 返回工作流
        </el-button>
        <el-button type="primary" @click="runWorkflow" :loading="isRunning">
          <el-icon><video-play /></el-icon> 运行
        </el-button>
        <el-button type="warning" @click="stopDebug" :disabled="!isRunning">
          <el-icon><video-pause /></el-icon> 停止
        </el-button>
        <el-button type="success" @click="stepExecution" :disabled="!isRunning || isStepRunning">
          <el-icon><right /></el-icon> 下一步
        </el-button>
      </div>
    </div>

    <el-row :gutter="20">
      <el-col :span="16">
        <!-- 左侧主区域 -->
        <el-card class="debug-main-panel" v-loading="isLoading">
          <template #header>
            <div class="panel-header">
              <h3>工作流步骤</h3>
              <el-tag v-if="debugStatus" :type="getDebugStatusType">{{ getDebugStatusText }}</el-tag>
            </div>
          </template>

          <div class="debug-steps">
            <div v-if="!workflow || !workflow.steps || workflow.steps.length === 0" class="no-steps">
              <el-empty description="此工作流没有定义步骤" />
            </div>
            <div v-else class="step-list">
              <div 
                v-for="(step, index) in workflow.steps" 
                :key="step.id"
                class="debug-step"
                :class="{
                  'active': currentStepIndex === index,
                  'completed': debugStepStatus[index] === 'completed',
                  'error': debugStepStatus[index] === 'error'
                }"
                @click="selectStep(index)"
              >
                <div class="step-header">
                  <div class="step-indicator">
                    <el-icon v-if="debugStepStatus[index] === 'completed'"><check /></el-icon>
                    <el-icon v-else-if="debugStepStatus[index] === 'error'"><close /></el-icon>
                    <el-icon v-else-if="currentStepIndex === index && isStepRunning"><loading /></el-icon>
                    <span v-else>{{ index + 1 }}</span>
                  </div>
                  <div class="step-title">{{ step.name }}</div>
                  <el-tag size="small" :type="getStepTypeTag(step.type)">{{ getStepTypeLabel(step.type) }}</el-tag>
                </div>
                <div class="step-body" v-if="currentStepIndex === index">
                  <div class="step-details">
                    <p v-if="step.type === 'a2a_client'">
                      <strong>智能体:</strong> {{ getAgentName(step.config?.agent_id) }}
                    </p>
                    <p v-if="step.type === 'condition'">
                      <strong>条件:</strong> <code>{{ step.config?.condition }}</code>
                    </p>
                    <p v-if="step.type === 'loop'">
                      <strong>循环条件:</strong> <code>{{ step.config?.condition }}</code>
                      <strong>当前迭代:</strong> {{ loopIterations[index] || 0 }}
                    </p>
                    <p v-if="step.type === 'transform'">
                      <strong>表达式:</strong> <code>{{ step.config?.expression }}</code>
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </el-card>

        <!-- 步骤输出 -->
        <el-card class="step-output-panel" v-if="currentStepIndex !== null">
          <template #header>
            <div class="panel-header">
              <h3>步骤输出</h3>
            </div>
          </template>

          <div class="output-content">
            <div v-if="!stepOutputs[currentStepIndex]" class="no-output">
              <el-empty description="暂无输出" />
            </div>
            <div v-else class="output-data">
              <pre>{{ formatOutput(stepOutputs[currentStepIndex]) }}</pre>
            </div>
          </div>
        </el-card>
      </el-col>

      <el-col :span="8">
        <!-- 右侧面板 - 上下文变量 -->
        <el-card class="debug-context-panel">
          <template #header>
            <div class="panel-header">
              <h3>上下文变量</h3>
              <el-button type="primary" link @click="refreshContext">
                <el-icon><refresh /></el-icon>
              </el-button>
            </div>
          </template>

          <div class="context-variables">
            <el-input
              v-model="contextSearch"
              placeholder="搜索变量"
              prefix-icon="Search"
              clearable
              class="context-search"
            />

            <el-empty v-if="Object.keys(filteredContext).length === 0" description="暂无变量" />

            <el-collapse v-else>
              <el-collapse-item 
                v-for="(section, sectionName) in categorizedContext" 
                :key="sectionName" 
                :title="sectionName"
              >
                <div class="variable-table">
                  <div v-for="(value, key) in section" :key="key" class="variable-row">
                    <div class="variable-name">{{ key }}</div>
                    <div class="variable-value">
                      <el-popover
                        v-if="typeof value === 'object' && value !== null"
                        placement="right"
                        :width="400"
                        trigger="hover"
                      >
                        <template #reference>
                          <span class="object-value">{{ getObjectSummary(value) }}</span>
                        </template>
                        <div class="json-content">
                          <pre>{{ JSON.stringify(value, null, 2) }}</pre>
                        </div>
                      </el-popover>
                      <span v-else>{{ value }}</span>
                    </div>
                  </div>
                </div>
              </el-collapse-item>
            </el-collapse>
          </div>
        </el-card>

        <!-- 右侧面板 - 断点设置 -->
        <el-card class="debug-breakpoints-panel">
          <template #header>
            <div class="panel-header">
              <h3>断点设置</h3>
            </div>
          </template>

          <div class="breakpoints-settings">
            <el-form label-position="top">
              <el-form-item label="自动步进">
                <el-switch v-model="autoStep" />
                <span class="setting-description">自动执行下一个步骤</span>
              </el-form-item>

              <el-form-item label="步骤间延迟 (ms)" v-if="autoStep">
                <el-slider v-model="stepDelay" :min="0" :max="2000" :step="100" show-input />
              </el-form-item>

              <el-divider>断点条件</el-divider>

              <el-form-item label="变量断点">
                <el-input v-model="breakpointCondition" placeholder="例如: context.value > 10" />
                <span class="setting-description">当条件满足时暂停执行</span>
              </el-form-item>
            </el-form>
          </div>
        </el-card>
      </el-col>
    </el-row>

    <!-- 输入参数对话框 -->
    <el-dialog
      v-model="inputDialogVisible"
      title="输入参数"
      width="50%"
    >
      <el-form ref="inputForm" :model="inputParams" label-position="top">
        <el-form-item 
          v-for="(param, index) in workflowParams" 
          :key="index"
          :label="param.name || `参数 ${index + 1}`"
        >
          <div class="param-description" v-if="param.description">
            {{ param.description }}
          </div>
          <el-input 
            v-if="param.type === 'string'" 
            v-model="inputParams[param.key]" 
            :placeholder="param.placeholder || '请输入参数值'"
          />
          <el-input-number 
            v-else-if="param.type === 'number'" 
            v-model="inputParams[param.key]" 
            :placeholder="param.placeholder || '请输入数值'"
            class="full-width"
          />
          <el-switch 
            v-else-if="param.type === 'boolean'" 
            v-model="inputParams[param.key]" 
            :active-text="param.trueLabel || '是'"
            :inactive-text="param.falseLabel || '否'"
          />
          <el-input 
            v-else
            v-model="inputParams[param.key]" 
            :placeholder="param.placeholder || '请输入参数值'"
            type="textarea"
            :rows="3"
          />
        </el-form-item>
      </el-form>
      
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="inputDialogVisible = false">取消</el-button>
          <el-button type="primary" @click="startDebugSession">开始调试</el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script>
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useWorkflowStore } from '@/store/workflow'
import { useAgentStore } from '@/store/agent'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Back,
  Check, 
  Close,
  Loading,
  Right,
  VideoPlay,
  VideoPause,
  Refresh,
  Search
} from '@element-plus/icons-vue'

export default {
  name: 'WorkflowDebug',
  components: {
    Back,
    Check,
    Close,
    Loading,
    Right,
    VideoPlay,
    VideoPause,
    Refresh,
    Search
  },
  setup() {
    const router = useRouter()
    const route = useRoute()
    const workflowStore = useWorkflowStore()
    const agentStore = useAgentStore()
    
    // 基本状态
    const workflowId = ref(route.params.id)
    const workflow = ref(null)
    const isLoading = ref(true)
    const debugStatus = ref('')  // 'ready', 'running', 'paused', 'completed', 'error'
    const currentStepIndex = ref(null)
    const debugStepStatus = ref({})  // {0: 'waiting', 1: 'completed', 2: 'error', ...}
    const stepOutputs = ref({})  // {0: {...}, 1: {...}, ...}
    const debugContext = ref({})  // 工作流上下文变量
    const loopIterations = ref({})  // 循环步骤的迭代计数 {2: 3} 表示第3个步骤当前是第4次迭代
    
    // 调试设置
    const isRunning = ref(false)
    const isStepRunning = ref(false)
    const autoStep = ref(false)
    const stepDelay = ref(500)
    const breakpointCondition = ref('')
    const contextSearch = ref('')
    
    // 参数输入
    const inputDialogVisible = ref(false)
    const inputParams = ref({})
    
    // 定时器
    const autoStepTimer = ref(null)
    
    // 计算属性
    const workflowParams = computed(() => {
      if (!workflow.value || !workflow.value.definition) return []
      
      try {
        let definition = workflow.value.definition
        if (typeof definition === 'string') {
          definition = JSON.parse(definition)
        }
        
        return definition.parameters || []
      } catch (err) {
        console.error('解析工作流参数失败', err)
        return []
      }
    })
    
    const getDebugStatusType = computed(() => {
      const statusMap = {
        'ready': 'info',
        'running': 'primary',
        'paused': 'warning',
        'completed': 'success',
        'error': 'danger'
      }
      return statusMap[debugStatus.value] || 'info'
    })
    
    const getDebugStatusText = computed(() => {
      const statusMap = {
        'ready': '准备调试',
        'running': '正在运行',
        'paused': '已暂停',
        'completed': '已完成',
        'error': '出错'
      }
      return statusMap[debugStatus.value] || '未知状态'
    })
    
    // 过滤和分类上下文变量
    const filteredContext = computed(() => {
      if (!debugContext.value) return {}
      
      if (!contextSearch.value) {
        return debugContext.value
      }
      
      const search = contextSearch.value.toLowerCase()
      const result = {}
      
      Object.entries(debugContext.value).forEach(([key, value]) => {
        if (key.toLowerCase().includes(search)) {
          result[key] = value
        } else if (typeof value === 'string' && value.toLowerCase().includes(search)) {
          result[key] = value
        } else if (typeof value === 'object' && value !== null) {
          // 尝试在对象内部搜索
          const jsonString = JSON.stringify(value).toLowerCase()
          if (jsonString.includes(search)) {
            result[key] = value
          }
        }
      })
      
      return result
    })
    
    const categorizedContext = computed(() => {
      const result = {
        '输入参数': {},
        '步骤输出': {},
        '系统变量': {},
        '其他': {}
      }
      
      Object.entries(filteredContext.value).forEach(([key, value]) => {
        if (key.startsWith('input_')) {
          result['输入参数'][key] = value
        } else if (key.startsWith('output_')) {
          result['步骤输出'][key] = value
        } else if (['i', 'loop_count', 'current_step'].includes(key)) {
          result['系统变量'][key] = value
        } else {
          result['其他'][key] = value
        }
      })
      
      // 移除空分类
      Object.keys(result).forEach(key => {
        if (Object.keys(result[key]).length === 0) {
          delete result[key]
        }
      })
      
      return result
    })
    
    // 方法
    // 初始化
    const fetchWorkflow = async () => {
      isLoading.value = true
      try {
        await workflowStore.fetchWorkflow(workflowId.value)
        workflow.value = workflowStore.currentWorkflow
        
        // 确保工作流定义是对象
        if (workflow.value && workflow.value.definition && typeof workflow.value.definition === 'string') {
          try {
            workflow.value.definition = JSON.parse(workflow.value.definition)
          } catch (e) {
            console.error('工作流定义解析失败', e)
            workflow.value.definition = { steps: [] }
          }
        }
        
        // 初始化步骤状态
        initStepStatus()
        
        debugStatus.value = 'ready'
      } catch (error) {
        ElMessage.error('获取工作流失败')
        console.error(error)
      } finally {
        isLoading.value = false
      }
    }
    
    const fetchAgents = async () => {
      try {
        await agentStore.fetchAgents()
      } catch (error) {
        console.error('获取智能体失败', error)
      }
    }
    
    const initStepStatus = () => {
      if (!workflow.value || !workflow.value.definition || !workflow.value.definition.steps) {
        return
      }
      
      const steps = workflow.value.definition.steps
      debugStepStatus.value = {}
      
      steps.forEach((_, index) => {
        debugStepStatus.value[index] = 'waiting'
      })
      
      stepOutputs.value = {}
      debugContext.value = {}
      loopIterations.value = {}
      currentStepIndex.value = steps.length > 0 ? 0 : null
    }
    
    // 调试控制
    const runWorkflow = () => {
      // 如果已经在运行，则暂停
      if (isRunning.value) {
        pauseDebug()
        return
      }
      
      // 初始化调试状态
      initStepStatus()
      
      // 如果工作流有参数，显示参数输入对话框
      if (workflowParams.value.length > 0) {
        // 初始化参数默认值
        inputParams.value = {}
        workflowParams.value.forEach(param => {
          if (param.key) {
            inputParams.value[param.key] = param.defaultValue !== undefined 
              ? param.defaultValue 
              : getDefaultValueForType(param.type)
          }
        })
        
        inputDialogVisible.value = true
      } else {
        // 没有参数，直接开始调试
        startDebugSession()
      }
    }
    
    const getDefaultValueForType = (type) => {
      switch (type) {
        case 'number': return 0
        case 'boolean': return false
        case 'object': return {}
        default: return ''
      }
    }
    
    const startDebugSession = () => {
      inputDialogVisible.value = false
      
      // 初始化调试上下文
      debugContext.value = {
        ...inputParams.value,
        // 添加其他系统变量
        current_step: 0
      }
      
      // 开始调试
      isRunning.value = true
      debugStatus.value = 'running'
      currentStepIndex.value = 0
      
      // 如果开启了自动步进，启动定时器
      if (autoStep.value) {
        scheduleNextStep()
      }
      
      ElMessage.success('调试会话已开始')
    }
    
    const pauseDebug = () => {
      if (!isRunning.value) return
      
      clearAutoStepTimer()
      isRunning.value = false
      debugStatus.value = 'paused'
      ElMessage.info('调试已暂停')
    }
    
    const stopDebug = () => {
      clearAutoStepTimer()
      isRunning.value = false
      isStepRunning.value = false
      debugStatus.value = 'ready'
      initStepStatus()
      ElMessage.info('调试已停止')
    }
    
    const scheduleNextStep = () => {
      clearAutoStepTimer()
      
      if (autoStep.value && isRunning.value) {
        autoStepTimer.value = setTimeout(() => {
          stepExecution()
        }, stepDelay.value)
      }
    }
    
    const clearAutoStepTimer = () => {
      if (autoStepTimer.value) {
        clearTimeout(autoStepTimer.value)
        autoStepTimer.value = null
      }
    }
    
    // 执行单个步骤
    const stepExecution = async () => {
      if (!isRunning.value || isStepRunning.value || currentStepIndex.value === null) return
      
      const steps = workflow.value.definition.steps
      const currentStep = steps[currentStepIndex.value]
      
      if (!currentStep) {
        finishDebug()
        return
      }
      
      isStepRunning.value = true
      
      try {
        // 执行步骤
        const result = await executeStep(currentStep, currentStepIndex.value)
        
        // 更新步骤状态
        debugStepStatus.value[currentStepIndex.value] = 'completed'
        
        // 存储步骤输出
        stepOutputs.value[currentStepIndex.value] = result
        
        // 更新上下文
        updateContext(result, currentStepIndex.value)
        
        // 检查是否应该暂停（断点条件）
        if (checkBreakpoint()) {
          pauseDebug()
          ElMessage.warning('断点触发，已暂停执行')
          isStepRunning.value = false
          return
        }
        
        // 移动到下一个步骤
        if (currentStepIndex.value < steps.length - 1) {
          currentStepIndex.value += 1
          debugContext.value.current_step = currentStepIndex.value
          
          // 如果自动步进，安排下一步执行
          if (autoStep.value && isRunning.value) {
            scheduleNextStep()
          }
        } else {
          // 所有步骤执行完成
          finishDebug()
        }
      } catch (error) {
        handleStepError(error)
      } finally {
        isStepRunning.value = false
      }
    }
    
    // 模拟执行步骤
    const executeStep = async (step, stepIndex) => {
      console.log(`执行步骤 ${stepIndex + 1}: ${step.name}`)
      
      // 根据步骤类型执行不同逻辑
      switch (step.type) {
        case 'a2a_client':
          return executeA2AStep(step)
        case 'condition':
          return executeConditionStep(step, stepIndex)
        case 'loop':
          return executeLoopStep(step, stepIndex)
        case 'transform':
          return executeTransformStep(step)
        default:
          throw new Error(`不支持的步骤类型: ${step.type}`)
      }
    }
    
    // 执行A2A步骤
    const executeA2AStep = async (step) => {
      // 模拟A2A调用
      // 在实际实现中，这里会调用A2A API，但在调试模式下，我们只模拟结果
      
      // 准备输入参数
      const inputMapping = step.config?.input_mapping || {}
      const inputParams = {}
      
      for (const [key, expr] of Object.entries(inputMapping)) {
        try {
          // 简单表达式评估，在实际环境中可能需要更复杂的实现
          const value = evaluateExpression(expr, debugContext.value)
          inputParams[key] = value
        } catch (error) {
          console.error(`表达式求值错误 [${expr}]:`, error)
          throw new Error(`输入映射表达式错误: ${expr}`)
        }
      }
      
      // 模拟A2A处理
      await new Promise(resolve => setTimeout(resolve, 500))
      
      // 生成模拟输出
      const mockOutput = {
        completion: `这是智能体 ${getAgentName(step.config?.agent_id) || '未知'} 的模拟输出`,
        input: inputParams,
        metadata: {
          tokens: {
            input: Math.floor(Math.random() * 100) + 50,
            output: Math.floor(Math.random() * 200) + 100
          },
          runtime: Math.random() * 2 + 0.5
        }
      }
      
      return mockOutput
    }
    
    // 执行条件步骤
    const executeConditionStep = (step) => {
      const condition = step.config?.condition
      if (!condition) {
        throw new Error('条件步骤缺少条件表达式')
      }
      
      let result
      try {
        result = evaluateExpression(condition, debugContext.value)
      } catch (error) {
        console.error('条件表达式评估错误:', error)
        throw new Error(`条件表达式错误: ${condition}`)
      }
      
      return {
        condition: condition,
        result: !!result,
        branch: result ? 'true_branch' : 'false_branch'
      }
    }
    
    // 执行循环步骤
    const executeLoopStep = (step, stepIndex) => {
      const condition = step.config?.condition
      if (!condition) {
        throw new Error('循环步骤缺少条件表达式')
      }
      
      // 获取或初始化当前迭代计数
      if (!loopIterations.value[stepIndex]) {
        loopIterations.value[stepIndex] = 0
        debugContext.value.i = 0
        debugContext.value.loop_count = 0
      } else {
        loopIterations.value[stepIndex]++
        debugContext.value.i = loopIterations.value[stepIndex]
        debugContext.value.loop_count = loopIterations.value[stepIndex]
      }
      
      // 检查最大迭代次数
      const maxIterations = step.config?.max_iterations || 10
      if (loopIterations.value[stepIndex] >= maxIterations) {
        return {
          condition: condition,
          result: false,
          iteration: loopIterations.value[stepIndex],
          maxIterations: maxIterations,
          status: 'max_iterations_reached'
        }
      }
      
      // 评估条件
      let result
      try {
        result = evaluateExpression(condition, debugContext.value)
      } catch (error) {
        console.error('循环条件表达式评估错误:', error)
        throw new Error(`循环条件表达式错误: ${condition}`)
      }
      
      return {
        condition: condition,
        result: !!result,
        iteration: loopIterations.value[stepIndex],
        maxIterations: maxIterations,
        status: result ? 'continue' : 'exit'
      }
    }
    
    // 执行转换步骤
    const executeTransformStep = (step) => {
      const expression = step.config?.expression
      const outputVar = step.config?.output_var || 'result'
      
      if (!expression) {
        throw new Error('转换步骤缺少表达式')
      }
      
      let result
      try {
        result = evaluateExpression(expression, debugContext.value)
      } catch (error) {
        console.error('转换表达式评估错误:', error)
        throw new Error(`转换表达式错误: ${expression}`)
      }
      
      return {
        expression: expression,
        output_var: outputVar,
        result: result
      }
    }
    
    // 表达式求值 - 简化版
    const evaluateExpression = (expr, context) => {
      // 安全的表达式求值函数
      try {
        // 创建一个新的函数，将上下文变量作为参数传入
        const contextKeys = Object.keys(context)
        const contextValues = Object.values(context)
        
        // 使用 Function 构造函数创建一个函数，该函数可以访问上下文变量
        const func = new Function(...contextKeys, `
          try {
            return (${expr});
          } catch (e) {
            throw new Error('表达式评估错误: ' + e.message);
          }
        `)
        
        // 调用函数并返回结果
        return func(...contextValues)
      } catch (e) {
        console.error('表达式求值错误:', e)
        throw new Error(`表达式 "${expr}" 求值失败: ${e.message}`)
      }
    }
    
    // 更新调试上下文
    const updateContext = (result, stepIndex) => {
      const step = workflow.value.definition.steps[stepIndex]
      
      if (step.type === 'a2a_client') {
        // 处理 A2A 步骤的输出映射
        const outputMapping = step.config?.output_mapping || {}
        
        for (const [key, expr] of Object.entries(outputMapping)) {
          try {
            // 对于 A2A 输出，我们假设 expr 是一个路径表达式，如 "completion" 或 "metadata.runtime"
            let value = result
            const parts = expr.split('.')
            
            for (const part of parts) {
              if (value && typeof value === 'object') {
                value = value[part]
              } else {
                value = undefined
                break
              }
            }
            
            debugContext.value[key] = value
          } catch (error) {
            console.warn(`输出映射处理错误 [${key}=${expr}]:`, error)
          }
        }
        
        // 添加原始输出
        debugContext.value[`output_${stepIndex}`] = result
      } else if (step.type === 'transform') {
        // 处理转换步骤的输出变量
        const outputVar = step.config?.output_var || 'result'
        debugContext.value[outputVar] = result.result
        debugContext.value[`output_${stepIndex}`] = result
      } else {
        // 其他步骤类型的输出
        debugContext.value[`output_${stepIndex}`] = result
      }
    }
    
    // 处理步骤执行错误
    const handleStepError = (error) => {
      console.error('步骤执行错误:', error)
      
      if (currentStepIndex.value !== null) {
        debugStepStatus.value[currentStepIndex.value] = 'error'
        
        // 存储错误信息
        stepOutputs.value[currentStepIndex.value] = {
          error: error.message || '未知错误'
        }
      }
      
      debugStatus.value = 'error'
      isRunning.value = false
      ElMessage.error(`步骤执行错误: ${error.message || '未知错误'}`)
    }
    
    // 完成调试
    const finishDebug = () => {
      isRunning.value = false
      debugStatus.value = 'completed'
      clearAutoStepTimer()
      ElMessage.success('工作流执行完成')
    }
    
    // 选择步骤
    const selectStep = (index) => {
      if (index >= 0 && index < workflow.value.definition.steps.length) {
        currentStepIndex.value = index
      }
    }
    
    // 检查断点条件
    const checkBreakpoint = () => {
      if (!breakpointCondition.value) return false
      
      try {
        return evaluateExpression(breakpointCondition.value, debugContext.value)
      } catch (error) {
        console.warn('断点条件评估错误:', error)
        return false
      }
    }
    
    // 刷新上下文
    const refreshContext = () => {
      // 这里只是重新触发视图更新
      debugContext.value = { ...debugContext.value }
    }
    
    // 格式化输出
    const formatOutput = (output) => {
      if (!output) return ''
      
      try {
        return JSON.stringify(output, null, 2)
      } catch (e) {
        return String(output)
      }
    }
    
    // 获取对象摘要
    const getObjectSummary = (obj) => {
      if (!obj) return '{}'
      
      if (Array.isArray(obj)) {
        return `数组 [${obj.length}项]`
      }
      
      return `对象 {${Object.keys(obj).length}属性}`
    }
    
    // 获取智能体名称
    const getAgentName = (agentId) => {
      if (!agentId) return '未指定'
      
      const agent = agentStore.agents.find(a => a.id === agentId)
      return agent ? agent.name : `智能体 ${agentId}`
    }
    
    // 获取步骤类型标签
    const getStepTypeLabel = (type) => {
      const types = {
        'a2a_client': 'A2A',
        'condition': '条件',
        'loop': '循环',
        'transform': '转换'
      }
      return types[type] || '未知'
    }
    
    // 获取步骤类型样式
    const getStepTypeTag = (type) => {
      const types = {
        'a2a_client': 'success',
        'condition': 'primary',
        'loop': 'warning',
        'transform': 'danger'
      }
      return types[type] || 'info'
    }
    
    // 监听自动步进设置变化
    watch(autoStep, (newValue) => {
      if (newValue && isRunning.value && !isStepRunning.value) {
        scheduleNextStep()
      } else if (!newValue) {
        clearAutoStepTimer()
      }
    })
    
    // 生命周期钩子
    onMounted(async () => {
      await fetchWorkflow()
      await fetchAgents()
    })
    
    onBeforeUnmount(() => {
      clearAutoStepTimer()
    })
    
    return {
      workflowId,
      workflow,
      isLoading,
      isRunning,
      isStepRunning,
      debugStatus,
      currentStepIndex,
      debugStepStatus,
      stepOutputs,
      debugContext,
      autoStep,
      stepDelay,
      breakpointCondition,
      contextSearch,
      inputDialogVisible,
      inputParams,
      loopIterations,
      workflowParams,
      getDebugStatusType,
      getDebugStatusText,
      filteredContext,
      categorizedContext,
      runWorkflow,
      pauseDebug,
      stopDebug,
      stepExecution,
      selectStep,
      refreshContext,
      formatOutput,
      getObjectSummary,
      getAgentName,
      getStepTypeLabel,
      getStepTypeTag,
      startDebugSession
    }
  }
}
</script> 