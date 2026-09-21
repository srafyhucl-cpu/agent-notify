#Requires -Version 5.1
<#
.SYNOPSIS
  把 Antigravity 的顶层 agent-notify Stop Hook 接到 agentnotify-antigravity-hook.exe。

.DESCRIPTION
  默认路径：
  - Hook：%USERPROFILE%\bin\agentnotify-antigravity-hook.exe
  - Antigravity 配置：%USERPROFILE%\.gemini\config\hooks.json
  - 同目录启动器：%USERPROFILE%\.gemini\config\agent-notify-hook.cmd
  - 事件入口：%USERPROFILE%\bin\agentnotify-ingress.exe

  保护规则：
  - 只维护顶层 agent-notify 键与它自己的启动器，其他顶层键、Hook 与权限保持原样。
  - 顶层 agent-notify 已被其他配置占用时直接报错，不做任何改写。
  - 修改 hooks.json 前备份为 hooks.json.bak-agent-notify。
  - 启动器缺少 AgentNotify 标记时拒绝覆盖，避免破坏用户自己的文件。
  - 失败一律抛错，不猜路径、不静默跳过。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-antigravity-v2.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-antigravity-v2.ps1 -HooksPath D:\Temp\sandbox\hooks.json -HookPath D:\Temp\app\agentnotify-antigravity-hook.exe
#>
param(
  [string]$HookPath = '',
  [string]$HooksPath = '',
  [string]$LauncherPath = '',
  [string]$Ingress = ''
)

$ErrorActionPreference = 'Stop'

# 与 Go 版安装器一致的顶层键、启动器名称与标记；旧启动器也带这个标记。
$TopLevelKey = 'agent-notify'
$LauncherName = 'agent-notify-hook.cmd'
$LauncherMarker = '@rem agent-notify-antigravity-launcher'
$HookCommand = '.\agent-notify-hook.cmd antigravity stop'
$StopTimeoutSeconds = 60
$IngressExeName = 'agentnotify-ingress.exe'
$BackupSuffix = '.bak-agent-notify'

