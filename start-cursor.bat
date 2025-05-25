@echo off
chcp 65001 > nul
echo 正在启动A2A工作流平台(Cursor终端版)...

echo 启动后端服务...
cd /d %~dp0\a2a_workflow_platform
start /b python manage.py runserver

echo 等待后端服务启动...
timeout /t 3 > nul

echo 启动前端服务...
cd /d %~dp0\a2a_workflow_platform\frontend_src
npm run dev

echo 服务器启动完成！
echo 后端：http://localhost:8000
echo 前端：http://localhost:3000 