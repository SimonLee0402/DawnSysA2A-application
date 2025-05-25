from .models import Agent
from .tools import default_tool_manager
from .llm_interface.client import LLMClient
from django.shortcuts import get_object_or_404

class AgentInteractionService:
    """Service to handle the interaction logic for an agent."""

    MAX_INTERACTION_STEPS = 5 # Maximum number of tool call iterations

    def __init__(self, agent_id: str, llm_api_key: str = None, llm_model_name: str = "mock-model"):
        """
        Initializes the service with a specific agent and LLM configuration.
        Args:
            agent_id (str): The UUID of the agent to interact with.
            llm_api_key (str, optional): API key for the LLM.
            llm_model_name (str, optional): Model name for the LLM.
        """
        self.agent = get_object_or_404(Agent, id=agent_id)
        self.llm_client = LLMClient(api_key=llm_api_key, model_name=llm_model_name)
        self.conversation_history = [] # To store a simple history for the current interaction

    def _get_available_tools_schema(self):
        """Retrieves the schema for tools available to the current agent."""
        agent_tool_names = self.agent.available_tools or [] # available_tools is a list of tool names
        all_tools_schemas = default_tool_manager.get_all_tools_schemas()
        
        available_schemas = [
            schema for schema in all_tools_schemas 
            if schema['name'] in agent_tool_names
        ]
        return available_schemas

    def process_interaction(self, user_query: str) -> str:
        """
        Processes a single user query through the agent, potentially involving LLM calls and tool executions.

        Args:
            user_query (str): The user's input query.

        Returns:
            str: The agent's final textual response.
        """
        self.conversation_history.append({"role": "user", "content": user_query})

        for _ in range(self.MAX_INTERACTION_STEPS):
            # Construct prompt from history and available tools
            # This is a very basic prompt construction, can be improved significantly.
            prompt_parts = [f"{msg['role']}: {msg['content']}" for msg in self.conversation_history]
            prompt = "\n".join(prompt_parts) + "\nassistant:"
            
            tools_schema = self._get_available_tools_schema()
            
            print(f"---- AgentInteractionService: Sending prompt to LLM ----")
            print(prompt)
            if tools_schema:
                print(f"---- With Tools Schema ----")
                # print(json.dumps(tools_schema, indent=2)) # Can be verbose
            print("-----------------------------------------------------")

            llm_response = self.llm_client.generate_response(prompt=prompt, tools_schema=tools_schema)
            
            print(f"---- AgentInteractionService: Received LLM Response ----")
            print(llm_response)
            print("------------------------------------------------------")

            if llm_response.get("type") == "text":
                response_content = llm_response.get("content", "Sorry, I could not process that.")
                self.conversation_history.append({"role": "assistant", "content": response_content})
                return response_content
            
            elif llm_response.get("type") == "tool_call":
                tool_name = llm_response.get("tool_name")
                tool_params = llm_response.get("tool_params")
                
                self.conversation_history.append({
                    "role": "assistant", 
                    "content": f"[Requesting to use tool: {tool_name} with params: {tool_params}]"
                })

                if not tool_name or not tool_params:
                    error_message = "LLM requested a tool call but did not provide name or params."
                    self.conversation_history.append({"role": "system", "content": f"[Error: {error_message}]"})
                    # Potentially inform the LLM about this error in the next turn if we continue
                    return f"Error: LLM tool call was malformed. {error_message}"

                # Check if agent is allowed to use this tool
                if tool_name not in (self.agent.available_tools or []):
                    error_message = f"Agent is not authorized to use tool: {tool_name}."
                    self.conversation_history.append({"role": "system", "content": f"[Tool Execution Error: {error_message}]"})
                    # In a real scenario, this error might be fed back to the LLM
                    # For now, we append to history and return a user-facing error that the loop will break.
                    # return f"Error: {error_message}" # Option 1: return error and stop
                    # Option 2: Feed error back to LLM (append to history and continue loop)
                    self.conversation_history.append({
                        "role": "tool_output", # Or some other role indicating tool feedback
                        "tool_name": tool_name,
                        "content": f"Error: You are not allowed to use the tool '{tool_name}'. Available tools are: {self.agent.available_tools}"
                    })
                    continue # Continue to next LLM call with this error as context

                tool_instance = default_tool_manager.get_tool(tool_name)
                if not tool_instance:
                    error_message = f"Tool '{tool_name}' not found in tool manager."
                    self.conversation_history.append({"role": "system", "content": f"[Tool Execution Error: {error_message}]"})
                    self.conversation_history.append({
                        "role": "tool_output",
                        "tool_name": tool_name,
                        "content": f"Error: Tool '{tool_name}' is not a recognized tool."
                    })
                    continue

                print(f"---- AgentInteractionService: Executing tool: {tool_name} ----")
                tool_result = tool_instance.execute(tool_params)
                print(f"---- Tool Result: {tool_result} ----")
                
                self.conversation_history.append({
                    "role": "tool_output", # Or some other role indicating tool feedback
                    "tool_name": tool_name,
                    "content": tool_result
                }) 
                # Loop continues, result will be in the next prompt to LLM
            else:
                # Unknown response type from LLM
                error_message = "LLM returned an unknown response type."
                self.conversation_history.append({"role": "system", "content": f"[Error: {error_message}]"})
                return f"Error: {error_message}"

        return "Error: Agent reached maximum interaction steps without a final answer."

# Example of how this service might be used (e.g., from an API view):
# if __name__ == '__main__':
#     # This is a Django-dependent example, so it needs Django context to run directly.
#     # You would typically call this from a Django view or management command.
#     
#     # Ensure you have an Agent in your DB with ID 'some_agent_uuid' 
#     # and that it has 'calculator' in its available_tools list.
#     # Also, your default_tool_manager should have CalculatorTool registered.
#     try:
#         # agent_service = AgentInteractionService(agent_id='YOUR_AGENT_ID_HERE')
#         # response = agent_service.process_interaction("Hello, can you calculate 5+7 for me?")
#         # print(f"Final Agent Response: {response}")
#         
#         # response = agent_service.process_interaction("Hi there!")
#         # print(f"Final Agent Response: {response}")
#         pass
#     except Exception as e:
#         print(f"Error during example usage: {e}")
#         print("Ensure Django is set up and you have a valid Agent ID.")
#         print("The Agent also needs 'calculator' in its 'available_tools' list in the database.") 