#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 卸载脚本：移除装上去的脚本/插件/计划任务，还原 codex 配置。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugin'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$TaskName = 'CodexNotifyWatch'
)

$ErrorActionPreference = 'Continue'

foreach ($n in @('notify-ai.ps1', 'codex-notify.ps1', 'codex-notify-watch.ps1')) {
  $p = Join-Path $InstallDir $n
  if (Test-Path $p) { Remove-Item $p -Force; Write-Output "[uninstall] 已删 $p" }
  else { Write-Output "[uninstall] 不存在，跳过：$p" }
}
$plug = Join-Path $PluginDir 'notify-pushplus.ts'
if (Test-Path $plug) { Remove-Item $plug -Force; Write-Output "[uninstall] 已删 $plug" }
else { Write-Output "[uninstall] 不存在，跳过：$plug" }

try {
  Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop
  Write-Output "[uninstall] 计划任务 $TaskName 已删。"
} catch {
  Write-Output "[uninstall] 计划任务 $TaskName 不存在或删不动（删任务要管理员权限）：$($_.Exception.Message)"
}

$bak = "$CodexConfig.bak-notify-wrapper"
if (Test-Path $bak) {
  Copy-Item $bak $CodexConfig -Force
  Write-Output "[uninstall] codex 配置已从备份还原：$bak"
} else {
  Write-Output '[uninstall] 无 codex 配置备份，不动现有 config.toml（如需恢复请手动改 notify 行）。'
}

Write-Output '[uninstall] 环境变量 PUSHPLUS_TOKEN 请手动清理（如 setx PUSHPLUS_TOKEN "" 后删注册表，或直接不管）。'
Write-Output '[uninstall] 完成，记得重启 opencode / codex 桌面端。'
