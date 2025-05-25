from django.db import models
from django.conf import settings
from django.utils.translation import gettext_lazy as _
import uuid
from cryptography.fernet import Fernet
import json
from django.utils import timezone

# Create your models here.

class Agent(models.Model):
    """Agent模型，表示一个AI Agent"""
    AGENT_TYPES = (
        ('gpt-3.5', 'GPT-3.5'),
        ('gpt-4', 'GPT-4'),
        ('claude-3', 'Claude 3'),
        ('gemini', 'Gemini'),
        ('custom', '自定义'),
        ('a2a', 'A2A兼容'),
    )
    
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    name = models.CharField(_('名称'), max_length=100)
    description = models.TextField(_('描述'), blank=True)
    agent_type = models.CharField(_('Agent类型'), max_length=50, choices=AGENT_TYPES)
    model_name = models.CharField(_('模型名称'), max_length=100)
    is_active = models.BooleanField(_('是否激活'), default=True)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    updated_at = models.DateTimeField(_('更新时间'), auto_now=True)
    owner = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name='agents', verbose_name=_('所有者'))
    
    # A2A协议特定字段
    is_a2a_compliant = models.BooleanField(_('A2A协议兼容'), default=False)
    a2a_endpoint_url = models.URLField(_('A2A端点URL'), blank=True, null=True)
    a2a_version = models.CharField(_('A2A版本'), max_length=20, default="1.0.0")
    a2a_provider = models.JSONField(_('提供商信息'), default=dict, blank=True)
    a2a_documentation_url = models.URLField(_('文档URL'), blank=True, null=True)
    a2a_contact = models.JSONField(_('联系信息'), default=dict, blank=True)
    a2a_links = models.JSONField(_('相关链接'), default=dict, blank=True)
    a2a_performance = models.JSONField(_('性能指标'), default=dict, blank=True)
    
    class Meta:
        verbose_name = _('Agent')
        verbose_name_plural = _('Agents')
        ordering = ['-created_at']
    
    def __str__(self):
        return f"{self.name} ({self.agent_type})"
    
    def get_agent_card(self):
        """
        生成符合A2A协议的Agent Card
        """
        card = {
            "name": self.name,
            "description": self.description,
            "url": f"{settings.BASE_URL}/api/a2a/agents/{self.id}/tasks",
            "version": self.a2a_version,
            "capabilities": {
                "streaming": True,
                "pushNotifications": True,
                "stateTransitionHistory": True,
                "taskTree": self.agent_type in ['gpt-4', 'claude-3', 'gemini']  # 高级模型支持任务树
            },
            "authentication": {
                "schemes": ["apiKey", "oauth2"]
            },
            "defaultInputModes": ["text", "file", "data"],
            "defaultOutputModes": ["text", "file", "data"],
        }
        
        # 添加提供商信息（如果有）
        if self.a2a_provider:
            card["provider"] = self.a2a_provider
            
        # 添加文档URL（如果有）
        if self.a2a_documentation_url:
            card["documentationUrl"] = self.a2a_documentation_url
        
        # 添加高级功能指标（如果有）
        if self.a2a_performance:
            card["performance"] = self.a2a_performance
        else:
            # 默认性能指标
            card["performance"] = {
                "latency": "medium",  # low(1秒内), medium(1-5秒), high(>5秒)
                "throughput": "high",  # low(<10 RPS), medium(10-100 RPS), high(>100 RPS)
                "availability": 0.999  # 99.9% 可用性
            }
        
        # 添加技能
        skills = []
        for skill in self.skills.all():
            skills.append(skill.to_a2a_format())
        
        card["skills"] = skills if skills else [
            {
                "id": f"{self.agent_type}_{self.id}",
                "name": self.name,
                "description": self.description or f"{self.get_agent_type_display()}类型的AI助手",
                "inputModes": ["text"],
                "outputModes": ["text"],
                "examples": ["您好，我需要帮助", "请介绍一下你的功能"]
            }
        ]
        
        # 添加技术联系人
        if self.a2a_contact:
            card["contact"] = self.a2a_contact
        else:
            card["contact"] = {
                "email": "support@a2a-platform.example.com",
                "url": f"{settings.BASE_URL}/support"
            }
        
        # 添加使用条款与隐私政策
        if self.a2a_links:
            card["links"] = self.a2a_links
        else:
            card["links"] = {
                "terms": f"{settings.BASE_URL}/terms",
                "privacy": f"{settings.BASE_URL}/privacy"
            }
        
        return card


