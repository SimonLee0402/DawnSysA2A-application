param(
    [string]$GemmaRoot = 'D:\Gemma 4',
    [string]$Model = 'gemma4-e2b-local',
    [string]$BaseUrl = 'http://127.0.0.1:11434',
    [switch]$SkipOllama
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$workspaceRoot = $PSScriptRoot
$dawnScript = Join-Path $workspaceRoot 'dawn.ps1'

if (-not (Test-Path -LiteralPath $dawnScript)) {
    throw "Cannot find dawn.ps1 under $workspaceRoot"
}

& $dawnScript secrets set OLLAMA_BASE_URL $BaseUrl | Out-Host
& $dawnScript secrets set OLLAMA_DEFAULT_MODEL $Model | Out-Host

$env:OLLAMA_BASE_URL = $BaseUrl
$env:OLLAMA_DEFAULT_MODEL = $Model

if (-not $SkipOllama) {
    $ollamaStartScript = Join-Path $GemmaRoot 'start-ollama.ps1'
    if (-not (Test-Path -LiteralPath $ollamaStartScript)) {
        throw "Cannot find $ollamaStartScript"
    }
    & $ollamaStartScript | Out-Host
}

$gatewayHealthUrl = 'http://127.0.0.1:8000/health'
$gatewayAlreadyRunning = $false
try {
    $response = Invoke-WebRequest -UseBasicParsing -Uri $gatewayHealthUrl -TimeoutSec 2
    if ($response.StatusCode -ge 200 -and $response.StatusCode -lt 300) {
        $gatewayAlreadyRunning = $true
    }
} catch {
}

if ($gatewayAlreadyRunning) {
    Write-Warning 'Dawn gateway is already running. Stored Gemma settings will apply fully after the gateway restarts.'
}

Set-Location -LiteralPath $workspaceRoot
& $dawnScript start --app
exit $LASTEXITCODE
