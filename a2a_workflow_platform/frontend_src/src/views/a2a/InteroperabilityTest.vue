<template>
  <div class="a2a-interop-test-container">
    <h1 class="app-page-title">A2A互操作性测试</h1>
    
    <el-tabs v-model="activeTab">
      <el-tab-pane label="测试配置" name="test-config">
        <el-card class="test-form-card app-card">
          <template #header>
            <div class="card-header">
              <h2 class="app-card-title">测试配置</h2>
            </div>
          </template>
          
          <interoperability-test-config 
            :current-config="testForm" 
            @load-config="loadTestConfig"
          />
          
          <el-form :model="testForm" :rules="rules" ref="testFormRef" label-position="top">
            <el-form-item label="测试本地Agent" prop="agentId">
              <el-select v-model="testForm.agentId" placeholder="选择一个Agent" filterable>
                <el-option 
                  v-for="agent in agents" 
                  :key="agent.id" 
                  :label="agent.name" 
                  :value="agent.id" 
                >
                  <span style="float: left">{{ agent.name }}</span>
                  <span style="float: right; color: #8492a6; font-size: 0.8em">{{ agent.agent_type }}</span>
                </el-option>
              </el-select>
            </el-form-item>
            
            <el-form-item label="目标A2A系统URL" prop="targetUrl">
              <el-input 
                v-model="testForm.targetUrl" 
                placeholder="请输入目标A2A系统的URL，例如: https://example.com"
              ></el-input>
            </el-form-item>
            
            <el-form-item label="测试类型" prop="testType">
              <el-select v-model="testForm.testType" placeholder="选择测试类型">
                <el-option label="基本互操作性测试" value="basic"></el-option>
                <el-option label="流式响应测试" value="streaming"></el-option>
                <el-option label="推送通知测试" value="push_notification"></el-option>
                <el-option label="完整测试套件" value="full"></el-option>
              </el-select>
            </el-form-item>
            
            <el-divider content-position="left">高级选项</el-divider>
            
            <el-collapse>
              <el-collapse-item title="测试参数配置" name="test-parameters">
                <el-form-item label="测试任务内容" prop="taskContent">
                  <el-input 
                    type="textarea"
                    v-model="testForm.taskContent" 
                    placeholder="请输入测试任务的内容，例如：'你好，这是一个测试任务'"
                    :rows="3"
                  ></el-input>
                </el-form-item>
                
                <el-form-item label="超时设置(秒)" prop="timeout">
                  <el-input-number v-model="testForm.timeout" :min="5" :max="300"></el-input-number>
                </el-form-item>
                
                <el-form-item label="重试次数" prop="retries">
                  <el-input-number v-model="testForm.retries" :min="0" :max="5"></el-input-number>
                </el-form-item>
                
                <el-form-item label="验证SSL证书">
                  <el-switch v-model="testForm.verifySSL"></el-switch>
                </el-form-item>
              </el-collapse-item>
              
              <el-collapse-item title="测试用例选择" name="test-cases">
                <div class="test-cases-container">
                  <el-checkbox-group v-model="testForm.testCases">
                    <el-checkbox label="agent_card">Agent Card 获取测试</el-checkbox>
                    <el-checkbox label="task_send">任务发送测试</el-checkbox>
                    <el-checkbox label="task_get">任务状态获取测试</el-checkbox>
                    <el-checkbox label="task_cancel">任务取消测试</el-checkbox>
                    <el-checkbox label="task_input">任务输入测试</el-checkbox>
                    <el-checkbox label="streaming">流式响应测试</el-checkbox>
                    <el-checkbox label="schema_validation">Schema验证测试</el-checkbox>
                  </el-checkbox-group>
                </div>
              </el-collapse-item>
            </el-collapse>
            
            <el-form-item>
              <el-button 
                type="primary" 
                @click="runTest" 
                :loading="a2aStore.isLoading"
              >
                执行互操作性测试
              </el-button>
              <el-button @click="resetForm">
                重置
              </el-button>
            </el-form-item>
          </el-form>
        </el-card>
      </el-tab-pane>
      
      <el-tab-pane label="测试结果" name="test-results" :disabled="!testResults">
        <el-card class="test-results-card app-card" v-if="testResults">
          <template #header>
            <div class="card-header">
              <h2 class="app-card-title">测试结果</h2>
              <div class="header-actions">
                <el-button size="small" type="primary" @click="exportTestResults">
                  <i class="el-icon-download"></i> 导出结果
                </el-button>
              </div>
            </div>
          </template>
          
          <div class="test-summary">
            <el-alert
              :title="`测试完成：成功率 ${testResults.summary.success_rate}`"
              :type="calculateSuccessType(testResults.summary.success_rate)"
              :description="`测试时间: ${formatDateTime(testResults.summary.timestamp)} | 总测试耗时: ${testResults.summary.total_time || '未知'}`"
              show-icon
            ></el-alert>
          </div>
          
          <el-divider></el-divider>
          
          <div class="test-filter-bar">
            <el-radio-group v-model="testResultFilter" size="small">
              <el-radio-button label="all">全部</el-radio-button>
              <el-radio-button label="success">成功</el-radio-button>
              <el-radio-button label="failed">失败</el-radio-button>
            </el-radio-group>
          </div>
          
          <h3>测试详情</h3>
          
          <el-collapse accordion>
            <el-collapse-item v-for="(test, key) in filteredTestResults" :key="key"
                              :title="getTestTitle(key)" :name="key">
              <el-descriptions 
                :column="1" 
                :border="true" 
                direction="vertical"
              >
                <el-descriptions-item label="状态">
                  <el-tag :type="getStatusType(test.status)">
                    {{ test.status }}
                  </el-tag>
                </el-descriptions-item>
                <el-descriptions-item label="时间戳">
                  {{ formatDateTime(test.timestamp) }}
                </el-descriptions-item>
                <el-descriptions-item label="耗时" v-if="test.duration">
                  {{ test.duration }}毫秒
                </el-descriptions-item>
                <el-descriptions-item label="请求数据" v-if="test.request">
                  <pre class="json-content">{{ JSON.stringify(test.request, null, 2) }}</pre>
                </el-descriptions-item>
                <el-descriptions-item label="响应数据" v-if="test.response">
                  <pre class="json-content">{{ JSON.stringify(test.response, null, 2) }}</pre>
                </el-descriptions-item>
                <el-descriptions-item label="错误信息" v-if="test.error">
                  <span class="error-text">{{ test.error }}</span>
                </el-descriptions-item>
              </el-descriptions>
            </el-collapse-item>
          </el-collapse>
        </el-card>
      </el-tab-pane>
      
      <el-tab-pane label="测试日志" name="test-logs">
        <el-card class="test-logs-card app-card">
          <template #header>
            <div class="card-header">
              <h2 class="app-card-title">实时日志</h2>
            </div>
          </template>
          
          <interoperability-test-logs ref="testLogsRef" :test-id="currentTestId" @log="handleLogEvent" />
        </el-card>
      </el-tab-pane>
      
      <el-tab-pane label="测试历史" name="test-history">
        <el-card class="test-history-card app-card">
          <template #header>
            <div class="card-header">
              <h2 class="app-card-title">测试历史记录</h2>
              <div class="header-actions">
                <el-button size="small" type="danger" @click="clearHistory" :disabled="!testHistory || testHistory.length === 0">
                  <i class="el-icon-delete"></i> 清除历史
                </el-button>
              </div>
            </div>
          </template>
          
          <el-empty v-if="!testHistory || testHistory.length === 0" description="暂无测试历史记录"></el-empty>
          
          <el-table v-else :data="testHistory" style="width: 100%" :default-sort="{prop: 'timestamp', order: 'descending'}">
            <el-table-column prop="timestamp" label="测试时间" sortable width="180">
              <template #default="scope">
                {{ formatDateTime(scope.row.summary.timestamp) }}
              </template>
            </el-table-column>
            <el-table-column prop="targetUrl" label="目标URL" width="250">
              <template #default="scope">
                {{ scope.row.targetUrl }}
              </template>
            </el-table-column>
            <el-table-column prop="testType" label="测试类型" width="120">
              <template #default="scope">
                {{ getTestTypeDisplayName(scope.row.testType) }}
              </template>
            </el-table-column>
            <el-table-column prop="successRate" label="成功率">
              <template #default="scope">
                <el-tag :type="calculateSuccessType(scope.row.summary.success_rate)">
                  {{ scope.row.summary.success_rate }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column label="操作" width="150">
              <template #default="scope">
                <el-button size="mini" type="primary" @click="viewHistoryResult(scope.row)">查看</el-button>
                <el-button size="mini" type="success" @click="rerunTest(scope.row)">重新测试</el-button>
              </template>
            </el-table-column>
          </el-table>
        </el-card>
      </el-tab-pane>
    </el-tabs>
    
    <el-alert
      v-if="a2aStore.error"
      type="error"
      :title="a2aStore.error"
      show-icon
      @close="a2aStore.clearError()"
      class="mt-3"
    ></el-alert>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useA2AStore } from '@/store/a2a'
