from django.shortcuts import render, get_object_or_404
from rest_framework import viewsets, status, permissions
from rest_framework.decorators import action
from rest_framework.response import Response
from rest_framework.views import APIView
from .models import Agent, AgentCredential, AgentSkill, Task, Session
from .serializers import (
    AgentSerializer, 
    AgentCredentialSerializer, 
    AgentCreateSerializer,
    AgentSkillSerializer,
    PushNotificationConfigSerializer,
    TaskSerializer,
    SessionSerializer
)
from django.conf import settings
import requests
import json
import uuid
from django.db import models
import logging # Added import
from . import agent_handlers # Added import for handlers

# Import the default tool manager
# Correct import for default_tool_manager from the agents.tools package __init__
from agents.tools import default_tool_manager

logger = logging.getLogger(__name__) # Added logger

# Create your views here.

class IsOwnerOrReadOnly(permissions.BasePermission):
    """
    自定义权限：只允许对象的所有者编辑它
    """
    def has_object_permission(self, request, view, obj):
        # 读取权限允许任何请求
        if request.method in permissions.SAFE_METHODS:
            return True
        
        # 写入权限只允许对象的所有者
        return obj.owner == request.user


class AgentCardView(APIView):
    """
    提供符合A2A协议规范的Agent Card
    端点：/.well-known/agent.json
    """
    permission_classes = [permissions.AllowAny]  # Agent Card通常是公开访问的
    
    def get(self, request, agent_id=None):
        """获取指定Agent的Agent Card"""
        if agent_id:
            agent = get_object_or_404(Agent, id=agent_id)
        else:
            # 如果没有指定agent_id，返回平台默认的Agent Card
            return Response(self.get_platform_agent_card())
        
        # 构建符合A2A协议的Agent Card
        agent_card = {
            "name": agent.name,
            "description": agent.description,
            "url": f"{settings.BASE_URL}/api/a2a/agents/{agent.id}/tasks",
            "version": "1.0.0",
            "capabilities": {
                "streaming": True,
                "pushNotifications": False,
                "stateTransitionHistory": True
            },
            "authentication": {
                "schemes": ["apiKey"]  # 支持apiKey认证
            },
            "defaultInputModes": ["text"],
            "defaultOutputModes": ["text"],
            "skills": [
                {
                    "id": f"{agent.agent_type}_{agent.id}",
                    "name": agent.name,
                    "description": agent.description or f"{agent.get_agent_type_display()}类型的AI助手",
                    "inputModes": ["text"],
                    "outputModes": ["text"],
                    "examples": ["您好，我需要帮助", "请介绍一下你的功能"]
                }
            ]
        }
        
        return Response(agent_card)
    
    def get_platform_agent_card(self):
        """获取平台的默认Agent Card"""
        return {
            "name": "A2A工作流平台",
            "description": "一个支持Agent-to-Agent协议的工作流平台",
            "url": f"{settings.BASE_URL}/api/a2a",
            "version": "1.0.0",
            "capabilities": {
                "streaming": True,
                "pushNotifications": False,
                "stateTransitionHistory": True
            },
            "authentication": {
                "schemes": ["apiKey", "oauth2"]
            },
            "defaultInputModes": ["text"],
            "defaultOutputModes": ["text"],
            "skills": [
                {
                    "id": "workflow_execution",
                    "name": "工作流执行",
                    "description": "执行预定义的AI工作流",
                    "inputModes": ["text"],
                    "outputModes": ["text"],
                    "examples": ["执行销售分析工作流", "启动客户服务流程"]
                },
                {
                    "id": "agent_discovery",
                    "name": "代理发现",
                    "description": "查找并连接可用的AI代理",
                    "inputModes": ["text"],
                    "outputModes": ["text"],
                    "examples": ["查找所有翻译代理", "连接到客户服务代理"]
                }
            ]
        }


