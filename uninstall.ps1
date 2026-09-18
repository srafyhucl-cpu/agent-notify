#Requires -Version 5.1
<#
.SYNOPSIS
  Agent-notify 卸载脚本：按安装记录删除运行程序与 opencode 插件，还原 Codex notify，清理快捷方式。
  同时只移除 Agent-notify 自己的 Antigravity 与 Devin Stop Hook。
.DESCRIPTION
  只清理 Agent-notify 自己安装的文件。登录凭据与配置（
  %USERPROFILE%\.config\agent-notify\）属于用户数据，默认保留。
  Devin 回复扩展仅在其 package.json 归属校验通过后删除。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
  # 沙箱/测试（不碰快捷方式与真实 Codex 配置）：
  powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1 -InstallDir D:\tmp\bin -PluginDir D:\tmp\plugins -SkipShortcuts -SkipCodexConfig
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugins'),
  [string]$DevinExtensionDir = (Join-Path $env:USERPROFILE '.devin\extensions\agent-notify'),
  [string]$CommandCodeModDir = (Join-Path $env:USERPROFILE '.commandcode\mods'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$AntigravityHooks = (Join-Path $env:USERPROFILE '.gemini\config\hooks.json'),
  [string]$DevinConfig = (Join-Path $env:APPDATA 'devin\config.json'),
  [switch]$SkipShortcuts,
  [switch]$SkipAntigravityConfig,
  [switch]$SkipDevinConfig,
  [switch]$SkipDevinExtension,
  [switch]$SkipCommandCodeMod,
  [switch]$SkipCodexConfig,
  [switch]$SkipProcessStop
)

$ErrorActionPreference = 'Continue'
$ExeName = 'agent-notify.exe'
$PluginName = 'agent-notify.ts'
$RecordName = 'agent-notify-install.json'

$hookConfigModule = Join-Path $PSScriptRoot 'tools\hook-config.ps1'
if (-not (Test-Path -LiteralPath $hookConfigModule -PathType Leaf)) {
  Write-Output "[uninstall] Hook 配置模块不存在，跳过 Hook 清理：$hookConfigModule"
} else {
  . $hookConfigModule
}

