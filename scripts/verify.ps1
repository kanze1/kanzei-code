# kanzei 验证证据：门禁全绿后产出绑定 commit 的 dist/verification.json(R-152/A-009)
# 用法: .\scripts\verify.ps1；失败步骤会继续执行并在末尾统一报告，不产出失败证据
param([switch]$Full)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
# Windows 扩展路径可供 node 使用，但 PowerShell Get-ChildItem 对 `\\?\` 通配不稳定；
# 先剥离 provider-qualified 前缀，再剥离本地扩展前缀，避免通配步骤把非空脚本集合误判为空。
$providerPrefix = "Microsoft.PowerShell.Core\FileSystem::"
if ($root.StartsWith($providerPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    $root = $root.Substring($providerPrefix.Length)
}
if ($root.StartsWith("\\?\")) {
    $root = $root.Substring(4)
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# 证据只绑定 commit：源码必须干净(口径与 package.ps1 一致，含未跟踪源码)
$dirty = @(git -C $root diff --name-only HEAD -- crates scripts .github Cargo.toml Cargo.lock) | Where-Object { $_ }
$untracked = @(git -C $root ls-files --others --exclude-standard -- crates scripts .github) | Where-Object { $_ }
$unclean = @($dirty) + @($untracked)
if ($unclean.Count -gt 0) {
    throw "工作树不干净，证据无法绑定 commit:`n$($unclean -join "`n")"
}
$full_hash = (git -C $root rev-parse HEAD).Trim()
$verifyBase = if ($env:KANZEI_VERIFY_BASE) { $env:KANZEI_VERIFY_BASE } else { "HEAD^" }
$changedPaths = @(git -C $root diff --name-only "$verifyBase..HEAD" -- .) | Where-Object { $_ }
$pathFile = Join-Path $env:TEMP "kanzei-verify-paths-$PID.json"
try {
    [IO.File]::WriteAllText(
        $pathFile,
        (@($changedPaths) | ConvertTo-Json -Compress),
        [Text.UTF8Encoding]::new($false)
    )
    $policyJson = node "$root\scripts\verify-policy.mjs" classify $pathFile
    if ($LASTEXITCODE -ne 0) { throw "verify path policy failed" }
    $policy = $policyJson | ConvertFrom-Json
} finally {
    Remove-Item $pathFile -Force -ErrorAction SilentlyContinue
}
if ($Full -or $env:KANZEI_VERIFY_FULL -eq "1") {
    $fullPathFile = Join-Path $env:TEMP "kanzei-verify-paths-$PID-full.json"
    try {
        [IO.File]::WriteAllText($fullPathFile, '["full verify"]', [Text.UTF8Encoding]::new($false))
        $env:KANZEI_VERIFY_FULL = "1"
        $policyJson = node "$root\scripts\verify-policy.mjs" classify $fullPathFile
        if ($LASTEXITCODE -ne 0) { throw "verify full path policy failed" }
        $policy = $policyJson | ConvertFrom-Json
    } finally {
        Remove-Item $fullPathFile -Force -ErrorAction SilentlyContinue
    }
}
Write-Host "==> verify policy: mode=$($policy.mode), rust=$($policy.run_rust), frontend=$($policy.run_frontend), changed=$($changedPaths.Count)" -ForegroundColor Cyan

$checks = [ordered]@{}
$failures = @()

# R-210:每步记秒数写进 verification.json 的 checks 值——门禁最慢环节从此可答。
# 命令文本保持与 git.rs 门禁/ci.yml 逐项一致(守护测试 gate_checklists_align_across_git_verify_and_ci 机械比对完整检查项集合)。
function Step-With-Timing {
    param([string]$Key, [string]$Label, [scriptblock]$Body)
    Write-Host "==> $Label" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        # 不继承上一步外部进程的 LASTEXITCODE；步骤必须用本次执行结果判定。
        $global:LASTEXITCODE = 0
        & $Body
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) { throw "$Label 失败(exit=$exitCode)" }
        $sw.Stop()
        $script:checks[$Key] = "pass $([math]::Round($sw.Elapsed.TotalSeconds, 1))s"
    } catch {
        $sw.Stop()
        $elapsed = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        $message = "$Label 失败: $($_.Exception.Message)"
        $script:checks[$Key] = "fail ${elapsed}s $message"
        $script:failures += $message
        Write-Host "!! $message" -ForegroundColor Red
    }
}

# 按 dist/verification.json 最近一次实测耗时排序：廉价检查先跑，cargo test 最后，先暴露低成本失败。
if ($policy.run_frontend) {
    Step-With-Timing "parallel_lines_regression" "parallel_lines_regression" {
        node "$root\scripts\parallel-lines-regression.mjs"
    }
    Step-With-Timing "ui_a11y" "ui_a11y" {
        node "$root\scripts\ui-a11y-smoke.mjs"
    }
    Step-With-Timing "ui_i18n" "ui_i18n" {
        node "$root\scripts\ui-i18n-smoke.mjs"
    }
    Step-With-Timing "ui_markdown" "ui_markdown" {
        node "$root\scripts\ui-markdown-smoke.mjs"
    }
} else {
    Write-Host "==> skip frontend smoke: no frontend paths in verify range" -ForegroundColor DarkGray
}
Step-With-Timing "crate_sync" "crate_sync (R-266 README 项目结构表与 workspace 一致)" {
    node "$root\scripts\check-design-freshness.mjs"
    if ($LASTEXITCODE -ne 0) { throw "design freshness gate failed (R-318)" }
    node "$root\scripts\check-readme-crates.mjs"
    if ($LASTEXITCODE -ne 0) { throw "README 项目结构表与 Cargo.toml members 不同步(R-266)" }
    & "$root\scripts\metrics-regression-gate.ps1" -Root $root
    if ($LASTEXITCODE -ne 0) { throw "metrics regression gate failed (R-300)" }
}
Step-With-Timing "ps1_bom" "ps1_bom (D-408 含中文的 .ps1 须带 UTF-8 BOM)" {
    node "$root\scripts\check-ps1-bom.mjs"
    if ($LASTEXITCODE -ne 0) { throw "含中文的 PowerShell 脚本缺 UTF-8 BOM,在 Windows PowerShell 5.1 下会解析失败(D-408)" }
}
if ($policy.run_frontend) {
    Step-With-Timing "ui_lint" "ui_lint (R-142 no-undef)" {
        node "$root\scripts\ui-lint-smoke.mjs"
    }
} else {
    Write-Host "==> skip ui_lint: no frontend paths in verify range" -ForegroundColor DarkGray
}
Step-With-Timing "ipc_event_contract" "ipc_event_contract (R-299 emit/listen 求差)" {
    node "$root\scripts\ipc-event-smoke.mjs"
}
if ($policy.run_rust) {
    Step-With-Timing "fmt" "fmt" {
        cargo fmt --all --manifest-path "$root\Cargo.toml" -- --check
    }
} else {
    Write-Host "==> skip fmt: no Rust paths in verify range" -ForegroundColor DarkGray
}
# ui_syntax(node --check)已删(2026-08-20 门禁审计 P0-2):ESLint(ui_lint 步)
# lint 同一批 ui/*.js + mobile-pwa/*.js + scripts/*.mjs,解析错误即 severity 2,
# 语法面完全覆盖;独立 node --check 是纯重复(1.6s + 一处三方同步负担)。
if ($policy.run_rust) {
    Step-With-Timing "clippy" "clippy(轻量:不含测试目标)" {
        # 刻意不带 --all-targets：实测碰底层 crate 后 37.9s → 4.9s，省 33 秒。
        # 编译覆盖不靠它——紧接着的 test 步骤会把全部测试代码编一遍；这里丢掉的只有
        # **测试代码的 lint**，那份由 ci.yml 每次 push 的 --all-targets 全量形态兜住。
        # 三处分工(提交门禁 git.rs / 本文件 / ci.yml)由守护测试
        # gate_checklists_align_across_git_verify_and_ci 显式断言，改任一处都要同步。
        cargo clippy --workspace --manifest-path "$root\Cargo.toml" -- -D warnings
    }
} else {
    Write-Host "==> skip clippy: no Rust paths in verify range" -ForegroundColor DarkGray
}
Step-With-Timing "ui_connectivity" "ui_connectivity" {
    node "$root\scripts\ui-connectivity.mjs"
}
if ($policy.run_frontend) {
    Step-With-Timing "ui_runtime" "ui_runtime" {
        node "$root\scripts\ui-runtime-smoke.mjs"
    }
} else {
    Write-Host "==> skip ui_runtime: no frontend paths in verify range" -ForegroundColor DarkGray
}
if ($policy.run_rust) {
    Step-With-Timing "test" "test" {
        cargo test --workspace --manifest-path "$root\Cargo.toml"
    }
} else {
    Write-Host "==> skip test: no Rust paths in verify range" -ForegroundColor DarkGray
}

if ($failures.Count -gt 0) {
    Write-Host "==> 验证失败：$($failures.Count) 个步骤失败" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "- $_" -ForegroundColor Red }
    throw "验证失败：已报告全部失败步骤，未写入 dist\verification.json"
}

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
[ordered]@{
    commit = $full_hash
    verified_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    mode = $policy.mode
    full_verify = [bool]$policy.full_verify
    verify_base = $verifyBase
    changed_paths = @($changedPaths)
    skipped_steps = @($policy.skipped_steps)
    checks = $checks
    all_pass = $true
} | ConvertTo-Json | Set-Content "$root\dist\verification.json" -Encoding UTF8
Write-Host "==> 证据已写入 dist\verification.json(commit $full_hash)" -ForegroundColor Green
