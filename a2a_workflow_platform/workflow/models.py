import uuid
from django.db import models
from django.utils.translation import gettext_lazy as _
from users.models import User
from django.contrib.auth.models import User
from django.utils import timezone
import json
from django.conf import settings

class Workflow(models.Model):
    """
    工作流定义，代表一个完整的工作流模板，可以被多次实例化
    """
    id = models.AutoField(primary_key=True)
    name = models.CharField(max_length=255, verbose_name=_('工作流名称'))
    description = models.TextField(blank=True, null=True, verbose_name=_('描述'))
    
    # 工作流定义格式，使用JSON存储
    definition = models.JSONField(verbose_name=_('工作流定义'))
    
    # 类型用于区分不同的工作流类别（例如，企业工作流、主播工作流等）
    workflow_type = models.CharField(
        max_length=50, 
        default='standard',
        verbose_name=_('工作流类型')
    )
    
    # 是否公开，公开的工作流可以被其他用户使用
    is_public = models.BooleanField(default=False, verbose_name=_('是否公开'))
    
    # 标签，用于搜索和分类
    tags = models.JSONField(default=list, blank=True, verbose_name=_('标签'))
    
    # 创建者
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL, 
        on_delete=models.CASCADE,
        related_name='created_workflows',
        verbose_name=_('创建者')
    )
    
    # 时间信息
    created_at = models.DateTimeField(auto_now_add=True, verbose_name=_('创建时间'))
    updated_at = models.DateTimeField(auto_now=True, verbose_name=_('更新时间'))
    
    version = models.IntegerField(default=1)
    
    class Meta:
        verbose_name = _('工作流')
        verbose_name_plural = _('工作流')
        ordering = ['-created_at']
    
    def __str__(self):
        return self.name


class WorkflowInstance(models.Model):
    """
    工作流实例，代表一个工作流的执行
    """
    instance_id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False, verbose_name=_('实例ID'))
    
    # 关联的工作流
    workflow = models.ForeignKey(
        Workflow, 
        on_delete=models.CASCADE,
        related_name='instances',
        verbose_name=_('工作流')
    )
    
    # 实例名称，可以有自己的名称，默认使用工作流名称
    name = models.CharField(max_length=255, blank=True, null=True, verbose_name=_('实例名称'))
    
    # 启动者
    started_by = models.ForeignKey(
        settings.AUTH_USER_MODEL, 
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name='started_workflow_instances',
        verbose_name=_('启动者')
    )
    
    # 当前状态
    STATUS_CHOICES = [
        ('created', _('已创建')),
        ('running', _('运行中')),
        ('paused', _('已暂停')),
        ('completed', _('已完成')),
        ('failed', _('失败')),
        ('canceled', _('已取消')),
    ]
    status = models.CharField(
        max_length=20, 
        choices=STATUS_CHOICES,
        default='created',
        verbose_name=_('状态')
    )
    
    # 当前步骤索引
    current_step_index = models.IntegerField(default=0, verbose_name=_('当前步骤索引'))
    
    # 实例执行的上下文数据，保存整个执行过程中的变量和状态
    context = models.JSONField(blank=True, null=True, verbose_name=_('上下文数据'))
    
    # 完成时的输出数据
    output = models.JSONField(blank=True, null=True, verbose_name=_('输出数据'))
    
    # 失败时的错误信息
    error = models.TextField(blank=True, null=True, verbose_name=_('错误信息'))
    
    # 时间信息
    created_at = models.DateTimeField(auto_now_add=True, verbose_name=_('创建时间'))
    started_at = models.DateTimeField(null=True, blank=True, verbose_name=_('开始时间'))
    updated_at = models.DateTimeField(auto_now=True, verbose_name=_('更新时间'))
    completed_at = models.DateTimeField(null=True, blank=True, verbose_name=_('完成时间'))
    
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL, 
        on_delete=models.CASCADE,
        related_name='workflow_instances',
        verbose_name=_('创建者'),
        null=True,
        blank=True
    )
    
    class Meta:
        verbose_name = _('工作流实例')
        verbose_name_plural = _('工作流实例')
        ordering = ['-created_at']
    
    def __str__(self):
        return f"{self.workflow.name} - {self.name or self.instance_id}"
    
    @property
    def display_name(self):
        return self.name or f"{self.workflow.name} #{str(self.instance_id)[:8]}"


