from django.contrib import admin
from .models import Agent, AgentCredential, Task, Message, Part, Artifact

class AgentCredentialInline(admin.StackedInline):
    model = AgentCredential
    fields = ('api_endpoint', 'additional_params')
    readonly_fields = ('created_at', 'updated_at')
    extra = 0
    can_delete = False  # 不允许直接删除凭证

@admin.register(Agent)
class AgentAdmin(admin.ModelAdmin):
    list_display = ('name', 'agent_type', 'model_name', 'is_active', 'owner', 'created_at')
    list_filter = ('agent_type', 'is_active')
    search_fields = ('name', 'description')
    readonly_fields = ('created_at', 'updated_at')
    inlines = [AgentCredentialInline]

@admin.register(AgentCredential)
class AgentCredentialAdmin(admin.ModelAdmin):
    list_display = ('agent', 'api_endpoint', 'created_at', 'updated_at')
    search_fields = ('agent__name',)
    readonly_fields = ('created_at', 'updated_at')
    
    def get_readonly_fields(self, request, obj=None):
        # 创建新记录时可以设置api_key，编辑现有记录时api_key为只读
        if obj:
            return ('agent', 'api_key', 'created_at', 'updated_at')
        return ('created_at', 'updated_at')

class PartInline(admin.TabularInline):
    model = Part
    extra = 0
    readonly_fields = ('created_at',)
    fields = ('part_type', 'content_type', 'text_content', 'created_at')

@admin.register(Message)
class MessageAdmin(admin.ModelAdmin):
    list_display = ('task', 'role', 'created_at')
    list_filter = ('role',)
    search_fields = ('task__id',)
    readonly_fields = ('created_at',)
    inlines = [PartInline]

class MessageInline(admin.TabularInline):
    model = Message
    extra = 0
    readonly_fields = ('created_at',)
    fields = ('role', 'created_at')

class ArtifactInline(admin.TabularInline):
    model = Artifact
    extra = 0
    readonly_fields = ('created_at',)
    fields = ('name', 'artifact_type', 'created_at')

@admin.register(Task)
class TaskAdmin(admin.ModelAdmin):
    list_display = ('id', 'agent', 'state', 'created_at', 'updated_at', 'completed_at')
    list_filter = ('state',)
    search_fields = ('id', 'agent__name')
    readonly_fields = ('id', 'created_at', 'updated_at', 'completed_at')
    inlines = [MessageInline, ArtifactInline]

@admin.register(Artifact)
class ArtifactAdmin(admin.ModelAdmin):
    list_display = ('name', 'artifact_type', 'task', 'created_at')
    list_filter = ('artifact_type',)
    search_fields = ('name', 'task__id')
    readonly_fields = ('created_at',)
    inlines = [PartInline]
