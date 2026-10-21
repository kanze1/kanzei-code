$kanzeiSource = $env:KANZEI_SHELL_SOURCE
Remove-Item Env:KANZEI_SHELL_SOURCE
$kanzeiTokens = $null
$kanzeiErrors = $null
$kanzeiAst = [System.Management.Automation.Language.Parser]::ParseInput($kanzeiSource, [ref]$kanzeiTokens, [ref]$kanzeiErrors)
function Stop-KanzeiSyntaxGuard([string]$code, [string]$message) {
    [Console]::Error.WriteLine('KANZEI_SHELL_GUARD:' + (@{code=$code; message=$message} | ConvertTo-Json -Compress))
    exit 2
}
if ($kanzeiErrors.Count -gt 0) {
    Stop-KanzeiSyntaxGuard 'SHELL_SYNTAX' ($kanzeiErrors | ForEach-Object Message | Out-String)
}
foreach ($kanzeiCall in $kanzeiAst.FindAll({param($node) $node -is [System.Management.Automation.Language.CommandAst]}, $true)) {
    $kanzeiName = $kanzeiCall.GetCommandName()
    if ($null -eq $kanzeiName) { continue }
    # Resolve native PowerShell aliases as commands, never inspect argument text.
    $kanzeiAlias = Get-Alias -Name $kanzeiName -ErrorAction SilentlyContinue
    if ($null -ne $kanzeiAlias) { $kanzeiName = $kanzeiAlias.ResolvedCommand.Name }
    $kanzeiName = [System.IO.Path]::GetFileNameWithoutExtension($kanzeiName)
    if ($env:KANZEI_SHELL_VERIFICATION -eq '1' -and $kanzeiName -in @('Set-Location', 'Push-Location', 'Pop-Location')) {
        Stop-KanzeiSyntaxGuard 'SHELL_TEST_WORKDIR' 'Verification must use a fixed workdir. Pass workdir in the tool input instead of changing directories inside the command.'
    }
    if ($kanzeiName -in @('Set-Content', 'Out-File')) {
        Stop-KanzeiSyntaxGuard 'SHELL_FILE_WRITE' "${kanzeiName} is blocked: whole-file rewrites must use edit/write for validation and review."
    }
    if ($kanzeiName -ne 'git') { continue }
    $kanzeiArguments = @($kanzeiCall.CommandElements | Select-Object -Skip 1 | ForEach-Object {
        if ($_ -is [System.Management.Automation.Language.StringConstantExpressionAst]) { $_.Value } else { $_.Extent.Text }
    })
    $kanzeiIndex = 0
    while ($kanzeiIndex -lt $kanzeiArguments.Count -and $kanzeiArguments[$kanzeiIndex].StartsWith('-')) {
        if ($kanzeiArguments[$kanzeiIndex] -cin @('-C', '-c', '--git-dir', '--work-tree')) { $kanzeiIndex += 2 } else { $kanzeiIndex++ }
    }
    if ($kanzeiIndex -lt $kanzeiArguments.Count -and $kanzeiArguments[$kanzeiIndex] -in @('add','stage','commit','checkout','switch','reset','restore','merge','rebase','pull','cherry-pick','revert','clean','rm','mv')) {
        Stop-KanzeiSyntaxGuard 'SHELL_GIT_MUTATION' "git $($kanzeiArguments[$kanzeiIndex]) is blocked: Git mutations must use the structured git tool."
    }
}
# Parse and execute in the same shell process. Strings, comments and here-strings
# are data; nested invocation ASTs are checked before any user statement runs.
if ($env:KANZEI_SHELL_VERIFICATION -eq '1') {
    Remove-Item Env:KANZEI_SHELL_VERIFICATION
    $ErrorActionPreference = 'Stop'
    if (Get-Variable PSNativeCommandUseErrorActionPreference -ErrorAction Ignore) {
        $PSNativeCommandUseErrorActionPreference = $true
    } elseif ($kanzeiAst.FindAll({param($node) $node -is [System.Management.Automation.Language.PipelineAst] -and $node.PipelineElements.Count -gt 1}, $true).Count -gt 0) {
        Stop-KanzeiSyntaxGuard 'SHELL_VERIFICATION_PIPELINE' 'This PowerShell cannot track native failures inside pipelines. Run separate verification statements or use PowerShell with native error tracking.'
    }
    trap {
        $kanzeiStepExit = if ($_.Exception.PSObject.Properties['ExitCode']) { $_.Exception.ExitCode } elseif ($LASTEXITCODE -ne 0) { $LASTEXITCODE } else { 1 }
        [Console]::Error.WriteLine($_.ToString())
        [Console]::Error.WriteLine('KANZEI_SHELL_STEP:' + (@{index=$kanzeiStepIndex; command=$kanzeiStatement.Extent.Text; exit_code=$kanzeiStepExit} | ConvertTo-Json -Compress))
        exit $kanzeiStepExit
    }
    # Verification is a sequence of top-level statements with checked status.
    # Dot-source in one scope so assignments and explicit error handling inside
    # a statement retain PowerShell semantics. Never let a later success hide
    # an earlier native process failure, including pipelines ending in Out-String.
    $kanzeiStepIndex = 0
    foreach ($kanzeiStatement in $kanzeiAst.EndBlock.Statements) {
        $kanzeiStepIndex++
        $global:LASTEXITCODE = 0
        $kanzeiStepResult = @'
$kanzeiStepSucceeded = $?
$kanzeiStepExit = if ($kanzeiStepSucceeded -and $LASTEXITCODE -eq 0) { 0 } elseif ($LASTEXITCODE -ne 0) { $LASTEXITCODE } else { 1 }
[Console]::Error.WriteLine('KANZEI_SHELL_STEP:' + (@{index=$kanzeiStepIndex; command=$kanzeiStatement.Extent.Text; exit_code=$kanzeiStepExit} | ConvertTo-Json -Compress))
if ($kanzeiStepExit -ne 0) { exit $kanzeiStepExit }
'@
        . ([ScriptBlock]::Create($kanzeiStatement.Extent.Text + "`n`n" + $kanzeiStepResult))
    }
    exit 0
}
$kanzeiCompletion = @'
$kanzeiSucceeded = $?
if (-not $kanzeiSucceeded) {
    if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    exit 1
}
'@
# Capture $? inside the user script scope: invoking a script block itself
# reports success even when its last cmdlet reported a non-terminating error.
& ([ScriptBlock]::Create($kanzeiSource + "`n`n" + $kanzeiCompletion))
