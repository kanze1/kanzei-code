param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [string]$MetricsOutputPath = ""
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
$baselinePath = Join-Path $Root "scripts\metrics-baseline.json"
if (-not (Test-Path $baselinePath)) {
    throw "metrics baseline not found: $baselinePath"
}

function Normalize-PathKey([string]$Value) {
    return (($Value.Trim() -replace "\\", "/").TrimStart("./"))
}

$contract = Get-Content -LiteralPath $baselinePath -Raw -Encoding UTF8 | ConvertFrom-Json
$baseline = @{}
foreach ($property in $contract.files.PSObject.Properties) {
    $baseline[(Normalize-PathKey $property.Name)] = [int]$property.Value
}
$baselineVersion = $contract.format_version
if ([string]::IsNullOrWhiteSpace($baselineVersion) -or $baseline.Count -lt 10) {
    throw "invalid metrics baseline; refusing a false-green gate"
}

# Exercise the rejection cases before measuring real source. Fixture invocations
# already provide MetricsOutputPath and do not recursively run the self-tests.
if ([string]::IsNullOrWhiteSpace($MetricsOutputPath)) {
    node (Join-Path $Root "scripts/tests/verification/metrics-gate.mjs")
    if ($LASTEXITCODE -ne 0) { throw "metrics gate self-tests failed" }
}

$targetDirectory = $env:CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($targetDirectory)) {
    $targetDirectory = Join-Path $Root "target"
} elseif (-not [IO.Path]::IsPathRooted($targetDirectory)) {
    $targetDirectory = Join-Path $Root $targetDirectory
}
$kz = Join-Path $targetDirectory "debug\kz.exe"
if ([string]::IsNullOrWhiteSpace($MetricsOutputPath)) {
    Push-Location $Root
    try {
        Write-Host "metrics executable: $kz"
        $metricsOutput = @(& $kz metrics --top 1000000 2>&1)
    } finally {
        Pop-Location
    }
    if ($LASTEXITCODE -ne 0) {
        throw "kz metrics failed (exit=$LASTEXITCODE)"
    }
} else {
    # 定向测试注入只含版本行的 fixture，验证口径漂移在解析 full inventory 前拒绝出数。
    $metricsOutput = @(Get-Content -LiteralPath $MetricsOutputPath)
}

$metricsVersion = $null
foreach ($line in $metricsOutput) {
    if ($line -match '^metrics format:\s*(?<version>\S+)') {
        $metricsVersion = $Matches.version
        break
    }
}
if ([string]::IsNullOrWhiteSpace($metricsVersion) -or $metricsVersion -ne $baselineVersion) {
    throw "metrics format version mismatch; refusing to emit metrics: baseline=$baselineVersion current=$metricsVersion"
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
    throw "kz metrics returned no parseable full inventory rows; refusing to pass"
}

$failures = @()
$growthAllowance = [int]$contract.existing_file_growth_budget
$giantThreshold = [int]$contract.max_production_lines
$seen = @{}
foreach ($row in $current) {
    if ($seen.ContainsKey($row.Path)) { throw "duplicate metrics row: $($row.Path)" }
    $seen[$row.Path] = $true
    if ($baseline.ContainsKey($row.Path)) {
        $growth = $row.Production - [int]$baseline[$row.Path]
        if ($growth -gt $growthAllowance) {
            $failures += "$($row.Path): production lines grew $growth (budget $growthAllowance)"
        }
    }
    if ($row.Production -gt $giantThreshold -and
        (-not $baseline.ContainsKey($row.Path) -or [int]$baseline[$row.Path] -le $giantThreshold)) {
        $failures += "$($row.Path): new giant with $($row.Production) production lines (threshold $giantThreshold)"
    }
}
foreach ($entry in $baseline.GetEnumerator()) {
    if ((Test-Path -LiteralPath (Join-Path $Root $entry.Key)) -and -not $seen.ContainsKey($entry.Key)) {
        $failures += "metrics inventory omitted existing baseline file $($entry.Key)"
    }
}
if ($failures.Count -gt 0) { throw "metrics regression gate failed:`n$($failures -join "`n")" }
$giants = @($current | Where-Object { $_.Production -gt $giantThreshold }).Count
Write-Host "metrics regression gate passed: $($current.Count) files, $giants existing giants, no new giants" -ForegroundColor Green