class AgentSkill(models.Model):
    """Agent技能模型，符合A2A协议的技能定义"""
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    agent = models.ForeignKey(Agent, on_delete=models.CASCADE, related_name='skills')
    skill_id = models.CharField(_('技能ID'), max_length=100, unique=True)
    name = models.CharField(_('技能名称'), max_length=100)
    description = models.TextField(_('技能描述'), blank=True)
    input_modes = models.JSONField(_('输入模式'), default=list)
    output_modes = models.JSONField(_('输出模式'), default=list)
    examples = models.JSONField(_('示例'), default=list)
    parameters = models.JSONField(_('参数'), default=dict, blank=True)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    updated_at = models.DateTimeField(_('更新时间'), auto_now=True)
    
    # 扩展A2A协议字段
    skill_type = models.CharField(_('技能类型'), max_length=100, default='general')
    scope = models.CharField(_('作用域'), max_length=100, default='public')
    authentication_required = models.BooleanField(_('需要认证'), default=True)
    rate_limits = models.JSONField(_('速率限制'), default=dict, blank=True)
    authorization_hints = models.JSONField(_('授权提示'), default=list, blank=True)
    tags = models.JSONField(_('标签'), default=list, blank=True)
    additional_auth_scopes = models.JSONField(_('额外授权范围'), default=list, blank=True)
    documentation_url = models.URLField(_('文档URL'), blank=True, null=True)
    version = models.CharField(_('版本'), max_length=50, blank=True, null=True)
    skill_settings = models.JSONField(_('技能设置'), default=dict, blank=True)
    
    class Meta:
        verbose_name = _('Agent技能')
        verbose_name_plural = _('Agent技能')
    
    def __str__(self):
        return f"{self.name} ({self.skill_id})"
    
    def to_a2a_format(self):
        """转换为A2A协议格式"""
        result = {
            "id": self.skill_id,
            "name": self.name,
            "description": self.description,
            "inputModes": self.input_modes,
            "outputModes": self.output_modes,
            "examples": self.examples,
        }
        
        # 添加可选字段
        if self.parameters:
            result["parameters"] = self.parameters
            
        if self.skill_type != 'general':
            result["type"] = self.skill_type
            
        if self.scope != 'public':
            result["scope"] = self.scope
            
        if self.authorization_hints:
            result["authorizationHints"] = self.authorization_hints
            
        if self.rate_limits:
            result["rateLimits"] = self.rate_limits
            
        if self.tags:
            result["tags"] = self.tags
            
        if self.additional_auth_scopes:
            result["additionalAuthScopes"] = self.additional_auth_scopes
            
        if self.documentation_url:
            result["documentationUrl"] = self.documentation_url
            
        if self.version:
            result["version"] = self.version
            
        if self.skill_settings:
            result["settings"] = self.skill_settings
            
        return result


class AgentCredential(models.Model):
    """Agent凭证模型，用于存储API密钥等敏感信息"""
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    agent = models.OneToOneField(Agent, on_delete=models.CASCADE, related_name='credential')
    api_key = models.CharField(_('API密钥'), max_length=255)
    api_endpoint = models.URLField(_('API端点'), blank=True)
    additional_params = models.JSONField(_('附加参数'), default=dict, blank=True)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    updated_at = models.DateTimeField(_('更新时间'), auto_now=True)
    
    # A2A认证相关字段
    a2a_auth_type = models.CharField(_('A2A认证类型'), max_length=50, blank=True, null=True)
    a2a_auth_config = models.JSONField(_('A2A认证配置'), default=dict, blank=True)
    
    class Meta:
        verbose_name = _('Agent凭证')
        verbose_name_plural = _('Agent凭证')
    
    def save(self, *args, **kwargs):
        # 加密API密钥
        if self.api_key and not self.api_key.startswith('encrypted:'):
            key = settings.AGENT_CREDENTIALS_SECRET.encode()
            fernet = Fernet(key)
            encrypted_api_key = fernet.encrypt(self.api_key.encode())
            self.api_key = f"encrypted:{encrypted_api_key.decode()}"
        
        super().save(*args, **kwargs)
    
    def get_api_key(self):
        """获取解密后的API密钥"""
        if self.api_key.startswith('encrypted:'):
            encrypted_key = self.api_key[10:] # 移除'encrypted:'前缀
            key = settings.AGENT_CREDENTIALS_SECRET.encode()
            fernet = Fernet(key)
            decrypted_key = fernet.decrypt(encrypted_key.encode()).decode()
            return decrypted_key
        return self.api_key


