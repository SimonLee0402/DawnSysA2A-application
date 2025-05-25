from django.shortcuts import get_object_or_404
from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework import permissions, status
from django.utils import timezone
import json
import uuid
from .models import Agent, Task, Message, Part, Artifact, PushNotificationConfig, TaskStateHistory
import logging
import time
from django.http import StreamingHttpResponse, JsonResponse
from django.conf import settings

logger = logging.getLogger(__name__)

class A2AWellKnownAgentView(APIView):
    """
    提供符合A2A协议的well-known agent.json端点
    """
    permission_classes = [permissions.AllowAny]  # agent.json通常是公开访问的
    
    def get(self, request, agent_id=None):
        """获取指定Agent的Agent Card"""
        try:
            if agent_id:
                agent = get_object_or_404(Agent, id=agent_id)
                # 使用Agent模型的get_agent_card方法
                agent_card = agent.get_agent_card()
            else:
                # 平台默认Agent Card
                agent_card = self.get_platform_agent_card()
            
            # 返回完整的A2A Agent卡片
            return Response(agent_card)
        except Exception as e:
            logger.exception(f"Error generating agent card: {e}")
            return Response(
                {"error": "无法生成Agent卡片", "detail": str(e)},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )
    
    def get_platform_agent_card(self):
        """获取平台的默认Agent Card"""
        return {
            "name": "A2A工作流平台",
            "description": "一个符合Google A2A协议的工作流协调平台",
            "url": f"{settings.BASE_URL}/api/a2a",
            "version": "1.0.0",
            "capabilities": {
                "streaming": True,
                "pushNotifications": True,
                "stateTransitionHistory": True,
                "taskTree": True
            },
            "authentication": {
                "schemes": ["apiKey", "oauth2"]
            },
            "defaultInputModes": ["text", "file", "data"],
            "defaultOutputModes": ["text", "file", "data"],
            "skills": [
                {
                    "id": "workflow_execution",
                    "name": "工作流执行",
                    "description": "执行预定义的AI工作流",
                    "inputModes": ["text", "data"],
                    "outputModes": ["text", "data"],
                    "examples": ["执行销售分析工作流", "启动客户服务流程"],
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "workflow_id": {
                                "type": "string",
                                "description": "要执行的工作流ID"
                            },
                            "context": {
                                "type": "object",
                                "description": "工作流执行上下文"
                            }
                        },
                        "required": ["workflow_id"]
                    }
                },
                {
                    "id": "agent_discovery",
                    "name": "代理发现",
                    "description": "查找并连接可用的AI代理",
                    "inputModes": ["text"],
                    "outputModes": ["text", "data"],
                    "examples": ["查找所有翻译代理", "连接到客户服务代理"]
                },
                {
                    "id": "message_routing",
                    "name": "消息路由",
                    "description": "将消息路由到合适的代理",
                    "inputModes": ["text", "data"],
                    "outputModes": ["text", "data"],
                    "examples": ["将这个问题路由给最合适的代理", "找到能解决这个技术问题的代理"],
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "routing_strategy": {
                                "type": "string",
                                "enum": ["auto", "manual", "hybrid"],
                                "description": "路由策略"
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["speed", "quality", "cost"],
                                "description": "优先考虑的因素"
                            }
                        }
                    }
                }
            ]
        }


