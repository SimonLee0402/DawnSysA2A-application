# API views for agents app 
from rest_framework import viewsets, permissions, status
from rest_framework.response import Response
from django.db.models import Count, Q # Import Q
from django.shortcuts import get_object_or_404 # Import get_object_or_404
from agents.models import Agent, AgentSkill, LinkedExternalAgent # AgentSkill for potential future use in actions
from agents.services import AgentInteractionService # Import AgentInteractionService
from .serializers import AgentCardSerializer, AgentSerializer, AgentLinkKnowledgeBaseSerializer, LinkedExternalAgentSerializer # Updated import for AgentLinkKnowledgeBaseSerializer
from workflow.models import Workflow # Import Workflow model
from workflow.api.serializers import WorkflowSerializer # Import WorkflowSerializer
from rest_framework.decorators import action, api_view, permission_classes as dec_permission_classes
from rest_framework.permissions import IsAuthenticated
from knowledgebase.models import KnowledgeBase, VisibilityChoices
from knowledgebase.serializers import KnowledgeBaseSerializer as KnowledgeBaseSummarySerializer
from rest_framework.views import APIView
import json # Ensure json is imported
import requests # Ensure requests is imported if using card_url

# Correct import for default_tool_manager from the package __init__
from agents.tools import default_tool_manager

# Copied IsAgentOwner permission class from agents.views.py
class IsAgentOwner(permissions.BasePermission):
    """Custom permission to only allow owners of an agent to edit or delete it."""
    def has_object_permission(self, request, view, obj):
        if request.method in permissions.SAFE_METHODS:
            # For retrieve, AgentCard is often public or semi-public.
            # List can be filtered by 'my_agents'.
            # Let's allow SAFE_METHODS for now, specific access to an agent card can be debated.
            # If an agent itself had a visibility flag, that would be checked here too.
            return True
        # Write permissions are only allowed to the created_by user of the agent.
        return obj.created_by == request.user