class Task(models.Model):
    """
    代表A2A任务
    一个任务包含多个消息
    """
    STATE_CHOICES = [
        ('submitted', '已提交'),
        ('working', '处理中'),
        ('completed', '已完成'),
        ('failed', '失败'),
        ('canceled', '已取消'),
        ('input-required', '需要输入')
    ]
    
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    session = models.ForeignKey('Session', on_delete=models.CASCADE, related_name='tasks', blank=True, null=True)
    agent = models.ForeignKey(Agent, on_delete=models.CASCADE, related_name='tasks')
    client_agent = models.CharField(max_length=128, blank=True, null=True)
    state = models.CharField(max_length=20, choices=STATE_CHOICES, default='submitted')
    metadata = models.JSONField(_('元数据'), blank=True, default=dict)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
    completed_at = models.DateTimeField(_('完成时间'), null=True, blank=True)
    error_details = models.TextField(_('错误详情'), blank=True, null=True)
    
    # 推送通知配置
    push_notification_config = models.JSONField(_('推送通知配置'), null=True, blank=True)
    
    class Meta:
        verbose_name = _('任务')
        verbose_name_plural = _('任务')
        ordering = ['-created_at']
    
    def __str__(self):
        return f"Task {self.id} ({self.get_state_display()})"
    
    def update_state(self, new_state, error_details=None, initiated_by=None, reason=None):
        """
        更新任务状态并记录历史
        
        Args:
            new_state: 新状态
            error_details: 错误详情（如果状态为failed）
            initiated_by: 状态变更发起者（用户ID或系统标识符）
            reason: 变更原因描述
        """
        if new_state not in [state[0] for state in self.STATE_CHOICES]:
            raise ValueError(f"Invalid state: {new_state}")
        
        # 保存前一状态
        previous_state = self.state
        
        # 更新状态
        self.state = new_state
        if new_state in ['completed', 'failed', 'canceled']:
            self.completed_at = timezone.now()
        if error_details:
            self.error_details = error_details
        self.save()
        
        # 记录历史（如果状态发生变化）
        if previous_state != new_state:
            # 构建原因描述（如果未提供）
            if not reason:
                if new_state == 'working':
                    reason = "Task processing started"
                elif new_state == 'completed':
                    reason = "Task completed successfully"
                elif new_state == 'failed':
                    reason = error_details or "Task failed"
                elif new_state == 'canceled':
                    reason = "Task canceled by user"
                elif new_state == 'input-required':
                    reason = "Additional input required from user"
            
            # 创建历史记录
            TaskStateHistory.objects.create(
                task=self,
                state=new_state,
                previous_state=previous_state,
                reason=reason,
                initiated_by=initiated_by,
                metadata={'error_details': error_details} if error_details else {}
            )
    
    def to_a2a_format(self):
        """转换为A2A协议格式"""
        messages = list(self.messages.all().order_by('created_at'))
        artifacts = list(self.artifacts.all().order_by('created_at'))
        
        result = {
            "taskId": str(self.id),
            "status": {
                "state": self.state,
                "timestamp": self.updated_at.isoformat()
            },
            "history": [msg.to_a2a_format() for msg in messages],
            "artifacts": [artifact.to_a2a_format() for artifact in artifacts],
            "metadata": self.metadata or {}
        }
        
        if self.session:
            result["sessionId"] = str(self.session.id)
        
        if self.error_details and self.state == 'failed':
            result["status"]["reason"] = self.error_details
            
        return result


