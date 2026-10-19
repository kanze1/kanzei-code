# Fixed-cache development entry. Each invocation owns its temporary test environment.
param(
    [ValidateSet("build", "check", "test")][string]$Command = "check",
    [switch]$Isolated
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot "artifact-lifecycle.ps1")
$run = $null
$succeeded = $false
Push-Location $root
try {
    $cacheMode = if ($Isolated) { "isolated" } else { "shared" }
    $run = Start-ArtifactRun $root $Command "cargo-$Command" "" $cacheMode
    $exitCode = Invoke-ArtifactCommand "cargo" (@($Command) + $args) $run
    if ($exitCode -ne 0) { throw "cargo $Command failed" }
    $succeeded = $true
} finally {
    try { Stop-ArtifactRun $root $run $succeeded } finally { Pop-Location }
}
