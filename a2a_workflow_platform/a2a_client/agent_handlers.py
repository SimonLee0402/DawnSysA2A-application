import json
import uuid
import requests # Keep requests import here as handlers will make calls

# Correct import for default_tool_manager from the agents.tools package __init__
from agents.tools import default_tool_manager

# Logger will be passed as an argument from views.py
# MAX_TOOL_CALL_ITERATIONS will also be defined or passed. Let's assume it's passed for flexibility or defined globally here.
MAX_TOOL_CALL_ITERATIONS = 3

def handle_openai_agent(agent, credential, user_message_content, base_endpoint, api_key, common_headers, logger):
    tool_call_iterations = 0
    final_reply_content = None
    tool_info_for_response = []

    openai_headers = {**common_headers, 'Authorization': f'Bearer {api_key}'}
    messages = [{"role": "user", "content": user_message_content}]
    tools_definition_for_openai = []
    
    if agent.available_tools and isinstance(agent.available_tools, list):
        for tool_name in agent.available_tools:
            tool_instance = default_tool_manager.get_tool(tool_name)
            if tool_instance:
                schema = tool_instance.get_schema()
                if schema: # Ensure schema is not None
                    tools_definition_for_openai.append({
                        "type": "function",
                        "function": schema
                    })
    
    current_payload = {
        'model': agent.model_name,
        'messages': messages,
        **credential.additional_params.get('parameters', {}) # Allow overriding temperature, top_p etc.
    }

    if tools_definition_for_openai:
        current_payload['tools'] = tools_definition_for_openai
        current_payload['tool_choice'] = "auto"

    while tool_call_iterations < MAX_TOOL_CALL_ITERATIONS:
        tool_call_iterations += 1
        logger.debug(f"OpenAI call iteration {tool_call_iterations}, payload: {json.dumps(current_payload, indent=2)}")
        
        response = requests.post(base_endpoint, headers=openai_headers, data=json.dumps(current_payload), timeout=60)
        response.raise_for_status() # This will raise an HTTPError if the HTTP request returned an unsuccessful status code
        response_data = response.json()
        logger.debug(f"OpenAI response iteration {tool_call_iterations}: {json.dumps(response_data, indent=2)}")
        
        if not response_data.get('choices') or not response_data['choices'][0].get('message'):
            logger.error(f"Unexpected OpenAI response structure: {response_data}")
            final_reply_content = "Error: Malformed response from OpenAI."
            break

        response_message = response_data['choices'][0]['message']
        messages.append(response_message) 

        if response_message.get("tool_calls"):
            tool_calls = response_message["tool_calls"]
            tool_iteration_log = {
                "iteration": tool_call_iterations,
                "llm_request_type": "openai_tool_calls",
                "llm_request": tool_calls,
                "executed_tools": []
            }

            tool_results_for_next_call = []
            for tool_call in tool_calls:
                if tool_call.get('type') != 'function':
                    logger.warning(f"Skipping non-function tool_call: {tool_call}")
                    continue

                tool_name = tool_call['function']['name']
                tool_args_str = tool_call['function']['arguments']
                tool_call_id = tool_call['id']
                
                try:
                    tool_args = json.loads(tool_args_str)
                except json.JSONDecodeError:
                    logger.error(f"Failed to parse tool arguments for {tool_name}: {tool_args_str}")
                    tool_results_for_next_call.append({
                        "tool_call_id": tool_call_id,
                        "role": "tool",
                        "name": tool_name,
                        "content": json.dumps({"error": "Invalid arguments format from LLM.", "details": "Could not parse JSON arguments."})
                    })
                    tool_iteration_log["executed_tools"].append({
                        "name": tool_name, "args_str": tool_args_str, "error": "Invalid arguments format (JSON Decode Error)"
                    })
                    continue

                tool_instance = default_tool_manager.get_tool(tool_name)
                executed_tool_info = {"name": tool_name, "args": tool_args}
                if tool_instance and tool_name in (agent.available_tools or []):
                    try:
                        logger.info(f"Executing tool (OpenAI): {tool_name} with args: {tool_args}")
                        execution_result = tool_instance.execute(tool_args)
                        tool_content_result = json.dumps(execution_result) if not isinstance(execution_result, str) else execution_result
                        
                        tool_results_for_next_call.append({
                            "tool_call_id": tool_call_id,
                            "role": "tool",
                            "name": tool_name,
                            "content": tool_content_result
                        })
                        executed_tool_info["result"] = execution_result
                    except Exception as e:
                        logger.error(f"Error executing tool {tool_name} (OpenAI): {str(e)}", exc_info=True)
                        tool_results_for_next_call.append({
                            "tool_call_id": tool_call_id,
                            "role": "tool",
                            "name": tool_name,
                            "content": json.dumps({"error": f"Tool execution failed: {str(e)}"})
                        })
                        executed_tool_info["error"] = str(e)
                else:
                    logger.warning(f"Tool {tool_name} called by OpenAI but not available or not permitted for this agent.")
                    tool_results_for_next_call.append({
                        "tool_call_id": tool_call_id,
                        "role": "tool",
                        "name": tool_name,
                        "content": json.dumps({"error": "Tool not available or not permitted for this agent."})
                    })
                    executed_tool_info["error"] = "Tool not available/permitted"
                tool_iteration_log["executed_tools"].append(executed_tool_info)
            
            tool_info_for_response.append(tool_iteration_log)
            messages.extend(tool_results_for_next_call)
            current_payload['messages'] = messages

            if tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
                final_reply_content = "Max tool call iterations reached. The last assistant message or tool results might be relevant."
                last_assistant_message = next((m.get("content") for m in reversed(messages) if m["role"] == "assistant" and m.get("content")), None)
                if last_assistant_message:
                    final_reply_content = last_assistant_message
                elif tool_iteration_log["executed_tools"]:
                    final_reply_content = f"Max iterations reached. Last tool action: {json.dumps(tool_iteration_log['executed_tools'])}"
                break 
        else: # No tool_calls in the response_message
            final_reply_content = response_message.get("content")
            if final_reply_content is None:
                 final_reply_content = "Assistant did not provide a textual reply after processing."
            break 
    
    if final_reply_content is None and tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
         final_reply_content = "Max tool call iterations reached without a final textual reply from the assistant."
    
    return final_reply_content, tool_info_for_response

