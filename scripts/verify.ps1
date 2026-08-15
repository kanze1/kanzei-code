# kanzei 验证证据：门禁全绿后产出绑定 commit 的 dist/verification.json(R-152/A-009)
# 用法: .\scripts\verify.ps1；任何一步失败即中止，不产出证据
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# 证据只绑定 commit：源码必须干净(口径与 package.ps1 一致，含未跟踪源码)
$dirty = @(git -C $root diff --name-only HEAD -- crates scripts .github Cargo.toml Cargo.lock) | Where-Object { $_ }
$untracked = @(git -C $root ls-files --others --exclude-standard -- crates scripts .github) | Where-Object { $_ }
$unclean = @($dirty) + @($untracked)
if ($unclean.Count -gt 0) {
    throw "工作树不干净，证据无法绑定 commit:`n$($unclean -join "`n")"
}
$full_hash = (git -C $root rev-parse HEAD).Trim()

$checks = [ordered]@{}

# R-210:每步记秒数写进 verification.json 的 checks 值——门禁最慢环节从此可答。
# 命令文本保持与 git.rs 门禁/ci.yml 逐项一致(守护测试 gate_checklists_align_across_git_verify_and_ci 机械比对完整检查项集合)。
function Step-With-Timing {
    param([string]$Key, [string]$Label, [scriptblock]$Body)
    Write-Host "==> $Label" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $Body
    if ($LASTEXITCODE -ne 0) { throw "$Label 失败(exit=$LASTEXITCODE)" }
    $sw.Stop()
    $script:checks[$Key] = "pass $([math]::Round($sw.Elapsed.TotalSeconds, 1))s"
}

Step-With-Timing "fmt" "fmt" {
    cargo fmt --all --manifest-path "$root\Cargo.toml" -- --check
}
Step-With-Timing "clippy" "clippy(轻量:不含测试目标)" {
    # 刻意不带 --all-targets：实测碰底层 crate 后 37.9s → 4.9s，省 33 秒。
    # 编译覆盖不靠它——紧接着的 test 步骤会把全部测试代码编一遍；这里丢掉的只有
    # **测试代码的 lint**，那份由 ci.yml 每次 push 的 --all-targets 全量形态兜住。
    # 三处分工(提交门禁 git.rs / 本文件 / ci.yml)由守护测试
    # gate_checklists_align_across_git_verify_and_ci 显式断言，改任一处都要同步。
    cargo clippy --workspace --manifest-path "$root\Cargo.toml" -- -D warnings
}
Step-With-Timing "test" "test" {
    cargo test --workspace --manifest-path "$root\Cargo.toml"
}
Step-With-Timing "ui_syntax" "ui_syntax" {
    Get-ChildItem "$root\crates\kanzei-app\ui\*.js" | ForEach-Object {
        node --check $_.FullName
        if ($LASTEXITCODE -ne 0) { throw "node --check 失败: $($_.Name)" }
    }
}
Step-With-Timing "ui_runtime" "ui_runtime" {
    node "$root\scripts\ui-runtime-smoke.mjs"
}
Step-With-Timing "ui_lint" "ui_lint (R-142 no-undef)" {
    node "$root\scripts\ui-lint-smoke.mjs"
}
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

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
[ordered]@{
    commit = $full_hash
    verified_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    checks = $checks
    all_pass = $true
} | ConvertTo-Json | Set-Content "$root\dist\verification.json" -Encoding UTF8
Write-Host "==> 证据已写入 dist\verification.json(commit $full_hash)" -ForegroundColor Green
