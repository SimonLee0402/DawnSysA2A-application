<template>
  <div class="workflow-node" :class="`node-type-${step.type}`">
    <div class="node-header">
      <div class="node-title">
        <el-tag size="small" :type="nodeTypeDetails.tagType" class="node-type-tag">
          {{ nodeTypeDetails.label }}
        </el-tag>
        <h4 class="node-name">{{ step.name }}</h4>
      </div>
      
      <div class="node-actions">
        <el-button-group>
          <el-button 
            size="small" 
            circle 
            @click="$emit('edit', step)"
            title="编辑步骤"
          >
            <el-icon><edit /></el-icon>
          </el-button>
          
          <el-button 
            size="small" 
            circle 
            @click="$emit('move-up')"
            :disabled="index === 0"
            title="上移"
          >
            <el-icon><top /></el-icon>
          </el-button>
          
          <el-button 
            size="small" 
            circle 
            @click="$emit('move-down')"
            title="下移"
          >
            <el-icon><bottom /></el-icon>
          </el-button>
          
          <el-button 
            size="small" 
            type="danger" 
            circle 
            @click="$emit('remove', step)"
            title="删除步骤"
          >
            <el-icon><delete /></el-icon>
          </el-button>
        </el-button-group>
      </div>
    </div>
    
    <div class="node-body">
      <div v-if="step.type === 'a2a_client'" class="a2a-client-node">
        <div class="config-item">
          <label>智能体:</label>
          <span>{{ getAgentName(step.config?.agent_id) || '未设置' }}</span>
        </div>
        
        <div class="config-item">
          <label>超时时间:</label>
          <span>{{ step.config?.timeout || 300 }}秒</span>
        </div>
        
        <div class="config-item">
          <label>输入映射:</label>
          <div v-if="Object.keys(step.config?.input_mapping || {}).length > 0">
            <div v-for="(value, key) in step.config?.input_mapping" :key="key" class="mapping-item">
              {{ key }} = {{ value }}
            </div>
          </div>
          <span v-else>未设置</span>
        </div>
        
        <div class="config-item">
          <label>输出映射:</label>
          <div v-if="Object.keys(step.config?.output_mapping || {}).length > 0">
            <div v-for="(value, key) in step.config?.output_mapping" :key="key" class="mapping-item">
              {{ key }} = {{ value }}
            </div>
          </div>
          <span v-else>未设置</span>
        </div>
      </div>
      
      <div v-else-if="step.type === 'condition'" class="condition-node">
        <div class="config-item">
          <label>条件表达式:</label>
          <code>{{ step.config?.condition || '未设置' }}</code>
        </div>
        
        <div class="branches">
          <div class="branch">
            <div class="branch-header">
              <el-tag size="small" type="success">True分支</el-tag>
              <span class="branch-count">{{ step.config?.true_branch?.length || 0 }}个步骤</span>
            </div>
          </div>
          
          <div class="branch">
            <div class="branch-header">
              <el-tag size="small" type="danger">False分支</el-tag>
              <span class="branch-count">{{ step.config?.false_branch?.length || 0 }}个步骤</span>
            </div>
          </div>
        </div>
      </div>
      
      <div v-else-if="step.type === 'loop'" class="loop-node">
        <div class="config-item">
          <label>循环条件:</label>
          <code>{{ step.config?.condition || '未设置' }}</code>
        </div>
        
        <div class="config-item">
          <label>最大迭代次数:</label>
          <span>{{ step.config?.max_iterations || 10 }}</span>
        </div>
        
        <div class="config-item">
          <label>循环体:</label>
          <span>{{ step.config?.body?.length || 0 }}个步骤</span>
        </div>
      </div>
      
      <div v-else-if="step.type === 'transform'" class="transform-node">
        <div class="config-item">
          <label>表达式:</label>
          <code>{{ step.config?.expression || '未设置' }}</code>
        </div>
        
        <div class="config-item">
          <label>输出变量:</label>
          <span>{{ step.config?.output_var || 'result' }}</span>
        </div>
      </div>
      
      <div v-else class="unknown-node">
        <el-alert
          title="未知步骤类型"
          type="warning"
          :closable="false"
          show-icon
        />
      </div>
    </div>
    
    <div class="node-connection" v-if="showConnection">
      <el-icon class="connection-icon"><arrow-down /></el-icon>
    </div>
  </div>
</template>

