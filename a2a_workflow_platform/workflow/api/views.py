from rest_framework import viewsets, permissions, status
from rest_framework.decorators import action, api_view, permission_classes
from rest_framework.response import Response
from django.shortcuts import get_object_or_404
from django.utils.timezone import timezone
from rest_framework.exceptions import NotFound

from workflow.models import Workflow, WorkflowInstance, WorkflowStep
from workflow.api.serializers import (
    WorkflowSerializer, 
    WorkflowInstanceSerializer, 
    WorkflowStepSerializer
)
from workflow.execution_engine import ExecutionEngine


class WorkflowViewSet(viewsets.ModelViewSet):
    """工作流API视图集"""
    serializer_class = WorkflowSerializer
    permission_classes = [permissions.IsAuthenticated]

    def get_queryset(self):
        """获取查询集，基于用户权限过滤"""
        user = self.request.user
        if user.is_superuser:
            return Workflow.objects.all()
        else:
            # 允许用户查看自己创建的工作流和其他公开的工作流
            return Workflow.objects.filter(
                created_by=user
            ) | Workflow.objects.filter(
                is_public=True
            )

    def perform_create(self, serializer):
        """创建时自动关联当前用户"""
        serializer.save(user=self.request.user)

    @action(detail=True, methods=['post'])
    def execute(self, request, pk=None):
        """执行工作流"""
        workflow = self.get_object()
        if not workflow:
            raise NotFound(detail="Workflow not found.")

        parameters = request.data.get('parameters', {})

        # Create a new workflow instance
        instance = WorkflowInstance.objects.create(
            workflow=workflow,
            started_by=request.user,
            context={'input_params': parameters},
            status='created'
        )

        # TODO: Trigger the actual workflow execution asynchronously (e.g., Celery task)
        # For now, let's assume direct execution or a synchronous first step
        # from ..engine import WorkflowEngine
        # engine = WorkflowEngine(instance)
        # engine.start()

        serializer = WorkflowInstanceSerializer(instance)
        return Response(serializer.data, status=status.HTTP_201_CREATED)


class WorkflowInstanceViewSet(viewsets.ReadOnlyModelViewSet):
    """工作流实例API视图集"""
    serializer_class = WorkflowInstanceSerializer
    permission_classes = [permissions.IsAuthenticated]

    def get_queryset(self):
        """获取查询集，基于用户权限过滤"""
        user = self.request.user
        if user.is_superuser:
            return WorkflowInstance.objects.all()
        else:
            # 用户只能查看自己启动的工作流实例
            return WorkflowInstance.objects.filter(started_by=user)

    @action(detail=True, methods=['post'])
    def cancel(self, request, pk=None):
        """取消工作流实例"""
        instance = self.get_object()
        
        if instance.status in ['completed', 'cancelled', 'failed']:
            return Response(
                {"detail": "无法取消已完成、已取消或已失败的工作流实例"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        instance.status = 'cancelled'
        instance.save()
        
        return Response({"status": "success"})

    @action(detail=True, methods=['post'])
    def pause(self, request, pk=None):
        """暂停工作流实例"""
        instance = self.get_object()
        
        if instance.status != 'running':
            return Response(
                {"detail": "只有运行中的工作流实例才能暂停"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        instance.status = 'paused'
        instance.save()
        
        return Response({"status": "success"})

    @action(detail=True, methods=['post'])
    def resume(self, request, pk=None):
        """恢复工作流实例"""
        instance = self.get_object()
        
        if instance.status != 'paused':
            return Response(
                {"detail": "只有已暂停的工作流实例才能恢复"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        instance.status = 'running'
        instance.save()
        
        # 重新启动执行引擎
        engine = ExecutionEngine()
        engine.resume_workflow(instance.id)
        
        return Response({"status": "success"})


class WorkflowStepViewSet(viewsets.ReadOnlyModelViewSet):
    """工作流步骤API视图集"""
    serializer_class = WorkflowStepSerializer
    permission_classes = [permissions.IsAuthenticated]

    def get_queryset(self):
        """获取查询集，基于用户权限过滤"""
        instance_id = self.kwargs.get('instance_pk')
        
        if not instance_id:
            return WorkflowStep.objects.none()
            
        instance = get_object_or_404(WorkflowInstance, id=instance_id)
        
        # 检查用户是否有权访问此工作流实例
        if not instance.user == self.request.user and not self.request.user.is_superuser:
            return WorkflowStep.objects.none()
            
        return WorkflowStep.objects.filter(workflow_instance=instance)

    @action(detail=True, methods=['post'])
    def retry(self, request, instance_pk=None, pk=None):
        """重试失败的工作流步骤"""
        step = self.get_object()
        
        if step.status != 'failed':
            return Response(
                {"detail": "只有失败的步骤才能重试"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # 重置步骤状态
        step.status = 'pending'
        step.error_message = ''
        step.save()
        
        # 重启工作流实例
        instance = step.workflow_instance
        instance.status = 'running'
        instance.save()
        
        # 重新启动执行引擎
        engine = ExecutionEngine()
        engine.retry_step(instance.id, step.id)
        
        return Response({"status": "success"})


# 添加Dashboard API视图
@api_view(['GET'])
@permission_classes([permissions.AllowAny])  # 明确允许所有用户访问
def dashboard_view(request):
    """
    获取平台概览信息，包括工作流数量、运行中实例、智能体数量和任务数量
    """
    from workflow.models import Workflow, WorkflowInstance
    from a2a_client.models import Agent, Task
    
    # 即使未登录也允许访问
    if request.user.is_anonymous:
        # 未登录用户返回空数据
        return Response({
            'workflows': 0,
            'runningInstances': 0,
            'agents': 0,
            'tasks': 0
        })
    
    # 已登录用户获取真实数据
    workflows_count = Workflow.objects.count()
    running_instances_count = WorkflowInstance.objects.filter(status='running').count()
    agents_count = Agent.objects.count()
    tasks_count = Task.objects.count()
    
    return Response({
        'workflows': workflows_count,
        'runningInstances': running_instances_count,
        'agents': agents_count,
        'tasks': tasks_count
    }) 