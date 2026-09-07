#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 安装脚本：把脚本装到 ~/bin，把 opencode 插件装到插件目录，
  接管 codex notify，并注册看守计划任务。

.DESCRIPTION
  默认安装位置（可用参数覆盖）：
  - 脚本目录：%USERPROFILE%\bin
  - opencode 插件：%USERPROFILE%\.config\opencode\plugin
  - codex 配置：%USERPROFILE%\.codex\config.toml（改写前备份 .bak-notify-wrapper）
  密钥只从环境变量 PUSHPLUS_TOKEN 读，本脚本不写任何密钥。

.EXAMPLE
  # 请在管理员 PowerShell 里跑（注册计划任务要提权）：
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -InstallDir D:\tools\bin -SkipScheduledTask
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugin'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$TaskName = 'CodexNotifyWatch',
  [switch]$SkipScheduledTask,
  [switch]$SkipCodexConfig
)

$ErrorActionPreference = 'Stop'
$RepoRoot = $PSScriptRoot

try {
  # 0. 自检：仓库文件齐全
  $wants = @(
    'scripts\notify-ai.ps1',
    'scripts\codex-notify.ps1',
    'scripts\codex-notify-watch.ps1',
    'scripts\notify-toggle.ps1',
    'scripts\linkweixin-widget.ps1',
    'opencode-plugin\notify-pushplus.ts'
  )
  foreach ($w in $wants) {
    if (-not (Test-Path (Join-Path $RepoRoot $w))) { throw "仓库缺文件：$w" }
  }

  # 1. 复制脚本与插件
  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  New-Item -ItemType Directory -Force -Path $PluginDir | Out-Null
  Copy-Item (Join-Path $RepoRoot 'scripts\notify-ai.ps1') (Join-Path $InstallDir 'notify-ai.ps1') -Force
  Copy-Item (Join-Path $RepoRoot 'scripts\codex-notify.ps1') (Join-Path $InstallDir 'codex-notify.ps1') -Force
  Copy-Item (Join-Path $RepoRoot 'scripts\codex-notify-watch.ps1') (Join-Path $InstallDir 'codex-notify-watch.ps1') -Force
  Copy-Item (Join-Path $RepoRoot 'scripts\notify-toggle.ps1') (Join-Path $InstallDir 'notify-toggle.ps1') -Force
  Copy-Item (Join-Path $RepoRoot 'scripts\linkweixin-widget.ps1') (Join-Path $InstallDir 'linkweixin-widget.ps1') -Force
  Copy-Item (Join-Path $RepoRoot 'opencode-plugin\notify-pushplus.ts') (Join-Path $PluginDir 'notify-pushplus.ts') -Force
  Write-Output "[install] 脚本已装到 $InstallDir，插件已装到 $PluginDir"

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
  if (-not $SkipScheduledTask) {
    try {
      $watch = Join-Path $InstallDir 'codex-notify-watch.ps1'
      $taskAction = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -ExecutionPolicy Bypass -File "' + $watch + '"')
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
  try {
    $psExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    if (-not (Test-Path $psExe)) { $psExe = 'powershell.exe' }
    $widget = Join-Path $InstallDir 'linkweixin-widget.ps1'
    $ws = New-Object -ComObject WScript.Shell
    foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
      $lnkPath = Join-Path $dir 'linkWeixin 悬浮窗.lnk'
      $sc = $ws.CreateShortcut($lnkPath)
      $sc.TargetPath = $psExe
      $sc.Arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $widget + '"'
      $sc.WorkingDirectory = $InstallDir
      $sc.Description = 'linkWeixin 推送悬浮窗'
      $sc.Save()
      Write-Output "[install] 悬浮窗快捷方式已建：$lnkPath"
    }
    try {
      Start-Process $psExe -ArgumentList @('-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass', '-File', $widget)
      Write-Output '[install] 悬浮窗已启动（右下角无边框小窗，拖标题区移动）。'
    } catch {
      Write-Output '[install] 悬浮窗本次未自动启动，手动跑一次上面的命令即可。'
    }
  } catch {
    Write-Output "[install] 警告：开机快捷方式没建成（不影响推送）：$($_.Exception.Message)"
  }
} catch {
  [Console]::Error.WriteLine('[install] 失败：' + $_.Exception.Message)
  exit 1
}
