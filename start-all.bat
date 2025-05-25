@echo off
chcp 65001 > nul
echo 正在启动A2A工作流平台...
echo.

echo 启动后端服务器...
start cmd /k "chcp 65001 > nul && cd /d %~dp0\a2a_workflow_platform && python manage.py runserver"

echo 启动前端开发服务器...
start cmd /k "chcp 65001 > nul && cd /d %~dp0\a2a_workflow_platform\frontend_src && npm run dev"

echo.
echo 服务器启动完成！
echo 请在浏览器访问 http://localhost:3000
echo. 