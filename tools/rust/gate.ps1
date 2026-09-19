param([switch]$RequireMsvc)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$target = 'x86_64-pc-windows-msvc'
$env:CARGO_HOME = 'D:\Tools\cargo'
$env:RUSTUP_HOME = 'D:\Tools\rustup'
$env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target'
$env:TEMP = 'D:\Temp\agentnotify-temp'
$env:TMP = $env:TEMP
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH

New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR,$env:TEMP | Out-Null
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    throw "Cargo executable not found: $cargo. Install the Rust toolchain on D drive."
}

$msvcLink = Get-Command link.exe -ErrorAction SilentlyContinue
if (-not $msvcLink) {
    if ($RequireMsvc) {
        throw 'MSVC link.exe not found. Install Desktop development with C++ before running the release gate.'
    }

    $xwinEnv = Join-Path $PSScriptRoot 'xwin-env.ps1'
    if (-not (Test-Path -LiteralPath $xwinEnv -PathType Leaf)) {
        throw 'MSVC link.exe and the local cargo-xwin fallback are unavailable.'
    }

    . $xwinEnv
    Write-Warning 'MSVC link.exe not found; using the local cargo-xwin fallback.'
}

Push-Location $root
try {
    & $cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
    & $cargo clippy --workspace --all-targets --all-features --target $target -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
    & $cargo test --workspace --all-features --target $target
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
}
finally {
    Pop-Location
}
