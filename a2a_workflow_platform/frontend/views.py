from django.shortcuts import render, get_object_or_404, redirect
from django.views.generic import TemplateView, ListView, DetailView, CreateView, UpdateView, DeleteView
from django.contrib.auth.mixins import LoginRequiredMixin
from django.urls import reverse_lazy, reverse
from django.contrib import messages
from django.http import JsonResponse, HttpResponseRedirect
from django.views.decorators.http import require_POST
from django.views import View
from django.utils.decorators import method_decorator
from django.contrib.auth.decorators import login_required
from django.utils import timezone
from django.db import transaction
import json
from django.db.models import Q
import uuid
import requests
import time
import logging
from django.views.decorators.csrf import ensure_csrf_cookie

from a2a_client.models import Agent, AgentCredential, Task, Message, Part, Artifact, Session
from workflow.models import Workflow, WorkflowInstance, WorkflowStep
from workflow.engine import execute_workflow, start_workflow_execution

# 获取日志记录器
logger = logging.getLogger(__name__)

# 主页视图
class HomeView(TemplateView):
    # 重定向到Vue应用入口
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

# Agent相关视图
class AgentListView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class AgentDetailView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class AgentCreateView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class AgentUpdateView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class AgentDeleteView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

@method_decorator(login_required, name='dispatch')
class AgentTestView(View):
    def get(self, request, pk):
        return redirect('frontend:vue_app')

class CredentialUpdateView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

# 工作流相关视图
class WorkflowListView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowCreateView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowDetailView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowUpdateView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowDeleteView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowStartInstanceView(LoginRequiredMixin, View):
    """
    创建并启动工作流实例
    """
    def post(self, request, pk):
        workflow = get_object_or_404(Workflow, pk=pk)
        name = request.POST.get('name', f"{workflow.name} 实例")
        start_now = request.POST.get('start_now') == 'on'
        
        # 创建工作流实例
        instance = WorkflowInstance.objects.create(
            workflow=workflow,
            created_by=request.user,
            name=name,
            status='created',
            context={}
        )
        
        # 如果选择立即启动
        if start_now:
            instance.status = 'running'
            instance.started_at = timezone.now()
            instance.save()
            
            # 启动异步任务执行工作流
            transaction.on_commit(lambda: start_workflow_execution.delay(str(instance.instance_id)))
            messages.success(request, f"工作流实例 {name} 已创建并启动")
        else:
            messages.success(request, f"工作流实例 {name} 已创建")
        
        return redirect('frontend:workflow_instance_detail', instance_id=instance.instance_id)

# 用户资料视图
class UserProfileView(LoginRequiredMixin, View):
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

@method_decorator(ensure_csrf_cookie, name='dispatch')
class VueAppView(TemplateView):
    """
    提供Vue单页应用的视图
    为所有路由提供同一个模板，让前端路由处理导航
    """
    template_name = 'frontend/vue_app.html'
    
    def get_context_data(self, **kwargs):
        context = super().get_context_data(**kwargs)
        # 将DEBUG设置传递给模板
        from django.conf import settings
        context['debug'] = settings.DEBUG
        return context

class IndexView(LoginRequiredMixin, View):
    """旧版首页视图，重定向到Vue应用"""
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowEditorView(LoginRequiredMixin, View):
    """旧版工作流编辑器视图，重定向到Vue应用"""
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowInstanceView(LoginRequiredMixin, View):
    """旧版工作流实例视图，重定向到Vue应用"""
    def get(self, request, *args, **kwargs):
        return redirect('frontend:vue_app')

