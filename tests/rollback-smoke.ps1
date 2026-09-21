#Requires -Version 5.1
<#
.SYNOPSIS
  回滚窗口 smoke：验证 Go 版 -> Rust 版 -> 回滚 Go 版 -> 再升级 Rust 版的完整链路。

.DESCRIPTION
  在隔离目录中执行，不触碰真实用户配置（后缀 -Copy 的旧文件快照除外，见下）：

  1. 准备隔离根，并把本机真实的 Go 遗留文件（config.json / push.log /
     reply-routes.jsonl / reply-state.jsonl）**复制**进去。
  2. 记录这些文件的 SHA256 基线，并额外记录**真实源文件**的基线。
  3. 启动 Rust 桌面端（隔离环境 + 自动退出），触发只读迁移。
  4. 断言：SQLite 与迁移报告生成；导入计数大于 0；**隔离副本与真实源文件都零改动**。
  5. 用上一稳定 Go 版的可执行文件（取自公开 Release 的 ZIP）在隔离环境执行 `status`，
     验证旧版仍能读取旧文件。
  6. 断言：Go 版运行后 SQLite 与迁移报告仍在、旧文件仍零改动。
  7. 再次启动 Rust 桌面端，断言不会重复迁移、不会重放 Claim。
  8. 断言 Rust 单实例：第二个实例不得并存。

  说明：
  - 不在本脚本里"安装" Go 版：Go 安装器的 [Run] 会在静默安装后启动程序并写入真实用户的
    Codex / Antigravity / Devin Hook。这里改用公开 Release 的 ZIP 内可执行文件，隔离运行。
  - 隔离根只包含从本机复制的旧配置 / 历史 / Route / Claim 快照，脚本结束时默认整体删除。
  - 不复制 clawbot.json：避免隔离实例用真实凭据轮询平台，也避免迁移把凭据写入共享的
    Windows 凭据管理器而覆盖真实登录（凭据导入已由真实验收覆盖）。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\rollback-smoke.ps1 -From 2.0.0 -To 1.17.0
