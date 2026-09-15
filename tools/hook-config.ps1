# Shared JSON Hook editing for install.ps1 and uninstall.ps1.

$script:AntigravityLauncherName = 'agent-notify-hook.cmd'
$script:AntigravityLauncherMarker = '@rem agent-notify-antigravity-launcher'

function Read-AgentNotifyJsonObject {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [switch]$AllowMissing
  )

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    if ($AllowMissing) {
      return [pscustomobject]@{}
    }
    throw "JSON 配置不存在：$Path"
  }

  $raw = [IO.File]::ReadAllText($Path)
  if ([string]::IsNullOrWhiteSpace($raw)) {
    if ($AllowMissing) {
      return [pscustomobject]@{}
    }
    throw "JSON 配置为空：$Path"
  }

  try {
    $value = $raw | ConvertFrom-Json -ErrorAction Stop
  } catch {
    throw "JSON 配置格式错误：$Path - $($_.Exception.Message)"
  }
  if ($null -eq $value -or $value -isnot [pscustomobject]) {
    throw "JSON 配置根节点必须是对象：$Path"
  }
  return $value
}

function Get-AgentNotifyJsonProperty {
  param(
    [Parameter(Mandatory = $true)]$Object,
    [Parameter(Mandatory = $true)][string]$Name
  )
  return $Object.PSObject.Properties[$Name]
}

function Set-AgentNotifyJsonProperty {
  param(
    [Parameter(Mandatory = $true)]$Object,
    [Parameter(Mandatory = $true)][string]$Name,
    $Value
  )
  $property = Get-AgentNotifyJsonProperty -Object $Object -Name $Name
  if ($null -eq $property) {
    Add-Member -InputObject $Object -MemberType NoteProperty -Name $Name -Value $Value
    return
  }
  $property.Value = $Value
}

function Remove-AgentNotifyJsonProperty {
  param(
    [Parameter(Mandatory = $true)]$Object,
    [Parameter(Mandatory = $true)][string]$Name
  )
  $Object.PSObject.Properties.Remove($Name)
}

function Write-AgentNotifyJsonFile {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)]$Value
  )

  $parent = Split-Path -Parent $Path
  if ([string]::IsNullOrWhiteSpace($parent)) {
    throw "JSON 配置缺少父目录：$Path"
  }
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
  $temporary = Join-Path $parent ((Split-Path -Leaf $Path) + '.new-' + [guid]::NewGuid().ToString('N'))
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
      Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
  }
}

function Get-AntigravityHookCommand {
  return '.\' + $script:AntigravityLauncherName + ' antigravity stop'
}

