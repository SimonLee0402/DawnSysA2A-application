Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$vsDevCmd = Join-Path $PSScriptRoot '.tools\VSBuildTools\Common7\Tools\VsDevCmd.bat'
if (-not (Test-Path -LiteralPath $vsDevCmd)) {
    throw "Dawn Build Tools were not found at $vsDevCmd"
}

$envLines = cmd /s /c "`"$vsDevCmd`" -arch=x64 -host_arch=x64 >nul && set"
foreach ($line in $envLines) {
    $separator = $line.IndexOf('=')
    if ($separator -le 0) {
        continue
    }
    $name = $line.Substring(0, $separator)
    $value = $line.Substring($separator + 1)
    [Environment]::SetEnvironmentVariable($name, $value, 'Process')
}

Write-Host 'Dawn MSVC Build Tools environment loaded for this PowerShell session.' -ForegroundColor Green
Write-Host 'You can now run cargo build or cargo test commands in this window.' -ForegroundColor Cyan
where.exe link
