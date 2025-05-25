from django.db import models
from django.conf import settings # For ForeignKey to User
import uuid
from django.contrib.auth import get_user_model
from django.utils.translation import gettext_lazy as _

User = get_user_model()

class VisibilityChoices(models.TextChoices):
    PUBLIC = 'PUBLIC', _('公开')
    PRIVATE = 'PRIVATE', _('私有 (仅限同一所有者的智能体)')

class KnowledgeBase(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    name = models.CharField(max_length=255, unique=True)
    description = models.TextField(blank=True, null=True)
    owner = models.ForeignKey(User, on_delete=models.CASCADE, related_name='knowledge_bases')
    visibility = models.CharField(
        max_length=20,
        choices=VisibilityChoices.choices,
        default=VisibilityChoices.PRIVATE,
        help_text=_('控制知识库的可见性和访问权限')
    )
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        ordering = ['-created_at']
        verbose_name = "Knowledge Base"
        verbose_name_plural = "Knowledge Bases"

    def __str__(self):
        return self.name

class Document(models.Model):
    class ProcessingStatus(models.TextChoices):
        PENDING = 'PENDING', _('待处理')
        PROCESSING = 'PROCESSING', _('处理中')
        COMPLETED = 'COMPLETED', _('成功')
        FAILED = 'FAILED', _('失败')

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    knowledge_base = models.ForeignKey(KnowledgeBase, on_delete=models.CASCADE, related_name='documents')
    file_name = models.CharField(max_length=255, help_text=_("原始文件名或用户指定的名称"))
    file_type = models.CharField(max_length=50, blank=True, help_text=_("文件类型/扩展名, e.g., pdf, txt, docx"))
    file_size = models.PositiveIntegerField(blank=True, null=True, help_text=_("文件大小 (bytes)"))
    original_file = models.FileField(upload_to='knowledge_base_documents/%Y/%m/%d/', null=True, blank=True, help_text=_("上传的原始文件"))
    extracted_text = models.TextField(blank=True, null=True, help_text=_("从文档中提取的文本内容"))
    uploaded_at = models.DateTimeField(auto_now_add=True)
    status = models.CharField(
        max_length=20,
        choices=ProcessingStatus.choices,
        default=ProcessingStatus.PENDING,
        help_text=_('文档处理状态')
    )
    error_message = models.TextField(blank=True, null=True, help_text=_("如果处理失败，记录错误信息"))
    processed_at = models.DateTimeField(null=True, blank=True, help_text=_("文档处理完成的时间"))
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        ordering = ['-uploaded_at']
        verbose_name = "Document"
        verbose_name_plural = "Documents"

    def __str__(self):
        return f"{self.file_name} in {self.knowledge_base.name}"

    def save(self, *args, **kwargs):
        if not self.file_size and self.original_file:
            self.file_size = self.original_file.size
        super().save(*args, **kwargs) 