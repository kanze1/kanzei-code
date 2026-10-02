param(
    [string]$RuntimeDirectory = (Join-Path $env:USERPROFILE '.kanzei/voice-runtime'),
    [string]$Distribution = 'Ubuntu'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
if (-not (Test-Path -LiteralPath (Join-Path $root 'service.json'))) { throw 'Run scripts/voice/setup.ps1 first.' }
& python (Join-Path $PSScriptRoot 'manage.py') start --root $root --distribution $Distribution
if ($LASTEXITCODE -ne 0) { throw 'Voice service did not start.' }