def handle_claude_agent(agent, credential, user_message_content, base_endpoint, api_key, common_headers, logger):
    tool_call_iterations = 0
    final_reply_content = None
    tool_info_for_response = []

    claude_headers = {
        **common_headers,
        'x-api-key': api_key,
        'anthropic-version': '2023-06-01', 
    }
    messages_claude = [{"role": "user", "content": user_message_content}]
    system_prompt_claude = agent.system_prompt
    tools_definition_for_claude = []

    if agent.available_tools and isinstance(agent.available_tools, list):
        for tool_name in agent.available_tools:
            tool_instance = default_tool_manager.get_tool(tool_name)
            if tool_instance:
                schema = tool_instance.get_schema()
                if schema:
                    tools_definition_for_claude.append(schema)
    
    current_payload_claude = {
        'model': agent.model_name,
        'messages': messages_claude,
        'max_tokens': credential.additional_params.get('max_tokens', 2048),
        **credential.additional_params.get('parameters', {})
    }
    if system_prompt_claude:
        current_payload_claude['system'] = system_prompt_claude
    
    if tools_definition_for_claude:
            current_payload_claude['tools'] = tools_definition_for_claude

    while tool_call_iterations < MAX_TOOL_CALL_ITERATIONS:
        tool_call_iterations += 1
        logger.debug(f"Claude call iteration {tool_call_iterations}, payload: {json.dumps(current_payload_claude, indent=2)}")

        response = requests.post(base_endpoint, headers=claude_headers, data=json.dumps(current_payload_claude), timeout=90)
        response.raise_for_status()
        response_data = response.json()
        logger.debug(f"Claude response iteration {tool_call_iterations}: {json.dumps(response_data, indent=2)}")

        assistant_response_content_blocks = response_data.get('content', [])
        if not isinstance(assistant_response_content_blocks, list):
            logger.error(f"Unexpected Claude response content structure: {assistant_response_content_blocks}")
            final_reply_content = "Error: Malformed response content from Claude."
            break

        assistant_text_reply = ""
        tool_use_blocks = []

        for block in assistant_response_content_blocks:
            if block.get('type') == 'text':
                assistant_text_reply += block.get('text', '')
            elif block.get('type') == 'tool_use':
                tool_use_blocks.append(block)
        
        messages_claude.append({"role": "assistant", "content": assistant_response_content_blocks})
        
        if tool_use_blocks:
            tool_iteration_log = {
                "iteration": tool_call_iterations,
                "llm_request_type": "claude_tool_use",
                "llm_request": tool_use_blocks,
                "executed_tools": []
            }
            
            tool_results_content_blocks_for_next_call = []
            for tool_block in tool_use_blocks:
                tool_name = tool_block.get('name')
                tool_input = tool_block.get('input', {})
                tool_use_id = tool_block.get('id')

                if not tool_name or not tool_use_id:
                    logger.error(f"Claude tool_use block missing name or id: {tool_block}")
                    tool_results_content_blocks_for_next_call.append({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id or f"error_id_{uuid.uuid4()}",
                        "content": json.dumps({"error": "Malformed tool_use block from LLM (missing name or id)."}),
                        "is_error": True
                    })
                    tool_iteration_log["executed_tools"].append({
                        "name": tool_name or "UnknownTool", "args": tool_input, "error": "Malformed tool_use block (missing name or id)"
                    })
                    continue

                tool_instance = default_tool_manager.get_tool(tool_name)
                executed_tool_info = {"name": tool_name, "args": tool_input, "id": tool_use_id}
                tool_result_content_for_claude = ""
                is_error_result = False

                if tool_instance and tool_name in (agent.available_tools or []):
                    try:
                        logger.info(f"Executing tool (Claude): {tool_name} with args: {tool_input}")
                        execution_result = tool_instance.execute(tool_input)
                        tool_result_content_for_claude = json.dumps(execution_result)
                        executed_tool_info["result"] = execution_result
                    except Exception as e:
                        logger.error(f"Error executing tool {tool_name} (Claude): {str(e)}", exc_info=True)
                        tool_result_content_for_claude = json.dumps({"error": f"Tool execution failed: {str(e)}"})
                        is_error_result = True
                        executed_tool_info["error"] = str(e)
                else:
                    logger.warning(f"Tool {tool_name} called by Claude but not available or not permitted.")
                    tool_result_content_for_claude = json.dumps({"error": "Tool not available or not permitted for this agent."})
                    is_error_result = True
                    executed_tool_info["error"] = "Tool not available/permitted"
                
                tool_results_content_blocks_for_next_call.append({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": tool_result_content_for_claude,
                    **({"is_error": True} if is_error_result else {})
                })
                tool_iteration_log["executed_tools"].append(executed_tool_info)

            tool_info_for_response.append(tool_iteration_log)
            messages_claude.append({"role": "user", "content": tool_results_content_blocks_for_next_call})
            current_payload_claude['messages'] = messages_claude

            if tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
                final_reply_content = "Max tool call iterations reached with Claude."
                if assistant_text_reply:
                    final_reply_content = assistant_text_reply
                elif tool_iteration_log["executed_tools"]:
                    final_reply_content = f"Max iterations. Last tool action: {json.dumps(tool_iteration_log['executed_tools'])}"
                break
        else: 
            final_reply_content = assistant_text_reply
            if not final_reply_content: 
                 stop_reason = response_data.get("stop_reason")
                 if stop_reason == "tool_use": 
                     final_reply_content = "Assistant indicated intent to use a tool but did not provide tool details. Please try rephrasing."
                 else:
                     final_reply_content = "Assistant did not provide a textual reply."
            break 

    if final_reply_content is None and tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
            final_reply_content = "Max tool call iterations reached with Claude without a final textual reply."
    
    return final_reply_content, tool_info_for_response