import { useAgentStore } from '@/store/agent'
import { ElMessage } from 'element-plus'
import InteroperabilityTestConfig from './InteroperabilityTestConfig.vue'
import InteroperabilityTestLogs from '@/components/a2a/InteroperabilityTestLogs.vue'

export default {
  name: 'InteroperabilityTest',
  
  components: {
    InteroperabilityTestConfig,
    InteroperabilityTestLogs
  },
  
  setup() {
    const router = useRouter()
    const a2aStore = useA2AStore()
    const agentStore = useAgentStore()
    
    const testFormRef = ref(null)
    const testForm = ref({
      agentId: '',
      targetUrl: '',
      testType: 'basic',
      taskContent: '请执行一个简单任务：计算1+1等于多少？',
      timeout: 30,
      retries: 2,
      verifySSL: true,
      testCases: ['agent_card', 'task_send', 'task_get']
    })
    
    const activeTab = ref('test-config')
    const testResults = computed(() => a2aStore.getInteropTestResults)
    const testHistory = ref(getTestHistoryFromLocalStorage())
    const testResultFilter = ref('all')
    const currentTestId = ref(null)
    const testLogsRef = ref(null)
    
    // 根据过滤条件筛选测试结果
    const filteredTestResults = computed(() => {
      if (!testResults.value) return {};
      
      if (testResultFilter.value === 'all') {
        return Object.entries(testResults.value)
          .filter(([key]) => key !== 'summary')
          .reduce((acc, [key, value]) => {
            acc[key] = value;
            return acc;
          }, {});
      } else {
        return Object.entries(testResults.value)
          .filter(([key, value]) => key !== 'summary' && value.status === testResultFilter.value)
          .reduce((acc, [key, value]) => {
            acc[key] = value;
            return acc;
          }, {});
      }
    });
    
    const rules = {
      agentId: [
        { required: true, message: '请选择一个Agent', trigger: 'change' }
      ],
      targetUrl: [
        { required: true, message: '请输入目标A2A系统的URL', trigger: 'blur' },
        { pattern: /^https?:\/\/.+/i, message: 'URL必须以http://或https://开头', trigger: 'blur' }
      ],
      testType: [
        { required: true, message: '请选择测试类型', trigger: 'change' }
      ]
    }
    
    // 从本地存储获取测试历史
    function getTestHistoryFromLocalStorage() {
      try {
        const historyString = localStorage.getItem('a2a_interop_test_history');
        return historyString ? JSON.parse(historyString) : [];
      } catch (error) {
        console.error('读取测试历史失败:', error);
        return [];
      }
    }
    
    // 保存测试历史到本地存储
    function saveTestHistoryToLocalStorage() {
      try {
        localStorage.setItem('a2a_interop_test_history', JSON.stringify(testHistory.value));
      } catch (error) {
        console.error('保存测试历史失败:', error);
      }
    }
    
    // 添加当前测试结果到历史记录
    function addToHistory(result) {
      const historyItem = {
        id: Date.now().toString(),
        timestamp: new Date().toISOString(),
        agentId: testForm.value.agentId,
        targetUrl: testForm.value.targetUrl,
        testType: testForm.value.testType,
        summary: result.summary,
        results: result
      };
      
      testHistory.value.unshift(historyItem);
      // 限制历史记录数量为20条
      if (testHistory.value.length > 20) {
        testHistory.value = testHistory.value.slice(0, 20);
      }
      
      saveTestHistoryToLocalStorage();
    }
    
    // 清除历史记录
    function clearHistory() {
      testHistory.value = [];
      saveTestHistoryToLocalStorage();
      ElMessage.success('历史记录已清除');
    }
    
    // 查看历史测试结果
    function viewHistoryResult(historyItem) {
      a2aStore.setInteropTestResults(historyItem.results);
      activeTab.value = 'test-results';
    }
    
    // 使用历史记录中的配置重新运行测试
    function rerunTest(historyItem) {
      testForm.value.agentId = historyItem.agentId;
      testForm.value.targetUrl = historyItem.targetUrl;
      testForm.value.testType = historyItem.testType;
      
      activeTab.value = 'test-config';
      ElMessage.info('已加载历史测试配置，请点击"执行互操作性测试"按钮开始测试');
    }
    
    // 加载Agent列表
    onMounted(async () => {
      if (agentStore.getAgents.length === 0) {
        await agentStore.fetchAgents()
      }
    })
    
    // 监听测试结果变化，如果有新结果就添加到历史
    watch(testResults, (newResults) => {
      if (newResults) {
        addToHistory(newResults);
        activeTab.value = 'test-results';
      }
    });
    
    // 运行测试
    const runTest = () => {
      testFormRef.value.validate(async (valid) => {
        if (valid) {
          // 生成一个新的测试ID
          currentTestId.value = Date.now().toString()
          
          // 记录测试开始日志
          if (testLogsRef.value) {
            testLogsRef.value.log(`开始测试目标: ${testForm.value.targetUrl}`);
            testLogsRef.value.log(`测试类型: ${getTestTypeDisplayName(testForm.value.testType)}`);
            testLogsRef.value.log(`已选择测试用例: ${testForm.value.testCases.join(', ')}`);
          }
          
          const testParams = {
            agent_id: testForm.value.agentId,
            target_url: testForm.value.targetUrl,
            test_type: testForm.value.testType,
            options: {
              task_content: testForm.value.taskContent,
              timeout: testForm.value.timeout,
              retries: testForm.value.retries,
              verify_ssl: testForm.value.verifySSL,
              test_cases: testForm.value.testCases
            }
          };
          
          try {
            if (testLogsRef.value) {
              testLogsRef.value.log('发送测试请求...');
            }
            
            await a2aStore.runInteroperabilityTest(testParams);
            
            if (testLogsRef.value && a2aStore.getInteropTestResults) {
              const results = a2aStore.getInteropTestResults;
              testLogsRef.value.success(`测试完成: 成功率 ${results.summary.success_rate}`);
              
              // 记录每个测试用例的结果
              Object.entries(results)
                .filter(([key]) => key !== 'summary')
                .forEach(([key, value]) => {
                  const logMethod = value.status === 'success' ? 'success' : 
                                    value.status === 'failed' ? 'error' : 'warn';
                  
                  testLogsRef.value[logMethod](`${getTestTitle(key)}: ${value.status.toUpperCase()}`);
                  
                  if (value.error) {
                    testLogsRef.value.error(`错误信息: ${value.error}`);
                  }
                });
            }
          } catch (error) {
            if (testLogsRef.value) {
              testLogsRef.value.error(`测试失败: ${error.message || '未知错误'}`);
            }
          }
        }
      })
    }
    
    // 重置表单
    const resetForm = () => {
      testFormRef.value.resetFields();
      testForm.value.taskContent = '请执行一个简单任务：计算1+1等于多少？';
      testForm.value.timeout = 30;
      testForm.value.retries = 2;
      testForm.value.verifySSL = true;
      testForm.value.testCases = ['agent_card', 'task_send', 'task_get'];
    }
    
    // 导出测试结果
    const exportTestResults = () => {
      if (!testResults.value) return;
      
      const dataStr = JSON.stringify(testResults.value, null, 2);
      const dataUri = 'data:application/json;charset=utf-8,'+ encodeURIComponent(dataStr);
      
      const exportFileName = `a2a_interop_test_${new Date().toISOString().replace(/:/g, '-')}.json`;
      
      const linkElement = document.createElement('a');
      linkElement.setAttribute('href', dataUri);
      linkElement.setAttribute('download', exportFileName);
      linkElement.click();
    }
    
    // 辅助函数 - 格式化日期时间
    const formatDateTime = (timestamp) => {
      if (!timestamp) return '-'
      return new Date(timestamp).toLocaleString()
    }
    
    // 辅助函数 - 获取状态类型对应的样式
    const getStatusType = (status) => {
      switch (status) {
        case 'success':
          return 'success'
        case 'failed':
          return 'danger'
        case 'skipped':
          return 'info'
        case 'pending':
          return 'warning'
        default:
          return 'info'
      }
    }
    
    // 辅助函数 - 计算总体成功率的标签类型
    const calculateSuccessType = (successRate) => {
      if (!successRate) return 'warning'
      
      const [success, total] = successRate.split('/')
      const rate = parseInt(success) / parseInt(total)
      
      if (rate === 1) return 'success'
      if (rate >= 0.7) return 'warning'
      return 'danger'
    }
    
    // 辅助函数 - 获取测试用例标题
    const getTestTitle = (key) => {
      const titleMap = {
        agent_card_test: 'Agent Card测试',
        task_send_test: '发送任务测试',
        task_get_test: '获取任务测试',
        task_cancel_test: '取消任务测试',
        task_input_test: '任务输入测试',
        streaming_test: '流式响应测试',
        schema_validation_test: 'Schema验证测试'
      };
      
      return titleMap[key] || key;
    }
    
    // 辅助函数 - 获取测试类型显示名称
    const getTestTypeDisplayName = (type) => {
      const typeMap = {
        'basic': '基本测试',
        'streaming': '流式响应',
        'push_notification': '推送通知',
        'full': '完整测试'
      };
      
      return typeMap[type] || type;
    }
    
    // 加载保存的测试配置
    const loadTestConfig = (config) => {
      testForm.value.agentId = config.agentId;
      testForm.value.targetUrl = config.targetUrl;
      testForm.value.testType = config.testType;
      testForm.value.taskContent = config.taskContent;
      testForm.value.timeout = config.timeout;
      testForm.value.retries = config.retries;
      testForm.value.verifySSL = config.verifySSL;
      testForm.value.testCases = [...config.testCases];
      
      ElMessage.success('测试配置已加载');
    }
    
    // 处理日志事件
    const handleLogEvent = (log) => {
      console.log('日志:', log);
      // 可以在这里做额外的处理，比如发送到远程日志服务器
    }
    
    return {
      a2aStore,
      testFormRef,
      testForm,
      rules,
      testResults,
      testHistory,
      activeTab,
      testResultFilter,
      filteredTestResults,
      agents: computed(() => agentStore.getAgents),
      runTest,
      resetForm,
      formatDateTime,
      getStatusType,
      calculateSuccessType,
      exportTestResults,
      clearHistory,
      viewHistoryResult,
      rerunTest,
      getTestTitle,
      getTestTypeDisplayName,
      loadTestConfig,
      currentTestId,
      testLogsRef,
      handleLogEvent
    }
  }
}
</script>