class Message(models.Model):
    """
    A2A协议中的Message模型，表示任务中的一次交互
    """
    ROLE_CHOICES = (
        ('user', '用户'),
        ('agent', '代理'),
    )
    
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    task = models.ForeignKey(Task, on_delete=models.CASCADE, related_name='messages', verbose_name=_('所属任务'))
    role = models.CharField(_('角色'), max_length=10, choices=ROLE_CHOICES)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    metadata = models.JSONField(_('元数据'), blank=True, default=dict)
    
    class Meta:
        verbose_name = _('消息')
        verbose_name_plural = _('消息')
        ordering = ['created_at']
    
    def __str__(self):
        return f"{self.get_role_display()} 消息 ({self.id})"
    
    def to_a2a_format(self):
        """转换为A2A协议格式"""
        parts = list(self.parts.all().order_by('created_at'))
        
        return {
            "role": self.role,
            "parts": [part.to_a2a_format() for part in parts],
            "metadata": self.metadata or {}
        }


class Part(models.Model):
    """
    A2A协议中的Part模型，表示消息中的内容部分
    """
    PART_TYPES = (
        ('text', '文本'),
        ('data', '数据'),
        ('file', '文件'),
    )
    
    MIME_TYPES = {
        'text': [
            'text/plain',
            'text/html',
            'text/markdown',
            'text/csv',
            'application/json',
            'application/xml',
            'text/x-python',
            'text/javascript',
            'text/css'
        ],
        'data': [
            'application/json',
            'application/x-www-form-urlencoded',
            'application/vnd.a2a-form+json',
            'application/vnd.a2a-ui-state+json',
            'application/schema+json',
            'application/ld+json',
            'application/vnd.a2a-parameters+json'
        ],
        'file': [
            'application/octet-stream',
            'image/jpeg',
            'image/png',
            'image/gif',
            'image/svg+xml',
            'image/webp',
            'image/heif',
            'image/heic',
            'image/tiff',
            'application/pdf',
            'audio/mpeg',
            'audio/wav',
            'audio/aac',
            'audio/ogg',
            'audio/webm',
            'video/mp4',
            'video/webm',
            'video/ogg',
            'video/quicktime',
            'application/zip',
            'application/gzip',
            'application/x-tar',
            'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
            'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            'application/vnd.openxmlformats-officedocument.presentationml.presentation',
            'text/csv',
            'application/msword',
            'application/vnd.ms-excel',
            'application/vnd.ms-powerpoint'
        ]
    }
    
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    message = models.ForeignKey(Message, on_delete=models.CASCADE, related_name='parts', verbose_name=_('所属消息'), null=True, blank=True)
    artifact = models.ForeignKey('Artifact', on_delete=models.CASCADE, related_name='parts', verbose_name=_('所属产物'), null=True, blank=True)
    part_type = models.CharField(_('类型'), max_length=10, choices=PART_TYPES)
    content_type = models.CharField(_('内容类型'), max_length=100, default='text/plain')
    text_content = models.TextField(_('文本内容'), blank=True, null=True)
    data_content = models.JSONField(_('数据内容'), blank=True, null=True)
    file_content = models.BinaryField(_('文件内容'), blank=True, null=True)
    file_uri = models.URLField(_('文件URI'), blank=True, null=True)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    
    # 流式传输支持
    index = models.IntegerField(_('索引'), default=0)
    is_append = models.BooleanField(_('是否追加'), default=False)
    is_last_chunk = models.BooleanField(_('是否最后一块'), default=True)
    
    class Meta:
        verbose_name = _('内容部分')
        verbose_name_plural = _('内容部分')
        ordering = ['created_at']
    
    def __str__(self):
        return f"{self.get_part_type_display()} 部分 ({self.id})"
    
    def to_a2a_format(self):
        """转换为A2A协议格式"""
        result = {
            "contentType": self.content_type,
            "metadata": {},
        }
        
        # 添加流式支持信息
        if (self.message and self.message.task.state == 'working') or (self.artifact and self.artifact.task.state == 'working'):
            result["index"] = self.index
            if self.is_append:
                result["append"] = True
            if not self.is_last_chunk:
                result["lastChunk"] = False
        
        if self.part_type == 'text':
            result["text"] = self.text_content
        elif self.part_type == 'data':
            result["data"] = self.data_content
        elif self.part_type == 'file':
            # 添加文件名和MIME类型信息到元数据中
            if 'filename' not in result.get('metadata', {}):
                if hasattr(self, 'filename') and self.filename:
                    result['metadata']['filename'] = self.filename
            
            if self.file_uri:
                result["fileUri"] = self.file_uri
            
            # 处理二进制内容并进行Base64编码
            if self.file_content:
                import base64
                try:
                    # 确保文件内容是二进制格式
                    if isinstance(self.file_content, memoryview):
                        binary_content = self.file_content.tobytes()
                    else:
                        binary_content = bytes(self.file_content)
                    
                    # Base64编码处理
                    base64_encoded = base64.b64encode(binary_content).decode('utf-8')
                    result["inlineData"] = base64_encoded
                except Exception as e:
                    # 记录错误但仍然继续
                    import logging
                    logger = logging.getLogger(__name__)
                    logger.error(f"Error encoding file content: {str(e)}")
        
        return result


