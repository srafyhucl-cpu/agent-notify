#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 安装脚本：整树安装运行文件到 ~/bin，安装 opencode 插件，
  接管 codex notify，注册看守计划任务，并写安装记录（供卸载精确清理）。

.DESCRIPTION
  默认安装位置（可用参数覆盖）：
  - 运行文件：%USERPROFILE%\bin（src\ 整树拷贝，结构原样保留；含依赖模块 lib\）
  - opencode 插件：%USERPROFILE%\.config\opencode\plugins（opencode V2 约定）
  - codex 配置：%USERPROFILE%\.codex\config.toml（改写前备份 .bak-notify-wrapper）
  密钥只从环境变量 PUSHPLUS_TOKEN 读，本脚本不写任何密钥。
  安装记录 linkweixin-install.json 列出本次装上去的文件与版本，卸载按它精准清理。

.EXAMPLE
  # 请在管理员 PowerShell 里跑（注册计划任务要提权）：
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -InstallDir D:\tools\bin -SkipScheduledTask
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugins'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$TaskName = 'CodexNotifyWatch',
  [ValidateSet('Auto', 'Python', 'Vbs')][string]$WidgetLauncher = 'Auto',
  [switch]$SkipScheduledTask,
  [switch]$SkipCodexConfig,
  [switch]$SkipShortcuts,
  [switch]$SkipWidgetLaunch
)

$ErrorActionPreference = 'Stop'
$RepoRoot = $PSScriptRoot