class WorkflowSaveView(LoginRequiredMixin, View):
    """保存工作流API"""
    
    def post(self, request, pk=None):
        """创建新工作流"""
        try:
            logger.info("正在通过API创建新工作流")
            # 解析工作流数据
            name = request.POST.get('name')
            description = request.POST.get('description', '')
            workflow_type = request.POST.get('workflow_type', 'general')
            is_public = request.POST.get('is_public', 'false').lower() == 'true'
            definition_str = request.POST.get('definition', '{}')
            
            logger.debug(f"工作流数据: name={name}, type={workflow_type}, public={is_public}")
            logger.debug(f"工作流定义: {definition_str[:100]}...")
            
            # 基本验证
            if not name:
                logger.warning("工作流名称为空")
                return JsonResponse({'error': '工作流名称不能为空'}, status=400)
            
            try:
                definition = json.loads(definition_str)
            except json.JSONDecodeError as e:
                logger.error(f"工作流定义JSON解析错误: {str(e)}")
                return JsonResponse({'error': f'无效的工作流定义格式: {str(e)}'}, status=400)
            
            workflow = Workflow.objects.create(
                name=name,
                description=description,
                workflow_type=workflow_type,
                is_public=is_public,
                definition=definition,
                created_by=request.user
            )
            
            logger.info(f"工作流API创建成功: ID={workflow.id}, 名称={workflow.name}")
            return JsonResponse({
                'id': workflow.id,
                'message': '工作流创建成功'
            })
            
        except json.JSONDecodeError as e:
            logger.error(f"工作流定义JSON解析错误: {str(e)}")
            return JsonResponse({'error': f'无效的工作流定义格式: {str(e)}'}, status=400)
        except Exception as e:
            logger.error(f"工作流API创建失败: {str(e)}", exc_info=True)
            return JsonResponse({'error': f'保存工作流失败: {str(e)}'}, status=500)
    
    def put(self, request, pk):
        """更新现有工作流"""
        try:
            logger.info(f"正在通过API更新工作流: ID={pk}")
            # 获取现有工作流
            workflow = get_object_or_404(Workflow, pk=pk)
            
            # 检查权限
            if workflow.created_by != request.user and not request.user.is_staff:
                logger.warning(f"用户 {request.user.username} 尝试编辑无权限的工作流: ID={pk}")
                return JsonResponse({'error': '您没有权限编辑此工作流'}, status=403)
            
            # 解析请求数据
            try:
                data = json.loads(request.body)
                logger.debug(f"更新数据: {data}")
            except json.JSONDecodeError as e:
                logger.error(f"请求体JSON解析错误: {str(e)}")
                return JsonResponse({'error': f'无效的请求格式: {str(e)}'}, status=400)
            
            # 更新工作流
            workflow.name = data.get('name', workflow.name)
            workflow.description = data.get('description', workflow.description)
            workflow.workflow_type = data.get('workflow_type', workflow.workflow_type)
            workflow.is_public = data.get('is_public', workflow.is_public)
            
            if 'definition' in data:
                workflow.definition = data.get('definition')
            
            workflow.save()
            logger.info(f"工作流API更新成功: ID={workflow.id}, 名称={workflow.name}")
            
            return JsonResponse({
                'id': workflow.id,
                'message': '工作流更新成功'
            })
            
        except json.JSONDecodeError as e:
            logger.error(f"工作流API更新JSON解析错误: {str(e)}")
            return JsonResponse({'error': f'无效的工作流定义格式: {str(e)}'}, status=400)
        except Exception as e:
            logger.error(f"工作流API更新失败: {str(e)}", exc_info=True)
            return JsonResponse({'error': f'更新工作流失败: {str(e)}'}, status=500)

