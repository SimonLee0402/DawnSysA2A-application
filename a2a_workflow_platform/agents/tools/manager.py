from typing import Dict, List, Type, Optional
from .base import BaseTool
# from .calculator import CalculatorTool # Example import, more tools can be added

class ToolManager:
    """Manages the registration and retrieval of available tools."""

    def __init__(self):
        self._tools: Dict[str, Type[BaseTool]] = {} # Stores tool classes
        self._tool_instances: Dict[str, BaseTool] = {} # Caches tool instances

    def register_tool(self, tool_class: Type[BaseTool]):
        """Registers a tool class with the manager."""
        # Ensure the tool_class itself has the 'name' attribute defined as a class variable.
        # This change aligns with the planned refactor of BaseTool and specific tools.
        if not hasattr(tool_class, 'name') or not isinstance(getattr(tool_class, 'name'), str):
            raise ValueError("Tool class must have a valid class attribute 'name'.")
        
        tool_name = getattr(tool_class, 'name')
        if tool_name in self._tools:
            # print(f"Warning: Tool '{tool_name}' is already registered. Overwriting.")
            pass # Allow overwrite for easier development/reloading
        
        self._tools[tool_name] = tool_class
        # Clear any cached instance of this tool if it's being re-registered
        if tool_name in self._tool_instances:
            del self._tool_instances[tool_name]

    def get_tool(self, tool_name: str) -> Optional[BaseTool]:
        """
        Retrieves an instance of a tool by its name.
        Instances are created on first request and then cached.
        """
        if tool_name not in self._tools:
            return None
        
        if tool_name not in self._tool_instances:
            tool_class = self._tools[tool_name]
            try:
                self._tool_instances[tool_name] = tool_class() # Instantiate the tool
            except Exception as e:
                # Log this error appropriately in a real application
                print(f"Error instantiating tool '{tool_name}': {e}")
                return None
        
        return self._tool_instances[tool_name]

    def get_all_tools_schemas(self) -> List[Dict]:
        """
        Returns a list of schemas for all registered tools.
        This can be used to inform an LLM about available tools and their parameters.
        """
        schemas = []
        for name in self._tools.keys(): # Iterate over registered tool names
            tool_instance = self.get_tool(name) # This will get/create and cache the instance
            if tool_instance:
                schemas.append(tool_instance.get_schema())
            else:
                # This case should ideally not happen if registration and instantiation are correct
                print(f"Warning: Could not retrieve instance for tool '{name}' when generating schemas.")
        return schemas

    def get_tool_names(self) -> List[str]:
        """Returns a list of names of all registered tools."""
        return list(self._tools.keys())

# Default tool manager instance to be used across the application
# This instance will be populated with tools in tools/__init__.py or elsewhere as needed.
# default_tool_manager = ToolManager() # This is now created in __init__.py

# Example of how to register tools (this would typically be done in tools/__init__.py or app ready state)
# default_tool_manager.register_tool(CalculatorTool) 