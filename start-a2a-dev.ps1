#!/usr/bin/env pwsh
# A2A工作流平台开发环境启动脚本

# 设置PowerShell环境
$ErrorActionPreference = "Stop"

# 定义颜色函数
function Write-ColorOutput {
    param (
        [Parameter(Mandatory=$true)]
        [string]$Message,
        [Parameter(Mandatory=$false)]
        [string]$ForegroundColor = "White"
    )
    Write-Host $Message -ForegroundColor $ForegroundColor
}

# 显示欢迎信息
Write-ColorOutput "启动A2A工作流平台开发环境..." "Cyan"
Write-ColorOutput "===========================" "Cyan"
Write-ColorOutput ""

# 检查Python环境
try {
    Write-ColorOutput "检查Python环境..." "Yellow"
    $pythonVersion = python --version
    Write-ColorOutput "已找到 $pythonVersion" "Green"
} catch {
    Write-ColorOutput "未找到Python! 请确保Python已安装并添加到PATH中" "Red"
    exit 1
}

# 检查Node.js环境
try {
    Write-ColorOutput "检查Node.js环境..." "Yellow"
    $nodeVersion = node --version
    Write-ColorOutput "已找到 Node.js $nodeVersion" "Green"
} catch {
    Write-ColorOutput "未找到Node.js! 请确保Node.js已安装并添加到PATH中" "Red"
    exit 1
}

# 检查npm环境
try {
    Write-ColorOutput "检查npm环境..." "Yellow"
    $npmVersion = npm --version
    Write-ColorOutput "已找到 npm $npmVersion" "Green"
} catch {
    Write-ColorOutput "未找到npm! 请确保npm已安装并添加到PATH中" "Red"
    exit 1
}

# 配置Django开发环境
$env:DJANGO_SETTINGS_MODULE = "a2a_platform.settings"
$env:DJANGO_DEBUG = "True"

# 切换到项目目录
Set-Location -Path "a2a_workflow_platform"

# 启动后端服务
$backendJob = Start-Job -ScriptBlock {
    Set-Location -Path $using:PWD
    Write-Host "启动Django后端服务..."
    python manage.py runserver 0.0.0.0:8000
}

# 延迟一秒，确保后端服务有时间启动
Start-Sleep -Seconds 1

# 启动前端服务
$frontendJob = Start-Job -ScriptBlock {
    Set-Location -Path "$using:PWD/frontend_src"
    Write-Host "启动Vue前端开发服务器..."
    npm run dev
}

# 显示启动信息
Write-ColorOutput ""
Write-ColorOutput "服务启动中..." "Magenta"
Write-ColorOutput "==============" "Magenta"
Write-ColorOutput "后端服务: http://localhost:8000" "Cyan"
Write-ColorOutput "前端服务: http://localhost:3000" "Cyan"
Write-ColorOutput "应用入口: http://localhost:8000/" "Green"
Write-ColorOutput ""
Write-ColorOutput "按Ctrl+C停止所有服务" "Yellow"

# 等待用户中断
try {
    # 持续接收和显示作业输出
    while ($true) {
        Receive-Job -Job $backendJob
        Receive-Job -Job $frontendJob
        Start-Sleep -Seconds 1
    }
} finally {
    # 清理作业
    Write-ColorOutput "正在停止服务..." "Yellow"
    Stop-Job -Job $backendJob
    Stop-Job -Job $frontendJob
    Remove-Job -Job $backendJob
    Remove-Job -Job $frontendJob
    Write-ColorOutput "服务已停止。" "Green"
} 