class AgentViewSet(viewsets.ModelViewSet):
    """
    API endpoint that allows Agents to be viewed or edited.
    GET (list, retrieve) uses AgentCardSerializer for AgentCard representation.
    POST, PUT, PATCH, DELETE use AgentSerializer for full CRUD operations.
    Also includes actions for linking/unlinking knowledge bases and interacting with the agent.
    """
    queryset = Agent.objects.prefetch_related('skills', 'linked_knowledge_bases').all().select_related('created_by') # Added linked_knowledge_bases
    permission_classes = [permissions.IsAuthenticated, IsAgentOwner] # Using IsAgentOwner for write ops
    lookup_field = 'id'

    def get_queryset(self):
        """
        Optionally filters agents by the current user for 'list' action if 'my_agents' is true.
        For retrieve, IsAgentOwner handles object-level permission.
        """
        queryset = Agent.objects.prefetch_related('skills', 'linked_knowledge_bases').all().select_related('created_by')

        my_agents = self.request.query_params.get('my_agents', None)
        if my_agents == 'true' and self.request.user.is_authenticated and self.action == 'list':
            queryset = queryset.filter(created_by=self.request.user)
        
        # For 'retrieve', 'update', 'partial_update', 'destroy' actions,
        # IsAgentOwner will be checked via check_object_permissions.
        # For 'list', it's a general list unless filtered.
        return queryset

    def perform_create(self, serializer):
        """
        Create a new agent instance, setting the created_by field to the current user.
        """
        serializer.save(created_by=self.request.user)

    def get_serializer_class(self):
        if self.action in ['list', 'retrieve']:
            return AgentCardSerializer
        # For linking/unlinking KBs
        if self.action in ['link_knowledgebase', 'unlink_knowledgebase']:
            return AgentLinkKnowledgeBaseSerializer
        # For listing available KBs
        if self.action == 'available_knowledgebases':
            return KnowledgeBaseSummarySerializer # Use the aliased serializer
        # For interact action, we might not need a specific serializer for request if it's just a query string
        # but if the request body is more complex, define one.
        # For now, no specific serializer for interact input, output is a direct Response.
        return AgentSerializer # For create, update, partial_update, delete

    # Potentially, in the future, skill creation/management could be handled here via @action
    # or through a separate AgentSkillViewSet if more complex logic is needed.

    # Example of how you might list skills for an agent (though skills are nested in AgentCardSerializer)
    # @action(detail=True, methods=['get'], serializer_class=AgentSkillCardSerializer) # Or AgentSkillSerializer
    # def skills(self, request, pk=None):
    #     agent = self.get_object()
    #     skills = agent.skills.all()
    #     serializer = self.get_serializer(skills, many=True)
    #     return Response(serializer.data)

    @action(detail=True, methods=['post'], url_path='link-knowledgebase')
    def link_knowledgebase(self, request, id=None): # pk is 'id' due to lookup_field
        agent = self.get_object() # Checks IsAgentOwner for write-like action

        serializer = self.get_serializer(data=request.data) # Use get_serializer for action-specific serializer
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
        
        knowledge_base_id = serializer.validated_data['knowledge_base_id']
        
        try:
            kb_to_link = KnowledgeBase.objects.get(id=knowledge_base_id)
            
            can_link = False
            if kb_to_link.visibility == VisibilityChoices.PUBLIC:
                can_link = True
            elif kb_to_link.owner == request.user: # User must own the agent (checked by IsAgentOwner) and the KB
                can_link = True
            
            if not can_link:
                return Response({'detail': 'You do not have permission to link this knowledge base (it must be public or you must own it).'}, status=status.HTTP_403_FORBIDDEN)

            agent.linked_knowledge_bases.add(kb_to_link)
            return Response({'status': 'Knowledge base linked'}, status=status.HTTP_200_OK)
        except KnowledgeBase.DoesNotExist:
            return Response({'detail': 'KnowledgeBase not found.'}, status=status.HTTP_404_NOT_FOUND)

    @action(detail=True, methods=['post'], url_path='unlink-knowledgebase')
    def unlink_knowledgebase(self, request, id=None): # pk is 'id'
        agent = self.get_object() # Checks IsAgentOwner
        
        serializer = self.get_serializer(data=request.data) # Use get_serializer for action-specific serializer
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
            
        knowledge_base_id = serializer.validated_data['knowledge_base_id']

        try:
            # We only need to check if the agent owner is performing this.
            # Whether the KB exists or is linked is handled by the remove() operation.
            kb_to_unlink = get_object_or_404(KnowledgeBase, id=knowledge_base_id)
            if kb_to_unlink in agent.linked_knowledge_bases.all():
                agent.linked_knowledge_bases.remove(kb_to_unlink)
                return Response({'status': 'Knowledge base unlinked'}, status=status.HTTP_200_OK)
            else:
                return Response({'detail': 'KnowledgeBase not linked to this agent.'}, status=status.HTTP_404_NOT_FOUND)
        except KnowledgeBase.DoesNotExist:
            return Response({'detail': 'KnowledgeBase not found.'}, status=status.HTTP_404_NOT_FOUND)
        
    @action(detail=False, methods=['get'], url_path='available-knowledgebases-for-linking') # Renamed for clarity
    def available_knowledgebases(self, request): # Not detail=True, it's a general list
        user = request.user
        # This action should be available to authenticated users to see what KBs they could link if they were creating/editing an agent
        # No agent instance context here.
        
        accessible_kbs = KnowledgeBase.objects.filter(
            Q(owner=user) | Q(visibility=VisibilityChoices.PUBLIC)
        ).distinct().order_by('name')
        
        serializer = KnowledgeBaseSummarySerializer(accessible_kbs, many=True, context={'request': request}) # Aliased serializer
        return Response(serializer.data)

    @action(detail=True, methods=['get'])
    def workflows(self, request, id=None):
        """
        获取与此智能体相关的工作流列表。
        """
        # TODO: Implement correct filtering of workflows based on the agent ID within the workflow definition JSON.
        # The current approach (using __icontains on JSONField) is likely incorrect and inefficient,
        # and needs to be replaced with proper JSONField lookups based on the actual definition structure.
        # For now, return an empty list to avoid errors.
        # agent = self.get_object()
        # agent_id_str = str(agent.id)
        # related_workflows = Workflow.objects.filter(
        #     definition__icontains=agent_id_str # This is likely the source of 500 errors
        # )
        # serializer = WorkflowSerializer(related_workflows, many=True)
        # return Response(serializer.data)
        return Response([]) # Return empty list temporarily

    @action(detail=True, methods=['post'], url_path='interact')
    def interact(self, request, id=None):
        """
        Allows interaction with the agent. 
        Expects a POST request with a 'query' in the JSON body.
        """
        user_query = request.data.get('query')
        if not user_query:
            return Response({"error": "Missing 'query' in request body."}, status=status.HTTP_400_BAD_REQUEST)

        # Permission check: IsAgentOwner also applies here due to how ModelViewSet permissions work with @action.
        # self.get_object() will trigger the permission check if IsAgentOwner is restrictive for POST.
        # Since IsAgentOwner allows SAFE_METHODS, but this is POST, it will check obj.created_by == request.user.
        # This means only the agent owner can interact with it. This might be desired or might need adjustment.
        # If interaction should be more public (e.g., for published agents), a different permission strategy for this action might be needed.
        
        # agent_id is passed as 'id' due to lookup_field = 'id'
        try:
            # Consider if LLM API key/model should be configurable per agent or globally
            # For now, using defaults from AgentInteractionService constructor
            service = AgentInteractionService(agent_id=id)
            agent_response = service.process_interaction(user_query)
            return Response({"response": agent_response}, status=status.HTTP_200_OK)
        except Agent.DoesNotExist:
            # This should be caught by get_object_or_404 in AgentInteractionService or a prior permission check.
            # If AgentInteractionService is robust, this might not be strictly necessary here.
            return Response({"error": "Agent not found."}, status=status.HTTP_404_NOT_FOUND)
        except Exception as e:
            # Catch-all for other unexpected errors during interaction
            # Log the error for debugging: import logging; logger = logging.getLogger(__name__); logger.error(f"Error in agent interaction: {e}", exc_info=True)
            return Response({"error": f"An unexpected error occurred: {str(e)}"}, status=status.HTTP_500_INTERNAL_SERVER_ERROR)