function Test-InsideDir {
  param([string]$Path, [string]$Root)
  try {
    $full = [IO.Path]::GetFullPath($Path)
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $full.StartsWith($rootFull + '\', [StringComparison]::OrdinalIgnoreCase)
  } catch { return $false }
}

# 0. 停掉安装目录里正在运行的悬浮窗，释放文件锁
if (-not $SkipProcessStop) {
  try {
    $escaped = [regex]::Escape([IO.Path]::GetFullPath($InstallDir).TrimEnd('\')) + '\\'
    Get-CimInstance Win32_Process -Filter "Name='agent-notify.exe'" -ErrorAction SilentlyContinue |
      Where-Object { $_.CommandLine -and ($_.CommandLine -match $escaped) -and ($_.ProcessId -ne $PID) } |
      ForEach-Object {
        Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        Write-Output "[uninstall] 已停止悬浮窗进程 $($_.ProcessId)"
      }
    Start-Sleep -Milliseconds 200
  } catch {
    Write-Output "[uninstall] 进程清理跳过：$($_.Exception.Message)"
  }
}

# 1. 按安装记录删除运行程序
$recordPath = Join-Path $InstallDir $RecordName
$removed = 0
if (Test-Path $recordPath) {
  try {
    $record = Get-Content $recordPath -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($rel in @($record.files)) {
      if ([string]::IsNullOrWhiteSpace($rel)) { continue }
      $full = Join-Path $InstallDir $rel
      if (-not (Test-InsideDir $full $InstallDir)) {
        Write-Output "[uninstall] 跳过越界路径：$rel"
        continue
      }
      if (Test-Path $full) {
        Remove-Item $full -Force
        $removed++
      }
    }
    Write-Output "[uninstall] 按安装记录清理 $removed 个文件（版本 $($record.version)）。"
  } catch {
    Write-Output "[uninstall] 安装记录解析失败：$($_.Exception.Message)"
  }
} else {
  $fallback = Join-Path $InstallDir $ExeName
  if (Test-Path $fallback) {
    Remove-Item $fallback -Force
    $removed++
  }
  Write-Output "[uninstall] 无安装记录，按默认文件名清理 $removed 个文件。"
}

if (Test-Path $recordPath) {
  Remove-Item $recordPath -Force
  Write-Output '[uninstall] 已删除安装记录。'
}

# 2. 删除 opencode 插件
$pluginPath = Join-Path $PluginDir $PluginName
if (Test-Path $pluginPath) {
  Remove-Item $pluginPath -Force
  Write-Output "[uninstall] 已删除插件：$pluginPath"
} else {
  Write-Output "[uninstall] 插件不存在，跳过：$pluginPath"
}

# 2.5 只删除归属校验通过的 Command Code mod，避免误删用户 mods 目录里的同名文件。
if (-not $SkipCommandCodeMod) {
  $modPath = Join-Path $CommandCodeModDir $PluginName
  if (Test-Path -LiteralPath $modPath -PathType Leaf) {
    try {
      $modText = [IO.File]::ReadAllText($modPath)
      if ($modText -notmatch 'agent-notify-commandcode-mod') {
        throw 'mod 不属于 Agent-notify'
      }
      Remove-Item -LiteralPath $modPath -Force
      Write-Output "[uninstall] 已删除 Command Code mod：$modPath"
    } catch {
      Write-Output "[uninstall] Command Code mod 归属校验失败，保持原样：$($_.Exception.Message)"
    }
  } else {
    Write-Output "[uninstall] Command Code mod 不存在，跳过：$modPath"
  }
}

# 3. 只删除经过 package.json 归属校验的 Devin 回复扩展。
if (-not $SkipDevinExtension) {
  $extensionPackage = Join-Path $DevinExtensionDir 'package.json'
  $extensionMain = Join-Path $DevinExtensionDir 'extension.js'
  $extensionACPBridge = Join-Path $DevinExtensionDir 'acp-bridge.js'
  if (Test-Path -LiteralPath $extensionPackage -PathType Leaf) {
    try {
      $manifest = Get-Content -LiteralPath $extensionPackage -Raw -Encoding utf8 | ConvertFrom-Json
      if ($manifest.name -ne 'agent-notify-reply' -or $manifest.publisher -ne 'agent-notify') {
        throw 'package.json 不属于 Agent-notify'
      }
      foreach ($file in @($extensionPackage, $extensionMain, $extensionACPBridge)) {
        if (Test-Path -LiteralPath $file -PathType Leaf) {
          Remove-Item -LiteralPath $file -Force
        }
      }
      Write-Output "[uninstall] 已删除 Devin 回复扩展：$DevinExtensionDir"
      if (Test-Path -LiteralPath $DevinExtensionDir -PathType Container) {
        try {
          [IO.Directory]::Delete($DevinExtensionDir, $false)
        } catch {
          Write-Output "[uninstall] Devin 扩展目录仍含其他文件，已保留：$DevinExtensionDir"
        }
      }
    } catch {
      Write-Output "[uninstall] Devin 扩展归属校验失败，保持原样：$($_.Exception.Message)"
    }
  } elseif (Test-Path -LiteralPath $extensionMain -PathType Leaf) {
    Write-Output '[uninstall] Devin 扩展缺少可校验的 package.json，保持原样。'
  } else {
    Write-Output "[uninstall] Devin 回复扩展不存在，跳过：$DevinExtensionDir"
  }
}

# 4. 移除 Agent-notify 自己的 Antigravity / Devin Stop Hook
if (Get-Command Remove-AntigravityAgentHook -ErrorAction SilentlyContinue) {
  if (-not $SkipAntigravityConfig -and (Test-Path -LiteralPath $AntigravityHooks -PathType Leaf)) {
    try {
      if (Remove-AntigravityAgentHook -Path $AntigravityHooks) {
        Write-Output "[uninstall] 已移除 Antigravity Hook：$AntigravityHooks"
      } else {
        Write-Output "[uninstall] Antigravity Hook 不存在，保持原样：$AntigravityHooks"
      }
    } catch {
      Write-Output "[uninstall] Antigravity Hook 清理跳过：$($_.Exception.Message)"
    }
  }

  if (-not $SkipAntigravityConfig) {
    try {
      if (Remove-AntigravityAgentLauncher -HooksPath $AntigravityHooks) {
        Write-Output "[uninstall] 已移除 Antigravity 启动器：$(Join-Path (Split-Path -Parent $AntigravityHooks) 'agent-notify-hook.cmd')"
      }
    } catch {
      Write-Output "[uninstall] Antigravity 启动器清理跳过：$($_.Exception.Message)"
    }
  }

  if (-not $SkipDevinConfig -and (Test-Path -LiteralPath $DevinConfig -PathType Leaf)) {
    try {
      if (Remove-DevinAgentHook -Path $DevinConfig) {
        Write-Output "[uninstall] 已移除 Devin Hook：$DevinConfig"
      } else {
        Write-Output "[uninstall] Devin Hook 不存在，保持原样：$DevinConfig"
      }
    } catch {
      Write-Output "[uninstall] Devin Hook 清理跳过：$($_.Exception.Message)"
    }
  }
}

# 5. 清理快捷方式（AgentNotify.lnk，以及改名前的 Agent-notify / Agent-notify 悬浮窗）
if (-not $SkipShortcuts) {
  foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
    foreach ($name in @('AgentNotify.lnk', 'Agent-notify.lnk', 'Agent-notify 悬浮窗.lnk')) {
      $lnk = Join-Path $dir $name
      if (Test-Path $lnk) {
        Remove-Item $lnk -Force
        Write-Output "[uninstall] 已删除快捷方式：$lnk"
      }
    }
  }
}

# 从 notify 行中移除 Agent-notify：链式包装只删 --previous-notify 载荷，直连项连同 codex/turn-ended 一起删，
# 保留用户自己的程序与其它参数。返回空串表示整行都应删除。
function Remove-AgentNotifyFromNotifyLine {
  param([string]$NotifyLine)
  $items = [regex]::Matches($NotifyLine, '"(?:\\.|[^"])*"')
  if ($items.Count -eq 0) { return '' }
  $kept = New-Object System.Collections.Generic.List[string]
  for ($i = 0; $i -lt $items.Count; $i++) {
    $value = Get-NotifyItemValue $items[$i].Value
    if ($value -ieq '--previous-notify') {
      $nextValue = ''
      if ($i + 1 -lt $items.Count) { $nextValue = Get-NotifyItemValue $items[$i + 1].Value }
      if ($nextValue -match '(?i)agent-notify') {
        # 跳过 --previous-notify 与它的 Agent-notify 载荷
        $i++
        continue
      }
    }
    if ($value -match '(?i)agent-notify\.exe') {
      # 直连项：连同其后的 codex / turn-ended 参数一起移除
      while ($i + 1 -lt $items.Count) {
        $nextValue = Get-NotifyItemValue $items[$i + 1].Value
        if ($nextValue -ieq 'codex' -or $nextValue -ieq 'turn-ended') { $i++ } else { break }
      }
      continue
    }
    $kept.Add($items[$i].Value)
  }
  if ($kept.Count -eq 0) { return '' }
  return 'notify = [ ' + ($kept -join ', ') + ' ]'
}

function Get-NotifyItemValue {
  param([string]$Raw)
  if ($Raw.Length -lt 2) { return $Raw }
  try { return [regex]::Unescape($Raw.Substring(1, $Raw.Length - 2)) } catch { return $Raw }
}

# 6. 还原 Codex notify：只改 notify 行，不整文件覆盖，保留安装后用户对配置的其它修改
if (-not $SkipCodexConfig -and (Test-Path $CodexConfig)) {
  $backup = "$CodexConfig.bak-notify-wrapper"
  $content = [IO.File]::ReadAllText($CodexConfig)
  $lineMatch = [regex]::Match($content, '(?m)^notify\s*=.*$')
  if (-not $lineMatch.Success -or $lineMatch.Value -notmatch '(?i)agent-notify') {
    if (Test-Path $backup) { Remove-Item $backup -Force }
    Write-Output '[uninstall] Codex notify 未指向 AgentNotify，保持原样。'
  } else {
    $backupLine = ''
    if (Test-Path $backup) {
      $backupMatch = [regex]::Match([IO.File]::ReadAllText($backup), '(?m)^notify\s*=.*$')
      if ($backupMatch.Success -and $backupMatch.Value -notmatch '(?i)agent-notify') {
        $backupLine = $backupMatch.Value
      }
    }
    if ($backupLine) {
      # 备份里原本就有 notify 行：只把这一行按备份原文还原，用户其它修改不受影响
      $updated = $content.Substring(0, $lineMatch.Index) + $backupLine + $content.Substring($lineMatch.Index + $lineMatch.Length)
      [IO.File]::WriteAllText($CodexConfig, $updated)
      Write-Output '[uninstall] Codex notify 已按备份原文定点还原，其它改动保持不变。'
    } else {
      $stripped = Remove-AgentNotifyFromNotifyLine $lineMatch.Value
      if ([string]::IsNullOrWhiteSpace($stripped)) {
        $updated = [regex]::Replace($content, '(?m)^notify\s*=.*(?:\r?\n|$)', '')
        [IO.File]::WriteAllText($CodexConfig, $updated)
        Write-Output '[uninstall] 已移除 config.toml 里的 AgentNotify notify 行。'
      } else {
        $updated = $content.Substring(0, $lineMatch.Index) + $stripped + $content.Substring($lineMatch.Index + $lineMatch.Length)
        [IO.File]::WriteAllText($CodexConfig, $updated)
        Write-Output '[uninstall] 已从 notify 链中移除 AgentNotify，保留其它程序。'
      }
    }
    if (Test-Path $backup) { Remove-Item $backup -Force }
  }
}

# 7. 清掉主动退出标记，避免重装后悬浮窗被旧标记挡住
$exitMarker = Join-Path $env:TEMP 'agent-notify\widget-exit.txt'
if (Test-Path $exitMarker) {
  Remove-Item $exitMarker -Force
  Write-Output "[uninstall] 已清理退出标记：$exitMarker"
}

Write-Output ''
Write-Output '[uninstall] AgentNotify 已卸载。'
Write-Output '[uninstall] 登录凭据与配置保留在 %USERPROFILE%\.config\agent-notify\，如需彻底清理请手动删除。'
exit 0
