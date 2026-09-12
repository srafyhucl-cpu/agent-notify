#Requires -Version 5.1
<#
.SYNOPSIS
  Agent-notify 安装脚本：分发 Go 单文件运行程序，安装 opencode 插件，接管 Codex notify。

.DESCRIPTION
  默认安装位置（可用参数覆盖）：
  - 运行程序：%USERPROFILE%\bin\agent-notify.exe
  - opencode 插件：%USERPROFILE%\.config\opencode\plugins\agent-notify.ts
  - Codex 配置：%USERPROFILE%\.codex\config.toml（只改写指向 codex-computer-use.exe 的 notify 行）
  安装记录 agent-notify-install.json 记录本次落盘文件，卸载按它精确清理。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -InstallDir D:\tools\bin -SkipWidgetLaunch
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugins'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [switch]$SkipCodexConfig,
  [switch]$SkipShortcuts,
  [switch]$SkipWidgetLaunch
)

$ErrorActionPreference = 'Stop'
$RepoRoot = $PSScriptRoot
$ExeName = 'agent-notify.exe'
$PluginName = 'agent-notify.ts'
$RecordName = 'agent-notify-install.json'

$HasSource = Test-Path (Join-Path $RepoRoot 'go.mod')
$HasPackage = (Test-Path (Join-Path $RepoRoot "bin\$ExeName")) -and (Test-Path (Join-Path $RepoRoot "plugin\$PluginName"))

# 在线/远程运行模式：仓库不在本地时下载新名称的 main 分支压缩包。
if ([string]::IsNullOrWhiteSpace($RepoRoot) -or (-not $HasSource -and -not $HasPackage)) {
  Write-Output '[install] 未检测到本地仓库，正在获取最新 Agent-notify 运行包...'
  $stageRoot = Join-Path ([IO.Path]::GetTempPath()) ('agent-notify-online-' + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null
  $zipPath = Join-Path $stageRoot 'agent-notify.zip'
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest -Uri 'https://github.com/srafyhucl-cpu/agent-notify/archive/refs/heads/main.zip' -OutFile $zipPath -UseBasicParsing
  Expand-Archive -Path $zipPath -DestinationPath $stageRoot -Force
  $RepoRoot = Join-Path $stageRoot 'agent-notify-main'
  $HasSource = $true
}

# 源码安装读取 internal/app/version.go；发布包读取 VERSION。
function Get-RepoVersion {
  try {
    $versionFile = Join-Path $RepoRoot 'internal\app\version.go'
    if (Test-Path $versionFile) {
      $m = Select-String -Path $versionFile -Pattern 'Version\s*=\s*"([^"]+)"' | Select-Object -First 1
      if ($m) { return $m.Matches[0].Groups[1].Value }
    }
    $packageVersion = Join-Path $RepoRoot 'VERSION'
    if (Test-Path $packageVersion) {
      $value = (Get-Content $packageVersion -Raw -Encoding utf8).Trim()
      if (-not [string]::IsNullOrWhiteSpace($value)) { return $value }
    }
  } catch { }
  return 'dev'
}

# 路径必须在根目录内，防安装记录拼接越界删除。
function Test-InsideDir {
  param([string]$Path, [string]$Root)
  try {
    $full = [IO.Path]::GetFullPath($Path)
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $full.StartsWith($rootFull + '\', [StringComparison]::OrdinalIgnoreCase)
  } catch { return $false }
}

# Go 构建缓存放仓库所在磁盘，避免默认写入 C 盘用户缓存。
function Initialize-GoEnvironment {
  $driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
  $cacheRoot = Join-Path $driveRoot 'Temp\agent-notify-go'
  if ([string]::IsNullOrWhiteSpace($env:GOPATH)) { $env:GOPATH = $cacheRoot }
  if ([string]::IsNullOrWhiteSpace($env:GOMODCACHE)) { $env:GOMODCACHE = Join-Path $cacheRoot 'pkg\mod' }
  if ([string]::IsNullOrWhiteSpace($env:GOCACHE)) { $env:GOCACHE = Join-Path $cacheRoot 'build' }
}

function Resolve-GoCommand {
  $candidates = @()
  if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_GO)) { $candidates += $env:AGENT_NOTIFY_GO }
  $onPath = Get-Command go.exe -ErrorAction SilentlyContinue
  if ($onPath) { $candidates += $onPath.Source }
  $candidates += 'D:\MyGO\install\bin\go.exe'
  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path $candidate)) { return $candidate }
  }
  return $null
}

