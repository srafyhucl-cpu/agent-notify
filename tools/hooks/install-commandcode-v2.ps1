#Requires -Version 5.1
<#
.SYNOPSIS
  安装 Command Code V2 mod：把事件入口写进 mod，原子部署到用户级 mods 目录。

.DESCRIPTION
  默认路径：
  - mod 源文件：<仓库>\plugin\commandcode-v2\agent-notify.ts
  - mod 目标：%USERPROFILE%\.commandcode\mods\agent-notify.ts
  - 事件入口：%USERPROFILE%\bin\agentnotify-ingress.exe

  保护规则：
  - 目标文件不含 AgentNotify 归属标识（agent-notify-commandcode-mod）时拒绝覆盖，绝不动其他 mod。
  - 覆盖前备份为 agent-notify.ts.bak-agent-notify。
  - 只写这一个文件；不读写 Command Code 的 config.json、auth.json 或会话数据。
  - 目标内容已是最新时不写盘（幂等）。
  - 失败一律抛错，不猜路径、不静默跳过。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-commandcode-v2.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-commandcode-v2.ps1 -ModPath D:\Temp\sandbox\mods\agent-notify.ts -Ingress D:\Temp\app\agentnotify-ingress.exe
#>
param(
  [string]$ModPath = '',
  [string]$Source = '',
  [string]$Ingress = ''
)

$ErrorActionPreference = 'Stop'

# 归属标识：V1 与 V2 mod 共用，升级时据此确认文件由 AgentNotify 创建。
$ModMarker = 'agent-notify-commandcode-mod'
# 安装器把事件入口绝对路径写进这个占位符；占位符缺失说明 mod 源文件已改版。
$BakedIngressPlaceholder = 'const BAKED_INGRESS = ""'
$BackupSuffix = '.bak-agent-notify'
$IngressExeName = 'agentnotify-ingress.exe'

# 回复窗口报告（只读）：实际生效值取应用写入的 window.json（界面设置），旧 JSON 配置只作回退；
# mod 侧完整优先级为 环境变量 > window.json > 旧配置，未设置时窗口关闭。
$ReplyInboxDirName = 'commandcode-reply-inbox'
$ReplyWindowFileName = 'window.json'
$ReplyWindowConfigKey = 'commandCodeReplyWindowSec'
$MaxReplyWindowSeconds = 600
$ReplyWindowPriorityText = '[hook] 回复窗口取值优先级：环境变量 AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC > 界面设置（window.json）> 旧配置 config.json。'

# 与桌面端 AppPaths 一致：AGENT_NOTIFY_CONFIG_DIR 覆盖，否则 %USERPROFILE%\.config\agent-notify。
$userProfileDirectory = [Environment]::GetFolderPath('UserProfile')
if ([string]::IsNullOrWhiteSpace($userProfileDirectory)) {
  $userProfileDirectory = $env:USERPROFILE
}
if ([string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_CONFIG_DIR)) {
  $configDirectory = if ([string]::IsNullOrWhiteSpace($userProfileDirectory)) {
    ''
  } else {
    Join-Path (Join-Path $userProfileDirectory '.config') 'agent-notify'
  }
} else {
  $configDirectory = $env:AGENT_NOTIFY_CONFIG_DIR
}

