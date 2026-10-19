# Shared build/test lifecycle. TEMP is restored after every owning task.
function Start-ArtifactRun([string]$Root, [string]$Kind, [string]$Suite, [string]$EvidenceNamespace = "", [ValidateSet("shared", "isolated")][string]$CacheMode = "shared") {
    $target = Join-Path $Root "target"
    if (-not (Test-Path -LiteralPath $target)) { New-Item -ItemType Directory -Path $target | Out-Null }
    $leasedTarget = if ($CacheMode -eq "shared") { $target } else { "-" }
    $runJson = & node (Join-Path $Root "scripts/artifact-lifecycle.mjs") begin $Kind $Suite $PID $leasedTarget $EvidenceNamespace
    if ($LASTEXITCODE -ne 0) { throw "Cannot establish artifact task ownership" }
    $run = ($runJson -join "`n") | ConvertFrom-Json
    $previous = @{}
    foreach ($name in @("TEMP", "TMP", "TMPDIR")) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name)
        [Environment]::SetEnvironmentVariable($name, (Join-Path $run.environment "tmp"))
    }
    $previous["CARGO_TARGET_DIR"] = $env:CARGO_TARGET_DIR
    if ($CacheMode -eq "isolated") { $env:CARGO_TARGET_DIR = Join-Path $run.environment "target" }
    $run | Add-Member -NotePropertyName previous_environment -NotePropertyValue $previous
    return $run
}
function Stop-ArtifactRun([string]$Root, $Run, [bool]$Succeeded) {
    if (-not $Run) { return }
    try {
        $state = if ($Succeeded) { "success" } else { "failure" }
        $result = & node (Join-Path $Root "scripts/artifact-lifecycle.mjs") finish $Run.id $state
        if ($LASTEXITCODE -ne 0) { throw "Artifact environment cleanup failed; retained details are in $($Run.evidence)" }
    } finally {
        foreach ($name in $Run.previous_environment.Keys) { [Environment]::SetEnvironmentVariable($name, $Run.previous_environment[$name]) }
    }
}
function Invoke-ArtifactCommand([string]$Program, [string[]]$Arguments, $Run, [string]$LogName = "command.log") {
    $logPath = Join-Path $Run.evidence $LogName
    $writer = [IO.StreamWriter]::new($logPath, $false, [Text.UTF8Encoding]::new($false))
    $savedPreference = $ErrorActionPreference
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $exitCode = 1
    try {
        $application = Get-Command $Program -CommandType Application -ErrorAction Stop | Select-Object -First 1
        $ErrorActionPreference = "Continue"
        & $application.Source @Arguments 2>&1 | ForEach-Object { $writer.WriteLine($_.ToString()); Write-Host $_.ToString() }
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $savedPreference
        $writer.Dispose()
        $timer.Stop()
        $receipt = [ordered]@{ command = @($Program) + $Arguments; exit_code = $exitCode; elapsed_seconds = $timer.Elapsed.TotalSeconds; log_sha256 = (Get-FileHash -LiteralPath $logPath -Algorithm SHA256).Hash.ToLowerInvariant() }
        [IO.File]::WriteAllText(($logPath + ".json"), ($receipt | ConvertTo-Json -Depth 4), [Text.UTF8Encoding]::new($false))
    }
    return $exitCode
}
function Register-ArtifactRelease([string]$Root, [string]$Commit, [string]$Installer, [string]$Stage, [string]$VerificationId = "", [string]$PackageId = "") {
    $fullCommit = (git -C $Root rev-parse $Commit).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Cannot resolve release source commit" }
    $result = & node (Join-Path $Root "scripts/artifact-lifecycle.mjs") release $fullCommit $Installer $Stage $VerificationId $PackageId
    if ($LASTEXITCODE -ne 0) { throw "Release artifact retention could not be updated" }
}
