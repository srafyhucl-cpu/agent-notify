#Requires -Version 5.1
<#
.SYNOPSIS
  在不触网、不修改生产数据的前提下，验证真实生产数据库快照的退出与重启语义。

.DESCRIPTION
  脚本把生产 state.db 通过 SQLite backup API 复制到 D:\Temp，关闭副本中的渠道账号并
  启用暂停状态，然后连续启动两次桌面 smoke。它会核对迁移只执行一次、历史/Route/Claim/
  Outbox/账号身份保持不变、每次退出均完成 WAL checkpoint，且没有残留进程。

  该脚本只操作明确创建在 D:\Temp 下的副本；源数据库始终以只读方式打开。
#>
[CmdletBinding()]
param(
  [string]$SourceDataDir = '',
  [string]$DesktopExe = '',
  [string]$Sqlite3 = '',
  [string]$WorkRoot = '',
  [int]$ExitAfterMs = 6000,
  [int]$TimeoutSeconds = 30,
  [switch]$KeepWorkRoot
)

$ErrorActionPreference = 'Stop'

function Resolve-FirstLeaf {
  param([string[]]$Candidates)
  foreach ($candidate in $Candidates) {
    if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }
  return ''
}

function Invoke-Sqlite {
  param(
    [string]$DatabasePath,
    [string]$Sql,
    [switch]$ReadOnly
  )

  $arguments = @()
  if ($ReadOnly) { $arguments += '-readonly' }
  $arguments += @($DatabasePath, $Sql)
  $output = @(& $Sqlite3 @arguments)
  if ($LASTEXITCODE -ne 0) {
    throw "SQLite 执行失败 exit=$LASTEXITCODE sql=$Sql"
  }
  return $output
}

function Get-TextHash {
  param([string[]]$Lines)

  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($Lines -join "`n"))
    return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '')
  } finally {
    $sha.Dispose()
  }
}

function Get-QueryHash {
  param(
    [string]$DatabasePath,
    [string]$Sql,
    [switch]$ReadOnly
  )

  $rows = @(Invoke-Sqlite -DatabasePath $DatabasePath -Sql $Sql -ReadOnly:$ReadOnly)
  return Get-TextHash -Lines $rows
}

function Get-DatabaseSnapshot {
  param(
    [string]$DatabasePath,
    [switch]$ReadOnly
  )

  $migrations = @(Invoke-Sqlite -DatabasePath $DatabasePath -ReadOnly:$ReadOnly `
      -Sql 'select version || ''|'' || checksum || ''|'' || applied_at from schema_migrations order by version;')
  $counts = @(Invoke-Sqlite -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select 'notifications=' || count(*) from notifications;
select 'deliveries=' || count(*) from deliveries;
select 'reply_routes=' || count(*) from reply_routes;
select 'inbound_claims=' || count(*) from inbound_claims;
select 'outbox=' || count(*) from outbox;
select 'channel_accounts=' || count(*) from channel_accounts;
select 'settings=' || count(*) from settings;
'@)

  return [pscustomobject]@{
    Migrations = $migrations -join "`n"
    Counts = $counts -join "`n"
    Notifications = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(notification_id) || '|' || quote(agent_id) || '|' || quote(ingest_key) || '|' ||
       quote(session_id) || '|' || quote(title) || '|' || quote(body)
from notifications order by notification_id;
'@
    Deliveries = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(delivery_id) || '|' || quote(notification_id) || '|' || quote(channel_id) || '|' ||
       quote(account_id) || '|' || quote(state) || '|' || quote(external_message_id) || '|' ||
       quote(error_code) || '|' || quote(error_message) || '|' || retryable || '|' || attempt_count
from deliveries order by delivery_id;
'@
    ReplyRoutes = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(channel_id) || '|' || quote(account_id) || '|' || quote(external_message_id) || '|' ||
       quote(agent_id) || '|' || quote(session_id)
from reply_routes order by channel_id, account_id, external_message_id;
'@
    InboundClaims = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(claim_key) || '|' || quote(channel_id) || '|' || quote(account_id) || '|' ||
       quote(external_message_id) || '|' || quote(state) || '|' || quote(error_code) || '|' ||
       quote(error_message)
from inbound_claims order by claim_key;
'@
    Outbox = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(outbox_id) || '|' || quote(notification_id) || '|' || quote(state) || '|' ||
       quote(lease_owner) || '|' || attempt_count || '|' || quote(last_error_code) || '|' ||
       quote(last_error_message)
from outbox order by outbox_id;
'@
    Accounts = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(account_id) || '|' || quote(channel_id) || '|' || quote(display_name) || '|' ||
       enabled || '|' || quote(config_json) || '|' || quote(secret_ref) || '|' || quote(cursor_json)
from channel_accounts order by account_id;
'@
    Settings = Get-QueryHash -DatabasePath $DatabasePath -ReadOnly:$ReadOnly -Sql @'
select quote(key) || '|' || quote(value_json) from settings order by key;
'@
  }
}

