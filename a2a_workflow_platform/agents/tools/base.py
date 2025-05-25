from abc import ABC, abstractmethod
from typing import Dict, Any

class BaseTool(ABC):
    """Abstract base class for all tools that an agent can use."""

    # Every tool must have a unique name. This name will be used in agent configuration
    # and by the LLM to specify which tool to use.
    name: str
    name_zh: str # Chinese name
    # A description that the LLM can use to understand what the tool does.
    description: str
    description_zh: str # Chinese description
    parameters: Dict[str, Dict[str, Any]] # JSON Schema for parameters

    @abstractmethod
    def execute(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """Executes the tool with the given parameters."""
        pass

    def get_schema(self) -> Dict[str, Any]:
        """Returns the schema for the tool, including parameters."""
        return {
            "name": self.name,
            "name_zh": self.name_zh,
            "description": self.description,
            "description_zh": self.description_zh,
            "parameters": self.parameters
        } 