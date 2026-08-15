# kanzei 安装包构建:cargo tauri build → NSIS setup.exe → dist/
# 该脚本必须以 UTF-8 BOM 保存,以兼容 Windows PowerShell 5.1 对中文字符串的解析。
# 用法: .\scripts\package.ps1 -Ack <本次要发的提交数> [-Publish]
#       -Publish = 同时发到 GitHub Releases,应用内"检查更新"即以此为源
#       -Ack     = 你认为自上个 build-* 标签以来应当发出去的提交条数。
#                  实际条数不符就中止(D-183)。
param([switch]$Publish, [int]$Ack = -1, [string]$VerificationPath)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
if (-not $env:HTTPS_PROXY) { $env:HTTPS_PROXY = "http://127.0.0.1:12000" }

# 步进标注:发版全程 6~8 步,每步一行 [N/M],配合活动面板的实时输出流,
# 长静默段(tauri build 数分钟)之前先说清"现在在哪一步、一共几步"。
$script:stepTotal = if ($Publish) { 8 } else { 6 }
$script:stepIndex = 0
function Step([string]$label) {
    $script:stepIndex += 1
    Write-Host "[$script:stepIndex/$script:stepTotal] $label" -ForegroundColor Cyan
}

$hash = (git -C $root rev-parse --short HEAD).Trim()
# 全 SHA 单独留一份:GitHub 的 target_commitish 只认 40 位 SHA 或分支名,
# 短 hash 会被 422 Validation Failed 挡回来(实测 build-84f843e)。
$full_hash = (git -C $root rev-parse HEAD).Trim()
$date = Get-Date -Format "yyyy-MM-dd"
$build_at = (Get-Date).ToUniversalTime().ToString("yyyyMMddHHmmss")
$env:KANZEI_BUILD_INFO = "$hash $build_at"

# ---- 发布范围核对(D-183)----
# 本仓库会有并发自举运行提交到同一分支,作者与人手动提交完全一样,git 元数据
# 分辨不出来。实测 build-ea6d058/96acfdf 就夹带了两个我没审过的提交而无人察觉。
# 这里不做猜测,只做一件事:把区间摊开,并要求发布者用 -Ack 明确说出条数——
# 数目对不上就中止。多出来一个提交就是一次强制停顿。
$lastTag = (git -C $root tag --list "build-*" --sort=-creatordate | Select-Object -First 1)
$range = if ($lastTag) { "$lastTag..HEAD" } else { "HEAD~10..HEAD" }
# git log 输出 UTF-8 提交信息;PowerShell 默认按系统代码页捕获,会把含多字节
# 的行尾吞掉、相邻行合并(实测 6 个提交被判成 5,发布被误拦)。先切到 UTF-8
# 捕获,行边界才精确——提交数判据错了,D-183 门禁就形同虚设。
$prevEncoding = [Console]::OutputEncoding
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$commits = @(git -C $root log $range --format="%h|%an|%s")
[Console]::OutputEncoding = $prevEncoding
Step "发布范围核对:$range —— $($commits.Count) 个提交"
foreach ($line in $commits) {
    $parts = $line -split '\|', 3
    # 标出是否触碰源码:只动 .kanzei/ 文档的提交不影响二进制,但仍然会被发出去。
    $touched = @(git -C $root show --stat --format="" --name-only $parts[0]) | Where-Object { $_ }
    $code = @($touched | Where-Object { $_ -like "crates/*" -or $_ -like "scripts/*" -or $_ -like "ui/*" }).Count
    $mark = if ($code -gt 0) { "[源码]" } else { "[文档]" }
    Write-Host "    $mark $($parts[0]) $($parts[2])"
}
if ($Ack -lt 0) {
    throw "发布范围未确认:核对上面 $($commits.Count) 个提交,确认都属于本次交付后,加 -Ack $($commits.Count) 重跑"
}
if ($Ack -ne $commits.Count) {
    throw "发布范围不符:你确认的是 $Ack 个提交,实际区间里有 $($commits.Count) 个。逐条核对上面的清单——多出来的很可能是并发运行提交的,不该跟着这次发出去"
}

# D-232:GitHub release 的 --target 必须是远端已知的完整 SHA。未 push 时若等到
# tauri/NSIS 都完成再由 gh 返回 422，会白白浪费整次构建；这里在构建前给出可执行出路。
if ($Publish) {
    $branch = (git -C $root branch --show-current).Trim()
    if ([string]::IsNullOrWhiteSpace($branch)) {
        throw "当前处于 detached HEAD，无法确认发布提交是否已推到远端；请切换到已跟踪分支后再发版"
    }
    $remoteRef = "origin/$branch"
    git -C $root rev-parse --verify --quiet "refs/remotes/$remoteRef" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "远端分支 $remoteRef 不存在或尚未 fetch；先 git push -u origin $branch 再发版"
    }
    $unpublished = @(git -C $root rev-list "$remoteRef..HEAD") | Where-Object { $_ }
    Step "远端可达检查:$remoteRef"
    if ($unpublished.Count -gt 0) {
        throw "HEAD 有 $($unpublished.Count) 个提交尚未推送到 $remoteRef；先执行 git push origin $branch，再重新运行 package.ps1 -Publish。GitHub 无法为未推送 SHA 创建 release target"
    }
}

