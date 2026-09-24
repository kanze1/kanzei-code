param(
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [int]$Port = 7389,
    [string]$PythonExecutable = (Get-Command python -ErrorAction Stop).Source
)
$ErrorActionPreference = 'Stop'
if ($Port -lt 1024 -or $Port -gt 65535) { throw 'Port must be between 1024 and 65535.' }
$runtime = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
$python = (Resolve-Path -LiteralPath $PythonExecutable).Path
$manager = Join-Path $runtime 'manage.py'
if (-not (Test-Path -LiteralPath $manager -PathType Leaf)) { throw 'Runtime directory must contain manage.py.' }
$voiceHome = if ($env:KANZEI_HOME) { $env:KANZEI_HOME } else { Join-Path $env:USERPROFILE '.kanzei' }
[System.IO.Directory]::CreateDirectory($voiceHome) | Out-Null
$settingsPath = Join-Path $voiceHome 'voice.json'
$settings = if (Test-Path -LiteralPath $settingsPath) { Get-Content -LiteralPath $settingsPath -Raw -Encoding utf8 | ConvertFrom-Json } else { [pscustomobject]@{language='auto'} }
$settings | Add-Member -NotePropertyName port -NotePropertyValue $Port -Force
$launcher = [ordered]@{port=$Port;program=$python;args=@('-u',$manager,'start','--port',"$Port");workingDirectory=$runtime}
$encoding = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText((Join-Path $voiceHome 'voice-launcher.json'),($launcher | ConvertTo-Json -Depth 5),$encoding)
[System.IO.File]::WriteAllText($settingsPath,($settings | ConvertTo-Json -Depth 5),$encoding)
Write-Output "Voice runtime registered on port $Port. Kanzei can start it automatically."
