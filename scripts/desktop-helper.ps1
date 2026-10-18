# Engine-owned Windows UIA adapter. A typed JSON parameter; no user code evaluation.
param([Parameter(Mandatory=$true)][string]$RequestJson)
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class KzDesktop {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
  [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y; public Point(int x,int y){X=x;Y=y;}}
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p,IntPtr data);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder s,int n);
  [DllImport("user32.dll",EntryPoint="GetWindowLongW")] static extern int GetWindowStyle(IntPtr h,int index);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr wParam,string value,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr wParam,IntPtr value,uint flags,uint timeout,out UIntPtr result);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint flag);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr window,IntPtr after,int x,int y,int width,int height,uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent,IntPtr child);
  [StructLayout(LayoutKind.Sequential)] struct GuiThreadInfo {public uint Size,Flags;public IntPtr Active,Focus,Capture,MenuOwner,MoveSize,Caret;public Rect CaretRect;}
  [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint thread,ref GuiThreadInfo info);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
  [StructLayout(LayoutKind.Sequential)] public struct MouseInput {public int X,Y;public uint Data,Flags,Time;public UIntPtr Extra;}
  [StructLayout(LayoutKind.Sequential)] public struct KeyInput {public ushort Key,Scan;public uint Flags,Time;public UIntPtr Extra;}
  [StructLayout(LayoutKind.Explicit)] public struct InputData {[FieldOffset(0)] public MouseInput Mouse;[FieldOffset(0)] public KeyInput Key;}
  [StructLayout(LayoutKind.Sequential)] public struct Input {public uint Type;public InputData Data;}
  [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] inputs,int size);
  public static void Click() {var inputs=new Input[2];inputs[0].Data.Mouse.Flags=2;inputs[1].Data.Mouse.Flags=4;if(SendInput(2,inputs,Marshal.SizeOf(typeof(Input)))!=2)throw new InvalidOperationException("Native pointer input was not fully dispatched");}
  static Input MakeKey(ushort key,ushort scan,uint flags,UIntPtr tag) {var input=new Input();input.Type=1;input.Data.Key.Key=key;input.Data.Key.Scan=scan;input.Data.Key.Flags=flags;input.Data.Key.Extra=tag;return input;}
  static void Dispatch(Input[] inputs) {if(SendInput((uint)inputs.Length,inputs,Marshal.SizeOf(typeof(Input)))!=inputs.Length)throw new InvalidOperationException("Native keyboard input was not fully dispatched");}
  public static void TypeText(string text,UIntPtr tag) {
    var inputs=new Input[text.Length*2];
    for(int i=0;i<text.Length;i++){if(Char.IsControl(text[i]))throw new InvalidOperationException("Use a named key for control characters");inputs[i*2]=MakeKey(0,text[i],4,tag);inputs[i*2+1]=MakeKey(0,text[i],6,tag);}
    if(inputs.Length>0)Dispatch(inputs);
  }
  public static void Key(string chord,UIntPtr tag) {
    var parts=chord.Split('+');var modifiers=new List<ushort>();
    for(int i=0;i<parts.Length-1;i++){
      ushort modifier;
      switch(parts[i].ToUpperInvariant()){case "CTRL":case "CONTROL":modifier=17;break;case "ALT":modifier=18;break;case "SHIFT":modifier=16;break;default:throw new InvalidOperationException("Unsupported keyboard modifier");}
      if(modifiers.Contains(modifier))throw new InvalidOperationException("Duplicate keyboard modifier");modifiers.Add(modifier);
    }
    var keys=new Dictionary<string,ushort>(StringComparer.OrdinalIgnoreCase){{"Enter",13},{"Tab",9},{"Escape",27},{"Backspace",8},{"Delete",46},{"Left",37},{"Up",38},{"Right",39},{"Down",40},{"Home",36},{"End",35},{"Space",32},{"PageUp",33},{"PageDown",34}};
    var name=parts[parts.Length-1].ToUpperInvariant();ushort key;
    if(!keys.TryGetValue(name,out key)){if(name.Length!=1||!(name[0]>='A'&&name[0]<='Z'||name[0]>='0'&&name[0]<='9'))throw new InvalidOperationException("Unsupported named key");key=name[0];}
    var inputs=new List<Input>();foreach(var modifier in modifiers)inputs.Add(MakeKey(modifier,0,0,tag));
    inputs.Add(MakeKey(key,0,0,tag));inputs.Add(MakeKey(key,0,2,tag));
    for(int i=modifiers.Count-1;i>=0;i--)inputs.Add(MakeKey(modifiers[i],0,2,tag));Dispatch(inputs.ToArray());
  }
  public static IntPtr Focus(IntPtr window) {
    uint process;var thread=GetWindowThreadProcessId(window,out process);var info=new GuiThreadInfo();info.Size=(uint)Marshal.SizeOf(typeof(GuiThreadInfo));
    if(!GetGUIThreadInfo(thread,ref info))throw new InvalidOperationException("Selected window focus is unavailable");return info.Focus;
  }
  const uint NativeMessageTimeoutMs=1000;
  const uint NoSize=1,NoMove=2,NoActivate=16;
  public static void Raise(IntPtr window) {
    // Raising with NOACTIVATE preserves focus. The actual mouse click performs
    // the normal Windows activation, including for windows without UIA focus.
    if(!SetWindowPos(window,IntPtr.Zero,0,0,0,0,NoSize|NoMove|NoActivate))throw new InvalidOperationException("Cannot raise selected window");
  }
  public static void Activate(IntPtr window) {
    if(GetForegroundWindow()!=window&&!SetForegroundWindow(window))throw new InvalidOperationException("Cannot focus selected window");
    ConfirmForeground(window);
  }
  public static void ConfirmForeground(IntPtr window) {
    // Cross-process activation is asynchronous. WM_NULL waits for that notification.
    UIntPtr result;
    if(SendMessageTimeout(window,0,UIntPtr.Zero,IntPtr.Zero,2,NativeMessageTimeoutMs,out result)==IntPtr.Zero)throw new TimeoutException("Selected window did not process its activation");
    if(GetForegroundWindow()!=window)throw new InvalidOperationException("Selected window lost foreground ownership");
  }
  public static string Title(IntPtr h) {var s=new StringBuilder(1024);GetWindowText(h,s,s.Capacity);return s.ToString();}
  public static bool Password(IntPtr h) {var s=new StringBuilder(256);GetClassName(h,s,s.Capacity);return s.ToString().IndexOf("EDIT",StringComparison.OrdinalIgnoreCase)>=0&&(GetWindowStyle(h,-16)&0x20)!=0;}
  public static bool NativeValue(IntPtr h) {var s=new StringBuilder(256);GetClassName(h,s,s.Capacity);var c=s.ToString();return c=="Edit"||c=="ComboBox"||c=="ComboBoxEx32"||c.StartsWith("WindowsForms10.EDIT.")||c.StartsWith("WindowsForms10.COMBOBOX.");}
  public static void SetNativeValue(IntPtr h,string value) {if(!NativeValue(h)||Password(h))throw new InvalidOperationException("Not an editable non-password Win32 control");UIntPtr result;if(SendMessageTimeout(h,0x000C,UIntPtr.Zero,value,2,NativeMessageTimeoutMs,out result)==IntPtr.Zero)throw new TimeoutException("Win32 value dispatch did not complete");}
  public static IntPtr[] Windows() {var list=new List<IntPtr>();EnumWindows((h,p)=>{if(IsWindowVisible(h)&&Title(h).Length>0)list.Add(h);return true;},IntPtr.Zero);return list.ToArray();}
}

