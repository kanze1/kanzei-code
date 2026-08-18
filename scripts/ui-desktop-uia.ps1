# Windows 原生桌面 E2 最小验证。
#
# 这条路线不使用 WebView2 DevTools/CDP，也不把 headless browser 当作桌面 E2。
# UIA 负责真实 kzapp 顶层窗口发现/聚焦和编辑控件断言；WebView2 内容通过真实
# UIA ValuePattern 写入/读回验证，截图来自前台真实桌面窗口。
#
# 用法:
#   pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\ui-desktop-uia.ps1
#   ... -Exe C:\path\kzapp.exe -Screenshot .\artifacts\kzapp.png

[CmdletBinding()]
param(
    [string]$Exe = (Join-Path $env:LOCALAPPDATA 'kanzei\kzapp.exe'),
    [string]$Screenshot = '.kanzei\research\r302-desktop-e2\kzapp-uia.png',
    [int]$TimeoutSeconds = 20
)

$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
    throw "桌面 UIA E2 失败: $Message"
}

if (-not (Test-Path -LiteralPath $Exe)) {
    Fail "安装位不存在: $Exe"
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing

if (-not ('KzDesktopWin32' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class KzDesktopWin32 {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
}
'@
}

function Get-KzProcess {
    param([string]$Path)
    Get-Process -Name kzapp -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and ((Resolve-Path -LiteralPath $_.Path).Path -ieq (Resolve-Path -LiteralPath $Path).Path) } |
        Select-Object -First 1
}

$process = Get-KzProcess $Exe
$owned = $false
if (-not $process) {
    $process = Start-Process -FilePath $Exe -PassThru
    $owned = $true
}

$started = [DateTime]::UtcNow
$window = $null
while (-not $window) {
    Start-Sleep -Milliseconds 250
    $process.Refresh()
    if ($process.HasExited) { Fail "kzapp 在窗口出现前退出，exit=$($process.ExitCode)" }
    if ($process.MainWindowHandle -ne 0) {
        try {
            $window = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
        } catch {
            $window = $null
        }
    }
    if (([DateTime]::UtcNow - $started).TotalSeconds -gt $TimeoutSeconds) {
        Fail "等待真实 kzapp 窗口超时"
    }
}

try {
    $current = $window.Current
    if ($current.ControlType -ne [System.Windows.Automation.ControlType]::Window) {
        Fail "顶层句柄不是 Window: $($current.ControlType.ProgrammaticName)"
    }
    if ($current.Name -ne 'kanzei') {
        Fail "窗口标题不匹配: '$($current.Name)'"
    }

    $closeCondition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'Close')
    $closeButton = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $closeCondition)
    # Tauri/WebView2 的标题栏节点在不同 UIA 查询时可能不出现在同一棵树中；
    # 顶层 Window + 原生句柄才是稳定的桌面真源，Close 只作为可选诊断字段。

    $hwnd = [IntPtr]$process.MainWindowHandle
    [void][KzDesktopWin32]::ShowWindow($hwnd, 9)
    [void][KzDesktopWin32]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 500

    # UIA 找到真实 WebView2 编辑控件并通过 ValuePattern 写入/读回，证明这是生产桌面
    # UI Automation 的真实消费者；不依赖静态 HTML、CDP 或剪贴板，也不发送请求。
    $promptCondition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'prompt')
    $prompt = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $promptCondition)
    if (-not $prompt) { Fail 'UIA 未找到生产 prompt 编辑控件' }
    if ($prompt.Current.ControlType -ne [System.Windows.Automation.ControlType]::Edit) {
        Fail "prompt 控件类型不是 Edit: $($prompt.Current.ControlType.ProgrammaticName)"
    }
    $valuePattern = $prompt.GetCurrentPattern([System.Windows.Automation.ValuePatternIdentifiers]::Pattern)
    $oldValue = $valuePattern.Current.Value
    $marker = "R302_UIA_$([DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff'))"
    $prompt.SetFocus()
    $valuePattern.SetValue($marker)
    Start-Sleep -Milliseconds 150
    $roundTrip = $valuePattern.Current.Value
    if ($roundTrip -ne $marker) {
        Fail "真实桌面 UIA ValuePattern 未能回读 marker；实际='$roundTrip'"
    }
    $valuePattern.SetValue($oldValue)

    $repoRoot = Split-Path -Parent $PSScriptRoot
    $providerPrefix = "Microsoft.PowerShell.Core\FileSystem::"
    if ($repoRoot.StartsWith($providerPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $repoRoot = $repoRoot.Substring($providerPrefix.Length)
    }
    if ($repoRoot.StartsWith("\\?\")) {
        $repoRoot = $repoRoot.Substring(4)
    }
    $shotPath = if ([System.IO.Path]::IsPathRooted($Screenshot)) {
        $Screenshot
    } else {
        Join-Path $repoRoot $Screenshot
    }
    $providerIndex = $shotPath.IndexOf($providerPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    if ($providerIndex -ge 0) {
        $shotPath = $shotPath.Substring($providerIndex + $providerPrefix.Length)
    }
    if ($shotPath.StartsWith("\\?\")) {
        $shotPath = $shotPath.Substring(4)
    }
    $shotPath = [System.IO.Path]::GetFullPath($shotPath)
    $shotDir = Split-Path -Parent $shotPath
    New-Item -ItemType Directory -Force -Path $shotDir | Out-Null
    $rect = New-Object KzDesktopWin32+RECT
    if (-not [KzDesktopWin32]::GetWindowRect($hwnd, [ref]$rect)) { Fail '取真实窗口矩形失败' }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0) { Fail "真实窗口尺寸非法: ${width}x${height}" }
    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try { $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size) }
        finally { $graphics.Dispose() }
        $bitmap.Save($shotPath, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
    if (-not (Test-Path -LiteralPath $shotPath)) { Fail "截图未落盘: $shotPath" }

    $closeAutomationId = if ($closeButton) { $closeButton.Current.AutomationId } else { $null }
    $result = [ordered]@{
        executable = (Resolve-Path -LiteralPath $Exe).Path
        process_id = $process.Id
        window_title = $current.Name
        window_class = $current.ClassName
        close_automation_id = $closeAutomationId
        input_control_automation_id = $prompt.Current.AutomationId
        input_pattern = 'ValuePattern'
        input_marker_round_trip = $true
        screenshot = $shotPath
        screenshot_bytes = (Get-Item -LiteralPath $shotPath).Length
        process_owned_by_test = $owned
    }
    $result | ConvertTo-Json -Compress
} finally {
    if ($owned -and $process -and -not $process.HasExited) {
        [void]$process.CloseMainWindow()
        if (-not $process.WaitForExit(5000)) { Write-Warning '测试启动的 kzapp 未在 5 秒内退出；不强杀用户进程。' }
    }
}
