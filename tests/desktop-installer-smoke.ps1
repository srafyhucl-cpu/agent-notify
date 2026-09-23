#Requires -Version 5.1
<#
.SYNOPSIS
  验证 2.0 正式安装器（Inno Setup）的分发清单，以及产物安装、启动、单实例、关闭隐藏和卸载边界。

.DESCRIPTION
  不传 -Installer 时只做安装器脚本定义检查（CI 门禁用，不需要构建产物）；
  传 -Installer 时额外校验安装器 PE 元数据，-Execute 再执行真实安装与运行时冒烟。
#>
param(
  # 正式安装器产物路径；缺省只跑脚本定义检查。
  [string]$Installer = '',
  [switch]$Execute,
  [switch]$KeepInstall,
  [string]$InstallRoot = ''
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
# 临时根跟随仓库所在盘（本地 D:、CI 各自的系统盘），不写死盘符。
$driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
$tempRoot = Join-Path $driveRoot 'Temp'
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
  $InstallRoot = Join-Path $tempRoot 'agentnotify-rust-smoke'
}
if ($Execute -and [string]::IsNullOrWhiteSpace($Installer)) {
  throw '-Execute 需要 -Installer 指向已构建的正式安装器'
}

if (-not [string]::IsNullOrWhiteSpace($Installer)) {
  if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) {
    throw "安装器不存在：$Installer"
  }

  $stream = [IO.File]::OpenRead($Installer)
  try {
    $reader = New-Object IO.BinaryReader($stream)
    if ($reader.ReadByte() -ne 0x4d -or $reader.ReadByte() -ne 0x5a) {
      throw '正式安装器不是 Windows PE 文件'
    }
  } finally {
    $stream.Dispose()
  }

  $installerInfo = Get-Item -LiteralPath $Installer
  if ([string]::IsNullOrWhiteSpace($installerInfo.VersionInfo.ProductName)) {
    throw '正式安装器缺少产品名称'
  }
  if ([string]::IsNullOrWhiteSpace($installerInfo.VersionInfo.ProductVersion)) {
    throw '正式安装器缺少产品版本号'
  }
}

$issPath = Join-Path $RepoRoot 'installer\agent-notify.iss'
$issueScript = Get-Content -LiteralPath $issPath -Raw -Encoding utf8
# 2.0 分发清单：5 个二进制 + 三份 v2 插件/mod/扩展 + 五个接入助手 + 升级清理项。
# 缺任何一项都会让用户装完缺组件，这里按项逐一钉住。
foreach ($needle in @(
    'DestName: "agentnotify-desktop.exe"',
    'DestName: "agentnotify-ingress.exe"',
    'DestName: "agentnotify-codex-hook.exe"',
    'DestName: "agentnotify-antigravity-hook.exe"',
    'DestName: "agentnotify-devin-hook.exe"',
    'Source: "{#RepoRoot}\plugin\rust\agent-notify.ts"',
    'Source: "{#RepoRoot}\plugin\devin-extension-v2\package.json"',
    'Source: "{#RepoRoot}\plugin\commandcode-v2\agent-notify.ts"',
    'Source: "{#RepoRoot}\tools\hooks\install-opencode-v2.ps1"',
    'Source: "{#RepoRoot}\tools\hooks\install-codex-v2.ps1"',
    'Source: "{#RepoRoot}\tools\hooks\install-antigravity-v2.ps1"',
    'Source: "{#RepoRoot}\tools\hooks\install-devin-v2.ps1"',
    'Source: "{#RepoRoot}\tools\hooks\install-commandcode-v2.ps1"',
    "WizardIsTaskSelected('opencode')",
    "WizardIsTaskSelected('codex')",
    "WizardIsTaskSelected('antigravity')",
    "WizardIsTaskSelected('devin')",
    "WizardIsTaskSelected('commandcode')",
    'Type: files; Name: "{app}\agent-notify.exe"'
  )) {
  if (-not $issueScript.Contains($needle)) {
    throw "正式安装器脚本缺少 2.0 分发项：$needle"
  }
}

