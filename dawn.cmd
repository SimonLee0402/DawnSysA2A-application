@echo off
setlocal
cd /d "%~dp0"

set "NODE_EXE=%~dp0dawn_node\target\debug\dawn_node.exe"
set "CARGO_MANIFEST=%~dp0dawn_node\Cargo.toml"

if "%~1"=="" (
  call :run_default
  exit /b %ERRORLEVEL%
)

if exist "%NODE_EXE%" (
  "%NODE_EXE%" %*
  exit /b %ERRORLEVEL%
)

cargo run --manifest-path "%CARGO_MANIFEST%" -- %*
exit /b %ERRORLEVEL%

:run_default
if exist "%NODE_EXE%" (
  "%NODE_EXE%" start --app
  exit /b %ERRORLEVEL%
)

cargo run --manifest-path "%CARGO_MANIFEST%" -- start --app
exit /b %ERRORLEVEL%
