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
function Invoke-Check([string]$name, [scriptblock]$body) {
    Write-Host "==> $name" -ForegroundColor Cyan
    # 在当前作用域执行，确保 native command 的 LASTEXITCODE 不会随子作用域丢失。
    # 否则 cargo fmt --check 失败后仍会继续跑 clippy，门禁会误记 fmt=pass(D-255)。
    . $body
    if ($LASTEXITCODE -ne 0) { throw "$name 失败(exit=$LASTEXITCODE)" }
    $checks[$name] = "pass"
}

# R-146(clippy)启用时必须同步修改 .github/workflows/ci.yml：两处门禁清单保持一致。
Invoke-Check "fmt" { cargo fmt --all --manifest-path "$root\Cargo.toml" -- --check }
Invoke-Check "clippy" { cargo clippy --workspace --all-targets --manifest-path "$root\Cargo.toml" -- -D warnings }
Invoke-Check "test" { cargo test --workspace --manifest-path "$root\Cargo.toml" }
Invoke-Check "ui_syntax" {
    Get-ChildItem "$root\crates\kanzei-app\ui\*.js" | ForEach-Object {
        node --check $_.FullName
        if ($LASTEXITCODE -ne 0) { throw "node --check 失败: $($_.Name)" }
    }
}
Invoke-Check "ui_runtime" { node "$root\scripts\ui-runtime-smoke.mjs" }
Invoke-Check "ui_a11y" { node "$root\scripts\ui-a11y-smoke.mjs" }
Invoke-Check "ui_i18n" { node "$root\scripts\ui-i18n-smoke.mjs" }
Invoke-Check "ui_markdown" { node "$root\scripts\ui-markdown-smoke.mjs" }

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
[ordered]@{
    commit = $full_hash
    verified_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    checks = $checks
    all_pass = $true
} | ConvertTo-Json | Set-Content "$root\dist\verification.json" -Encoding UTF8
Write-Host "==> 证据已写入 dist\verification.json(commit $full_hash)" -ForegroundColor Green
