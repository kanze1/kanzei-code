# kanzei 安装包构建:cargo tauri build → NSIS setup.exe → dist/
# 用法: .\scripts\package.ps1 [-Publish]   (-Publish = 同时发到 GitHub Releases,应用内"检查更新"即以此为源)
param([switch]$Publish)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
if (-not $env:HTTPS_PROXY) { $env:HTTPS_PROXY = "http://127.0.0.1:12000" }

$hash = (git -C $root rev-parse --short HEAD).Trim()
$date = Get-Date -Format "yyyy-MM-dd"
$build_at = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss")
$env:KANZEI_BUILD_INFO = "$hash $build_at"

# kz CLI 随安装包一起发(D-175)。桌面端与 CLI 共用同一个 .kanzei/state.db,
# 而 schema 迁移是单向的:只发 kzapp 的话,一次 schema 变更就会让机器上的旧 kz
# 直接打不开库。两个二进制必须同版本出厂,由 kzapp 启动时同步到 ~\.cargo\bin。
Write-Host "==> cargo build --release -p kanzei (sidecar kz)" -ForegroundColor Cyan
cargo build --release -p kanzei --manifest-path "$root\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "kz build failed" }
$triple = (rustc -vV | Select-String '^host:').Line.Split(' ')[1].Trim()
$sidecar_dir = "$root\crates\kanzei-app\binaries"
New-Item -ItemType Directory -Force $sidecar_dir | Out-Null
Copy-Item "$root\target\release\kz.exe" "$sidecar_dir\kz-$triple.exe" -Force

# externalBin 只在打包时注入,不写进 tauri.conf.json:tauri-build 在 **build script**
# 阶段就校验 sidecar 存在,写死进配置会让每一次普通 cargo build / cargo test 都失败。
$bundle_config = Join-Path $env:TEMP "kanzei-bundle-config.json"
Set-Content $bundle_config '{"bundle":{"externalBin":["binaries/kz"]}}' -Encoding UTF8

Write-Host "==> cargo tauri build ($hash)" -ForegroundColor Cyan
Push-Location "$root\crates\kanzei-app"
try { cargo tauri build --config $bundle_config 2>&1 | ForEach-Object { $_ } } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

$setup = Get-ChildItem "$root\target\release\bundle\nsis\*-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $setup) { throw "installer not found under target\release\bundle\nsis" }
New-Item -ItemType Directory -Force "$root\dist" | Out-Null
$out = "$root\dist\kanzei-setup-$hash.exe"
Copy-Item $setup.FullName $out -Force
Write-Host "==> installer: $out ($([math]::Round((Get-Item $out).Length/1MB)) MB)" -ForegroundColor Green

if ($Publish) {
    $tag = "build-$hash"
    Write-Host "==> publishing GitHub release $tag" -ForegroundColor Cyan
    # 变更日志:自上一个 release 标签以来的提交(引流物料,顺手生成)。
    $lastTag = (git -C $root tag --list "build-*" --sort=-creatordate | Select-Object -First 1)
    $log = if ($lastTag) { git -C $root log "$lastTag..HEAD" --format="- %s" } else { git -C $root log -10 --format="- %s" }
    $notes = "## 变更`n$($log -join "`n")`n`n---`n构建 $hash($date)。应用内「检查更新」以此为源。"
    $notesFile = Join-Path $env:TEMP "kanzei-release-notes.md"
    Set-Content $notesFile $notes -Encoding UTF8
    gh release create $tag $out --repo kanze1/kanzei-code --title "kanzei $date ($hash)" --notes-file $notesFile 2>&1 | ForEach-Object { $_ }
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
    Write-Host "==> published: https://github.com/kanze1/kanzei-code/releases/tag/$tag" -ForegroundColor Green
}
