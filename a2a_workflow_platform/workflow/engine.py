import logging
import json
import traceback
from datetime import datetime
from django.utils import timezone
from django.db import transaction
from asgiref.sync import async_to_sync
from channels.layers import get_channel_layer
from .models import WorkflowInstance, WorkflowStep, A2AAgent
from a2a_client.models import Task, Message, Part

logger = logging.getLogger(__name__)
channel_layer = get_channel_layer()

def start_workflow_execution(instance_id):
    """
    启动工作流执行的入口函数
    
    Args:
        instance_id: 工作流实例ID
    """
    try:
        engine = WorkflowEngine(instance_id)
        engine.start()
    except Exception as e:
        logger.error(f"执行工作流实例 {instance_id} 时发生错误: {str(e)}")
        logger.error(traceback.format_exc())
        # 更新实例状态为失败
        try:
            instance = WorkflowInstance.objects.get(instance_id=instance_id)
            instance.status = 'failed'
            instance.error_message = str(e)
            instance.completed_at = timezone.now()
            instance.save()
            
            # 发送WebSocket通知
            try:
                async_to_sync(channel_layer.group_send)(
                    f'workflow_instance_{instance_id}',
                    {
                        'type': 'instance_update',
                        'instance': {
                            'status': 'failed',
                            'error_message': str(e),
                            'completed_at': instance.completed_at.isoformat() if instance.completed_at else None
                        }
                    }
                )
            except Exception as ws_error:
                logger.error(f"发送WebSocket通知时发生错误: {str(ws_error)}")
                
        except Exception as update_error:
            logger.error(f"更新失败状态时发生错误: {str(update_error)}")

# 模拟Celery的delay方法，使用线程实现异步执行
def delay(instance_id):
    """
    模拟Celery的delay方法，使用线程异步执行工作流
    """
    import threading
    t = threading.Thread(target=start_workflow_execution, args=(instance_id,))
    t.daemon = True
    t.start()
    return t

# 添加delay方法作为start_workflow_execution的属性，模拟Celery任务
start_workflow_execution.delay = delay

