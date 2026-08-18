param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"
# PowerShell 的文件系统 cmdlet 在 Windows 扩展路径(`\\?\`)下无法稳定执行
# Test-Path/Get-Content；verify.ps1 从同一工作树启动时会把该前缀传入 Root。
# 仅去掉 Windows 本地盘的扩展前缀，UNC/普通路径保持原样。
if ($Root.StartsWith("\\?\")) {
    $Root = $Root.Substring(4)
}
$baselinePath = Join-Path $Root "docs\design\metrics_baseline.md"
if (-not (Test-Path $baselinePath)) {
    throw "metrics baseline not found: $baselinePath"
}

function Normalize-PathKey([string]$Value) {
    return (($Value.Trim() -replace "\\", "/").TrimStart("./"))
}

$baseline = @{}
foreach ($line in Get-Content -LiteralPath $baselinePath) {
    if ($line -match '^\|\s*\d+\s*\|\s*(?<path>[^|]+?)\s*\|\s*\d+\s*\|\s*(?<production>\d+)\s*\|') {
        $key = Normalize-PathKey $Matches.path
        $baseline[$key] = [int]$Matches.production
    }
}
if ($baseline.Count -lt 10) {
    throw "metrics baseline has too few parsed rows ($($baseline.Count)); refusing to run a false-green gate"
}

$kz = Join-Path $env:USERPROFILE ".cargo\bin\kz.exe"
$metricsOutput = @(& $kz metrics --top 30 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "kz metrics failed (exit=$LASTEXITCODE)"
}

$current = @()
foreach ($line in $metricsOutput) {
    if ($line -match '(?<path>crates[\\/]\S+)\s+(?<total>\d+)\s+(?<production>\d+)\s+(?<tests>\d+)\s+(?<functions>\d+)\s+(?<max_fn>\d+)\s+(?<args>\d+)\s*$') {
        $current += [pscustomobject]@{
            Path = Normalize-PathKey $Matches.path
            Production = [int]$Matches.production
        }
    }
}
if ($current.Count -eq 0) {
    throw "kz metrics returned no parseable Top-30 rows; refusing to pass"
}

$failures = @()
$growthAllowance = 100
foreach ($entry in $baseline.GetEnumerator()) {
    $row = $current | Where-Object { $_.Path -eq $entry.Key } | Select-Object -First 1
    if ($null -eq $row) {
        continue
    }
    $growth = $row.Production - [int]$entry.Value
    if ($growth -gt $growthAllowance) {
        $failures += "$($entry.Key): production lines grew $growth (baseline $($entry.Value), current $($row.Production), allowance $growthAllowance)"
    }
}

$baselineGiantCount = @($baseline.Values | Where-Object { [int]$_ -gt 1200 }).Count
$currentGiantCount = @($current | Where-Object { $_.Production -gt 1200 }).Count
if ($currentGiantCount -gt ($baselineGiantCount + 1)) {
    $failures += "Top-30 giant count grew from $baselineGiantCount to $currentGiantCount (allowance 1)"
}

if ($failures.Count -gt 0) {
    throw "metrics regression gate failed:`n$($failures -join "`n")"
}

Write-Host "metrics regression gate passed: $($current.Count) rows, giants $currentGiantCount/$baselineGiantCount, per-file allowance $growthAllowance" -ForegroundColor Green
