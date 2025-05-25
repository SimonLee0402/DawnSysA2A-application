@echo off
chcp 65001 > nul
echo 正在启动A2A工作流平台...

:: 打开新的cmd窗口启动后端
start /min cmd /k "cd /d %~dp0\a2a_workflow_platform && python manage.py runserver"

:: 等待2秒
timeout /t 2 > nul

:: 打开新的cmd窗口启动前端
start /min cmd /k "cd /d %~dp0\a2a_workflow_platform\frontend_src && npm run dev"

echo 服务器启动完成！
echo 后端：http://localhost:8000
echo 前端：http://localhost:3000 