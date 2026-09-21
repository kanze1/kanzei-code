param(
    [Parameter(Mandatory = $true)][string]$VoiceDirectory,
    [string]$RuntimeDirectory = (Join-Path $env:USERPROFILE '.kanzei/voice-runtime'),
    [string]$Distribution = 'Ubuntu'
)
$ErrorActionPreference = 'Stop'
$env:WSL_UTF8 = '1'
$source = (Resolve-Path -LiteralPath $VoiceDirectory).Path
$requirements = (& wsl.exe -d $Distribution --exec wslpath -u (Join-Path $PSScriptRoot 'requirements.txt').Replace('\', '/')).Trim()
& wsl.exe -d $Distribution -- bash -lc 'python3 -m venv ~/.venvs/kanzei-voice && ~/.venvs/kanzei-voice/bin/python -m pip install uv'
if ($LASTEXITCODE -ne 0) { throw 'Voice Python environment setup failed.' }
$linuxUserHome = (& wsl.exe -d $Distribution --exec printenv HOME).Trim()
& wsl.exe -d $Distribution --exec "$linuxUserHome/.venvs/kanzei-voice/bin/uv" pip install --python "$linuxUserHome/.venvs/kanzei-voice/bin/python" -r $requirements
if ($LASTEXITCODE -ne 0) { throw 'Voice dependencies could not be installed.' }
$doctor = (& wsl.exe -d $Distribution --exec wslpath -u (Join-Path $PSScriptRoot 'doctor.py').Replace('\', '/')).Trim()
& wsl.exe -d $Distribution --exec "$linuxUserHome/.venvs/kanzei-voice/bin/python" $doctor
if ($LASTEXITCODE -ne 0) { throw 'CUDA compiler validation failed.' }
& uv run --with huggingface-hub python (Join-Path $PSScriptRoot 'download_models.py') --root $RuntimeDirectory
if ($LASTEXITCODE -ne 0) { throw 'Voice models could not be downloaded.' }
& uv run python (Join-Path $PSScriptRoot 'prepare_runtime.py') --root $RuntimeDirectory --voice-directory $source
if ($LASTEXITCODE -ne 0) { throw 'Voice reference configuration failed.' }
Write-Output 'Voice setup complete. Run scripts/voice/start.ps1 to start it.'