class A2ABaseView(APIView):
    """A2A基础视图类"""
    permission_classes = [permissions.IsAuthenticated]
    
    def get_agent(self, agent_id):
        """获取指定的代理"""
        try:
            return Agent.objects.get(id=agent_id)
        except Agent.DoesNotExist:
            raise ValueError(f"Agent with ID {agent_id} not found")
    
    def get_task(self, task_id):
        """获取指定的任务"""
        try:
            return Task.objects.get(id=task_id)
        except Task.DoesNotExist:
            raise ValueError(f"Task with ID {task_id} not found")
    
    def validate_message_parts(self, parts):
        """验证消息部分是否符合规范"""
        if not parts or not isinstance(parts, list):
            return False
        
        for part in parts:
            if not isinstance(part, dict):
                return False
            
            # 检查消息部分的类型
            if 'text' in part:
                # 文本类型
                if not isinstance(part['text'], str):
                    return False
            elif 'data' in part:
                # 数据类型
                if not isinstance(part['data'], dict):
                    return False
            elif 'fileUri' in part or 'inlineData' in part:
                # 文件类型
                if 'contentType' not in part:
                    return False
            else:
                # 未知类型
                return False
                
        return True
    
    def error_response(self, message, code, status_code=400):
        """
        创建错误响应
        """
        return Response({
            "jsonrpc": "2.0",
            "error": {
                "code": code,
                "message": message
            },
            "id": None
        }, status=status_code)


