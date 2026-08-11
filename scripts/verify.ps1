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

# 每条 native command 后立刻检查退出码，不把 LASTEXITCODE 跨函数/脚本块传递(D-255)。
Write-Host "==> fmt" -ForegroundColor Cyan
cargo fmt --all --manifest-path "$root\Cargo.toml" -- --check
if ($LASTEXITCODE -ne 0) { throw "fmt 失败(exit=$LASTEXITCODE)" }
$checks["fmt"] = "pass"

# R-146(clippy)启用时必须同步修改 .github/workflows/ci.yml：两处门禁清单保持一致。
Write-Host "==> clippy" -ForegroundColor Cyan
cargo clippy --workspace --all-targets --manifest-path "$root\Cargo.toml" -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "clippy 失败(exit=$LASTEXITCODE)" }
$checks["clippy"] = "pass"

Write-Host "==> test" -ForegroundColor Cyan
cargo test --workspace --manifest-path "$root\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "test 失败(exit=$LASTEXITCODE)" }
$checks["test"] = "pass"

Write-Host "==> ui_syntax" -ForegroundColor Cyan
Get-ChildItem "$root\crates\kanzei-app\ui\*.js" | ForEach-Object {
    node --check $_.FullName
    if ($LASTEXITCODE -ne 0) { throw "node --check 失败: $($_.Name)" }
}
$checks["ui_syntax"] = "pass"

Write-Host "==> ui_runtime" -ForegroundColor Cyan
node "$root\scripts\ui-runtime-smoke.mjs"
if ($LASTEXITCODE -ne 0) { throw "ui_runtime 失败(exit=$LASTEXITCODE)" }
$checks["ui_runtime"] = "pass"

Write-Host "==> parallel_lines_regression" -ForegroundColor Cyan
node "$root\scripts\parallel-lines-regression.mjs"
if ($LASTEXITCODE -ne 0) { throw "parallel_lines_regression 失败(exit=$LASTEXITCODE)" }
$checks["parallel_lines_regression"] = "pass"

Write-Host "==> ui_a11y" -ForegroundColor Cyan
node "$root\scripts\ui-a11y-smoke.mjs"
if ($LASTEXITCODE -ne 0) { throw "ui_a11y 失败(exit=$LASTEXITCODE)" }
$checks["ui_a11y"] = "pass"

Write-Host "==> ui_i18n" -ForegroundColor Cyan
node "$root\scripts\ui-i18n-smoke.mjs"
if ($LASTEXITCODE -ne 0) { throw "ui_i18n 失败(exit=$LASTEXITCODE)" }
$checks["ui_i18n"] = "pass"

Write-Host "==> ui_markdown" -ForegroundColor Cyan
node "$root\scripts\ui-markdown-smoke.mjs"
if ($LASTEXITCODE -ne 0) { throw "ui_markdown 失败(exit=$LASTEXITCODE)" }
$checks["ui_markdown"] = "pass"

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
[ordered]@{
    commit = $full_hash
    verified_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    checks = $checks
    all_pass = $true
} | ConvertTo-Json | Set-Content "$root\dist\verification.json" -Encoding UTF8
Write-Host "==> 证据已写入 dist\verification.json(commit $full_hash)" -ForegroundColor Green
