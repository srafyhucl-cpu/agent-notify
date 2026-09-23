#Requires -Version 5.1
<#
.SYNOPSIS
  Agent-notify 安装脚本（Go 1.x 遗留运行时）：分发 Go 单文件运行程序，安装 opencode 插件，接管 Codex notify。

.DESCRIPTION
  2.0 起正式入口是 Inno 安装器 Agent-notify-Setup-vX.Y.Z.exe（Tauri 桌面版 + ingress）。
  本脚本只服务 Go 1.x 单文件运行时的源码安装与回滚窗口，普通用户请使用正式安装器。

  默认安装位置（可用参数覆盖）：
  - 运行程序：%USERPROFILE%\bin\agent-notify.exe
  - opencode 插件：%USERPROFILE%\.config\opencode\plugins\agent-notify.ts
  - Devin 回复扩展：%USERPROFILE%\.devin\extensions\agent-notify
  - Codex 配置：%USERPROFILE%\.codex\config.toml（只改写指向 codex-computer-use.exe 的 notify 行）
  - Antigravity Hook：%USERPROFILE%\.gemini\config\hooks.json（只维护顶层 agent-notify Hook）
  - Devin Hook：%APPDATA%\devin\config.json（只维护 hooks.Stop 中的 Agent-notify handler）
  安装记录 agent-notify-install.json 记录本次落盘文件，卸载按它精确清理。
  安装时会把 $InstallDir 里的绝对路径写进插件副本，插件不依赖默认安装目录。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -InstallDir D:\tools\bin -SkipWidgetLaunch
#>
param(
  [string]$InstallDir = (Join-Path $env:USERPROFILE 'bin'),
  [string]$PluginDir = (Join-Path $env:USERPROFILE '.config\opencode\plugins'),
  [string]$DevinExtensionDir = (Join-Path $env:USERPROFILE '.devin\extensions\agent-notify'),
  [string]$CommandCodeModDir = (Join-Path $env:USERPROFILE '.commandcode\mods'),
  [string]$CodexConfig = (Join-Path $env:USERPROFILE '.codex\config.toml'),
  [string]$AntigravityHooks = (Join-Path $env:USERPROFILE '.gemini\config\hooks.json'),
  [string]$DevinConfig = (Join-Path $env:APPDATA 'devin\config.json'),
  [switch]$SkipCodexConfig,
  [switch]$SkipAntigravityConfig,
  [switch]$SkipDevinConfig,
  [switch]$SkipDevinExtension,
  [switch]$SkipCommandCodeMod,
  [switch]$SkipShortcuts,
  [switch]$SkipWidgetLaunch,
  [switch]$SkipLoginLaunch,
  [switch]$ConfigureOnly
)

$ErrorActionPreference = 'Stop'
$RepoRoot = $PSScriptRoot
$ExeName = 'agent-notify.exe'
$PluginName = 'agent-notify.ts'
$RecordName = 'agent-notify-install.json'

# 归一化为绝对路径：相对路径会被写进插件与 Codex 配置，换工作目录后就失联。
foreach ($pathParam in @('InstallDir', 'PluginDir', 'AntigravityHooks', 'DevinConfig', 'DevinExtensionDir', 'CommandCodeModDir', 'CodexConfig')) {
  $currentValue = Get-Variable -Name $pathParam -ValueOnly -ErrorAction SilentlyContinue
  if (-not [string]::IsNullOrWhiteSpace($currentValue)) {
    Set-Variable -Name $pathParam -Value ([IO.Path]::GetFullPath($currentValue))
  }
}
# 快捷方式名与标准安装器保持一致，旧名字只做清理，避免重复。
$ShortcutName = 'AgentNotify.lnk'
# 改名前的快捷方式名，安装与卸载都要清掉，避免桌面上留下两个图标。
$LegacyShortcutNames = @('Agent-notify.lnk', 'Agent-notify 悬浮窗.lnk')
# 客户端正在读取被替换文件时的有界重试次数与间隔。
$InstallReplaceAttempts = 5
$InstallReplaceDelayMs = 300

# 正式入口是 2.0 Inno 安装器；先把归属讲清楚，避免把遗留运行时误装给普通用户。
Write-Output '[install] 注意：本脚本安装 Go 1.x 遗留单文件运行时（源码安装 / 回滚窗口用）。'
Write-Output '[install] 2.0 正式入口是安装器 Agent-notify-Setup-vX.Y.Z.exe（Tauri 桌面版），普通用户请改用它。'

