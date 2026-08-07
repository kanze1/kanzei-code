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

# 终端 `kzapp` 必须与快捷方式启动同一个二进制:删掉 cargo bin 的副本后,
# 这里放一个转发启动器补回该能力——它没有自己的版本,不可能再产生第二份旧版(D-145)。
# 必须在 try 之前装:app 正在运行时下面会走 pending 分支并 throw,放在后面就永远装不上。
$launcher = "$env:USERPROFILE\.cargo\bin\kzapp.cmd"
# 注释用英文:.cmd 由 cmd.exe 按 OEM 代码页读取,中文注释在任何编码下都会显示为乱码。
@"
@echo off
rem kanzei desktop launcher. The desktop app has exactly ONE install location
rem (%LOCALAPPDATA%\kanzei); this file only forwards to it.
rem Never put kzapp.exe back into ~\.cargo\bin -- two copies updated by two
rem different channels is exactly what caused D-145 ("shipped but still running old").
start "" "%LOCALAPPDATA%\kanzei\kzapp.exe" %*
"@ | Set-Content $launcher -Encoding ASCII

try {
    Copy-Item $app_source $app_destination -Force -ErrorAction Stop
    # 硬校验:安装位内容必须与本次构建逐字节一致,杜绝"发布成功但仍在跑旧版"。
    if ((Get-FileHash $app_source).Hash -ne (Get-FileHash $app_destination).Hash) {
        throw "安装校验失败:$app_destination 与本次构建不一致"
    }
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

$stale = "$env:USERPROFILE\.cargo\bin\kzapp.exe"
if (Test-Path $stale) {
    Write-Host "警告:$stale 仍存在(可能正在运行),它是旧版且不再被更新;关闭后重跑本脚本清理。" -ForegroundColor Yellow
}

Write-Host "==> installed:" -ForegroundColor Green
kz --version
"kzapp.exe $((Get-Item $app_destination).Length / 1MB -as [int]) MB -> $app_destination"
"启动器: $launcher(终端输入 kzapp 即转发到上面这一份)"
