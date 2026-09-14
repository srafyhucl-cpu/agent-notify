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
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$AntigravityHooks = (Join-Path $env:USERPROFILE '.gemini\config\hooks.json'),
  [string]$DevinConfig = (Join-Path $env:APPDATA 'devin\config.json'),
  [switch]$SkipShortcuts,
  [switch]$SkipAntigravityConfig,
  [switch]$SkipDevinConfig,
  [switch]$SkipDevinExtension,
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
    $escaped = [regex]::Escape([IO.Path]::GetFullPath($InstallDir).TrimEnd('\'))
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

# 5. 清理快捷方式
if (-not $SkipShortcuts) {
  foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
    $lnk = Join-Path $dir 'Agent-notify 悬浮窗.lnk'
    if (Test-Path $lnk) {
      Remove-Item $lnk -Force
      Write-Output "[uninstall] 已删除快捷方式：$lnk"
    }
  }
}

# 6. 还原 Codex notify
if (-not $SkipCodexConfig -and (Test-Path $CodexConfig)) {
  $backup = "$CodexConfig.bak-notify-wrapper"
  if (Test-Path $backup) {
    Copy-Item $backup $CodexConfig -Force
    Remove-Item $backup -Force
    Write-Output "[uninstall] Codex 配置已从备份还原。"
  } else {
    $content = [IO.File]::ReadAllText($CodexConfig)
    if ($content -match '(?m)^notify\s*=.*agent-notify') {
      $updated = [regex]::Replace($content, '(?m)^notify\s*=.*agent-notify.*(?:\r?\n|$)', '')
      [IO.File]::WriteAllText($CodexConfig, $updated)
      Write-Output '[uninstall] 已移除 config.toml 里的 Agent-notify notify 行。'
    } else {
      Write-Output '[uninstall] Codex notify 未指向 Agent-notify，保持原样。'
    }
  }
}

# 7. 清掉主动退出标记，避免重装后悬浮窗被旧标记挡住
$exitMarker = Join-Path $env:TEMP 'agent-notify\widget-exit.txt'
if (Test-Path $exitMarker) {
  Remove-Item $exitMarker -Force
  Write-Output "[uninstall] 已清理退出标记：$exitMarker"
}

Write-Output ''
Write-Output '[uninstall] Agent-notify 已卸载。'
Write-Output '[uninstall] 登录凭据与配置保留在 %USERPROFILE%\.config\agent-notify\，如需彻底清理请手动删除。'
exit 0
