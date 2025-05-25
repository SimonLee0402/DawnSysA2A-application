# Admin configurations for agents app 
from django.contrib import admin
from .models import Agent, AgentSkill

@admin.register(Agent)
class AgentAdmin(admin.ModelAdmin):
    list_display = ('name', 'service_url', 'created_by', 'created_at', 'updated_at')
    search_fields = ('name', 'description', 'service_url')
    list_filter = ('created_at', 'updated_at')
    readonly_fields = ('id', 'created_at', 'updated_at')
    fieldsets = (
        (None, {
            'fields': ('id', 'name', 'description', 'service_url', 'a2a_version')
        }),
        ('Provider & Capabilities', {
            'fields': ('provider_info', 'capabilities')
        }),
        ('Authentication', {
            'fields': ('authentication_schemes',)
        }),
        ('Meta', {
            'fields': ('created_by', 'created_at', 'updated_at')
        }),
    )

class AgentSkillInline(admin.TabularInline):
    model = AgentSkill
    extra = 1
    fields = ('skill_id', 'name', 'description', 'input_modes', 'output_modes', 'examples')
    readonly_fields = ('id',)

# 如果希望在AgentAdmin中直接编辑skills，取消上面的AgentAdmin注册，并使用下面的
# @admin.register(Agent)
# class AgentAdminWithSkills(admin.ModelAdmin):
#     list_display = ('name', 'service_url', 'created_by', 'created_at', 'updated_at')
#     search_fields = ('name', 'description', 'service_url')
#     list_filter = ('created_at', 'updated_at')
#     readonly_fields = ('id', 'created_at', 'updated_at')
#     fieldsets = (
#         (None, {
#             'fields': ('id', 'name', 'description', 'service_url', 'a2a_version')
#         }),
#         ('Provider & Capabilities', {
#             'fields': ('provider_info', 'capabilities')
#         }),
#         ('Authentication', {
#             'fields': ('authentication_schemes',)
#         }),
#         ('Meta', {
#             'fields': ('created_by', 'created_at', 'updated_at')
#         }),
#     )
#     inlines = [AgentSkillInline]

@admin.register(AgentSkill)
class AgentSkillAdmin(admin.ModelAdmin):
    list_display = ('name', 'agent', 'skill_id', 'created_at')
    search_fields = ('name', 'skill_id', 'agent__name')
    list_filter = ('agent', 'created_at')
    readonly_fields = ('id', 'created_at', 'updated_at')
    autocomplete_fields = ('agent',) 