class WorkflowInstanceDetailView(LoginRequiredMixin, DetailView):
    """工作流实例详情页面"""
    model = WorkflowInstance
    template_name = 'frontend/workflow_instance_detail.html'
    context_object_name = 'instance'
    slug_field = 'instance_id'
    slug_url_kwarg = 'instance_id'
    
    def get_queryset(self):
        return WorkflowInstance.objects.filter(created_by=self.request.user)
    
    def get_context_data(self, **kwargs):
        context = super().get_context_data(**kwargs)
        
        # 获取步骤列表
        steps = WorkflowStep.objects.filter(instance=self.object).order_by('step_index')
        context['steps'] = steps
        
        # 计算进度
        total_steps = len(self.object.workflow.definition.get('steps', []))
        completed_steps = steps.filter(status='completed').count()
        context['total_steps'] = total_steps
        context['completed_steps'] = completed_steps
        
        # 计算进度百分比
        if total_steps > 0:
            progress_percentage = int((completed_steps / total_steps) * 100)
        else:
            progress_percentage = 0
        context['progress_percentage'] = progress_percentage
        
        # 获取执行日志
        logs = self.object.logs.all().order_by('-timestamp')[:100]  # 限制最近的100条日志
        context['logs'] = logs
        
        # 格式化JSON数据
        if self.object.context:
            import json
            context['context_json'] = json.dumps(self.object.context, indent=2, ensure_ascii=False)
        
        if self.object.output:
            import json
            context['output_json'] = json.dumps(self.object.output, indent=2, ensure_ascii=False)
        
        # 格式化步骤参数和输出数据
        for step in steps:
            if hasattr(step, 'parameters') and step.parameters:
                import json
                step.parameters_display = json.dumps(step.parameters, indent=2, ensure_ascii=False)
            else:
                step.parameters_display = "{}"
                
            if hasattr(step, 'output_data') and step.output_data:
                import json
                step.output_data_display = json.dumps(step.output_data, indent=2, ensure_ascii=False)
        
        return context

class WorkflowInstanceStartView(LoginRequiredMixin, View):
    """启动工作流实例"""
    def post(self, request, instance_id):
        try:
            instance = WorkflowInstance.objects.get(instance_id=instance_id, created_by=request.user)
            
            if instance.status in ['created', 'paused']:
                from workflow.engine import execute_workflow
                
                # 异步执行工作流（在实际环境中应使用Celery等异步任务）
                instance.status = 'running'
                instance.started_at = timezone.now()
                instance.save()
                
                # 这里简单起见使用线程执行，实际应该使用Celery
                import threading
                t = threading.Thread(target=execute_workflow, args=(instance_id,))
                t.daemon = True
                t.start()
                
                messages.success(request, f"工作流实例 {instance.display_name} 已开始执行")
            else:
                messages.error(request, f"工作流实例状态为 {instance.get_status_display()}，无法启动")
                
            return redirect('frontend:workflow_instance_detail', instance_id=instance_id)
        except WorkflowInstance.DoesNotExist:
            messages.error(request, "找不到指定的工作流实例")
            return redirect('frontend:workflow_list')

class WorkflowInstancePauseView(LoginRequiredMixin, View):
    """暂停工作流实例"""
    def post(self, request, instance_id):
        try:
            instance = WorkflowInstance.objects.get(instance_id=instance_id, created_by=request.user)
            
            if instance.status == 'running':
                instance.status = 'paused'
                instance.save()
                messages.success(request, f"工作流实例 {instance.display_name} 已暂停")
            else:
                messages.error(request, f"工作流实例状态为 {instance.get_status_display()}，无法暂停")
                
            return redirect('frontend:workflow_instance_detail', instance_id=instance_id)
        except WorkflowInstance.DoesNotExist:
            messages.error(request, "找不到指定的工作流实例")
            return redirect('frontend:workflow_list')

class WorkflowInstanceCancelView(LoginRequiredMixin, View):
    """取消工作流实例"""
    def post(self, request, instance_id):
        try:
            instance = WorkflowInstance.objects.get(instance_id=instance_id, created_by=request.user)
            
            if instance.status in ['running', 'paused']:
                instance.status = 'canceled'
                instance.save()
                messages.success(request, f"工作流实例 {instance.display_name} 已取消")
            else:
                messages.error(request, f"工作流实例状态为 {instance.get_status_display()}，无法取消")
                
            return redirect('frontend:workflow_instance_detail', instance_id=instance_id)
        except WorkflowInstance.DoesNotExist:
            messages.error(request, "找不到指定的工作流实例")
            return redirect('frontend:workflow_list')