class WorkflowStep(models.Model):
    """
    工作流步骤执行记录，记录每个实例的步骤执行情况
    """
    # 实例
    instance = models.ForeignKey(
        WorkflowInstance, 
        on_delete=models.CASCADE,
        related_name='steps',
        verbose_name=_('工作流实例')
    )
    
    # 步骤索引和名称
    step_index = models.IntegerField(verbose_name=_('步骤索引'))
    step_id = models.CharField(max_length=255, verbose_name=_('步骤ID'))
    step_name = models.CharField(max_length=255, verbose_name=_('步骤名称'))
    
    # 步骤类型，例如 a2a_client、condition、loop 等
    step_type = models.CharField(max_length=50, verbose_name=_('步骤类型'))
    
    # 步骤参数，保存调用时的参数
    parameters = models.JSONField(default=dict, verbose_name=_('参数'))
    
    # 步骤状态
    STATUS_CHOICES = (
        ('pending', _('待执行')),
        ('running', _('执行中')),
        ('completed', _('已完成')),
        ('failed', _('失败')),
        ('skipped', _('已跳过')),
    )
    status = models.CharField(
        max_length=20, 
        choices=STATUS_CHOICES,
        default='pending',
        verbose_name=_('状态')
    )
    
    # 步骤输入输出
    input_data = models.JSONField(null=True, blank=True, verbose_name=_('输入数据'))
    output_data = models.JSONField(null=True, blank=True, verbose_name=_('输出数据'))
    
    # 错误信息
    error = models.TextField(null=True, blank=True, verbose_name=_('错误信息'))
    
    # 时间信息
    started_at = models.DateTimeField(null=True, blank=True, verbose_name=_('开始时间'))
    completed_at = models.DateTimeField(null=True, blank=True, verbose_name=_('完成时间'))
    
    # 任务ID和状态，用于A2A调用的关联
    a2a_task_id = models.CharField(max_length=255, null=True, blank=True, verbose_name=_('A2A任务ID'))
    a2a_task_status = models.CharField(max_length=50, null=True, blank=True, verbose_name=_('A2A任务状态'))
    
    class Meta:
        verbose_name = _('工作流步骤')
        verbose_name_plural = _('工作流步骤')
        ordering = ['instance', 'step_index']
        unique_together = [['instance', 'step_index']]
    
    def __str__(self):
        return f"{self.instance.display_name} - {self.step_name} ({self.step_index})"


class A2AAgent(models.Model):
    """
    已知的A2A代理（Agent）配置
    """
    id = models.AutoField(primary_key=True)
    name = models.CharField(max_length=100, verbose_name=_('代理名称'))
    description = models.TextField(blank=True, null=True, verbose_name=_('描述'))
    
    # 代理的endpoint URL
    endpoint_url = models.URLField(verbose_name=_('端点URL'))
    
    # Agent Card URL或缓存的Agent Card内容
    agent_card_url = models.URLField(null=True, blank=True, verbose_name=_('Agent Card URL'))
    agent_card_content = models.JSONField(null=True, blank=True, verbose_name=_('Agent Card内容'))
    
    # 认证信息（加密存储）
    auth_type = models.CharField(max_length=50, null=True, blank=True, verbose_name=_('认证类型'))
    auth_config = models.JSONField(null=True, blank=True, verbose_name=_('认证配置'))
    
    # 可见性和访问控制
    is_public = models.BooleanField(default=False, verbose_name=_('是否公开'))
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL, 
        on_delete=models.CASCADE,
        related_name='a2a_agents',
        verbose_name=_('创建者')
    )
    
    # 最后检查Agent Card的时间
    last_checked_at = models.DateTimeField(null=True, blank=True, verbose_name=_('最后检查时间'))
    
    # 代理状态
    is_active = models.BooleanField(default=True, verbose_name=_('是否活跃'))
    
    # 时间信息
    created_at = models.DateTimeField(auto_now_add=True, verbose_name=_('创建时间'))
    updated_at = models.DateTimeField(auto_now=True, verbose_name=_('更新时间'))
    
    class Meta:
        verbose_name = _('A2A代理')
        verbose_name_plural = _('A2A代理')
        ordering = ['-created_at']
    
    def __str__(self):
        return self.name


class WorkflowLog(models.Model):
    """工作流执行日志模型"""
    id = models.AutoField(primary_key=True)
    instance = models.ForeignKey('WorkflowInstance', on_delete=models.CASCADE, related_name='logs')
    step = models.ForeignKey('WorkflowStep', on_delete=models.SET_NULL, null=True, blank=True, related_name='logs')
    
    LOG_LEVEL_CHOICES = [
        ('info', '信息'),
        ('warning', '警告'),
        ('error', '错误'),
        ('debug', '调试'),
    ]
    level = models.CharField(max_length=10, choices=LOG_LEVEL_CHOICES, default='info')
    message = models.TextField()
    details = models.JSONField(null=True, blank=True)
    
    timestamp = models.DateTimeField(auto_now_add=True)
    
    class Meta:
        ordering = ['-timestamp']
        verbose_name = '工作流日志'
        verbose_name_plural = '工作流日志'
    
    def __str__(self):
        return f"{self.get_level_display()}: {self.message[:50]}"