class AgentViewSet(viewsets.ModelViewSet):
    """
    Agent视图集，提供完整的CRUD操作
    """
    serializer_class = AgentSerializer
    permission_classes = [permissions.IsAuthenticated, IsOwnerOrReadOnly]
    
    def get_queryset(self):
        """
        根据当前用户过滤Agents
        """
        user = self.request.user
        return Agent.objects.filter(owner=user)
    
    def get_serializer_class(self):
        """
        根据操作返回不同的序列化器
        """
        if self.action == 'create':
            return AgentCreateSerializer
        if self.action in ['list_tasks', 'create_task']:
            return TaskSerializer
        return self.serializer_class
    
    @action(detail=True, methods=['post'])
    def test_connection(self, request, pk=None):
        """
        测试与Agent的连接
        """
        agent = self.get_object()
        credential = get_object_or_404(AgentCredential, agent=agent)
        
        # 根据不同的Agent类型构建不同的测试请求
        api_key = credential.get_api_key()
        endpoint = credential.api_endpoint
        
        try:
            headers = {
                'Content-Type': 'application/json',
                'Authorization': f'Bearer {api_key}'
            }
            
            # 构建简单的测试请求内容
            if agent.agent_type in ['gpt-3.5', 'gpt-4']:
                # OpenAI格式
                payload = {
                    'model': agent.model_name,
                    'messages': [{'role': 'user', 'content': 'Hello'}],
                    'max_tokens': 10
                }
            elif agent.agent_type == 'claude-3':
                # Anthropic格式
                payload = {
                    'model': agent.model_name,
                    'messages': [{'role': 'user', 'content': 'Hello'}],
                    'max_tokens': 10
                }
            elif agent.agent_type == 'gemini':
                # Google格式
                payload = {
                    'contents': [{'role': 'user', 'parts': [{'text': 'Hello'}]}],
                    'generationConfig': {'maxOutputTokens': 10}
                }
            else:
                # 自定义格式，使用附加参数
                payload = credential.additional_params.get('test_payload', {})
                if not payload:
                    return Response(
                        {'error': '自定义Agent需要在additional_params中提供test_payload'},
                        status=status.HTTP_400_BAD_REQUEST
                    )
            
            # 发送测试请求
            response = requests.post(
                endpoint,
                headers=headers,
                data=json.dumps(payload),
                timeout=10
            )
            
            # 检查响应
            if response.status_code >= 200 and response.status_code < 300:
                return Response({
                    'status': 'success',
                    'message': '连接测试成功',
                    'response': response.json()
                })
            else:
                return Response({
                    'status': 'error',
                    'message': '连接测试失败',
                    'response_code': response.status_code,
                    'response_text': response.text
                }, status=status.HTTP_400_BAD_REQUEST)
                
        except Exception as e:
            return Response({
                'status': 'error',
                'message': f'连接测试出错: {str(e)}'
            }, status=status.HTTP_500_INTERNAL_SERVER_ERROR)
    
    @action(detail=True, methods=['post'])
    def send_message(self, request, pk=None):
        """
        向Agent发送消息 (已修改以实现完整的工具调用循环)
        """
        agent = self.get_object() # type: Agent
        credential = get_object_or_404(AgentCredential, agent=agent)
        
        user_message_content = request.data.get('message', '')
        if not user_message_content:
            return Response(
                {'error': '消息不能为空'},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        api_key = credential.get_api_key()
        # The 'endpoint' from credential might be just a base URL for some APIs (like Gemini)
        # It will be adjusted per API type if needed.
        base_endpoint = credential.api_endpoint 
        
        MAX_TOOL_CALL_ITERATIONS = 3 # 最大工具调用迭代次数
        tool_call_iterations = 0
        
        # 初始化请求参数 - 通用headers
        common_headers = {
            'Content-Type': 'application/json',
        }
        # Specific auth headers (like Bearer token) will be added per API type if not part of common_headers
        # Merge additional_headers from credential if they exist
        if credential.additional_headers and isinstance(credential.additional_headers, dict):
             common_headers.update(credential.additional_headers)

        final_reply_content = None
        tool_info_for_response = [] # 用于在最终响应中告知前端工具使用情况

        try:
            # ==========================================================================
            # OpenAI (gpt-3.5, gpt-4, gpt-4-turbo, gpt-4o)
            # ==========================================================================
            if agent.agent_type in ['gpt-3.5', 'gpt-4', 'gpt-4-turbo', 'gpt-4o']:
                final_reply_content, tool_info_for_response = agent_handlers.handle_openai_agent(
                    agent=agent,
                    credential=credential,
                    user_message_content=user_message_content,
                    base_endpoint=base_endpoint,
                    api_key=api_key,
                    common_headers=common_headers,
                    logger=logger
                )
            # ==========================================================================
            # Claude 3
            # ==========================================================================
            elif agent.agent_type.startswith('claude-3'):
                final_reply_content, tool_info_for_response = agent_handlers.handle_claude_agent(
                    agent=agent,
                    credential=credential,
                    user_message_content=user_message_content,
                    base_endpoint=base_endpoint,
                    api_key=api_key,
                    common_headers=common_headers,
                    logger=logger
                )
            # ==========================================================================
            # Gemini
            # ==========================================================================
            elif agent.agent_type.startswith('gemini'): 
                final_reply_content, tool_info_for_response = agent_handlers.handle_gemini_agent(
                    agent=agent,
                    credential=credential,
                    user_message_content=user_message_content,
                    base_endpoint=base_endpoint,
                    api_key=api_key,
                    common_headers=common_headers,
                    logger=logger
                )
            # ==========================================================================
            # Custom Agent
            # ==========================================================================
            elif agent.agent_type == 'custom':
                final_reply_content, tool_info_for_response = agent_handlers.handle_custom_agent(
                    agent=agent,
                    credential=credential,
                    user_message_content=user_message_content,
                    base_endpoint=base_endpoint,
                    api_key=api_key,
                    common_headers=common_headers,
                    logger=logger
                )
            # ==========================================================================
            # Fallback for unknown agent type or if logic above fails to set final_reply_content
            # ==========================================================================
            else: 
                # This block should ideally not be reached if agent_type is one of the handled ones.
                # If it's an unhandled agent_type:
                if agent.agent_type not in ['gpt-3.5', 'gpt-4', 'gpt-4-turbo', 'gpt-4o'] and \
                   not agent.agent_type.startswith('claude-3') and \
                   not agent.agent_type.startswith('gemini') and \
                   agent.agent_type != 'custom':
                    logger.error(f"Unsupported agent type for advanced tool processing loop: {agent.agent_type}. Basic proxy mode might be required.")
                    return Response(
                        {'error': f'Agent type "{agent.agent_type}" is not configured for the advanced tool calling loop.'},
                        status=status.HTTP_400_BAD_REQUEST
                    )
                # If it's a known type but final_reply_content is still None (should have been caught by specific blocks)
                if final_reply_content is None: # Safeguard
                    logger.error(f"Logic error: final_reply_content is None after processing loop for agent type {agent.agent_type}. This indicates an issue in the specific agent's loop logic.")
                    final_reply_content = "An internal error occurred: the agent finished processing iterations without a conclusive reply."


        except requests.exceptions.HTTPError as e:
            logger.error(f"LLM API HTTPError for agent {agent.id} ({agent.agent_type}) calling {e.request.url if e.request else 'N/A'}: {str(e)}", exc_info=True)
            error_detail = str(e)
            if e.response is not None:
                try:
                    error_content = e.response.json()
                    error_detail = {"status_code": e.response.status_code, "content": error_content }
                except json.JSONDecodeError:
                    error_detail = {"status_code": e.response.status_code, "content_text": e.response.text }
            return Response(
                {'error': 'LLM API request failed due to HTTP error.', 'detail': error_detail},
                status=status.HTTP_502_BAD_GATEWAY # Or e.response.status_code if appropriate and not client error
            )
        except requests.exceptions.RequestException as e: # Other network issues (timeout, connection error)
            logger.error(f"LLM API RequestException for agent {agent.id} ({agent.agent_type}): {str(e)}", exc_info=True)
            return Response(
                {'error': 'LLM API request failed due to a network issue.', 'detail': str(e)},
                status=status.HTTP_504_GATEWAY_TIMEOUT
            )
        except json.JSONDecodeError as e: # Errors decoding LLM JSON response or tool args
             logger.error(f"JSONDecodeError during agent send_message for agent {agent.id}: {str(e)}", exc_info=True)
             return Response(
                {'error': 'Failed to decode JSON from LLM response or tool arguments.', 'detail': str(e)},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )
        except Exception as e: # Catch-all for other unexpected errors within the try block
            logger.error(f"Unexpected error during agent send_message for agent {agent.id} ({agent.agent_type}): {str(e)}", exc_info=True)
            return Response(
                {'error': f'An unexpected server error occurred: {str(e)}'},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )

        # Ensure final_reply_content has a value if all paths are exhausted without error, but no reply
        if final_reply_content is None:
            logger.warning(f"send_message for agent {agent.id} completed without error but final_reply_content is None. Iterations: {tool_call_iterations}")
            final_reply_content = "Agent processing completed, but no specific textual reply was generated."
            if tool_info_for_response: # If tools were used, mention it
                final_reply_content += f" Tool interactions occurred: {json.dumps(tool_info_for_response[-1]['executed_tools'] if tool_info_for_response[-1]['executed_tools'] else tool_info_for_response[-1]['llm_request'] )}"


        return Response({
            'reply': final_reply_content,
            'tool_usage_details': tool_info_for_response if tool_info_for_response else "No tool call iterations occurred."
        })

    @action(detail=True, methods=['get'], url_path='tasks', url_name='list-tasks')
    def list_tasks(self, request, pk=None):
        """列出此Agent的所有任务"""
        agent = self.get_object() # 获取当前Agent实例
        tasks = Task.objects.filter(agent=agent).order_by('-created_at')
        
        # 使用分页 (可选, 但推荐)
        page = self.paginate_queryset(tasks)
        if page is not None:
            serializer = self.get_serializer(page, many=True)
            return self.get_paginated_response(serializer.data)
            
        serializer = self.get_serializer(tasks, many=True)
        return Response(serializer.data)

    @action(detail=True, methods=['post'], url_path='tasks', url_name='create-task') # 与list_tasks使用相同的url_path，通过HTTP方法区分
    def create_task(self, request, pk=None):
        """为此Agent创建一个新任务"""
        agent = self.get_object() # 获取当前Agent实例
        serializer = self.get_serializer(data=request.data)
        if serializer.is_valid():
            serializer.save(agent=agent) # 在保存时关联Agent
            return Response(serializer.data, status=status.HTTP_201_CREATED)
        return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)