class WorkflowInstanceCloneView(LoginRequiredMixin, View):
    """克隆工作流实例"""
    def post(self, request, instance_id):
        try:
            instance = WorkflowInstance.objects.get(instance_id=instance_id, created_by=request.user)
            
            name = request.POST.get('name', f"{instance.name} - 克隆")
            copy_context = request.POST.get('copy_context') == 'on'
            
            # 创建新实例
            new_instance = WorkflowInstance.objects.create(
                workflow=instance.workflow,
                name=name,
                created_by=request.user,
                context=instance.context if copy_context else None,
                status='created'
            )
            
            messages.success(request, f"工作流实例 {instance.display_name} 已克隆")
            return redirect('frontend:workflow_instance_detail', instance_id=new_instance.instance_id)
        except WorkflowInstance.DoesNotExist:
            messages.error(request, "找不到指定的工作流实例")
            return redirect('frontend:workflow_list')

class WorkflowInstanceListView(LoginRequiredMixin, ListView):
    """
    工作流实例列表页面
    """
    model = WorkflowInstance
    template_name = 'frontend/workflow_instance_list.html'
    context_object_name = 'instances'
    paginate_by = 10
    
    def get_queryset(self):
        queryset = WorkflowInstance.objects.filter(created_by=self.request.user).order_by('-created_at')
        
        # 应用过滤条件
        search = self.request.GET.get('search', '')
        workflow_id = self.request.GET.get('workflow', '')
        status = self.request.GET.get('status', '')
        
        if search:
            queryset = queryset.filter(
                Q(name__icontains=search) | 
                Q(instance_id__icontains=search)
            )
        
        if workflow_id:
            queryset = queryset.filter(workflow_id=workflow_id)
            
        if status:
            queryset = queryset.filter(status=status)
            
        return queryset
    
    def get_context_data(self, **kwargs):
        context = super().get_context_data(**kwargs)
        # 获取所有工作流列表用于过滤选择
        context['workflows'] = Workflow.objects.filter(
            Q(created_by=self.request.user) | Q(is_public=True)
        ).order_by('name')
        return context

# 添加A2A任务视图
class TaskListView(LoginRequiredMixin, ListView):
    """
    A2A任务列表视图
    显示用户拥有的Agent的所有任务
    """
    model = Task
    template_name = 'frontend/task_list.html'
    context_object_name = 'tasks'
    paginate_by = 10
    
    def get_queryset(self):
        """获取当前用户的所有Agent的任务，支持筛选"""
        user = self.request.user
        user_agents = Agent.objects.filter(owner=user)
        queryset = Task.objects.filter(agent__in=user_agents)
        
        # 筛选条件
        agent_id = self.request.GET.get('agent')
        state = self.request.GET.get('state')
        date_filter = self.request.GET.get('date')
        
        # 按代理筛选
        if agent_id:
            queryset = queryset.filter(agent_id=agent_id)
        
        # 按状态筛选
        if state:
            queryset = queryset.filter(state=state)
        
        # 按日期筛选
        if date_filter:
            today = timezone.now().date()
            if date_filter == 'today':
                queryset = queryset.filter(created_at__date=today)
            elif date_filter == 'yesterday':
                yesterday = today - timezone.timedelta(days=1)
                queryset = queryset.filter(created_at__date=yesterday)
            elif date_filter == 'week':
                week_ago = today - timezone.timedelta(days=7)
                queryset = queryset.filter(created_at__date__gte=week_ago)
            elif date_filter == 'month':
                month_ago = today - timezone.timedelta(days=30)
                queryset = queryset.filter(created_at__date__gte=month_ago)
        
        return queryset.order_by('-created_at')
    
    def get_context_data(self, **kwargs):
        context = super().get_context_data(**kwargs)
        context['agents'] = Agent.objects.filter(owner=self.request.user)
        return context


