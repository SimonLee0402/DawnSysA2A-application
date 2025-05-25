from django.shortcuts import render, get_object_or_404
from django.http import JsonResponse, HttpResponseBadRequest
from django.views import View
from django.utils import timezone
from django.db import transaction
from rest_framework import generics, permissions, status, viewsets
from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework.decorators import action
import logging
import json
import traceback
from django.db.models import Q

from .models import Workflow, WorkflowInstance, WorkflowStep, A2AAgent
from .serializers import (
    WorkflowSerializer, WorkflowInstanceSerializer, 
    WorkflowStepSerializer, A2AAgentSerializer,
    WorkflowStepInstanceSerializer
)
from .permissions import IsWorkflowOwnerOrReadOnly, IsInstanceOwnerOrReadOnly
from .engine import execute_workflow, resume_workflow

logger = logging.getLogger(__name__)

# Create your views here.

# ======== 工作流模板视图 ========

class WorkflowListCreateView(generics.ListCreateAPIView):
    """
    列出所有工作流模板或创建新的工作流模板
    """
    queryset = Workflow.objects.all()
    serializer_class = WorkflowSerializer
    permission_classes = [permissions.IsAuthenticated]
    
    def get_queryset(self):
        """过滤只返回当前用户有权限查看的工作流"""
        return Workflow.objects.filter(
            Q(created_by=self.request.user) | Q(is_public=True)
        )
    
    def perform_create(self, serializer):
        serializer.save(created_by=self.request.user)


class WorkflowRetrieveUpdateDestroyView(generics.RetrieveUpdateDestroyAPIView):
    queryset = Workflow.objects.all()
    serializer_class = WorkflowSerializer
    permission_classes = [permissions.IsAuthenticated]
    
    def get_queryset(self):
        """过滤只返回当前用户有权限查看的工作流"""
        return Workflow.objects.filter(
            Q(created_by=self.request.user) | Q(is_public=True)
        )


# ======== 工作流实例视图 ========

class WorkflowInstanceListView(generics.ListCreateAPIView):
    """
    列出所有工作流实例或创建新的工作流实例
    """
    serializer_class = WorkflowInstanceSerializer
    permission_classes = [permissions.IsAuthenticated]
    
    def get_queryset(self):
        """
        根据用户权限过滤工作流实例
        """
        user = self.request.user
        if user.is_staff:
            # 管理员可以看到所有实例
            return WorkflowInstance.objects.all()
        else:
            # 普通用户只能看到自己启动的实例
            return WorkflowInstance.objects.filter(started_by=user)
    
    def perform_create(self, serializer):
        """
        创建新实例时，自动设置启动者为当前用户
        """
        workflow_id = self.request.data.get('workflow')
        workflow = get_object_or_404(Workflow, id=workflow_id)
        
        # 检查用户是否有权限使用该工作流
        if not (workflow.is_public or 
                workflow.created_by == self.request.user or 
                workflow.workflow_type == self.request.user.user_type):
            return Response(
                {"detail": "您没有权限使用此工作流"},
                status=status.HTTP_403_FORBIDDEN
            )
        
        serializer.save(started_by=self.request.user)


class WorkflowInstanceDetailView(generics.RetrieveAPIView):
    """
    查看工作流实例详情
    """
    serializer_class = WorkflowInstanceSerializer
    permission_classes = [permissions.IsAuthenticated, IsInstanceOwnerOrReadOnly]
    lookup_field = 'instance_id'
    
    def get_queryset(self):
        user = self.request.user
        if user.is_staff:
            return WorkflowInstance.objects.all()
        else:
            return WorkflowInstance.objects.filter(started_by=user)