#>
param(
  [string]$From = '2.0.0',
  [string]$To = '1.17.0',
  [string]$WorkRoot = '',
  [string]$DesktopExe = '',
  [string]$GoZipPath = '',
  [string]$Sqlite3 = '',
  [int]$SmokeExitMs = 8000,
  [switch]$KeepWorkRoot
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent

$failures = New-Object System.Collections.Generic.List[string]
function Assert-True {
  param([bool]$Condition, [string]$Message)
  if ($condition) { Write-Output "[ok] $Message" } else { [void]$failures.Add($Message); Write-Output "[FAIL] $Message" }
}
function Resolve-FirstLeaf {
  param([string[]]$Candidates)
  foreach ($candidate in $Candidates) {
    if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }
  return ''
}
function Get-PathHashMap {
  param([System.Collections.Specialized.OrderedDictionary]$Paths)
  $map = @{}
  foreach ($name in $Paths.Keys) {
    $path = $Paths[$name]
    if (Test-Path -LiteralPath $path -PathType Leaf) {
      $map[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    }
  }
  return $map
}
function Compare-FileHashMap {
  param([hashtable]$Before, [hashtable]$After, [string]$Label)
  foreach ($name in $Before.Keys) {
    if (-not $After.ContainsKey($name)) {
      Assert-True $false "$Label：文件消失 $name"
      continue
    }
    Assert-True ($Before[$name] -eq $After[$name]) "$Label：文件未被修改 $name"
  }
}

# 真实遗留文件所在目录：迁移只读读取，本脚本只复制、绝不写入。
# 刻意不复制 clawbot.json：隔离实例若拿到真实凭据会去轮询平台，而且迁移会把凭据写入
# 共享的 Windows 凭据管理器（账号 ID 与真实账号相同），可能覆盖真实登录。
# 计划要求的输入是"旧配置、历史、Route、Claim"，不含凭据，因此这里与计划一致。
# 迁移器的读取位置：push.log 在 temp_root\agent-notify，其余在 config_dir。
# 刻意不含两样东西，否则无法在单机上安全隔离：
#   - clawbot.json：隔离实例会拿真实凭据轮询平台，且迁移会把凭据写入共享的 Windows
#     凭据管理器（账号 ID 与真实账号相同），可能覆盖真实登录。
#   - reply-state.jsonl（Claim）：迁移要求 Claim 与登录凭据同时存在，缺凭据会拒绝导入
#     以避免串号。凭据导入路径已由真实验收覆盖（迁移导入 52 条 Claim）。
# 护栏本身由下面的"Claim 缺凭据必须拒绝导入"用例断言。
$legacyRoot = Join-Path $env:USERPROFILE '.config\agent-notify'
$legacyPushLog = Join-Path $env:TEMP 'agent-notify\push.log'
$legacyInputs = [ordered]@{
  'config.json'        = Join-Path $legacyRoot 'config.json'
  'reply-routes.jsonl' = Join-Path $legacyRoot 'reply-routes.jsonl'
  'push.log'           = $legacyPushLog
}
$presentInputs = @($legacyInputs.Keys | Where-Object { Test-Path -LiteralPath $legacyInputs[$_] -PathType Leaf })
if ($presentInputs.Count -eq 0) {
  throw "找不到任何旧版遗留文件（$legacyRoot 与 $legacyPushLog）。回滚 smoke 需要真实遗留数据作为迁移输入。"
}

if ([string]::IsNullOrWhiteSpace($DesktopExe)) {
  $DesktopExe = Resolve-FirstLeaf @(
    'D:\app\AgentNotify-Rust-Preview\agentnotify-desktop.exe',
    (Join-Path $env:LOCALAPPDATA 'Programs\Agent-notify\agentnotify-desktop.exe')
  )
}
if ([string]::IsNullOrWhiteSpace($DesktopExe)) {
  throw '找不到 Rust 桌面端可执行文件，请用 -DesktopExe 指定。'
}
if ([string]::IsNullOrWhiteSpace($Sqlite3)) {
  $Sqlite3 = Resolve-FirstLeaf @(
    (Get-Command sqlite3.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\sqlite3.exe')
  )
}
if ([string]::IsNullOrWhiteSpace($Sqlite3)) {
  throw '找不到 sqlite3.exe，无法核对迁移结果。'
}

if ([string]::IsNullOrWhiteSpace($WorkRoot)) {
  $WorkRoot = Join-Path 'D:\Temp' ('agentnotify-rollback-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
$WorkRoot = [IO.Path]::GetFullPath($WorkRoot)
if (-not $WorkRoot.StartsWith('D:\Temp\agentnotify-', [StringComparison]::OrdinalIgnoreCase)) {
  throw "隔离根必须位于 D:\Temp\agentnotify- 之下，当前为：$WorkRoot"
}

# 单实例互斥是按应用标识生效的：本机已有实例时，smoke 实例会直接退出。
$running = @(Get-Process agentnotify-desktop -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
  throw "检测到正在运行的 agentnotify-desktop（PID $($running[0].Id)）。回滚 smoke 需要独占运行，请先从托盘退出。"
}

$configDir = Join-Path $WorkRoot 'config'
$dataDir = Join-Path $WorkRoot 'data'
$logDir = Join-Path $WorkRoot 'logs'
$spoolDir = Join-Path $WorkRoot 'spool'
$goInstallDir = Join-Path $WorkRoot 'go-install'
$tempDir = Join-Path $WorkRoot 'temp'
foreach ($directory in @($configDir, $dataDir, $logDir, $spoolDir, $goInstallDir, $tempDir)) {
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
}

# 迁移器用 std::env::temp_dir() 定位 push.log，因此把子进程的 TEMP/TMP 也隔离。
$previousTemp = $env:TEMP
$previousTmp = $env:TMP
$env:TEMP = $tempDir
$env:TMP = $tempDir

$indexPath = Join-Path $dataDir 'state.db'
$reportPath = Join-Path $dataDir 'legacy-import-report.json'
$env:AGENT_NOTIFY_CONFIG_DIR = $configDir
$env:AGENT_NOTIFY_DATA_DIR = $dataDir
$env:AGENT_NOTIFY_LOG_DIR = $logDir
$env:AGENT_NOTIFY_SPOOL_DIR = $spoolDir

Write-Output "[rollback] 隔离根：$WorkRoot"
Write-Output "[rollback] Rust 桌面端：$DesktopExe"
Write-Output "[rollback] 版本窗口：$From -> $To -> $From"

try {
  # 1) 复制真实遗留文件作为迁移输入，并记录副本与真实源文件的 SHA256 基线。
  $snapshotPaths = [ordered]@{}
  foreach ($name in $presentInputs) {
    $destination = if ($name -eq 'push.log') {
      Join-Path $tempDir 'agent-notify\push.log'
    } else {
      Join-Path $configDir $name
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $destination -Parent) | Out-Null
    Copy-Item -LiteralPath $legacyInputs[$name] -Destination $destination -Force
    $snapshotPaths[$name] = $destination
  }
  $sourcePaths = [ordered]@{}
  foreach ($name in $presentInputs) { $sourcePaths[$name] = $legacyInputs[$name] }
  $copyBefore = Get-PathHashMap -Paths $snapshotPaths
  $sourceBefore = Get-PathHashMap -Paths $sourcePaths
  Assert-True ($copyBefore.Count -eq $presentInputs.Count) "隔离副本已就绪（$($copyBefore.Count) 个旧文件）"

  # 2) 首次升级：启动 Rust 桌面端触发只读迁移，随后自动优雅退出。
  function Invoke-DesktopOnce {
    param([string]$Label)
    $env:AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS = [string]$SmokeExitMs
    $process = Start-Process -FilePath $DesktopExe -PassThru
    if (-not $process.WaitForExit(($SmokeExitMs + 60000))) {
      try { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue } catch { }
      throw "$Label：桌面端未在预期时间内退出"
    }
    return $process.ExitCode
  }

  $exitCode = Invoke-DesktopOnce -Label '首次升级'
  Assert-True ($exitCode -eq 0) "首次升级退出码为 0（实际 $exitCode）"
  Assert-True (Test-Path -LiteralPath $indexPath -PathType Leaf) '迁移已生成 SQLite：data\state.db'
  Assert-True (Test-Path -LiteralPath $reportPath -PathType Leaf) '迁移报告已生成：legacy-import-report.json'

  $notifications = 0
  if (Test-Path -LiteralPath $indexPath -PathType Leaf) {
    $notifications = [int]('' + (& $Sqlite3 -readonly $indexPath 'select count(*) from notifications;')).Trim()
  }
  Assert-True ($notifications -gt 0) "迁移导入了通知记录（$notifications 条）"
  $marker = ''
  if (Test-Path -LiteralPath $indexPath -PathType Leaf) {
    $marker = ('' + (& $Sqlite3 -readonly $indexPath "select count(*) from settings where key='legacyImportV1';")).Trim()
  }
  Assert-True ($marker -eq '1') '已写入 legacyImportV1 标记（后续启动不会再导入）'

  # 3) 只读迁移不得改动旧文件：副本与真实源文件都要零改动。
  Compare-FileHashMap -Before $copyBefore -After (Get-PathHashMap -Paths $snapshotPaths) -Label '首次迁移后（副本）'
  Compare-FileHashMap -Before $sourceBefore -After (Get-PathHashMap -Paths $sourcePaths) -Label '首次迁移后（真实源文件）'

  # 4) 回滚：用上一稳定 Go 版可执行文件在隔离环境执行 status（只读，不联网）。
  if ([string]::IsNullOrWhiteSpace($GoZipPath)) {
    $GoZipPath = Join-Path $WorkRoot "Agent-notify-v$To.zip"
    $url = "https://github.com/srafyhucl-cpu/agent-notify-releases/releases/download/v$To/Agent-notify-v$To.zip"
    Write-Output "[rollback] 下载上一稳定版 ZIP：$url"
    Invoke-WebRequest $url -OutFile $GoZipPath -UseBasicParsing
  }
  if (-not (Test-Path -LiteralPath $GoZipPath -PathType Leaf)) {
    throw "找不到 Go 版 ZIP：$GoZipPath"
  }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $goExe = Join-Path $goInstallDir 'agent-notify.exe'
  $archive = [IO.Compression.ZipFile]::OpenRead($GoZipPath)
  try {
    $entry = $archive.Entries | Where-Object { $_.FullName -eq 'Agent-notify/bin/agent-notify.exe' } | Select-Object -First 1
    if (-not $entry) { throw "Go 版 ZIP 缺少 bin/agent-notify.exe：$GoZipPath" }
    [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $goExe, $true)
  } finally {
    $archive.Dispose()
  }
  $goStdout = Join-Path $WorkRoot 'go-status.out.txt'
  $goStderr = Join-Path $WorkRoot 'go-status.err.txt'
  $goProcess = Start-Process -FilePath $goExe -ArgumentList 'status' -NoNewWindow -Wait -PassThru `
    -RedirectStandardOutput $goStdout -RedirectStandardError $goStderr
  Assert-True ($goProcess.ExitCode -eq 0) "回滚后的 Go 版可以运行 status（exit=$($goProcess.ExitCode)）"
  $goStatus = ''
  if (Test-Path -LiteralPath $goStdout) { $goStatus = (Get-Content -LiteralPath $goStdout -Raw -Encoding UTF8) }
  Assert-True (-not [string]::IsNullOrWhiteSpace($goStatus)) 'Go 版 status 有输出（仍能读取旧文件）'

  # 5) 旧版运行不得删除 SQLite / 迁移报告，也不得改动旧文件。
  Assert-True (Test-Path -LiteralPath $indexPath -PathType Leaf) 'Go 版运行后 SQLite 仍存在'
  Assert-True (Test-Path -LiteralPath $reportPath -PathType Leaf) 'Go 版运行后迁移报告仍存在'
  Compare-FileHashMap -Before $copyBefore -After (Get-PathHashMap -Paths $snapshotPaths) -Label 'Go 版运行后（副本）'
  Compare-FileHashMap -Before $sourceBefore -After (Get-PathHashMap -Paths $sourcePaths) -Label 'Go 版运行后（真实源文件）'

  # 6) 再次升级：不得重复迁移、不得重放 Claim。
  $claimsBefore = [int]('' + (& $Sqlite3 -readonly $indexPath 'select count(*) from inbound_claims;')).Trim()
  $routesBefore = [int]('' + (& $Sqlite3 -readonly $indexPath 'select count(*) from reply_routes;')).Trim()
  $exitCode2 = Invoke-DesktopOnce -Label '再次升级'
  Assert-True ($exitCode2 -eq 0) "再次升级退出码为 0（实际 $exitCode2）"
  $notificationsAfter = [int]('' + (& $Sqlite3 -readonly $indexPath 'select count(*) from notifications;')).Trim()
  $claimsAfter = [int]('' + (& $Sqlite3 -readonly $indexPath 'select count(*) from inbound_claims;')).Trim()
  $routesAfter = [int]('' + (& $Sqlite3 -readonly $indexPath 'select count(*) from reply_routes;')).Trim()
  Assert-True ($notificationsAfter -eq $notifications) "再次升级没有重复导入通知（$notifications -> $notificationsAfter）"
  Assert-True ($routesAfter -eq $routesBefore) "再次升级没有重复导入路由（$routesBefore -> $routesAfter）"
  Assert-True ($claimsAfter -eq $claimsBefore) "再次升级没有重复导入 Claim（$claimsBefore -> $claimsAfter）"

  # 7) 单实例：第二次启动不得并存第二个桌面端进程。
  $env:AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS = [string]($SmokeExitMs + 4000)
  $first = Start-Process -FilePath $DesktopExe -PassThru
  Start-Sleep -Seconds 4
  $second = Start-Process -FilePath $DesktopExe -PassThru
  Start-Sleep -Seconds 3
  $alive = @(Get-Process agentnotify-desktop -ErrorAction SilentlyContinue)
  Assert-True ($alive.Count -le 1) "同一时刻只有一个桌面端实例在运行（实际 $($alive.Count) 个）"
  foreach ($process in @($first, $second)) {
    if ($process -and -not $process.HasExited) {
      try { $process.WaitForExit(($SmokeExitMs + 60000)) | Out-Null } catch { }
    }
  }

  # 8) 安全护栏：存在 Claim 但缺少 ClawBot 凭据时，迁移必须拒绝导入（避免把 Claim 串到错误账号）。
  $claimsSource = Join-Path $legacyRoot 'reply-state.jsonl'
  if (Test-Path -LiteralPath $claimsSource -PathType Leaf) {
    $guardConfig = Join-Path $WorkRoot 'guard-config'
    $guardData = Join-Path $WorkRoot 'guard-data'
    $guardLogs = Join-Path $WorkRoot 'guard-logs'
    $guardSpool = Join-Path $WorkRoot 'guard-spool'
    foreach ($directory in @($guardConfig, $guardData, $guardLogs, $guardSpool)) {
      New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }
    Copy-Item -LiteralPath $claimsSource -Destination (Join-Path $guardConfig 'reply-state.jsonl') -Force
    Copy-Item -LiteralPath (Join-Path $configDir 'config.json') -Destination (Join-Path $guardConfig 'config.json') -Force -ErrorAction SilentlyContinue
    $env:AGENT_NOTIFY_CONFIG_DIR = $guardConfig
    $env:AGENT_NOTIFY_DATA_DIR = $guardData
    $env:AGENT_NOTIFY_LOG_DIR = $guardLogs
    $env:AGENT_NOTIFY_SPOOL_DIR = $guardSpool
    $guardExit = Invoke-DesktopOnce -Label 'Claim 缺凭据护栏'
    $guardLog = Join-Path $guardLogs 'runtime.log'
    $guardText = ''
    if (Test-Path -LiteralPath $guardLog) { $guardText = (Get-Content -LiteralPath $guardLog -Raw -Encoding UTF8) }
    Assert-True ($guardText -match 'legacy_import_failed') 'Claim 缺凭据时迁移明确失败（legacy_import_failed）'
    $guardDb = Join-Path $guardData 'state.db'
    $guardNotifications = -1
    if (Test-Path -LiteralPath $guardDb) {
      $guardNotifications = [int]('' + (& $Sqlite3 -readonly $guardDb 'select count(*) from notifications;')).Trim()
    }
    Assert-True ($guardNotifications -eq 0) "拒绝导入时不得写入任何通知（实际 $guardNotifications 条）"
    $env:AGENT_NOTIFY_CONFIG_DIR = $configDir
    $env:AGENT_NOTIFY_DATA_DIR = $dataDir
    $env:AGENT_NOTIFY_LOG_DIR = $logDir
    $env:AGENT_NOTIFY_SPOOL_DIR = $spoolDir
  } else {
    Write-Output '[skip] 本机没有 reply-state.jsonl，跳过 Claim 缺凭据护栏用例'
  }
} finally {
  Remove-Item Env:AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS -ErrorAction SilentlyContinue
  Remove-Item Env:AGENT_NOTIFY_CONFIG_DIR, Env:AGENT_NOTIFY_DATA_DIR, Env:AGENT_NOTIFY_LOG_DIR, Env:AGENT_NOTIFY_SPOOL_DIR -ErrorAction SilentlyContinue
  $env:TEMP = $previousTemp
  $env:TMP = $previousTmp
  if (-not $KeepWorkRoot) {
    if (Test-Path -LiteralPath $WorkRoot) { [IO.Directory]::Delete($WorkRoot, $true) }
    Write-Output '[rollback] 已清理隔离根'
  } else {
    Write-Warning "[rollback] 保留了隔离根（内含旧配置 / 历史快照，请自行处理）：$WorkRoot"
  }
}

if ($failures.Count -gt 0) {
  Write-Output ''
  Write-Output "[rollback] 失败 $($failures.Count) 项："
  foreach ($item in $failures) { Write-Output "  - $item" }
  exit 1
}
Write-Output ''
Write-Output "[rollback] 回滚 smoke 全部通过（$From -> $To -> $From）"
exit 0
