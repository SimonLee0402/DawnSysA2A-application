# URL routing for agents app API 
from django.urls import path, include
from rest_framework.routers import DefaultRouter
from .views import AgentViewSet, list_available_tools, AgentImportView, LinkExternalAgentView, LinkedExternalAgentViewSet

app_name = 'agents_api' # Namespace for clarity

# Router for managed agents (Agent model)
router = DefaultRouter()
router.register(r'', AgentViewSet, basename='agent') # Handles /api/agents/ and /api/agents/<id>/

# Router for linked external agents (LinkedExternalAgent model)
external_agent_router = DefaultRouter()
external_agent_router.register(r'manage', LinkedExternalAgentViewSet, basename='linked-external-agent')
# This will generate URLs like /api/agents/external/manage/ and /api/agents/external/manage/<id>/

# urlpatterns for specific, non-router-generated paths
custom_urlpatterns = [
    path('tools/available/', list_available_tools, name='list-available-tools'),
    path('import/', AgentImportView.as_view(), name='agent-import'), # Kept for now
    path('external/link/', LinkExternalAgentView.as_view(), name='link-external-agent'),
]

urlpatterns = custom_urlpatterns + [
    # Include URLs from the main agent router (for Agent model)
    path('', include(router.urls)), 
    # Include URLs from the external agent router (for LinkedExternalAgent model)
    # This will be prefixed by the path in the main project urls.py (e.g., api/agents/)
    # so full path could be api/agents/external/manage/
    path('external/', include(external_agent_router.urls)),
] 