# 读取 JSON 文件：缺失返回 Exists=$false；读取/解析失败返回 Error 文本，由调用方回退，绝不抛错。
function Read-JsonDocument {
  param([string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return @{ Exists = $false; Value = $null; Error = $null }
  }
  try {
    $raw = [IO.File]::ReadAllText($Path)
  } catch {
    return @{ Exists = $true; Value = $null; Error = "读取失败：$($_.Exception.Message)" }
  }
  if ([string]::IsNullOrWhiteSpace($raw)) {
    return @{ Exists = $true; Value = $null; Error = '文件为空' }
  }
  try {
    return @{ Exists = $true; Value = ($raw | ConvertFrom-Json); Error = $null }
  } catch {
    return @{ Exists = $true; Value = $null; Error = 'JSON 语法非法' }
  }
}

# 窗口秒数：0 表示关闭；负数、非数字返回 $null（回退下一优先级）；超过上限收敛到 600（与 mod 一致）。
# window.json 只接受 JSON 数字；旧配置沿用 mod 的宽容解析，允许数字字符串（-AllowNumericText）。
function ConvertTo-ReplyWindowSeconds {
  param($Value, [switch]$AllowNumericText)

  if ($null -eq $Value -or $Value -is [bool]) {
    return $null
  }
  if ($Value -is [string]) {
    if (-not $AllowNumericText) {
      return $null
    }
  } elseif ($Value -isnot [ValueType]) {
    return $null
  }
  $parsed = 0.0
  if (-not [double]::TryParse([string]$Value, [ref]$parsed)) {
    return $null
  }
  if ($parsed -lt 0) {
    return $null
  }
  return [int][Math]::Min([Math]::Floor($parsed), $MaxReplyWindowSeconds)
}

# 报告实际生效的回复窗口；任何读取失败只打印可读说明，不影响安装结果。
function Write-ReplyWindowReport {
  if ([string]::IsNullOrWhiteSpace($configDirectory)) {
    Write-Output '[hook] 无法确定 AgentNotify 配置目录（缺少 USERPROFILE），跳过读取回复窗口设置。'
    return
  }

  $windowFile = Join-Path (Join-Path $configDirectory $ReplyInboxDirName) $ReplyWindowFileName
  $windowRead = Read-JsonDocument -Path $windowFile
  if ($windowRead.Exists -and $null -ne $windowRead.Error) {
    Write-Output "[hook] 界面设置文件无法读取（$($windowRead.Error)），已回退旧配置：$windowFile"
  } elseif ($windowRead.Exists) {
    $windowSeconds = ConvertTo-ReplyWindowSeconds ($windowRead.Value.$ReplyWindowConfigKey)
    if ($null -ne $windowSeconds) {
      if ($windowSeconds -eq 0) {
        Write-Output '[hook] 回复窗口当前生效值：关闭（界面设置为 0 秒）；需要引用回复时在界面里设为 1–600 秒（保存后下次任务生效，无需重启）。'
      } else {
        Write-Output "[hook] 回复窗口当前生效值：$windowSeconds 秒（来自界面设置）；改完界面值后下次任务生效（无需重启）。"
      }
      Write-Output $ReplyWindowPriorityText
      return
    }
    Write-Output "[hook] 界面设置里的 $ReplyWindowConfigKey 不是有效秒数，已回退旧配置：$windowFile"
  }

  $configFile = Join-Path $configDirectory 'config.json'
  $configRead = Read-JsonDocument -Path $configFile
  if ($configRead.Exists -and $null -ne $configRead.Error) {
    Write-Output "[hook] 旧配置无法读取（$($configRead.Error)），按未设置处理：$configFile"
  } elseif ($configRead.Exists) {
    $windowSeconds = ConvertTo-ReplyWindowSeconds ($configRead.Value.$ReplyWindowConfigKey) -AllowNumericText
    if ($null -ne $windowSeconds -and $windowSeconds -gt 0) {
      Write-Output "[hook] 回复窗口当前生效值：$windowSeconds 秒（来自旧配置 config.json）；推荐改用界面设置，界面值优先。"
      Write-Output $ReplyWindowPriorityText
      return
    }
    if ($null -ne $windowSeconds) {
      Write-Output '[hook] 回复窗口当前生效值：关闭（旧配置 commandCodeReplyWindowSec = 0）。'
      Write-Output $ReplyWindowPriorityText
      return
    }
    Write-Output "[hook] 旧配置里的 $ReplyWindowConfigKey 不是有效秒数，按未设置处理：$configFile"
  }

  Write-Output '[hook] 回复窗口当前生效值：未设置（回复窗口关闭）；需要引用回复时在界面里设为 1–600 秒（保存后下次任务生效，无需重启）。'
  Write-Output $ReplyWindowPriorityText
}

if ([string]::IsNullOrWhiteSpace($ModPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $ModPath = Join-Path $homeDirectory '.commandcode\mods\agent-notify.ts'
}
if ([string]::IsNullOrWhiteSpace($Source)) {
  $repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $Source = Join-Path $repoRoot 'plugin\commandcode-v2\agent-notify.ts'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Ingress = Join-Path $homeDirectory "bin\$IngressExeName"
}

$modPathFull = [IO.Path]::GetFullPath($ModPath)
$sourcePathFull = [IO.Path]::GetFullPath($Source)
$ingressPathFull = [IO.Path]::GetFullPath($Ingress)

if (-not (Test-Path -LiteralPath $sourcePathFull -PathType Leaf)) {
  throw "Command Code mod source not found: $sourcePathFull"
}
if (-not (Test-Path -LiteralPath $ingressPathFull -PathType Leaf)) {
  throw "Ingress executable not found: $ingressPathFull"
}
if ([IO.Path]::GetExtension($ingressPathFull) -ine '.exe') {
  throw "Ingress executable must be an .exe file: $ingressPathFull"
}

$sourceContent = [IO.File]::ReadAllText($sourcePathFull)
if (-not $sourceContent.Contains($BakedIngressPlaceholder)) {
  throw "Command Code mod 缺少 BAKED_INGRESS 占位符，安装器不会猜测路径：$sourcePathFull"
}
$escapedIngress = $ingressPathFull.Replace('\', '\\').Replace('"', '\"')
$content = $sourceContent.Replace($BakedIngressPlaceholder, "const BAKED_INGRESS = `"$escapedIngress`"")

if (Test-Path -LiteralPath $modPathFull -PathType Container) {
  throw "Command Code mod 目标路径已被目录占用：$modPathFull"
}

$existed = Test-Path -LiteralPath $modPathFull -PathType Leaf
if ($existed) {
  $existing = [IO.File]::ReadAllText($modPathFull)
  if (-not $existing.Contains($ModMarker)) {
    throw "Command Code mods 目录里的 agent-notify.ts 不是 AgentNotify 部署的 mod（缺少归属标识 $ModMarker），已拒绝覆盖：$modPathFull"
  }
  if ($existing -eq $content) {
    Write-Output "[hook] Command Code V2 mod 已是最新，无需改动：$modPathFull"
    Write-Output "[hook] 事件入口：$ingressPathFull"
    Write-ReplyWindowReport
    exit 0
  }
}

$modDirectory = Split-Path -Parent $modPathFull
if ([string]::IsNullOrWhiteSpace($modDirectory)) {
  throw "Command Code mod 路径缺少父目录：$modPathFull"
}
New-Item -ItemType Directory -Force -Path $modDirectory | Out-Null

if ($existed) {
  Copy-Item -LiteralPath $modPathFull -Destination "$modPathFull$BackupSuffix" -Force
}

# 原子替换：先写同目录临时文件，再 Move-Item -Force，避免半成品 mod 被 Command Code 加载。
$temporary = Join-Path $modDirectory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
try {
  [IO.File]::WriteAllText($temporary, $content, (New-Object Text.UTF8Encoding($false)))
  Move-Item -LiteralPath $temporary -Destination $modPathFull -Force
} finally {
  if (Test-Path -LiteralPath $temporary) {
    Remove-Item -LiteralPath $temporary -Force
  }
}

if ($existed) {
  Write-Output "[hook] 已更新 Command Code V2 mod：$modPathFull（备份：$modPathFull$BackupSuffix）"
} else {
  Write-Output "[hook] 已安装 Command Code V2 mod：$modPathFull"
}
Write-Output "[hook] 事件入口：$ingressPathFull"
Write-ReplyWindowReport
