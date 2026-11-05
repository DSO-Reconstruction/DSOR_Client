# Builds the client to play (target/fast/dsor-client.exe) from the current
# sources; cargo rebuilds only what changed, so an up-to-date build returns at
# once. Play.cmd runs this before launching, so it never starts a stale client.
# The "fast" profile (Cargo.toml) is optimised but skips cross-crate LTO and
# compiles incrementally: about 20 s after a small change instead of the
# release build's minutes, for frames about 7% slower. Releases and frame-time
# measurements use the release build (cargo build --release).
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
if (Test-Path (Join-Path $workspace '.local/cargo/bin/cargo.exe')) {
    $env:CARGO_HOME = Join-Path $workspace '.local/cargo'
    $env:RUSTUP_HOME = Join-Path $workspace '.local/rustup'
    $env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
}
$installer = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer'
$vswhere = Join-Path $installer 'vswhere.exe'
if (Test-Path $vswhere) {
    $installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($installation) {
        & (Join-Path $installation 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
    }
}
# A client still running holds its executable; Windows lets it be renamed, so
# the new build can take its place (the old copy is removed once it is free).
$binary = Join-Path $workspace 'target/fast/dsor-client.exe'
Get-ChildItem -LiteralPath (Join-Path $workspace 'target/fast') -Filter 'dsor-client.exe.running-*' -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-Item -LiteralPath $_.FullName -ErrorAction SilentlyContinue }
if (Test-Path -LiteralPath $binary) {
    try {
        $stream = [System.IO.File]::Open($binary, 'Open', 'ReadWrite', 'None')
        $stream.Close()
    } catch {
        Move-Item -LiteralPath $binary -Destination ($binary + '.running-' + [guid]::NewGuid().ToString('N'))
    }
}
Push-Location $workspace
try {
    $ErrorActionPreference = 'Continue'
    # The client crate shows no progress while it compiles (the bar rests on
    # "dsor-client(bin)"). The first build in target/fast also builds every
    # dependency once.
    Write-Host 'Building the client (unchanged: a few seconds; after a small change: about 20 seconds; after many new commits: a few minutes)...'
    cargo build --locked --profile fast -p dsor-client
    if ($LASTEXITCODE -ne 0) { throw "Client build failed with code $LASTEXITCODE." }
    # DLSS loads its runtime from the executable's directory (when the SDK is installed).
    $dlss = Join-Path $workspace '.local/dlss/sdk/lib/Windows_x86_64/rel/nvngx_dlss.dll'
    if (Test-Path -LiteralPath $dlss) { Copy-Item -LiteralPath $dlss -Destination (Join-Path $workspace 'target/fast/nvngx_dlss.dll') -Force }
} finally {
    Pop-Location
}
