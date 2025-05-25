from rest_framework import serializers
from .models import Agent, AgentSkill
from knowledgebase.serializers import KnowledgeBaseSerializer # For potential nested display

class AgentSkillSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentSkill
        fields = '__all__' # Or specify fields as needed

class AgentSerializer(serializers.ModelSerializer):
    skills = AgentSkillSerializer(many=True, read_only=True)
    created_by_username = serializers.CharField(source='created_by.username', read_only=True)
    
    # Display linked KBs (read-only). For linking/unlinking, use dedicated actions.
    # Using a simple string representation or a light-nested serializer.
    linked_knowledge_bases_summary = serializers.SerializerMethodField()

    class Meta:
        model = Agent
        fields = [
            'id', 'name', 'description', 'agent_type', 'model_name', 
            'is_active', 'provider_info', 'service_url', 'is_a2a_compliant', 
            'capabilities', 'available_tools', 'authentication_schemes', 
            'a2a_version', 'created_by', 'created_by_username', 'created_at', 'updated_at', 
            'skills', 'linked_knowledge_bases', 'linked_knowledge_bases_summary'
        ]
        read_only_fields = ['id', 'created_by_username', 'created_at', 'updated_at', 'skills', 'linked_knowledge_bases_summary']
        extra_kwargs = {
            'created_by': {'write_only': True, 'required': False, 'allow_null': True},
             # linked_knowledge_bases will be managed by custom actions, not direct write here.
            'linked_knowledge_bases': {'read_only': True, 'required': False}
        }

    def get_linked_knowledge_bases_summary(self, obj):
        # Provides a list of names or ids for quick reference
        return [{'id': kb.id, 'name': kb.name, 'visibility': kb.visibility} for kb in obj.linked_knowledge_bases.all()]

# class AgentLinkKnowledgeBaseSerializer(serializers.Serializer):
#     knowledge_base_id = serializers.UUIDField()
# 
#     def validate_knowledge_base_id(self, value):
#         from knowledgebase.models import KnowledgeBase # Local import to avoid circularity
#         if not KnowledgeBase.objects.filter(id=value).exists():
#             raise serializers.ValidationError("KnowledgeBase with this ID does not exist.")
#         return value 