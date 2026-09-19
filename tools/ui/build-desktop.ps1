#Requires -Version 5.1
<#
.SYNOPSIS
  构建独立命名的 Tauri/Rust Windows 预览安装器。

.DESCRIPTION
  默认先跑完整 UI/Rust 门禁，再构建 release 二进制并调用 Inno Setup。
  预览包不会覆盖现有 Go 正式安装入口。未签名是允许的，但必须通过 -RequireSignature
  才能生成正式签名包；签名工具约定与现有发布脚本一致。
#>
param(
  [string]$Version,
  [string]$OutputDir,
  [switch]$SkipGate,
  [switch]$RequireSignature
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

$env:CARGO_HOME = if ($env:CARGO_HOME -like 'D:\*') { $env:CARGO_HOME } else { 'D:\Tools\cargo' }
$env:RUSTUP_HOME = if ($env:RUSTUP_HOME -like 'D:\*') { $env:RUSTUP_HOME } else { 'D:\Tools\rustup' }
$env:CARGO_TARGET_DIR = if ($env:CARGO_TARGET_DIR -like 'D:\*') { $env:CARGO_TARGET_DIR } else { 'D:\Temp\agentnotify-rust-target' }
$env:npm_config_cache = if ($env:npm_config_cache -like 'D:\*') { $env:npm_config_cache } else { 'D:\Temp\npm-cache' }
$env:TEMP = if ($env:TEMP -like 'D:\*') { $env:TEMP } else { 'D:\Temp\agentnotify-temp' }
$env:TMP = $env:TEMP
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR,$env:TEMP,$env:npm_config_cache | Out-Null

if (-not $OutputDir) {
  $OutputDir = Join-Path $RepoRoot 'dist\rust-preview'
}
$OutputDir = [IO.Path]::GetFullPath($OutputDir)

$tauriConfigPath = Join-Path $RepoRoot 'hosts\desktop-tauri\tauri.conf.json'
$tauriConfig = Get-Content -LiteralPath $tauriConfigPath -Raw -Encoding utf8 | ConvertFrom-Json
if (-not $Version) {
  $Version = [string]$tauriConfig.version
}
if ($Version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') {
  throw "版本号不是有效的 SemVer：$Version"
}
$versionMatch = [regex]::Match($Version, '^(\d+)\.(\d+)\.(\d+)')
if (-not $versionMatch.Success) {
  throw "无法从版本号生成 Windows 文件版本：$Version"
}
$VersionInfo = "$($versionMatch.Groups[1].Value).$($versionMatch.Groups[2].Value).$($versionMatch.Groups[3].Value).0"

$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
  throw "Cargo executable not found: $cargo"
}
if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
  . (Join-Path $RepoRoot 'tools\rust\xwin-env.ps1')
}

if (-not $SkipGate) {
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\ui\gate.ps1')
  if ($LASTEXITCODE -ne 0) {
    throw "UI/Rust gate failed with exit code $LASTEXITCODE"
  }
} else {
  & npm --prefix (Join-Path $RepoRoot 'apps\desktop-ui') run build
  if ($LASTEXITCODE -ne 0) {
    throw "UI build failed with exit code $LASTEXITCODE"
  }
}

Push-Location $RepoRoot
try {
  & $cargo build -p agentnotify-desktop --release --locked --target x86_64-pc-windows-msvc
  if ($LASTEXITCODE -ne 0) {
    throw "Tauri host release build failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

$exePath = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc\release\agentnotify-desktop.exe'
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
  throw "Tauri host executable not found: $exePath"
}

$signTool = $null
if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_SIGNTOOL)) {
  $signToolCandidate = $env:AGENT_NOTIFY_SIGNTOOL.Trim()
  if (Test-Path -LiteralPath $signToolCandidate -PathType Leaf) {
    $signTool = (Resolve-Path -LiteralPath $signToolCandidate).Path
  } else {
    $signCommand = Get-Command $signToolCandidate -ErrorAction SilentlyContinue
    if (-not $signCommand) {
      throw "找不到签名工具：$signToolCandidate"
    }
    $signTool = $signCommand.Source
  }
}
if ($RequireSignature -and -not $signTool) {
  throw '要求签名时必须设置 AGENT_NOTIFY_SIGNTOOL。'
}

if ($signTool) {
  & $signTool sign $exePath
  if ($LASTEXITCODE -ne 0) {
    throw "宿主程序签名失败 exit=$LASTEXITCODE"
  }
}

function Resolve-Iscc {
  if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_ISCC)) {
    if (-not (Test-Path -LiteralPath $env:AGENT_NOTIFY_ISCC -PathType Leaf)) {
      throw "AGENT_NOTIFY_ISCC 指向的文件不存在：$env:AGENT_NOTIFY_ISCC"
    }
    return (Resolve-Path -LiteralPath $env:AGENT_NOTIFY_ISCC).Path
  }
  $command = Get-Command iscc.exe -ErrorAction SilentlyContinue
  if ($command) { return $command.Source }
  foreach ($candidate in @(
      'D:\Temp\InnoSetup\ISCC.exe',
      "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
      "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    )) {
    if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }
  return $null
}

$iscc = Resolve-Iscc
if (-not $iscc) {
  throw '找不到 ISCC.exe。请安装 Inno Setup 6，或设置 AGENT_NOTIFY_ISCC。'
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$installer = Join-Path $OutputDir "Agent-notify-Rust-Preview-Setup-v$Version.exe"
if (Test-Path -LiteralPath $installer -PathType Leaf) {
  [IO.File]::Delete($installer)
}

$isccArgs = @(
  "/DAppVersion=$Version",
  "/DVersionInfo=$VersionInfo",
  "/DRepoRoot=$RepoRoot",
  "/DOutputDir=$OutputDir",
  "/DExePath=$exePath",
  "/DIconPath=$(Join-Path $RepoRoot 'hosts\desktop-tauri\icons\icon.ico')"
)
if ($signTool) {
  $isccArgs += '/DSignToolCommand=1'
  $isccArgs += ('/Sagentnotify="{0}" sign "$f"' -f $signTool)
}
& $iscc @isccArgs (Join-Path $RepoRoot 'installer\agent-notify-rust.iss')
if ($LASTEXITCODE -ne 0) {
  throw "Inno Setup build failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
  throw "预览安装器未生成：$installer"
}

if ($signTool) {
  $signature = Get-AuthenticodeSignature -LiteralPath $installer
  if (-not $signature.SignerCertificate -or $signature.Status -notin @('Valid', 'UnknownError', 'NotTrusted')) {
    throw "预览安装器签名无效：$($signature.Status)"
  }
  Write-Output "[build-desktop] signed installer: $installer"
} else {
  Write-Warning '[build-desktop] 生成的是未签名 Rust 预览包，不会替换正式安装入口。'
}

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\ui\smoke-desktop.ps1') -Installer $installer
if ($LASTEXITCODE -ne 0) {
  throw "桌面安装 smoke 失败 exit=$LASTEXITCODE"
}

$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumPath = Join-Path $OutputDir "SHA256SUMS-Rust-Preview-$Version.txt"
[IO.File]::WriteAllText($checksumPath, "$hash  $([IO.Path]::GetFileName($installer))`r`n", (New-Object Text.UTF8Encoding($false)))

Write-Output "[build-desktop] $installer"
Write-Output "[build-desktop] $checksumPath"