function Set-AntigravityAgentLauncher {
  param(
    [Parameter(Mandatory = $true)][string]$HooksPath,
    [Parameter(Mandatory = $true)][string]$Executable
  )

  $parent = Split-Path -Parent $HooksPath
  if ([string]::IsNullOrWhiteSpace($parent)) {
    throw "Antigravity Hook 路径缺少父目录：$HooksPath"
  }
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
  $launcher = Join-Path $parent $script:AntigravityLauncherName
  if (Test-Path -LiteralPath $launcher -PathType Leaf) {
    $existing = [IO.File]::ReadAllText($launcher)
    if (-not $existing.Contains($script:AntigravityLauncherMarker)) {
      throw "Antigravity 启动器路径已被其他文件占用：$launcher"
    }
  }

  $escapedExecutable = $Executable.Replace('%', '%%')
  $content = "@echo off`r`n$($script:AntigravityLauncherMarker)`r`n`"$escapedExecutable`" antigravity stop`r`n"
  [IO.File]::WriteAllText($launcher, $content, (New-Object Text.UTF8Encoding($false)))
  return $launcher
}

function Remove-AntigravityAgentLauncher {
  param([Parameter(Mandatory = $true)][string]$HooksPath)

  $parent = Split-Path -Parent $HooksPath
  if ([string]::IsNullOrWhiteSpace($parent)) {
    return $false
  }
  $launcher = Join-Path $parent $script:AntigravityLauncherName
  if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    return $false
  }
  $existing = [IO.File]::ReadAllText($launcher)
  if (-not $existing.Contains($script:AntigravityLauncherMarker)) {
    return $false
  }
  Remove-Item -LiteralPath $launcher -Force
  return $true
}

function Test-AgentNotifyHookCommand {
  param(
    [string]$Command,
    [Parameter(Mandatory = $true)][string]$Agent
  )
  if ([string]::IsNullOrWhiteSpace($Command)) {
    return $false
  }
  $normalized = $Command.Replace('\', '/')
  $binaryPattern = '(?i)(^|/)(?:agent-notify(?:\.exe)?|agent-notify-hook\.cmd)(?:"|\s)'
  $actionPattern = '(?i)\b' + [regex]::Escape($Agent) + '\s+stop\b'
  return ($normalized -match $binaryPattern) -and ($normalized -match $actionPattern)
}

function Get-FlatHookCommands {
  param($Handlers)
  foreach ($handler in @($Handlers)) {
    if ($null -eq $handler -or $handler -is [string]) {
      continue
    }
    $commandProperty = Get-AgentNotifyJsonProperty -Object $handler -Name 'command'
    if ($null -ne $commandProperty -and -not [string]::IsNullOrWhiteSpace([string]$commandProperty.Value)) {
      Write-Output ([string]$commandProperty.Value)
    }
  }
}

function Test-AntigravityAgentHook {
  param($HookGroup)
  if ($null -eq $HookGroup -or $HookGroup -is [string]) {
    return $false
  }
  $stopProperty = Get-AgentNotifyJsonProperty -Object $HookGroup -Name 'Stop'
  if ($null -eq $stopProperty) {
    return $false
  }
  foreach ($command in @(Get-FlatHookCommands -Handlers $stopProperty.Value)) {
    if (Test-AgentNotifyHookCommand -Command $command -Agent 'antigravity') {
      return $true
    }
  }
  return $false
}

function Set-AntigravityAgentHook {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Command
  )

  $root = Read-AgentNotifyJsonObject -Path $Path -AllowMissing
  Repair-AntigravityFlatHookEvents -Root $root
  $existing = Get-AgentNotifyJsonProperty -Object $root -Name 'agent-notify'
  if ($null -ne $existing -and -not (Test-AntigravityAgentHook -HookGroup $existing.Value)) {
    throw "Antigravity 顶层键 agent-notify 已被其他配置占用：$Path"
  }

  $group = [pscustomobject][ordered]@{
    Stop = @(
      [pscustomobject][ordered]@{
        type    = 'command'
        command = $Command
        timeout = 60
      }
    )
  }
  Set-AgentNotifyJsonProperty -Object $root -Name 'agent-notify' -Value $group
  Write-AgentNotifyJsonFile -Path $Path -Value $root
}

function Remove-AntigravityAgentHook {
  param([Parameter(Mandatory = $true)][string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return $false
  }
  $root = Read-AgentNotifyJsonObject -Path $Path
  Repair-AntigravityFlatHookEvents -Root $root
  $existing = Get-AgentNotifyJsonProperty -Object $root -Name 'agent-notify'
  if ($null -eq $existing -or -not (Test-AntigravityAgentHook -HookGroup $existing.Value)) {
    return $false
  }
  Remove-AgentNotifyJsonProperty -Object $root -Name 'agent-notify'
  Write-AgentNotifyJsonFile -Path $Path -Value $root
  return $true
}

function Test-DevinAgentHook {
  param($Hooks)
  if ($null -eq $Hooks -or $Hooks -is [string]) {
    return $false
  }
  $stopProperty = Get-AgentNotifyJsonProperty -Object $Hooks -Name 'Stop'
  if ($null -eq $stopProperty) {
    return $false
  }
  foreach ($group in @($stopProperty.Value)) {
    if ($null -eq $group -or $group -is [string]) {
      continue
    }
    $handlersProperty = Get-AgentNotifyJsonProperty -Object $group -Name 'hooks'
    if ($null -eq $handlersProperty) {
      continue
    }
    foreach ($command in @(Get-FlatHookCommands -Handlers $handlersProperty.Value)) {
      if (Test-AgentNotifyHookCommand -Command $command -Agent 'devin') {
        return $true
      }
    }
  }
  return $false
}

function Remove-AgentNotifyHandlersFromDevinGroups {
  param($Groups)

  foreach ($group in @($Groups)) {
    if ($null -eq $group -or $group -is [string]) {
      Write-Output $group
      continue
    }
    $handlersProperty = Get-AgentNotifyJsonProperty -Object $group -Name 'hooks'
    if ($null -eq $handlersProperty) {
      Write-Output $group
      continue
    }

    $remaining = @()
    $removed = $false
    foreach ($handler in @($handlersProperty.Value)) {
      $commandProperty = $null
      if ($null -ne $handler -and $handler -isnot [string]) {
        $commandProperty = Get-AgentNotifyJsonProperty -Object $handler -Name 'command'
      }
      $command = ''
      if ($null -ne $commandProperty) {
        $command = [string]$commandProperty.Value
      }
      if (Test-AgentNotifyHookCommand -Command $command -Agent 'devin') {
        $removed = $true
        continue
      }
      $remaining += $handler
    }

    if (-not $removed) {
      Write-Output $group
      continue
    }
    if ($remaining.Count -gt 0) {
      Set-AgentNotifyJsonProperty -Object $group -Name 'hooks' -Value @($remaining)
      Write-Output $group
    }
  }
}

function Set-DevinAgentHook {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Command
  )

  $root = Read-AgentNotifyJsonObject -Path $Path -AllowMissing
  $hooksProperty = Get-AgentNotifyJsonProperty -Object $root -Name 'hooks'
  $hooks = $null
  if ($null -eq $hooksProperty -or $null -eq $hooksProperty.Value) {
    $hooks = [pscustomobject]@{}
    Set-AgentNotifyJsonProperty -Object $root -Name 'hooks' -Value $hooks
  } else {
    $hooks = $hooksProperty.Value
    if ($hooks -isnot [pscustomobject]) {
      throw "Devin hooks 必须是对象：$Path"
    }
  }

  $stopProperty = Get-AgentNotifyJsonProperty -Object $hooks -Name 'Stop'
  $groups = @()
  if ($null -ne $stopProperty -and $null -ne $stopProperty.Value) {
    $groups = @($stopProperty.Value)
  }
  $remaining = @(Remove-AgentNotifyHandlersFromDevinGroups -Groups $groups)
  $remaining += [pscustomobject][ordered]@{
    matcher = ''
    hooks   = @(
      [pscustomobject][ordered]@{
        type    = 'command'
        command = $Command
        timeout = 60
      }
    )
  }
  Set-AgentNotifyJsonProperty -Object $hooks -Name 'Stop' -Value @($remaining)
  Write-AgentNotifyJsonFile -Path $Path -Value $root
}

function Remove-DevinAgentHook {
  param([Parameter(Mandatory = $true)][string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return $false
  }
  $root = Read-AgentNotifyJsonObject -Path $Path
  $hooksProperty = Get-AgentNotifyJsonProperty -Object $root -Name 'hooks'
  if ($null -eq $hooksProperty -or -not (Test-DevinAgentHook -Hooks $hooksProperty.Value)) {
    return $false
  }

  $stopProperty = Get-AgentNotifyJsonProperty -Object $hooksProperty.Value -Name 'Stop'
  $groups = @()
  if ($null -ne $stopProperty -and $null -ne $stopProperty.Value) {
    $groups = @($stopProperty.Value)
  }
  $remaining = @(Remove-AgentNotifyHandlersFromDevinGroups -Groups $groups)
  Set-AgentNotifyJsonProperty -Object $hooksProperty.Value -Name 'Stop' -Value @($remaining)
  Write-AgentNotifyJsonFile -Path $Path -Value $root
  return $true
}
function Repair-AntigravityFlatHookEvents {
  param([Parameter(Mandatory = $true)]$Root)

  # Antigravity flat events must be arrays. Older LinkWeixin configs wrote a
  # single object, which made hooks.json invalid. Normalize it in place.
  foreach ($groupProperty in @($Root.PSObject.Properties)) {
    $group = $groupProperty.Value
    if ($null -eq $group -or $group -is [string] -or $group -is [System.Array] -or $group -isnot [pscustomobject]) {
      continue
    }
    foreach ($eventName in @('Stop', 'PreInvocation', 'PostInvocation')) {
      $eventProperty = Get-AgentNotifyJsonProperty -Object $group -Name $eventName
      if ($null -eq $eventProperty -or $null -eq $eventProperty.Value -or $eventProperty.Value -is [System.Array]) {
        continue
      }
      Set-AgentNotifyJsonProperty -Object $group -Name $eventName -Value @($eventProperty.Value)
    }
  }
}