@api_view(['GET'])
@dec_permission_classes([IsAuthenticated]) # Or IsAuthenticatedOrReadOnly if preferred
def list_available_tools(request):
    """
    Returns a list of all available tools that agents can use.
    Each tool includes its name and description.
    """
    tool_manager = default_tool_manager
    tools_data = []
    for tool_name in tool_manager.get_tool_names(): # Iterate over tool names
        tool_instance = tool_manager.get_tool(tool_name) # Get the instance
        if tool_instance: # Ensure the tool instance exists
            tools_data.append({
                "name": tool_instance.name,
                "description": tool_instance.description,
                "name_zh": getattr(tool_instance, 'name_zh', tool_instance.name),  # Fallback to name if name_zh not present
                "description_zh": getattr(tool_instance, 'description_zh', tool_instance.description), # Fallback to description
                # We could also include the schema if the frontend needs it for display purposes
                # "schema": tool_instance.get_schema()
            })
    return Response(tools_data)

# Note: We are replacing AgentCardViewSet with a more comprehensive AgentViewSet
# If you specifically need a separate, strictly read-only AgentCard endpoint later,
# you could re-introduce a ReadOnlyModelViewSet for that with a different route.

# Placeholder for a full Agent ViewSet if CRUD operations are needed later
# class AgentViewSet(viewsets.ModelViewSet):
#     queryset = Agent.objects.all()
#     serializer_class = AgentSerializer
# permission_classes = [permissions.IsAuthenticated] # Or more specific permissions 

