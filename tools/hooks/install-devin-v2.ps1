#Requires -Version 5.1
<#
.SYNOPSIS
  把 Devin 的 hooks.Stop 接到 agentnotify-devin-hook.exe，并安装 V2 回复扩展。

.DESCRIPTION
  默认路径：
  - Hook：%USERPROFILE%\bin\agentnotify-devin-hook.exe
  - Devin 配置：%APPDATA%\devin\config.json
  - 回复扩展：%USERPROFILE%\.devin\extensions\agent-notify-reply-v2
  - 事件入口：%USERPROFILE%\bin\agentnotify-ingress.exe

  保护规则：
  - 只维护 hooks.Stop 中指向 AgentNotify 的 handler，其他事件、matcher 与 handler 原样保留。
  - 修改 config.json 前备份为 config.json.bak-agent-notify。
  - 回复扩展目录只接受 package.json 里 publisher/name 与 V2 包一致的扩展；
    被其他扩展占用时直接报错，且只替换本扩展的三个文件，不递归删除其他文件。
  - 失败一律抛错，不猜路径、不静默跳过。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-devin-v2.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-devin-v2.ps1 -HookPath D:\Temp\app\agentnotify-devin-hook.exe -ConfigPath D:\Temp\sandbox\devin\config.json -ExtensionDir D:\Temp\sandbox\extensions\agent-notify-reply-v2
#>
param(
  [string]$HookPath = '',
  [string]$ConfigPath = '',
  [string]$ExtensionDir = '',
  [string]$ExtensionSource = '',
  [string]$Ingress = ''
)

$ErrorActionPreference = 'Stop'

$StopTimeoutSeconds = 60
$IngressExeName = 'agentnotify-ingress.exe'
$BackupSuffix = '.bak-agent-notify'
# V2 扩展包名与发布者：升级时用它确认目录没有被其他扩展占用。
$ExtensionPublisher = 'agent-notify'
$ExtensionName = 'agent-notify-reply-v2'
$ExtensionFiles = @('package.json', 'extension.js', 'acp-bridge.js')

if ([string]::IsNullOrWhiteSpace($HookPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $HookPath = Join-Path $homeDirectory 'bin\agentnotify-devin-hook.exe'
}
if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
  $ConfigPath = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'devin\config.json'
}
if ([string]::IsNullOrWhiteSpace($ExtensionDir)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $ExtensionDir = Join-Path $homeDirectory '.devin\extensions\agent-notify-reply-v2'
}
if ([string]::IsNullOrWhiteSpace($ExtensionSource)) {
  $repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $ExtensionSource = Join-Path $repoRoot 'plugin\devin-extension-v2'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Ingress = Join-Path $homeDirectory "bin\$IngressExeName"
}

$hookPathFull = [IO.Path]::GetFullPath($HookPath)
$configPathFull = [IO.Path]::GetFullPath($ConfigPath)
$extensionDirFull = [IO.Path]::GetFullPath($ExtensionDir)
$extensionSourceFull = [IO.Path]::GetFullPath($ExtensionSource)
$ingressPathFull = [IO.Path]::GetFullPath($Ingress)

if (-not (Test-Path -LiteralPath $hookPathFull -PathType Leaf)) {
  throw "Devin Hook executable not found: $hookPathFull"
}
if ([IO.Path]::GetExtension($hookPathFull) -ine '.exe') {
  throw "Devin Hook must be an .exe file: $hookPathFull"
}
foreach ($name in $ExtensionFiles) {
  $sourceFile = Join-Path $extensionSourceFull $name
  if (-not (Test-Path -LiteralPath $sourceFile -PathType Leaf)) {
    throw "Devin V2 扩展源文件缺失：$sourceFile"
  }
}

function Write-TextAtomically {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Content
  )
  $directory = Split-Path -Parent $Path
  if ([string]::IsNullOrWhiteSpace($directory)) {
    throw "文件路径缺少父目录：$Path"
  }
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
  $temporary = Join-Path $directory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
  try {
    [IO.File]::WriteAllText($temporary, $Content, (New-Object Text.UTF8Encoding($false)))
    Move-Item -LiteralPath $temporary -Destination $Path -Force
  } finally {
    if (Test-Path -LiteralPath $temporary) {
      Remove-Item -LiteralPath $temporary -Force
    }
  }
}