'@
if ([KzDesktop]::SetThreadDpiAwarenessContext([IntPtr]::new(-4)) -eq [IntPtr]::Zero) { throw 'Physical coordinate context is unavailable' }

function Window-Identity([IntPtr]$handle) {
    if (-not [KzDesktop]::IsWindow($handle) -or -not [KzDesktop]::IsWindowVisible($handle)) { throw 'Window is closed or not visible' }
    [uint32]$processId = 0
    [void][KzDesktop]::GetWindowThreadProcessId($handle, [ref]$processId)
    $process = [System.Diagnostics.Process]::GetProcessById($processId)
    $image = $process.MainModule.FileName
    if ([IO.Path]::GetFileName($image) -match '^(kzapp|codex|chatgpt)\.exe$') { throw 'Agent and approval windows cannot be controlled' }
    return @{ hwnd=$handle.ToInt64().ToString(); pid=$processId; started=$process.StartTime.ToUniversalTime().Ticks.ToString(); image=$image; title=[KzDesktop]::Title($handle) }
}
function Same-Window($expected) {
    $handle = [IntPtr][long]$expected.hwnd
    $actual = Window-Identity $handle
    if ($actual.pid -ne $expected.pid -or $actual.started -ne $expected.started -or $actual.image -ne $expected.image) { throw 'Window process identity changed' }
    return $handle
}
function Runtime-Key($element) { return (($element.GetRuntimeId() | ForEach-Object { $_.ToString() }) -join '.') }
function Descendants($root) {
    # UIA properties come from the selected process. Do not descend into the desktop.
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $queue = New-Object 'System.Collections.Generic.Queue[System.Windows.Automation.AutomationElement]'
    $queue.Enqueue($root)
    $found = New-Object 'System.Collections.Generic.List[System.Windows.Automation.AutomationElement]'
    while ($queue.Count -gt 0 -and $found.Count -lt 400) {
        $current = $queue.Dequeue(); $found.Add($current)
        $child = $walker.GetFirstChild($current)
        while ($null -ne $child -and $queue.Count + $found.Count -lt 400) { $queue.Enqueue($child); $child = $walker.GetNextSibling($child) }
    }
    return ,$found
}
function Pattern($element, $kind) {
    $value = $null
    if ($element.TryGetCurrentPattern($kind,[ref]$value)) { return $value }
    return $null
}
function Active-Root([IntPtr]$handle, $expected) {
    # A modal dialog is allowed only when its owner chain leads to the selected window.
    foreach ($candidate in [KzDesktop]::Windows()) {
        if ($candidate -eq $handle) { continue }
        $owner = [KzDesktop]::GetWindow($candidate,4)
        for ($depth=0; $owner -ne [IntPtr]::Zero -and $depth -lt 16; $depth++) {
            if ($owner -eq $handle) {
                [uint32]$childProcess=0; [void][KzDesktop]::GetWindowThreadProcessId($candidate,[ref]$childProcess)
                if ($childProcess -eq $expected.pid) { return [System.Windows.Automation.AutomationElement]::FromHandle($candidate) }
                break
            }
            $owner = [KzDesktop]::GetWindow($owner,4)
        }
    }
    return [System.Windows.Automation.AutomationElement]::FromHandle($handle)
}
function Click-Element($element,$root,[double]$offsetX,[double]$offsetY) {
    $bounds=$element.Current.BoundingRectangle
    if ($offsetX -lt 0 -or $offsetY -lt 0 -or $offsetX -ge $bounds.Width -or $offsetY -ge $bounds.Height) { throw 'Pointer is outside the observed element bounds' }
    $x=$bounds.X+$offsetX; $y=$bounds.Y+$offsetY
    Click-Point $element $root $x $y
}
function Click-Point($element,$root,[double]$x,[double]$y) {
    $activeHandle=[IntPtr]$root.Current.NativeWindowHandle
    $geometry=Surface-Signature $root
    [KzDesktop]::Raise($activeHandle)
    if((Surface-Signature $root) -ne $geometry){throw 'Window geometry changed during activation; inspect again'}
    $hit=[KzDesktop]::WindowFromPoint([KzDesktop+Point]::new([int]$x,[int]$y))
    if ($hit -ne $activeHandle -and -not [KzDesktop]::IsChild($activeHandle,$hit)) { throw 'Another window covers the pointer target' }
    $nativeElement=[IntPtr]$element.Current.NativeWindowHandle
    if ($nativeElement -ne [IntPtr]::Zero) {
        if ($hit -ne $nativeElement -and -not [KzDesktop]::IsChild($nativeElement,$hit)) { throw 'Observed control is covered by another control' }
    } else {
        $hitElement=[System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($x,$y))
        $matched=$false;$expected=Runtime-Key $element
        for($depth=0;$null -ne $hitElement -and $depth -lt 64;$depth++) {if((Runtime-Key $hitElement)-eq$expected){$matched=$true;break};$hitElement=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($hitElement)}
        if(-not $matched){throw 'Observed virtual control no longer owns the pointer location'}
    }
    if (-not [KzDesktop]::SetCursorPos([int]$x,[int]$y)) { throw 'Pointer position could not be set' }
    [KzDesktop]::Click()
}
function Set-ElementValue($element,[string]$text) {
    $pattern=Pattern $element ([System.Windows.Automation.ValuePattern]::Pattern)
    if ($null -ne $pattern) {
        if ($pattern.Current.IsReadOnly) { throw 'ValuePattern is read-only' }
        $pattern.SetValue($text)
    } else {
        [KzDesktop]::SetNativeValue([IntPtr]$element.Current.NativeWindowHandle,$text)
    }
}
function Hash-Identity([string]$identity) {
    $hash=[Security.Cryptography.SHA256]::Create()
    try { return [Convert]::ToHexString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity))) } finally { $hash.Dispose() }
}
function Element-Signature($element) {
    $current=$element.Current
    $password=$current.IsPassword -or [KzDesktop]::Password([IntPtr]$current.NativeWindowHandle)
    $value=$null
    if (-not $password) {
        $pattern=Pattern $element ([System.Windows.Automation.ValuePattern]::Pattern)
        if ($null -ne $pattern) { $value=$pattern.Current.Value }
        elseif ([KzDesktop]::NativeValue([IntPtr]$current.NativeWindowHandle)) { $value=[KzDesktop]::Title([IntPtr]$current.NativeWindowHandle) }
    }
    $name=if($password){'[password]'}else{$current.Name}
    $identity=@($current.AutomationId,$current.ControlType.ProgrammaticName,$current.IsEnabled,$password,$name,$value,$current.BoundingRectangle.ToString()) | ConvertTo-Json -Compress
    return Hash-Identity $identity
}
function Window-Bounds([IntPtr]$handle) {
    $rect=New-Object KzDesktop+Rect
    if (-not [KzDesktop]::GetWindowRect($handle,[ref]$rect)) { throw 'Cannot read target window bounds' }
    return @{x=$rect.Left;y=$rect.Top;width=$rect.Right-$rect.Left;height=$rect.Bottom-$rect.Top}
}
function Surface-Signature($root) {
    # A window title is presentation content, not the identity of a screenshot surface.
    $handle=[IntPtr]$root.Current.NativeWindowHandle
    $bounds=Window-Bounds $handle
    $identity=@((Runtime-Key $root),$handle.ToInt64(),$bounds.x,$bounds.y,$bounds.width,$bounds.height) | ConvertTo-Json -Compress
    return Hash-Identity $identity
}
function Confirm-SurfaceFocus($root,$expected) {
    $handle=[IntPtr]$root.Current.NativeWindowHandle
    $focus=[KzDesktop]::Focus($handle)
    if($focus -eq [IntPtr]::Zero -or $focus.ToInt64().ToString() -ne $expected -or ($focus -ne $handle -and -not [KzDesktop]::IsChild($handle,$focus))){throw 'Observed input focus changed or left the selected surface; inspect again'}
    $native=[System.Windows.Automation.AutomationElement]::FromHandle($focus)
    $focused=[System.Windows.Automation.AutomationElement]::FocusedElement
    if([KzDesktop]::Password($focus) -or $native.Current.IsPassword -or ($null -ne $focused -and $focused.Current.IsPassword)){throw 'Password controls require manual user input'}
}
function Clip-Field([string]$text,[int]$limit) { if($text.Length -gt $limit){return $text.Substring(0,$limit)};return $text }
function Snapshot($root) {
    $elements = Descendants $root
    $rows = @()
    foreach ($element in $elements) {
        $current=$element.Current; $rect=$current.BoundingRectangle
        $value = $null
        $password=$current.IsPassword -or [KzDesktop]::Password([IntPtr]$current.NativeWindowHandle)
        $editable=$false
        if (-not $password) {
            $pattern=Pattern $element ([System.Windows.Automation.ValuePattern]::Pattern)
            if ($null -ne $pattern) { $value=$pattern.Current.Value; $editable=-not $pattern.Current.IsReadOnly }
            elseif ([KzDesktop]::NativeValue([IntPtr]$current.NativeWindowHandle)) { $value=[KzDesktop]::Title([IntPtr]$current.NativeWindowHandle); $editable=$true }
        }
        $label=if($password){'[password]'}else{$current.Name}
        $rows += @{runtime_id=(Runtime-Key $element);signature=(Element-Signature $element);name=(Clip-Field $label 1024);automation_id=(Clip-Field $current.AutomationId 1024);type=$current.ControlType.ProgrammaticName;enabled=$current.IsEnabled;password=$password;editable=$editable;value=if($null -eq $value){$null}else{Clip-Field $value 4096};bounds=@{x=$rect.X;y=$rect.Y;width=$rect.Width;height=$rect.Height}}
    }
    $bounds=$root.Current.BoundingRectangle
    return @{ elements=$rows; limit=400; truncated=($elements.Count -eq 400); root=(Runtime-Key $root); hwnd=$root.Current.NativeWindowHandle.ToString();bounds=@{x=$bounds.X;y=$bounds.Y;width=$bounds.Width;height=$bounds.Height} }
}
function Screenshot([IntPtr]$handle) {
    $bounds=Window-Bounds $handle
    $width=$bounds.width; $height=$bounds.height
    if ($width -lt 1 -or $height -lt 1 -or [long]$width*$height -gt 16000000) { throw 'Window dimensions outside 16 megapixel capture budget' }
    $bitmap=New-Object Drawing.Bitmap($width,$height)
    try {
        $graphics=[Drawing.Graphics]::FromImage($bitmap)
        try { $dc=$graphics.GetHdc(); try { if (-not [KzDesktop]::PrintWindow($handle,$dc,2)) { throw 'Window provider does not support scoped capture' } } finally { $graphics.ReleaseHdc($dc) } } finally { $graphics.Dispose() }
        $scale=[Math]::Min(1.0,[Math]::Min(1600.0/$width,1000.0/$height))
        $resized=New-Object Drawing.Bitmap($bitmap,[Drawing.Size]::new([Math]::Max(1,[int]($width*$scale)),[Math]::Max(1,[int]($height*$scale))))
        try { $stream=New-Object IO.MemoryStream; try { $resized.Save($stream,[Drawing.Imaging.ImageFormat]::Png); $bytes=$stream.ToArray(); if ($bytes.Length -gt 2097152) { throw 'Window PNG exceeds 2 MiB' }; return @{data=[Convert]::ToBase64String($bytes);width=$resized.Width;height=$resized.Height;bounds=$bounds;media_type='image/png'} } finally { $stream.Dispose() } } finally { $resized.Dispose() }
    } finally { $bitmap.Dispose() }
}