function Test-WindowsGuiSubsystem {
  param([string]$Path)
  try {
    $stream = [IO.File]::OpenRead($Path)
    try {
      $reader = New-Object IO.BinaryReader($stream)
      $stream.Position = 0x3c
      $peOffset = $reader.ReadInt32()
      $stream.Position = $peOffset + 0x5c
      return $reader.ReadUInt16() -eq 2
    } finally {
      $stream.Dispose()
    }
  } catch {
    return $false
  }
}

try {
  # 0. 自检：仓库文件齐全
  if ($HasSource) {
    foreach ($required in @('go.mod', 'cmd\agent-notify\main.go', 'plugin\agent-notify.ts')) {
      if (-not (Test-Path (Join-Path $RepoRoot $required))) {
        throw "仓库缺文件：$required"
      }
    }
  } elseif (-not $HasPackage) {
    throw '安装包缺预编译运行程序或 OpenCode 插件。'
  }

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  New-Item -ItemType Directory -Force -Path $PluginDir | Out-Null
  $recordPath = Join-Path $InstallDir $RecordName
  $oldFiles = @()
  if (Test-Path $recordPath) {
    try {
      $old = Get-Content $recordPath -Raw -Encoding utf8 | ConvertFrom-Json
      $oldFiles = @($old.files)
    } catch { $oldFiles = @() }
  }

  # 1. 停掉正在运行的悬浮窗，释放二进制文件锁
  try {
    $escaped = [regex]::Escape([IO.Path]::GetFullPath($InstallDir).TrimEnd('\'))
    Get-CimInstance Win32_Process -Filter "Name='agent-notify.exe'" -ErrorAction SilentlyContinue |
      Where-Object { $_.CommandLine -and ($_.CommandLine -match $escaped) -and ($_.ProcessId -ne $PID) } |
      ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 200
  } catch { }

  # 2. 编译并分发 Go 单文件运行程序
  $repoExe = Join-Path $RepoRoot "bin\$ExeName"
  $needsBuild = -not (Test-Path $repoExe) -or -not (Test-WindowsGuiSubsystem $repoExe)
  if ($needsBuild -and -not $HasSource) {
    throw "发布包中的 $ExeName 不是 Windows GUI 子系统，请重新下载正确版本。"
  }
  if ($needsBuild) {
    $goExe = Resolve-GoCommand
    if (-not $goExe) {
      throw "找不到 go.exe，无法编译 $ExeName。请安装 Go 或通过 AGENT_NOTIFY_GO 指定路径。"
    }
    Initialize-GoEnvironment
    Write-Output "[install] 正在编译 $ExeName ..."
    New-Item -ItemType Directory -Force -Path (Split-Path $repoExe -Parent) | Out-Null
    Push-Location $RepoRoot
    try {
      & $goExe build -ldflags '-H windowsgui -s -w' -trimpath -o $repoExe '.\cmd\agent-notify\'
      if ($LASTEXITCODE -ne 0) { throw "go build 失败 exit=$LASTEXITCODE" }
    } finally {
      Pop-Location
    }
    if (-not (Test-WindowsGuiSubsystem $repoExe)) {
      throw "编译结果不是 Windows GUI 子系统：$repoExe"
    }
  }

  $installedExe = Join-Path $InstallDir $ExeName
  Copy-Item $repoExe $installedExe -Force
  Copy-Item (Join-Path $RepoRoot "plugin\$PluginName") (Join-Path $PluginDir $PluginName) -Force
  Write-Output "[install] 已安装运行程序：$installedExe"
  Write-Output "[install] 已安装 opencode 插件：$(Join-Path $PluginDir $PluginName)"

  # 3. 写安装记录（卸载按它精确清理；files 为相对 InstallDir 的正斜杠路径）
  $newFiles = @($ExeName)
  $record = [ordered]@{
    name        = 'Agent-notify'
    version     = (Get-RepoVersion)
    installedAt = (Get-Date -Format o)
    files       = $newFiles
  }
  $json = $record | ConvertTo-Json -Depth 4
  $json = [regex]::Replace($json, '"files":\s*"([^"]+)"', '"files": [ "$1" ]')
  [IO.File]::WriteAllText($recordPath, $json, (New-Object System.Text.UTF8Encoding($false)))

  # 4. 清理旧记录里已不再分发的文件
  $stale = @($oldFiles | Where-Object { $_ -and ($newFiles -notcontains $_) })
  foreach ($rel in $stale) {
    $full = Join-Path $InstallDir $rel
    if (-not (Test-InsideDir $full $InstallDir)) { Write-Output "[install] 跳过越界路径：$rel"; continue }
    if (Test-Path $full) { Remove-Item $full -Force; Write-Output "[install] 清理旧版本文件：$rel" }
  }

  # 5. 接管 Codex notify：只动指向 codex-computer-use.exe 的行，自定义配置不覆盖
  if (-not $SkipCodexConfig) {
    if (-not (Test-Path $CodexConfig)) {
      Write-Output "[install] 跳过 Codex 配置：找不到 $CodexConfig"
    } else {
      $content = [IO.File]::ReadAllText($CodexConfig)
      $exeSlash = ($installedExe -replace '\\', '/')
      $want = "notify = [ `"$exeSlash`", `"codex`", `"turn-ended`" ]"
      $notifyLine = [regex]::Match($content, '(?m)^notify\s*=.*$').Value
      if ($notifyLine -match 'agent-notify') {
        Write-Output '[install] Codex notify 已指向 Agent-notify，无需改动。'
      } elseif ($notifyLine -match 'codex-computer-use\.exe') {
        Copy-Item $CodexConfig "$CodexConfig.bak-notify-wrapper" -Force
        $updated = [regex]::Replace($content, '(?m)^notify\s*=.*$', $want)
        [IO.File]::WriteAllText($CodexConfig, $updated)
        Write-Output "[install] Codex notify 已接管（原文件备份到 $CodexConfig.bak-notify-wrapper）。"
      } elseif ($notifyLine -eq '') {
        Copy-Item $CodexConfig "$CodexConfig.bak-notify-wrapper" -Force
        $updated = $content.TrimEnd() + "`r`n" + $want + "`r`n"
        [IO.File]::WriteAllText($CodexConfig, $updated)
        Write-Output "[install] Codex notify 已写入 config.toml。"
      } else {
        Write-Output '[install] Codex notify 是自定义程序，保持原样；如需接入见 README。'
      }
    }
  }

  # 6. 快捷方式（开机自启 + 桌面），目标就是 exe 的 widget 子命令
  if (-not $SkipShortcuts) {
    try {
      $ws = New-Object -ComObject WScript.Shell
      foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
        $lnkPath = Join-Path $dir 'Agent-notify 悬浮窗.lnk'
        $sc = $ws.CreateShortcut($lnkPath)
        $sc.TargetPath = $installedExe
        $sc.Arguments = 'widget'
        $sc.WorkingDirectory = $InstallDir
        $sc.Description = 'Agent-notify 推送悬浮窗'
        $sc.Save()
        Write-Output "[install] 已创建快捷方式：$lnkPath"
      }
    } catch {
      Write-Output "[install] 警告：快捷方式创建失败（不影响推送）：$($_.Exception.Message)"
    }
  }

  # 7. 启动悬浮窗
  if (-not $SkipWidgetLaunch) {
    try {
      Start-Process $installedExe -ArgumentList @('widget') -WindowStyle Hidden
      Write-Output '[install] 悬浮窗已启动。'
    } catch {
      Write-Output '[install] 悬浮窗本次未启动，可双击桌面快捷方式。'
    }
  }

  Write-Output ''
  Write-Output '============================================================'
  Write-Output "  Agent-notify v$(Get-RepoVersion) 安装完成"
  Write-Output '============================================================'
  Write-Output '[install] 下一步：'
  Write-Output "  1. 微信扫码登录：& `"$installedExe`" login"
  Write-Output "  2. 发送测试通知：& `"$installedExe`" test"
  Write-Output '  3. 重启 opencode / Codex 桌面端，使插件与配置生效。'
} catch {
  [Console]::Error.WriteLine('[install] 失败：' + $_.Exception.Message)
  exit 1
}
