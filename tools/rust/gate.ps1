$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$env:CARGO_HOME = 'D:\Tools\cargo'
$env:RUSTUP_HOME = 'D:\Tools\rustup'
$env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target'
$env:TEMP = 'D:\Temp\agentnotify-temp'
$env:TMP = $env:TEMP
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH

New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR,$env:TEMP | Out-Null
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    throw "找不到 cargo.exe：$cargo。请先把 Rust 工具链安装到 D 盘。"
}

Push-Location $root
try {
    & $cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt 失败' }
    & $cargo clippy --workspace --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy 失败' }
    & $cargo test --workspace --all-features
    if ($LASTEXITCODE -ne 0) { throw 'cargo test 失败' }
}
finally {
    Pop-Location
}
