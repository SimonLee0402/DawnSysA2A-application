# A2A工作流平台启动脚本 - VSCode集成终端版本
Write-Host "正在启动A2A工作流平台..." -ForegroundColor Cyan
Write-Host ""

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path

# 创建一个新的作业来运行后端服务器
$backendJob = Start-Job -ScriptBlock {
    param($path)
    Set-Location "$path\a2a_workflow_platform"
    python manage.py runserver
} -ArgumentList $scriptPath

# 创建一个新的作业来运行前端服务器
$frontendJob = Start-Job -ScriptBlock {
    param($path)
    Set-Location "$path\a2a_workflow_platform\frontend_src"
    npm run dev
} -ArgumentList $scriptPath

Write-Host "后端和前端服务器已启动为后台作业" -ForegroundColor Green
Write-Host ""

# 实时显示后端作业的输出
Write-Host "显示后端服务器输出 (按 Ctrl+C 停止查看但保持服务器运行):" -ForegroundColor Yellow
try {
    Receive-Job -Job $backendJob -Wait
}
catch {
    Write-Host "停止查看后端输出，但服务器仍在运行" -ForegroundColor Cyan
}

Write-Host ""
Write-Host "服务器启动完成！" -ForegroundColor Cyan
Write-Host "请在浏览器访问 http://localhost:3000" -ForegroundColor Yellow
Write-Host "要查看服务器输出，请使用以下命令:" -ForegroundColor Yellow
Write-Host "Receive-Job -Job `$backendJob" -ForegroundColor Gray
Write-Host "Receive-Job -Job `$frontendJob" -ForegroundColor Gray
Write-Host ""
Write-Host "要停止服务器，请使用以下命令:" -ForegroundColor Yellow
Write-Host "Stop-Job -Job `$backendJob, `$frontendJob; Remove-Job -Job `$backendJob, `$frontendJob" -ForegroundColor Gray
Write-Host ""

# 将作业变量导出到全局会话，以便用户可以在脚本完成后管理它们
$global:backendJob = $backendJob
$global:frontendJob = $frontendJob 