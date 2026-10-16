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
