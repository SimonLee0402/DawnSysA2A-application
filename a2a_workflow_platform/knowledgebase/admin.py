from django.contrib import admin
from knowledgebase.models import KnowledgeBase, Document

@admin.register(KnowledgeBase)
class KnowledgeBaseAdmin(admin.ModelAdmin):
    list_display = ('name', 'owner', 'created_at', 'updated_at')
    search_fields = ('name', 'description', 'owner__username')
    list_filter = ('created_at', 'owner')

@admin.register(Document)
class DocumentAdmin(admin.ModelAdmin):
    list_display = ('file_name', 'knowledge_base', 'file_type', 'status', 'uploaded_at', 'processed_at')
    search_fields = ('file_name', 'knowledge_base__name')
    list_filter = ('status', 'file_type', 'knowledge_base', 'uploaded_at')
    readonly_fields = ('uploaded_at', 'processed_at') 