#Requires -Version 5.1
<#
.SYNOPSIS
  准备「渠道失效提示」界面验收的隔离环境（Task 9 Step 5 的界面展示项）。

.DESCRIPTION
  从生产库做 SQLite .backup 副本放进隔离根，并在副本里把 ClawBot 账号置为
  「已停用 + stale」。这样：

  - inspect() 先查凭据、再查 stale_at，因此界面会显示真实的
    「ClawBot 会话已失效，请重新扫码或发送消息恢复」；
  - 运行时的 enabled_accounts 只为启用账号启动长轮询，账号已停用则不会轮询，
    因此不会与生产实例争抢入站消息，也不写凭据、不访问平台。

  本脚本只读生产库、只写隔离根；不修改生产库、不写 Windows 凭据、不访问网络。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\stale-ui-acceptance.ps1 -Prepare
  powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\stale-ui-acceptance.ps1 -Clean
#>
param(
  [switch]$Prepare,
  [switch]$Clean,
  [string]$Root = 'D:\Temp\agentnotify-stale-ui',
  [string]$Sqlite3 = '',
  [string]$DesktopExe = ''
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

function Assert-IsolatedRoot {
  param([string]$Path)
  $full = [IO.Path]::GetFullPath($Path)
  if (-not $full.StartsWith('D:\Temp\agentnotify-', [StringComparison]::OrdinalIgnoreCase)) {
    throw "隔离根必须位于 D:\Temp\agentnotify- 之下，当前为：$full"
  }
  return $full
}

$productionDataDir = Join-Path $env:LOCALAPPDATA 'AgentNotify\data'
$productionDb = Join-Path $productionDataDir 'state.db'
$resolvedRoot = Assert-IsolatedRoot -Path $Root

if ([string]::IsNullOrWhiteSpace($Sqlite3)) {
  $Sqlite3 = Resolve-FirstLeaf @(
    (Get-Command sqlite3.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\sqlite3.exe')
  )
}
if ([string]::IsNullOrWhiteSpace($DesktopExe)) {
  $DesktopExe = Resolve-FirstLeaf @(
    'D:\app\AgentNotify-Rust-Preview\agentnotify-desktop.exe',
    (Join-Path $env:LOCALAPPDATA 'Programs\Agent-notify\agentnotify-desktop.exe')
  )
}

if ($Clean) {
  if (Test-Path -LiteralPath $resolvedRoot) {
    [IO.Directory]::Delete($resolvedRoot, $true)
    Write-Output "已清理隔离根：$resolvedRoot"
  } else {
    Write-Output "隔离根不存在，无需清理：$resolvedRoot"
  }
  exit 0
}

if (-not $Prepare) {
  throw '请指定 -Prepare 或 -Clean。'
}
if ([string]::IsNullOrWhiteSpace($Sqlite3)) {
  throw '找不到 sqlite3.exe，无法备份生产库。'
}
if (-not (Test-Path -LiteralPath $productionDb -PathType Leaf)) {
  throw "找不到生产库：$productionDb"
}
if ([string]::IsNullOrWhiteSpace($DesktopExe)) {
  throw '找不到预览桌面二进制，无法生成启动器。'
}

$configDir = Join-Path $resolvedRoot 'config'
$dataDir = Join-Path $resolvedRoot 'data'
$logDir = Join-Path $resolvedRoot 'logs'
$spoolDir = Join-Path $resolvedRoot 'spool'
foreach ($directory in @($configDir, $dataDir, $logDir, $spoolDir)) {
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
}

$isolatedDb = Join-Path $dataDir 'state.db'
if (Test-Path -LiteralPath $isolatedDb) {
  Remove-Item -LiteralPath $isolatedDb -Force
}

# 生产库正在被桌面端使用，必须用 SQLite .backup 取一致快照，不能直接复制文件。
$backupSource = $productionDb.Replace('\', '/')
$backupTarget = $isolatedDb.Replace('\', '/')
& $Sqlite3 $productionDb ".backup '$backupTarget'"
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $isolatedDb)) {
  throw "生产库快照失败 exit=$LASTEXITCODE"
}

# 隔离副本里的改造：
# 1) 账号停用，确保运行时不启动长轮询（不会与生产争抢入站消息）
# 2) 账号置 stale，使 inspect() 返回 clawbot_session_stale 与可读提示
# 3) 保留通知暂停与引用回复关闭，避免任何出站动作
$apply = @'
update channel_accounts set enabled = 0;
update channel_accounts
set config_json = replace(
  config_json,
  'stale_at' || char(34) || ':null',
  'stale_at' || char(34) || ':[2026,264,18,0,0,0,0,0,0]'
);
insert into settings(key, value_json, updated_at)
values('notificationsPaused', 'true', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
on conflict(key) do update set value_json = 'true', updated_at = excluded.updated_at;
update settings set value_json = 'false' where key = 'reply.enabled';
'@
& $Sqlite3 $isolatedDb $apply
if ($LASTEXITCODE -ne 0) {
  throw "隔离副本改造失败 exit=$LASTEXITCODE"
}

$checkSql = @'
select config_json from channel_accounts;
'@
$staleCount = 0
$accountRows = @(& $Sqlite3 -readonly $isolatedDb $checkSql)
if ($accountRows.Count -eq 0) {
  throw '隔离副本里没有读取到任何账号，验收环境无意义。'
}
foreach ($row in $accountRows) {
  if ([string]::IsNullOrWhiteSpace($row)) { continue }
  $parsed = $row | ConvertFrom-Json
  if ($null -ne $parsed.stale_at) { $staleCount++ }
}
$enabledCount = (& $Sqlite3 -readonly $isolatedDb 'select count(*) from channel_accounts where enabled = 1;').Trim()
if ($staleCount -ne $accountRows.Count) {
  throw "隔离副本应让全部 $($accountRows.Count) 个账号都 stale，实际只有 $staleCount 个；界面验收会看不到失效提示。"
}
if ($enabledCount -ne '0') {
  throw "隔离副本里仍有 $enabledCount 个启用账号，运行时会启动长轮询并与生产争抢入站消息。"
}

$launcher = Join-Path $resolvedRoot 'start-isolated.cmd'
$launcherLines = @(
  '@echo off',
  'REM Task 9 Step 5 界面验收用的隔离实例启动器。',
  'REM 启动前请在托盘退出正在运行的生产实例：单实例互斥，否则本实例会直接退出。',
  'REM 请用资源管理器双击本文件，或在任意会话执行：explorer.exe "<本文件路径>"。',
  "set AGENT_NOTIFY_CONFIG_DIR=$configDir",
  "set AGENT_NOTIFY_DATA_DIR=$dataDir",
  "set AGENT_NOTIFY_LOG_DIR=$logDir",
  "set AGENT_NOTIFY_SPOOL_DIR=$spoolDir",
  "start `"`" `"$DesktopExe`""
)
[IO.File]::WriteAllText($launcher, ($launcherLines -join "`r`n") + "`r`n", (New-Object Text.UTF8Encoding($false)))

Write-Output "[stale-ui] 隔离根：$resolvedRoot"
Write-Output "[stale-ui] 生产库快照：$isolatedDb"
Write-Output "[stale-ui] 账号总数：$staleCount 个已置 stale；启用账号：$enabledCount 个（应为 0）"
Write-Output "[stale-ui] 启动器：$launcher"
Write-Output ''
Write-Output '下一步：'
Write-Output '  1. 右键托盘图标 → 退出，停掉正在运行的生产实例'
Write-Output "  2. explorer.exe `"$launcher`""
Write-Output '  3. 打开「渠道」页，查看账号状态标签与明细文案'
Write-Output '  4. 看完后在托盘退出该隔离实例，再重新启动生产实例'
Write-Output "  5. 清理：powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\stale-ui-acceptance.ps1 -Clean"
