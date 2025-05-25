from django.shortcuts import render
from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework.permissions import IsAuthenticated

from workflow.models import Workflow, WorkflowInstance
from a2a_client.models import Agent, Task

# Create your views here.

# 假设的模型导入路径 - 需要根据实际情况调整
# from workflow.models import Workflow, WorkflowInstance
# from a2a_client.models import Agent # 示例，Agent模型位置待确认
# from task.models import Task # 示例，Task模型位置待确认

class DashboardStatsView(APIView):
    permission_classes = [IsAuthenticated]

    def get(self, request, *args, **kwargs):
        user = request.user
        stats = {
            'workflows': 0,
            'running_instances': 0,
            'completed_instances': 0,
            'failed_instances': 0,
            'agents': 0,
            'tasks_total': 0,
            'tasks_completed': 0,
        }

        try:
            # 工作流统计
            stats['workflows'] = Workflow.objects.filter(created_by=user).count()
            
            user_workflows = Workflow.objects.filter(created_by=user)
            stats['running_instances'] = WorkflowInstance.objects.filter(workflow__in=user_workflows, status='RUNNING').count()
            stats['completed_instances'] = WorkflowInstance.objects.filter(workflow__in=user_workflows, status='COMPLETED').count()
            stats['failed_instances'] = WorkflowInstance.objects.filter(workflow__in=user_workflows, status='FAILED').count()

            # 智能体统计
            stats['agents'] = Agent.objects.filter(owner=user).count()

            # 任务统计 (通过 Agent 的 owner 关联到用户)
            user_agents = Agent.objects.filter(owner=user)
            stats['tasks_total'] = Task.objects.filter(agent__in=user_agents).count()
            stats['tasks_completed'] = Task.objects.filter(agent__in=user_agents, state='COMPLETED').count() # 假设 Task 的完成状态字段是 'state' 且值为 'COMPLETED'
            
        except Exception as e:
            # 实际生产中应记录更详细的错误日志
            print(f"Error fetching dashboard stats for user {user.username}: {e}")
            # 可以选择返回部分成功的数据或一个错误响应
            # 为了简单起见，如果出错，当前会返回初始的零值 stats，但前端会使用映射后的字段
            pass # 保持默认的0值如果查询失败

        # 为了确保前端能接收到正确的字段名，我们在这里映射一下
        # 前端 Home.vue 中使用的是: dashboardData.workflows, dashboardData.runningInstances, dashboardData.agents, dashboardData.tasks
        response_data = {
            'workflows': stats['workflows'],
            'runningInstances': stats['running_instances'],
            'agents': stats['agents'],
            'tasks': stats['tasks_total'],
            # 可以选择性添加更多数据到响应中，如果前端需要
            # 'completedInstances': stats['completed_instances'],
            # 'failedInstances': stats['failed_instances'],
            # 'tasksCompleted': stats['tasks_completed'],
        }

        return Response(response_data)