class TaskDetailView(LoginRequiredMixin, DetailView):
    """
    A2A任务详情视图
    显示任务的详细信息，包括消息历史和产物
    """
    model = Task
    template_name = 'frontend/task_detail.html'
    context_object_name = 'task'
    pk_url_kwarg = 'task_id'
    
    def get_queryset(self):
        """确保用户只能查看自己Agent的任务"""
        user = self.request.user
        user_agents = Agent.objects.filter(owner=user)
        return Task.objects.filter(agent__in=user_agents)
    
    def get_context_data(self, **kwargs):
        context = super().get_context_data(**kwargs)
        task = self.object
        
        # 获取消息历史
        messages = Message.objects.filter(task=task).order_by('created_at')
        message_data = []
        
        for msg in messages:
            parts = Part.objects.filter(message=msg).order_by('created_at')
            parts_data = []
            
            for part in parts:
                part_info = {
                    'type': part.get_part_type_display(),
                    'content_type': part.content_type,
                }
                
                if part.part_type == 'text':
                    part_info['content'] = part.text_content
                elif part.part_type == 'data':
                    part_info['content'] = part.data_content
                elif part.part_type == 'file':
                    if part.file_uri:
                        part_info['file_uri'] = part.file_uri
                    else:
                        part_info['content'] = '二进制文件内容'
                
                parts_data.append(part_info)
            
            message_data.append({
                'id': msg.id,
                'role': msg.get_role_display(),
                'created_at': msg.created_at,
                'parts': parts_data
            })
        
        # 获取产物
        artifacts = Artifact.objects.filter(task=task).order_by('created_at')
        
        context['messages'] = message_data
        context['artifacts'] = artifacts
        return context

class TaskCreateView(LoginRequiredMixin, View):
    """
    创建新的A2A任务视图
    允许用户选择Agent并发送消息来创建新任务
    """
    template_name = 'frontend/task_create.html'
    
    def get(self, request):
        # 获取用户的所有Agent
        agents = Agent.objects.filter(owner=request.user, is_active=True)
        return render(request, self.template_name, {'agents': agents})
    
    def post(self, request):
        try:
            # 获取表单数据
            agent_id = request.POST.get('agent')
            message = request.POST.get('message')
            json_data = request.POST.get('data', '')
            uploaded_file = request.FILES.get('file')
            
            # 验证数据
            if not agent_id or not message:
                messages.error(request, '代理和消息内容不能为空')
                return redirect('frontend:task_create')
            
            # 获取Agent
            agent = get_object_or_404(Agent, id=agent_id, owner=request.user)
            
            # 创建任务ID
            task_id = uuid.uuid4()
            
            # 准备消息部分
            message_parts = [
                {
                    'text': message,
                    'contentType': 'text/plain'
                }
            ]
            
            # 处理结构化数据
            if json_data:
                try:
                    data_obj = json.loads(json_data)
                    message_parts.append({
                        'data': data_obj,
                        'contentType': 'application/json'
                    })
                except json.JSONDecodeError:
                    messages.error(request, '结构化数据必须是有效的JSON格式')
                    return redirect('frontend:task_create')
            
            # 处理上传的文件
            if uploaded_file:
                # 存储上传的文件
                import base64
                import os
                from django.conf import settings
                
                # 确保上传目录存在
                upload_dir = os.path.join(settings.MEDIA_ROOT, 'task_uploads')
                os.makedirs(upload_dir, exist_ok=True)
                
                # 生成唯一文件名
                file_ext = os.path.splitext(uploaded_file.name)[1]
                unique_filename = f"{task_id}{file_ext}"
                file_path = os.path.join(upload_dir, unique_filename)
                
                # 保存文件
                with open(file_path, 'wb+') as destination:
                    for chunk in uploaded_file.chunks():
                        destination.write(chunk)
                
                # 对于小文件，直接使用内联数据
                if uploaded_file.size <= 1024 * 1024:  # 小于1MB
                    with open(file_path, 'rb') as f:
                        file_data = f.read()
                        encoded_data = base64.b64encode(file_data).decode('utf-8')
                        
                        message_parts.append({
                            'inlineData': encoded_data,
                            'contentType': uploaded_file.content_type or 'application/octet-stream'
                        })
                else:
                    # 对于大文件，使用文件URL
                    # 注意：在实际生产环境中，应该使用适当的URL生成方法
                    file_url = f"{request.scheme}://{request.get_host()}/media/task_uploads/{unique_filename}"
                    
                    message_parts.append({
                        'fileUri': file_url,
                        'contentType': uploaded_file.content_type or 'application/octet-stream'
                    })
            
            # 发送A2A任务请求
            response = requests.post(
                f"{request.scheme}://{request.get_host()}/api/a2a/tasks/send",
                headers={
                    'Content-Type': 'application/json',
                    'X-CSRFToken': request.COOKIES.get('csrftoken')
                },
                json={
                    'jsonrpc': '2.0',
                    'method': 'tasks/send',
                    'params': {
                        'taskId': str(task_id),
                        'agentId': str(agent.id),
                        'message': {
                            'role': 'user',
                            'parts': message_parts
                        }
                    },
                    'id': int(time.time())
                },
                cookies=request.COOKIES
            )
            
            # 处理响应
            if response.status_code == 200:
                data = response.json()
                if 'result' in data:
                    messages.success(request, '任务创建成功')
                    return redirect('frontend:task_detail', task_id=task_id)
                elif 'error' in data:
                    messages.error(request, f'创建任务失败: {data["error"]["message"]}')
                    return redirect('frontend:task_create')
            else:
                messages.error(request, f'请求失败: {response.status_code}')
                return redirect('frontend:task_create')
        
        except Exception as e:
            messages.error(request, f'发生错误: {str(e)}')
            return redirect('frontend:task_create')