<script>
import { computed } from 'vue'
import { useAgentStore } from '@/store/agent'
import {
  Edit,
  Delete,
  Top,
  Bottom,
  ArrowDown
} from '@element-plus/icons-vue'

export default {
  name: 'WorkflowNode',
  components: {
    Edit,
    Delete,
    Top,
    Bottom,
    ArrowDown
  },
  props: {
    step: {
      type: Object,
      required: true
    },
    index: {
      type: Number,
      required: true
    },
    isLast: {
      type: Boolean,
      default: false
    }
  },
  emits: ['edit', 'remove', 'move-up', 'move-down'],
  setup(props) {
    const agentStore = useAgentStore()
    
    const showConnection = computed(() => !props.isLast)
    
    const getAgentName = (agentId) => {
      if (!agentId) return ''
      const agent = agentStore.agents.find(a => a.id === agentId)
      return agent ? agent.name : '未知智能体'
    }
    
    // Merged node type details
    const nodeTypeDetails = computed(() => {
      const type = props.step.type;
      const details = {
        'a2a_client': { label: 'A2A', tagType: 'success', icon: 'Connection' }, // Example icon
        'condition': { label: '条件', tagType: 'primary', icon: 'Switch' },
        'loop': { label: '循环', tagType: 'warning', icon: 'RefreshRight' },
        'transform': { label: '转换', tagType: 'danger', icon: 'Refresh' }
      };
      return details[type] || { label: '未知', tagType: 'info', icon: 'QuestionFilled' };
    });
    
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
    
    return {
      showConnection,
      getAgentName,
      nodeTypeDetails,
      getNodeTypeLabel,
      getNodeTypeTag
    }
  }
}
</script>

<style scoped>
.workflow-node {
  border: 1px solid #e0e0e0; /* Slightly softer border */
  border-radius: 6px; /* Slightly more rounded */
  background-color: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08); /* Softer shadow */
  position: relative;
  padding: 0; /* Remove padding, header/body will handle it */
  transition: box-shadow 0.2s ease-in-out, transform 0.2s ease-in-out;
  overflow: hidden; /* Ensure border-left doesn't cause layout shift */
}

.workflow-node:hover {
  box-shadow: 0 3px 6px rgba(0, 0, 0, 0.12);
  transform: translateY(-2px); /* Add a slight lift */
}

/* Type-specific styling using border-left */
.node-type-a2a_client {
  border-left: 5px solid var(--el-color-success);
}
.node-type-condition {
  border-left: 5px solid var(--el-color-primary);
}
.node-type-loop {
  border-left: 5px solid var(--el-color-warning);
}
.node-type-transform {
  border-left: 5px solid var(--el-color-danger);
}
.node-type-unknown { /* Assuming you add this class for unknown types */
  border-left: 5px solid var(--el-color-info);
}

.node-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px; /* Add padding here */
  /* margin-bottom: 16px; */ /* Removed, body will have top padding */
  border-bottom: 1px solid #f0f0f0; /* Lighter separator */
  /* padding-bottom: 12px; */ /* Combined with main padding */
}

.node-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.node-name {
  margin: 0;
  font-size: 16px;
}

.node-type-tag {
  text-transform: uppercase;
  font-size: 10px;
}

.node-actions {
  display: flex;
  gap: 5px;
}

.node-body {
  padding: 16px; /* Add padding here */
  /* padding: 8px 0; */ /* Replaced */
}

.config-item {
  margin-bottom: 12px;
}

.config-item label {
  font-weight: bold;
  color: #606266;
  margin-right: 8px;
}

.config-item code {
  background-color: #f5f7fa;
  padding: 2px 4px;
  border-radius: 3px;
  font-family: monospace;
  color: #f56c6c;
}

.mapping-item {
  padding: 4px 0;
  font-family: monospace;
  font-size: 12px;
}

.branches {
  display: flex;
  gap: 20px;
  margin-top: 12px;
}

.branch {
  flex: 1;
  border: 1px dashed #ddd;
  padding: 8px;
  border-radius: 4px;
}

.branch-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.branch-count {
  font-size: 12px;
  color: #909399;
}

.node-connection {
  position: absolute;
  bottom: -20px;
  left: 50%;
  transform: translateX(-50%);
  height: 20px;
  z-index: 1;
  display: flex;
  justify-content: center;
  align-items: flex-end;
}

.connection-icon {
  color: #909399;
  font-size: 20px;
}
</style> 