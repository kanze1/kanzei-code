param([Parameter(Mandatory=$true)][string]$StateDirectory)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System.Windows.Forms;
public class DesktopCanvas : Panel {
  [System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr window,int command);
  public DesktopCanvas() { SetStyle(ControlStyles.Selectable,true); TabStop=true; }
  protected override bool IsInputKey(Keys key) { return true; }
}
'@ -ReferencedAssemblies System.Windows.Forms,System.Drawing,System.Windows.Forms.Primitives,System.ComponentModel.Primitives
if(-not [Windows.Forms.Application]::SetHighDpiMode([Windows.Forms.HighDpiMode]::PerMonitorV2)){throw 'Visual fixture requires physical client coordinates'}
[Windows.Forms.Application]::EnableVisualStyles()
$statePath=Join-Path $StateDirectory 'state.json'
$fixtureState=@{text='';cleared_text=$null;submitted=$false;enter_count=0;clicks=@();revision=0;moved=$false}
function Save-Fixture {
  $temporary=$statePath+'.pending'
  [IO.File]::WriteAllText($temporary,($fixtureState|ConvertTo-Json -Depth 8 -Compress),[Text.UTF8Encoding]::new($false))
  [IO.File]::Move($temporary,$statePath,$true)
}
$form=[Windows.Forms.Form]::new()
$title='Kanzei visual fixture '+[IO.Path]::GetFileName($StateDirectory)
$form.Text=$title
$form.Size=[Drawing.Size]::new(1800,1200)
$form.StartPosition='CenterScreen'
$canvas=[DesktopCanvas]::new()
$canvas.Name='VisualCanvas';$canvas.Dock='Fill';$canvas.BackColor=[Drawing.Color]::White
$canvas.Add_Paint({param($sender,$event)
  $font=[Drawing.Font]::new('Segoe UI',18)
  try {
    $event.Graphics.DrawRectangle([Drawing.Pens]::Black,100,100,800,70)
    $event.Graphics.DrawString($fixtureState.text,$font,[Drawing.Brushes]::Black,110,115)
    $event.Graphics.FillRectangle([Drawing.Brushes]::SteelBlue,1000,800,300,80)
    $event.Graphics.DrawString('Submit',$font,[Drawing.Brushes]::White,1070,820)
  } finally {$font.Dispose()}
})
$canvas.Add_MouseClick({param($sender,$event)
  [void]$canvas.Focus()
  $fixtureState.clicks+=@{x=$event.X;y=$event.Y}
  if($event.X -ge 1000 -and $event.X -lt 1300 -and $event.Y -ge 800 -and $event.Y -lt 880){$fixtureState.submitted=$true}
  Save-Fixture
})
$canvas.Add_KeyPress({param($sender,$event)
  if(-not [char]::IsControl($event.KeyChar)){$fixtureState.text+=$event.KeyChar;$canvas.Invalidate();Save-Fixture}
})
$canvas.Add_KeyDown({param($sender,$event)
  if($event.Control -and $event.KeyCode -eq 'A'){$fixtureState.cleared_text=$fixtureState.text;$fixtureState.text='';$canvas.Invalidate();$event.SuppressKeyPress=$true;Save-Fixture}
  if($event.KeyCode -eq 'Enter'){$fixtureState.enter_count++;Save-Fixture}
})
$form.Controls.Add($canvas)
$timer=[Windows.Forms.Timer]::new()
$timer.Interval=100
$timer.Add_Tick({
  $fixtureState.revision++;$form.Text=$title+' '+$fixtureState.revision
  $move=Join-Path $StateDirectory 'move.request'
  if(Test-Path -LiteralPath $move){Remove-Item -LiteralPath $move;$form.Left+=30;$form.Top+=15;$fixtureState.moved=$true}
  Save-Fixture
})
$form.Add_Shown({
  [void][DesktopCanvas]::ShowWindow($form.Handle,5)
  $form.Activate()
  $canvas.Select()
  $process=[Diagnostics.Process]::GetCurrentProcess()
  $identity=@{hwnd=$form.Handle.ToInt64().ToString();pid=$process.Id;started=$process.StartTime.ToUniversalTime().Ticks.ToString();image=$process.MainModule.FileName;title=$form.Text}
  [IO.File]::WriteAllText((Join-Path $StateDirectory 'window.json'),($identity|ConvertTo-Json -Compress),[Text.UTF8Encoding]::new($false))
  Save-Fixture;$timer.Start()
})
try{[Windows.Forms.Application]::Run($form)}finally{$timer.Dispose()}
