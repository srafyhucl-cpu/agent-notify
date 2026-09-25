$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$target = 'x86_64-pc-windows-msvc'
# Local convention keeps the toolchain, caches and target dir on the D drive.
# On CI those paths do not exist, so use the ambient rustup/cargo install instead.
# Keep in sync with tools\rust\gate.ps1.
$localCargo = 'D:\Tools\cargo'
if (Test-Path -LiteralPath (Join-Path $localCargo 'bin\cargo.exe') -PathType Leaf) {
    $env:CARGO_HOME = $localCargo
    if (-not $env:RUSTUP_HOME) { $env:RUSTUP_HOME = 'D:\Tools\rustup' }
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target' }
    if (-not $env:TEMP -or $env:TEMP -like 'C:\*') { $env:TEMP = 'D:\Temp\agentnotify-temp' }
    $env:TMP = $env:TEMP
    $env:PATH = (Join-Path $localCargo 'bin') + ';' + $env:PATH
}
if ($env:CARGO_TARGET_DIR) { New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR | Out-Null }
if ($env:TEMP) { New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null }

$cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
if (-not $cargoCommand) {
    throw 'cargo.exe not found. Install the Rust toolchain (local convention: D:\Tools\cargo) or add cargo to PATH.'
}
$cargo = $cargoCommand.Source

$msvcLink = Get-Command link.exe -ErrorAction SilentlyContinue
if (-not $msvcLink) {
    $xwinEnv = Join-Path $PSScriptRoot 'xwin-env.ps1'
    if (-not (Test-Path -LiteralPath $xwinEnv -PathType Leaf)) {
        throw 'MSVC link.exe and the local cargo-xwin fallback are unavailable.'
    }
    . $xwinEnv
}

Push-Location $root
try {
    & $cargo run --locked -p agentnotify-desktop --bin export-bindings --target $target
    if ($LASTEXITCODE -ne 0) {
        throw 'HostBridge TypeScript generation failed'
    }
}
finally {
    Pop-Location
}