class TaskStreamView(LoginRequiredMixin, View):
    """
    创建支持流式响应的任务视图
    使用SSE(Server-Sent Events)实现实时对话
    """
    template_name = 'frontend/task_create_stream.html'
    
    def get(self, request):
        # 获取用户的所有Agent
        agents = Agent.objects.filter(owner=request.user, is_active=True)
        return render(request, self.template_name, {'agents': agents})

class SessionListView(LoginRequiredMixin, ListView):
    """会话列表视图"""
    model = Session
    template_name = 'frontend/session_list.html'
    context_object_name = 'sessions'
    paginate_by = 10
    
    def get_queryset(self):
        """过滤当前用户的会话"""
        queryset = Session.objects.filter(owner=self.request.user)
        
        # 过滤条件
        agent_id = self.request.GET.get('agent')
        status = self.request.GET.get('status')
        sort = self.request.GET.get('sort', 'latest')
        
        # 按代理过滤
        if agent_id:
            queryset = queryset.filter(agent_id=agent_id)
        
        # 按状态过滤
        if status:
            if status == 'empty':
                # 空会话没有任务
                no_tasks_sessions = [s.id for s in queryset if s.get_task_count() == 0]
                queryset = queryset.filter(id__in=no_tasks_sessions)
            else:
                # 查找最新任务状态匹配的会话
                sessions_with_status = []
                for session in queryset:
                    latest_task = session.get_latest_task()
                    if latest_task and latest_task.state == status:
                        sessions_with_status.append(session.id)
                queryset = queryset.filter(id__in=sessions_with_status)
        
        # 排序
        if sort == 'oldest':
            queryset = queryset.order_by('created_at')
        else:
            queryset = queryset.order_by('-updated_at')
        
        return queryset
    
    def get_context_data(self, **kwargs):
        """添加额外上下文数据"""
        context = super().get_context_data(**kwargs)
        # 添加用户的代理列表，用于过滤
        context['user_agents'] = Agent.objects.filter(owner=self.request.user)
        return context