class AgentSkillViewSet(viewsets.ModelViewSet):
    """
    Agent技能视图集，提供对Agent技能的CRUD操作
    """
    serializer_class = AgentSkillSerializer
    permission_classes = [permissions.IsAuthenticated, IsOwnerOrReadOnly]
    
    def get_queryset(self):
        """返回当前用户可以访问的Agent技能"""
        user = self.request.user
        agent_id = self.kwargs.get('agent_pk')
        return AgentSkill.objects.filter(agent__owner=user, agent_id=agent_id)
    
    def perform_create(self, serializer):
        """创建时自动设置Agent"""
        agent_id = self.kwargs.get('agent_pk')
        agent = get_object_or_404(Agent, id=agent_id, owner=self.request.user)
        serializer.save(agent=agent)
    
    @action(detail=True, methods=['get'])
    def formats(self, request, pk=None, agent_pk=None):
        """获取特定技能的格式化信息，如A2A格式"""
        skill = self.get_object()
        return Response({
            'a2a_format': skill.to_a2a_format()
        })
    
    @action(detail=False, methods=['get'])
    def a2a_card(self, request, agent_pk=None):
        """获取所有技能的A2A格式，用于构建Agent Card"""
        skills = self.get_queryset()
        return Response([skill.to_a2a_format() for skill in skills])