class AgentImportView(APIView):
    """
    API endpoint to import an Agent from a JSON "Agent Card" definition fetched from a URL or directly from JSON content.
    """
    permission_classes = [permissions.IsAuthenticated]
    # http_method_names can be reverted to ['post', 'options'] if GET debug is no longer needed
    http_method_names = ['post', 'options', 'get'] 

    def dispatch(self, request, *args, **kwargs):
        print(f"[DEBUG] AgentImportView dispatch called. Request method: {request.method}")
        print(f"[DEBUG] Allowed HTTP methods for AgentImportView: {self.http_method_names}")
        handler = getattr(self, request.method.lower(), None)
        if handler:
            print(f"[DEBUG] Handler found for method {request.method.lower()}: {handler.__name__}")
        else:
            print(f"[DEBUG] NO handler found for method {request.method.lower()} in AgentImportView. This would lead to 405.")
        
        response = super().dispatch(request, *args, **kwargs)
        print(f"[DEBUG] AgentImportView dispatch finished. Response status: {response.status_code}, Response data: {response.data if hasattr(response, 'data') else 'N/A'}")
        return response
    
    def get(self, request, *args, **kwargs): # Temporarily add GET handler for testing
        print("[DEBUG] AgentImportView GET method called!")
        return Response({"message": "GET request received by AgentImportView. Use POST to import."}, status=status.HTTP_200_OK)

    def post(self, request, *args, **kwargs):
        print("[DEBUG] AgentImportView POST method called!") 
        card_url = request.data.get('card_url')
        card_content_str = request.data.get('card_content')
        agent_card_data = None

        if card_url:
            print(f"[DEBUG] Importing from URL: {card_url}")
            try:
                response = requests.get(card_url, timeout=10)
                response.raise_for_status()
                agent_card_data = response.json()
                print("[DEBUG] Successfully fetched and parsed JSON from URL.")
            except requests.exceptions.RequestException as e:
                print(f"[DEBUG] Error fetching from URL: {str(e)}")
                return Response(
                    {"error": f"Failed to fetch agent card from URL: {str(e)}"},
                    status=status.HTTP_400_BAD_REQUEST
                )
            except json.JSONDecodeError as e:
                print(f"[DEBUG] JSON decode error from URL content: {str(e)}")
                return Response(
                    {"error": "Invalid JSON format in the fetched agent card from URL."},
                    status=status.HTTP_400_BAD_REQUEST
                )
            except Exception as e:
                print(f"[DEBUG] Unexpected error fetching/parsing from URL: {str(e)}")
                return Response(
                    {"error": f"An unexpected error occurred while fetching from URL: {str(e)}"},
                    status=status.HTTP_500_INTERNAL_SERVER_ERROR
                )
        elif card_content_str:
            print("[DEBUG] Importing from direct JSON content.")
            try:
                agent_card_data = json.loads(card_content_str)
                print("[DEBUG] Successfully parsed direct JSON content.")
            except json.JSONDecodeError as e:
                print(f"[DEBUG] JSON decode error from direct content: {str(e)}")
                return Response(
                    {"error": "Invalid JSON format in the provided card content."},
                    status=status.HTTP_400_BAD_REQUEST
                )
            except Exception as e:
                print(f"[DEBUG] Unexpected error parsing direct content: {str(e)}")
                return Response(
                    {"error": f"An unexpected error occurred while parsing card content: {str(e)}"},
                    status=status.HTTP_500_INTERNAL_SERVER_ERROR
                )
        else:
            print("[DEBUG] Missing 'card_url' or 'card_content' in request.")
            return Response(
                {"error": "Missing 'card_url' or 'card_content' in request body."},
                status=status.HTTP_400_BAD_REQUEST
            )

        if agent_card_data is None:
            print("[DEBUG] Agent card data is None after attempting to load/parse.")
            return Response(
                {"error": "Failed to obtain agent card data."},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR # Should have been caught earlier
            )

        print(f"[DEBUG] Agent card data to be processed: {str(agent_card_data)[:200]}...") # Log snippet
        
        serializer_data = agent_card_data.copy()
        if 'created_by' in serializer_data:
            del serializer_data['created_by']
        if 'owner_username' in serializer_data:
             del serializer_data['owner_username']

        raw_tools_data = serializer_data.pop('tools', [])
        linked_knowledge_bases_data = serializer_data.pop('linked_knowledge_bases', [])
        print(f"[DEBUG] Serializer data after popping tools/KBs: {str(serializer_data)[:200]}...")

        serializer = AgentSerializer(data=serializer_data, context={'request': request})

        if serializer.is_valid():
            print("[DEBUG] AgentSerializer is valid.")
            try:
                agent_instance = serializer.save(created_by=request.user)
                print(f"[DEBUG] Agent instance created/saved: ID {agent_instance.id}")
                
                # ... (tool and KB linking logic - keep as is for now) ...
                if raw_tools_data:
                    # ... (tool linking) ...
                    pass 
                if linked_knowledge_bases_data:
                    # ... (KB linking) ...
                    pass
                
                response_serializer = AgentCardSerializer(agent_instance, context={'request': request})
                print("[DEBUG] Successfully created agent, returning serialized card.")
                return Response(response_serializer.data, status=status.HTTP_201_CREATED)
            except Exception as e:
                print(f"[DEBUG] Error during serializer.save() or post-processing: {str(e)}")
                return Response(
                    {"error": f"Error processing and saving agent card: {str(e)}"},
                    status=status.HTTP_500_INTERNAL_SERVER_ERROR
                )
        else:
            print(f"[DEBUG] AgentSerializer is NOT valid. Errors: {serializer.errors}")
            return Response(
                {"error": "Invalid agent card data.", "details": serializer.errors},
                status=status.HTTP_400_BAD_REQUEST
            ) 

