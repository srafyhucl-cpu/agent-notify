param([switch]$Force)

$ErrorActionPreference = 'Stop'

$toolRoot = 'D:\Tools'
$tempRoot = 'D:\Temp\agentnotify-tools'
$cargoXwinHome = Join-Path $toolRoot 'cargo-xwin'
$llvmHome = Join-Path $toolRoot 'LLVM-23.1.1\LLVM'
$xwinCacheDir = Join-Path $toolRoot 'xwin-cache'
$cargoXwinArchive = Join-Path $tempRoot 'cargo-xwin-v0.23.1.windows-x64.zip'
$llvmInstaller = Join-Path $tempRoot 'LLVM-23.1.1-win64.msi'
$cargoXwinUrl = 'https://github.com/rust-cross/cargo-xwin/releases/download/v0.23.1/cargo-xwin-v0.23.1.windows-x64.zip'
$cargoXwinSha256 = '114534C57CDCCA604F59C9D70024583D20FF7C5505677F06247E971184A23872'
$llvmUrl = 'https://github.com/llvm/llvm-project/releases/download/llvmorg-23.1.1/LLVM-23.1.1-win64.msi'
$llvmSha256 = '11AF43BA261A1158090DD6F0DA40D7A34F32DBFB4A76B43240BFFA1EBB140984'

function Assert-Hash {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    if ($actual -ne $Expected) {
        throw "SHA256 mismatch for $Path. Expected $Expected, got $actual."
    }
}

function Get-VerifiedDownload {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    if (-not $Force -and (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $existingHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
        if ($existingHash -eq $Expected) {
            return
        }
    }

    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Path
    Assert-Hash -Path $Path -Expected $Expected
}

New-Item -ItemType Directory -Force -Path $toolRoot,$tempRoot | Out-Null

$cargoXwinExecutable = Join-Path $cargoXwinHome 'cargo-xwin.exe'
$cargoXwinArchiveUsed = $false
if ($Force -or -not (Test-Path -LiteralPath $cargoXwinExecutable -PathType Leaf)) {
    Get-VerifiedDownload -Url $cargoXwinUrl -Path $cargoXwinArchive -Expected $cargoXwinSha256
    New-Item -ItemType Directory -Force -Path $cargoXwinHome | Out-Null
    Expand-Archive -LiteralPath $cargoXwinArchive -DestinationPath $cargoXwinHome -Force
    $cargoXwinArchiveUsed = $true
}

$clangCl = Join-Path $llvmHome 'bin\clang-cl.exe'
$llvmInstallerUsed = $false
if ($Force -or -not (Test-Path -LiteralPath $clangCl -PathType Leaf)) {
    Get-VerifiedDownload -Url $llvmUrl -Path $llvmInstaller -Expected $llvmSha256
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $llvmHome) | Out-Null
    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList @(
        '/a',
        $llvmInstaller,
        '/qn',
        "TARGETDIR=$(Split-Path -Parent $llvmHome)"
    ) -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
        throw "LLVM MSI administrative extraction failed with exit code $($process.ExitCode)."
    }
    $llvmInstallerUsed = $true
}

$env:AGENTNOTIFY_LLVM_HOME = $llvmHome
$env:AGENTNOTIFY_CARGO_XWIN_HOME = $cargoXwinHome
$env:AGENTNOTIFY_XWIN_CACHE_DIR = $xwinCacheDir
. (Join-Path $PSScriptRoot 'xwin-env.ps1')

$env:CARGO_HOME = 'D:\Tools\cargo'
$env:RUSTUP_HOME = 'D:\Tools\rustup'
$env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target'
$env:TEMP = 'D:\Temp\agentnotify-temp'
$env:TMP = $env:TEMP
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    throw "Cargo executable not found: $cargo"
}

Push-Location (Resolve-Path (Join-Path $PSScriptRoot '..\..'))
try {
    & $cargo xwin check -p agentnotify-domain --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) {
        throw 'cargo-xwin cache initialization failed.'
    }
}
finally {
    Pop-Location
}

if ($cargoXwinArchiveUsed) {
    Remove-Item -LiteralPath $cargoXwinArchive -Force -ErrorAction SilentlyContinue
}
if ($llvmInstallerUsed) {
    Remove-Item -LiteralPath $llvmInstaller -Force -ErrorAction SilentlyContinue
}
