from django.db import models
from django.contrib.auth.models import AbstractUser
from django.utils.translation import gettext_lazy as _

class User(AbstractUser):
    """
    自定义用户模型，添加了用户类型以区分企业用户和主播用户
    """
    USER_TYPE_CHOICES = (
        ('enterprise', _('企业用户')),
        ('streamer', _('网络主播')),
        ('admin', _('管理员')),
    )
    
    user_type = models.CharField(
        max_length=20, 
        choices=USER_TYPE_CHOICES,
        default='enterprise',
        verbose_name=_('用户类型')
    )
    
    company_name = models.CharField(
        max_length=255, 
        blank=True, 
        null=True,
        verbose_name=_('公司名称')
    )
    
    channel_name = models.CharField(
        max_length=255, 
        blank=True, 
        null=True,
        verbose_name=_('频道名称')
    )
    
    platform = models.CharField(
        max_length=100, 
        blank=True, 
        null=True,
        verbose_name=_('所属平台')
    )
    
    # 可以添加其他字段，例如头像、联系方式等
    
    class Meta:
        verbose_name = _('用户')
        verbose_name_plural = _('用户')
        
    def __str__(self):
        return self.username
    
    @property
    def is_enterprise(self):
        return self.user_type == 'enterprise'
    
    @property
    def is_streamer(self):
        return self.user_type == 'streamer'
    
    @property
    def display_name(self):
        if self.is_enterprise and self.company_name:
            return self.company_name
        elif self.is_streamer and self.channel_name:
            return self.channel_name
        else:
            return self.username


class UserProfile(models.Model):
    """
    用户额外信息，可以根据需要扩展
    """
    user = models.OneToOneField(
        User, 
        on_delete=models.CASCADE,
        related_name='profile',
        verbose_name=_('用户')
    )
    
    # 更多用户相关字段可以添加在这里
    
    class Meta:
        verbose_name = _('用户资料')
        verbose_name_plural = _('用户资料')
        
    def __str__(self):
        return f"{self.user.username}的资料"
