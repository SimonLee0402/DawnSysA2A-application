$ErrorActionPreference = "Stop"

Write-Host "正在启动 Dawn Rust 栈..." -ForegroundColor Cyan
Write-Host ""

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path

$gatewayJob = Start-Job -ScriptBlock {
    param($path)
    Set-Location "$path\dawn_core"
    cargo run
} -ArgumentList $scriptPath

$nodeJob = Start-Job -ScriptBlock {
    param($path)
    Set-Location "$path\dawn_node"
    cargo run
} -ArgumentList $scriptPath

Write-Host "已启动后台作业:" -ForegroundColor Green
Write-Host "  DawnCore 网关: http://127.0.0.1:8000" -ForegroundColor Yellow
Write-Host "  DawnNode 节点: ws://127.0.0.1:8000/api/gateway/control-plane/nodes/node-local/session" -ForegroundColor Yellow
Write-Host ""
Write-Host "查看输出:" -ForegroundColor Cyan
Write-Host "  Receive-Job -Job `$gatewayJob" -ForegroundColor Gray
Write-Host "  Receive-Job -Job `$nodeJob" -ForegroundColor Gray
Write-Host ""
Write-Host "停止服务:" -ForegroundColor Cyan
Write-Host "  Stop-Job -Job `$gatewayJob, `$nodeJob; Remove-Job -Job `$gatewayJob, `$nodeJob" -ForegroundColor Gray
Write-Host ""

$global:gatewayJob = $gatewayJob
$global:nodeJob = $nodeJob
