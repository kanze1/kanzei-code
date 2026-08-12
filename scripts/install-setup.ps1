# kanzei 安装器静默安装 + 装后校验(D-266)
# 用法: .\scripts\install-setup.ps1 -Setup dist\kanzei-setup-<hash>.exe [-ExpectedHash <hash>]
# 背景:setup.exe /S 在 kzapp 运行时「退出码 0、文件没换、无提示」——NSIS 模板在目标程序
# 运行中无法处理占用,静默模式无人可问,直接放弃却仍返回成功。所以这里**不信退出码,信产物**:
# 装完必须证明安装位 kzapp.exe 真的变了、且(给了 hash 时)二进制内确实含本次构建标识。
param(
    [Parameter(Mandatory = $true)][string]$Setup,
    [string]$ExpectedHash = "",
    [string]$ProcessName = "kzapp"
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$app_dir = "$env:LOCALAPPDATA\kanzei"
$exe = "$app_dir\kzapp.exe"

if (-not (Test-Path $Setup)) {
    throw "安装器不存在:$Setup"
}

# ① 前置检测:kzapp 运行中静默安装必然无效,当场拒绝并说明原因,不给"装好了"的假象。
$running = @(Get-Process $ProcessName -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    $pids = ($running | ForEach-Object { $_.Id }) -join ","
    throw "kzapp 正在运行(pid $pids),静默安装无法替换正在使用的 exe。请先关闭 kzapp 再重跑本脚本,或改用交互式安装(不带 /S)"
}

# ② 记录安装前状态(mtime/大小;不存在则为空)。
$before = $null
if (Test-Path $exe) {
    $f = Get-Item $exe
    $before = @{ LastWriteTime = $f.LastWriteTime; Length = $f.Length }
}

# ③ 执行静默安装。退出码在这里只是快速失败信号,不是成功判据——D-266 已实测 0 也会没装上。
Write-Host "==> 静默安装 $Setup" -ForegroundColor Cyan
$proc = Start-Process -FilePath $Setup -ArgumentList "/S" -Wait -PassThru
if ($proc.ExitCode -ne 0) {
    throw "安装器退出码 $($proc.ExitCode)(非 0)"
}

# ④ 装后校验一:安装位必须真实存在且内容发生了变化。
if (-not (Test-Path $exe)) {
    throw "装后校验失败:$exe 不存在——安装器退出码 0 但并未装上 kzapp。请关闭 kzapp 后重装"
}
$after = Get-Item $exe
if ($null -ne $before -and $after.LastWriteTime -eq $before.LastWriteTime -and $after.Length -eq $before.Length) {
    throw "装后校验失败:$exe 与安装前完全相同(mtime/大小均未变)——kzapp 可能仍在运行,安装器静默放弃了。请关闭 kzapp 后重跑本脚本"
}
Write-Host "==> 安装位已更新:$exe ($($after.Length) bytes, $($after.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')))" -ForegroundColor Green

# ⑤ 装后校验二(给了 ExpectedHash 时):二进制内必须含本次构建标识,证明装上的就是本次版本。
if ($ExpectedHash) {
    $bytes = [System.IO.File]::ReadAllBytes($exe)
    $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
    if (-not $ascii.Contains($ExpectedHash)) {
        throw "装后校验失败:安装位 kzapp.exe 内未找到本次构建标识 '$ExpectedHash'——装上的不是本次版本。请关闭 kzapp 后重装"
    }
    Write-Host "==> 校验通过:安装位含本次构建标识 $ExpectedHash" -ForegroundColor Green
}

Write-Host "==> 安装完成。重启 kzapp 即运行新版。" -ForegroundColor Green
