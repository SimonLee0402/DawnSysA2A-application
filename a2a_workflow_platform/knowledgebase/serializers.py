from rest_framework import serializers
from .models import KnowledgeBase, Document, VisibilityChoices
from django.contrib.auth import get_user_model

User = get_user_model()

class DocumentSerializer(serializers.ModelSerializer):
    # Make knowledge_base field read-only for nested representation, 
    # or use PrimaryKeyRelatedField for writable nested updates if needed.
    # For simplicity, initially, we might handle Document creation under a specific KnowledgeBase.
    knowledge_base_id = serializers.UUIDField(source='knowledge_base.id', read_only=True)
    knowledge_base_name = serializers.CharField(source='knowledge_base.name', read_only=True)
    original_file = serializers.FileField(use_url=True, required=False, allow_null=True) # For file uploads
    
    # file field might need special handling for uploads if using ModelViewSet directly for create/update
    # For now, assume it's a charfield storing a path or URL. If it's a FileField,
    # use serializers.FileField() and ensure multipart form data for uploads.
    # file = serializers.FileField(use_url=True, required=False, allow_null=True) # Removed as model doesn't have a 'file' FileField


    class Meta:
        model = Document
        fields = [
            'id', 
            'knowledge_base_id', 
            'knowledge_base_name',
            'file_name',
            'file_type',
            'file_size',
            'original_file', # Added field
            'extracted_text', # Added field
            # 'file', # Removed from fields as model doesn't have a 'file' FileField directly
            'uploaded_at',
            'processed_at',
            'status',
            'error_message',
            'updated_at'
        ]
        read_only_fields = (
            'extracted_text', 
            'uploaded_at', 
            'processed_at', 
            'status', 
            'error_message',
            'updated_at'
        )
        extra_kwargs = {
            'knowledge_base': {'write_only': True, 'required': False}, 
            'extracted_text': {'read_only': True, 'allow_null': True}
        }

class KnowledgeBaseSerializer(serializers.ModelSerializer):
    owner = serializers.SlugRelatedField(
        slug_field='username', 
        read_only=True
    )
    visibility = serializers.ChoiceField(choices=VisibilityChoices.choices, default=VisibilityChoices.PRIVATE)
    document_count = serializers.SerializerMethodField()
    # documents = DocumentSerializer(many=True, read_only=True) # Keep if you want full nested documents

    class Meta:
        model = KnowledgeBase
        fields = ['id', 'name', 'description', 'owner', 'visibility', 'document_count', 'created_at', 'updated_at']
        read_only_fields = ['id', 'owner', 'document_count', 'created_at', 'updated_at']
        # Remove 'is_public' as it's replaced by 'visibility'
        # Remove 'owner_username' as 'owner' with SlugRelatedField is sufficient

    def get_document_count(self, obj):
        return obj.documents.count()

    # The create method to set owner should be in the ViewSet (perform_create)
    # def create(self, validated_data):
    #     validated_data['owner'] = self.context['request'].user
    #     return super().create(validated_data) 