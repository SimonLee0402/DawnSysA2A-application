# This file makes the 'tools' directory a Python package. 

from .base import BaseTool
from .manager import ToolManager
from .calculator import CalculatorTool
from .web_search_tool import WebSearchTool # Ensure this is present
from .knowledge_base_query_tool import KnowledgeBaseQueryTool # Ensure this is present

# Create the default manager instance
default_tool_manager = ToolManager()

# Register tool CLASSES with the manager
# This aligns with the refactored ToolManager and the upcoming BaseTool refactor
default_tool_manager.register_tool(CalculatorTool)
default_tool_manager.register_tool(WebSearchTool)
default_tool_manager.register_tool(KnowledgeBaseQueryTool)

__all__ = ["default_tool_manager", "BaseTool"] 