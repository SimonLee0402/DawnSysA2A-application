from rest_framework import serializers
from workflow.models import Workflow, WorkflowInstance, WorkflowStep


class WorkflowSerializer(serializers.ModelSerializer):
    """工作流序列化器"""
    user_name = serializers.SerializerMethodField()
    is_owner = serializers.SerializerMethodField()
    can_edit = serializers.SerializerMethodField()
    can_delete = serializers.SerializerMethodField()
    
    class Meta:
        model = Workflow
        fields = [
            'id', 'name', 'description', 'workflow_type', 'is_public',
            'definition', 'created_by', 'user_name', 'created_at', 'updated_at',
            'is_owner', 'can_edit', 'can_delete', 'version'
        ]
        read_only_fields = [
            'id',
            'created_by',
            'user_name',
            'created_at',
            'updated_at',
            'is_owner',
            'can_edit',
            'can_delete',
            'version'
        ]
    
    def get_user_name(self, obj):
        return obj.created_by.username if obj.created_by else None
    
    def get_is_owner(self, obj):
        request = self.context.get('request')
        if request and hasattr(request, 'user') and request.user.is_authenticated:
            return obj.created_by == request.user
        return False
    
    def get_can_edit(self, obj):
        request = self.context.get('request')
        if request and hasattr(request, 'user') and request.user.is_authenticated:
            if request.user.is_superuser:
                return True
            return obj.created_by == request.user
        return False
    
    def get_can_delete(self, obj):
        request = self.context.get('request')
        if request and hasattr(request, 'user') and request.user.is_authenticated:
            if request.user.is_superuser:
                return True
            return obj.created_by == request.user
        return False

    def create(self, validated_data):
        # validated_data['created_by'] = self.context['request'].user
        # return super().create(validated_data)

        # 明确指定哪些字段用于创建
        workflow_data = {
            'name': validated_data.get('name'),
            'description': validated_data.get('description'),
            'definition': validated_data.get('definition'),
            'workflow_type': validated_data.get('workflow_type', 'standard'),
            'is_public': validated_data.get('is_public', False),
            'tags': validated_data.get('tags', []),
            # version 字段在模型中有 default=1，所以创建时不需要提供，除非想覆盖
        }
        
        workflow_data['created_by'] = self.context['request'].user
        
        # 确保 definition 是存在的 (模型中是非空JSONField)
        if workflow_data.get('definition') is None:
            # 根据业务逻辑，可以抛出 serializers.ValidationError 或提供默认值
            # serializers.ValidationError({"definition": "This field is required."})
            workflow_data['definition'] = {} # 默认空JSON，如果允许

        return Workflow.objects.create(**workflow_data)


class WorkflowStepSerializer(serializers.ModelSerializer):
    """工作流步骤序列化器"""
    class Meta:
        model = WorkflowStep
        fields = [
            'id', 'workflow_instance', 'step_id', 'step_type', 'name',
            'status', 'started_at', 'completed_at', 'input_data',
            'output_data', 'error_message'
        ]
        read_only_fields = fields


class WorkflowInstanceSerializer(serializers.ModelSerializer):
    """工作流实例序列化器"""
    workflow_name = serializers.SerializerMethodField()
    started_by_username = serializers.SerializerMethodField()
    steps = WorkflowStepSerializer(many=True, read_only=True, source='workflowstep_set')
    
    class Meta:
        model = WorkflowInstance
        fields = [
            'instance_id', 'workflow', 'workflow_name', 'started_by', 'started_by_username',
            'status', 'started_at', 'completed_at',
            'context',
            'output',
            'error',
            'steps',
            'created_at', 'updated_at'
        ]
        read_only_fields = fields
    
    def get_workflow_name(self, obj):
        return obj.workflow.name if obj.workflow else None
    
    def get_started_by_username(self, obj):
        return obj.started_by.username if obj.started_by else None 