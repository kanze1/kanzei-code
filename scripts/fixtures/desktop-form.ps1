param([Parameter(Mandatory=$true)][string]$StateDirectory)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[Windows.Forms.Application]::EnableVisualStyles()
[Windows.Forms.Application]::SetCompatibleTextRenderingDefault($false)
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class KzFixtureWindow { [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window,int command); }'
$statePath=Join-Path $StateDirectory 'state.json'
$fixtureState=@{submitted=$false;name='';current_name='';canvas_clicks=0;file=$null;enter_count=0;escape_count=0;blocked=$false}
$blockName='Local\KanzeiDesktopFixture-'+[Diagnostics.Process]::GetCurrentProcess().Id
$blockEvent=[Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,$blockName)
function Save-Fixture {
  $temporary=$statePath+'.pending'
  [IO.File]::WriteAllText($temporary,($fixtureState|ConvertTo-Json -Compress),(New-Object Text.UTF8Encoding($false)))
  [IO.File]::Move($temporary,$statePath,$true)
}
$form=New-Object Windows.Forms.Form
$form.Text='Kanzei Desktop Fixture '+[IO.Path]::GetFileName($StateDirectory)
$form.Size=New-Object Drawing.Size(650,520)
$form.StartPosition='CenterScreen'
$name=New-Object Windows.Forms.TextBox
$name.Name='CustomerName';$name.AccessibleName='Customer name';$name.Location=New-Object Drawing.Point(25,25);$name.Width=350
$name.Add_TextChanged({
  $fixtureState.current_name=$name.Text
  if($name.Text -eq 'CONTROL_BLOCK'){$fixtureState.blocked=$true;Save-Fixture;[void]$blockEvent.WaitOne();$fixtureState.blocked=$false}
  Save-Fixture
})
$name.Add_KeyDown({param($sender,$event) if($event.KeyCode -eq 'Enter'){$fixtureState.enter_count++;Save-Fixture};if($event.KeyCode -eq 'Escape'){$fixtureState.escape_count++;Save-Fixture}})
$secret=New-Object Windows.Forms.TextBox
$secret.Name='Secret';$secret.AccessibleName='Password';$secret.UseSystemPasswordChar=$true;$secret.Text='UIA_PASSWORD_CANARY';$secret.Location=New-Object Drawing.Point(25,65)
$submit=New-Object Windows.Forms.Button
$submit.Text='Submit form';$submit.Name='Submit';$submit.Location=New-Object Drawing.Point(25,105);$submit.Width=130
$submit.Add_Click({$fixtureState.name=$name.Text;$fixtureState.submitted=$true;Save-Fixture})
$choose=New-Object Windows.Forms.Button
$choose.Text='Choose file';$choose.Name='ChooseFile';$choose.Location=New-Object Drawing.Point(180,105);$choose.Width=130
$choose.Add_Click({$dialog=New-Object Windows.Forms.OpenFileDialog;try{$dialog.InitialDirectory=$StateDirectory;if($dialog.ShowDialog($form)-eq[Windows.Forms.DialogResult]::OK){$fixtureState.file=$dialog.FileName;Save-Fixture}}finally{$dialog.Dispose()}})
$canvas=New-Object Windows.Forms.Panel
$canvas.Name='DrawingCanvas';$canvas.AccessibleName='Drawing canvas';$canvas.Location=New-Object Drawing.Point(25,160);$canvas.Size=New-Object Drawing.Size(550,260);$canvas.BackColor=[Drawing.Color]::SteelBlue
$canvas.Add_MouseClick({param($sender,$event)$fixtureState.canvas_clicks++;$fixtureState.canvas_point=@{x=$event.X;y=$event.Y};Save-Fixture})
$form.Controls.AddRange(@($name,$secret,$submit,$choose,$canvas))
$form.Add_Shown({
    # The parent hides the PowerShell console; explicitly show this owned test form.
    [void][KzFixtureWindow]::ShowWindow($form.Handle,5)
    Save-Fixture
    $process=[Diagnostics.Process]::GetCurrentProcess()
    $identity=@{hwnd=$form.Handle.ToInt64().ToString();pid=$process.Id;started=$process.StartTime.ToUniversalTime().Ticks.ToString();image=$process.MainModule.FileName;title=$form.Text;block_event=$blockName}
    [IO.File]::WriteAllText((Join-Path $StateDirectory 'window.json'),($identity|ConvertTo-Json -Compress),(New-Object Text.UTF8Encoding($false)))
})
try{[Windows.Forms.Application]::Run($form)}finally{$blockEvent.Dispose()}
