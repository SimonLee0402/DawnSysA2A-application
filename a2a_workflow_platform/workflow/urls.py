from django.urls import path
from . import views

app_name = 'workflow'

urlpatterns = [
    # 工作流模板API
    path('', views.WorkflowListCreateView.as_view(), name='workflow_list_create'),
    path('<int:pk>/', views.WorkflowRetrieveUpdateDestroyView.as_view(), name='workflow_detail'),
    
    # 工作流实例API
    path('instances/', views.WorkflowInstanceListView.as_view(), name='workflow_instance_list'),
    path('instances/<uuid:instance_id>/', views.WorkflowInstanceDetailView.as_view(), name='workflow_instance_detail'),
    path('instances/<uuid:instance_id>/start/', views.WorkflowInstanceStartView.as_view(), name='workflow_instance_start'),
    path('instances/<uuid:instance_id>/cancel/', views.WorkflowInstanceCancelView.as_view(), name='workflow_instance_cancel'),
    path('instances/<uuid:instance_id>/retry-step/<str:step_id>/', views.WorkflowStepRetryView.as_view(), name='workflow_step_retry'),
    
    # 回调API
    path('callback/', views.WorkflowCallbackView.as_view(), name='workflow_callback'),
] 