# 路径必须落在根目录内（防安装记录/参数拼接越界删除）。
function Test-InsideDir {
  param([string]$Path, [string]$Root)
  try {
    $full = [IO.Path]::GetFullPath($Path)
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $full.StartsWith($rootFull + '\', [StringComparison]::OrdinalIgnoreCase)
  } catch { return $false }
}

# 版本号单一来源：模块清单 psd1 的 ModuleVersion；模块尚未落地时用 dev。
function Get-RepoVersion {
  try {
    $psd1 = Join-Path $RepoRoot 'src\lib\LinkWeixin\LinkWeixin.psd1'
    if (-not (Test-Path $psd1)) { return 'dev' }
    $m = Select-String -Path $psd1 -Pattern "ModuleVersion\s*=\s*'([^']+)'" | Select-Object -First 1
    if ($m) { return $m.Matches[0].Groups[1].Value }
  } catch { }
  return 'dev'
}

# src 树里的相对路径清单（正斜杠，跨机器一致）。
function Get-SrcFileList {
  $srcRoot = Join-Path $RepoRoot 'src'
  return @(Get-ChildItem -Path $srcRoot -Recurse -File -Force | ForEach-Object {
      $_.FullName.Substring($srcRoot.Length + 1).Replace('\', '/')
    })
}

try {
  # 0. 自检：仓库文件齐全
  $wants = @(
    'src\notify-ai.ps1',
    'src\codex-notify.ps1',
    'src\codex-notify-watch.ps1',
    'src\notify-toggle.ps1',
    'src\linkweixin-widget.ps1',
    'src\run-hidden.vbs',
    'src\widget-detached.py',
    'plugin\notify-pushplus.ts'
  )
  foreach ($w in $wants) {
    if (-not (Test-Path (Join-Path $RepoRoot $w))) { throw "仓库缺文件：$w" }
  }

  # 1. 安装运行文件（src 整树拷贝，结构原样保留）+ 记录升级前的旧文件清单
  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  New-Item -ItemType Directory -Force -Path $PluginDir | Out-Null
  $recordPath = Join-Path $InstallDir 'linkweixin-install.json'
  $oldFiles = @()
  if (Test-Path $recordPath) {
    try {
      $old = Get-Content $recordPath -Raw -Encoding utf8 | ConvertFrom-Json
      $oldFiles = @($old.files)
    } catch { $oldFiles = @() }
  }
  Copy-Item -Path (Join-Path $RepoRoot 'src\*') -Destination $InstallDir -Recurse -Force
  Copy-Item (Join-Path $RepoRoot 'plugin\notify-pushplus.ts') (Join-Path $PluginDir 'notify-pushplus.ts') -Force
  Write-Output "[install] 运行文件已整树装到 $InstallDir，插件已装到 $PluginDir"

  # 兼容清理：OpenCode V1 的单数目录 plugin\ 里如有本插件残留，清掉避免双加载/误导检查。
  # 用 PluginDir 推导（沙箱临时目录下两个路径相同，自动跳过，不会误删）。
  $legacyPlugDir = Join-Path (Split-Path $PluginDir -Parent) 'plugin'
  if ($legacyPlugDir -ne $PluginDir) {
    $legacyPlug = Join-Path $legacyPlugDir 'notify-pushplus.ts'
    if (Test-Path $legacyPlug) {
      Remove-Item $legacyPlug -Force
      if (-not (Get-ChildItem $legacyPlugDir -Force -ErrorAction SilentlyContinue)) { Remove-Item $legacyPlugDir -Force -ErrorAction SilentlyContinue }
      Write-Output '[install] 已清理旧版单数目录中的插件残留。'
    }
  }

  # 1.1 悬浮窗启动方式：Auto = 有 pythonw 且 widget-detached.py 已装就优先 Python
  #（GUI 子系统，无控制台、无 WT 页签，防误杀），否则回退 run-hidden.vbs（现状）。
  $pyLauncher = Join-Path $InstallDir 'widget-detached.py'
  $hasPythonw = [bool](Get-Command pythonw.exe -ErrorAction SilentlyContinue)
  $launcherMode = $WidgetLauncher
  if ($launcherMode -eq 'Auto') {
    $launcherMode = if ($hasPythonw -and (Test-Path $pyLauncher)) { 'Python' } else { 'Vbs' }
  }
  if ($launcherMode -eq 'Python' -and -not $hasPythonw) {
    throw '指定 -WidgetLauncher Python 但找不到 pythonw.exe（装 Python，或改用 Auto/Vbs）'
  }
  if ($launcherMode -eq 'Python' -and -not (Test-Path $pyLauncher)) {
    throw "指定 -WidgetLauncher Python 但缺少 $pyLauncher"
  }
  $launcherRecord = if ($launcherMode -eq 'Python') { 'python' } else { 'vbs' }
  Write-Output "[install] 悬浮窗启动方式：$launcherRecord（Auto 检测 = 有 pythonw 用 Python，否则 Vbs）"

  # 1.1 清理上一版记录里、这一版已不存在的陈旧文件（防止旧 lib/widget 残留）
  $newFiles = Get-SrcFileList
  $stale = @($oldFiles | Where-Object { $_ -and ($newFiles -notcontains $_) })
  foreach ($rel in $stale) {
    $full = Join-Path $InstallDir $rel
    if (-not (Test-InsideDir $full $InstallDir)) { Write-Output "[install] 跳过越界路径：$rel"; continue }
    if (Test-Path $full) { Remove-Item $full -Force; Write-Output "[install] 清理旧版本文件：$rel" }
  }

  # 1.2 写安装记录（卸载按它精准清理；files 为相对 InstallDir 的正斜杠路径）
  $record = [ordered]@{
    name        = 'linkWeixin'
    version     = (Get-RepoVersion)
    installedAt = (Get-Date -Format o)
    launcher    = $launcherRecord
    files       = $newFiles
  }
  $json = $record | ConvertTo-Json -Depth 4
  [IO.File]::WriteAllText($recordPath, $json, (New-Object System.Text.UTF8Encoding($false)))
  Write-Output "[install] 安装记录已写：$recordPath（版本 $($record.version)，$($newFiles.Count) 个文件）"

  # 2. Token 检查（只读环境变量，不写入）
  if ([string]::IsNullOrWhiteSpace($env:PUSHPLUS_TOKEN)) {
    Write-Output '[install] 警告：未检测到 PUSHPLUS_TOKEN。去 pushplus.plus 拿 token 后执行：'
    Write-Output '  setx PUSHPLUS_TOKEN "你的token"'
    Write-Output '  设完必须重启 opencode / codex 桌面端（含后台 service）才生效。'
  } else {
    Write-Output '[install] PUSHPLUS_TOKEN 已检测到（值不显示）。'
  }

  # 3. 接管 codex notify（只动指向 codex-computer-use.exe 的行，自定义配置不动）
  if (-not $SkipCodexConfig) {
    if (-not (Test-Path $CodexConfig)) {
      Write-Output "[install] 跳过 codex 配置：找不到 $CodexConfig"
    } else {
      $t = [IO.File]::ReadAllText($CodexConfig)
      if ($t -match 'codex-notify\.ps1') {
        Write-Output '[install] codex 配置已指向 wrapper，无需改动。'
      } elseif ($t -notmatch '(?m)^notify\s*=') {
        Write-Output '[install] 跳过 codex 配置：config.toml 里没有 notify 行，请手动加（见 README）。'
      } elseif ($t -notmatch 'codex-computer-use\.exe') {
        Write-Output '[install] 跳过 codex 配置：notify 是自定义程序，不覆盖（见 README 手动接法）。'
      } else {
        Copy-Item $CodexConfig "$CodexConfig.bak-notify-wrapper" -Force
        $wrapperSlash = ((Join-Path $InstallDir 'codex-notify.ps1') -replace '\\', '/')
        $want = "notify = [ `"powershell.exe`", `"-NoProfile`", `"-ExecutionPolicy`", `"Bypass`", `"-File`", `"$wrapperSlash`", `"turn-ended`" ]"
        $t2 = [regex]::Replace($t, '(?m)^notify\s*=.*$', $want)
        [IO.File]::WriteAllText($CodexConfig, $t2)
        Write-Output "[install] codex 配置已接管（原文件备份到 $CodexConfig.bak-notify-wrapper）。"
      }
    }
  }

  # 4. 注册看守计划任务（codex 桌面会把 notify 改回去，看守负责恢复）。
  #    需要管理员权限：请在管理员 PowerShell 里跑本脚本，或手动执行下面这段。
  #    动作经 run-hidden.vbs 中转：Win11 默认终端是 Windows Terminal 时，
  #    任务直接拉 powershell.exe 会先建可见窗口/页签再藏，闪一下；
  #    wscript 本身无控制台，子进程全程隐藏，WT 拦截不到任何东西。
  if (-not $SkipScheduledTask) {
    try {
      $watch = Join-Path $InstallDir 'codex-notify-watch.ps1'
      $launcher = Join-Path $InstallDir 'run-hidden.vbs'
      $wshExe = Join-Path $env:SystemRoot 'System32\wscript.exe'
      if (-not (Test-Path $wshExe)) { $wshExe = 'wscript.exe' }
      $taskAction = New-ScheduledTaskAction -Execute $wshExe -Argument ('"' + $launcher + '" "' + $watch + '"')
      $taskT1 = New-ScheduledTaskTrigger -AtLogOn
      $taskT2 = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5)
      Register-ScheduledTask -TaskName $TaskName -Action $taskAction -Trigger @($taskT1, $taskT2) -Force | Out-Null
      Write-Output "[install] 计划任务 $TaskName 已注册（登录触发 + 每 5 分钟）。"
    } catch {
      throw "注册计划任务失败（多半缺管理员权限，请用管理员 PowerShell 重跑）：$($_.Exception.Message)"
    }
  }

  Write-Output '[install] 下一步：'
  Write-Output '  1. 重启 opencode / codex 桌面端（含后台 service）。'
  Write-Output '  2. 跑冒烟测试：powershell -NoProfile -ExecutionPolicy Bypass -File tests\smoke.ps1'
  Write-Output '  3. 真推一条验证：powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-ai.ps1" -Title "安装验证" -Summary "linkWeixin 安装成功"'
  Write-Output '  4. 随用随开：powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-toggle.ps1"（翻转；-On/-Off 显式指定）'

  # 5. 悬浮窗开机自启（shell:startup 快捷方式，无需管理员）
  #    + 桌面快捷方式（关掉窗体后从桌面双击即可再打开）。
  #    快捷方式目标是 wscript+run-hidden.vbs（同上，.lnk 拉 powershell 在 WT 下必闪）。
  if (-not $SkipShortcuts) {
    try {
      $wshExe = Join-Path $env:SystemRoot 'System32\wscript.exe'
      if (-not (Test-Path $wshExe)) { $wshExe = 'wscript.exe' }
      $widget = Join-Path $InstallDir 'linkweixin-widget.ps1'
      $vbsLauncher = Join-Path $InstallDir 'run-hidden.vbs'
      $ws = New-Object -ComObject WScript.Shell
      foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
        $lnkPath = Join-Path $dir 'linkWeixin 悬浮窗.lnk'
        $sc = $ws.CreateShortcut($lnkPath)
        if ($launcherMode -eq 'Python') {
          $sc.TargetPath = (Get-Command pythonw.exe -ErrorAction Stop).Source
          $sc.Arguments = '"' + $pyLauncher + '"'
        } else {
          $sc.TargetPath = $wshExe
          $sc.Arguments = '"' + $vbsLauncher + '" "' + $widget + '"'
        }
        $sc.WorkingDirectory = $InstallDir
        $sc.Description = 'linkWeixin 推送悬浮窗'
        $sc.Save()
        Write-Output "[install] 悬浮窗快捷方式已建（$launcherRecord）：$lnkPath"
      }
    } catch {
      Write-Output "[install] 警告：开机快捷方式没建成（不影响推送）：$($_.Exception.Message)"
    }
  }

  if (-not $SkipWidgetLaunch) {
    try {
      if ($launcherMode -eq 'Python') {
        Start-Process (Get-Command pythonw.exe -ErrorAction Stop).Source -ArgumentList @('"' + $pyLauncher + '"') -WindowStyle Hidden
      } else {
        $wshExe = Join-Path $env:SystemRoot 'System32\wscript.exe'
        if (-not (Test-Path $wshExe)) { $wshExe = 'wscript.exe' }
        $widget = Join-Path $InstallDir 'linkweixin-widget.ps1'
        $vbsLauncher = Join-Path $InstallDir 'run-hidden.vbs'
        Start-Process $wshExe -ArgumentList @('"' + $vbsLauncher + '"', '"' + $widget + '"') -WindowStyle Hidden
      }
      Write-Output '[install] 悬浮窗已启动（右下角无边框小窗，拖标题区移动）。'
    } catch {
      Write-Output '[install] 悬浮窗本次未自动启动，手动跑一次桌面快捷方式即可。'
    }
  }
} catch {
  [Console]::Error.WriteLine('[install] 失败：' + $_.Exception.Message)
  exit 1
}