Step "发布树干净检查"
# 源码工作区必须干净:否则构建出来的二进制与 $hash 标签不对应,发布物无从追溯。
# 判据用 `git diff --name-only HEAD` 而不是 `status --porcelain`:后者会把纯行尾
# 差异(CRLF/LF)也报成 M,而那类文件内容与 HEAD 完全一致,拦下来纯属误伤。
# 另单独查未跟踪的源码文件——新加的 .rs 忘了 git add 同样会让产物对不上标签。
$dirty = @(git -C $root diff --name-only HEAD -- crates scripts ui) | Where-Object { $_ }
$untracked = @(git -C $root ls-files --others --exclude-standard -- crates scripts ui) | Where-Object { $_ }
$unclean = @($dirty) + @($untracked)
if ($unclean.Count -gt 0) {
    throw "发布树源码未提交($($unclean.Count) 处),构建产物将与标签 $hash 不一致:`n$($unclean -join "`n")"
}

# ---- 验证证据门禁(R-152/A-009):无绑定 HEAD 的全绿证据不打包 ----
Step "验证证据门禁"
$verification = if ($VerificationPath) { $VerificationPath } else { Join-Path $root "dist\verification.json" }
if (-not (Test-Path $verification)) {
    throw "缺验证证据 ${verification}:先跑 scripts\verify.ps1。发布树打包时可用 -VerificationPath 指向 dev 树产出的证据(ff 合并后两树 HEAD 相同)"
}
$evidence = Get-Content $verification -Raw | ConvertFrom-Json
if ($evidence.commit -ne $full_hash) {
    throw "验证证据绑定 $($evidence.commit),HEAD 是 ${full_hash}:commit 变了就要重新 verify——这正是本门禁存在的原因"
}
if (-not $evidence.all_pass) { throw "验证证据未全绿,不得打包" }
Write-Host "==> 验证证据核对通过($($evidence.verified_at_utc))" -ForegroundColor Green

# kz CLI 随安装包一起发(D-175)。桌面端与 CLI 共用同一个 .kanzei/state.db,
# 而 schema 迁移是单向的:只发 kzapp 的话,一次 schema 变更就会让机器上的旧 kz
# 直接打不开库。两个二进制必须同版本出厂,由 kzapp 启动时同步到 ~\.cargo\bin。
Step "cargo build --release -p kanzei(sidecar kz)"
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

Step "cargo tauri build($hash)—— 最长的一步,输出会持续滚动"
Push-Location "$root\crates\kanzei-app"
try { cargo tauri build --config $bundle_config 2>&1 | ForEach-Object { $_ } } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

Step "收集安装包产物"
$setup = Get-ChildItem "$root\target\release\bundle\nsis\*-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $setup) { throw "installer not found under target\release\bundle\nsis" }
New-Item -ItemType Directory -Force "$root\dist" | Out-Null
$out = "$root\dist\kanzei-setup-$hash.exe"
Copy-Item $setup.FullName $out -Force
Write-Host "==> installer: $out ($([math]::Round((Get-Item $out).Length/1MB)) MB)" -ForegroundColor Green

if ($Publish) {
    $tag = "build-$hash"
    Step "发布 GitHub release $tag"
    # 变更日志复用上面已核对过的同一个区间,不再另算一遍——两处口径必须一致,
    # 否则 release notes 说的和实际发出去的可能不是一回事。同样在 UTF-8 捕获下
    # 取,否则中文提交信息吞行会让 notes 与清单差一行。
    $prevEncoding = [Console]::OutputEncoding
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    $log = git -C $root log $range --format="- %s"
    [Console]::OutputEncoding = $prevEncoding
    $notes = "## 变更`n$($log -join "`n")`n`n---`n构建 $hash($date),范围 $range 共 $($commits.Count) 个提交(已核对)。应用内「检查更新」以此为源。"
    $notesFile = Join-Path $env:TEMP "kanzei-release-notes.md"
    Set-Content $notesFile $notes -Encoding UTF8
    # --target 是必需的,不是可选优化:tag 不存在时 gh 会在**远端默认分支(main)**的
    # HEAD 上建它,而我们从 dev 发版。实测 build-ecdab96 因此指向了 5dcf469——发布 tag
    # 对不上真正构建的提交,更要命的是上面那段 D-183 区间判据从此一直从 main 起算,
    # 每次发版都把同一批 dev 提交重新数一遍,守卫的精度就没了。
    gh release create $tag $out --repo kanze1/kanzei-code --target $full_hash --title "kanzei $date ($hash)" --notes-file $notesFile 2>&1 | ForEach-Object { $_ }
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
    Write-Host "==> published: https://github.com/kanze1/kanzei-code/releases/tag/$tag" -ForegroundColor Green
}
