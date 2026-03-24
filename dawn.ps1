Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Set-Location -LiteralPath $PSScriptRoot

$nodeExe = Join-Path $PSScriptRoot 'dawn_node\target\debug\dawn_node.exe'
$manifest = Join-Path $PSScriptRoot 'dawn_node\Cargo.toml'

if ($args.Count -eq 0) {
    if (Test-Path -LiteralPath $nodeExe) {
        & $nodeExe start --app
    } else {
        cargo run --manifest-path $manifest -- start --app
    }
    exit $LASTEXITCODE
}

if (Test-Path -LiteralPath $nodeExe) {
    & $nodeExe @args
} else {
    cargo run --manifest-path $manifest -- @args
}

exit $LASTEXITCODE