function Assert-SnapshotEqual {
  param(
    [pscustomobject]$Expected,
    [pscustomobject]$Actual,
    [string[]]$Properties,
    [string]$Label
  )

  foreach ($property in $Properties) {
    if ($Expected.$property -ne $Actual.$property) {
      throw "$Label 不一致：$property"
    }
  }
}

function Start-SmokeRun {
  param(
    [string]$Label,
    [string]$DesktopPath,
    [int]$WaitSeconds,
    [string]$LogPath
  )

  $process = Start-Process -FilePath $DesktopPath -PassThru -WindowStyle Hidden
  if (-not $process.WaitForExit($WaitSeconds * 1000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    throw "$Label 在 $WaitSeconds 秒内未退出"
  }
  if ($process.ExitCode -ne 0) {
    throw "$Label 退出码异常：$($process.ExitCode)"
  }

  $walPath = Join-Path (Split-Path $LogPath -Parent) '..\data\state.db-wal'
  $walPath = [IO.Path]::GetFullPath($walPath)
  if (-not (Test-Path -LiteralPath $walPath -PathType Leaf)) {
    throw "$Label 未生成 WAL 文件：$walPath"
  }
  if ((Get-Item -LiteralPath $walPath).Length -ne 0) {
    throw "$Label 退出后 WAL 未 checkpoint：$((Get-Item -LiteralPath $walPath).Length) 字节"
  }

  $remaining = @(Get-CimInstance Win32_Process -Filter "Name = 'agentnotify-desktop.exe'" |
      Where-Object { $_.ExecutablePath -and $_.ExecutablePath.Equals($DesktopPath, [StringComparison]::OrdinalIgnoreCase) })
  if ($remaining.Count -ne 0) {
    throw "$Label 退出后仍有残留进程：$($remaining.ProcessId -join ',')"
  }

  return [pscustomobject]@{
    ExitCode = $process.ExitCode
    WalBytes = (Get-Item -LiteralPath $walPath).Length
  }
}

if ([string]::IsNullOrWhiteSpace($SourceDataDir)) {
  $SourceDataDir = Join-Path $env:LOCALAPPDATA 'AgentNotify\data'
}
if ([string]::IsNullOrWhiteSpace($DesktopExe)) {
  $DesktopExe = Resolve-FirstLeaf @(
    'D:\app\AgentNotify-Rust-Preview\agentnotify-desktop.exe',
    (Join-Path $env:LOCALAPPDATA 'Programs\AgentNotify-Rust-Preview\agentnotify-desktop.exe')
  )
}
if ([string]::IsNullOrWhiteSpace($Sqlite3)) {
  $Sqlite3 = Resolve-FirstLeaf @(
    (Get-Command sqlite3.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\sqlite3.exe')
  )
}

$sourceDb = Join-Path $SourceDataDir 'state.db'
if (-not (Test-Path -LiteralPath $sourceDb -PathType Leaf)) {
  throw "找不到源数据库：$sourceDb"
}
if (-not (Test-Path -LiteralPath $DesktopExe -PathType Leaf)) {
  throw "找不到桌面程序：$DesktopExe"
}
if (-not (Test-Path -LiteralPath $Sqlite3 -PathType Leaf)) {
  throw "找不到 sqlite3.exe：$Sqlite3"
}

$running = @(Get-Process -Name 'agentnotify-desktop' -ErrorAction SilentlyContinue)
if ($running.Count -ne 0) {
  throw '检测到正在运行的 AgentNotify 桌面实例，请先退出后再执行重启验收。'
}

$tempRoot = [IO.Path]::GetFullPath('D:\Temp\')
if ([string]::IsNullOrWhiteSpace($WorkRoot)) {
  $WorkRoot = Join-Path $tempRoot ('agentnotify-restart-acceptance-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
$resolvedWorkRoot = [IO.Path]::GetFullPath($WorkRoot)
if (-not $resolvedWorkRoot.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
  throw "重启验收目录必须位于 D:\Temp 下：$resolvedWorkRoot"
}
if (Test-Path -LiteralPath $resolvedWorkRoot) {
  throw "重启验收目录已存在，拒绝覆盖：$resolvedWorkRoot"
}

$configDir = Join-Path $resolvedWorkRoot 'config'
$dataDir = Join-Path $resolvedWorkRoot 'data'
$logDir = Join-Path $resolvedWorkRoot 'logs'
$spoolDir = Join-Path $resolvedWorkRoot 'spool'
$targetDb = Join-Path $dataDir 'state.db'
$targetLog = Join-Path $logDir 'runtime.log'

$originalEnvironment = @{}
$environmentNames = @(
  'AGENT_NOTIFY_CONFIG_DIR',
  'AGENT_NOTIFY_DATA_DIR',
  'AGENT_NOTIFY_LOG_DIR',
  'AGENT_NOTIFY_SPOOL_DIR',
  'AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS'
)
foreach ($name in $environmentNames) {
  $originalEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

try {
  foreach ($directory in @($configDir, $dataDir, $logDir, $spoolDir)) {
    New-Item -ItemType Directory -Path $directory | Out-Null
  }

  $backupTarget = $targetDb.Replace("'", "''")
  $backupOutput = @(& $Sqlite3 $sourceDb ".backup '$backupTarget'")
  if ($LASTEXITCODE -ne 0) {
    throw "复制生产数据库失败 exit=$LASTEXITCODE $($backupOutput -join ' ')"
  }

  # 验收只关心重启语义，禁止副本访问平台或处理积压投递。
  $disable = @'
update channel_accounts set enabled = 0;
insert into settings(key, value_json, updated_at)
values('notificationsPaused', 'true', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
on conflict(key) do update set value_json = 'true', updated_at = excluded.updated_at;
update settings set value_json = 'false' where key = 'reply.enabled';
'@
  [void](Invoke-Sqlite -DatabasePath $targetDb -Sql $disable)
  [void](Invoke-Sqlite -DatabasePath $targetDb -Sql 'pragma wal_checkpoint(truncate);')

  $before = Get-DatabaseSnapshot -DatabasePath $targetDb

  $env:AGENT_NOTIFY_CONFIG_DIR = $configDir
  $env:AGENT_NOTIFY_DATA_DIR = $dataDir
  $env:AGENT_NOTIFY_LOG_DIR = $logDir
  $env:AGENT_NOTIFY_SPOOL_DIR = $spoolDir
  $env:AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS = [string]$ExitAfterMs

  $first = Start-SmokeRun -Label '第一次启动' -DesktopPath $DesktopExe `
    -WaitSeconds $TimeoutSeconds -LogPath $targetLog
  $afterFirst = Get-DatabaseSnapshot -DatabasePath $targetDb
  $migrationVersions = @(Invoke-Sqlite -DatabasePath $targetDb -Sql 'select version from schema_migrations order by version;')
  if ($migrationVersions -notcontains '2') {
    throw "第一次启动未应用迁移 0002，当前版本：$($migrationVersions -join ',')"
  }
  Assert-SnapshotEqual -Expected $before -Actual $afterFirst -Label '第一次启动后' -Properties @(
    'Counts', 'Notifications', 'Deliveries', 'ReplyRoutes', 'InboundClaims', 'Outbox', 'Accounts', 'Settings'
  )

  $second = Start-SmokeRun -Label '第二次启动' -DesktopPath $DesktopExe `
    -WaitSeconds $TimeoutSeconds -LogPath $targetLog
  $afterSecond = Get-DatabaseSnapshot -DatabasePath $targetDb
  Assert-SnapshotEqual -Expected $afterFirst -Actual $afterSecond -Label '第二次启动后' -Properties @(
    'Migrations', 'Counts', 'Notifications', 'Deliveries', 'ReplyRoutes', 'InboundClaims', 'Outbox', 'Accounts', 'Settings'
  )

  $logText = Get-Content -LiteralPath $targetLog -Raw -Encoding UTF8
  if (($logText | Select-String -Pattern '启动桌面运行时' -AllMatches).Matches.Count -ne 2) {
    throw 'runtime.log 未记录两次桌面运行时启动'
  }
  if (($logText | Select-String -Pattern '停止桌面运行时' -AllMatches).Matches.Count -ne 2) {
    throw 'runtime.log 未记录两次桌面运行时停止'
  }

  Write-Output 'restartAcceptance=PASS'
  Write-Output "sourceDb=$sourceDb"
  Write-Output "sourceDbSha256=$((Get-FileHash -Algorithm SHA256 -LiteralPath $sourceDb).Hash)"
  Write-Output "desktopSha256=$((Get-FileHash -Algorithm SHA256 -LiteralPath $DesktopExe).Hash)"
  Write-Output "migrations=$($migrationVersions -join ',')"
  Write-Output ($afterSecond.Counts -replace "`r?`n", ';')
  Write-Output "firstExitCode=$($first.ExitCode)"
  Write-Output "firstWalBytes=$($first.WalBytes)"
  Write-Output "secondExitCode=$($second.ExitCode)"
  Write-Output "secondWalBytes=$($second.WalBytes)"
  Write-Output 'network=disabled_by_accounts_and_pause'
  Write-Output "workRoot=$resolvedWorkRoot"
} finally {
  foreach ($name in $environmentNames) {
    $original = $originalEnvironment[$name]
    if ($null -eq $original) {
      Remove-Item -LiteralPath ("Env:\" + $name) -ErrorAction SilentlyContinue
    } else {
      Set-Item -LiteralPath ("Env:\" + $name) -Value $original
    }
  }
  if (-not $KeepWorkRoot -and (Test-Path -LiteralPath $resolvedWorkRoot -PathType Container)) {
    $resolvedCleanup = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $resolvedWorkRoot).Path)
    if (-not $resolvedCleanup.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
      throw "拒绝清理 D:\Temp 之外的路径：$resolvedCleanup"
    }
    [IO.Directory]::Delete($resolvedCleanup, $true)
  }
}