def handle_gemini_agent(agent, credential, user_message_content, base_endpoint, api_key, common_headers, logger):
    tool_call_iterations = 0
    final_reply_content = None
    tool_info_for_response = []

    gemini_endpoint_url = f"{base_endpoint}/v1beta/models/{agent.model_name}:generateContent?key={api_key}"
    gemini_headers = {**common_headers}

    contents_gemini = [{"role": "user", "parts": [{"text": user_message_content}]}]
    tools_definition_for_gemini = []

    if agent.available_tools and isinstance(agent.available_tools, list):
        function_declarations = []
        for tool_name in agent.available_tools:
            tool_instance = default_tool_manager.get_tool(tool_name)
            if tool_instance:
                schema = tool_instance.get_schema()
                if schema:
                    function_declarations.append(schema) 
        if function_declarations:
            tools_definition_for_gemini = [{"function_declarations": function_declarations}]
    
    current_payload_gemini = {
        'contents': contents_gemini,
        'generationConfig': credential.additional_params.get('generationConfig', {'maxOutputTokens': 2048}),
        **credential.additional_params.get('parameters', {})
    }
    if tools_definition_for_gemini:
        current_payload_gemini['tools'] = tools_definition_for_gemini
    
    while tool_call_iterations < MAX_TOOL_CALL_ITERATIONS:
        tool_call_iterations += 1
        logger.debug(f"Gemini call iteration {tool_call_iterations}, payload: {json.dumps(current_payload_gemini, indent=2)}")
        
        response = requests.post(gemini_endpoint_url, headers=gemini_headers, data=json.dumps(current_payload_gemini), timeout=90)
        response.raise_for_status()
        response_data = response.json()
        logger.debug(f"Gemini response iteration {tool_call_iterations}: {json.dumps(response_data, indent=2)}")

        if not response_data.get('candidates') or \
            not response_data['candidates'][0].get('content') or \
            not isinstance(response_data['candidates'][0]['content'].get('parts'), list):
            logger.error(f"Unexpected Gemini response structure: {response_data}")
            final_reply_content = "Error: Malformed response from Gemini."
            break 

        assistant_content_parts = response_data['candidates'][0]['content']['parts']
        contents_gemini.append(response_data['candidates'][0]['content'])

        assistant_text_reply_gemini = ""
        function_calls_gemini = []

        for part in assistant_content_parts:
            if 'text' in part:
                assistant_text_reply_gemini += part['text']
            elif 'functionCall' in part:
                function_calls_gemini.append(part['functionCall'])
        
        if function_calls_gemini:
            tool_iteration_log = {
                "iteration": tool_call_iterations,
                "llm_request_type": "gemini_function_call",
                "llm_request": function_calls_gemini,
                "executed_tools": []
            }

            function_response_parts_for_next_call = []
            for func_call in function_calls_gemini:
                tool_name = func_call.get('name')
                tool_args = func_call.get('args', {})

                if not tool_name:
                    logger.error(f"Gemini functionCall missing name: {func_call}")
                    function_response_parts_for_next_call.append({
                        "functionResponse": {
                            "name": tool_name or f"error_tool_{uuid.uuid4()}",
                            "response": {"error": "Malformed functionCall from LLM (missing name)."}
                        }
                    })
                    tool_iteration_log["executed_tools"].append({
                        "name": tool_name or "UnknownTool", "args": tool_args, "error": "Malformed functionCall (missing name)"
                    })
                    continue
                
                tool_instance = default_tool_manager.get_tool(tool_name)
                executed_tool_info = {"name": tool_name, "args": tool_args}
                result_payload_for_gemini = {}

                if tool_instance and tool_name in (agent.available_tools or []):
                    try:
                        logger.info(f"Executing tool (Gemini): {tool_name} with args: {tool_args}")
                        execution_result = tool_instance.execute(tool_args)
                        result_payload_for_gemini = execution_result if isinstance(execution_result, dict) else {"result": execution_result}
                        executed_tool_info["result"] = execution_result
                    except Exception as e:
                        logger.error(f"Error executing tool {tool_name} (Gemini): {str(e)}", exc_info=True)
                        result_payload_for_gemini = {"error": f"Tool execution failed: {str(e)}"}
                        executed_tool_info["error"] = str(e)
                else:
                    logger.warning(f"Tool {tool_name} called by Gemini but not available or not permitted.")
                    result_payload_for_gemini = {"error": "Tool not available or not permitted for this agent."}
                    executed_tool_info["error"] = "Tool not available/permitted"
                
                function_response_parts_for_next_call.append({
                    "functionResponse": { 
                        "name": tool_name,
                        "response": result_payload_for_gemini
                    }
                })
                tool_iteration_log["executed_tools"].append(executed_tool_info)
            
            tool_info_for_response.append(tool_iteration_log)
            contents_gemini.append({"role": "tool", "parts": function_response_parts_for_next_call})
            current_payload_gemini['contents'] = contents_gemini

            if tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
                final_reply_content = "Max tool call iterations reached with Gemini."
                if assistant_text_reply_gemini:
                    final_reply_content = assistant_text_reply_gemini
                elif tool_iteration_log["executed_tools"]:
                    final_reply_content = f"Max iterations. Last tool action: {json.dumps(tool_iteration_log['executed_tools'])}"
                break
        else: 
            final_reply_content = assistant_text_reply_gemini
            if not final_reply_content:
                finish_reason = response_data['candidates'][0].get('finishReason', '')
                if finish_reason == "TOOL_CODE": 
                     final_reply_content = "Assistant indicated intent to use a tool but did not provide tool details. Please try rephrasing."
                else:
                     final_reply_content = "Assistant did not provide a textual reply (Gemini)."
            break 
    
    if final_reply_content is None and tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
            final_reply_content = "Max tool call iterations reached with Gemini without a final textual reply."
    
    return final_reply_content, tool_info_for_response

