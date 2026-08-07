# kanzei 发包流程:测试 → release 构建 → 安装到实际桌面端目录与 ~/.cargo/bin(kz) CLI。
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

$app_source = "$root\target\release\kzapp.exe"
$app_dir = "$env:LOCALAPPDATA\kanzei"
$app_destination = "$app_dir\kzapp.exe"
New-Item -ItemType Directory -Force $app_dir | Out-Null
try {
    Copy-Item $app_source $app_destination -Force -ErrorAction Stop
    # 本次直接安装成功时清理旧 pending，避免下次启动重复回滚/覆盖。
    Remove-Item "$app_destination.pending" -Force -ErrorAction SilentlyContinue
    # 桌面端统一由 LocalAppData 运行；cargo bin 只保留 kz CLI，清理历史桌面副本。
    Remove-Item "$env:USERPROFILE\.cargo\bin\kzapp.exe" -Force -ErrorAction SilentlyContinue
    Remove-Item "$env:USERPROFILE\.cargo\bin\kzapp.exe.pending" -Force -ErrorAction SilentlyContinue
} catch {
    # Windows 不允许覆盖正在运行的 exe。保留待安装副本，避免构建成功但新版本丢失。
    $pending_destination = "$app_destination.pending"
    Copy-Item $app_source $pending_destination -Force -ErrorAction Stop
    Write-Host "应用构建成功，但安装失败：$app_destination 可能正在运行。" -ForegroundColor Yellow
    Write-Host "新版本已保存到：$pending_destination；关闭 kzapp 后下次启动将自动完成更新。" -ForegroundColor Yellow
    throw "app build succeeded, installation deferred to next startup"
}

Write-Host "==> installed:" -ForegroundColor Green
kz --version
"kzapp.exe $((Get-Item $app_destination).Length / 1MB -as [int]) MB -> $app_destination"
