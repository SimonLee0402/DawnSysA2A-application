from django.urls import path, include
from rest_framework.routers import DefaultRouter
from rest_framework_nested.routers import NestedDefaultRouter

from workflow.api.views import (
    WorkflowViewSet,
    WorkflowInstanceViewSet,
    WorkflowStepViewSet,
    dashboard_view
)

# 创建标准路由器
router = DefaultRouter()
router.register(r'', WorkflowViewSet, basename='workflow')
router.register(r'workflows/instances', WorkflowInstanceViewSet, basename='workflow-instance')

# 创建嵌套路由器
instance_router = NestedDefaultRouter(router, r'workflows/instances', lookup='instance')
instance_router.register(r'steps', WorkflowStepViewSet, basename='workflow-instance-step')

# API URL模式
urlpatterns = [
    path('', include(router.urls)),
    path('', include(instance_router.urls)),
    path('dashboard/', dashboard_view, name='dashboard')
] 