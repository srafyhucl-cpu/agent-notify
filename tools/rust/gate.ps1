param([switch]$RequireMsvc)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$target = 'x86_64-pc-windows-msvc'

# Local convention keeps the toolchain, caches and target dir on the D drive.
# On CI those paths do not exist, so use the ambient rustup/cargo install instead.
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
    if ($RequireMsvc) {
        throw 'MSVC link.exe not found. Install Desktop development with C++ before running the release gate.'
    }

    # The cargo-xwin fallback is a local-only convenience for machines without Visual Studio.
    # CI runners do have it and rustc locates MSVC by itself, so only enable the fallback when
    # the LLVM toolchain it depends on is actually present.
    $llvmHome = if ($env:AGENTNOTIFY_LLVM_HOME) { $env:AGENTNOTIFY_LLVM_HOME } else { 'D:\Tools\LLVM-23.1.1\LLVM' }
    $xwinEnv = Join-Path $PSScriptRoot 'xwin-env.ps1'
    if ((Test-Path -LiteralPath (Join-Path $llvmHome 'bin\clang-cl.exe')) -and (Test-Path -LiteralPath $xwinEnv -PathType Leaf)) {
        . $xwinEnv
        Write-Warning 'MSVC link.exe not found; using the local cargo-xwin fallback.'
    }
}

# Test TempDir cleanup can lose the race against the dedicated SQLite thread that still holds
# state.db, so the gate sweeps its own test directories once the run is green.
function Remove-TestResidue {
    param([Parameter(Mandatory = $true)][string]$Root)

    if (-not (Test-Path -LiteralPath $Root)) { return }
    $cleaned = 0
    $candidates = @(Get-ChildItem -LiteralPath $Root -Directory -Force -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^agentnotify-[A-Za-z0-9-]+-test-[A-Za-z0-9]+$' })
    foreach ($candidate in $candidates) {
        $path = $candidate.FullName
        try {
            [IO.Directory]::Delete($path, $true)
            $cleaned++
        } catch {
            Write-Warning "failed to clean test temp directory: $path"
        }
    }
    if ($cleaned -gt 0) { Write-Output "[gate] cleaned $cleaned test temp directories" }
}

Push-Location $root
try {
    & $cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
    & $cargo clippy --workspace --all-targets --all-features --target $target -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
    & $cargo test --workspace --all-features --target $target
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }

    # hosts/desktop-tauri/tests/production_contract.rs builds its temp roots in D:\Temp
    # (D_DRIVE_TEMP) rather than $env:TEMP, so sweep that path. Only on a green run: a failed
    # run keeps the residue for inspection, matching tests\smoke.ps1.
    Remove-TestResidue -Root 'D:\Temp'
}
finally {
    Pop-Location
}