if ([string]::IsNullOrWhiteSpace($HookPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $HookPath = Join-Path $homeDirectory 'bin\agentnotify-antigravity-hook.exe'
}
if ([string]::IsNullOrWhiteSpace($HooksPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $HooksPath = Join-Path $homeDirectory '.gemini\config\hooks.json'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Ingress = Join-Path $homeDirectory "bin\$IngressExeName"
}

$hookPathFull = [IO.Path]::GetFullPath($HookPath)
$hooksPathFull = [IO.Path]::GetFullPath($HooksPath)
$ingressPathFull = [IO.Path]::GetFullPath($Ingress)
if ([string]::IsNullOrWhiteSpace($LauncherPath)) {
  $LauncherPath = Join-Path (Split-Path -Parent $hooksPathFull) $LauncherName
}
$launcherPathFull = [IO.Path]::GetFullPath($LauncherPath)

if (-not (Test-Path -LiteralPath $hookPathFull -PathType Leaf)) {
  throw "Antigravity Hook executable not found: $hookPathFull"
}
if ([IO.Path]::GetExtension($hookPathFull) -ine '.exe') {
  throw "Antigravity Hook must be an .exe file: $hookPathFull"
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
    throw "Antigravity hooks 路径缺少父目录：$Path"
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

# 与 Go 版识别规则一致：命令里同时出现 AgentNotify 入口与 antigravity stop。
function Test-AgentNotifyStopCommand {
  param([string]$Command)
  if ([string]::IsNullOrWhiteSpace($Command)) {
    return $false
  }
  $normalized = $Command.Replace('\', '/')
  if ($normalized -notmatch '(?i)(agentnotify-antigravity-hook\.exe|agent-notify\.exe|agent-notify-hook\.cmd)') {
    return $false
  }
  return $normalized -match '(?i)antigravity\s+stop'
}

function Get-StopHandlers {
  param($Group)
  if ($null -eq $Group -or $Group -is [string]) {
    return @()
  }
  $stopProperty = $Group.PSObject.Properties['Stop']
  if ($null -eq $stopProperty -or $null -eq $stopProperty.Value) {
    return @()
  }
  return @($stopProperty.Value)
}

function Test-AgentNotifyHookGroup {
  param($Group)
  foreach ($handler in @(Get-StopHandlers -Group $Group)) {
    if ($null -eq $handler -or $handler -is [string]) {
      continue
    }
    $commandProperty = $handler.PSObject.Properties['command']
    if ($null -ne $commandProperty -and (Test-AgentNotifyStopCommand -Command ([string]$commandProperty.Value))) {
      return $true
    }
  }
  return $false
}

# 启动器只允许写向本 Hook；已有其他内容时拒绝覆盖。
function Write-Launcher {
  param([Parameter(Mandatory = $true)][string]$Path)
  $escapedHookPath = $hookPathFull.Replace('%', '%%')
  $content = "@echo off`r`n$LauncherMarker`r`n`"$escapedHookPath`" antigravity stop`r`n"
  if (Test-Path -LiteralPath $Path -PathType Leaf) {
    $existing = [IO.File]::ReadAllText($Path)
    if (-not $existing.Contains($LauncherMarker)) {
      throw "Antigravity 启动器路径已被其他文件占用：$Path"
    }
    if ($existing -eq $content) {
      return 'unchanged'
    }
    Write-TextAtomically -Path $Path -Content $content
    return 'updated'
  }
  Write-TextAtomically -Path $Path -Content $content
  return 'created'
}

# 先完整校验 hooks.json，任何拒绝都不能留下半成品（启动器也不会被改写）。
$root = $null
$hooksExisted = Test-Path -LiteralPath $hooksPathFull -PathType Leaf
if ($hooksExisted) {
  $raw = [IO.File]::ReadAllText($hooksPathFull)
  if (-not [string]::IsNullOrWhiteSpace($raw)) {
    try {
      $root = $raw | ConvertFrom-Json -ErrorAction Stop
    } catch {
      throw "Antigravity hooks 配置格式错误：$hooksPathFull - $($_.Exception.Message)"
    }
    if ($null -eq $root -or $root -isnot [pscustomobject]) {
      throw "Antigravity hooks 配置根节点必须是对象：$hooksPathFull"
    }
  } else {
    $root = [pscustomobject]@{}
  }
} else {
  $root = [pscustomobject]@{}
}

$existingGroup = $null
$existingProperty = $root.PSObject.Properties[$TopLevelKey]
if ($null -ne $existingProperty) {
  $existingGroup = $existingProperty.Value
  if (-not (Test-AgentNotifyHookGroup -Group $existingGroup)) {
    throw "Antigravity 顶层键 $TopLevelKey 已被其他配置占用，安装器不会改写：$hooksPathFull"
  }
}

$launcherState = Write-Launcher -Path $launcherPathFull

$canonicalGroup = [pscustomobject][ordered]@{
  Stop = @(
    [pscustomobject][ordered]@{
      type    = 'command'
      command = $HookCommand
      timeout = $StopTimeoutSeconds
    }
  )
}

$hooksState = 'unchanged'
$existingJson = if ($null -ne $existingGroup) { $existingGroup | ConvertTo-Json -Depth 32 } else { '' }
$canonicalJson = $canonicalGroup | ConvertTo-Json -Depth 32
if ($existingJson -ne $canonicalJson) {
  if ($null -ne $existingProperty) {
    $existingProperty.Value = $canonicalGroup
  } else {
    Add-Member -InputObject $root -MemberType NoteProperty -Name $TopLevelKey -Value $canonicalGroup
  }
  if ($hooksExisted) {
    Copy-Item -LiteralPath $hooksPathFull -Destination "$hooksPathFull$BackupSuffix" -Force
  }
  Write-JsonAtomically -Path $hooksPathFull -Value $root
  $hooksState = 'updated'
}

# 汇总本次改动，保持输出可核对。
Write-Output "[hook] Antigravity Hook：$hookPathFull"
switch ($launcherState) {
  'created' { Write-Output "[hook] 已创建启动器：$launcherPathFull（调用 $HookCommand）" }
  'updated' { Write-Output "[hook] 已更新启动器：$launcherPathFull（调用 $HookCommand）" }
  default { Write-Output "[hook] 启动器已指向本 Hook，无需改动：$launcherPathFull" }
}
switch ($hooksState) {
  'updated' {
    if ($hooksExisted) {
      Write-Output "[hook] 已维护顶层 $TopLevelKey Stop Hook（备份：$hooksPathFull$BackupSuffix）"
    } else {
      Write-Output "[hook] 已写入顶层 $TopLevelKey Stop Hook：$hooksPathFull"
    }
    Write-Output "[hook] 其他顶层键与 Hook 保持原样。"
  }
  default { Write-Output "[hook] 顶层 $TopLevelKey Stop Hook 已是本 Hook，无需改动。" }
}

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
