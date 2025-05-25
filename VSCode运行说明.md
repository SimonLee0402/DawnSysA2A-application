# A2A工作流平台 - VSCode运行说明

## 在VSCode集成终端中运行

为了使A2A工作流平台可以在VSCode的集成终端中运行而不打开外部窗口，我们提供了以下解决方案：

### 方法1：使用修改后的PowerShell脚本

1. 在VSCode中，点击菜单 `终端 > 新建终端` 打开集成终端
2. 执行以下命令运行平台：
   ```powershell
   .\Start-A2APlatform.ps1
   ```
3. 脚本会启动后端和前端服务器作为PowerShell作业，并在当前终端显示后端输出
4. 要查看各个服务器的输出，可以使用：
   ```powershell
   Receive-Job -Job $backendJob  # 查看后端输出
   Receive-Job -Job $frontendJob  # 查看前端输出
   ```
5. 要停止服务器，使用：
   ```powershell
   Stop-Job -Job $backendJob, $frontendJob
   Remove-Job -Job $backendJob, $frontendJob
   ```

### 方法2：使用多个终端标签

1. 在VSCode中，点击终端区域的 `+` 按钮打开多个终端标签
2. 在第一个标签中运行后端：
   ```
   cd a2a_workflow_platform
   python manage.py runserver
   ```
3. 在第二个标签中运行前端：
   ```
   cd a2a_workflow_platform/frontend_src
   npm run dev
   ```
4. 使用VSCode终端窗口底部的标签在不同服务器之间切换

## 配置VSCode终端默认为PowerShell

要确保VSCode使用PowerShell作为默认终端：

1. 按下 `Ctrl+,` 打开设置
2. 搜索 "terminal.integrated.defaultProfile.windows"
3. 将其设置为 "PowerShell"

## 解决CSRF错误

如果遇到CSRF错误 "CSRF Failed: Origin checking failed"，请确保：

1. Django设置中已配置了正确的CSRF_TRUSTED_ORIGINS
2. 前端Axios配置了正确的baseURL和withCredentials
3. 登录/注册表单包含csrfmiddlewaretoken字段
4. API端点正确指向Django的登录、注销和注册URL

## 访问应用

前端应用：http://localhost:3000
后端API：http://localhost:8000 