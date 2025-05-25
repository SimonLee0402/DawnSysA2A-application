from rest_framework import serializers
from .models import Workflow, WorkflowInstance, WorkflowStep, A2AAgent
from users.serializers import UserSerializer

class A2AAgentSerializer(serializers.ModelSerializer):
    """A2A代理序列化器"""
    
    class Meta:
        model = A2AAgent
        fields = ['id', 'name', 'description', 'agent_type', 'configuration', 
                  'status', 'created_at', 'updated_at', 'created_by']
        read_only_fields = ['id', 'created_at', 'updated_at', 'created_by']


class WorkflowStepSerializer(serializers.ModelSerializer):
    """工作流步骤序列化器"""
    
    agent = A2AAgentSerializer(read_only=True)
    agent_id = serializers.PrimaryKeyRelatedField(
        queryset=A2AAgent.objects.all(),
        write_only=True,
        source='agent'
    )
    
    class Meta:
        model = WorkflowStep
        fields = ['id', 'workflow', 'step_number', 'name', 'description', 
                  'agent', 'agent_id', 'input_schema', 'output_schema', 
                  'created_at', 'updated_at']
        read_only_fields = ['id', 'created_at', 'updated_at']


class WorkflowSerializer(serializers.ModelSerializer):
    """工作流序列化器"""
    
    steps = WorkflowStepSerializer(many=True, read_only=True)
    created_by_username = serializers.SerializerMethodField()
    
    class Meta:
        model = Workflow
        fields = ['id', 'name', 'description', 'version', 'status', 
                  'input_schema', 'output_schema', 'steps', 
                  'created_at', 'updated_at', 'created_by', 'created_by_username']
        read_only_fields = ['id', 'created_at', 'updated_at', 'created_by', 'created_by_username']
    
    def get_created_by_username(self, obj):
        return obj.created_by.username if obj.created_by else None


class WorkflowInstanceSerializer(serializers.ModelSerializer):
    """工作流实例序列化器"""
    
    workflow_name = serializers.SerializerMethodField()
    started_by_username = serializers.SerializerMethodField()
    
    class Meta:
        model = WorkflowInstance
        fields = ['id', 'workflow', 'workflow_name', 'status', 'input_data', 
                 'output_data', 'error_message', 'started_at', 'completed_at', 
                 'started_by', 'started_by_username']
        read_only_fields = ['id', 'status', 'output_data', 'error_message', 
                           'started_at', 'completed_at', 'started_by', 
                           'started_by_username', 'workflow_name']
    
    def get_workflow_name(self, obj):
        return obj.workflow.name if obj.workflow else None
    
    def get_started_by_username(self, obj):
        return obj.started_by.username if obj.started_by else None


class WorkflowStepInstanceSerializer(serializers.ModelSerializer):
    """工作流步骤实例序列化器"""
    
    step_name = serializers.SerializerMethodField()
    agent_name = serializers.SerializerMethodField()
    
    class Meta:
        model = WorkflowStep
        fields = ['id', 'workflow_instance', 'step', 'step_name', 'agent', 
                 'agent_name', 'status', 'input_data', 'output_data', 
                 'error_message', 'started_at', 'completed_at']
        read_only_fields = ['id', 'workflow_instance', 'step', 'step_name', 
                           'agent', 'agent_name', 'status', 'input_data', 
                           'output_data', 'error_message', 'started_at', 
                           'completed_at']
    
    def get_step_name(self, obj):
        return obj.step.name if obj.step else None
    
    def get_agent_name(self, obj):
        return obj.agent.name if obj.agent else None


class WorkflowSerializer(serializers.ModelSerializer):
    created_by = UserSerializer(read_only=True)
    
    class Meta:
        model = Workflow
        fields = '__all__'
        read_only_fields = ['id', 'created_at', 'updated_at', 'created_by']
    
    def validate_definition(self, value):
        """
        验证工作流定义的格式是否正确，实际实现中应该有更严格的校验
        """
        # 检查基本结构
        if not isinstance(value, dict):
            raise serializers.ValidationError("工作流定义必须是一个JSON对象")
            
        # 检查是否存在必要的字段
        if 'steps' not in value or not isinstance(value['steps'], list):
            raise serializers.ValidationError("工作流定义必须包含steps字段，且为数组")
            
        # 可以在这里添加更多的验证规则，例如：
        # - 检查每个步骤是否包含必要的字段（id, name, type等）
        # - 检查步骤之间的连接是否正确
        # - 检查条件分支是否有效
        # - 等等
            
        return value
    
    def create(self, validated_data):
        # 设置创建者为当前用户
        validated_data['created_by'] = self.context['request'].user
        return super().create(validated_data) 