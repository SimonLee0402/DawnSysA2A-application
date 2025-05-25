import uuid
from django.db import models
from django.utils.translation import gettext_lazy as _
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