# kanzei 发包流程:测试 → release 构建 → 安装到实际桌面端目录与 ~/.cargo/bin(kz) CLI。
# 用法:  .\scripts\release.ps1            # 完整流程
#        .\scripts\release.ps1 -SkipTests # 跳过测试快速装
#
# 开发通道最低门禁(R-298 留档):cargo test --workspace 全绿(除非显式 -SkipTests)。
# 这是开发通道的下限——能装进本机系统的二进制必须过得了全量测试,不允许"只 build 不测"
# 就把 kz/kzapp 装到机器上。发布通道(package.ps1)门槛更高:它要求 verify.ps1 十步
# 全绿证据绑定 HEAD(A-009),见 docs/design/ci_release_evidence_chain.md。
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

# 版本信息:git 短 hash + UTC 构建时间,注入 kz --version。
$hash = (git rev-parse --short HEAD 2>$null); if (-not $hash) { $hash = "nogit" }
$build_at = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss")
$env:KANZEI_BUILD_INFO = "$hash $build_at"

# `cargo install --path` 默认在自己的私有 target 目录里从头构建，不复用 target/release；
# 紧接着的 kanzei-app 构建又要把同一批依赖重编一遍 = 整整一轮全量 release 构建白烧。
# 改成「构建 → 拷贝」：两步共享 target/release 的依赖产物，落点仍是 ~\.cargo\bin\kz.exe。
Write-Host "==> cargo build --release -p kanzei (kz CLI)" -ForegroundColor Cyan
cargo build --release -p kanzei
if ($LASTEXITCODE -ne 0) { throw "kz build failed" }

$cargo_bin = "$env:USERPROFILE\.cargo\bin"
New-Item -ItemType Directory -Force $cargo_bin | Out-Null
# 与 cargo install --force 同语义：目标正在运行时 Windows 会拒绝覆盖，照样当场报错。
Copy-Item "$root\target\release\kz.exe" "$cargo_bin\kz.exe" -Force
if (-not $?) { throw "install failed" }

Write-Host "==> cargo build --release -p kanzei-app (kzapp)" -ForegroundColor Cyan
cargo build --release -p kanzei-app
if ($LASTEXITCODE -ne 0) { throw "app build failed" }

$app_source = "$root\target\release\kzapp.exe"
$app_dir = "$env:LOCALAPPDATA\kanzei"
$app_destination = "$app_dir\kzapp.exe"

# ---- 容器重定向检测(D-198)----
# 下面那个 Get-FileHash 硬校验有个它自己看不见的前提:写进去的和读回来的是同一个
# 真实文件。在 AppContainer 里(Claude 桌面端的会话进程就是)对 %LOCALAPPDATA% 的写入
# 被重定向到 ...\Packages\<pkg>\LocalCache\Local\,读回也走影子——于是校验在沙箱里
# 跟自己对账、逐字节通过,而用户开始菜单指向的真实 kzapp.exe 一个字节没变。
# 实测 2026-08-09:ccfecff 发版后用户版本卡在 4ad666c,脚本全程报成功。
# 校验不出这种情况,只能在动手之前把它判出来并拒绝——装不上要当场说,不能事后骗。
$probe_name = ".kanzei-install-probe-$PID"
$probe = Join-Path $env:LOCALAPPDATA $probe_name
Set-Content $probe "probe" -Encoding ASCII
$shadow = @(Get-ChildItem "$env:LOCALAPPDATA\Packages\*\LocalCache\Local\$probe_name" -ErrorAction SilentlyContinue)
Remove-Item $probe -Force -ErrorAction SilentlyContinue
foreach ($s in $shadow) { Remove-Item $s.FullName -Force -ErrorAction SilentlyContinue }
if ($shadow.Count -gt 0) {
    Write-Host "kz CLI 已安装到 ~\.cargo\bin(不受重定向影响),仅桌面端这一步无法在此环境完成。" -ForegroundColor Yellow
    throw @"
%LOCALAPPDATA% 的写入被容器重定向到 $($shadow[0].DirectoryName)
在这里装桌面端只会装进影子目录,而且本脚本的 hash 校验会读回同一份影子、自洽通过——
安装看起来成功,用户真正运行的 $app_destination 不会变(D-198)。
请在容器外的终端里装,例如:
  .\scripts\install-setup.ps1 -Setup dist\kanzei-setup-<hash>.exe -ExpectedHash <hash>(带装后校验,避免 D-266 静默无效)
"@
}

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
