# 偶发红加压脚本(R-211):循环 N 次跑目标测试,统计失败率并存档失败输出。
# 用法:
#   .\scripts\stress-test.ps1 -Rounds 20                          # 全量 20 轮
#   .\scripts\stress-test.ps1 -Target "kanzei-tools" -Rounds 30   # 单 crate 30 轮
#   .\scripts\stress-test.ps1 -Target "kanzei-tools" -Filter "docstore::" -Rounds 50
#   .\scripts\stress-test.ps1 -Rounds 10 -Parallel 2              # 并行 2 轮(仅 -Target 单 crate 时)
#
# 输出:机械结论「连续 N 次全绿」或「N 次内命中 M 次失败」;失败详情存档到
# output/stress-<时间戳>/round-N.log,可回查(验收②)。
# 约定:偶发红一律先跑它出数字再定位,不靠"重跑一次就绿"当证据(D-293 教训)。
param(
    [string]$Target = "",
    [int]$Rounds = 20,
    [int]$Parallel = 1,
    [string]$Filter = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

if ($Rounds -lt 1) { throw "Rounds 必须 ≥ 1" }
if ($Parallel -lt 1 -or $Parallel -gt 4) { throw "Parallel 必须在 1..4" }
if ($Parallel -gt 1 -and -not $Target) {
    throw "并行轮只支持 -Target 单 crate(全量并行会互相踩 CARGO_TARGET_DIR)"
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$outDir = Join-Path $root "output\stress-$stamp"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# 组装测试命令。Filter 只对 -Target 有意义(cargo test 的 TESTNAME 参数)。
$testArgs = @("test", "--manifest-path", "$root\Cargo.toml")
if ($Target) { $testArgs += @("-p", $Target) }
if ($Filter) { $testArgs += $Filter }
$label = if ($Target) { "cargo test -p $Target$(if ($Filter) { " $Filter" })" } else { "cargo test --workspace" }

Write-Host "==> 压测: $label × $Rounds 轮(并行 $Parallel)" -ForegroundColor Cyan
Write-Host "    存档目录: $outDir"

$failCount = 0
$failRounds = @()

for ($round = 1; $round -le $Rounds; $round++) {
    $ok = $false
    if ($Parallel -gt 1) {
        # 并行:同一轮内同时起 N 个 cargo test 实例(测 CARGO_TARGET_DIR 并发隔离)。
        $jobs = @()
        for ($p = 1; $p -le $Parallel; $p++) {
            $jobs += Start-Job -ScriptBlock {
                param($argsLine, $round, $p)
                $out = & cargo @argsLine 2>&1
                [pscustomobject]@{ Code = $LASTEXITCODE; Output = ($out -join "`n") }
            } -ArgumentList $testArgs, $round, $p
        }
        $results = $jobs | Wait-Job | Receive-Job
        $jobs | Remove-Job
        $ok = ($results | Where-Object { $_.Code -ne 0 }).Count -eq 0
        if (-not $ok) {
            for ($i = 0; $i -lt $results.Count; $i++) {
                if ($results[$i].Code -ne 0) {
                    $logPath = Join-Path $outDir "round-$round-parallel-$i.log"
                    $results[$i].Output | Set-Content -Path $logPath -Encoding utf8
                }
            }
        }
    } else {
        $output = & cargo $testArgs 2>&1
        $code = $LASTEXITCODE
        $ok = ($code -eq 0)
        if (-not $ok) {
            $logPath = Join-Path $outDir "round-$round.log"
            $output | Set-Content -Path $logPath -Encoding utf8
        }
    }

    if ($ok) {
        Write-Host "  round $round/$Rounds ✓" -ForegroundColor Green
    } else {
        $failCount++
        $failRounds += $round
        Write-Host "  round $round/$Rounds ✗ (存档见 output\stress-$stamp)" -ForegroundColor Red
    }
}

# 机械结论(验收①):连续全绿 → 明确说「连续 N 次全绿」;有失败 → 「N 次内命中 M 次」。
$rate = [math]::Round($failCount / $Rounds * 100, 1)
Write-Host ""
if ($failCount -eq 0) {
    $conclusion = "连续 $Rounds 次全绿($label) — 0 失败,失败率 0%"
    Write-Host "结论: $conclusion" -ForegroundColor Green
} else {
    $conclusion = "$Rounds 次内命中 $failCount 次失败($label) — 失败率 $rate%: 第 $($failRounds -join ', ') 轮"
    Write-Host "结论: $conclusion" -ForegroundColor Red
    Write-Host "失败输出存档于: $outDir(可回查,定位后再谈修复)" -ForegroundColor Yellow
}
$summaryPath = Join-Path $outDir "summary.txt"
$conclusion | Set-Content -Path $summaryPath -Encoding utf8
Write-Host "摘要写入: $summaryPath"
