# Views for agents app (non-API) 
# from rest_framework import viewsets, permissions, status, serializers # Commented out
# from rest_framework.response import Response # Commented out
# from rest_framework.decorators import action # Commented out
# from django.shortcuts import get_object_or_404 # Commented out
# from django.db.models import Q # Commented out

# from .models import Agent # Commented out, Agent model itself is still used by agents.api.views
# from .serializers import AgentSerializer, AgentLinkKnowledgeBaseSerializer # Commented out
# from knowledgebase.models import KnowledgeBase, VisibilityChoices # Commented out
# from knowledgebase.serializers import KnowledgeBaseSerializer # Commented out

# class IsAgentOwner(permissions.BasePermission):
#     """Custom permission to only allow owners of an agent to edit or delete it."""
#     def has_object_permission(self, request, view, obj):
#         # Read permissions are allowed for any request (if listing/retrieving agents is public/authenticated-only)
#         if request.method in permissions.SAFE_METHODS:
#             return True # Or further restrict if agents themselves have visibility settings
#         
#         # Write permissions are only allowed to the created_by user of the agent.
#         return obj.created_by == request.user

# class AgentViewSet(viewsets.ModelViewSet):
#     queryset = Agent.objects.all().order_by('-created_at')
#     serializer_class = AgentSerializer
#     permission_classes = [permissions.IsAuthenticated, IsAgentOwner]
# 
#     def perform_create(self, serializer):
#         # Automatically set created_by to the current authenticated user
#         serializer.save(created_by=self.request.user)
# 
#     def get_queryset(self):
#         # Users can see all agents, but can only edit/delete their own (due to IsAgentOwner)
#         # This could be further restricted if agents shouldn't be visible to everyone.
#         return Agent.objects.all().order_by('-created_at')
# 
#     @action(detail=True, methods=['post'], url_path='link-knowledgebase', serializer_class=AgentLinkKnowledgeBaseSerializer)
#     def link_knowledgebase(self, request, pk=None):
#         agent = self.get_object() # Checks IsAgentOwner for write-like action
# 
#         serializer = AgentLinkKnowledgeBaseSerializer(data=request.data)
#         if not serializer.is_valid():
#             return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
#         
#         knowledge_base_id = serializer.validated_data['knowledge_base_id']
#         
#         try:
#             # Check if KB exists and if user has permission to link it
#             kb_to_link = KnowledgeBase.objects.get(id=knowledge_base_id)
#             
#             # Permission check: User can link if KB is public OR user owns the KB
#             can_link = False
#             if kb_to_link.visibility == VisibilityChoices.PUBLIC:
#                 can_link = True
#             elif kb_to_link.owner == request.user:
#                 can_link = True
#             
#             if not can_link:
#                 return Response({'detail': 'You do not have permission to link this knowledge base.'}, status=status.HTTP_403_FORBIDDEN)
# 
#             agent.linked_knowledge_bases.add(kb_to_link)
#             return Response({'status': 'Knowledge base linked'}, status=status.HTTP_200_OK)
#         except KnowledgeBase.DoesNotExist:
#             return Response({'detail': 'KnowledgeBase not found.'}, status=status.HTTP_404_NOT_FOUND)
# 
#     @action(detail=True, methods=['post'], url_path='unlink-knowledgebase', serializer_class=AgentLinkKnowledgeBaseSerializer)
#     def unlink_knowledgebase(self, request, pk=None):
#         agent = self.get_object() # Checks IsAgentOwner
#         
#         serializer = AgentLinkKnowledgeBaseSerializer(data=request.data)
#         if not serializer.is_valid():
#             return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
#             
#         knowledge_base_id = serializer.validated_data['knowledge_base_id']
# 
#         try:
#             kb_to_unlink = KnowledgeBase.objects.get(id=knowledge_base_id)
#             agent.linked_knowledge_bases.remove(kb_to_unlink)
#             return Response({'status': 'Knowledge base unlinked'}, status=status.HTTP_200_OK)
#         except KnowledgeBase.DoesNotExist:
#             # Agent might not have it linked, or KB doesn't exist. remove() handles non-existent links gracefully.
#             return Response({'detail': 'KnowledgeBase not found or not linked to this agent.'}, status=status.HTTP_404_NOT_FOUND)
#         
#     # Could add an action to list KBs linkable by this agent owner
#     @action(detail=False, methods=['get'], url_path='available-knowledgebases')
#     def available_knowledgebases(self, request):
#         user = request.user
#         if not user.is_authenticated:
#             return Response({"detail": "Authentication required."}, status=status.HTTP_401_UNAUTHORIZED)
# 
#         accessible_kbs = KnowledgeBase.objects.filter(
#             Q(owner=user) | Q(visibility=VisibilityChoices.PUBLIC)
#         ).distinct()
#         
#         serializer = KnowledgeBaseSerializer(accessible_kbs, many=True, context={'request': request})
#         return Response(serializer.data)

# Agent card action was already commented out previously
#     # @action(detail=True, methods=['get'], url_path='agent-card')
#     # def agent_card(self, request, pk=None):
#     #     agent = self.get_object() # Permissions handled by IsAgentOwner (allows SAFE methods)
#     #     card_data = agent.generate_agent_card_data()
#     #     return Response(card_data) 