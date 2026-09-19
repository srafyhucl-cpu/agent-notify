$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$env:CARGO_HOME = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { 'D:\Tools\cargo' }
$env:RUSTUP_HOME = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { 'D:\Tools\rustup' }
$env:CARGO_TARGET_DIR = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'D:\Temp\agentnotify-rust-target' }
$env:TEMP = if ($env:TEMP -like 'D:\*') { $env:TEMP } else { 'D:\Temp\agentnotify-temp' }
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR,$env:TEMP | Out-Null

Push-Location $root
try {
    cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt 失败' }
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy 失败' }
    cargo test --workspace --all-features
    if ($LASTEXITCODE -ne 0) { throw 'cargo test 失败' }
}
finally {
    Pop-Location
}
