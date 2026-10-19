# Hold Cargo's profile locks until the owning Node process closes stdin.
param([Parameter(Mandatory = $true)][string]$Directory)
$ErrorActionPreference = 'Stop'
$streams = [Collections.Generic.List[IO.FileStream]]::new()
try {
    foreach ($name in @('.cargo-lock', '.cargo-build-lock', '.cargo-artifact-lock')) {
        $stream = [IO.FileStream]::new((Join-Path $Directory $name), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
        $streams.Add($stream)
        $stream.Lock(0, [long]::MaxValue)
    }
    [Console]::Out.WriteLine('locked')
    [Console]::Out.Flush()
    [Console]::In.ReadLine() | Out-Null
} finally {
    foreach ($stream in $streams) { $stream.Dispose() }
}
