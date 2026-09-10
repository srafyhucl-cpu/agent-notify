#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 卸载脚本：按安装记录清理运行文件/插件，移除计划任务与快捷方式，
  还原 codex 配置。没有安装记录时（老装机）按 src 树扫描兜底。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
  # 沙箱/测试用（不碰任务、快捷方式、codex 配置）：
  powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1 -InstallDir C:\tmp\bin -SkipScheduledTask -SkipShortcuts -SkipCodexConfig
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugins'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$TaskName = 'CodexNotifyWatch',
  [switch]$SkipShortcuts,
  [switch]$SkipCodexConfig,
  [switch]$SkipScheduledTask
)

$ErrorActionPreference = 'Continue'

# 路径必须落在根目录内（防安装记录/参数拼接越界删除）。
function Test-InsideDir {
  param([string]$Path, [string]$Root)
  try {
    $full = [IO.Path]::GetFullPath($Path)
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $full.StartsWith($rootFull + '\', [StringComparison]::OrdinalIgnoreCase)
  } catch { return $false }
}

$script:removedCount = 0
$dirCandidates = @('lib', 'lib\LinkWeixin', 'widget')

function Remove-InstalledFile {
  param([string]$Rel)
  if ([string]::IsNullOrWhiteSpace($Rel)) { return }
  $full = Join-Path $InstallDir $Rel
  if (-not (Test-InsideDir $full $InstallDir)) {
    Write-Output "[uninstall] 跳过越界路径：$Rel"
    return
  }
  if (Test-Path $full) {
    Remove-Item $full -Force
    $script:removedCount++
  }
  # 记下沿途目录，最后自底向上剪空目录
  $d = Split-Path $Rel -Parent
  while ($d) {
    $script:dirCandidates += $d
    $d = Split-Path $d -Parent
  }
}
$script:dirCandidates = $dirCandidates

# 1. 运行文件：优先安装记录，其次仓库 src 树，最后老版白名单
$recordPath = Join-Path $InstallDir 'linkweixin-install.json'
if (Test-Path $recordPath) {
  try {
    $rec = Get-Content $recordPath -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($rel in @($rec.files)) { Remove-InstalledFile $rel }
    Write-Output "[uninstall] 按安装记录清理 $($script:removedCount) 个文件（安装版本 $($rec.version)）。"
  } catch {
    Write-Output "[uninstall] 安装记录解析失败：$($_.Exception.Message)"
  }
} else {
  $srcRoot = Join-Path $PSScriptRoot 'src'
  if (Test-Path $srcRoot) {
    Get-ChildItem -Path $srcRoot -Recurse -File -Force | ForEach-Object {
      Remove-InstalledFile ($_.FullName.Substring($srcRoot.Length + 1))
    }
    Write-Output "[uninstall] 无安装记录，按 src 树扫描清理 $($script:removedCount) 个文件。"
  } else {
    foreach ($n in @('notify-ai.ps1', 'codex-notify.ps1', 'codex-notify-watch.ps1', 'notify-toggle.ps1', 'linkweixin-widget.ps1', 'run-hidden.vbs', 'widget-detached.py')) {
      Remove-InstalledFile $n
    }
    Write-Output "[uninstall] 无安装记录且无 src 树，按白名单清理 $($script:removedCount) 个文件。"
  }
}

if (Test-Path $recordPath) { Remove-Item $recordPath -Force; Write-Output '[uninstall] 已删安装记录。' }

# 自底向上剪空目录（只动 InstallDir 内、且确实空了的目录）
foreach ($d in @($dirCandidates | Sort-Object -Unique | Sort-Object Length -Descending)) {
  $full = Join-Path $InstallDir $d
  if (-not (Test-InsideDir $full $InstallDir)) { continue }
  if ((Test-Path $full) -and -not (Get-ChildItem $full -Force -ErrorAction SilentlyContinue)) {
    Remove-Item $full -Force -ErrorAction SilentlyContinue
    Write-Output "[uninstall] 已删空目录 $full"
  }
}

# 2. 插件
$plug = Join-Path $PluginDir 'notify-pushplus.ts'
if (Test-Path $plug) { Remove-Item $plug -Force; Write-Output "[uninstall] 已删 $plug" }
else { Write-Output "[uninstall] 不存在，跳过：$plug" }

# 旧版单数目录残留也清理（V1 约定；沙箱路径相同时自动跳过）。
$legacyPlugDir = Join-Path (Split-Path $PluginDir -Parent) 'plugin'
if ($legacyPlugDir -ne $PluginDir) {
  $legacyPlug = Join-Path $legacyPlugDir 'notify-pushplus.ts'
  if (Test-Path $legacyPlug) {
    Remove-Item $legacyPlug -Force
    Write-Output "[uninstall] 已清理旧版单数目录残留：$legacyPlug"
    if (-not (Get-ChildItem $legacyPlugDir -Force -ErrorAction SilentlyContinue)) { Remove-Item $legacyPlugDir -Force -ErrorAction SilentlyContinue }
  }
}

# 3. 悬浮窗：杀窗体进程（限本安装目录的 powershell / python 宿主）+ 删开机/桌面快捷方式。
try {
  $esc = [regex]::Escape([IO.Path]::GetFullPath($InstallDir).TrimEnd('\'))
  $myParent = (Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction SilentlyContinue).ParentProcessId
  Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe' OR Name='pythonw.exe' OR Name='python.exe'" -ErrorAction Stop |
    Where-Object {
      $cl = $_.CommandLine
      $cl -and ($cl -match $esc) -and
      (($cl -match 'linkweixin-widget\.ps1') -or ($cl -match 'widget-detached\.py')) -and
      ($_.ProcessId -ne $PID) -and ($_.ProcessId -ne $myParent)
    } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force; Write-Output "[uninstall] 已杀悬浮窗进程 $($_.ProcessId)" }
} catch {
  Write-Output "[uninstall] 悬浮窗进程清理跳过：$($_.Exception.Message)"
}
if (-not $SkipShortcuts) {
  $lnkNames = @('linkWeixin 悬浮窗.lnk', 'linkWeixin Widget.lnk')
  foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
    foreach ($n in $lnkNames) {
      $lnk = Join-Path $dir $n
      if (Test-Path $lnk) { Remove-Item $lnk -Force; Write-Output "[uninstall] 已删快捷方式 $lnk" }
    }
  }
}

# 4. 计划任务
if (-not $SkipScheduledTask) {
  try {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop
    Write-Output "[uninstall] 计划任务 $TaskName 已删。"
  } catch {
    Write-Output "[uninstall] 计划任务 $TaskName 不存在或删不动（删任务要管理员权限）：$($_.Exception.Message)"
  }
}

# 5. codex 配置还原
if (-not $SkipCodexConfig) {
  $bak = "$CodexConfig.bak-notify-wrapper"
  if (Test-Path $bak) {
    Copy-Item $bak $CodexConfig -Force
    Write-Output "[uninstall] codex 配置已从备份还原：$bak"
  } else {
    Write-Output '[uninstall] 无 codex 配置备份，不动现有 config.toml（如需恢复请手动改 notify 行）。'
  }
}

Write-Output '[uninstall] 环境变量 PUSHPLUS_TOKEN 请手动清理（如 setx PUSHPLUS_TOKEN "" 后删注册表，或直接不管）。'
Write-Output '[uninstall] 完成，记得重启 opencode / codex 桌面端。'
exit 0