class A2AInteroperabilityTestView(APIView):
    """
    A2A协议互操作性测试视图
    用于测试平台与其他A2A兼容系统的互操作性
    """
    permission_classes = [permissions.IsAuthenticated]
    
    def post(self, request):
        """启动A2A互操作性测试"""
        try:
            agent_id = request.data.get('agent_id')
            target_url = request.data.get('target_url')
            test_type = request.data.get('test_type', 'basic')
            
            if not agent_id:
                return Response({"error": "必须提供agent_id"}, status=status.HTTP_400_BAD_REQUEST)
            
            if not target_url:
                return Response({"error": "必须提供target_url"}, status=status.HTTP_400_BAD_REQUEST)
            
            # 获取Agent
            agent = get_object_or_404(Agent, id=agent_id, owner=request.user)
            
            # 根据测试类型执行不同的测试
            if test_type == 'basic':
                result = self.run_basic_test(agent, target_url)
            elif test_type == 'streaming':
                result = self.run_streaming_test(agent, target_url)
            elif test_type == 'push_notification':
                result = self.run_push_notification_test(agent, target_url)
            else:
                return Response({"error": f"不支持的测试类型: {test_type}"}, status=status.HTTP_400_BAD_REQUEST)
            
            return Response(result)
        except Exception as e:
            import traceback
            return Response({
                "error": f"测试执行失败: {str(e)}",
                "traceback": traceback.format_exc()
            }, status=status.HTTP_500_INTERNAL_SERVER_ERROR)
    
    def run_basic_test(self, agent, target_url):
        """运行基本的A2A互操作性测试"""
        import requests
        import json
        import uuid
        from django.utils import timezone
        
        test_results = {
            "agent_card_test": {"status": "pending"},
            "task_send_test": {"status": "pending"},
            "task_get_test": {"status": "pending"}
        }
        
        # 步骤1: 获取目标系统的Agent Card
        try:
            if not target_url.endswith("/.well-known/agent.json"):
                if target_url.endswith("/"):
                    target_url = target_url + ".well-known/agent.json"
                else:
                    target_url = target_url + "/.well-known/agent.json"
            
            response = requests.get(target_url, timeout=10)
            
            if response.status_code == 200:
                agent_card = response.json()
                test_results["agent_card_test"] = {
                    "status": "success",
                    "agent_card": agent_card,
                    "timestamp": timezone.now().isoformat()
                }
            else:
                test_results["agent_card_test"] = {
                    "status": "failed",
                    "error": f"Failed to get agent card: {response.status_code}",
                    "timestamp": timezone.now().isoformat()
                }
                return test_results
        except Exception as e:
            test_results["agent_card_test"] = {
                "status": "failed",
                "error": f"Exception while getting agent card: {str(e)}",
                "timestamp": timezone.now().isoformat()
            }
            return test_results
        
        # 步骤2: 发送任务
        try:
            # 从agent card获取任务端点
            task_url = agent_card.get("url")
            
            if not task_url:
                test_results["task_send_test"] = {
                    "status": "failed",
                    "error": "Agent card does not contain a valid URL",
                    "timestamp": timezone.now().isoformat()
                }
                return test_results
            
            # 构建A2A任务发送请求
            task_id = str(uuid.uuid4())
            payload = {
                "jsonrpc": "2.0",
                "method": "tasks/send",
                "params": {
                    "taskId": task_id,
                    "message": {
                        "role": "user",
                        "parts": [
                            {
                                "text": "Hello from A2A Workflow Platform interoperability test",
                                "contentType": "text/plain"
                            }
                        ]
                    }
                },
                "id": str(uuid.uuid4())
            }
            
            headers = {
                "Content-Type": "application/json"
            }
            
            # 发送请求
            response = requests.post(task_url, json=payload, headers=headers, timeout=30)
            
            if response.status_code >= 200 and response.status_code < 300:
                result = response.json()
                test_results["task_send_test"] = {
                    "status": "success",
                    "response": result,
                    "timestamp": timezone.now().isoformat()
                }
                
                # 保存任务ID用于下一步
                if "result" in result and "task" in result["result"] and "taskId" in result["result"]["task"]:
                    task_id = result["result"]["task"]["taskId"]
                else:
                    task_id = None
            else:
                test_results["task_send_test"] = {
                    "status": "failed",
                    "error": f"Failed to send task: {response.status_code} - {response.text}",
                    "timestamp": timezone.now().isoformat()
                }
                return test_results
        except Exception as e:
            test_results["task_send_test"] = {
                "status": "failed",
                "error": f"Exception while sending task: {str(e)}",
                "timestamp": timezone.now().isoformat()
            }
            return test_results
        
        # 步骤3: 获取任务状态
        if task_id:
            try:
                # 构建获取任务请求
                payload = {
                    "jsonrpc": "2.0",
                    "method": "tasks/get",
                    "params": {
                        "taskId": task_id
                    },
                    "id": str(uuid.uuid4())
                }
                
                # 发送请求
                response = requests.post(task_url, json=payload, headers=headers, timeout=30)
                
                if response.status_code >= 200 and response.status_code < 300:
                    result = response.json()
                    test_results["task_get_test"] = {
                        "status": "success",
                        "response": result,
                        "timestamp": timezone.now().isoformat()
                    }
                else:
                    test_results["task_get_test"] = {
                        "status": "failed",
                        "error": f"Failed to get task: {response.status_code} - {response.text}",
                        "timestamp": timezone.now().isoformat()
                    }
            except Exception as e:
                test_results["task_get_test"] = {
                    "status": "failed",
                    "error": f"Exception while getting task: {str(e)}",
                    "timestamp": timezone.now().isoformat()
                }
        else:
            test_results["task_get_test"] = {
                "status": "skipped",
                "error": "No valid task ID from previous step",
                "timestamp": timezone.now().isoformat()
            }
        
        # 计算总体结果
        successes = sum(1 for test in test_results.values() if test["status"] == "success")
        total = len(test_results)
        
        test_results["summary"] = {
            "success_rate": f"{successes}/{total}",
            "timestamp": timezone.now().isoformat(),
            "target_url": target_url,
            "agent_id": str(agent.id)
        }
        
        return test_results
    
    def run_streaming_test(self, agent, target_url):
        """运行流式请求互操作性测试"""
        # 实现流式请求测试逻辑
        return {
            "status": "not_implemented",
            "message": "Streaming test not yet implemented"
        }
    
    def run_push_notification_test(self, agent, target_url):
        """运行推送通知互操作性测试"""
        # 实现推送通知测试逻辑
        return {
            "status": "not_implemented",
            "message": "Push notification test not yet implemented"
        }