try {
    $request = $RequestJson | ConvertFrom-Json
    if ($request.action -eq 'list') {
        $windows=@()
        $unavailable=@()
        foreach ($handle in [KzDesktop]::Windows()) {
            try { $windows += Window-Identity $handle } catch { $unavailable += @{hwnd=$handle.ToInt64().ToString();reason=$_.Exception.Message} }
        }
        @{ok=$true;windows=$windows;unavailable=$unavailable} | ConvertTo-Json -Depth 12 -Compress
        exit 0
    }
    $handle=Same-Window $request.window
    $root=Active-Root $handle $request.window
    $elements=Descendants $root
    if ($request.action -notin @('inspect','screenshot')) {
        if ($root.Current.ClassName -eq '#32770' -and $request.action -ne 'choose_file') { throw 'Native file/dialog controls require a dedicated file choice; generic input is disabled' }
        if($request.target -eq 'surface') {
            if($request.action -notin @('click','type','key')){throw 'Screenshot surfaces support click, type and key; use a UIA element for control actions'}
            if((Surface-Signature $root) -ne $request.signature){throw 'Observed window surface moved, resized or changed; inspect again'}
            $element=$root
            if($request.action -eq 'click') {
                $bounds=Window-Bounds ([IntPtr]$root.Current.NativeWindowHandle)
                if($request.x -lt 0 -or $request.y -lt 0 -or $request.x -ge $bounds.width -or $request.y -ge $bounds.height){throw 'Pointer is outside the observed screenshot bounds'}
                Click-Point $root $root ($bounds.x+$request.x) ($bounds.y+$request.y)
            } else {
                [KzDesktop]::ConfirmForeground([IntPtr]$root.Current.NativeWindowHandle)
                Confirm-SurfaceFocus $root ([string]$request.focus)
                if($request.action -eq 'type') {
                    if($null -eq $request.text -or $request.text.Length -gt 4096){throw 'Literal text is required and limited to 4096 characters'}
                    [KzDesktop]::TypeText([string]$request.text,[UIntPtr]::new([uint64]$request.input_tag))
                } else { [KzDesktop]::Key([string]$request.key,[UIntPtr]::new([uint64]$request.input_tag)) }
            }
        } else {
        $element=$elements | Where-Object { (Runtime-Key $_) -eq $request.runtime_id } | Select-Object -First 1
        if ($null -eq $element -or -not $element.Current.IsEnabled) { throw 'Element is stale, outside the selected window, or disabled; inspect again' }
        if ($element.Current.IsPassword -or [KzDesktop]::Password([IntPtr]$element.Current.NativeWindowHandle)) { throw 'Password controls require manual user input' }
        if ($request.signature -and (Element-Signature $element) -ne $request.signature) { throw 'Observed control content or identity changed; inspect again' }
        switch ($request.action) {
            'choose_file' {
                if ($root.Current.ClassName -ne '#32770') { throw 'No owned native file dialog is active' }
                if ($element.Current.AutomationId -ne '1148') { throw 'Dedicated choice requires the observed standard file-name control' }
                Set-ElementValue $element ([string]$request.file)
                $confirm=$elements | Where-Object { $_.Current.AutomationId -eq '1' } | Select-Object -First 1
                if ($null -eq $confirm) { throw 'Native dialog has no standard confirmation control' }
                Click-Element $confirm $root ($confirm.Current.BoundingRectangle.Width/2) ($confirm.Current.BoundingRectangle.Height/2)
                $invocation='dispatched'
            }
            'fill' { Set-ElementValue $element ([string]$request.text) }
            'invoke' { Click-Element $element $root ($element.Current.BoundingRectangle.Width/2) ($element.Current.BoundingRectangle.Height/2); $invocation='dispatched' }
            'toggle' { $pattern=Pattern $element ([System.Windows.Automation.TogglePattern]::Pattern); if ($null -eq $pattern) { throw 'Element does not support TogglePattern' }; $pattern.Toggle() }
            'select' { $pattern=Pattern $element ([System.Windows.Automation.SelectionItemPattern]::Pattern); if ($null -eq $pattern) { throw 'Element does not support SelectionItemPattern' }; $pattern.Select() }
            'click' { Click-Element $element $root ([double]$request.x) ([double]$request.y) }
            'key' {
                $activeHandle=[IntPtr]$root.Current.NativeWindowHandle
                $element.SetFocus()
                [KzDesktop]::Activate($activeHandle)
                [KzDesktop]::Key([string]$request.key,[UIntPtr]::new([uint64]$request.input_tag))
            }
            default { throw 'Unknown desktop action' }
        }
        }
    }
    if ($request.action -notin @('inspect','screenshot')) {
        # Native input dispatch does not prove a form submitted or a dialog closed.
        # A new inspect independently verifies application state.
        @{ok=$true;action=$request.action;invocation=$invocation;status='dispatched';verification='inspect_required'} | ConvertTo-Json -Depth 8 -Compress
        [Environment]::Exit(0)
    }
    [void](Same-Window $request.window)
    $root=Active-Root $handle $request.window
    $snapshot=Snapshot $root
    $surfaceSignature=Surface-Signature $root
    $screenshot=Screenshot ([IntPtr]$root.Current.NativeWindowHandle)
    if((Surface-Signature $root) -ne $surfaceSignature){throw 'Window geometry changed during capture; inspect again'}
    $snapshot.surface=@{runtime_id=(Runtime-Key $root);signature=$surfaceSignature;bounds=$screenshot.bounds;width=$screenshot.width;height=$screenshot.height;focus=[KzDesktop]::Focus([IntPtr]$root.Current.NativeWindowHandle).ToInt64().ToString()}
    @{ok=$true;action=$request.action;invocation=$invocation;window=$request.window;snapshot=$snapshot;image=$screenshot} | ConvertTo-Json -Depth 14 -Compress
} catch { @{ok=$false;error=$_.Exception.Message} | ConvertTo-Json -Compress; exit 1 }