class SessionDetailView(LoginRequiredMixin, DetailView):
    """会话详情视图"""
    model = Session
    template_name = 'frontend/session_detail.html'
    context_object_name = 'session'
    
    def get_queryset(self):
        """只允许用户查看自己的会话"""
        return Session.objects.filter(owner=self.request.user)
    
    def get_context_data(self, **kwargs):
        """添加会话任务到上下文"""
        context = super().get_context_data(**kwargs)
        session = self.get_object()
        # 获取会话中的所有任务，按时间排序
        context['tasks'] = session.tasks.all().order_by('created_at')
        return context

class SessionCreateView(LoginRequiredMixin, CreateView):
    """创建会话视图"""
    model = Session
    template_name = 'frontend/session_form.html'
    fields = ['agent', 'name', 'metadata']
    success_url = reverse_lazy('frontend:session_list')
    
    def get_form(self, form_class=None):
        """自定义表单"""
        form = super().get_form(form_class)
        # 限制只能选择当前用户的Agent
        form.fields['agent'].queryset = Agent.objects.filter(owner=self.request.user)
        return form
    
    def form_valid(self, form):
        """设置会话所有者为当前用户"""
        form.instance.owner = self.request.user
        messages.success(self.request, '会话创建成功！')
        return super().form_valid(form)

class SessionUpdateView(LoginRequiredMixin, UpdateView):
    """更新会话视图"""
    model = Session
    template_name = 'frontend/session_form.html'
    fields = ['name', 'metadata']
    context_object_name = 'session'
    
    def get_queryset(self):
        """只允许用户编辑自己的会话"""
        return Session.objects.filter(owner=self.request.user)
    
    def get_success_url(self):
        """返回到会话详情页"""
        return reverse('frontend:session_detail', kwargs={'pk': self.object.pk})
    
    def form_valid(self, form):
        """更新成功消息"""
        messages.success(self.request, '会话更新成功！')
        return super().form_valid(form)

class SessionDeleteView(LoginRequiredMixin, DeleteView):
    """删除会话视图"""
    model = Session
    template_name = 'frontend/session_confirm_delete.html'
    success_url = reverse_lazy('frontend:session_list')
    context_object_name = 'session'
    
    def get_queryset(self):
        """只允许用户删除自己的会话"""
        return Session.objects.filter(owner=self.request.user)
    
    def delete(self, request, *args, **kwargs):
        """自定义删除消息"""
        messages.success(self.request, '会话已删除！')
        return super().delete(request, *args, **kwargs)