$HasSource = Test-Path (Join-Path $RepoRoot 'go.mod')
$HasDevinExtension = (Test-Path (Join-Path $RepoRoot 'plugin\devin-extension\package.json')) -and
  (Test-Path (Join-Path $RepoRoot 'plugin\devin-extension\extension.js'))
$HasDevinExtension = $HasDevinExtension -and
  (Test-Path (Join-Path $RepoRoot 'plugin\devin-extension\acp-bridge.js'))
$HasCommandCodeMod = Test-Path (Join-Path $RepoRoot 'plugin\commandcode-mod\agent-notify.ts')
$HasPackage = ((Test-Path (Join-Path $RepoRoot "bin\$ExeName")) -or
  ($ConfigureOnly -and (Test-Path (Join-Path $RepoRoot $ExeName)))) -and
  (Test-Path (Join-Path $RepoRoot "plugin\$PluginName")) -and
  $HasDevinExtension -and $HasCommandCodeMod

# 在线/远程运行模式：仓库不在本地时下载新名称的 main 分支压缩包。
if ([string]::IsNullOrWhiteSpace($RepoRoot) -or (-not $HasSource -and -not $HasPackage)) {
  Write-Output '[install] 未检测到本地仓库，正在获取最新 AgentNotify 运行包...'
  $stageRoot = Join-Path ([IO.Path]::GetTempPath()) ('agent-notify-online-' + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null
  $zipPath = Join-Path $stageRoot 'agent-notify.zip'
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest -Uri 'https://github.com/srafyhucl-cpu/agent-notify/archive/refs/heads/main.zip' -OutFile $zipPath -UseBasicParsing
  Expand-Archive -Path $zipPath -DestinationPath $stageRoot -Force
  $RepoRoot = Join-Path $stageRoot 'agent-notify-main'
  $HasSource = $true
}

$hookConfigModule = Join-Path $RepoRoot 'tools\hook-config.ps1'
if (-not (Test-Path -LiteralPath $hookConfigModule -PathType Leaf)) {
  throw "安装包缺少 Hook 配置模块：$hookConfigModule"
}
. $hookConfigModule

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

function Get-AgentNotifyIntegrationStatus {
  param([Parameter(Mandatory = $true)][string]$Executable)
  try {
    # GUI 子系统程序不会把标准输出回传给 PowerShell 的调用运算符，必须显式重定向。
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Executable
    $startInfo.Arguments = 'integration-status --json'
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.CreateNoWindow = $true

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
      return @()
    }
    $json = $process.StandardOutput.ReadToEnd()
    $null = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($json)) {
      return @()
    }
    $parsed = $json.Trim() | ConvertFrom-Json
    return $parsed
  } catch {
    return @()
  }
}

function Get-NotifyTargetPath {
  param([string]$NotifyLine)
  if ([string]::IsNullOrWhiteSpace($NotifyLine)) {
    return ''
  }
  $match = [regex]::Match($NotifyLine, '"(?:\\.|[^"])*"')
  if (-not $match.Success) {
    return ''
  }
  $quoted = $match.Value
  return [regex]::Unescape($quoted.Substring(1, $quoted.Length - 2))
}