<style scoped>
.a2a-interop-test-container {
  max-width: 1200px;
  margin: 0 auto;
  padding: 20px;
}

/* 通用页面标题样式 */
.app-page-title {
  font-size: 24px;
  font-weight: 600;
  color: #303133;
  margin-bottom: 20px;
}

/* 通用卡片样式 */
.app-card {
  border-radius: 8px;
  border: 1px solid #ebeef5;
  box-shadow: 0 2px 12px 0 rgba(0,0,0,.05);
  margin-bottom: 20px;
}

/* 卡片标题样式 */
.app-card-title {
  font-size: 18px;
  font-weight: 600;
  color: #303133;
  margin: 0;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.header-actions {
  display: flex;
  gap: 10px;
}

.test-form-card, .test-results-card, .test-logs-card, .test-history-card {
  margin-bottom: 20px;
}

.test-summary {
  margin-bottom: 20px;
}

.test-filter-bar {
  margin: 15px 0;
}

.test-cases-container {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.json-content {
  background-color: #f9f9f9;
  padding: 10px;
  border-radius: 4px;
  max-height: 200px;
  overflow: auto;
  font-family: monospace;
  font-size: 13px;
  white-space: pre-wrap;
}

.error-text {
  color: #f56c6c;
  font-weight: 500;
}

.mt-3 {
  margin-top: 15px;
}
</style> 