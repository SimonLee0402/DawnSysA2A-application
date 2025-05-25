import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '../store/auth' // 导入Auth Store

// 导入布局组件
import MainLayout from '../components/layout/MainLayout.vue'

// 导入视图组件
const Home = () => import('../src/views/Home.vue')
const Login = () => import('../src/views/auth/Login.vue')
const Register = () => import('../src/views/auth/Register.vue')
const Profile = () => import('../src/views/auth/Profile.vue')

// 工作流相关视图
const WorkflowList = () => import('../src/views/workflow/WorkflowList.vue')
const WorkflowDetail = () => import('../src/views/workflow/WorkflowDetail.vue')
const WorkflowEditor = () => import('../src/views/workflow/WorkflowEditor.vue')
const WorkflowInstanceList = () => import('../src/views/workflow/WorkflowInstanceList.vue')
const WorkflowInstanceDetail = () => import('../src/views/workflow/WorkflowInstanceDetail.vue')

// Agent相关视图
const AgentList = () => import('../src/views/agent/AgentListView.vue')
const AgentDetail = () => import('../src/views/agent/AgentDetailView.vue')
const AgentTest = () => import('../src/views/agent/AgentTest.vue')
const LinkedAgentsDisplay = () => import('../src/views/agent/LinkedAgentsDisplay.vue')

// Task相关视图
const TaskList = () => import('../src/views/task/TaskList.vue')
const TaskDetail = () => import('../src/views/task/TaskDetail.vue')
const TaskCreate = () => import('../src/views/task/TaskCreate.vue')

// A2A互操作性测试
const InteroperabilityTest = () => import('../src/views/a2a/InteroperabilityTest.vue')

// Session相关视图
const SessionList = () => import('../src/views/session/SessionList.vue')
const SessionDetail = () => import('../src/views/session/SessionDetail.vue')
const SessionForm = () => import('../src/views/session/SessionForm.vue')

// KnowledgeBase 相关视图 (新增)
const KnowledgeBaseList = () => import('../src/views/knowledgebase/KnowledgeBaseListView.vue') // 注意路径是 ../src/views
const KnowledgeBaseDetail = () => import('../src/views/knowledgebase/KnowledgeBaseDetailView.vue') // 新增详情视图导入

// 404页面
const NotFound = () => import('../src/views/NotFound.vue')

