Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Set-Location -LiteralPath $PSScriptRoot

$manifest = Join-Path $PSScriptRoot 'dawn_node\Cargo.toml'
$remainingArgs = @($args)
$devMode = $env:DAWN_DEV -eq '1' -or $env:DAWN_ALLOW_CARGO_FALLBACK -eq '1'

if ($remainingArgs.Count -gt 0 -and ($remainingArgs[0] -eq '--dev' -or $remainingArgs[0] -eq 'dev')) {
    $devMode = $true
    $remainingArgs = @($remainingArgs | Select-Object -Skip 1)
}

if ($devMode) {
    $env:DAWN_DEV = '1'
}

$nodeExeCandidates = @(
    (Join-Path $PSScriptRoot 'dawn_node.exe'),
    (Join-Path $PSScriptRoot 'bin\dawn_node.exe'),
    (Join-Path $PSScriptRoot 'dawn_node\dawn_node.exe'),
    (Join-Path $PSScriptRoot 'dawn_node\target\release\dawn_node.exe'),
    (Join-Path $PSScriptRoot 'dawn_node\target\debug\dawn_node.exe')
)

$nodeExe = $nodeExeCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1

if ($remainingArgs.Count -eq 0) {
    $remainingArgs = @('start', '--app')
}

if ($nodeExe) {
    & $nodeExe @remainingArgs
    exit $LASTEXITCODE
}

if ($devMode) {
    cargo run --manifest-path $manifest -- @remainingArgs
    exit $LASTEXITCODE
}

Write-Host 'Prebuilt dawn_node.exe was not found.' -ForegroundColor Yellow
Write-Host ''
Write-Host 'For normal users, download the Windows Release installer or ZIP. The release package should include dawn_node.exe and dawn_core.exe and must not require Rust, Visual Studio Build Tools, or link.exe.' -ForegroundColor Cyan
Write-Host ''
Write-Host 'For developers, install the Rust MSVC build environment, then run:' -ForegroundColor Cyan
Write-Host '  .\dawn.ps1 --dev start --app' -ForegroundColor Gray
Write-Host ''
Write-Host 'Release builders or CI should build dawn_node and dawn_core in release mode, then place both executables in the release package.' -ForegroundColor Cyan
exit 1