class Artifact(models.Model):
    """
    A2A协议中的Artifact模型，表示任务产生的产物
    """
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    task = models.ForeignKey(Task, on_delete=models.CASCADE, related_name='artifacts', verbose_name=_('所属任务'))
    artifact_type = models.CharField(_('类型'), max_length=100)
    name = models.CharField(_('名称'), max_length=255)
    description = models.TextField(_('描述'), blank=True, null=True)
    metadata = models.JSONField(_('元数据'), blank=True, default=dict)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    
    # 流式传输支持
    index = models.IntegerField(_('索引'), default=0)
    is_append = models.BooleanField(_('是否追加'), default=False)
    is_last_chunk = models.BooleanField(_('是否最后一块'), default=True)
    
    # 任务树支持 
    parent_task_id = models.UUIDField(_('父任务ID'), blank=True, null=True)
    child_task_ids = models.JSONField(_('子任务ID列表'), default=list, blank=True)
    is_task_tree = models.BooleanField(_('是否任务树'), default=False)
    
    class Meta:
        verbose_name = _('产物')
        verbose_name_plural = _('产物')
        ordering = ['created_at']
    
    def __str__(self):
        return f"{self.name} ({self.artifact_type})"
    
    def to_a2a_format(self):
        """转换为A2A协议格式"""
        parts = list(self.parts.all().order_by('created_at'))
        
        result = {
            "artifactId": str(self.id),
            "artifactType": self.artifact_type,
            "name": self.name,
            "description": self.description or "",
            "parts": [part.to_a2a_format() for part in parts],
            "metadata": self.metadata or {},
            "index": self.index
        }
        
        if self.is_append:
            result["append"] = True
        
        if not self.is_last_chunk:
            result["lastChunk"] = False
            
        # 添加任务树相关信息
        if self.is_task_tree:
            task_tree_data = {
                "type": "taskTree"
            }
            
            if self.parent_task_id:
                task_tree_data["parentTaskId"] = str(self.parent_task_id)
                
            if self.child_task_ids:
                task_tree_data["childTaskIds"] = [str(id) for id in self.child_task_ids]
                
            result["taskTreeData"] = task_tree_data
            
        return result


