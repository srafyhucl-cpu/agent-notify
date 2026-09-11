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
  [string]$AntigravityHooks = (Join-Path $env:USERPROFILE '.gemini\config\hooks.json'),
  [string]$TaskName = 'CodexNotifyWatch',
  [ValidateSet('Auto', 'Python', 'Vbs')][string]$WidgetLauncher = 'Auto',
  [switch]$BinaryOnly,
  [switch]$SkipScheduledTask,
  [switch]$SkipCodexConfig,
  [switch]$SkipAntigravityConfig,
  [switch]$SkipShortcuts,
  [switch]$SkipWidgetLaunch
)

$ErrorActionPreference = 'Stop'
$RepoRoot = $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($RepoRoot) -or -not (Test-Path (Join-Path $RepoRoot 'src'))) {
  Write-Output '[install] 检测到在线/远程运行模式，正在获取最新 linkWeixin 运行包...'
  $onlineDir = Join-Path $env:TEMP ('linkweixin-online-' + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Force -Path $onlineDir | Out-Null
  $zipPath = Join-Path $onlineDir 'linkweixin.zip'
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest -Uri 'https://github.com/srafyhucl-cpu/linkWeixin/archive/refs/heads/main.zip' -OutFile $zipPath -UseBasicParsing
  Expand-Archive -Path $zipPath -DestinationPath $onlineDir -Force
  $RepoRoot = Join-Path $onlineDir 'linkWeixin-main'
}

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
    $widgetFile = Join-Path $RepoRoot 'internal\ui\widget.go'
    if (Test-Path $widgetFile) {
      $m = Select-String -Path $widgetFile -Pattern 'AppVersion\s*=\s*"([^"]+)"' | Select-Object -First 1
      if ($m) { return $m.Matches[0].Groups[1].Value }
    }
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
    'src\antigravity-notify.ps1',
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

  # 1. 安装运行文件（优先分发 Go 原生二进制，记录升级前的旧文件清单）
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

  # 1.0 停止运行中的旧悬浮窗进程以释放二进制与脚本文件锁
  try {
    $esc = [regex]::Escape([IO.Path]::GetFullPath($InstallDir).TrimEnd('\'))
    Get-CimInstance Win32_Process -Filter "Name='linkweixin.exe' OR Name='powershell.exe' OR Name='pwsh.exe' OR Name='pythonw.exe' OR Name='python.exe'" -ErrorAction SilentlyContinue |
      Where-Object {
        $cl = $_.CommandLine
        $cl -and ($cl -match $esc) -and ($_.ProcessId -ne $PID)
      } |
      ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 200
  } catch { }

  # 1.1 分发 Go 原生绿色单文件 linkweixin.exe
  $repoExe = Join-Path $RepoRoot 'bin\linkweixin.exe'
  if (-not (Test-Path $repoExe)) {
    $goCmd = Get-Command go.exe -ErrorAction SilentlyContinue
    if ($goCmd) {
      Write-Output '[install] 正在编译最新 Go 原生单文件 linkweixin.exe...'
      Push-Location $RepoRoot
      try {
        & go build -ldflags "-H windowsgui -s -w" -trimpath -o $repoExe .\cmd\linkweixin\
      } catch { }
      Pop-Location
    }
  }
  if (Test-Path $repoExe) {
    Copy-Item $repoExe (Join-Path $InstallDir 'linkweixin.exe') -Force
    Write-Output "[install] Go 原生绿色单文件已安装：$(Join-Path $InstallDir 'linkweixin.exe')"
  }

  if ($BinaryOnly) {
    # 纯净模式：清理所有遗留旧脚本，实现绿色单文件
    foreach ($f in (Get-SrcFileList)) {
      $p = Join-Path $InstallDir $f
      if (Test-Path $p) { Remove-Item $p -Recurse -Force -ErrorAction SilentlyContinue }
    }
  } else {
    Copy-Item -Path (Join-Path $RepoRoot 'src\*') -Destination $InstallDir -Recurse -Force
  }
  Copy-Item (Join-Path $RepoRoot 'plugin\notify-pushplus.ts') (Join-Path $PluginDir 'notify-pushplus.ts') -Force
  Write-Output "[install] 运行文件已装到 $InstallDir，插件已装到 $PluginDir"

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
  $hasExe = Test-Path (Join-Path $InstallDir 'linkweixin.exe')
  $newFiles = if ($BinaryOnly -and $hasExe) {
    @('linkweixin.exe')
  } else {
    $srcFiles = Get-SrcFileList
    if ($hasExe) { @('linkweixin.exe') + $srcFiles } else { $srcFiles }
  }
  $stale = @($oldFiles | Where-Object { $_ -and ($newFiles -notcontains $_) })
  foreach ($rel in $stale) {
    $full = Join-Path $InstallDir $rel
    if (-not (Test-InsideDir $full $InstallDir)) { Write-Output "[install] 跳过越界路径：$rel"; continue }
    if (Test-Path $full) { Remove-Item $full -Force; Write-Output "[install] 清理旧版本文件：$rel" }
  }

  # 1.2 写安装记录（卸载按它精准清理；files 为相对 InstallDir 的正斜杠路径）
  $record = [ordered]@{
    name         = 'linkWeixin'
    version      = (Get-RepoVersion)
    architecture = if ($hasExe) { 'go-native' } else { 'powershell' }
    installedAt  = (Get-Date -Format o)
    launcher     = if ($BinaryOnly -and $hasExe) { 'binary' } else { $launcherRecord }
    files        = $newFiles
  }
  $json = $record | ConvertTo-Json -Depth 4
  $json = [regex]::Replace($json, '"files":\s*"([^"]+)"', '"files": [ "$1" ]')
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
      $exeBinary = Join-Path $InstallDir 'linkweixin.exe'
      if (Test-Path $exeBinary) {
        Copy-Item $CodexConfig "$CodexConfig.bak-notify-wrapper" -Force
        $exeSlash = ($exeBinary -replace '\\', '/')
        $want = "notify = [ `"$exeSlash`", `"codex`", `"turn-ended`" ]"
        $t2 = [regex]::Replace($t, '(?m)^notify\s*=.*$', $want)
        [IO.File]::WriteAllText($CodexConfig, $t2)
        Write-Output "[install] codex 配置已接管（原生二进制，原文件备份到 $CodexConfig.bak-notify-wrapper）。"
      } elseif ($t -match 'codex-notify\.ps1' -or $t -match 'linkweixin') {
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

  # 3.1 配置 Antigravity Stop 钩子（~/.gemini/config/hooks.json）
  if (-not $SkipAntigravityConfig) {
    try {
      $hooksDir = Split-Path $AntigravityHooks -Parent
      if (-not (Test-Path $hooksDir)) {
        New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null
      }
      $exeBinary = Join-Path $InstallDir 'linkweixin.exe'
      $agScriptPath = Join-Path $InstallDir 'antigravity-notify.ps1'
      $hookCmd = if (Test-Path $exeBinary) {
        "$exeBinary antigravity"
      } elseif ($agScriptPath -match '\s') {
        "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$agScriptPath`""
      } else {
        "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File $agScriptPath"
      }
      $existingHooks = $null
      if (Test-Path $AntigravityHooks) {
        Copy-Item $AntigravityHooks "$AntigravityHooks.bak-notify-wrapper" -Force
        try {
          $existingHooks = Get-Content $AntigravityHooks -Raw -Encoding UTF8 | ConvertFrom-Json
        } catch { $existingHooks = $null }
      }
      if ($null -eq $existingHooks) {
        $existingHooks = [ordered]@{}
      }

      # 规范与双重兼容：支持 linkweixin-notify 具名钩子与 hooks 顶层
      $newEntry = [ordered]@{
        type    = 'command'
        command = $hookCmd
        note    = "Fallback script: $agScriptPath"
      }

      $updated = $false
      $namedHook = if ($existingHooks.'linkweixin-notify') { $existingHooks.'linkweixin-notify' } else { [ordered]@{} }
      $namedStop = if ($namedHook.Stop) { @($namedHook.Stop) } else { @() }
      $hasNamed = $false
      foreach ($h in $namedStop) {
        if ($h.command -and (($h.command -match 'antigravity-notify\.ps1') -or ($h.command -match 'linkweixin'))) {
          $hasNamed = $true
          if ($h.command -ne $hookCmd) { $h.command = $hookCmd; $updated = $true }
          break
        }
      }
      if (-not $hasNamed) {
        $namedStop = @($namedStop) + $newEntry
        $updated = $true
      }
      $namedHook.Stop = $namedStop
      $existingHooks | Add-Member -NotePropertyName 'linkweixin-notify' -NotePropertyValue $namedHook -Force

      # 兼容顶层 hooks 键
      $legacyHook = if ($existingHooks.hooks) { $existingHooks.hooks } else { [ordered]@{} }
      $legacyStop = if ($legacyHook.Stop) { @($legacyHook.Stop) } else { @() }
      $hasLegacy = $false
      foreach ($h in $legacyStop) {
        if ($h.command -and (($h.command -match 'antigravity-notify\.ps1') -or ($h.command -match 'linkweixin'))) {
          $hasLegacy = $true
          if ($h.command -ne $hookCmd) { $h.command = $hookCmd; $updated = $true }
          break
        }
      }
      if (-not $hasLegacy) {
        $legacyStop = @($legacyStop) + $newEntry
        $updated = $true
      }
      $legacyHook.Stop = $legacyStop
      $existingHooks | Add-Member -NotePropertyName 'hooks' -NotePropertyValue $legacyHook -Force

      $jsonContent = $existingHooks | ConvertTo-Json -Depth 6
      [IO.File]::WriteAllText($AntigravityHooks, $jsonContent, (New-Object System.Text.UTF8Encoding($false)))
      if ($updated) {
        Write-Output "[install] Antigravity Stop 钩子已成功配置到 $AntigravityHooks"
      } else {
        Write-Output '[install] Antigravity hooks 已配置，无需重复添加。'
      }
    } catch {
      Write-Output "[install] 配置 Antigravity hooks 跳过或失败：$($_.Exception.Message)"
    }
  }

  # 4. 看守计划任务（Go 架构由悬浮窗内置协程看护，清理失效任务；旧版注册计划任务）
  if (-not $SkipScheduledTask) {
    $exeBinary = Join-Path $InstallDir 'linkweixin.exe'
    if (Test-Path $exeBinary) {
      try {
        $existingTask = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
        if ($existingTask) {
          Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
          Write-Output "[install] 已清理失效计划任务 $TaskName（由 Go 悬浮窗内部协程守护）。"
        } else {
          Write-Output "[install] Go 原生架构由悬浮窗内置协程看护 Codex 配置，无需注册外部计划任务 $TaskName。"
        }
      } catch {
        Write-Output "[install] 计划任务清理跳过：$($_.Exception.Message)"
      }
    } else {
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
  }

  Write-Output ''
  Write-Output '============================================================'
  Write-Output "  🎉 linkWeixin v$($record.version) 部署就绪！"
  Write-Output '  • 运行目录：' + $InstallDir
  Write-Output '  • 悬浮窗：深色 Fluent 质感、三 Agent 独立大开关、多通道支持'
  Write-Output '  • 功能特性：推送历史详情、图形化设置中心、闪屏任务一键自愈'
  Write-Output '============================================================'
  Write-Output ''
  Write-Output '[install] 快速指引：'
  Write-Output '  1. 重启 OpenCode / Codex / Antigravity（使插件与配置生效）。'
  Write-Output '  2. 在悬浮窗右键菜单进入「通道与偏好设置」，填入 Token / Webhook。'
  Write-Output "  3. 发送验证推送：powershell -NoProfile -ExecutionPolicy Bypass -File `"$InstallDir\notify-ai.ps1`" -Title `"安装验证`" -Summary `"linkWeixin 部署成功`""
  Write-Output "  4. 随用随开：powershell -NoProfile -ExecutionPolicy Bypass -File `"$InstallDir\notify-toggle.ps1`"" 

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
        $exeBinary = Join-Path $InstallDir 'linkweixin.exe'
        if (Test-Path $exeBinary) {
          $sc.TargetPath = $exeBinary
          $sc.Arguments = 'widget'
        } elseif ($launcherMode -eq 'Python') {
          $sc.TargetPath = (Get-Command pythonw.exe -ErrorAction Stop).Source
          $sc.Arguments = '"' + $pyLauncher + '"'
        } else {
          $sc.TargetPath = $wshExe
          $sc.Arguments = '"' + $vbsLauncher + '" "' + $widget + '"'
        }
        $sc.WorkingDirectory = $InstallDir
        $sc.Description = 'linkWeixin 推送悬浮窗'
        $sc.Save()
        $modeDesc = if (Test-Path $exeBinary) { 'binary' } else { $launcherRecord }
        Write-Output "[install] 悬浮窗快捷方式已建（$modeDesc）：$lnkPath"
      }
    } catch {
      Write-Output "[install] 警告：开机快捷方式没建成（不影响推送）：$($_.Exception.Message)"
    }
  }

  if (-not $SkipWidgetLaunch) {
    try {
      $exeBinary = Join-Path $InstallDir 'linkweixin.exe'
      if (Test-Path $exeBinary) {
        Start-Process $exeBinary -ArgumentList @('widget')
      } elseif ($launcherMode -eq 'Python') {
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