class TaskViewSet(viewsets.ReadOnlyModelViewSet):
    """
    任务API端点，允许查看任务。
    提供 /api/tasks/ 路径。
    """
    serializer_class = TaskSerializer
    permission_classes = [permissions.IsAuthenticated]

    def get_queryset(self):
        """
        此查询集应返回当前认证用户的所有任务。
        任务可以通过其所属的Agent或所属的Session与用户关联。
        """
        user = self.request.user
        if user.is_superuser:
            return Task.objects.all().order_by('-created_at')
        # 用户可以看到属于他们Agent的任务 或 属于他们Session的任务
        # 使用 distinct() 确保在任务同时满足两个条件时不会重复返回
        return Task.objects.filter(
            models.Q(agent__owner=user) | models.Q(session__owner=user)
        ).distinct().order_by('-created_at')

class SessionViewSet(viewsets.ReadOnlyModelViewSet):
    """
    会话API端点，允许查看会话。
    提供 /api/sessions/ 路径。
    """
    serializer_class = SessionSerializer
    permission_classes = [permissions.IsAuthenticated]

    def get_queryset(self):
        """
        此查询集应返回当前认证用户的所有会话。
        """
        user = self.request.user
        if user.is_superuser:
            return Session.objects.all().order_by('-created_at')
        return Session.objects.filter(owner=user).order_by('-created_at')
