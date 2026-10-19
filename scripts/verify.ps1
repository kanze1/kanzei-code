# Verification and CI execute the same named checks from verification-checks.json.
param([switch]$Full, [switch]$Ci)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
. (Join-Path $PSScriptRoot "artifact-lifecycle.ps1")
$artifactRun = $null
$artifactSucceeded = $false
Push-Location $root
try {
    $baselineJson = node scripts/verify-policy.mjs assert-clean $root
    if ($LASTEXITCODE -ne 0) { throw "Build inputs are not clean" }
    $baseline = $baselineJson | ConvertFrom-Json
    $suite = if ($Full -or $Ci) { "verify-full" } else { "verify-targeted" }
    $artifactRun = Start-ArtifactRun $root "verification" $suite "dist/verification-runs"
    # A new run supersedes the previous receipt even when HEAD is unchanged.
    # Failed or interrupted verification must not leave an older green receipt
    # available for packaging the same commit.
    $evidencePath = Join-Path $root "dist/verification.json"
    if (Test-Path -LiteralPath $evidencePath) { Remove-Item -LiteralPath $evidencePath }
    $verifyBase = if ($env:KANZEI_VERIFY_BASE) { $env:KANZEI_VERIFY_BASE } else { "HEAD^" }
    $changedPaths = @(git diff --name-only "$verifyBase..HEAD" -- .)
    if ($LASTEXITCODE -ne 0) { throw "Cannot determine verification range: $verifyBase..HEAD" }
    $pathFile = Join-Path $env:TEMP "kanzei-verify-paths-$PID.json"
    $previousFull = $env:KANZEI_VERIFY_FULL
    try {
        [IO.File]::WriteAllText($pathFile, (ConvertTo-Json -InputObject @($changedPaths)), [Text.UTF8Encoding]::new($false))
        if ($Full -or $Ci) { $env:KANZEI_VERIFY_FULL = "1" }
        $policyJson = node scripts/verify-policy.mjs classify $pathFile
        if ($LASTEXITCODE -ne 0) { throw "Cannot select verification checks" }
        $policy = $policyJson | ConvertFrom-Json
    } finally {
        $env:KANZEI_VERIFY_FULL = $previousFull
        Remove-Item -LiteralPath $pathFile -Force
    }
    $checks = [ordered]@{}
    $runId = $artifactRun.id
    $runDirectory = $artifactRun.evidence
    foreach ($check in $policy.checks) {
        Write-Host "==> $($check.key)" -ForegroundColor Cyan
        $timer = [Diagnostics.Stopwatch]::StartNew()
        $arguments = @($check.args)
        if ($Ci -and $check.ci_args) { $arguments = @($check.ci_args) }
        $logRelative = "dist/verification-runs/$runId/$($check.key).log"
        $logPath = Join-Path $runDirectory "$($check.key).log"
        $writer = [IO.StreamWriter]::new($logPath, $false, [Text.UTF8Encoding]::new($false))
        $result = [ordered]@{ status = "pass"; exit_code = 0; elapsed_seconds = 0.0; command = @($check.program) + $arguments; log_path = $logRelative; log_sha256 = "" }
        try {
            foreach ($dependency in $check.depends_on) {
                if (-not $checks.Contains($dependency) -or $checks[$dependency].status -ne "pass") {
                    throw "Dependency did not pass: $dependency"
                }
            }
            $program = Get-Command $check.program -CommandType Application -ErrorAction Stop | Select-Object -First 1
            $global:LASTEXITCODE = 0
            # Windows PowerShell wraps native stderr as ErrorRecord. Preserve it
            # as process output; only the native exit code determines failure.
            $savedPreference = $ErrorActionPreference
            try {
                $ErrorActionPreference = "Continue"
                & $program.Source @arguments 2>&1 | ForEach-Object {
                    $line = $_.ToString()
                    $writer.WriteLine($line)
                    Write-Host $line
                }
            } finally { $ErrorActionPreference = $savedPreference }
            $result.exit_code = $LASTEXITCODE
            if ($LASTEXITCODE -ne 0) { throw "Check exited with $LASTEXITCODE" }
        } catch {
            $result.status = "fail"
            if ($result.exit_code -eq 0) { $result.exit_code = 1 }
            $result.error = $_.Exception.Message
            $writer.WriteLine($result.error)
            Write-Host "!! $($check.key): $($result.error)" -ForegroundColor Red
        } finally {
            $timer.Stop()
            $writer.Dispose()
            $result.elapsed_seconds = [math]::Round($timer.Elapsed.TotalSeconds, 3)
            $hash = [Security.Cryptography.SHA256]::Create()
            $logStream = [IO.File]::OpenRead($logPath)
            try { $result.log_sha256 = [BitConverter]::ToString($hash.ComputeHash($logStream)).Replace("-", "").ToLowerInvariant() }
            finally { $logStream.Dispose(); $hash.Dispose() }
            $checks[$check.key] = $result
            if ($check.program -eq "cargo") {
                node (Join-Path $root "scripts/artifact-lifecycle.mjs") sample $artifactRun.id $check.key | Out-Null
                if ($LASTEXITCODE -ne 0) { throw "Cannot record build-cache measurement" }
            }
        }
    }
    $allPassed = @($checks.Values | Where-Object { $_.status -ne "pass" }).Count -eq 0
    $runReport = [ordered]@{ commit = $baseline.commit; execution_environment = $(if ($Ci) { "ci" } else { "local" }); checks = $checks; all_pass = $allPassed }
    [IO.File]::WriteAllText((Join-Path $runDirectory "results.json"), ($runReport | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    if (-not $allPassed) { throw "Verification failed; no release evidence was written" }
    # Detect edits and HEAD movement during the checks before publishing evidence.
    node scripts/verify-policy.mjs assert-clean $root $baseline.commit
    if ($LASTEXITCODE -ne 0) { throw "Build inputs changed during verification" }
    $evidence = [ordered]@{
        schema_version = 3
        lifecycle_run_id = $artifactRun.id
        artifact_project_root = $artifactRun.project
        execution_environment = $runReport.execution_environment
        commit = $baseline.commit
        tree = $baseline.tree
        verified_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        mode = $policy.mode
        full_verify = [bool]$policy.full_verify
        verify_base = $verifyBase
        changed_paths = @($changedPaths)
        skipped_steps = @($policy.skipped_steps)
        checks = $checks
        all_pass = $allPassed
    }
    New-Item -ItemType Directory -Force (Join-Path $root "dist") | Out-Null
    [IO.File]::WriteAllText($evidencePath, ($evidence | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    Copy-Item -LiteralPath $evidencePath -Destination (Join-Path $runDirectory "verification.json")
    $artifactSucceeded = $true
    Write-Host "==> Evidence saved for $($baseline.commit) ($($policy.mode))" -ForegroundColor Green
} finally {
    try {
        Stop-ArtifactRun $root $artifactRun $artifactSucceeded
        if ($artifactSucceeded) {
            node (Join-Path $root "scripts/artifact-lifecycle.mjs") pin "verification:$root" $artifactRun.id | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Cannot pin current verification evidence" }
        }
    } catch {
        if ($evidencePath -and (Test-Path -LiteralPath $evidencePath)) { Remove-Item -LiteralPath $evidencePath }
        throw
    } finally { Pop-Location }
}
