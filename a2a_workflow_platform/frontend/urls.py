from django.urls import path
from . import views

app_name = 'frontend'

urlpatterns = [
    # Vue应用路由 - 捕获所有路径并交由前端路由处理
    path('app/', views.VueAppView.as_view(), name='vue_app'),
    path('app/<path:path>', views.VueAppView.as_view(), name='vue_app_all'),
    
    # 保留必要的API路由
    path('workflow/save/', views.WorkflowSaveView.as_view(), name='workflow_save'),
    path('workflow/save/<int:pk>/', views.WorkflowSaveView.as_view(), name='workflow_save'),
    path('workflow/<int:pk>/start/', views.WorkflowStartInstanceView.as_view(), name='workflow_start_instance'),
    path('workflow/instance/<uuid:instance_id>/start/', views.WorkflowInstanceStartView.as_view(), name='workflow_instance_start'),
    path('workflow/instance/<uuid:instance_id>/pause/', views.WorkflowInstancePauseView.as_view(), name='workflow_instance_pause'),
    path('workflow/instance/<uuid:instance_id>/cancel/', views.WorkflowInstanceCancelView.as_view(), name='workflow_instance_cancel'),
    path('workflow/instance/<uuid:instance_id>/clone/', views.WorkflowInstanceCloneView.as_view(), name='workflow_instance_clone'),
    path('workflow/instance/<uuid:instance_id>/retry-step/<str:step_id>/', views.WorkflowStepRetryView.as_view(), name='workflow_step_retry'),
] 