from django.urls import path, include
from rest_framework.routers import DefaultRouter
from rest_framework_nested import routers
from .views import AgentViewSet, AgentCardView, AgentSkillViewSet, A2AInteroperabilityTestView, TaskViewSet, SessionViewSet
from .views_a2a import (
    A2AWellKnownAgentView,
    A2ATasksSendView,
    A2ATasksGetView,
    A2ATasksCancelView,
    A2ATasksSendSubscribeView,
    A2ATasksResubscribeView,
    A2ATasksPushNotificationSetView,
    A2ATasksListView,
    A2ATasksTreeView,
    A2ATasksStateHistoryView
)

# 创建主路由器并注册视图集
router = DefaultRouter()
router.register(r'agents', AgentViewSet, basename='agent')
router.register(r'tasks', TaskViewSet, basename='task')
router.register(r'sessions', SessionViewSet, basename='session')

# 创建嵌套路由器用于Agent技能
agent_router = routers.NestedSimpleRouter(router, r'agents', lookup='agent')
agent_router.register(r'skills', AgentSkillViewSet, basename='agent-skill')

app_name = 'a2a_client'

urlpatterns = [
    # 常规API端点
    path('', include(router.urls)),
    path('', include(agent_router.urls)),
    
    # Agent Card端点
    path('agents/<uuid:agent_id>/card/', AgentCardView.as_view(), name='agent-card'),
    
    # A2A协议端点
    path('.well-known/agent.json', A2AWellKnownAgentView.as_view(), name='well-known-agent'),
    path('api/a2a/agents/<uuid:agent_id>/.well-known/agent.json', A2AWellKnownAgentView.as_view(), name='agent-well-known'),
    
    # A2A JSON-RPC端点
    path('api/a2a/tasks/send', A2ATasksSendView.as_view(), name='a2a-tasks-send'),
    path('api/a2a/tasks/get', A2ATasksGetView.as_view(), name='a2a-tasks-get'),
    path('api/a2a/tasks/list', A2ATasksListView.as_view(), name='a2a-tasks-list'),
    path('api/a2a/tasks/cancel', A2ATasksCancelView.as_view(), name='a2a-tasks-cancel'),
    path('api/a2a/tasks/pushNotification/set', A2ATasksPushNotificationSetView.as_view(), name='a2a-tasks-push-notification-set'),
    
    # A2A SSE流式端点
    path('api/a2a/tasks/sendSubscribe', A2ATasksSendSubscribeView.as_view(), name='a2a-tasks-send-subscribe'),
    path('api/a2a/tasks/resubscribe', A2ATasksResubscribeView.as_view(), name='a2a-tasks-resubscribe'),
    
    # A2A任务树和状态历史
    path('api/a2a/tasks/tree', A2ATasksTreeView.as_view(), name='a2a-tasks-tree'),
    path('api/a2a/tasks/stateHistory', A2ATasksStateHistoryView.as_view(), name='a2a-tasks-state-history'),
    
    # A2A互操作性测试
    path('api/a2a/test/interoperability', A2AInteroperabilityTestView.as_view(), name='a2a-interoperability-test'),
] 