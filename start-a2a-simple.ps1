Write-Host "正在启动A2A工作流平台..." -ForegroundColor Cyan

# 启动后端
Start-Process -NoNewWindow -FilePath "cmd.exe" -ArgumentList "/c cd a2a_workflow_platform && python manage.py runserver"

# 等待2秒钟
Start-Sleep -Seconds 2

# 启动前端
Start-Process -NoNewWindow -FilePath "cmd.exe" -ArgumentList "/c cd a2a_workflow_platform\frontend_src && npm run dev"

Write-Host "服务器启动完成！" -ForegroundColor Green
Write-Host "后端: http://localhost:8000" -ForegroundColor Yellow
Write-Host "前端: http://localhost:3000" -ForegroundColor Yellow 