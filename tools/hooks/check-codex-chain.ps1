#Requires -Version 5.1
<#
.SYNOPSIS
  Codex 通知链巡检：发现 notify 行被电脑操控插件（CUA）重新包裹或损坏时，
  自动调用 install-codex-v2.ps1 提升回 Hook 直连；可选微信提醒。

.DESCRIPTION
  默认路径：
  - Hook：%LOCALAPPDATA%\Programs\Agent-notify\agentnotify-codex-hook.exe
  - 事件入口：%LOCALAPPDATA%\Programs\Agent-notify\agentnotify-ingress.exe
  - Codex 配置：%USERPROFILE%\.codex\config.toml
  - 巡检日志：%LOCALAPPDATA%\AgentNotify\logs\codex-chain-check.log

  输出契约（供 push-verify 等脚本消费）：
    [codex-chain] status=<ok|fixed|skip|warn|error> detail=<一行描述>
  退出码：0 = ok/fixed/skip/warn；1 = error。

  注意：修复只改 config.toml；运行中的 Codex 在启动时读取配置，
  修复后需要重启 Codex 客户端才会生效（-Alert 会提醒这一点）。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\check-codex-chain.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\check-codex-chain.ps1 -Alert
#>
param(
  [string]$HookPath = '',
  [string]$Ingress = '',
  [string]$ConfigPath = '',
  [string]$LogPath = '',
  [switch]$Alert
)

$ErrorActionPreference = 'Stop'

# 与桌面端安装目录一致（AppPaths）：%LOCALAPPDATA%\Programs\Agent-notify。
$appInstallDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\Agent-notify'
$homeDirectory = [Environment]::GetFolderPath('UserProfile')
if ([string]::IsNullOrWhiteSpace($HookPath)) {
  $HookPath = Join-Path $appInstallDir 'agentnotify-codex-hook.exe'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $Ingress = Join-Path $appInstallDir 'agentnotify-ingress.exe'
}
if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
  $ConfigPath = Join-Path $homeDirectory '.codex\config.toml'
}
if ([string]::IsNullOrWhiteSpace($LogPath)) {
  $LogPath = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'AgentNotify\logs\codex-chain-check.log'
}

$installerPath = Join-Path $PSScriptRoot 'install-codex-v2.ps1'

function Write-CheckLog {
  param([string]$Message)
  try {
    $directory = Split-Path -Parent $LogPath
    if ($directory -and -not (Test-Path -LiteralPath $directory)) {
      New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }
    $line = '{0} {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    [IO.File]::AppendAllText($LogPath, $line + "`r`n", (New-Object Text.UTF8Encoding($false)))
  } catch {
    # 日志写入失败不影响巡检结论。
  }
}

# 通过 Hook 直接提交提醒：不依赖运行中的 Codex，链刚修好也能发出去。
# 载荷走 stdin 文件重定向：Windows PowerShell 5.1 给原生程序传参会丢掉 JSON 里的引号（空载荷），
# 管道也不保证写入 Hook 的 stdin；只有显式重定向 stdin 才能稳定送达。
function Send-ChainAlert {
  param([string]$Title, [string]$Message)
  if (-not (Test-Path -LiteralPath $HookPath -PathType Leaf)) { return $false }
  $payloadPath = Join-Path ([IO.Path]::GetTempPath()) ('agent-notify-codex-alert-' + [guid]::NewGuid().ToString('N') + '.json')
  try {
    $payload = @{ 'last-assistant-message' = $Message; 'input-messages' = @($Title) } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($payloadPath, $payload, (New-Object Text.UTF8Encoding($false)))
    $process = Start-Process -FilePath $HookPath -ArgumentList @('codex', 'turn-ended') -RedirectStandardInput $payloadPath -PassThru -WindowStyle Hidden
    return [bool]$process.WaitForExit(20000)
  } catch {
    return $false
  } finally {
    if (Test-Path -LiteralPath $payloadPath) { Remove-Item -LiteralPath $payloadPath -Force -ErrorAction SilentlyContinue }
  }
}

$status = 'error'
$detail = '未知错误'
$installerOutput = ''
try {
  foreach ($required in @(
      @{ Path = $installerPath; Name = '修复脚本 install-codex-v2.ps1' },
      @{ Path = $HookPath; Name = 'Codex Hook' },
      @{ Path = $Ingress; Name = 'ingress' },
      @{ Path = $ConfigPath; Name = 'Codex 配置' })) {
    if (-not (Test-Path -LiteralPath $required.Path -PathType Leaf)) {
      throw ('缺少{0}：{1}' -f $required.Name, $required.Path)
    }
  }

  # install-codex-v2.ps1 幂等：链已正确时只报告「无需改动」，不会重写文件。
  $installerOutput = @(& $installerPath -HookPath $HookPath -Ingress $Ingress -ConfigPath $ConfigPath *>&1) |
    ForEach-Object { [string]$_ }
  $text = $installerOutput -join "`n"

  if ($text -match '已提升为 Hook 直连|已替换为 Hook|已接管为 Hook 直连|已写入配置') {
    $status = 'fixed'
    $detail = '链被重新包裹或损坏，已修复为 Hook 直连'
  } elseif ($text -match '保持原样') {
    $status = 'skip'
    $detail = 'notify 是自定义程序，未接管'
  } elseif ($text -match '不在最外层|警告') {
    $status = 'warn'
    $detail = '链状态异常，自动修复未完全生效，请人工检查'
  } else {
    $status = 'ok'
    $detail = 'Hook 直连正常，无需改动'
  }
} catch {
  $status = 'error'
  $detail = $_.Exception.Message
}

$resultLine = '[codex-chain] status={0} detail={1}' -f $status, $detail
Write-Output $resultLine
Write-CheckLog $resultLine
if ($status -ne 'ok' -and -not [string]::IsNullOrWhiteSpace($installerOutput)) {
  Write-CheckLog ('installer: ' + (($installerOutput -join ' | ').Substring(0, [Math]::Min(2000, ($installerOutput -join ' | ').Length))))
}

if ($Alert) {
  $alertTitle = ''
  $alertMessage = ''
  switch ($status) {
    'fixed' {
      $alertTitle = '通知链被电脑操控插件重新包裹，已自动修复（请重启 Codex）'
      $alertMessage = '检测到 Codex 通知链被电脑操控插件（CUA）重新包裹，已自动修复为 Hook 直连。请重启 Codex 客户端后生效。'
    }
    'warn' {
      $alertTitle = '通知链状态异常，请人工检查'
      $alertMessage = 'Codex 通知链状态异常（AgentNotify 不在最外层），自动修复未完全生效，请人工检查。'
    }
    'error' {
      $alertTitle = '通知链自动巡检失败，请人工检查'
      $alertMessage = 'Codex 通知链自动巡检失败：' + $detail
    }
    default { }
  }
  if (-not [string]::IsNullOrWhiteSpace($alertTitle)) {
    $sent = Send-ChainAlert -Title $alertTitle -Message $alertMessage
    Write-CheckLog ('alert sent={0}' -f $sent)
  }
}

if ($status -eq 'error') { exit 1 }
exit 0