class SessionSendMessageView(LoginRequiredMixin, View):
    """在会话中发送新消息"""
    
    def post(self, request, pk):
        """处理消息发送请求"""
        try:
            # 获取会话
            session = get_object_or_404(Session, id=pk, owner=request.user)
            
            # 获取表单数据
            message = request.POST.get('message')
            json_data = request.POST.get('data', '')
            uploaded_file = request.FILES.get('file')
            
            # 验证数据
            if not message:
                messages.error(request, '消息内容不能为空')
                return redirect('frontend:session_detail', pk=pk)
            
            # 创建任务ID
            task_id = uuid.uuid4()
            
            # 准备消息部分
            message_parts = [
                {
                    'text': message,
                    'contentType': 'text/plain'
                }
            ]
            
            # 处理结构化数据
            if json_data:
                try:
                    data_obj = json.loads(json_data)
                    message_parts.append({
                        'data': data_obj,
                        'contentType': 'application/json'
                    })
                except json.JSONDecodeError:
                    messages.error(request, '结构化数据必须是有效的JSON格式')
                    return redirect('frontend:session_detail', pk=pk)
            
            # 处理上传的文件
            if uploaded_file:
                # 存储上传的文件
                import base64
                import os
                from django.conf import settings
                
                # 确保上传目录存在
                upload_dir = os.path.join(settings.MEDIA_ROOT, 'task_uploads')
                os.makedirs(upload_dir, exist_ok=True)
                
                # 生成唯一文件名
                file_ext = os.path.splitext(uploaded_file.name)[1]
                unique_filename = f"{task_id}{file_ext}"
                file_path = os.path.join(upload_dir, unique_filename)
                
                # 保存文件
                with open(file_path, 'wb+') as destination:
                    for chunk in uploaded_file.chunks():
                        destination.write(chunk)
                
                # 对于小文件，直接使用内联数据
                if uploaded_file.size <= 1024 * 1024:  # 小于1MB
                    with open(file_path, 'rb') as f:
                        file_data = f.read()
                        encoded_data = base64.b64encode(file_data).decode('utf-8')
                        
                        message_parts.append({
                            'inlineData': encoded_data,
                            'contentType': uploaded_file.content_type or 'application/octet-stream'
                        })
                else:
                    # 对于大文件，使用文件URL
                    file_url = f"{request.scheme}://{request.get_host()}/media/task_uploads/{unique_filename}"
                    
                    message_parts.append({
                        'fileUri': file_url,
                        'contentType': uploaded_file.content_type or 'application/octet-stream'
                    })
            
            # 发送A2A任务请求
            response = requests.post(
                f"{request.scheme}://{request.get_host()}/api/a2a/tasks/send",
                headers={
                    'Content-Type': 'application/json',
                    'X-CSRFToken': request.COOKIES.get('csrftoken')
                },
                json={
                    'jsonrpc': '2.0',
                    'method': 'tasks/send',
                    'params': {
                        'taskId': str(task_id),
                        'agentId': str(session.agent.id),
                        'sessionId': str(session.id),
                        'message': {
                            'role': 'user',
                            'parts': message_parts
                        }
                    },
                    'id': int(time.time())
                },
                cookies=request.COOKIES
            )
            
            # 处理响应
            if response.status_code == 200:
                data = response.json()
                if 'result' in data:
                    # 关联任务到会话
                    task = Task.objects.get(id=task_id)
                    task.session = session
                    task.save()
                    
                    # 更新会话修改时间
                    session.save()
                    
                    messages.success(request, '消息发送成功')
                elif 'error' in data:
                    messages.error(request, f'发送消息失败: {data["error"]["message"]}')
            else:
                messages.error(request, f'请求失败: {response.status_code}')
            
            return redirect('frontend:session_detail', pk=pk)
            
        except Exception as e:
            messages.error(request, f'发生错误: {str(e)}')
            return redirect('frontend:session_detail', pk=pk)

class WorkflowStepRetryView(LoginRequiredMixin, View):
    """
    重试工作流步骤
    允许重新执行失败的工作流步骤
    """
    def post(self, request, instance_id, step_id):
        try:
            instance = WorkflowInstance.objects.get(
                instance_id=instance_id, 
                created_by=request.user
            )
            
            # 查找对应的步骤记录
            step = WorkflowStep.objects.get(
                instance=instance,
                step_id=step_id
            )
            
            # 只允许重试失败的步骤
            if step.status != 'failed':
                messages.error(
                    request, 
                    f'只有失败的步骤可以重试，当前步骤状态: {step.get_status_display()}'
                )
                return redirect('frontend:workflow_instance_detail', instance_id=instance_id)
            
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
            
            # 异步执行工作流
            import threading
            from workflow.engine import execute_workflow
            t = threading.Thread(target=execute_workflow, args=(instance_id,))
            t.daemon = True
            t.start()
            
            messages.success(request, f'步骤 "{step.step_name}" 已重新执行')
            
            return redirect('frontend:workflow_instance_detail', instance_id=instance_id)
            
        except WorkflowInstance.DoesNotExist:
            messages.error(request, "找不到指定的工作流实例")
            return redirect('frontend:workflow_instance_list')
            
        except WorkflowStep.DoesNotExist:
            messages.error(request, "找不到指定的工作流步骤")
            return redirect('frontend:workflow_instance_detail', instance_id=instance_id)
            
        except Exception as e:
            messages.error(request, f"重试步骤时发生错误: {str(e)}")
            return redirect('frontend:workflow_instance_detail', instance_id=instance_id) 