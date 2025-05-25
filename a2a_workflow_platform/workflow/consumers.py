import json
from channels.generic.websocket import AsyncWebsocketConsumer
from channels.db import database_sync_to_async
from .models import WorkflowInstance, WorkflowStep

class WorkflowInstanceConsumer(AsyncWebsocketConsumer):
    async def connect(self):
        self.instance_id = self.scope['url_route']['kwargs']['instance_id']
        self.instance_group_name = f'workflow_instance_{self.instance_id}'
        
        # 验证用户是否有权限查看此实例
        if not await self.has_instance_permission():
            await self.close()
            return
            
        # 加入实例组
        await self.channel_layer.group_add(
            self.instance_group_name,
            self.channel_name
        )
        
        await self.accept()
        
        # 发送初始数据
        instance_data = await self.get_instance_data()
        await self.send(text_data=json.dumps({
            'type': 'instance_update',
            'instance': instance_data
        }))
        
    async def disconnect(self, close_code):
        # 离开实例组
        await self.channel_layer.group_discard(
            self.instance_group_name,
            self.channel_name
        )
        
    async def receive(self, text_data):
        """
        接收从客户端发送的消息
        """
        text_data_json = json.loads(text_data)
        message_type = text_data_json.get('type', 'refresh')
        
        if message_type == 'refresh':
            # 请求刷新数据
            instance_data = await self.get_instance_data()
            await self.send(text_data=json.dumps({
                'type': 'instance_update',
                'instance': instance_data
            }))
    
    async def instance_update(self, event):
        """
        处理实例更新消息
        """
        # 将消息发送到WebSocket
        await self.send(text_data=json.dumps(event))
        
    @database_sync_to_async
    def has_instance_permission(self):
        """
        检查当前用户是否有权限访问此实例
        """
        user = self.scope['user']
        if not user.is_authenticated:
            return False
            
        try:
            instance = WorkflowInstance.objects.get(instance_id=self.instance_id)
            return instance.created_by == user
        except WorkflowInstance.DoesNotExist:
            return False
            
    @database_sync_to_async
    def get_instance_data(self):
        """
        获取实例数据
        """
        try:
            instance = WorkflowInstance.objects.get(instance_id=self.instance_id)
            steps = WorkflowStep.objects.filter(instance=instance).order_by('step_index')
            
            # 计算进度
            total_steps = len(instance.workflow.definition.get('steps', []))
            completed_steps = steps.filter(status='completed').count()
            
            # 计算进度百分比
            if total_steps > 0:
                progress_percentage = int((completed_steps / total_steps) * 100)
            else:
                progress_percentage = 0
                
            # 获取最近的100条日志
            logs = list(instance.logs.all().order_by('-timestamp')[:100].values(
                'timestamp', 'level', 'message', 'details'
            ))
                
            # 格式化步骤数据
            steps_data = []
            for step in steps:
                steps_data.append({
                    'id': step.id,
                    'step_id': step.step_id,
                    'step_index': step.step_index,
                    'step_name': step.step_name,
                    'step_type': step.step_type,
                    'status': step.status,
                    'started_at': step.started_at.isoformat() if step.started_at else None,
                    'completed_at': step.completed_at.isoformat() if step.completed_at else None,
                    'parameters': step.parameters,
                    'output_data': step.output_data,
                    'error': step.error
                })
                
            return {
                'id': str(instance.instance_id),
                'name': instance.name,
                'display_name': instance.display_name,
                'status': instance.status,
                'current_step_index': instance.current_step_index,
                'created_at': instance.created_at.isoformat(),
                'started_at': instance.started_at.isoformat() if instance.started_at else None,
                'completed_at': instance.completed_at.isoformat() if instance.completed_at else None,
                'context': instance.context,
                'output': instance.output,
                'error': instance.error,
                'steps': steps_data,
                'logs': logs,
                'progress': {
                    'total_steps': total_steps,
                    'completed_steps': completed_steps,
                    'percentage': progress_percentage
                }
            }
        except WorkflowInstance.DoesNotExist:
            return None 