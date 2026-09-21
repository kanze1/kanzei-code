param(
    [string]$RuntimeDirectory = (Join-Path $env:USERPROFILE '.kanzei/voice-runtime'),
    [string]$Distribution = 'Ubuntu'
)
$ErrorActionPreference = 'Stop'
& python (Join-Path $PSScriptRoot 'manage.py') stop --root $RuntimeDirectory --distribution $Distribution
if ($LASTEXITCODE -ne 0) { throw 'Voice service could not stop.' }
