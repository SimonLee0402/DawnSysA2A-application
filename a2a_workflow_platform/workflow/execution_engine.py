from .engine import WorkflowEngine, execute_workflow
from django.utils import timezone
from .models import WorkflowInstance

class ExecutionEngine:
    """
    工作流执行引擎封装类
    作为WorkflowEngine的高级接口
    """
    
    def execute_workflow_async(self, instance_id):
        """
        异步执行工作流
        
        Args:
            instance_id: 工作流实例ID
        """
        # 使用引擎提供的delay函数实现异步执行
        execute_workflow.delay(instance_id)
        return True
    
    def resume_workflow(self, instance_id):
        """
        恢复暂停的工作流
        
        Args:
            instance_id: 工作流实例ID
        """
        try:
            instance = WorkflowInstance.objects.get(id=instance_id)
            if instance.status == 'paused':
                instance.status = 'running'
                instance.save()
                
                # 启动引擎继续执行
                execute_workflow.delay(instance_id)
                return True
            return False
        except WorkflowInstance.DoesNotExist:
            return False
    
    def retry_step(self, instance_id, step_id):
        """
        重试失败的工作流步骤
        
        Args:
            instance_id: 工作流实例ID
            step_id: 步骤ID
        """
        try:
            instance = WorkflowInstance.objects.get(id=instance_id)
            # 重置步骤状态后重新执行
            execute_workflow.delay(instance_id)
            return True
        except WorkflowInstance.DoesNotExist:
            return False 