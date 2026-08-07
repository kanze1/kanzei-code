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

Write-Host "==> cargo tauri build ($hash)" -ForegroundColor Cyan
Push-Location "$root\crates\kanzei-app"
try { cargo tauri build 2>&1 | ForEach-Object { $_ } } finally { Pop-Location }
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