function Write-JsonAtomically {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)]$Value
  )
  $directory = Split-Path -Parent $Path
  if ([string]::IsNullOrWhiteSpace($directory)) {
    throw "Devin 配置路径缺少父目录：$Path"
  }
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
  $temporary = Join-Path $directory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
  try {
    $json = $Value | ConvertTo-Json -Depth 32
    [IO.File]::WriteAllText($temporary, $json + [Environment]::NewLine, (New-Object Text.UTF8Encoding($false)))
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
      [IO.File]::Replace($temporary, $Path, [NullString]::Value, $true)
    } else {
      [IO.File]::Move($temporary, $Path)
    }
  } finally {
    if (Test-Path -LiteralPath $temporary) {
      Remove-Item -LiteralPath $temporary -Force
    }
  }
}

# 与 Go 版识别规则一致：命令里同时出现 AgentNotify 入口与 devin stop。
function Test-AgentNotifyStopCommand {
  param([string]$Command)
  if ([string]::IsNullOrWhiteSpace($Command)) {
    return $false
  }
  $normalized = $Command.Replace('\', '/')
  if ($normalized -notmatch '(?i)(agentnotify-devin-hook\.exe|agent-notify\.exe)') {
    return $false
  }
  return $normalized -match '(?i)devin\s+stop'
}

# 只摘掉 hooks.Stop 分组里指向 AgentNotify 的 handler；空分组不再输出。
function Get-RemainingStopGroups {
  param($Groups)
  $remainingGroups = @()
  foreach ($group in @($Groups)) {
    if ($null -eq $group -or $group -is [string]) {
      $remainingGroups += $group
      continue
    }
    $handlersProperty = $group.PSObject.Properties['hooks']
    if ($null -eq $handlersProperty) {
      $remainingGroups += $group
      continue
    }

    $remainingHandlers = @()
    $removed = $false
    foreach ($handler in @($handlersProperty.Value)) {
      $command = ''
      if ($null -ne $handler -and $handler -isnot [string]) {
        $commandProperty = $handler.PSObject.Properties['command']
        if ($null -ne $commandProperty) {
          $command = [string]$commandProperty.Value
        }
      }
      if (Test-AgentNotifyStopCommand -Command $command) {
        $removed = $true
        continue
      }
      $remainingHandlers += $handler
    }

    if ($removed -and $remainingHandlers.Count -eq 0) {
      continue
    }
    if ($removed) {
      $handlersProperty.Value = @($remainingHandlers)
    }
    $remainingGroups += $group
  }
  return @($remainingGroups)
}

function Assert-ExtensionPackage {
  param([Parameter(Mandatory = $true)][string]$Path)
  $raw = [IO.File]::ReadAllText($Path)
  if ([string]::IsNullOrWhiteSpace($raw)) {
    throw "扩展 package.json 为空：$Path"
  }
  try {
    $package = $raw | ConvertFrom-Json -ErrorAction Stop
  } catch {
    throw "扩展 package.json 格式错误：$Path - $($_.Exception.Message)"
  }
  if ($package.publisher -ne $ExtensionPublisher -or $package.name -ne $ExtensionName) {
    throw "扩展目录已被其他扩展占用（期望 $ExtensionPublisher/$ExtensionName）：$Path"
  }
}

function Install-ExtensionFile {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Destination
  )
  $directory = Split-Path -Parent $Destination
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
  $temporary = Join-Path $directory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
  try {
    Copy-Item -LiteralPath $Source -Destination $temporary -Force
    if (Test-Path -LiteralPath $Destination -PathType Leaf) {
      [IO.File]::Replace($temporary, $Destination, [NullString]::Value, $true)
    } else {
      [IO.File]::Move($temporary, $Destination)
    }
  } finally {
    if (Test-Path -LiteralPath $temporary) {
      Remove-Item -LiteralPath $temporary -Force
    }
  }
}

# 先完整校验配置与扩展，任何拒绝都不能留下半成品（配置文件也不会被改写）。
$root = [pscustomobject]@{}
$configExisted = Test-Path -LiteralPath $configPathFull -PathType Leaf
if ($configExisted) {
  $raw = [IO.File]::ReadAllText($configPathFull)
  if (-not [string]::IsNullOrWhiteSpace($raw)) {
    try {
      $root = $raw | ConvertFrom-Json -ErrorAction Stop
    } catch {
      throw "Devin 配置格式错误：$configPathFull - $($_.Exception.Message)"
    }
    if ($null -eq $root -or $root -isnot [pscustomobject]) {
      throw "Devin 配置根节点必须是对象：$configPathFull"
    }
  }
}