# 把 notify 行里指向 agent-notify.exe 的路径替换成 $NewPath。
# 直连项与 --previous-notify 内嵌的 JSON 数组都能处理，路径形式不限（盘符、UNC、相对路径）。
function Update-AgentNotifyPathInNotifyLine {
  param(
    [string]$NotifyLine,
    [string]$NewPath
  )
  $items = [regex]::Matches($NotifyLine, '"(?:\\.|[^"])*"')
  if ($items.Count -eq 0) { return $NotifyLine }

  $changed = $false
  $kept = New-Object System.Collections.Generic.List[string]
  foreach ($item in $items) {
    $raw = $item.Value
    $value = $raw
    if ($raw.Length -gt 2) {
      try { $value = [regex]::Unescape($raw.Substring(1, $raw.Length - 2)) } catch { $value = $raw }
    }

    if ($value -match '(?i)agent-notify\.exe\s*$') {
      $kept.Add('"' + $NewPath + '"')
      $changed = $true
      continue
    }

    if ($value -match '(?i)agent-notify\.exe') {
      # --previous-notify 的载荷是内嵌 JSON 数组，逐项替换后重新转义
      $innerFixed = $null
      try {
        $inner = ConvertFrom-Json -InputObject $value
        if ($inner -is [System.Array]) {
          $innerChanged = $false
          for ($index = 0; $index -lt $inner.Count; $index++) {
            if ([string]$inner[$index] -match '(?i)agent-notify\.exe\s*$') {
              $inner[$index] = $NewPath
              $innerChanged = $true
            }
          }
          if ($innerChanged) {
            $innerFixed = (ConvertTo-Json -InputObject @($inner) -Compress).Replace('"', '\"')
          }
        }
      } catch { }

      if (-not $innerFixed) {
        # 兜底：历史或手写的不规范转义会让 JSON 解析失败，此时按路径片段替换。
        $pattern = '(?i)[^"\[\],\s]*agent-notify\.exe'
        $replaced = [regex]::Replace($value, $pattern, [System.Text.RegularExpressions.MatchEvaluator] { param($match) $NewPath })
        if ($replaced -ne $value) {
          $innerFixed = $replaced.Replace('\', '\\').Replace('"', '\"')
        }
      }

      if ($innerFixed) {
        $kept.Add('"' + $innerFixed + '"')
        $changed = $true
        continue
      }
    }

    $kept.Add($raw)
  }

  if (-not $changed) { return $NotifyLine }
  return 'notify = [ ' + ($kept -join ', ') + ' ]'
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

# 同目录写入再替换，避免安装中断留下半写入的 exe 或插件。
# OpenCode / Devin 可能正在读取被替换的文件，遇到瞬时占用时有界重试。
function Install-FileAtomically {
  param([string]$Source, [string]$Destination)
  $parent = Split-Path -Parent $Destination
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
  $temporary = Join-Path $parent ((Split-Path -Leaf $Destination) + '.new-' + [guid]::NewGuid().ToString('N'))
  try {
    Copy-Item -LiteralPath $Source -Destination $temporary -Force
    for ($attempt = 1; ; $attempt++) {
      try {
        if (Test-Path -LiteralPath $Destination) {
          [IO.File]::Replace($temporary, $Destination, [NullString]::Value, $true)
        } else {
          [IO.File]::Move($temporary, $Destination)
        }
        break
      } catch {
        if ($attempt -ge $InstallReplaceAttempts) { throw }
        Start-Sleep -Milliseconds $InstallReplaceDelayMs
      }
    }
  } finally {
    if (Test-Path -LiteralPath $temporary) {
      Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
  }
}

# Go 构建缓存放仓库所在磁盘，避免默认写入 C 盘用户缓存。
function Initialize-GoEnvironment {
  $driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
  $cacheRoot = Join-Path $driveRoot 'Temp\agent-notify-go'
  $env:GOPATH = $cacheRoot
  $env:GOMODCACHE = Join-Path $cacheRoot 'pkg\mod'
  $env:GOCACHE = Join-Path $cacheRoot 'build'
  $env:GOTMPDIR = Join-Path $cacheRoot 'tmp'
  New-Item -ItemType Directory -Force -Path $env:GOTMPDIR | Out-Null
}

function Resolve-GoCommand {
  $candidates = @()
  if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_GO)) { $candidates += $env:AGENT_NOTIFY_GO }
  $onPath = Get-Command go.exe -ErrorAction SilentlyContinue
  if ($onPath) { $candidates += $onPath.Source }
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
  if ($ConfigureOnly) {
    foreach ($required in @(
        $ExeName,
        'plugin\agent-notify.ts',
        'plugin\devin-extension\package.json',
        'plugin\devin-extension\extension.js',
        'plugin\devin-extension\acp-bridge.js',
        'plugin\commandcode-mod\agent-notify.ts'
      )) {
      if (-not (Test-Path (Join-Path $RepoRoot $required))) {
        throw "仅配置模式的安装目录缺文件：$required"
      }
    }
  } elseif ($HasSource) {
    foreach ($required in @(
        'go.mod',
        'cmd\agent-notify\main.go',
        'plugin\agent-notify.ts',
        'plugin\devin-extension\package.json',
        'plugin\devin-extension\extension.js',
        'plugin\devin-extension\acp-bridge.js',
        'plugin\commandcode-mod\agent-notify.ts'
      )) {
      if (-not (Test-Path (Join-Path $RepoRoot $required))) {
        throw "仓库缺文件：$required"
      }
    }
  } elseif (-not $HasPackage) {
    throw '安装包缺预编译运行程序、OpenCode 插件或 Devin 回复扩展。'
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

  $installedExe = Join-Path $InstallDir $ExeName
  if (-not $ConfigureOnly) {
    # 2. 编译 Go 单文件运行程序；构建成功前保留正在运行的旧版本。
    $repoExe = Join-Path $RepoRoot "bin\$ExeName"
    $needsBuild = $HasSource -or -not (Test-Path $repoExe) -or -not (Test-WindowsGuiSubsystem $repoExe)
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
        $commit = 'unknown'
        try {
          $resolvedCommit = (& git rev-parse --short HEAD 2>$null).Trim()
          if ($resolvedCommit) { $commit = $resolvedCommit }
        } catch { }
        $buildTime = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        $module = 'github.com/srafyhucl-cpu/agent-notify/internal/app'
        $ldflags = "-H windowsgui -s -w -X $module.Version=$(Get-RepoVersion) -X $module.Commit=$commit -X $module.BuildTime=$buildTime"
        & $goExe build -ldflags $ldflags -trimpath -o $repoExe '.\cmd\agent-notify\'
        if ($LASTEXITCODE -ne 0) { throw "go build 失败 exit=$LASTEXITCODE" }
      } finally {
        Pop-Location
      }
      if (-not (Test-WindowsGuiSubsystem $repoExe)) {
        throw "编译结果不是 Windows GUI 子系统：$repoExe"
      }
    }

    # 3. 停掉正在运行的悬浮窗，释放二进制文件锁并替换文件。
    try {
      # 目录边界必须带分隔符，避免 D:\bin 误伤 D:\bin2 的进程。
      $escaped = [regex]::Escape([IO.Path]::GetFullPath($InstallDir).TrimEnd('\')) + '\\'
      Get-CimInstance Win32_Process -Filter "Name='agent-notify.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -and ($_.CommandLine -match $escaped) -and ($_.ProcessId -ne $PID) } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
      Start-Sleep -Milliseconds 200
    } catch { }

    Install-FileAtomically -Source $repoExe -Destination $installedExe
  } elseif (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
    throw "仅配置模式找不到已安装程序：$installedExe"
  }

  # 3.5 把自身、卸载脚本与插件一并部署到安装目录：悬浮窗首次启动会在 exe 旁边
  #     执行 install.ps1 -ConfigureOnly，缺这些文件就只会在启动时报“首次接入失败”。
  $payloadFiles = [ordered]@{
    'install.ps1'                          = (Join-Path $RepoRoot 'install.ps1')
    'uninstall.ps1'                        = (Join-Path $RepoRoot 'uninstall.ps1')
    'VERSION'                              = (Join-Path $RepoRoot 'VERSION')
    'tools/hook-config.ps1'                = (Join-Path $RepoRoot 'tools\hook-config.ps1')
    "plugin/$PluginName"                   = (Join-Path $RepoRoot "plugin\$PluginName")
    'plugin/devin-extension/package.json'  = (Join-Path $RepoRoot 'plugin\devin-extension\package.json')
    'plugin/devin-extension/extension.js'  = (Join-Path $RepoRoot 'plugin\devin-extension\extension.js')
    'plugin/devin-extension/acp-bridge.js' = (Join-Path $RepoRoot 'plugin\devin-extension\acp-bridge.js')
    'plugin/commandcode-mod/agent-notify.ts' = (Join-Path $RepoRoot 'plugin\commandcode-mod\agent-notify.ts')
  }
  $payloadInstalled = @()
  foreach ($relative in $payloadFiles.Keys) {
    $source = $payloadFiles[$relative]
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
      Write-Output "[install] 警告：安装包缺少 $relative，跳过部署。"
      continue
    }
    $destination = Join-Path $InstallDir $relative
    if ([IO.Path]::GetFullPath($source) -ne [IO.Path]::GetFullPath($destination)) {
      Install-FileAtomically -Source $source -Destination $destination
    }
    $payloadInstalled += $relative
  }
  Write-Output "[install] 已部署运行文件到安装目录：$InstallDir"

  $installedPlugin = Join-Path $PluginDir $PluginName
  Install-FileAtomically -Source (Join-Path $RepoRoot "plugin\$PluginName") -Destination $installedPlugin

  # 插件默认只认 %USERPROFILE%\bin；把真实安装路径写进插件副本，自定义目录才不会失联。
  $pluginText = [IO.File]::ReadAllText($installedPlugin)
  $bakedPath = $installedExe.Replace('\', '\\').Replace('"', '\"')
  $patchedText = [regex]::Replace($pluginText, '(?m)^const BAKED_BIN = ".*"$', { param($match) 'const BAKED_BIN = "' + $bakedPath + '"' })
  if ($patchedText -eq $pluginText) {
    Write-Output '[install] 警告：插件缺少 BAKED_BIN 占位，将回退到 %USERPROFILE%\bin 或 PATH。'
  } else {
    [IO.File]::WriteAllText($installedPlugin, $patchedText, (New-Object System.Text.UTF8Encoding($false)))
  }

  Write-Output "[install] 已安装运行程序：$installedExe"
  Write-Output "[install] 已安装 opencode 插件：$installedPlugin"

  if (-not $SkipDevinExtension) {
    $devinExtensionSource = Join-Path $RepoRoot 'plugin\devin-extension'
    if (-not (Test-Path (Join-Path $devinExtensionSource 'package.json')) -or
        -not (Test-Path (Join-Path $devinExtensionSource 'extension.js'))) {
      throw "Devin 回复扩展源文件不完整：$devinExtensionSource"
    }
    if (-not (Test-Path (Join-Path $devinExtensionSource 'acp-bridge.js'))) {
      throw "Devin 回复扩展缺少 ACP 通道模块：$devinExtensionSource"
    }
    New-Item -ItemType Directory -Force -Path $DevinExtensionDir | Out-Null
    Install-FileAtomically -Source (Join-Path $devinExtensionSource 'package.json') -Destination (Join-Path $DevinExtensionDir 'package.json')
    Install-FileAtomically -Source (Join-Path $devinExtensionSource 'extension.js') -Destination (Join-Path $DevinExtensionDir 'extension.js')
    Install-FileAtomically -Source (Join-Path $devinExtensionSource 'acp-bridge.js') -Destination (Join-Path $DevinExtensionDir 'acp-bridge.js')
    Write-Output "[install] 已安装 Devin 回复扩展：$DevinExtensionDir"
  }

  # Command Code mod：部署到用户级 mods 目录，并把安装路径写进 BAKED_BIN。
  if (-not $SkipCommandCodeMod) {
    New-Item -ItemType Directory -Force -Path $CommandCodeModDir | Out-Null
    $installedMod = Join-Path $CommandCodeModDir $PluginName
    Install-FileAtomically -Source (Join-Path $RepoRoot 'plugin\commandcode-mod\agent-notify.ts') -Destination $installedMod
    $modText = [IO.File]::ReadAllText($installedMod)
    $modBakedPath = $installedExe.Replace('\', '\\').Replace('"', '\"')
    $modPatchedText = [regex]::Replace($modText, '(?m)^const BAKED_BIN = ".*"$', { param($match) 'const BAKED_BIN = "' + $modBakedPath + '"' })
    if ($modPatchedText -eq $modText) {
      Write-Output '[install] 警告：Command Code mod 缺少 BAKED_BIN 占位，将回退到 %USERPROFILE%\bin 或 PATH。'
    } else {
      [IO.File]::WriteAllText($installedMod, $modPatchedText, (New-Object System.Text.UTF8Encoding($false)))
    }
    Write-Output "[install] 已安装 Command Code mod：$installedMod"
  }

  # 4. 写安装记录（卸载按它精确清理；files 为相对 InstallDir 的正斜杠路径）
  $newFiles = @($ExeName) + $payloadInstalled
  $record = [ordered]@{
    name        = 'Agent-notify'
    version     = (Get-RepoVersion)
    installedAt = (Get-Date -Format o)
    files       = $newFiles
  }
  $json = $record | ConvertTo-Json -Depth 4
  $json = [regex]::Replace($json, '"files":\s*"([^"]+)"', '"files": [ "$1" ]')
  [IO.File]::WriteAllText($recordPath, $json, (New-Object System.Text.UTF8Encoding($false)))

  # 5. 清理旧记录里已不再分发的文件
  $stale = @($oldFiles | Where-Object { $_ -and ($newFiles -notcontains $_) })
  foreach ($rel in $stale) {
    $full = Join-Path $InstallDir $rel
    if (-not (Test-InsideDir $full $InstallDir)) { Write-Output "[install] 跳过越界路径：$rel"; continue }
    if (Test-Path $full) { Remove-Item $full -Force; Write-Output "[install] 清理旧版本文件：$rel" }
  }

  # 6. 接入 Antigravity / Devin Stop hook，只维护 Agent-notify 自己的配置。
  if (-not $SkipAntigravityConfig) {
    $antigravityParent = Split-Path -Parent $AntigravityHooks
    if ((Test-Path -LiteralPath $AntigravityHooks -PathType Leaf) -or (Test-Path -LiteralPath $antigravityParent -PathType Container)) {
      $antigravityLauncher = Set-AntigravityAgentLauncher -HooksPath $AntigravityHooks -Executable $installedExe
      $antigravityCommand = Get-AntigravityHookCommand
      Set-AntigravityAgentHook -Path $AntigravityHooks -Command $antigravityCommand
      Write-Output "[install] 已写入 Antigravity 启动器：$antigravityLauncher"
      Write-Output "[install] 已接入 Antigravity Stop hook：$AntigravityHooks"
    } else {
      Write-Output "[install] 跳过 Antigravity 配置：未发现 $AntigravityHooks"
    }
  }

  if (-not $SkipDevinConfig) {
    $devinParent = Split-Path -Parent $DevinConfig
    if ((Test-Path -LiteralPath $DevinConfig -PathType Leaf) -or (Test-Path -LiteralPath $devinParent -PathType Container)) {
      $devinCommand = '"' + $installedExe + '" devin stop'
      Set-DevinAgentHook -Path $DevinConfig -Command $devinCommand
      Write-Output "[install] 已接入 Devin Stop hook：$DevinConfig"
    } else {
      Write-Output "[install] 跳过 Devin 配置：未发现 $DevinConfig"
    }
  }

  # 7. 接管 Codex notify：只动指向 codex-computer-use.exe 的行，自定义配置不覆盖
  if (-not $SkipCodexConfig) {
    if (-not (Test-Path $CodexConfig)) {
      Write-Output "[install] 跳过 Codex 配置：找不到 $CodexConfig"
    } else {
      $content = [IO.File]::ReadAllText($CodexConfig)
      $exeSlash = ($installedExe -replace '\\', '/')
      $want = "notify = [ `"$exeSlash`", `"codex`", `"turn-ended`" ]"
      $notifyLine = [regex]::Match($content, '(?m)^notify\s*=.*$').Value
      $notifyTarget = Get-NotifyTargetPath $notifyLine
      if ($notifyLine -match '(?i)agent-notify\.exe') {
        # 直连或 Codex computer-use 的链式包装：保留包装，只把链里的 agent-notify.exe 更新到当前安装目录。
        $updatedLine = Update-AgentNotifyPathInNotifyLine -NotifyLine $notifyLine -NewPath $exeSlash
        if ($updatedLine -ne $notifyLine) {
          Copy-Item $CodexConfig "$CodexConfig.bak-notify-wrapper" -Force
          $lineMatch = [regex]::Match($content, '(?m)^notify\s*=.*$')
          $updated = $content.Substring(0, $lineMatch.Index) + $updatedLine + $content.Substring($lineMatch.Index + $lineMatch.Length)
          [IO.File]::WriteAllText($CodexConfig, $updated)
          Write-Output "[install] Codex notify 已更新到当前 AgentNotify 路径（原文件备份到 $CodexConfig.bak-notify-wrapper）。"
        } elseif ($notifyLine -notmatch [regex]::Escape($exeSlash)) {
          # 兜底：确实无法自动改写时明确警告，避免"无需改动"掩盖指向旧路径的事实。
          Write-Output "[install] 警告：Codex notify 里的 agent-notify.exe 不指向本安装目录，且无法自动更新。请手动改为：$installedExe"
        } else {
          Write-Output '[install] Codex notify 已指向 AgentNotify，无需改动。'
        }
      } elseif ($notifyTarget -match '(?i)codex-computer-use\.exe') {
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

  # 8. 快捷方式（开机自启 + 桌面），目标就是 exe 的 widget 子命令。
  #    与标准安装器共用 AgentNotify.lnk，避免两套安装体系各建一份快捷方式。
  if (-not $SkipShortcuts -and -not $ConfigureOnly) {
    try {
      $ws = New-Object -ComObject WScript.Shell
      foreach ($dir in @([Environment]::GetFolderPath('Startup'), [Environment]::GetFolderPath('Desktop'))) {
        $lnkPath = Join-Path $dir $ShortcutName
        $sc = $ws.CreateShortcut($lnkPath)
        $sc.TargetPath = $installedExe
        $sc.Arguments = 'widget'
        $sc.WorkingDirectory = $InstallDir
        $sc.Description = 'AgentNotify 推送悬浮窗'
        $sc.Save()
        Write-Output "[install] 已创建快捷方式：$lnkPath"
        foreach ($legacyName in $LegacyShortcutNames) {
          $legacyLnk = Join-Path $dir $legacyName
          if (Test-Path -LiteralPath $legacyLnk) {
            Remove-Item -LiteralPath $legacyLnk -Force
            Write-Output "[install] 已清理旧快捷方式：$legacyLnk"
          }
        }
      }
    } catch {
      Write-Output "[install] 警告：快捷方式创建失败（不影响推送）：$($_.Exception.Message)"
    }
  }

  # 9. 启动悬浮窗
  if (-not $SkipWidgetLaunch -and -not $ConfigureOnly) {
    try {
      Start-Process $installedExe -ArgumentList @('widget') -WindowStyle Hidden
      Write-Output '[install] 悬浮窗已启动。'
    } catch {
      Write-Output '[install] 悬浮窗本次未启动，可双击桌面快捷方式。'
    }
  }

  # 首次安装直接打开扫码登录，并在登录成功后等待微信发送首条消息建立会话。
  if (-not $SkipLoginLaunch -and -not $ConfigureOnly) {
    $credentialPath = $env:AGENT_NOTIFY_CREDENTIAL_FILE
    if ([string]::IsNullOrWhiteSpace($credentialPath)) {
      $credentialPath = Join-Path $env:USERPROFILE '.config\agent-notify\clawbot.json'
    }
    if (Test-Path -LiteralPath $credentialPath -PathType Leaf) {
      Write-Output '[install] 已检测到微信登录凭据，跳过自动扫码。'
    } else {
      try {
        Start-Process $installedExe -ArgumentList @('login')
        Write-Output '[install] 已打开微信扫码登录，扫码后请发送一条消息完成会话绑定。'
      } catch {
        Write-Output '[install] 自动打开扫码登录失败，请运行：& "' + $installedExe + '" login'
      }
    }
  }

  Write-Output ''
  Write-Output '============================================================'
  Write-Output "  AgentNotify v$(Get-RepoVersion) 安装完成"
  Write-Output '============================================================'
  Write-Output '[install] 下一步：'
  Write-Output "  1. 如未自动打开扫码窗口：& `"$installedExe`" login"
  Write-Output "  2. 扫码后给 ClawBot 发送一条微信消息；可用 `& `"$installedExe`" test` 验证推送。"
  $integrationStatus = @(Get-AgentNotifyIntegrationStatus -Executable $installedExe)
  if ($integrationStatus.Count -gt 0) {
    Write-Output '  3. 当前 Agent 接入状态：'
    foreach ($item in $integrationStatus) {
      $label = switch ($item.state) {
        'connected' { '已接入' }
        'pending_restart' { '待重启' }
        'error' { '接入异常' }
        default { '未接入' }
      }
      Write-Output ("     {0}: {1} - {2}" -f $item.name, $label, $item.detail)
      if (-not [string]::IsNullOrWhiteSpace([string]$item.action)) {
        Write-Output ("       {0}" -f $item.action)
      }
    }
  } else {
    Write-Output "  3. 检查接入状态：& `"$installedExe`" integration-status"
  }
} catch {
  [Console]::Error.WriteLine('[install] 失败：' + $_.Exception.Message)
  exit 1
}
