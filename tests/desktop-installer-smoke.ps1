#Requires -Version 5.1
<#
.SYNOPSIS
  验证 Rust 桌面预览包的安装、启动、单实例、关闭隐藏和卸载边界。
#>
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [switch]$Execute,
  [switch]$KeepInstall,
  [string]$InstallRoot = 'D:\Temp\agentnotify-rust-smoke'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) {
  throw "安装器不存在：$Installer"
}

$stream = [IO.File]::OpenRead($Installer)
try {
  $reader = New-Object IO.BinaryReader($stream)
  if ($reader.ReadByte() -ne 0x4d -or $reader.ReadByte() -ne 0x5a) {
    throw '预览安装器不是 Windows PE 文件'
  }
} finally {
  $stream.Dispose()
}

$installerInfo = Get-Item -LiteralPath $Installer
if ([string]::IsNullOrWhiteSpace($installerInfo.VersionInfo.ProductName)) {
  throw '预览安装器缺少产品名称'
}
if ([string]::IsNullOrWhiteSpace($installerInfo.VersionInfo.ProductVersion)) {
  throw '预览安装器缺少产品版本号'
}
$issPath = Join-Path $RepoRoot 'installer\agent-notify-rust.iss'
$issueScript = Get-Content -LiteralPath $issPath -Raw -Encoding utf8
if (-not $issueScript.Contains('DestName: "agentnotify-desktop.exe"')) {
  throw '预览安装器脚本没有安装 agentnotify-desktop.exe'
}

if (-not $Execute) {
  Write-Output '[desktop-installer-smoke] 预览安装器结构检查通过'
  exit 0
}

function Test-WebView2Runtime {
  foreach ($root in @(
      'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients',
      'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients',
      'HKCU:\Software\Microsoft\EdgeUpdate\Clients'
    )) {
    foreach ($client in @(Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue)) {
      $properties = Get-ItemProperty -LiteralPath $client.PSPath -ErrorAction SilentlyContinue
      if ($properties.name -like '*WebView2*') {
        return $true
      }
    }
  }
  return $false
}

if (-not (Test-WebView2Runtime)) {
  throw '未检测到 Microsoft Edge WebView2 Runtime，无法执行真实宿主 smoke。'
}

$InstallRoot = [IO.Path]::GetFullPath($InstallRoot)
$allowedRoot = [IO.Path]::GetFullPath('D:\Temp\agentnotify-rust-smoke')
if (-not $InstallRoot.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase)) {
  throw "InstallRoot 必须位于 $allowedRoot 下：$InstallRoot"
}

$smokeRoot = Join-Path $InstallRoot ([guid]::NewGuid().ToString('N'))
$installDir = Join-Path $smokeRoot 'app'
$dataRoot = Join-Path $smokeRoot 'data-root'
New-Item -ItemType Directory -Force -Path $smokeRoot,$dataRoot | Out-Null