class LinkExternalAgentView(APIView):
    """
    API endpoint to link an external Agent by fetching and parsing its Agent Card 
    from a URL or direct JSON content.
    Creates a LinkedExternalAgent record.
    """
    permission_classes = [permissions.IsAuthenticated]

    def post(self, request, *args, **kwargs):
        card_url = request.data.get('card_url')
        card_content_str = request.data.get('card_content')
        agent_card_data = None
        source_url_for_record = None # To store the card_url if that's the source

        if card_url:
            source_url_for_record = card_url
            try:
                response = requests.get(card_url, timeout=10)
                response.raise_for_status()
                agent_card_data = response.json()
            except requests.exceptions.RequestException as e:
                return Response(
                    {"error": f"Failed to fetch agent card from URL: {str(e)}"},
                    status=status.HTTP_400_BAD_REQUEST
                )
            except json.JSONDecodeError:
                return Response(
                    {"error": "Invalid JSON format in the fetched agent card from URL."},
                    status=status.HTTP_400_BAD_REQUEST
                )
            except Exception as e: # Catch any other unexpected errors during fetch/parse
                 return Response(
                    {"error": f"An unexpected error occurred while processing the URL: {str(e)}"},
                    status=status.HTTP_500_INTERNAL_SERVER_ERROR
                )
        elif card_content_str:
            try:
                agent_card_data = json.loads(card_content_str)
            except json.JSONDecodeError as e:
                return Response(
                    {"error": "Invalid JSON format in the provided card content."},
                    status=status.HTTP_400_BAD_REQUEST
                )
            except Exception as e: # Catch any other unexpected errors during parse
                 return Response(
                    {"error": f"An unexpected error occurred while parsing the card content: {str(e)}"},
                    status=status.HTTP_500_INTERNAL_SERVER_ERROR
                )
        else:
            return Response(
                {"error": "Missing 'card_url' or 'card_content' in request body."},
                status=status.HTTP_400_BAD_REQUEST
            )

        if agent_card_data is None:
            # This case should ideally be caught by the specific error handling above
            return Response(
                {"error": "Failed to obtain agent card data."},
                status=status.HTTP_500_INTERNAL_SERVER_ERROR
            )

        # Extract data for LinkedExternalAgent model
        # Required fields from AgentCard spec (or sensible defaults)
        name = agent_card_data.get('name')
        service_url = agent_card_data.get('url') # AgentCard.url is the service_url

        if not name or not service_url:
            return Response(
                {"error": "Agent card must contain 'name' and 'url' fields."},
                status=status.HTTP_400_BAD_REQUEST
            )
        
        # Optional fields from AgentCard spec
        description = agent_card_data.get('description')
        capabilities = agent_card_data.get('capabilities', {})
        authentication = agent_card_data.get('authentication', {})
        # The model stores 'authentication_schemes' as a list of scheme objects,
        # while AgentCard has 'authentication': {'schemes': [], 'credentials': ''}.
        # We'll store the raw 'authentication' dict from the card for now, 
        # or adapt it if a specific structure is decided for the model.
        # For simplicity, let's try to extract scheme names if possible.
        auth_schemes_from_card = []
        if isinstance(authentication, dict) and isinstance(authentication.get('schemes'), list):
            auth_schemes_from_card = authentication.get('schemes')
        
        # a2a_version_from_card = agent_card_data.get('a2aVersion') # Check A2A spec for exact field name if needed
        # For now, let's assume it's not a primary field for linking, or use a default.
        # The `a2a_version` in `LinkedExternalAgent` refers to version claimed by external agent.
        claimed_a2a_version = agent_card_data.get('a2aVersion', agent_card_data.get('a2a_version')) # Accommodate variations

        default_input_modes = agent_card_data.get('defaultInputModes', [])
        default_output_modes = agent_card_data.get('defaultOutputModes', [])
        
        skills = agent_card_data.get('skills', [])
        skills_summary = [] # Create a simple summary (e.g., list of skill names or ids)
        if isinstance(skills, list):
            for skill_item in skills:
                if isinstance(skill_item, dict) and skill_item.get('name'):
                    skills_summary.append({'id': skill_item.get('id'), 'name': skill_item.get('name')})
                elif isinstance(skill_item, dict) and skill_item.get('id'): # Fallback to id if name is missing
                    skills_summary.append({'id': skill_item.get('id')})

        # Prepare data for LinkedExternalAgentSerializer
        link_data = {
            'name': name,
            'description': description,
            'service_url': service_url,
            'card_url': source_url_for_record, # This will be the original URL if fetched, or None
            'card_content': agent_card_data, # Store the whole card for reference
            'capabilities': capabilities,
            'authentication_schemes': auth_schemes_from_card, # Or the raw 'authentication' dict
            'a2a_version': claimed_a2a_version,
            'default_input_modes': default_input_modes,
            'default_output_modes': default_output_modes,
            'skills_summary': skills_summary,
            # 'linked_by' will be set in serializer.save() via context or directly
        }

        serializer = LinkedExternalAgentSerializer(data=link_data, context={'request': request})
        if serializer.is_valid():
            try:
                # Check for existing link by the same user to the same service_url
                if LinkedExternalAgent.objects.filter(linked_by=request.user, service_url=service_url).exists():
                    return Response(
                        {"error": "You have already linked an agent with this service URL."},
                        status=status.HTTP_409_CONFLICT # Conflict
                    )
                
                serializer.save(linked_by=request.user)
                return Response(serializer.data, status=status.HTTP_201_CREATED)
            except Exception as e: # Catch unexpected errors during save
                return Response(
                    {"error": f"Could not save the linked agent: {str(e)}"},
                    status=status.HTTP_500_INTERNAL_SERVER_ERROR
                )
        else:
            return Response(
                {"error": "Invalid data for linking agent.", "details": serializer.errors},
                status=status.HTTP_400_BAD_REQUEST
            ) 

