# 需要删除的模板文件列表
$templates = @(
    # 工作流相关模板
    "templates/frontend/workflow_confirm_delete.html",
    "templates/frontend/workflow_detail.html",
    "templates/frontend/workflow_edit.html",
    "templates/frontend/workflow_editor.html",
    "templates/frontend/workflow_instance_detail.html",
    "templates/frontend/workflow_instance_list.html",
    "templates/frontend/workflow_list.html",

    # 智能体相关模板
    "templates/frontend/agent_detail.html",
    "templates/frontend/agent_form.html",
    "templates/frontend/agent_list.html",
    "templates/frontend/agent_test.html",
    "templates/frontend/credential_form.html",

    # 会话相关模板
    "templates/frontend/session_confirm_delete.html",
    "templates/frontend/session_detail.html",
    "templates/frontend/session_form.html",
    "templates/frontend/session_list.html",

    # 任务相关模板
    "templates/frontend/task_create.html",
    "templates/frontend/task_create_stream.html",
    "templates/frontend/task_detail.html",
    "templates/frontend/task_list.html"
)

# 遍历并删除每个文件
foreach ($template in $templates) {
    if (Test-Path $template) {
        Write-Host "Deleting: $template"
        Remove-Item -Path $template
    } else {
        Write-Host "File not found: $template"
    }
}

Write-Host "Clean-up completed!" 