// 路由配置
const routes = [
  {
    path: '/', // 所有使用 MainLayout 的页面都将基于此根路径
    component: MainLayout,
    redirect: '/home',
    children: [
      {
        path: 'home',
        name: 'Home',
        component: Home,
        meta: { title: '首页 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'profile', // 对应之前的 path: '/profile'
        name: 'Profile',
        component: Profile,
        meta: { title: '个人资料 - A2A工作流平台', requiresAuth: true }
      },
      // 工作流相关路由 (作为 MainLayout 的子路由)
      {
        path: 'workflow',
        name: 'WorkflowList',
        component: WorkflowList,
        meta: { title: '工作流列表 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'workflow/:id',
        name: 'WorkflowDetail',
        component: WorkflowDetail,
        meta: { title: '工作流详情 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'workflow/create',
        name: 'WorkflowCreate',
        component: WorkflowEditor,
        meta: { title: '创建工作流 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'workflow/designer',
        name: 'WorkflowDesigner',
        component: WorkflowEditor,
        props: { isDesignerMode: true },
        meta: { title: '工作流设计器 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'workflow/:id/edit',
        name: 'WorkflowEdit',
        component: WorkflowEditor,
        meta: { title: '编辑工作流 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'workflow/instance',
        name: 'WorkflowInstanceList',
        component: WorkflowInstanceList,
        meta: { title: '工作流实例列表 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'workflow/instance/:id',
        name: 'WorkflowInstanceDetail',
        component: WorkflowInstanceDetail,
        meta: { title: '工作流实例详情 - A2A工作流平台', requiresAuth: true }
      },
      // Agent相关路由
      {
        path: 'agents',
        name: 'AgentList',
        component: AgentList,
        meta: { title: '智能体列表 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'agents/:id',
        name: 'AgentDetail',
        component: AgentDetail,
        meta: { title: '智能体详情 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'agents/:id/test',
        name: 'AgentTest',
        component: AgentTest,
        meta: { title: '测试智能体 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'linked-agents',
        name: 'LinkedAgentsDisplay',
        component: LinkedAgentsDisplay,
        meta: { title: '已链接的外部智能体 - A2A工作流平台', requiresAuth: true }
      },
      // Task相关路由
      {
        path: 'tasks',
        name: 'TaskList',
        component: TaskList,
        meta: { title: '任务列表 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'tasks/:id',
        name: 'TaskDetail',
        component: TaskDetail,
        meta: { title: '任务详情 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'tasks/create',
        name: 'TaskCreate',
        component: TaskCreate,
        meta: { title: '创建任务 - A2A工作流平台', requiresAuth: true }
      },
      // Session相关路由
      {
        path: 'sessions',
        name: 'SessionList',
        component: SessionList,
        meta: { title: '会话列表 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'sessions/:id',
        name: 'SessionDetail',
        component: SessionDetail,
        meta: { title: '会话详情 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'sessions/create',
        name: 'SessionCreate',
        component: SessionForm,
        meta: { title: '创建会话 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'sessions/:id/edit',
        name: 'SessionEdit',
        component: SessionForm,
        meta: { title: '编辑会话 - A2A工作流平台', requiresAuth: true }
      },
      // A2A协议相关路由
      {
        path: 'a2a/interop-test',
        name: 'InteroperabilityTest',
        component: InteroperabilityTest,
        meta: { title: 'A2A互操作性测试 - A2A工作流平台', requiresAuth: true }
      },
      // KnowledgeBase 相关路由 (新增)
      {
        path: 'knowledgebases',
        name: 'KnowledgeBaseList',
        component: KnowledgeBaseList,
        meta: { title: '知识库管理 - A2A工作流平台', requiresAuth: true }
      },
      {
        path: 'knowledgebases/:id',
        name: 'KnowledgeBaseDetail', // 这个 name 在 ListView 中已经用到了
        component: KnowledgeBaseDetail,
        meta: { title: '知识库详情 - A2A工作流平台', requiresAuth: true }
      },
    ]
  },
  // 不使用 MainLayout 的顶级路由
  {
    path: '/login',
    name: 'Login',
    component: Login,
    meta: { title: '登录 - A2A工作流平台', allowAnonymous: true, guestOnly: true }
  },
  {
    path: '/register',
    name: 'Register',
    component: Register,
    meta: { title: '注册 - A2A工作流平台', allowAnonymous: true, guestOnly: true }
  },
  {
    path: '/404',
    name: 'NotFound',
    component: NotFound,
    meta: { title: '页面未找到 - A2A工作流平台', allowAnonymous: true }
  },
  // 通配符路由，放在最后
  // 如果 /:pathMatch(.*)* 仍然重定向到 '/', 
  // 而 '/' 的根路由是 MainLayout 下的 Home，这通常是期望的行为。
  // 如果希望未匹配的路由直接到404，可以改为 redirect: '/404' 或 redirect: { name: 'NotFound' }
  {
    path: '/:pathMatch(.*)*',
    redirect: '/', // 或者 redirect: { name: 'NotFound' }
    meta: { allowAnonymous: true } 
  }
]

// 创建路由实例
const router = createRouter({
  // 确保使用history模式而不是hash模式
  history: createWebHistory(),
  routes
})

// 路由前置守卫 - 处理页面标题和认证
router.beforeEach((to, from, next) => {
  // 设置页面标题
  document.title = to.meta.title || 'A2A工作流平台'
  
  // 在守卫函数内部获取 authStore 实例
  // 注意：这要求 Pinia 实例已在 router 实例创建并传递给 Vue app 之前或同时被创建和使用
  // 通常在 main.js 中 app.use(pinia) 会先于 app.use(router)
  const authStore = useAuthStore();
  const isAuthenticated = authStore.isAuthenticated; // 使用 store 的实时状态
  
  console.log(`[Router Guard] From: ${from.path}, To: ${to.path}, IsAuthenticated: ${isAuthenticated}`);
  
  // 规范化URL路径 (移除尾部斜杠)
  if (to.path.endsWith('/') && to.path !== '/') {
    const newPath = to.path.slice(0, -1);
    console.log(`[Router Guard] Normalizing URL: ${to.path} -> ${newPath}`);
    next({ path: newPath, query: to.query, hash: to.hash, replace: true });
    return;
  }
  
  // 如果目标路由只允许访客访问 (如登录、注册页)
  if (to.meta.guestOnly) {
    if (isAuthenticated) {
      console.log('[Router Guard] User is authenticated, redirecting from guest-only page to Home.');
      next({ name: 'Home' }); // 或重定向到仪表盘 'Dashboard'
      return;
    } else {
      next(); // 允许未认证用户访问
      return;
    }
  }
  
  // 如果目标路由需要认证
  if (to.meta.requiresAuth) {
    if (!isAuthenticated) {
      console.log('[Router Guard] Route requires auth, user not authenticated. Redirecting to Login.');
      next({ name: 'Login', query: { redirect: to.fullPath } });
      return;
    }
  }
  
  // 对于 allowAnonymous 或已通过认证检查的，允许访问
  // (Home 页面虽然 allowAnonymous，但如果用户已登录也不应被 guestOnly 规则重定向)
  console.log(`[Router Guard] Allowing navigation to: ${to.path}`);
  next();
})

// 路由错误处理
router.onError((error) => {
  console.error('路由错误:', error)
  
  // 捕获特定类型的错误并进行处理
  const targetError = error.toString()
  
  if (targetError.includes('Failed to fetch dynamically imported module')) {
    console.error('动态导入模块失败，可能是网络问题或代码拆分错误')
    // 重定向到错误页面或者刷新
    window.location.reload()
  } else if (targetError.includes('Loading chunk') && targetError.includes('failed')) {
    console.error('加载代码块失败，可能是版本不兼容')
    // 刷新页面尝试重新加载
    window.location.reload()
  }
})

export default router 