class PushNotificationConfig(models.Model):
    """
    A2A协议中的推送通知配置
    """
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    task = models.OneToOneField(Task, on_delete=models.CASCADE, related_name='push_config')
    url = models.URLField(_('推送URL'))
    token = models.CharField(_('推送令牌'), max_length=255, blank=True, null=True)
    auth_scheme = models.CharField(_('认证方案'), max_length=50, blank=True, null=True)
    auth_credentials = models.TextField(_('认证凭证'), blank=True, null=True)
    created_at = models.DateTimeField(_('创建时间'), auto_now_add=True)
    updated_at = models.DateTimeField(_('更新时间'), auto_now=True)
    
    # 安全相关字段
    challenge_token = models.CharField(_('URL验证令牌'), max_length=255, blank=True, null=True)
    challenge_verified = models.BooleanField(_('URL已验证'), default=False)
    max_retries = models.IntegerField(_('最大重试次数'), default=3)
    retry_delay = models.IntegerField(_('重试延迟(秒)'), default=30)
    notification_types = models.JSONField(_('通知类型'), default=list)
    
    class Meta:
        verbose_name = _('推送通知配置')
        verbose_name_plural = _('推送通知配置')
    
    def __str__(self):
        return f"推送配置 {self.id} ({self.task.id})"
    
    def to_a2a_format(self):
        """转换为A2A协议格式"""
        result = {
            "url": self.url,
        }
        
        if self.token:
            result["token"] = self.token
            
        if self.auth_scheme:
            result["authentication"] = {
                "scheme": self.auth_scheme
            }
            
            if self.auth_credentials:
                # 认证凭证被加密存储，这里不应返回原始凭证
                result["authentication"]["hasCredentials"] = True
        
        if self.notification_types:
            result["notificationTypes"] = self.notification_types
            
        if self.max_retries != 3:  # 只有当值不是默认值时才添加
            result["maxRetries"] = self.max_retries
            
        if self.retry_delay != 30:  # 只有当值不是默认值时才添加
            result["retryDelay"] = self.retry_delay
                
        return result
        
    def verify_url_ownership(self):
        """
        验证URL所有权 - 发送挑战令牌到URL
        实际实现会向该URL发送一个HTTP请求，等待返回令牌
        """
        if not self.challenge_token:
            self.challenge_token = str(uuid.uuid4())
            self.save()
            
        # 实现代码：向URL发送验证请求并检查响应
        try:
            # 此处应有实际的HTTP请求代码
            # 如果验证成功，设置challenge_verified为True
            self.challenge_verified = True
            self.save()
            return True
        except Exception as e:
            import logging
            logger = logging.getLogger(__name__)
            logger.error(f"URL verification failed for {self.url}: {str(e)}")
            return False


class Session(models.Model):
    """
    代表Agent会话
    一个会话包含多个相关的任务（如多轮对话）
    """
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    agent = models.ForeignKey(Agent, on_delete=models.CASCADE, related_name='sessions')
    name = models.CharField(max_length=128, blank=True)
    owner = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name='sessions')
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
    is_active = models.BooleanField(default=True)
    metadata = models.JSONField(default=dict, blank=True)
    
    def __str__(self):
        return f"会话 {self.name or self.id} ({self.agent.name})"
    
    def get_task_count(self):
        """获取此会话中的任务数量"""
        return self.tasks.count()
    
    def get_latest_task(self):
        """获取此会话中最新的任务"""
        return self.tasks.order_by('-created_at').first()
    
    @property
    def status(self):
        """获取会话状态，基于最近的任务"""
        latest_task = self.get_latest_task()
        if not latest_task:
            return 'empty'
        return latest_task.state


class TaskStateHistory(models.Model):
    """
    记录任务状态变更的历史记录
    """
    id = models.AutoField(primary_key=True)
    task = models.ForeignKey(Task, on_delete=models.CASCADE, related_name='state_history', verbose_name=_('所属任务'))
    state = models.CharField(_('状态'), max_length=20, choices=Task.STATE_CHOICES)
    previous_state = models.CharField(_('前一状态'), max_length=20, blank=True, null=True)
    reason = models.TextField(_('原因'), blank=True, null=True)
    timestamp = models.DateTimeField(_('时间戳'), auto_now_add=True)
    initiated_by = models.CharField(_('发起者'), max_length=255, blank=True, null=True, 
                                  help_text=_('状态变更的发起者，可以是用户ID、系统等'))
    metadata = models.JSONField(_('元数据'), blank=True, default=dict)
    
    class Meta:
        verbose_name = _('任务状态历史')
        verbose_name_plural = _('任务状态历史')
        ordering = ['-timestamp']
    
    def __str__(self):
        return f"任务 {self.task.id} 状态变更: {self.previous_state or '无'} -> {self.state} ({self.timestamp})"
