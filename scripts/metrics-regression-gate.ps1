param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"
# verify.ps1 可能把 FileSystem provider 名称附在本地路径前。
$providerPrefix = "Microsoft.PowerShell.Core\FileSystem::"
if ($Root.StartsWith($providerPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    $Root = $Root.Substring($providerPrefix.Length)
}
# PowerShell 的文件系统 cmdlet 在 Windows 扩展路径(`\\?\`)下无法稳定执行
# Test-Path/Get-Content；verify.ps1 从同一工作树启动时会把扩展或 provider-qualified 前缀传入 Root。
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

# D-555:量测必须与基线同口径。基线由源码构建的 kz 生成;安装版 ~/.cargo/bin/kz.exe
# 在口径修复(R-300 B5)未随包发布时会量出假回涨(实测 phase_pipeline.rs 安装版
# 923/源码版 796,基线 796)。构建当前工作树的 kz 再量,闸门与基线生成器永远共用
# 同一计数实现。
cargo build -q -p kanzei --manifest-path (Join-Path $Root "Cargo.toml")
if ($LASTEXITCODE -ne 0) {
    throw "cargo build -p kanzei failed (exit=$LASTEXITCODE)"
}
$kz = Join-Path $Root "target\debug\kz.exe"
Push-Location $Root
try {
    $metricsOutput = @(& $kz metrics --top 30 2>&1)
} finally {
    Pop-Location
}
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