$environmentNames = @(
  'AGENT_NOTIFY_CONFIG_DIR',
  'AGENT_NOTIFY_DATA_DIR',
  'AGENT_NOTIFY_LOG_DIR',
  'AGENT_NOTIFY_SPOOL_DIR',
  'AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS'
)
$originalEnvironment = @{}
foreach ($name in $environmentNames) {
  $originalEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

$firstProcess = $null
$secondProcess = $null
try {
  $installArguments = @(
    '/VERYSILENT',
    '/SUPPRESSMSGBOXES',
    '/NORESTART',
    '/SP-',
    "/DIR=`"$installDir`""
  )
  $installResult = Start-Process -FilePath $Installer -ArgumentList $installArguments -Wait -PassThru
  if ($installResult.ExitCode -ne 0) {
    throw "预览安装器退出码异常：$($installResult.ExitCode)"
  }

  $installedExe = Join-Path $installDir 'agentnotify-desktop.exe'
  if (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
    throw "安装后找不到主程序：$installedExe"
  }

  $env:AGENT_NOTIFY_CONFIG_DIR = Join-Path $dataRoot 'config'
  $env:AGENT_NOTIFY_DATA_DIR = Join-Path $dataRoot 'data'
  $env:AGENT_NOTIFY_LOG_DIR = Join-Path $dataRoot 'logs'
  $env:AGENT_NOTIFY_SPOOL_DIR = Join-Path $dataRoot 'spool'
  $env:AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS = '15000'

  $firstProcess = Start-Process -FilePath $installedExe -PassThru -WindowStyle Hidden
  Start-Sleep -Seconds 5
  $firstProcess.Refresh()
  if ($firstProcess.HasExited) {
    throw "首次启动提前退出，ExitCode=$($firstProcess.ExitCode)"
  }
  if (-not (Test-Path -LiteralPath $env:AGENT_NOTIFY_CONFIG_DIR -PathType Container)) {
    throw "首次启动没有创建隔离配置目录：$env:AGENT_NOTIFY_CONFIG_DIR"
  }

  $secondProcess = Start-Process -FilePath $installedExe -PassThru -WindowStyle Hidden
  Start-Sleep -Seconds 3
  $secondProcess.Refresh()
  if (-not $secondProcess.HasExited) {
    throw '第二个实例没有退出，单实例约束失效。'
  }

  $firstProcess.Refresh()
  if ($firstProcess.MainWindowHandle -ne 0) {
    Add-Type -Namespace AgentNotifySmoke -Name NativeMethods -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true)]
public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
'@
    [AgentNotifySmoke.NativeMethods]::PostMessage($firstProcess.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Seconds 2
    $firstProcess.Refresh()
    if ($firstProcess.HasExited) {
      throw '关闭主窗口后进程退出，托盘常驻约束失效。'
    }
  }

  for ($attempt = 0; $attempt -lt 30; $attempt++) {
    $firstProcess.Refresh()
    if ($firstProcess.HasExited) { break }
    Start-Sleep -Milliseconds 500
  }
  if (-not $firstProcess.HasExited) {
    throw '触发 smoke 退出后进程仍未结束。'
  }
  if ($firstProcess.ExitCode -ne 0) {
    throw "宿主 smoke 退出码异常：$($firstProcess.ExitCode)"
  }

  $remaining = @(Get-CimInstance Win32_Process -Filter "Name = 'agentnotify-desktop.exe'" |
    Where-Object { $_.ExecutablePath -eq $installedExe })
  if ($remaining.Count -ne 0) {
    throw "退出后仍有残留进程：$($remaining.ProcessId -join ',')"
  }

  $uninstaller = Get-ChildItem -LiteralPath $installDir -Filter 'unins*.exe' -File | Select-Object -First 1
  if (-not $uninstaller) {
    throw '预览安装器没有生成卸载程序。'
  }
  $uninstallResult = Start-Process -FilePath $uninstaller.FullName -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
  if ($uninstallResult.ExitCode -ne 0) {
    throw "卸载退出码异常：$($uninstallResult.ExitCode)"
  }
  if (Test-Path -LiteralPath $installedExe) {
    throw '卸载后主程序仍存在。'
  }
  if (-not (Test-Path -LiteralPath $env:AGENT_NOTIFY_CONFIG_DIR -PathType Container)) {
    throw '卸载错误删除了用户配置目录。'
  }

  Write-Output '[desktop-installer-smoke] 安装、启动、单实例、关闭隐藏、退出和卸载检查通过'
} finally {
  foreach ($name in $environmentNames) {
    $original = $originalEnvironment[$name]
    if ($null -eq $original) {
      Remove-Item -LiteralPath ("Env:\" + $name) -ErrorAction SilentlyContinue
    } else {
      Set-Item -LiteralPath ("Env:\" + $name) -Value $original
    }
  }
  if ($secondProcess -and -not $secondProcess.HasExited) {
    Stop-Process -Id $secondProcess.Id -Force -ErrorAction SilentlyContinue
  }
  if ($firstProcess -and -not $firstProcess.HasExited) {
    Stop-Process -Id $firstProcess.Id -Force -ErrorAction SilentlyContinue
  }
  if (-not $KeepInstall -and (Test-Path -LiteralPath $smokeRoot)) {
    $resolvedSmokeRoot = [IO.Path]::GetFullPath($smokeRoot)
    if ($resolvedSmokeRoot.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase)) {
      Remove-Item -LiteralPath $resolvedSmokeRoot -Recurse -Force
    }
  }
}
