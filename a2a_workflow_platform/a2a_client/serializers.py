from rest_framework import serializers
from .models import Agent, AgentCredential, AgentSkill, PushNotificationConfig, Task, Session


class AgentCredentialSerializer(serializers.ModelSerializer):
    """Agent凭证序列化器"""
    api_key_masked = serializers.SerializerMethodField()
    
    class Meta:
        model = AgentCredential
        fields = ['id', 'agent', 'api_key_masked', 'api_endpoint', 
                  'additional_params', 'a2a_auth_type', 'a2a_auth_config',
                  'created_at', 'updated_at']
        read_only_fields = ['id', 'created_at', 'updated_at', 'api_key_masked']
    
    def get_api_key_masked(self, obj):
        """返回掩码后的API密钥"""
        if obj.api_key:
            # 显示前4后4个字符，中间用*代替
            key = obj.get_api_key()
            if len(key) <= 8:
                return "*" * len(key)
            return key[:4] + "*" * (len(key) - 8) + key[-4:]
        return ""


class AgentSerializer(serializers.ModelSerializer):
    """Agent模型序列化器"""
    credential = serializers.SerializerMethodField()
    skills_count = serializers.SerializerMethodField()
    
    class Meta:
        model = Agent
        fields = ['id', 'name', 'description', 'agent_type', 'model_name', 'is_active', 
                  'is_a2a_compliant', 'a2a_endpoint_url', 'a2a_version',
                  'created_at', 'updated_at', 'owner',
                  'credential', 'skills_count']
        read_only_fields = ['id', 'created_at', 'updated_at']
    
    def get_credential(self, obj):
        """获取Agent凭证信息，隐藏API密钥"""
        try:
            credential = obj.credential
            return {
                'id': credential.id,
                'api_endpoint': credential.api_endpoint,
                'has_api_key': bool(credential.api_key),
            }
        except Exception:
            return None
    
    def get_skills_count(self, obj):
        """获取Agent技能数量"""
        return obj.skills.count() if hasattr(obj, 'skills') else 0


class AgentCreateSerializer(serializers.ModelSerializer):
    """用于创建Agent的序列化器，包含凭证信息"""
    api_key = serializers.CharField(write_only=True, required=True)
    api_endpoint = serializers.URLField(write_only=True, required=False, allow_blank=True)
    additional_params = serializers.JSONField(write_only=True, required=False, default=dict)
    
    class Meta:
        model = Agent
        fields = ['name', 'description', 'agent_type', 'model_name', 'is_active', 
                  'is_a2a_compliant', 'a2a_endpoint_url', 'a2a_version',
                  'api_key', 'api_endpoint', 'additional_params']
    
    def create(self, validated_data):
        # 提取凭证相关字段
        api_key = validated_data.pop('api_key')
        api_endpoint = validated_data.pop('api_endpoint', '')
        additional_params = validated_data.pop('additional_params', {})
        
        # 创建Agent
        validated_data['owner'] = self.context['request'].user
        agent = Agent.objects.create(**validated_data)
        
        # 创建AgentCredential
        AgentCredential.objects.create(
            agent=agent,
            api_key=api_key,
            api_endpoint=api_endpoint,
            additional_params=additional_params
        )
        
        return agent


class AgentSkillSerializer(serializers.ModelSerializer):
    """Agent技能序列化器"""
    
    class Meta:
        model = AgentSkill
        fields = ['id', 'agent', 'skill_id', 'name', 'description',
                  'input_modes', 'output_modes', 'examples', 'parameters',
                  'created_at', 'updated_at']
        read_only_fields = ['id', 'created_at', 'updated_at']
    
    def create(self, validated_data):
        # 确保技能ID的唯一性，如果未提供则自动生成
        if not validated_data.get('skill_id'):
            import uuid
            # 生成基于代理和技能名称的唯一ID
            agent = validated_data.get('agent')
            name = validated_data.get('name', '')
            skill_id = f"{agent.agent_type}_{name.lower().replace(' ', '_')}_{uuid.uuid4().hex[:8]}"
            validated_data['skill_id'] = skill_id
        
        return super().create(validated_data)


class TaskSerializer(serializers.ModelSerializer):
    """任务序列化器"""
    class Meta:
        model = Task
        # 初始可以选择所有字段，后续根据需要精简
        fields = '__all__' 
        # agent 通常从URL获取并自动设置，在序列化器中设为只读，避免通过请求体修改
        # 对于创建操作，我们会在视图中处理agent的赋值
        read_only_fields = ('id', 'created_at', 'updated_at', 'completed_at', 'agent')


class PushNotificationConfigSerializer(serializers.ModelSerializer):
    """推送通知配置序列化器"""
    
    class Meta:
        model = PushNotificationConfig
        fields = ['id', 'task', 'url', 'token', 'auth_scheme', 'auth_credentials',
                 'created_at', 'updated_at']
        read_only_fields = ['id', 'created_at', 'updated_at']
        extra_kwargs = {
            'auth_credentials': {'write_only': True}  # 不在响应中显示凭证
        }


class SessionSerializer(serializers.ModelSerializer):
    """会话序列化器"""
    # 可以考虑添加关联模型的字段，例如 agent 的名称, owner 的用户名
    agent_name = serializers.CharField(source='agent.name', read_only=True)
    owner_username = serializers.CharField(source='owner.username', read_only=True)
    task_count = serializers.SerializerMethodField()

    class Meta:
        model = Session
        fields = ['id', 'name', 'agent', 'agent_name', 'owner', 'owner_username', 
                  'created_at', 'updated_at', 'is_active', 'metadata', 'task_count']
        read_only_fields = ('id', 'created_at', 'updated_at', 'agent_name', 'owner_username', 'task_count')

    def get_task_count(self, obj):
        """获取会话中的任务数量"""
        return obj.tasks.count() 