def handle_custom_agent(agent, credential, user_message_content, base_endpoint, api_key, common_headers, logger):
    tool_call_iterations = 0
    final_reply_content = None
    tool_info_for_response = []

    custom_headers = {**common_headers}
    if not credential.additional_params:
        logger.error(f"Custom agent {agent.id} is missing additional_params for API configuration.")
        # This function should raise an exception that views.py can catch and turn into a 500 response.
        # Or return a specific error tuple that views.py can interpret.
        # For now, let's assume it should not directly return a Response object.
        # Returning error string and empty tool_info.
        return "Error: Custom agent configuration is incomplete (missing additional_params).", []

    messages_field = credential.additional_params.get('messages_field', 'messages')
    prompt_field = credential.additional_params.get('prompt_field')
    reply_field = credential.additional_params.get('reply_field', 'reply')
    tool_calls_field = credential.additional_params.get('tool_calls_field', 'tool_calls')
    tool_call_name_field = credential.additional_params.get('tool_call_name_field', 'name')
    tool_call_id_field = credential.additional_params.get('tool_call_id_field', 'id')
    tool_call_args_field = credential.additional_params.get('tool_call_args_field', 'arguments')
    tool_results_field = credential.additional_params.get('tool_results_field', 'tool_results')
    tool_result_call_id_field = credential.additional_params.get('tool_result_call_id_field', 'call_id')
    tool_result_output_field = credential.additional_params.get('tool_result_output_field', 'output')
    
    custom_api_messages = []
    initial_user_message_obj = {"role": credential.additional_params.get('user_role_name', 'user'), 
                                "content": user_message_content}
    custom_api_messages.append(initial_user_message_obj)

    current_payload_custom = credential.additional_params.get('base_payload', {}).copy()
    if prompt_field:
        current_payload_custom[prompt_field] = (current_payload_custom.get(prompt_field, "") + "\n" + user_message_content).strip()
    else: 
        if messages_field not in current_payload_custom:
            current_payload_custom[messages_field] = []
        current_payload_custom[messages_field].extend(custom_api_messages)

    custom_tools_definition_payload_key = credential.additional_params.get('tool_definition_payload_key')
    if agent.available_tools and isinstance(agent.available_tools, list) and custom_tools_definition_payload_key:
        custom_tool_schemas = []
        for tool_name in agent.available_tools:
            tool_instance = default_tool_manager.get_tool(tool_name)
            if tool_instance:
                schema = tool_instance.get_schema()
                if schema: custom_tool_schemas.append(schema)
        if custom_tool_schemas:
            current_payload_custom[custom_tools_definition_payload_key] = custom_tool_schemas
            custom_tool_choice_key = credential.additional_params.get('tool_choice_payload_key')
            custom_tool_choice_value = credential.additional_params.get('tool_choice_value', 'auto')
            if custom_tool_choice_key:
                current_payload_custom[custom_tool_choice_key] = custom_tool_choice_value
    
    while tool_call_iterations < MAX_TOOL_CALL_ITERATIONS:
        tool_call_iterations += 1
        logger.debug(f"Custom Agent call iteration {tool_call_iterations}, payload: {json.dumps(current_payload_custom, indent=2)}")

        response = requests.post(base_endpoint, headers=custom_headers, data=json.dumps(current_payload_custom), timeout=60)
        response.raise_for_status()
        response_data = response.json()
        logger.debug(f"Custom Agent response iteration {tool_call_iterations}: {json.dumps(response_data, indent=2)}")

        assistant_reply_text_custom = response_data.get(reply_field)
        
        if not prompt_field and assistant_reply_text_custom is not None :
            assistant_message_obj_custom = response_data if isinstance(response_data.get(messages_field), list) else \
                                        {"role": credential.additional_params.get('assistant_role_name', 'assistant'), 
                                            "content": assistant_reply_text_custom}
            custom_api_messages.append(assistant_message_obj_custom)

        raw_tool_calls = response_data.get(tool_calls_field)
        parsed_tool_calls_custom = []
        if raw_tool_calls and isinstance(raw_tool_calls, list):
            parsed_tool_calls_custom = raw_tool_calls

        if parsed_tool_calls_custom:
            tool_iteration_log = {
                "iteration": tool_call_iterations,
                "llm_request_type": "custom_agent_tool_calls",
                "llm_request": parsed_tool_calls_custom,
                "executed_tools": []
            }
            
            tool_results_for_custom_api_next_call = []
            for tool_call in parsed_tool_calls_custom:
                if not isinstance(tool_call, dict): 
                    logger.warning(f"Skipping malformed tool_call entry (not a dict): {tool_call}")
                    continue

                tool_name = tool_call.get(tool_call_name_field)
                tool_args = tool_call.get(tool_call_args_field, {})
                tool_call_id = tool_call.get(tool_call_id_field)

                if not tool_name or not tool_call_id:
                    logger.error(f"Custom tool call missing '{tool_call_name_field}' or '{tool_call_id_field}': {tool_call}")
                    error_call_id = tool_call_id or f"error_id_{uuid.uuid4()}"
                    tool_results_for_custom_api_next_call.append({
                        tool_result_call_id_field: error_call_id,
                        tool_result_output_field: {"error": f"Malformed tool call from LLM (missing {tool_call_name_field} or {tool_call_id_field})."}
                    })
                    tool_iteration_log["executed_tools"].append({
                        "name": tool_name or "UnknownTool", "args": tool_args, "id": error_call_id, 
                        "error": f"Malformed tool call (missing {tool_call_name_field} or {tool_call_id_field})"
                    })
                    continue

                tool_instance = default_tool_manager.get_tool(tool_name)
                executed_tool_info = {"name": tool_name, "args": tool_args, "id": tool_call_id}
                current_tool_output = {}

                if tool_instance and tool_name in (agent.available_tools or []):
                    try:
                        logger.info(f"Executing tool (Custom): {tool_name} with args: {tool_args}")
                        execution_result = tool_instance.execute(tool_args)
                        current_tool_output = execution_result
                        executed_tool_info["result"] = execution_result
                    except Exception as e:
                        logger.error(f"Error executing tool {tool_name} (Custom): {str(e)}", exc_info=True)
                        current_tool_output = {"error": f"Tool execution failed: {str(e)}"}
                        executed_tool_info["error"] = str(e)
                else:
                    logger.warning(f"Tool {tool_name} called by Custom Agent but not available or not permitted.")
                    current_tool_output = {"error": "Tool not available or not permitted for this agent."}
                    executed_tool_info["error"] = "Tool not available/permitted"
                
                tool_results_for_custom_api_next_call.append({
                    tool_result_call_id_field: tool_call_id,
                    tool_result_output_field: current_tool_output
                })
                tool_iteration_log["executed_tools"].append(executed_tool_info)
            
            tool_info_for_response.append(tool_iteration_log)

            tool_results_payload_or_message = credential.additional_params.get('tool_results_location', 'payload')
            if tool_results_payload_or_message == 'message' and not prompt_field:
                tool_results_message_role = credential.additional_params.get('tool_results_role_name', 'tool_executor')
                custom_api_messages.append({"role": tool_results_message_role, "content": tool_results_for_custom_api_next_call})
                if tool_results_field in current_payload_custom:
                    current_payload_custom.pop(tool_results_field, None)
            else: 
                current_payload_custom[tool_results_field] = tool_results_for_custom_api_next_call
            
            if not prompt_field:
                current_payload_custom[messages_field] = custom_api_messages

            if tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
                final_reply_content = "Max tool call iterations reached with Custom Agent."
                if assistant_reply_text_custom:
                    final_reply_content = assistant_reply_text_custom
                elif tool_iteration_log["executed_tools"]:
                    final_reply_content = f"Max iterations. Last tool action: {json.dumps(tool_iteration_log['executed_tools'])}"
                break
        else: 
            final_reply_content = assistant_reply_text_custom
            if not final_reply_content:
                final_reply_content = "Assistant (Custom) did not provide a textual reply."
            break 

    if final_reply_content is None and tool_call_iterations >= MAX_TOOL_CALL_ITERATIONS:
            final_reply_content = "Max tool call iterations reached with Custom Agent without a final textual reply."
    
    return final_reply_content, tool_info_for_response

# Potential dispatch dictionary (optional, views.py can also do the dispatch)
# AGENT_HANDLER_MAP = {
#     'openai': handle_openai_agent, # Key would need to be more specific e.g. 'gpt-4'
#     'claude': handle_claude_agent, # Key would need to be more specific e.g. 'claude-3-opus'
#     'gemini': handle_gemini_agent,
#     'custom': handle_custom_agent,
# } 