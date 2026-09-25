#Requires -Version 5.1
<#
.SYNOPSIS
  快速构建并就地部署桌面端，用于真机目检（比正式发布链快得多）。

.DESCRIPTION
  1) 构建前端（Vite）；
  2) 触发 build.rs 重新打包最新 dist，并增量构建 Tauri 宿主；
  3) 若本机安装目录存在，则结束正在运行的实例、覆盖主程序并清理 WebView2 缓存，
     避免窗口加载旧页面。

  注意：本脚本会**强制结束**正在运行的 AgentNotify 进程，并删除 WebView2 缓存目录，
  仅用于开发机。

.PARAMETER Run
  构建完成后启动最新产物。

.PARAMETER OpenDist
  在资源管理器中定位产物。

.PARAMETER InstallDir
  部署目标目录；默认 %LOCALAPPDATA%\Programs\Agent-notify，不存在时跳过部署。
#>
param(
  [switch]$Run,
  [switch]$OpenDist,
  [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Agent-notify')
)

$ErrorActionPreference = 'Stop'
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

# 本机约定：D 盘工具链与缓存（与 tools\rust\gate.ps1 一致，存在才启用；其它机器用环境默认值）
$localCargo = 'D:\Tools\cargo'
if (Test-Path -LiteralPath (Join-Path $localCargo 'bin\cargo.exe') -PathType Leaf) {
  $env:CARGO_HOME = $localCargo
  if (-not $env:RUSTUP_HOME) { $env:RUSTUP_HOME = 'D:\Tools\rustup' }
  if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target' }
  if (-not $env:TEMP -or $env:TEMP -like 'C:\*') { $env:TEMP = 'D:\Temp\agentnotify-temp' }
  $env:PATH = (Join-Path $localCargo 'bin') + ';' + $env:PATH
}
$env:TMP = $env:TEMP
if ($env:CARGO_TARGET_DIR) { New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR | Out-Null }
if ($env:TEMP) { New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null }

$cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
if (-not $cargoCommand) {
  throw 'cargo.exe not found. Install the Rust toolchain (local convention: D:\Tools\cargo) or add cargo to PATH.'
}
$cargo = $cargoCommand.Source
if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
  . (Join-Path $RepoRoot 'tools\rust\xwin-env.ps1')
}

Write-Host '==> [1/3] Building frontend UI (Vite)...' -ForegroundColor Cyan
& npm --prefix (Join-Path $RepoRoot 'apps\desktop-ui') run build
if ($LASTEXITCODE -ne 0) {
  throw "Frontend build failed with exit code $LASTEXITCODE"
}

# 强制触发 build.rs 重新打包最新的前端 dist 资源
$buildRs = Join-Path $RepoRoot 'hosts\desktop-tauri\build.rs'
if (Test-Path -LiteralPath $buildRs) {
  (Get-Item -LiteralPath $buildRs).LastWriteTime = Get-Date
}

Write-Host '==> [2/3] Building Tauri host incremental binary...' -ForegroundColor Cyan
Push-Location $RepoRoot
try {
  & $cargo build -p agentnotify-desktop --release --locked --target x86_64-pc-windows-msvc --features tauri/custom-protocol
  if ($LASTEXITCODE -ne 0) {
    throw "Tauri incremental build failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

$targetDir = $env:CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($targetDir)) { $targetDir = Join-Path $RepoRoot 'target' }
$exePath = Join-Path $targetDir 'x86_64-pc-windows-msvc\release\agentnotify-desktop.exe'
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
  throw "Target binary not found: $exePath"
}

$sw.Stop()
$elapsed = [math]::Round($sw.Elapsed.TotalSeconds, 1)
Write-Host "==> [3/3] Quick build completed in $elapsed seconds!" -ForegroundColor Green
Write-Host "Binary: $exePath" -ForegroundColor Gray

# 就地部署到本机安装目录并清理 WebView2 缓存（避免窗口加载旧页面）
if (Test-Path -LiteralPath $InstallDir) {
  Write-Host "==> Syncing binary to local installation directory: $InstallDir" -ForegroundColor Cyan
  Get-Process | Where-Object { $_.Path -like '*agentnotify*' -or $_.ProcessName -like '*agentnotify*' } | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 800
  Copy-Item -LiteralPath $exePath -Destination (Join-Path $InstallDir 'agentnotify-desktop.exe') -Force

  $webviewRoot = Join-Path $env:LOCALAPPDATA 'com.agentnotify.desktop\EBWebView\Default'
  foreach ($cacheName in 'Cache', 'Code Cache', 'GPUCache') {
    $cacheDir = Join-Path $webviewRoot $cacheName
    if (Test-Path -LiteralPath $cacheDir) {
      Remove-Item -LiteralPath $cacheDir -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
  Write-Host '==> WebView2 cache cleared.' -ForegroundColor Green
} else {
  Write-Host "==> Install directory not found, skipped deployment: $InstallDir" -ForegroundColor Yellow
}

if ($Run) {
  Write-Host 'Stopping existing instances...' -ForegroundColor Yellow
  Get-Process -Name 'agentnotify-desktop', 'agentnotify' -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Milliseconds 600
  Write-Host 'Launching latest binary...' -ForegroundColor Green
  Start-Process -FilePath $exePath
}

if ($OpenDist) {
  Start-Process -FilePath 'explorer.exe' -ArgumentList "/select,$exePath"
}