class IsLinkedAgentOwner(permissions.BasePermission):
    """Custom permission to only allow owners of a linked agent record to modify it."""
    def has_object_permission(self, request, view, obj):
        # Write permissions are only allowed to the linked_by user of the record.
        # Safe methods (GET, HEAD, OPTIONS) are allowed for any authenticated user to view their own links.
        # If we want to restrict even viewing to only the owner, this can be adjusted.
        if request.method in permissions.SAFE_METHODS:
            return obj.linked_by == request.user # User can only see their own links
        return obj.linked_by == request.user

class LinkedExternalAgentViewSet(viewsets.ModelViewSet):
    """
    API endpoint for listing, retrieving, updating, and deleting LinkedExternalAgent records.
    """
    serializer_class = LinkedExternalAgentSerializer
    permission_classes = [permissions.IsAuthenticated, IsLinkedAgentOwner]
    lookup_field = 'id'

    def get_queryset(self):
        """Users can only see and manage their own linked external agents."""
        user = self.request.user
        if user.is_authenticated:
            return LinkedExternalAgent.objects.filter(linked_by=user).order_by('-created_at')
        return LinkedExternalAgent.objects.none() # Should not happen due to IsAuthenticated

    def perform_create(self, serializer):
        # Creation is handled by LinkExternalAgentView, not directly by this ViewSet's POST.
        # If POST were enabled here, we'd set linked_by=self.request.user.
        # This ViewSet is more for GET, PUT, PATCH, DELETE of existing links.
        # For now, we can raise an error or simply not map POST here.
        # Let's assume standard ModelViewSet behavior; if POST comes, it would try to create.
        # We will explicitly set linked_by if a direct POST to this ViewSet were used.
        # However, our primary create path is LinkExternalAgentView.
        serializer.save(linked_by=self.request.user) 

    def perform_update(self, serializer):
        # Ensure linked_by cannot be changed on update
        serializer.save(linked_by=self.request.user)

    # perform_destroy is standard and will respect IsLinkedAgentOwner permission. 