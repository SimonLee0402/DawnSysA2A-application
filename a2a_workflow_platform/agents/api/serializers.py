# Serializers for agents app 
from rest_framework import serializers
from agents.models import Agent, AgentSkill, LinkedExternalAgent
from knowledgebase.models import KnowledgeBase
from knowledgebase.serializers import KnowledgeBaseSerializer

class AgentSkillCardSerializer(serializers.ModelSerializer):
    """Serializer for AgentSkill, focused on AgentCard's 'skills' array items.
       Maps model fields to A2A AgentCard skill specification.
    """
    id = serializers.CharField(source='skill_id') # Renaming skill_id to id for the skill object
    inputModes = serializers.JSONField(source='input_modes')
    outputModes = serializers.JSONField(source='output_modes')

    class Meta:
        model = AgentSkill
        fields = [
            'id', 
            'name', 
            'description', 
            'inputModes', 
            'outputModes', 
            'examples'
        ]

class AgentCardSerializer(serializers.ModelSerializer):
    """Serializer for Agent, providing A2A AgentCard compliant data alongside other useful fields.
    """
    # Agent's own database ID, crucial for frontend operations
    id = serializers.UUIDField(read_only=True)
    owner_id = serializers.IntegerField(source='created_by_id', read_only=True, allow_null=True)
    owner_username = serializers.ReadOnlyField(source='created_by.username')
    created_at = serializers.DateTimeField(read_only=True)
    updated_at = serializers.DateTimeField(read_only=True)

    # Fields for UI display / general info, may duplicate some info from a2a_card_content
    name = serializers.CharField(read_only=True) # Also in A2A card
    description = serializers.CharField(read_only=True, allow_null=True) # Also in A2A card
    agent_type = serializers.CharField(read_only=True) 
    model_name = serializers.CharField(allow_null=True, required=False, read_only=True) 
    is_active = serializers.BooleanField(read_only=True) 
    is_a2a_compliant = serializers.BooleanField(read_only=True)

    # Counts for UI display
    session_count = serializers.ReadOnlyField(default=0) 
    task_count = serializers.ReadOnlyField(default=0)
    workflow_instance_count = serializers.ReadOnlyField(default=0)

    # A2A AgentCard content
    a2a_card_content = serializers.SerializerMethodField()

    # Linked KBs (not part of A2A spec, but useful for our UI)
    linked_knowledge_bases = serializers.SerializerMethodField()

    class Meta:
        model = Agent
        fields = [
            'id', 
            'owner_id',
            'owner_username',
            'name', # For quick access in UI lists/details
            'description', # For quick access
            'agent_type',
            'model_name',
            'is_active',
            'is_a2a_compliant',
            'created_at',
            'updated_at',
            'session_count',
            'task_count',
            'workflow_instance_count',
            'linked_knowledge_bases',
            'a2a_card_content' # The actual A2A Card
        ]

    def get_a2a_card_content(self, obj):
        if hasattr(obj, 'generate_agent_card_data'):
            return obj.generate_agent_card_data()
        return None

    def get_linked_knowledge_bases(self, obj):
        """
        Returns a summarized list of knowledge bases linked to the agent.
        Only includes ID, name, and visibility.
        Filters for knowledge bases accessible to the user making the request if needed,
        but for an Agent Card, we typically show what the Agent itself declares/uses.
        For simplicity, showing all linked KBs.
        """
        kbs_data = []
        for kb in obj.linked_knowledge_bases.all():
            kbs_data.append({
                'id': kb.id,
                'name': kb.name,
                'visibility': kb.visibility,
                # Add a direct URL if applicable and desired for public KBs
                # 'url': request.build_absolute_uri(reverse('knowledgebase-detail', kwargs={'pk': kb.pk})) if kb.visibility == 'PUBLIC' else None
            })
        return kbs_data

# --- Standard Serializers for CRUD (can be used by AgentViewSet for CUD operations) ---
class AgentSerializer(serializers.ModelSerializer):
    # skills = AgentSkillSerializer(many=True, read_only=True) # For listing skills in a CRUD context if needed
    # skills = serializers.PrimaryKeyRelatedField(many=True, queryset=AgentSkill.objects.all(), required=False) # For writable nested

    # Explicitly define created_by as read-only to be set in the view
    created_by = serializers.PrimaryKeyRelatedField(read_only=True)

    class Meta:
        model = Agent
        # List all writable fields explicitly, plus the read-only created_by
        fields = (
            'id', # Often included even if read-only for response data
            'name',
            'description',
            'agent_type',
            'model_name',
            'is_active',
            'provider_info',
            'service_url',
            'is_a2a_compliant',
            'capabilities',
            'authentication_schemes',
            'a2a_version',
            'available_tools',  # Added available_tools
            'created_by', # Include the read-only created_by field
            # created_at and updated_at are auto-generated, no need to list for write
        )
        # For create/update, we might want to handle skills separately or allow writing them nested.
        # If skills are managed separately (e.g. via their own CRUD endpoint or an action on AgentViewSet),
        # then this serializer might not need to deal with them directly for write operations.

class AgentSkillSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentSkill
        fields = '__all__' 

class LinkedExternalAgentSerializer(serializers.ModelSerializer):
    """Serializer for the LinkedExternalAgent model."""
    linked_by_username = serializers.ReadOnlyField(source='linked_by.username')
    # card_url is used for input when creating via a URL, but also stored.
    # card_content is used for input when creating via direct JSON, but also stored (optional).
    # These fields might be handled more directly by the view during creation/
    # but making them part of serializer allows validation and representation.

    # For creation, we'll expect the view to pass parsed card data directly to model fields.
    # So, many fields below are read_only=True for typical GET, but the view will populate them on POST.
    # Alternative for POST: a different, write-only serializer that takes card_url or card_content.
    # For simplicity here, we define one serializer and the view manages data mapping for creation.

    class Meta:
        model = LinkedExternalAgent
        fields = [
            'id',
            'name',
            'description',
            'service_url',
            'card_url', # Can be writable on create/update if user can change the source URL
            'card_content', # Snapshot, typically read-only after initial creation from it
            'capabilities',
            'authentication_schemes',
            'a2a_version',
            'default_input_modes',
            'default_output_modes',
            'skills_summary',
            'linked_by', # This will be set by the view based on request.user
            'linked_by_username',
            'is_enabled',
            'last_successful_contact',
            'last_failed_contact',
            'failure_reason',
            'created_at',
            'updated_at',
        ]
        read_only_fields = [
            'id', 
            'linked_by', # Set by view
            'linked_by_username', 
            'created_at', 
            'updated_at', 
            'last_successful_contact', 
            'last_failed_contact',
            'failure_reason',
            # Consider if card_content should be read-only after creation
        ]

    # If we need specific validation for card_url or card_content when it's provided for creation/update,
    # it can be added here. However, the primary parsing and extraction from the card
    # will happen in the View.

# Moved AgentLinkKnowledgeBaseSerializer from agents.serializers (top-level)
class AgentLinkKnowledgeBaseSerializer(serializers.Serializer):
    knowledge_base_id = serializers.UUIDField()

    def validate_knowledge_base_id(self, value):
        from knowledgebase.models import KnowledgeBase # Local import to avoid circularity if any, or keep it top if safe
        if not KnowledgeBase.objects.filter(id=value).exists():
            raise serializers.ValidationError("KnowledgeBase with this ID does not exist.")
        return value 