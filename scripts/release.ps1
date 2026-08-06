# kanzei 发包流程:测试 → release 构建 → 安装到 ~/.cargo/bin(已在 PATH)。
# 用法:  .\scripts\release.ps1            # 完整流程
#        .\scripts\release.ps1 -SkipTests # 跳过测试快速装
param([switch]$SkipTests)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

# 确保 cargo 在 PATH(新终端由 rustup 配好;兜底手动加)。
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
}

if (-not $SkipTests) {
    Write-Host "==> cargo test --workspace" -ForegroundColor Cyan
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "tests failed" }
}

# 版本信息:git 短 hash + 构建日期,注入 kz --version。
$hash = (git rev-parse --short HEAD 2>$null); if (-not $hash) { $hash = "nogit" }
$env:KANZEI_BUILD_INFO = "$hash $(Get-Date -Format yyyy-MM-dd)"

Write-Host "==> cargo install --path crates/kanzei" -ForegroundColor Cyan
cargo install --path crates/kanzei --force
if ($LASTEXITCODE -ne 0) { throw "install failed" }

Write-Host "==> cargo build --release -p kanzei-app (kzapp)" -ForegroundColor Cyan
cargo build --release -p kanzei-app
if ($LASTEXITCODE -ne 0) { throw "app build failed" }
Copy-Item "$root\target\release\kzapp.exe" "$env:USERPROFILE\.cargo\bin\kzapp.exe" -Force

Write-Host "==> installed:" -ForegroundColor Green
kz --version
"kzapp.exe $((Get-Item "$env:USERPROFILE\.cargo\bin\kzapp.exe").Length / 1MB -as [int]) MB -> 桌面端,任意终端输入 kzapp 启动"
