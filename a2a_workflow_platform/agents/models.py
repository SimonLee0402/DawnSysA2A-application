from django.db import models
from django.contrib.auth import get_user_model
import uuid
from knowledgebase.models import VisibilityChoices

User = get_user_model()

class Agent(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    name = models.CharField(max_length=255, unique=True, help_text="Human-readable name for the agent.")
    description = models.TextField(blank=True, null=True, help_text="Optional. A more detailed description of the agent and its purpose.")
    
    agent_type = models.CharField(max_length=50, default='custom', help_text="The type of agent (e.g., 'gpt', 'claude', 'custom').")
    model_name = models.CharField(max_length=255, blank=True, null=True, help_text="The specific model used by the agent (e.g., 'gpt-4', 'claude-3-sonnet').")
    is_active = models.BooleanField(default=True, help_text="Whether the agent is currently active and usable.")

    provider_info = models.JSONField(blank=True, null=True, help_text="Optional. Information about the agent provider, structured as {'organization': 'Provider Name', 'url': 'https://provider.url'}.")
    
    service_url = models.URLField(max_length=1024, help_text="The A2A service endpoint URL where this agent can be reached. Corresponds to AgentCard.url.")
    agent_software_version = models.CharField(max_length=50, blank=True, null=True, help_text="Version of the agent software/implementation. Corresponds to AgentCard.version.")
    documentation_url = models.URLField(max_length=1024, blank=True, null=True, help_text="Optional. URL to human-readable documentation for the agent. Corresponds to AgentCard.documentationUrl.")

    is_a2a_compliant = models.BooleanField(default=False, help_text="Whether this agent is compliant with the A2A protocol.")

    capabilities = models.JSONField(default=dict, blank=True, help_text="Optional. Describes supported A2A protocol features. Corresponds to AgentCard.capabilities. Expected structure: {'streaming': false, 'pushNotifications': false, 'stateTransitionHistory': false}.")
    available_tools = models.JSONField(default=list, blank=True, help_text="Optional. List of tool names the agent is allowed to use (e.g., ['calculator', 'web_search']).")
    
    # Link to Knowledge Bases
    linked_knowledge_bases = models.ManyToManyField(
        'knowledgebase.KnowledgeBase',
        related_name='linked_agents',
        blank=True,
        help_text="Knowledge bases linked to this agent."
    )

    authentication_schemes = models.JSONField(default=list, blank=True, help_text="Authentication schemes supported by the agent. For AgentCard.authentication, this should be transformed into {'schemes': ['SchemeName'], 'credentials': 'JSON string with details'}. Model stores an array of scheme objects, e.g., [{'type': 'Bearer', 'details': 'Requires a Bearer token.'}].")

    a2a_version = models.CharField(max_length=10, default="0.1.0", help_text="A2A protocol version this agent card claims to conform to (e.g., '0.1.0'). This is distinct from agent_software_version.")
    default_input_modes = models.JSONField(default=list, blank=True, help_text="Optional. Array of MIME types the agent generally accepts as input. Corresponds to AgentCard.defaultInputModes. Example: ['text/plain', 'application/json']. Defaults to ['text/plain'] if empty.")
    default_output_modes = models.JSONField(default=list, blank=True, help_text="Optional. Array of MIME types the agent generally produces as output. Corresponds to AgentCard.defaultOutputModes. Example: ['text/plain', 'application/json']. Defaults to ['text/plain'] if empty.")

    created_by = models.ForeignKey(User, on_delete=models.SET_NULL, null=True, blank=True, related_name="created_agents")
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    def __str__(self):
        return self.name

    def generate_agent_card_data(self):
        """Generates AgentCard data conforming to Google A2A Specification version 0.1.0."""
        
        # Provider information
        provider_data = None
        if self.provider_info and isinstance(self.provider_info, dict):
            provider_data = {
                "organization": self.provider_info.get("organization"),
                "url": self.provider_info.get("url")
            }
            # Remove None values for cleaner output if not provided
            provider_data = {k: v for k, v in provider_data.items() if v is not None}
            if not provider_data.get("organization"): # Organization is required by spec if provider is present
                provider_data = None # or raise an error/log a warning

        # Capabilities
        # Ensure boolean values, default to false if not specified or not boolean
        agent_capabilities_data = {
            "streaming": isinstance(self.capabilities.get("streaming"), bool) and self.capabilities.get("streaming"),
            "pushNotifications": isinstance(self.capabilities.get("pushNotifications"), bool) and self.capabilities.get("pushNotifications"),
            "stateTransitionHistory": isinstance(self.capabilities.get("stateTransitionHistory"), bool) and self.capabilities.get("stateTransitionHistory"),
        }

        # Authentication: Transform model's list of scheme objects to spec's structure
        # Spec: "authentication": {"schemes": ["SchemeName"], "credentials": "details_string_or_json_string"}
        # Model: [{"type": "Bearer", "details": "Requires a Bearer token."}]
        # For simplicity, if multiple schemes are in model, we'll list them. Credentials might be complex to map directly.
        # A single primary scheme with its credentials string is cleaner for A2A card.
        # Let's assume for now: if authentication_schemes has items, pick the first as primary for 'credentials' if applicable.
        # This part might need more sophisticated mapping logic based on how `authentication_schemes` is populated.
        auth_data = None
        if self.authentication_schemes and isinstance(self.authentication_schemes, list):
            schemes_list = []
            cred_details_str = None # Placeholder for credentials string
            for scheme_obj in self.authentication_schemes:
                if isinstance(scheme_obj, dict) and scheme_obj.get("type"):
                    schemes_list.append(scheme_obj.get("type"))
                    # Example: if a scheme object has a 'credentials_string' key, use it.
                    if scheme_obj.get("credentials_string") and not cred_details_str: # take first one
                         cred_details_str = scheme_obj.get("credentials_string") 
           
            if schemes_list:
                auth_data = {"schemes": schemes_list}
                if cred_details_str:
                    auth_data["credentials"] = cred_details_str
        
        # Skills
        skills_data = []
        for skill_model in self.skills.all(): # Assuming self.skills is the related manager
            skill_card_item = {
                "id": skill_model.skill_id,
                "name": skill_model.name,
                "description": skill_model.description,
                "tags": skill_model.tags if isinstance(skill_model.tags, list) else [],
                "examples": skill_model.examples if isinstance(skill_model.examples, list) else [],
                "inputModes": skill_model.input_modes if isinstance(skill_model.input_modes, list) else [],
                "outputModes": skill_model.output_modes if isinstance(skill_model.output_modes, list) else [],
            }
            # Remove None/empty values for cleaner output
            skill_card_item = {k: v for k, v in skill_card_item.items() if v is not None and v != []}
            skills_data.append(skill_card_item)

        card_data = {
            "name": self.name,
            "description": self.description,
            "url": self.service_url,
            "provider": provider_data,
            "version": self.agent_software_version or "unknown", # AgentCard.version is agent's software version
            "documentationUrl": self.documentation_url,
            "capabilities": agent_capabilities_data,
            "authentication": auth_data,
            "defaultInputModes": self.default_input_modes if isinstance(self.default_input_modes, list) and self.default_input_modes else ["text/plain"],
            "defaultOutputModes": self.default_output_modes if isinstance(self.default_output_modes, list) and self.default_output_modes else ["text/plain"],
            "skills": skills_data,
            # Custom fields not in A2A spec but potentially useful for internal representation / richer cards if allowed by consumers:
            # "agent_id_internal": str(self.id), 
            # "a2a_protocol_version_supported": self.a2a_version, # To show what spec version this card aims for
        }

        # Remove top-level keys if their value is None, as per A2A spec for optional fields
        card_data = {k: v for k, v in card_data.items() if v is not None}
        
        return card_data

    class Meta:
        ordering = ['name']
        verbose_name = "Agent"
        verbose_name_plural = "Agents"

class AgentSkill(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    agent = models.ForeignKey(Agent, related_name='skills', on_delete=models.CASCADE)
    skill_id = models.CharField(max_length=255, help_text="Unique ID for the skill within the agent (e.g., 'generate-report', 'translate-text'). Corresponds to AgentSkill.id in AgentCard.")
    name = models.CharField(max_length=255, help_text="Human-readable name for the skill. Corresponds to AgentSkill.name in AgentCard.")
    description = models.TextField(blank=True, null=True, help_text="Optional. A description of what the skill does. Corresponds to AgentSkill.description in AgentCard.")
    tags = models.JSONField(default=list, blank=True, help_text="Optional. Array of keywords or categories for discoverability. Corresponds to AgentSkill.tags in AgentCard. Example: ['finance', 'conversion'].")
    
    input_modes = models.JSONField(default=list, blank=True, help_text='Supported input content types for this skill (array of MIME strings). Overrides agent default. Corresponds to AgentSkill.inputModes. Example: ["application/json", "text/plain"].')
    output_modes = models.JSONField(default=list, blank=True, help_text='Supported output content types for this skill (array of MIME strings). Overrides agent default. Corresponds to AgentSkill.outputModes. Example: ["application/json", "text/plain"].')

    # workflow_definition = models.ForeignKey('workflow.WorkflowDefinition', on_delete=models.SET_NULL, null=True, blank=True, help_text="Optional. The workflow definition that implements this skill.")
    
    examples = models.JSONField(default=list, blank=True, help_text="Optional. Array of example prompts or use cases (strings) illustrating how to use this skill. Corresponds to AgentSkill.examples. Example: ['convert 100 USD to EUR', 'summarize this text: ...'].")

    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    def __str__(self):
        return f"{self.agent.name} - {self.name}"

    class Meta:
        unique_together = ('agent', 'skill_id') 
        ordering = ['agent', 'name']
        verbose_name = "Agent Skill"
        verbose_name_plural = "Agent Skills" 

class LinkedExternalAgent(models.Model):
    """Represents a link to an external A2A-compliant agent for interaction purposes."""
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    
    # Information extracted or derived from the Agent Card
    name = models.CharField(max_length=255, help_text="Name of the external agent, from its card.")
    description = models.TextField(blank=True, null=True, help_text="Description from the agent card.")
    service_url = models.URLField(max_length=1024, help_text="The A2A service endpoint URL of the external agent.")
    
    # Original Card source information
    card_url = models.URLField(max_length=1024, blank=True, null=True, unique=True, db_index=True, help_text="Optional. The URL from which this agent card was fetched. If provided, should be unique.")
    card_content = models.JSONField(blank=True, null=True, help_text="A snapshot of the agent card JSON content.")
    
    # Key details for interaction, extracted from card
    capabilities = models.JSONField(default=dict, blank=True, help_text="Capabilities like streaming, pushNotifications, stateTransitionHistory.")
    authentication_schemes = models.JSONField(default=list, blank=True, help_text="Authentication schemes supported by the external agent.")
    a2a_version = models.CharField(max_length=20, blank=True, null=True, help_text="A2A protocol version claimed by the external agent.")
    default_input_modes = models.JSONField(default=list, blank=True)
    default_output_modes = models.JSONField(default=list, blank=True)
    skills_summary = models.JSONField(default=list, blank=True, help_text="Optional summary of skills (e.g., list of skill names or IDs) for quick reference.")

    # Linking and metadata in our system
    linked_by = models.ForeignKey(User, on_delete=models.CASCADE, related_name="linked_external_agents", help_text="The user who linked this external agent.")
    is_enabled = models.BooleanField(default=True, help_text="Whether this link is active and can be used for interactions.")
    last_successful_contact = models.DateTimeField(null=True, blank=True, help_text="Timestamp of the last successful interaction.")
    last_failed_contact = models.DateTimeField(null=True, blank=True, help_text="Timestamp of the last failed interaction.")
    failure_reason = models.TextField(blank=True, null=True, help_text="Reason for the last failed contact.")
    
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    def __str__(self):
        return f"{self.name} (Linked by {self.linked_by.username})"

    class Meta:
        ordering = ['linked_by', 'name']
        unique_together = ('linked_by', 'service_url') # A user cannot link the same service_url twice.
        # Consider if ('linked_by', 'name') should also be unique, or if name uniqueness is per card_url/service_url.
        verbose_name = "Linked External Agent"
        verbose_name_plural = "Linked External Agents" 