class WorkflowEngine:
    """
    工作流执行引擎
    负责解析和执行工作流定义，处理工作流实例的运行流程
    """
    
    def __init__(self, instance_id):
        """
        初始化工作流引擎
        
        Args:
            instance_id: 工作流实例ID (UUID)
        """
        self.instance = WorkflowInstance.objects.get(instance_id=instance_id)
        self.workflow = self.instance.workflow
        self.definition = self.workflow.definition
        self.context = self.instance.context or {}
        self.instance_group_name = f'workflow_instance_{self.instance.instance_id}'
        
    def _send_update(self):
        """
        向WebSocket发送实例更新消息
        """
        try:
            # 通过channels向前端发送更新
            async_to_sync(channel_layer.group_send)(
                self.instance_group_name,
                {
                    'type': 'instance_update',
                    'message': 'Instance updated'
                }
            )
        except Exception as e:
            logger.error(f"向WebSocket发送更新失败: {str(e)}")

    def start(self):
        """
        启动工作流实例
        """
        logger.info(f"启动工作流实例: {self.instance.instance_id}")
        
        try:
            # 更新实例状态
            with transaction.atomic():
                self.instance.status = 'running'
                self.instance.started_at = timezone.now()
                self.instance.save()
                
                # 发送更新
                self._send_update()
                
                # 开始执行工作流
                self._execute_workflow()
                
        except Exception as e:
            logger.error(f"工作流启动失败: {str(e)}")
            logger.error(traceback.format_exc())
            
            # 更新实例状态为失败
            self.instance.status = 'failed'
            self.instance.error = str(e)
            self.instance.save()
            
            # 发送更新
            self._send_update()
            
            raise e
    
    def _execute_workflow(self):
        """
        执行工作流
        """
        steps = self.definition.get('steps', [])
        
        # 如果没有步骤，直接完成
        if not steps:
            self._complete_workflow()
            return
        
        # 从当前步骤开始执行
        current_index = self.instance.current_step_index or 0
        
        # 执行工作流，直到所有步骤完成，或遇到异步步骤
        while current_index < len(steps):
            step = steps[current_index]
            
            # 记录当前步骤索引
            self.instance.current_step_index = current_index
            self.instance.save()
            
            # 发送更新
            self._send_update()
            
            # 检查是否需要执行该步骤（条件逻辑）
            if not self._should_execute_step(step):
                logger.info(f"步骤 {current_index} 条件不满足，跳过")
                current_index += 1
                continue
            
            # 执行步骤
            try:
                # 创建或获取步骤记录
                step_record, created = WorkflowStep.objects.get_or_create(
                    instance=self.instance,
                    step_index=current_index,
                    defaults={
                        'step_id': step.get('id', f'step_{current_index}'),
                        'step_name': step.get('name', f'步骤 {current_index+1}'),
                        'step_type': step.get('type', 'unknown'),
                        'parameters': step.get('parameters', {}),
                        'status': 'pending',
                    }
                )
                
                # 如果步骤已经完成，直接移动到下一步
                if step_record.status == 'completed':
                    current_index += 1
                    continue
                
                # 更新步骤状态
                step_record.status = 'running'
                step_record.started_at = timezone.now()
                step_record.save()
                
                # 发送更新
                self._send_update()
                
                # 执行步骤
                result = self._execute_step(step, step_record)
                
                # 如果步骤需要异步执行（比如等待A2A任务完成），暂停工作流
                if result.get('status') == 'async':
                    logger.info(f"步骤 {current_index} 异步执行中，暂停工作流")
                    return
                
                # 更新步骤状态
                step_record.status = 'completed'
                step_record.completed_at = timezone.now()
                step_record.output_data = result.get('output', {})
                step_record.save()
                
                # 发送更新
                self._send_update()
                
                # 更新上下文
                if 'output' in result:
                    self._update_context(step, result['output'])
                
                # 检查特殊流程控制
                flow_control = result.get('flow_control', {})
                
                if flow_control.get('type') == 'jump':
                    # 跳转到指定步骤
                    target = flow_control.get('target')
                    if isinstance(target, int):
                        current_index = target
                        continue
                    elif isinstance(target, str):
                        # 按步骤ID查找
                        for i, s in enumerate(steps):
                            if s.get('id') == target:
                                current_index = i
                                break
                        else:
                            # 如果没找到，继续下一步
                            current_index += 1
                    else:
                        current_index += 1
                else:
                    # 常规顺序执行，移动到下一步
                    current_index += 1
                
            except Exception as e:
                logger.error(f"步骤 {current_index} 执行失败: {str(e)}")
                logger.error(traceback.format_exc())
                
                # 更新步骤状态
                if step_record:
                    step_record.status = 'failed'
                    step_record.error = str(e)
                    step_record.save()
                
                # 更新实例状态
                self.instance.status = 'failed'
                self.instance.error = f"步骤 {current_index} 执行失败: {str(e)}"
                self.instance.save()
                
                # 发送更新
                self._send_update()
                
                # 跳出循环
                return
        
        # 所有步骤执行完成
        self._complete_workflow()
    
    def _complete_workflow(self):
        """完成工作流"""
        logger.info(f"工作流实例完成: {self.instance.instance_id}")
        
        # 更新实例状态
        self.instance.status = 'completed'
        self.instance.completed_at = timezone.now()
        
        # 处理输出
        if 'output' in self.definition:
            output_mapping = self.definition['output']
            output = {}
            for key, path in output_mapping.items():
                output[key] = self._get_context_value(path)
            self.instance.output = output
        
        self.instance.save()
        
        # 发送更新
        self._send_update()
    
    def _should_execute_step(self, step):
        """
        检查是否应该执行步骤（条件判断）
        
        Args:
            step: 步骤定义
        
        Returns:
            bool: 是否应该执行
        """
        if 'condition' not in step:
            return True
        
        condition = step['condition']
        
        if isinstance(condition, bool):
            return condition
        
        if isinstance(condition, dict):
            return self._evaluate_condition(condition)
        
        return True
    
    def _evaluate_condition(self, condition):
        """
        评估条件表达式
        
        Args:
            condition: 条件表达式
        
        Returns:
            bool: 条件是否满足
        """
        if 'operator' not in condition:
            return True
        
        operator = condition['operator']
        
        if operator == 'equals':
            left = self._resolve_value(condition.get('left', None))
            right = self._resolve_value(condition.get('right', None))
            return left == right
        
        elif operator == 'not_equals':
            left = self._resolve_value(condition.get('left', None))
            right = self._resolve_value(condition.get('right', None))
            return left != right
        
        elif operator == 'greater_than':
            left = self._resolve_value(condition.get('left', None))
            right = self._resolve_value(condition.get('right', None))
            return left > right
        
        elif operator == 'less_than':
            left = self._resolve_value(condition.get('left', None))
            right = self._resolve_value(condition.get('right', None))
            return left < right
        
        elif operator == 'contains':
            container = self._resolve_value(condition.get('container', None))
            item = self._resolve_value(condition.get('item', None))
            if container is None:
                return False
            return item in container
        
        elif operator == 'and':
            conditions = condition.get('conditions', [])
            return all(self._evaluate_condition(cond) for cond in conditions)
        
        elif operator == 'or':
            conditions = condition.get('conditions', [])
            return any(self._evaluate_condition(cond) for cond in conditions)
        
        elif operator == 'not':
            subcondition = condition.get('condition', {})
            return not self._evaluate_condition(subcondition)
        
        # 默认返回True
        return True
    
    def _resolve_value(self, value_expr):
        """
        解析值表达式
        
        Args:
            value_expr: 值表达式，可以是直接值或变量引用
        
        Returns:
            解析后的值
        """
        if not value_expr:
            return None
        
        if isinstance(value_expr, dict) and 'variable' in value_expr:
            var_path = value_expr['variable']
            return self._get_context_value(var_path)
        
        return value_expr
    
    def _get_context_value(self, path):
        """
        从上下文中获取变量值
        
        Args:
            path: 变量路径，形如 "step1.output.result"
        
        Returns:
            变量值
        """
        if not path:
            return None
        
        parts = path.split('.')
        value = self.context
        
        for part in parts:
            if isinstance(value, dict) and part in value:
                value = value[part]
            elif isinstance(value, list) and part.isdigit():
                index = int(part)
                if 0 <= index < len(value):
                    value = value[index]
                else:
                    return None
            else:
                return None
        
        return value
    
    def _update_context(self, step, output):
        """
        更新上下文数据
        
        Args:
            step: 步骤定义
            output: 步骤输出
        """
        step_id = step.get('id', f'step_{self.instance.current_step_index}')
        
        # 更新上下文
        if 'steps' not in self.context:
            self.context['steps'] = {}
        
        self.context['steps'][step_id] = {
            'output': output
        }
        
        # 如果有指定的输出映射，则按映射更新上下文
        if 'output_mapping' in step:
            for dest, src_path in step['output_mapping'].items():
                parts = src_path.split('.')
                src_value = output
                
                for part in parts:
                    if isinstance(src_value, dict) and part in src_value:
                        src_value = src_value[part]
                    else:
                        src_value = None
                        break
                
                if src_value is not None:
                    dest_parts = dest.split('.')
                    if len(dest_parts) == 1:
                        self.context[dest] = src_value
                    else:
                        # 创建嵌套字典
                        target = self.context
                        for i, part in enumerate(dest_parts[:-1]):
                            if part not in target:
                                target[part] = {}
                            target = target[part]
                        target[dest_parts[-1]] = src_value
        
        # 保存更新后的上下文
        self.instance.context = self.context
        self.instance.save()
        
        # 发送更新
        self._send_update()
    
    def _execute_step(self, step, step_record):
        """
        执行单个步骤
        
        Args:
            step: 步骤定义
            step_record: 步骤记录对象
        
        Returns:
            dict: 执行结果
        """
        step_type = step.get('type', 'unknown')
        
        # 解析步骤参数（替换变量引用）
        params = self._resolve_parameters(step.get('parameters', {}))
        
        # 保存解析后的参数
        step_record.input_data = params
        step_record.save()
        
        # 根据步骤类型执行不同的处理
        if step_type == 'a2a_client':
            return self._execute_a2a_step(step, params, step_record)
        elif step_type == 'condition':
            return self._execute_condition_step(step, params)
        elif step_type == 'loop':
            return self._execute_loop_step(step, params)
        elif step_type == 'transform':
            return self._execute_transform_step(step, params)
        else:
            logger.warning(f"未知的步骤类型: {step_type}")
            return {'output': {}, 'status': 'completed'}
    
    def _resolve_parameters(self, params):
        """
        解析步骤参数，替换其中的变量引用
        
        Args:
            params: 原始参数
        
        Returns:
            dict: 解析后的参数
        """
        if isinstance(params, dict):
            result = {}
            for key, value in params.items():
                result[key] = self._resolve_parameters(value)
            return result
        
        elif isinstance(params, list):
            return [self._resolve_parameters(item) for item in params]
        
        elif isinstance(params, str):
            # 处理字符串中的变量引用 ${variable}
            if '${' in params and '}' in params:
                # 简单的字符串模板替换
                result = params
                start = params.find('${')
                
                while start != -1:
                    end = result.find('}', start)
                    if end == -1:
                        break
                    
                    var_path = result[start+2:end]
                    var_value = self._get_context_value(var_path)
                    
                    # 替换变量
                    if var_value is not None:
                        if isinstance(var_value, (dict, list)):
                            # 如果是复杂类型，替换整个字符串
                            return var_value
                        else:
                            # 简单类型，替换占位符
                            result = result[:start] + str(var_value) + result[end+1:]
                            
                            # 更新下一个搜索位置
                            start = result.find('${', start + len(str(var_value)))
                    else:
                        # 跳过这个占位符
                        start = result.find('${', end)
                
                return result
            return params
        else:
            return params
    
    def _execute_a2a_step(self, step, params, step_record):
        """
        执行A2A客户端步骤
        
        Args:
            step: 步骤定义
            params: 解析后的参数
            step_record: 步骤记录对象
            
        Returns:
            dict: 执行结果
        """
        from a2a_client.models import Agent, Task, Message, Part
        
        agent_id = params.get('agent_id')
        if not agent_id:
            raise ValueError("A2A步骤缺少agent_id参数")
        
        try:
            agent = Agent.objects.get(id=agent_id)
        except Agent.DoesNotExist:
            raise ValueError(f"找不到ID为{agent_id}的Agent")
        
        # 创建A2A任务
        task = Task.objects.create(
            agent=agent,
            state='submitted',
            metadata={
                'workflow_instance_id': str(self.instance.instance_id),
                'workflow_step_id': step_record.id,
            }
        )
        
        # 记录任务ID
        step_record.a2a_task_id = task.id
        step_record.a2a_task_status = task.state
        step_record.save()
        
        # 创建用户消息
        message = Message.objects.create(
            task=task,
            role='user',
        )
        
        # 添加消息内容
        content = params.get('message', '')
        Part.objects.create(
            message=message,
            part_type='text',
            content_type='text/plain',
            text_content=content
        )
        
        # 在真实场景中，这里应该调用A2A客户端发送任务
        # 对于异步任务，这里应该返回需要等待的状态
        
        # TODO: 如果这是一个同步调用，可以等待结果并返回
        # 在本示例中，我们假设所有A2A任务都是异步的
        
        return {
            'status': 'async',
            'task_id': str(task.id)
        }
    
    def _execute_condition_step(self, step, params):
        """
        执行条件步骤
        
        Args:
            step: 步骤定义
            params: 解析后的参数
            
        Returns:
            dict: 执行结果
        """
        condition = params.get('condition', {})
        condition_result = self._evaluate_condition(condition)
        
        # 根据条件结果跳转
        then_path = params.get('then', None)
        else_path = params.get('else', None)
        
        if condition_result and then_path:
            return {
                'output': {'result': True},
                'flow_control': {
                    'type': 'jump',
                    'target': then_path
                }
            }
        elif not condition_result and else_path:
            return {
                'output': {'result': False},
                'flow_control': {
                    'type': 'jump',
                    'target': else_path
                }
            }
        
        return {
            'output': {'result': condition_result},
            'status': 'completed'
        }
    
    def _execute_loop_step(self, step, params):
        """
        执行循环步骤
        
        Args:
            step: 步骤定义
            params: 解析后的参数
            
        Returns:
            dict: 执行结果
        """
        loop_type = params.get('loop_type', 'foreach')
        
        if loop_type == 'foreach':
            # 获取要遍历的列表
            items = params.get('items', [])
            
            # 确保items是列表
            if not isinstance(items, list):
                if isinstance(items, dict):
                    # 如果是字典，转换为列表
                    items = list(items.values())
                else:
                    # 尝试转换为列表，如果失败则创建单元素列表
                    try:
                        items = list(items)
                    except (TypeError, ValueError):
                        items = [items] if items is not None else []
            
            # 获取循环变量名
            index_var = params.get('index_var', 'loop_index')
            item_var = params.get('item_var', 'loop_item')
            
            # 获取当前循环索引
            current_index = self.context.get(index_var, 0)
            
            # 检查是否已完成循环
            if current_index >= len(items):
                # 循环完成，重置循环变量
                self.context.pop(index_var, None)
                self.context.pop(item_var, None)
                self.instance.context = self.context
                self.instance.save()
                
                # 记录循环完成
                logger.info(f"Foreach循环完成，共{len(items)}项")
                
                # 跳转到循环结束步骤
                end_step = params.get('end_step')
                if end_step:
                    return {
                        'output': {'completed': True, 'iterations': len(items)},
                        'flow_control': {
                            'type': 'jump',
                            'target': end_step
                        }
                    }
                
                return {
                    'output': {'completed': True, 'iterations': len(items)},
                    'status': 'completed'
                }
            
            # 设置循环变量
            self.context[index_var] = current_index
            self.context[item_var] = items[current_index]
            self.instance.context = self.context
            self.instance.save()
            
            logger.info(f"Foreach循环执行第{current_index+1}项，共{len(items)}项")
            
            # 如果这是新的迭代，跳转到循环体步骤
            body_step = params.get('body_step')
            if body_step:
                return {
                    'output': {
                        'index': current_index,
                        'item': items[current_index],
                        'completed': False
                    },
                    'flow_control': {
                        'type': 'jump',
                        'target': body_step
                    }
                }
            
            # 如果没有指定循环体步骤，自动进行下一次迭代
            self.context[index_var] = current_index + 1
            self.instance.context = self.context
            self.instance.save()
            
            # 递归调用自身进行下一次迭代
            return self._execute_loop_step(step, params)
                
        elif loop_type == 'while':
            condition = params.get('condition', {})
            iteration_var = params.get('iteration_var', 'loop_iteration')
            
            # 获取当前迭代次数
            current_iteration = self.context.get(iteration_var, 0)
            
            # 检查最大迭代次数
            max_iterations = params.get('max_iterations', 100)
            if current_iteration >= max_iterations:
                # 达到最大迭代次数，重置循环变量
                self.context.pop(iteration_var, None)
                self.instance.context = self.context
                self.instance.save()
                
                logger.warning(f"While循环达到最大迭代次数: {max_iterations}")
                
                # 跳转到循环结束步骤
                end_step = params.get('end_step')
                if end_step:
                    return {
                        'output': {'completed': True, 'iterations': current_iteration, 'max_reached': True},
                        'flow_control': {
                            'type': 'jump',
                            'target': end_step
                        }
                    }
                
                return {
                    'output': {'completed': True, 'iterations': current_iteration, 'max_reached': True},
                    'status': 'completed'
                }
            
            # 评估条件
            condition_result = self._evaluate_condition(condition)
            
            if not condition_result:
                # 条件不满足，退出循环
                # 重置循环变量
                self.context.pop(iteration_var, None)
                self.instance.context = self.context
                self.instance.save()
                
                logger.info(f"While循环条件不满足，完成循环，共迭代{current_iteration}次")
                
                # 跳转到循环结束步骤
                end_step = params.get('end_step')
                if end_step:
                    return {
                        'output': {'completed': True, 'iterations': current_iteration, 'condition': False},
                        'flow_control': {
                            'type': 'jump',
                            'target': end_step
                        }
                    }
                
                return {
                    'output': {'completed': True, 'iterations': current_iteration, 'condition': False},
                    'status': 'completed'
                }
            
            # 条件满足，继续循环
            logger.info(f"While循环条件满足，执行第{current_iteration+1}次迭代")
            
            # 更新迭代次数
            self.context[iteration_var] = current_iteration + 1
            self.instance.context = self.context
            self.instance.save()
            
            # 跳转到循环体步骤
            body_step = params.get('body_step')
            if body_step:
                return {
                    'output': {
                        'iteration': current_iteration,
                        'completed': False
                    },
                    'flow_control': {
                        'type': 'jump',
                        'target': body_step
                    }
                }
            
            # 如果没有指定循环体步骤，自动进行下一次迭代
            # 递归调用自身进行下一次评估
            return self._execute_loop_step(step, params)
        
        return {
            'output': {'error': f"不支持的循环类型: {loop_type}"},
            'status': 'completed'
        }
    
    def _execute_transform_step(self, step, params):
        """
        执行数据转换步骤
        
        Args:
            step: 步骤定义
            params: 解析后的参数
            
        Returns:
            dict: 执行结果
        """
        input_data = params.get('input', {})
        transform_type = params.get('transform_type', 'map')
        
        if transform_type == 'map':
            mapping = params.get('mapping', {})
            result = {}
            
            for key, value_expr in mapping.items():
                if isinstance(value_expr, str) and value_expr.startswith('$input.'):
                    # 直接映射输入字段
                    path = value_expr[7:]  # 移除 "$input."
                    value = self._get_nested_value(input_data, path.split('.'))
                    if value is not None:
                        result[key] = value
                else:
                    # 使用原始值
                    result[key] = value_expr
            
            return {
                'output': result,
                'status': 'completed'
            }
            
        elif transform_type == 'filter':
            items = input_data.get('items', [])
            condition = params.get('condition', {})
            
            filtered_items = []
            for item in items:
                # 临时将当前项目放入上下文
                self.context['current_item'] = item
                
                # 评估条件
                if self._evaluate_condition(condition):
                    filtered_items.append(item)
                
                # 清理临时上下文
                self.context.pop('current_item', None)
            
            return {
                'output': {'items': filtered_items, 'count': len(filtered_items)},
                'status': 'completed'
            }
            
        elif transform_type == 'merge':
            objects = params.get('objects', [])
            result = {}
            
            for obj in objects:
                if isinstance(obj, dict):
                    result.update(obj)
            
            return {
                'output': result,
                'status': 'completed'
            }
        
        return {
            'output': {'error': f"不支持的转换类型: {transform_type}"},
            'status': 'completed'
        }
    
    def _get_nested_value(self, data, path_parts):
        """
        从嵌套数据结构中获取值
        
        Args:
            data: 嵌套的数据结构
            path_parts: 路径部分列表
        
        Returns:
            找到的值，或者None
        """
        current = data
        for part in path_parts:
            if isinstance(current, dict) and part in current:
                current = current[part]
            elif isinstance(current, list) and part.isdigit():
                index = int(part)
                if 0 <= index < len(current):
                    current = current[index]
                else:
                    return None
            else:
                return None
        return current
        
    def resume(self, task_id, task_result):
        """
        恢复被异步任务暂停的工作流
        
        Args:
            task_id: 异步任务ID
            task_result: 任务结果
        """
        logger.info(f"恢复工作流实例: {self.instance.instance_id}, 任务: {task_id}")
        
        # 查找对应的步骤记录
        try:
            step_record = WorkflowStep.objects.get(
                instance=self.instance,
                a2a_task_id=task_id
            )
        except WorkflowStep.DoesNotExist:
            logger.error(f"找不到对应的步骤记录: {task_id}")
            return
        
        # 更新步骤状态
        step_record.status = 'completed'
        step_record.completed_at = timezone.now()
        step_record.output_data = task_result
        step_record.save()
        
        # 发送更新
        self._send_update()
        
        # 更新上下文
        steps = self.definition.get('steps', [])
        current_index = step_record.step_index
        if 0 <= current_index < len(steps):
            step = steps[current_index]
            self._update_context(step, task_result)
        
        # 继续执行工作流（从下一步开始）
        self.instance.current_step_index = step_record.step_index + 1
        self.instance.save()
        
        # 发送更新
        self._send_update()
        
        # 继续执行
        self._execute_workflow()
    
    def execute(self):
        """
        执行工作流（作为start方法的别名）
        """
        return self.start()

def execute_workflow(instance_id):
    """
    执行工作流的辅助函数
    
    Args:
        instance_id: 工作流实例ID
    """
    engine = WorkflowEngine(instance_id)
    engine.start()
    
def resume_workflow(instance_id, task_id, task_result):
    """
    恢复工作流的辅助函数
    
    Args:
        instance_id: 工作流实例ID
        task_id: 异步任务ID
        task_result: 任务结果
    """
    engine = WorkflowEngine(instance_id)
    engine.resume(task_id, task_result) 