class A2ATasksSendView(A2ABaseView):
    """实现A2A协议的tasks/send方法"""
    
    def post(self, request):
        """
        处理tasks/send请求
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId', str(uuid.uuid4()))
            agent_id = params.get('agentId')
            session_id = params.get('sessionId')
            message = params.get('message', {})
            
            # 验证消息格式
            if not message or not isinstance(message, dict):
                return self.error_response("Invalid message format", -32602)
            
            role = message.get('role')
            if role not in ['user', 'agent']:
                return self.error_response("Invalid message role", -32602)
            
            parts = message.get('parts', [])
            if not self.validate_message_parts(parts):
                return self.error_response("Invalid message parts format", -32602)
            
            # 获取代理和任务
            try:
                agent = self.get_agent(agent_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 获取或创建任务
            task = None
            try:
                task = Task.objects.get(id=task_id)
                # 如果任务已完成，则不允许继续发送消息
                if task.state in ['completed', 'failed', 'canceled']:
                    return self.error_response(f"Task is already in {task.state} state", -32602)
            except Task.DoesNotExist:
                # 创建新任务
                task = Task.objects.create(
                    id=task_id,
                    session_id=session_id,
                    agent=agent,
                    client_agent=None,  # 可以从认证信息中获取
                    state='submitted'
                )
            
            # 创建消息
            message_obj = Message.objects.create(
                task=task,
                role=role,
                metadata=message.get('metadata', {})
            )
            
            # 创建消息部分
            for part in parts:
                part_obj = None
                
                if 'text' in part:
                    # 文本部分
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='text',
                        content_type=part.get('contentType', 'text/plain'),
                        text_content=part['text']
                    )
                elif 'data' in part:
                    # 数据部分
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='data',
                        content_type=part.get('contentType', 'application/json'),
                        data_content=part['data']
                    )
                elif 'fileUri' in part:
                    # 文件URI部分
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='file',
                        content_type=part.get('contentType', 'application/octet-stream'),
                        file_uri=part['fileUri']
                    )
                elif 'inlineData' in part:
                    # 内联文件部分
                    # 注意：实际实现时需要处理base64解码等
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='file',
                        content_type=part.get('contentType', 'application/octet-stream'),
                        file_content=b''  # 示例，实际应解码inlineData
                    )
            
            # 更新任务状态
            if task.state == 'submitted':
                task.update_state('working')
            
            # 如果是初始消息，则进行处理
            if role == 'user' and task.messages.count() <= 1:
                # 在实际实现中，可能需要启动异步任务来处理
                # 这里我们简单地创建一个响应消息
                self.process_task(task)
            
            # 返回更新后的任务
            return Response({
                "jsonrpc": "2.0",
                "result": {
                    "task": task.to_a2a_format()
                },
                "id": data.get('id')
            })
            
        except Exception as e:
            logger.exception("Error in tasks/send")
            return self.error_response(f"Internal error: {str(e)}", -32603, status_code=500)
    
    def process_task(self, task):
        """
        处理任务
        注意：在实际实现中，这应该是异步的
        """
        try:
            # 获取最近的用户消息
            user_message = task.messages.filter(role='user').order_by('-created_at').first()
            if not user_message:
                return
            
            # 获取用户消息的文本内容
            user_text = ""
            for part in user_message.parts.filter(part_type='text'):
                user_text += part.text_content + " "
            
            # 创建代理响应消息
            agent_message = Message.objects.create(
                task=task,
                role='agent',
                metadata={}
            )
            
            # 创建响应文本部分
            response_text = f"这是来自代理的自动响应：我已收到您的消息 '{user_text}'"
            Part.objects.create(
                message=agent_message,
                part_type='text',
                content_type='text/plain',
                text_content=response_text
            )
            
            # 任务完成
            task.update_state('completed')
            
        except Exception as e:
            logger.exception(f"Error processing task {task.id}")
            task.update_state('failed', error_details=str(e))


class A2ATasksGetView(A2ABaseView):
    """实现A2A协议的tasks/get方法"""
    
    def post(self, request):
        """
        处理tasks/get请求
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId')
            if not task_id:
                return self.error_response("Missing taskId parameter", -32602)
            
            # 获取任务
            try:
                task = self.get_task(task_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 返回任务信息
            return Response({
                "jsonrpc": "2.0",
                "result": {
                    "task": task.to_a2a_format()
                },
                "id": data.get('id')
            })
            
        except Exception as e:
            logger.exception("Error in tasks/get")
            return self.error_response(f"Internal error: {str(e)}", -32603, status_code=500)


class A2ATasksCancelView(A2ABaseView):
    """实现A2A协议的tasks/cancel方法"""
    
    def post(self, request):
        """
        处理tasks/cancel请求
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId')
            reason = params.get('reason', '用户取消')
            
            if not task_id:
                return self.error_response("Missing taskId parameter", -32602)
            
            # 获取任务
            try:
                task = self.get_task(task_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 只有处于'submitted'、'working'或'input-required'状态的任务才能被取消
            if task.state not in ['submitted', 'working', 'input-required']:
                return self.error_response(f"Cannot cancel task in {task.state} state", -32602)
            
            # 更新任务状态
            task.update_state('canceled', error_details=reason)
            
            # 返回任务信息
            return Response({
                "jsonrpc": "2.0",
                "result": {
                    "task": task.to_a2a_format()
                },
                "id": data.get('id')
            })
            
        except Exception as e:
            logger.exception("Error in tasks/cancel")
            return self.error_response(f"Internal error: {str(e)}", -32603, status_code=500)


class A2ATasksPushNotificationSetView(A2ABaseView):
    """实现A2A协议的tasks/pushNotification/set方法"""
    
    def post(self, request):
        """
        设置任务的推送通知配置
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId')
            push_config = params.get('pushNotificationConfig', {})
            
            if not task_id:
                return self.error_response("Missing taskId", -32602)
            
            if not push_config or not isinstance(push_config, dict):
                return self.error_response("Invalid push notification config", -32602)
            
            # 获取任务
            try:
                task = self.get_task(task_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 验证推送配置
            url = push_config.get('url')
            if not url:
                return self.error_response("Missing push notification URL", -32602)
            
            # 创建或更新推送通知配置
            try:
                push_notification_config, created = PushNotificationConfig.objects.update_or_create(
                    task=task,
                    defaults={
                        'url': url,
                        'token': push_config.get('token'),
                        'auth_scheme': push_config.get('authentication', {}).get('schemes', ['none'])[0],
                        'auth_credentials': push_config.get('authentication', {}).get('credentials')
                    }
                )
                
                # 保存配置到任务中
                task.push_notification_config = push_notification_config.to_a2a_format()
                task.save()
                
                return Response({
                    "jsonrpc": "2.0",
                    "result": {
                        "success": True
                    },
                    "id": data.get('id')
                })
            except Exception as e:
                logger.exception(f"Error setting push notification: {str(e)}")
                return self.error_response(f"Error setting push notification: {str(e)}", -32603)
        except Exception as e:
            logger.exception(f"Unexpected error in tasks/pushNotification/set: {str(e)}")
            return self.error_response(f"Unexpected error: {str(e)}", -32603)


class A2ATasksListView(A2ABaseView):
    """实现A2A协议的tasks/list方法"""
    
    def post(self, request):
        """
        列出任务
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            session_id = params.get('sessionId')
            agent_id = params.get('agentId')
            state = params.get('state')  # 使用state而不是status，符合A2A协议
            limit = params.get('limit', 20)
            offset = params.get('offset', 0)
            created_after = params.get('createdAfter')
            created_before = params.get('createdBefore')
            
            # 构建查询
            query = Task.objects.all()
            
            if session_id:
                query = query.filter(session_id=session_id)
            
            if agent_id:
                query = query.filter(agent_id=agent_id)
            
            if state:
                query = query.filter(state=state)
                
            # 添加时间过滤
            if created_after:
                try:
                    created_after_date = timezone.datetime.fromisoformat(created_after.replace('Z', '+00:00'))
                    query = query.filter(created_at__gte=created_after_date)
                except (ValueError, TypeError):
                    return self.error_response("Invalid createdAfter format", -32602)
                    
            if created_before:
                try:
                    created_before_date = timezone.datetime.fromisoformat(created_before.replace('Z', '+00:00'))
                    query = query.filter(created_at__lte=created_before_date)
                except (ValueError, TypeError):
                    return self.error_response("Invalid createdBefore format", -32602)
            
            # 执行分页查询
            total = query.count()
            tasks = query.order_by('-created_at')[offset:offset+limit]
            
            # 构建响应
            return Response({
                "jsonrpc": "2.0",
                "result": {
                    "tasks": [task.to_a2a_format() for task in tasks],
                    "pagination": {
                        "total": total,
                        "offset": offset,
                        "limit": limit
                    }
                },
                "id": data.get('id')
            })
        except Exception as e:
            logger.exception(f"Unexpected error in tasks/list: {str(e)}")
            return self.error_response(f"Unexpected error: {str(e)}", -32603)


class A2ATasksSendSubscribeView(A2ABaseView):
    """实现A2A协议的tasks/sendSubscribe方法"""
    
    def post(self, request):
        """
        处理tasks/sendSubscribe请求 - 流式处理版本的tasks/send
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId', str(uuid.uuid4()))
            agent_id = params.get('agentId')
            session_id = params.get('sessionId')
            message = params.get('message', {})
            push_notification = params.get('pushNotification')
            
            # 验证消息格式
            if not message or not isinstance(message, dict):
                return self.error_response("Invalid message format", -32602)
            
            role = message.get('role')
            if role not in ['user', 'agent']:
                return self.error_response("Invalid message role", -32602)
            
            parts = message.get('parts', [])
            if not self.validate_message_parts(parts):
                return self.error_response("Invalid message parts format", -32602)
            
            # 获取代理和任务
            try:
                agent = self.get_agent(agent_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 获取或创建任务
            task = None
            try:
                task = Task.objects.get(id=task_id)
                # 如果任务已完成，则不允许继续发送消息
                if task.state in ['completed', 'failed', 'canceled']:
                    return self.error_response(f"Task is already in {task.state} state", -32602)
            except Task.DoesNotExist:
                # 创建新任务
                task = Task.objects.create(
                    id=task_id,
                    session_id=session_id,
                    agent=agent,
                    client_agent=None,
                    state='submitted'
                )
            
            # 创建消息
            message_obj = Message.objects.create(
                task=task,
                role=role,
                metadata=message.get('metadata', {})
            )
            
            # 创建消息部分
            for part in parts:
                part_obj = None
                
                if 'text' in part:
                    # 文本部分
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='text',
                        content_type=part.get('contentType', 'text/plain'),
                        text_content=part['text']
                    )
                elif 'data' in part:
                    # 数据部分
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='data',
                        content_type=part.get('contentType', 'application/json'),
                        data_content=part['data']
                    )
                elif 'fileUri' in part:
                    # 文件URI部分
                    part_obj = Part.objects.create(
                        message=message_obj,
                        part_type='file',
                        content_type=part.get('contentType', 'application/octet-stream'),
                        file_uri=part['fileUri']
                    )
                elif 'inlineData' in part:
                    # 内联文件部分
                    # 注意：实际实现时需要处理base64解码等
                    import base64
                    try:
                        file_data = base64.b64decode(part['inlineData'])
                        part_obj = Part.objects.create(
                            message=message_obj,
                            part_type='file',
                            content_type=part.get('contentType', 'application/octet-stream'),
                            file_content=file_data
                        )
                    except Exception as e:
                        logger.error(f"Error decoding inline file data: {str(e)}")
                        part_obj = Part.objects.create(
                            message=message_obj,
                            part_type='file',
                            content_type=part.get('contentType', 'application/octet-stream'),
                            file_content=b''
                        )
            
            # 设置推送通知
            if push_notification:
                try:
                    # 创建推送通知配置
                    push_config, created = PushNotificationConfig.objects.update_or_create(
                        task=task,
                        defaults={
                            'url': push_notification.get('url'),
                            'token': push_notification.get('token'),
                            'auth_scheme': push_notification.get('authentication', {}).get('schemes', ['none'])[0] if push_notification.get('authentication') else None,
                            'auth_credentials': push_notification.get('authentication', {}).get('credentials') if push_notification.get('authentication') else None
                        }
                    )
                    
                    # 保存配置到任务中
                    task.push_notification_config = push_notification
                    task.save()
                except Exception as e:
                    logger.error(f"Error setting push notification: {str(e)}")
            
            # 更新任务状态
            if task.state == 'submitted':
                task.update_state('working')
            
            # 返回SSE响应
            response = StreamingHttpResponse(
                self.event_stream(task, message_obj),
                content_type='text/event-stream'
            )
            
            # 添加SSE相关的响应头
            response['X-Accel-Buffering'] = 'no'  # 禁用Nginx的缓冲
            response['Cache-Control'] = 'no-cache'
            response['Connection'] = 'keep-alive'
            
            return response
        except Exception as e:
            logger.exception(f"Unexpected error in tasks/sendSubscribe: {str(e)}")
            return self.error_response(f"Unexpected error: {str(e)}", -32603)
    
    def event_stream(self, task, message_obj):
        """生成事件流"""
        # 首先发送任务创建确认
        yield f"event: task-status-update\ndata: {json.dumps({{'id': str(task.id), 'status': {{'state': task.state, 'timestamp': task.updated_at.isoformat()}}, 'final': False}})} \n\n"
        
        # 如果是初始消息，则进行处理
        if message_obj.role == 'user':
            # 在真实应用中，这里应该启动异步任务而不是直接处理
            # 但为了示例，我们简单地创建一个响应
            
            # 创建Agent响应消息
            agent_message = Message.objects.create(
                task=task,
                role='agent',
                metadata={}
            )
            
            # 创建多个部分来演示流式传输
            import time
            
            # 第一部分 - 文本开始
            text_part1 = Part.objects.create(
                message=agent_message,
                part_type='text',
                content_type='text/plain',
                text_content='这是一个分块传输的',
                index=0,
                is_append=False,
                is_last_chunk=False
            )
            
            # 发送第一部分的通知
            yield f"event: task-status-update\ndata: {json.dumps({{'id': str(task.id), 'status': {{'state': task.state, 'timestamp': task.updated_at.isoformat(), 'message': agent_message.to_a2a_format()}}, 'final': False}})} \n\n"
            
            # 模拟处理延迟
            time.sleep(0.5)
            
            # 第二部分 - 文本继续
            text_part2 = Part.objects.create(
                message=agent_message,
                part_type='text',
                content_type='text/plain',
                text_content='回答示例，',
                index=1,
                is_append=True,
                is_last_chunk=False
            )
            
            # 发送第二部分
            yield f"event: task-status-update\ndata: {json.dumps({{'id': str(task.id), 'status': {{'state': task.state, 'timestamp': task.updated_at.isoformat(), 'message': {{'role': 'agent', 'parts': [text_part2.to_a2a_format()]}}}}, 'final': False}})} \n\n"
            
            # 模拟处理延迟
            time.sleep(0.5)
            
            # 第三部分 - 文本完成
            text_part3 = Part.objects.create(
                message=agent_message,
                part_type='text',
                content_type='text/plain',
                text_content='展示了A2A协议的流式处理能力。',
                index=2,
                is_append=True,
                is_last_chunk=True
            )
            
            # 发送第三部分
            yield f"event: task-status-update\ndata: {json.dumps({{'id': str(task.id), 'status': {{'state': task.state, 'timestamp': task.updated_at.isoformat(), 'message': {{'role': 'agent', 'parts': [text_part3.to_a2a_format()]}}}}, 'final': False}})} \n\n"
            
            # 模拟处理延迟
            time.sleep(0.5)
            
            # 创建Artifact
            artifact = Artifact.objects.create(
                task=task,
                artifact_type='text/result',
                name='完整结果',
                description='完整的响应文本',
                index=0,
                is_append=False,
                is_last_chunk=True
            )
            
            # 创建Artifact部分
            artifact_part = Part.objects.create(
                artifact=artifact,
                part_type='text',
                content_type='text/plain',
                text_content='这是一个分块传输的回答示例，展示了A2A协议的流式处理能力。'
            )
            
            # 发送Artifact
            yield f"event: task-artifact-update\ndata: {json.dumps({{'id': str(task.id), 'artifact': artifact.to_a2a_format()}})} \n\n"
            
            # 添加状态转换历史记录 - A2A协议新特性
            state_history = [
                {
                    "state": "submitted",
                    "timestamp": (task.created_at - timezone.timedelta(seconds=2)).isoformat(),
                    "reason": "Task submitted by user"
                },
                {
                    "state": "working", 
                    "timestamp": task.created_at.isoformat(),
                    "reason": "Processing user input"
                },
                {
                    "state": "completed",
                    "timestamp": timezone.now().isoformat(),
                    "reason": "Response generated successfully"
                }
            ]
            
            # 发送状态历史更新
            yield f"event: task-state-history-update\ndata: {json.dumps({{'id': str(task.id), 'stateHistory': state_history}})} \n\n"
            
            # 更新任务状态为已完成
            task.update_state('completed')
            
            # 发送最终状态通知
            yield f"event: task-status-update\ndata: {json.dumps({{'id': str(task.id), 'status': {{'state': task.state, 'timestamp': task.updated_at.isoformat()}}, 'final': True}})} \n\n"
            
            # 如果有推送通知配置，调用推送通知
            if task.push_notification_config:
                self.send_push_notification(task)
        else:
            # 如果不是用户消息，则任务可能处于继续状态
            # 在这里添加继续处理的逻辑
            pass
    
    def send_push_notification(self, task):
        """发送推送通知"""
        try:
            # 获取推送配置
            push_config = task.push_config if hasattr(task, 'push_config') else None
            if not push_config:
                return
            
            import requests
            
            # 构建通知数据
            notification_data = {
                "jsonrpc": "2.0",
                "method": "task/update",
                "params": {
                    "task": task.to_a2a_format()
                }
            }
            
            # 构建请求头
            headers = {
                'Content-Type': 'application/json'
            }
            
            # 添加认证信息
            if push_config.auth_scheme and push_config.auth_scheme != 'none':
                if push_config.auth_scheme == 'apiKey':
                    headers['Authorization'] = f"Bearer {push_config.auth_credentials}"
                elif push_config.auth_scheme == 'basic':
                    import base64
                    auth_string = base64.b64encode(push_config.auth_credentials.encode()).decode()
                    headers['Authorization'] = f"Basic {auth_string}"
            
            # 发送通知
            response = requests.post(
                push_config.url,
                headers=headers,
                json=notification_data,
                timeout=5  # 5秒超时
            )
            
            # 记录结果
            if response.status_code >= 200 and response.status_code < 300:
                logger.info(f"Successfully sent push notification for task {task.id}")
            else:
                logger.error(f"Failed to send push notification for task {task.id}. Status: {response.status_code}, Response: {response.text}")
        except Exception as e:
            logger.exception(f"Error sending push notification: {str(e)}")


class A2ATasksResubscribeView(A2ABaseView):
    """实现A2A协议的tasks/resubscribe方法（SSE流式响应）"""
    
    def post(self, request):
        """
        处理tasks/resubscribe请求
        为简化实现，这里不使用真正的SSE，而是返回常规响应
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId')
            if not task_id:
                return self.error_response("Missing taskId parameter", -32602)
            
            # 获取任务
            try:
                task = self.get_task(task_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 返回任务信息
            return Response({
                "jsonrpc": "2.0",
                "result": {
                    "task": task.to_a2a_format()
                },
                "id": data.get('id')
            })
            
        except Exception as e:
            logger.exception("Error in tasks/resubscribe")
            return self.error_response(f"Internal error: {str(e)}", -32603, status_code=500)


class A2ATasksTreeView(A2ABaseView):
    """实现A2A协议的tasks/tree方法"""
    
    def post(self, request):
        """
        处理tasks/tree请求
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId')
            operation = params.get('operation')
            
            if not task_id:
                return self.error_response("Missing taskId parameter", -32602)
                
            if operation not in ['get', 'update']:
                return self.error_response("Invalid operation parameter. Must be 'get' or 'update'", -32602)
            
            # 获取任务
            try:
                task = self.get_task(task_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 根据操作类型处理请求
            if operation == 'get':
                result = self.get_task_tree(request, params, task)
            else:  # operation == 'update'
                result = self.update_task_tree(request, params, task)
            
            # 返回结果
            return Response({
                "jsonrpc": "2.0",
                "result": result,
                "id": data.get('id')
            })
            
        except Exception as e:
            logger.exception(f"Unexpected error in tasks/tree: {str(e)}")
            return self.error_response(f"Unexpected error: {str(e)}", -32603)
    
    def get_task_tree(self, request, params, task):
        """获取任务树"""
        # 查找与此任务相关的所有任务树Artifact
        tree_artifacts = Artifact.objects.filter(
            task=task,
            is_task_tree=True
        ).order_by('-created_at')
        
        # 如果没有任务树数据，则构建一个基本的树
        if not tree_artifacts.exists():
            tree = self.build_task_tree(task)
        else:
            # 使用最新的任务树数据
            latest_tree_artifact = tree_artifacts.first()
            tree = {
                "taskId": str(task.id),
                "parentTaskId": str(latest_tree_artifact.parent_task_id) if latest_tree_artifact.parent_task_id else None,
                "childTaskIds": [str(id) for id in latest_tree_artifact.child_task_ids] if latest_tree_artifact.child_task_ids else []
            }
            
            # 添加子任务的详细信息
            if tree["childTaskIds"]:
                child_tasks = []
                for child_id in tree["childTaskIds"]:
                    try:
                        child_task = Task.objects.get(id=child_id)
                        child_tasks.append({
                            "taskId": str(child_task.id),
                            "status": {
                                "state": child_task.state,
                                "timestamp": child_task.updated_at.isoformat()
                            },
                            "metadata": child_task.metadata or {}
                        })
                    except Task.DoesNotExist:
                        # 子任务不存在，跳过
                        pass
                
                tree["childTasks"] = child_tasks
        
        return {"taskTree": tree}
    
    def update_task_tree(self, request, params, task):
        """更新任务树"""
        tree_update = params.get('taskTree', {})
        
        # 验证参数
        if not isinstance(tree_update, dict):
            raise ValueError("Invalid taskTree parameter")
            
        parent_task_id = tree_update.get('parentTaskId')
        child_task_ids = tree_update.get('childTaskIds', [])
        
        # 验证父任务ID
        if parent_task_id:
            try:
                parent_task = self.get_task(parent_task_id)
            except ValueError:
                raise ValueError(f"Parent task with ID {parent_task_id} not found")
        
        # 验证子任务ID
        valid_child_ids = []
        for child_id in child_task_ids:
            try:
                child_task = self.get_task(child_id)
                valid_child_ids.append(str(child_task.id))
            except ValueError:
                # 无效的子任务ID，记录日志但不中断
                logger.warning(f"Child task with ID {child_id} not found")
        
        # 创建或更新任务树Artifact
        tree_artifact = Artifact.objects.create(
            task=task,
            artifact_type="taskTree",
            name="Task Tree",
            description="Task relationship tree",
            is_task_tree=True,
            parent_task_id=parent_task_id,
            child_task_ids=valid_child_ids
        )
        
        # 返回更新后的任务树
        return {
            "taskTree": {
                "taskId": str(task.id),
                "parentTaskId": str(tree_artifact.parent_task_id) if tree_artifact.parent_task_id else None,
                "childTaskIds": tree_artifact.child_task_ids
            }
        }
    
    def build_task_tree(self, task):
        """构建基本的任务树"""
        # 查找可能的子任务
        child_tasks = Task.objects.filter(
            metadata__parentTaskId=str(task.id)
        ).order_by('created_at')
        
        # 构建树
        tree = {
            "taskId": str(task.id),
            "parentTaskId": task.metadata.get('parentTaskId') if task.metadata else None,
            "childTaskIds": [str(child.id) for child in child_tasks]
        }
        
        # 如果有子任务，添加详细信息
        if child_tasks:
            tree["childTasks"] = [{
                "taskId": str(child.id),
                "status": {
                    "state": child.state,
                    "timestamp": child.updated_at.isoformat()
                },
                "metadata": child.metadata or {}
            } for child in child_tasks]
        
        return tree


class A2ATasksStateHistoryView(A2ABaseView):
    """实现A2A协议的tasks/stateHistory方法"""
    
    def post(self, request):
        """
        处理tasks/stateHistory请求
        """
        try:
            # 获取并验证请求参数
            data = request.data
            params = data.get('params', {})
            
            task_id = params.get('taskId')
            if not task_id:
                return self.error_response("Missing taskId parameter", -32602)
            
            # 获取任务
            try:
                task = self.get_task(task_id)
            except ValueError as e:
                return self.error_response(str(e), -32602)
            
            # 获取状态历史
            state_history = self.get_state_history(task)
            
            # 返回历史记录
            return Response({
                "jsonrpc": "2.0",
                "result": {
                    "stateHistory": state_history
                },
                "id": data.get('id')
            })
            
        except Exception as e:
            logger.exception(f"Unexpected error in tasks/stateHistory: {str(e)}")
            return self.error_response(f"Unexpected error: {str(e)}", -32603)
    
    def get_state_history(self, task):
        """获取任务状态历史记录"""
        # 这里我们将使用TaskStateHistory模型，而不是简单地存储在任务元数据中
        # 如果您没有这个模型，需要创建它，或者从现有日志中构建历史记录
        history_records = TaskStateHistory.objects.filter(task=task).order_by('timestamp')
        
        if not history_records.exists():
            # 如果没有记录，至少提供当前状态
            return [{
                "state": task.state,
                "timestamp": task.updated_at.isoformat(),
                "reason": task.error_details if task.state == 'failed' else None
            }]
        
        # 构建历史记录列表
        state_history = []
        for record in history_records:
            history_item = {
                "state": record.state,
                "timestamp": record.timestamp.isoformat()
            }
            
            if record.reason:
                history_item["reason"] = record.reason
                
            state_history.append(history_item)
            
        return state_history 