class WorkflowInstanceStartView(APIView):
    """
    启动工作流实例
    """
    permission_classes = [permissions.IsAuthenticated, IsInstanceOwnerOrReadOnly]
    
    def post(self, request, instance_id):
        instance = get_object_or_404(WorkflowInstance, instance_id=instance_id)
        
        # 检查实例状态
        if instance.status not in ['created', 'paused']:
            return Response(
                {"detail": f"实例状态为 {instance.status}，无法启动"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # 使用工作流引擎启动实例
        try:
            execute_workflow(instance.instance_id)
            
            # 返回实例信息
            serializer = WorkflowInstanceSerializer(instance)
            return Response(serializer.data)
        except Exception as e:
            logger.error(f"启动工作流实例失败: {str(e)}")
            logger.error(traceback.format_exc())
            
            # 更新实例状态
            instance.status = 'failed'
            instance.error = str(e)
            instance.save()
            return Response(
                {"detail": f"启动失败: {str(e)}"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )


class WorkflowInstanceCancelView(APIView):
    """
    取消工作流实例
    """
    permission_classes = [permissions.IsAuthenticated, IsInstanceOwnerOrReadOnly]
    
    def post(self, request, instance_id):
        instance = get_object_or_404(WorkflowInstance, instance_id=instance_id)
        
        # 检查实例状态
        if instance.status not in ['created', 'running', 'paused']:
            return Response(
                {"detail": f"实例状态为 {instance.status}，无法取消"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # 更新实例状态
        instance.status = 'canceled'
        instance.save()
        
        # 实际实现中，应该通知工作流引擎取消执行
        
        return Response(
            {"detail": "工作流实例已取消"},
            status=status.HTTP_200_OK
        )


class WorkflowStepListView(generics.ListAPIView):
    """
    获取工作流实例的步骤列表
    """
    serializer_class = WorkflowStepSerializer
    permission_classes = [permissions.IsAuthenticated, IsInstanceOwnerOrReadOnly]
    
    def get_queryset(self):
        instance_id = self.kwargs.get('instance_id')
        return WorkflowStep.objects.filter(instance__instance_id=instance_id)


# ======== A2A代理视图 ========

class A2AAgentListCreateView(generics.ListCreateAPIView):
    """
    列出所有A2A代理或创建新的A2A代理
    """
    serializer_class = A2AAgentSerializer
    permission_classes = [permissions.IsAuthenticated]
    
    def get_queryset(self):
        user = self.request.user
        if user.is_staff:
            return A2AAgent.objects.all()
        else:
            # 普通用户只能看到自己创建的和公开的代理
            return A2AAgent.objects.filter(created_by=user) | A2AAgent.objects.filter(is_public=True)
    
    def perform_create(self, serializer):
        serializer.save(created_by=self.request.user)


class A2AAgentDetailView(generics.RetrieveUpdateDestroyAPIView):
    """
    查看、更新或删除A2A代理
    """
    serializer_class = A2AAgentSerializer
    permission_classes = [permissions.IsAuthenticated]
    
    def get_queryset(self):
        user = self.request.user
        if user.is_staff:
            return A2AAgent.objects.all()
        else:
            # 普通用户只能操作自己创建的代理
            return A2AAgent.objects.filter(created_by=user)


class A2AAgentRefreshView(APIView):
    """
    刷新A2A代理的Agent Card信息
    """
    permission_classes = [permissions.IsAuthenticated]
    
    def post(self, request, pk):
        agent = get_object_or_404(A2AAgent, pk=pk)
        
        # 检查权限
        if not request.user.is_staff and agent.created_by != request.user:
            return Response(
                {"detail": "您没有权限刷新此代理"},
                status=status.HTTP_403_FORBIDDEN
            )
        
        # 实际实现中，应该从代理的endpoint获取Agent Card
        # 并更新agent.agent_card_content
        try:
            # 假设以下调用A2A客户端模块获取Agent Card
            # agent_card = a2a_client.get_agent_card(agent.endpoint_url)
            # agent.agent_card_content = agent_card
            agent.last_checked_at = timezone.now()
            agent.save()
            
            return Response(
                {"detail": "代理信息已刷新"},
                status=status.HTTP_200_OK
            )
        except Exception as e:
            return Response(
                {"detail": f"刷新失败: {str(e)}"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )


class A2AAgentViewSet(viewsets.ModelViewSet):
    """A2A代理视图集"""
    queryset = A2AAgent.objects.all()
    serializer_class = A2AAgentSerializer
    permission_classes = [permissions.IsAuthenticated]

    def perform_create(self, serializer):
        serializer.save(created_by=self.request.user)

    def get_queryset(self):
        """过滤查询集"""
        queryset = super().get_queryset()
        # 仅管理员可以查看所有代理，普通用户只能查看自己创建的代理
        if not self.request.user.is_staff:
            queryset = queryset.filter(created_by=self.request.user)
        return queryset


class WorkflowViewSet(viewsets.ModelViewSet):
    """工作流视图集"""
    queryset = Workflow.objects.all()
    serializer_class = WorkflowSerializer
    permission_classes = [permissions.IsAuthenticated, IsWorkflowOwnerOrReadOnly]

    def perform_create(self, serializer):
        serializer.save(created_by=self.request.user)

    def get_queryset(self):
        """过滤查询集"""
        queryset = super().get_queryset()
        # 仅管理员可以查看所有工作流，普通用户只能查看自己创建的工作流
        if not self.request.user.is_staff:
            queryset = queryset.filter(created_by=self.request.user)
        return queryset
    
    @action(detail=True, methods=['post'])
    def create_instance(self, request, pk=None):
        """创建工作流实例"""
        workflow = self.get_object()
        
        # 创建工作流实例
        serializer = WorkflowInstanceSerializer(data={
            'workflow': workflow.id,
            'input_data': request.data.get('input_data', {})
        })
        
        if serializer.is_valid():
            with transaction.atomic():
                instance = serializer.save(
                    started_by=request.user,
                    started_at=timezone.now(),
                    status='pending'
                )
                
                # 返回创建的实例
                return Response(
                    WorkflowInstanceSerializer(instance).data,
                    status=status.HTTP_201_CREATED
                )
        
        return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)


class WorkflowStepViewSet(viewsets.ModelViewSet):
    """工作流步骤视图集"""
    queryset = WorkflowStep.objects.all()
    serializer_class = WorkflowStepSerializer
    permission_classes = [permissions.IsAuthenticated]

    def get_queryset(self):
        """过滤查询集"""
        queryset = super().get_queryset()
        workflow_id = self.request.query_params.get('workflow', None)
        
        if workflow_id:
            queryset = queryset.filter(workflow_id=workflow_id)
        
        # 仅管理员可以查看所有步骤，普通用户只能查看自己创建的工作流的步骤
        if not self.request.user.is_staff:
            queryset = queryset.filter(workflow__created_by=self.request.user)
            
        return queryset.order_by('step_number')


class WorkflowInstanceViewSet(viewsets.ModelViewSet):
    """工作流实例视图集"""
    queryset = WorkflowInstance.objects.all()
    serializer_class = WorkflowInstanceSerializer
    permission_classes = [permissions.IsAuthenticated, IsInstanceOwnerOrReadOnly]

    def perform_create(self, serializer):
        serializer.save(started_by=self.request.user)

    def get_queryset(self):
        """过滤查询集"""
        queryset = super().get_queryset()
        # 仅管理员可以查看所有实例，普通用户只能查看自己创建的实例
        if not self.request.user.is_staff:
            queryset = queryset.filter(started_by=self.request.user)
        return queryset
    
    @action(detail=True, methods=['post'])
    def run(self, request, pk=None):
        """运行工作流实例"""
        instance = self.get_object()
        
        # 检查实例状态
        if instance.status not in ['created', 'paused']:
            return Response(
                {"detail": f"实例状态为 {instance.status}，无法启动"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # 使用工作流引擎启动实例
        try:
            execute_workflow(instance.instance_id)
            
            # 返回实例信息
            serializer = WorkflowInstanceSerializer(instance)
            return Response(serializer.data)
        except Exception as e:
            logger.error(f"启动工作流实例失败: {str(e)}")
            logger.error(traceback.format_exc())
            
            # 更新实例状态
            instance.status = 'failed'
            instance.error = str(e)
            instance.save()
            return Response(
                {"detail": f"启动失败: {str(e)}"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )
    
    @action(detail=True, methods=['post'])
    def resume(self, request, pk=None):
        """恢复工作流实例"""
        instance = self.get_object()
        
        # 检查是否提供了任务ID和结果
        task_id = request.data.get('task_id')
        task_result = request.data.get('task_result', {})
        
        if not task_id:
            return Response(
                {"detail": "缺少必要参数: task_id"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # 恢复工作流实例
        try:
            resume_workflow(instance.instance_id, task_id, task_result)
            
            # 返回实例信息
            serializer = WorkflowInstanceSerializer(instance)
            return Response(serializer.data)
        except Exception as e:
            logger.error(f"恢复工作流实例失败: {str(e)}")
            logger.error(traceback.format_exc())
            
            return Response(
                {"detail": f"恢复失败: {str(e)}"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )


class A2ACallbackView(APIView):
    """
    处理A2A任务完成后的回调，恢复被暂停的工作流实例
    """
    permission_classes = [permissions.AllowAny]  # 可以根据需要设置权限
    
    def post(self, request, task_id):
        # 查找任务
        from a2a_client.models import Task
        try:
            task = Task.objects.get(id=task_id)
        except Task.DoesNotExist:
            return Response(
                {"detail": f"找不到ID为{task_id}的任务"},
                status=status.HTTP_404_NOT_FOUND
            )
        
        # 获取任务结果
        task_result = request.data
        
        # 检查任务元数据中是否包含工作流实例ID
        metadata = task.metadata or {}
        workflow_instance_id = metadata.get('workflow_instance_id')
        
        if not workflow_instance_id:
            return Response(
                {"detail": "任务元数据中不包含工作流实例ID"},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # 尝试恢复工作流实例
        try:
            resume_workflow(workflow_instance_id, task_id, task_result)
            return Response(
                {"detail": "工作流实例已恢复执行"},
                status=status.HTTP_200_OK
            )
        except WorkflowInstance.DoesNotExist:
            return Response(
                {"detail": f"找不到ID为{workflow_instance_id}的工作流实例"},
                status=status.HTTP_404_NOT_FOUND
            )
        except Exception as e:
            logger.error(f"恢复工作流实例失败: {str(e)}")
            logger.error(traceback.format_exc())
            return Response(
                {"detail": f"恢复失败: {str(e)}"},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )


class WorkflowStepRetryView(View):
    """
    工作流步骤重试视图
    允许重新执行失败的工作流步骤
    """
    def post(self, request, instance_id, step_id):
        try:
            instance = WorkflowInstance.objects.get(instance_id=instance_id)
            
            # 查找对应的步骤记录
            step = WorkflowStep.objects.get(
                instance=instance,
                step_id=step_id
            )
            
            # 只允许重试失败的步骤
            if step.status != 'failed':
                return JsonResponse({
                    'success': False,
                    'message': f'只有失败的步骤可以重试，当前步骤状态: {step.get_status_display()}'
                }, status=400)
            
            # 重置步骤状态
            step.status = 'pending'
            step.error = None
            step.started_at = None
            step.completed_at = None
            step.save()
            
            # 重置实例到该步骤的位置
            instance.current_step_index = step.step_index
            instance.status = 'running'
            instance.error = None
            
            # 如果实例之前已完成或失败，更新状态
            if instance.status in ['failed', 'completed', 'canceled']:
                instance.completed_at = None
                
            instance.save()
            
            # 异步执行工作流（在实际环境中应使用Celery等异步任务）
            from threading import Thread
            from .engine import execute_workflow
            t = Thread(target=execute_workflow, args=(instance_id,))
            t.daemon = True
            t.start()
            
            return JsonResponse({
                'success': True,
                'message': f'步骤 {step.step_name} 已重新执行'
            })
            
        except WorkflowInstance.DoesNotExist:
            return JsonResponse({
                'success': False,
                'message': '找不到指定的工作流实例'
            }, status=404)
            
        except WorkflowStep.DoesNotExist:
            return JsonResponse({
                'success': False,
                'message': '找不到指定的工作流步骤'
            }, status=404)
            
        except Exception as e:
            return JsonResponse({
                'success': False,
                'message': f'重试步骤时发生错误: {str(e)}'
            }, status=500)


class WorkflowCallbackView(View):
    """工作流回调API，用于处理外部系统的回调"""
    def post(self, request):
        try:
            data = json.loads(request.body)
            
            # 处理回调逻辑
            workflow_instance_id = data.get('workflow_instance_id')
            task_id = data.get('task_id')
            task_result = data.get('result', {})
            
            if not workflow_instance_id or not task_id:
                return JsonResponse(
                    {"detail": "缺少必要的参数"}, 
                    status=400
                )
            
            # 恢复工作流执行
            success = resume_workflow(workflow_instance_id, task_id, task_result)
            
            if success:
                return JsonResponse({"detail": "工作流已恢复执行"})
            else:
                return JsonResponse(
                    {"detail": "恢复工作流失败"}, 
                    status=500
                )
                
        except json.JSONDecodeError:
            return JsonResponse(
                {"detail": "无效的JSON数据"},
                status=400
            )
        except Exception as e:
            logger.error(f"工作流回调处理失败: {str(e)}")
            logger.error(traceback.format_exc())
            return JsonResponse(
                {"detail": f"处理回调时发生错误: {str(e)}"}, 
                status=500
            )