$hooksProperty = $root.PSObject.Properties['hooks']
$hooks = $null
if ($null -eq $hooksProperty -or $null -eq $hooksProperty.Value) {
  $hooks = [pscustomobject]@{}
} else {
  $hooks = $hooksProperty.Value
  if ($hooks -isnot [pscustomobject]) {
    throw "Devin hooks 必须是对象：$configPathFull"
  }
}

$stopProperty = $hooks.PSObject.Properties['Stop']
$existingGroups = @()
if ($null -ne $stopProperty -and $null -ne $stopProperty.Value) {
  $existingGroups = @($stopProperty.Value)
}

$canonicalHandler = [pscustomobject][ordered]@{
  type    = 'command'
  command = '"' + $hookPathFull + '" devin stop'
  timeout = $StopTimeoutSeconds
}
$canonicalGroup = [pscustomobject][ordered]@{
  matcher = ''
  hooks   = @($canonicalHandler)
}

$remainingGroups = @(Get-RemainingStopGroups -Groups $existingGroups)
$nextGroups = @($remainingGroups + $canonicalGroup)
$existingJson = $existingGroups | ConvertTo-Json -Depth 32
$nextJson = $nextGroups | ConvertTo-Json -Depth 32

$configState = 'unchanged'
if ($existingJson -ne $nextJson) {
  if ($null -eq $hooksProperty) {
    Add-Member -InputObject $root -MemberType NoteProperty -Name 'hooks' -Value $hooks
  }
  if ($null -ne $stopProperty) {
    $stopProperty.Value = @($nextGroups)
  } else {
    Add-Member -InputObject $hooks -MemberType NoteProperty -Name 'Stop' -Value @($nextGroups)
  }
  if ($configExisted) {
    Copy-Item -LiteralPath $configPathFull -Destination "$configPathFull$BackupSuffix" -Force
  }
  Write-JsonAtomically -Path $configPathFull -Value $root
  $configState = 'updated'
}

# 扩展安装：先确认目标是本扩展，再逐个文件原子替换。
if (Test-Path -LiteralPath $extensionDirFull -PathType Container) {
  $existingPackage = Join-Path $extensionDirFull 'package.json'
  if (Test-Path -LiteralPath $existingPackage -PathType Leaf) {
    Assert-ExtensionPackage -Path $existingPackage
  }
}
Assert-ExtensionPackage -Path (Join-Path $extensionSourceFull 'package.json')
foreach ($name in $ExtensionFiles) {
  Install-ExtensionFile -Source (Join-Path $extensionSourceFull $name) -Destination (Join-Path $extensionDirFull $name)
}

# 汇总本次改动，保持输出可核对。
Write-Output "[hook] Devin Hook：$hookPathFull"
switch ($configState) {
  'updated' {
    if ($configExisted) {
      Write-Output "[hook] 已维护 hooks.Stop 中的 AgentNotify handler（备份：$configPathFull$BackupSuffix）"
    } else {
      Write-Output "[hook] 已写入 hooks.Stop 中的 AgentNotify handler：$configPathFull"
    }
    Write-Output "[hook] 其他事件、matcher 与 handler 保持原样。"
  }
  default { Write-Output "[hook] hooks.Stop 中的 AgentNotify handler 已是最新，无需改动。" }
}
Write-Output "[hook] 已安装 Devin V2 回复扩展：$extensionDirFull（$ExtensionPublisher/$ExtensionName）"

# Hook 运行时按 env → 同目录 → 正式安装目录查找 ingress，这里提前提醒不可达的情况。
$hookDirectory = Split-Path -Parent $hookPathFull
$ingressCandidates = @(
  (Join-Path $hookDirectory $IngressExeName),
  $ingressPathFull,
  (Join-Path (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\Agent-notify') $IngressExeName)
)
$ingressReachable = $false
foreach ($candidate in $ingressCandidates) {
  if (Test-Path -LiteralPath $candidate -PathType Leaf) { $ingressReachable = $true; break }
}
if (-not $ingressReachable) {
  Write-Output "[hook] 警告：Hook 同目录与正式安装目录都没有 $IngressExeName，事件无法提交；请把 Hook 放到 ingress 同目录，或设置 AGENT_NOTIFY_INGRESS。"
}
