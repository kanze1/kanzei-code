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
    $isFull = $Full -or $Ci
    $inputCommand = if ($isFull) { "assert-clean" } else { "inspect" }
    $baselineJson = node scripts/verify-policy.mjs $inputCommand $root
    if ($LASTEXITCODE -ne 0) { throw "Cannot establish verification inputs" }
    $baseline = $baselineJson | ConvertFrom-Json
    $suite = if ($Full -or $Ci) { "verify-full" } else { "verify-targeted" }
    $artifactRun = Start-ArtifactRun $root "verification" $suite "dist/verification-runs"
    # A new run supersedes the previous receipt even when HEAD is unchanged.
    # Failed or interrupted verification must not leave an older green receipt
    # available for packaging the same commit.
    $receiptName = if ($isFull) { "verification.json" } else { "verification-targeted.json" }
    $evidencePath = Join-Path $root "dist/$receiptName"
    if (Test-Path -LiteralPath $evidencePath) { Remove-Item -LiteralPath $evidencePath }
    $verifyBase = if ($env:KANZEI_VERIFY_BASE) { $env:KANZEI_VERIFY_BASE } elseif ($baseline.dirty_paths.Count -gt 0) { "HEAD" } else { "HEAD^" }
    $changedPaths = @(git diff --name-only "$verifyBase..HEAD" -- .)
    if ($LASTEXITCODE -ne 0) { throw "Cannot determine verification range: $verifyBase..HEAD" }
    if (-not $isFull) { $changedPaths = @($changedPaths) + @($baseline.dirty_paths) | Sort-Object -Unique }
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
    $runId = $artifactRun.id
    $runDirectory = $artifactRun.evidence
    $planFile = Join-Path $runDirectory "plan.json"
    [IO.File]::WriteAllText($planFile, $policyJson, [Text.UTF8Encoding]::new($false))
    $executionEnvironment = if ($Ci) { "ci" } else { "local" }
    node scripts/verify-runner.mjs $planFile $runDirectory $runId $executionEnvironment
    $runReport = Get-Content -Raw -Encoding UTF8 (Join-Path $runDirectory "results.json") | ConvertFrom-Json
    $runReport | Add-Member -NotePropertyName commit -NotePropertyValue $baseline.commit
    [IO.File]::WriteAllText((Join-Path $runDirectory "results.json"), ($runReport | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    $checks = $runReport.checks
    $allPassed = $runReport.all_pass
    # Record the completed Cargo graph once, after every producer has stopped.
    if (@($policy.checks | Where-Object { $_.program -eq "cargo" -or $_.resources -contains "cargo" }).Count -gt 0) {
        node scripts/artifact-lifecycle.mjs sample $artifactRun.id "verification" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Cannot record build-cache measurement" }
    }
    if (-not $allPassed) { throw "Verification failed; no release evidence was written" }
    # Detect edits and HEAD movement during the checks before publishing evidence.
    if ($isFull) {
        node scripts/verify-policy.mjs assert-clean $root $baseline.commit
    } else {
        $baselineFile = Join-Path $runDirectory "inputs.json"
        [IO.File]::WriteAllText($baselineFile, $baselineJson, [Text.UTF8Encoding]::new($false))
        node scripts/verify-policy.mjs inspect $root $baselineFile
    }
    if ($LASTEXITCODE -ne 0) { throw "Build inputs changed during verification" }
    $evidence = [ordered]@{
        schema_version = 3
        lifecycle_run_id = $artifactRun.id
        artifact_project_root = $artifactRun.project
        execution_environment = $runReport.execution_environment
        commit = $baseline.commit
        tree = $baseline.tree
        worktree_sha256 = $baseline.worktree_sha256
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
            $pinName = if ($isFull) { "verification:$root" } else { "verification-targeted:$root" }
            node (Join-Path $root "scripts/artifact-lifecycle.mjs") pin $pinName $artifactRun.id | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Cannot pin current verification evidence" }
        }
    } catch {
        if ($evidencePath -and (Test-Path -LiteralPath $evidencePath)) { Remove-Item -LiteralPath $evidencePath }
        throw
    } finally { Pop-Location }
}