$pluginSmokeRoot = Join-Path $tempRoot ('agentnotify-plugin-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $pluginSmokeRoot | Out-Null
try {
  $pluginSource = Join-Path $RepoRoot 'plugin\rust\agent-notify.ts'
  $pluginInstaller = Join-Path $RepoRoot 'tools\hooks\install-opencode-v2.ps1'
  $fakeIngress = Join-Path $pluginSmokeRoot 'agentnotify-ingress.exe'
  $pluginDestination = Join-Path $pluginSmokeRoot 'plugins\agent-notify.ts'
  [IO.File]::WriteAllBytes($fakeIngress, [byte[]](0x4d, 0x5a))
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $pluginInstaller `
    -Source $pluginSource `
    -Destination $pluginDestination `
    -Ingress $fakeIngress
  if ($LASTEXITCODE -ne 0) {
    throw "OpenCode 插件安装助手失败 exit=$LASTEXITCODE"
  }
  $installedPlugin = Get-Content -LiteralPath $pluginDestination -Raw -Encoding utf8
  $escapedIngress = $fakeIngress.Replace('\', '\\').Replace('"', '\"')
  if (-not $installedPlugin.Contains("const BAKED_INGRESS = `"$escapedIngress`"")) {
    throw 'OpenCode 插件安装助手没有写入 ingress 绝对路径'
  }
  if ($installedPlugin.Contains('const BAKED_INGRESS = ""')) {
    throw 'OpenCode 插件安装助手没有替换空 ingress 标记'
  }
} finally {
  $resolvedPluginSmokeRoot = [IO.Path]::GetFullPath($pluginSmokeRoot)
  $allowedPluginSmokeRoot = [IO.Path]::GetFullPath($tempRoot)
  if ($resolvedPluginSmokeRoot.StartsWith($allowedPluginSmokeRoot, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $resolvedPluginSmokeRoot)) {
    Remove-Item -LiteralPath $resolvedPluginSmokeRoot -Recurse -Force
  }
}

if (-not $Execute) {
  if ([string]::IsNullOrWhiteSpace($Installer)) {
    Write-Output '[desktop-installer-smoke] 安装器脚本定义检查通过（未提供 -Installer，跳过产物安装检查）'
    exit 0
  }
  Write-Output '[desktop-installer-smoke] 正式安装器产物结构检查通过'
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
$allowedRoot = [IO.Path]::GetFullPath((Join-Path $tempRoot 'agentnotify-rust-smoke'))
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
    '/MERGETASKS=!opencode',
    "/DIR=`"$installDir`""
  )
  $installResult = Start-Process -FilePath $Installer -ArgumentList $installArguments -Wait -PassThru
  if ($installResult.ExitCode -ne 0) {
    throw "正式安装器退出码异常：$($installResult.ExitCode)"
  }

  $installedExe = Join-Path $installDir 'agentnotify-desktop.exe'
  if (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
    throw "安装后找不到主程序：$installedExe"
  }

  $installedIngress = Join-Path $installDir 'agentnotify-ingress.exe'
  if (-not (Test-Path -LiteralPath $installedIngress -PathType Leaf)) {
    throw "安装后找不到 ingress：$installedIngress"
  }

  $foreignInstance = @(Get-CimInstance Win32_Process -Filter "Name = 'agentnotify-desktop.exe'" |
    Where-Object { $_.ExecutablePath -and $_.ExecutablePath -ne $installedExe })
  $skipRuntimeSmoke = $foreignInstance.Count -gt 0
  if ($skipRuntimeSmoke) {
    Write-Warning '检测到其他 AgentNotify 桌面实例，跳过启动与单实例检查，不终止用户进程。'
  }

  if (-not $skipRuntimeSmoke) {
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
  }

  $uninstaller = Get-ChildItem -LiteralPath $installDir -Filter 'unins*.exe' -File | Select-Object -First 1
  if (-not $uninstaller) {
    throw '正式安装器没有生成卸载程序。'
  }
  $uninstallResult = Start-Process -FilePath $uninstaller.FullName -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
  if ($uninstallResult.ExitCode -ne 0) {
    throw "卸载退出码异常：$($uninstallResult.ExitCode)"
  }
  if (Test-Path -LiteralPath $installedExe) {
    throw '卸载后主程序仍存在。'
  }
  if (-not $skipRuntimeSmoke) {
    if (-not (Test-Path -LiteralPath $env:AGENT_NOTIFY_CONFIG_DIR -PathType Container)) {
      throw '卸载错误删除了用户配置目录。'
    }
    Write-Output '[desktop-installer-smoke] 安装、启动、单实例、关闭隐藏、退出和卸载检查通过'
  } else {
    Write-Output '[desktop-installer-smoke] 安装与卸载检查通过，启动交互已因已有实例跳过'
  }
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
