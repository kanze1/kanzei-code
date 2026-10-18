# Independent OS input, restricted to the owned fixture and its control UI.
param([Parameter(Mandatory=$true)][string]$WindowFile,[Parameter(Mandatory=$true)][string]$RequestJson)
$ErrorActionPreference='Stop'
$window=Get-Content -LiteralPath $WindowFile -Raw -Encoding UTF8 | ConvertFrom-Json
$request=$RequestJson|ConvertFrom-Json
$formProcess=Get-Process -Id $window.pid
if($formProcess.StartTime.ToUniversalTime().Ticks.ToString() -ne $window.started){throw 'Owned form process identity changed'}
if($request.action -eq 'release-block'){
  $event=[Threading.EventWaitHandle]::OpenExisting($window.block_event)
  try{[void]$event.Set()}finally{$event.Dispose()}
  exit 0
}
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class AcceptanceInput {
 public delegate bool EnumProc(IntPtr window,IntPtr data);
 [StructLayout(LayoutKind.Sequential)] struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr window,StringBuilder text,int count);
 [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public static void Escape(){keybd_event(27,0,0,UIntPtr.Zero);keybd_event(27,0,2,UIntPtr.Zero);}
 public static void ClickControl(uint process,string title,double x,double y){
  if(Double.IsNaN(x)||Double.IsNaN(y)||x<=0||x>=1||y<=0||y>=1)throw new Exception("Control click must be inside the observed viewport");
  if(SetThreadDpiAwarenessContext(new IntPtr(-4))==IntPtr.Zero)throw new Exception("Physical coordinate context unavailable");
  IntPtr target=IntPtr.Zero;
  EnumWindows((window,data)=>{uint owner;GetWindowThreadProcessId(window,out owner);if(owner==process){var text=new StringBuilder(1024);GetWindowTextW(window,text,text.Capacity);if(text.ToString()==title&&IsWindowVisible(window))target=window;}return true;},IntPtr.Zero);
  if(target==IntPtr.Zero)throw new Exception("Owned control window is not visible");
  Rect rect;if(!GetClientRect(target,out rect))throw new Exception("Control viewport unavailable");
  var point=new Point{x=(int)Math.Round(x*(rect.right-rect.left)),y=(int)Math.Round(y*(rect.bottom-rect.top))};
  if(!ClientToScreen(target,ref point)||!SetCursorPos(point.x,point.y))throw new Exception("Cannot locate the owned control button");
  mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);
 }
}
'@
$foreground=[AcceptanceInput]::GetForegroundWindow().ToInt64().ToString()
if($request.action -eq 'foreground'){@{owned_form_foreground=($foreground -eq $window.hwnd)}|ConvertTo-Json -Compress;exit 0}
if($foreground -ne $window.hwnd){throw 'Owned form must hold foreground before the independent OS input'}
switch($request.action){
 'escape'{[AcceptanceInput]::Escape()}
 'click-control'{
  $agentProcess=Get-Process -Id ([int]$request.agent_pid)
  $suiteRoot=(Split-Path -Parent (Split-Path -Parent $WindowFile))+[IO.Path]::DirectorySeparatorChar
  if(-not $agentProcess.Path.StartsWith($suiteRoot,[StringComparison]::OrdinalIgnoreCase)){throw 'Control process is outside the owned acceptance suite'}
  [AcceptanceInput]::ClickControl([uint32]$request.agent_pid,[string]$request.title,[double]$request.x,[double]$request.y)
 }
 default{throw 'Unknown independent input action'}
}
