import json

class LLMClient:
    """A stub for an LLM client, to be replaced with a real implementation."""

    def __init__(self, api_key: str = None, model_name: str = "mock-model"):
        """
        Initializes the LLM client.
        Args:
            api_key (str, optional): The API key for the LLM service. Defaults to None.
            model_name (str, optional): The model name to use. Defaults to "mock-model".
        """
        self.api_key = api_key
        self.model_name = model_name
        print(f"LLMClient initialized with model: {self.model_name}")

    def generate_response(self, prompt: str, tools_schema: list = None) -> dict:
        """
        Generates a response from the LLM based on the prompt and available tools.

        Args:
            prompt (str): The input prompt for the LLM.
            tools_schema (list, optional): A list of schemas for tools the LLM can request to use.
                                        Each schema should be a dict, e.g., from ToolManager.

        Returns:
            dict: A dictionary representing the LLM's response. 
                  This could be a direct text answer or a tool call request.
                  Example text answer: {"type": "text", "content": "Hello there!"}
                  Example tool call: {
                      "type": "tool_call", 
                      "tool_name": "calculator", 
                      "tool_params": {"expression": "2+2"}
                  }
        """
        print(f"LLMClient received prompt: {prompt}")
        if tools_schema:
            print(f"LLMClient received tools schema: {json.dumps(tools_schema, indent=2)}")

        # Mock behavior: If prompt mentions "calculate", simulate a tool call
        if "calculate" in prompt.lower() or "what is" in prompt.lower():
            # Try to extract a simple expression if possible, otherwise use a default
            expression_to_calculate = "2+2" # Default
            if "calculate" in prompt.lower():
                try:
                    expression_to_calculate = prompt.lower().split("calculate")[-1].strip()
                    if not expression_to_calculate: expression_to_calculate = "5*3" # fallback
                except:
                    pass # Keep default
            elif "what is" in prompt.lower():
                try:
                    expression_to_calculate = prompt.lower().split("what is")[-1].strip()
                    # Remove question mark if present
                    if expression_to_calculate.endswith("?"):
                        expression_to_calculate = expression_to_calculate[:-1].strip()
                    if not expression_to_calculate: expression_to_calculate = "10-3" # fallback
                except:
                    pass # Keep default
            
            print(f"LLMClient simulating tool call for: {expression_to_calculate}")
            return {
                "type": "tool_call",
                "tool_name": "calculator",
                "tool_params": {"expression": expression_to_calculate}
            }
        # Mock behavior: If prompt mentions "hello" or "hi", simulate a greeting
        elif "hello" in prompt.lower() or "hi" in prompt.lower():
            print("LLMClient simulating a text greeting response.")
            return {"type": "text", "content": "Hello! How can I help you today?"}
        # Default mock response
        else:
            print("LLMClient simulating a generic text response.")
            return {"type": "text", "content": "I am a mock LLM. I received your prompt."}

# Example Usage (for testing this stub):
if __name__ == '__main__':
    client = LLMClient()
    
    # Test with a prompt that should trigger a greeting
    greeting_prompt = "Hi there!"
    response = client.generate_response(prompt=greeting_prompt)
    print(f"Prompt: {greeting_prompt}\nResponse: {response}\n")

    # Test with a prompt that should trigger a calculator tool call
    calc_prompt_1 = "Can you calculate 25/5 for me?"
    # Example tool schema (simplified for this test)
    example_tools = [
        {
            "name": "calculator",
            "description": "Useful for evaluating mathematical expressions.",
            "parameters": {
                "type": "object",
                "properties": {"expression": {"type": "string"}},
                "required": ["expression"]
            }
        }
    ]
    response = client.generate_response(prompt=calc_prompt_1, tools_schema=example_tools)
    print(f"Prompt: {calc_prompt_1}\nResponse: {response}\n")

    calc_prompt_2 = "what is 12 * 3?"
    response = client.generate_response(prompt=calc_prompt_2, tools_schema=example_tools)
    print(f"Prompt: {calc_prompt_2}\nResponse: {response}\n")

    # Test with a generic prompt
    generic_prompt = "Tell me about yourself."
    response = client.generate_response(prompt=generic_prompt)
    print(f"Prompt: {generic_prompt}\nResponse: {response}\n") 