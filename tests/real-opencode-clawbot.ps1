#Requires -Version 5.1
<#
.SYNOPSIS
  真实 OpenCode + ClawBot 闭环验收探针。

.DESCRIPTION
  该脚本只读取 AgentNotify SQLite 状态并调用既有 ingress、OpenCode CLI，
  不启动或停止用户正在运行的桌面端，不修改用户配置。

  Mode:
    Status       输出指定会话最近的推送、路由和 Claim 证据。
    Send         通过 ingress 提交一条测试事件，并等待 Delivery 终态。
    VerifyReply  等待引用回复 Claim 完成，并确认回复文本已进入 OpenCode 会话。
                 传入 -OtherSessionId 时会额外确认对照会话没有收到同一文本。
    LocateReply  用唯一回复文本在最近的路由候选里反查命中的会话，并给出可直接复制的 VerifyReply 命令。
    Prepare      建立隔离验收目录并输出启动参数，不读取也不修改生产数据。
#>
[CmdletBinding()]
param(
  [ValidateSet('Prepare', 'Status', 'Send', 'VerifyReply', 'LocateReply')]
  [string]$Mode = 'Status',
  [string]$SessionId = '',
  [string]$OtherSessionId = '',
  [string]$ReplyText = 'AGENT_NOTIFY_REPLY_OK',
  [string]$ExternalMessageId = '',
  [string]$DataDir = '',
  [string]$IngressPath = '',
  [string]$OpenCodeCli = '',
  [string]$Sqlite3 = '',
  [string]$TargetAccountId = '',
  [switch]$InstallPlugin,
  [switch]$UseProductionDataDir,
  [int]$TimeoutSeconds = 180,
  # LocateReply 反查的时间窗与候选上限；默认覆盖 24 小时路由 TTL 加余量。
  [int]$IntervalHours = 30,
  [int]$MaxCandidates = 12
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($DataDir)) {
  $DataDir = Join-Path $env:LOCALAPPDATA 'AgentNotify\data'
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

# OpenCode 桌面端把 CLI 放在 %APPDATA%\ai.opencode.desktop\cli\<版本>\opencode-cli.exe，
# 版本目录会随升级新增；这里取版本号最大的一个，避免写死版本号后升级即失效。
function Resolve-OpenCodeDesktopCli {
  $root = Join-Path $env:APPDATA 'ai.opencode.desktop\cli'
  if (-not (Test-Path -LiteralPath $root)) { return '' }
  $directories = @(Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
    Sort-Object -Property @{ Expression = { try { [version]$_.Name } catch { [version]'0.0.0' } } } -Descending)
  foreach ($directory in $directories) {
    $leaf = Join-Path $directory.FullName 'opencode-cli.exe'
    if (Test-Path -LiteralPath $leaf -PathType Leaf) { return $leaf }
  }
  return ''
}

if ([string]::IsNullOrWhiteSpace($IngressPath)) {
  $IngressPath = Resolve-FirstLeaf @(
    (Join-Path $env:USERPROFILE 'bin\agentnotify-ingress.exe'),
    'D:\app\AgentNotify-Rust-Preview\agentnotify-ingress.exe'
  )
}
if ([string]::IsNullOrWhiteSpace($OpenCodeCli)) {
  $OpenCodeCli = Resolve-FirstLeaf @(
    (Resolve-OpenCodeDesktopCli),
    (Get-Command opencode-cli.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    (Get-Command opencode -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1)
  )
}
if ([string]::IsNullOrWhiteSpace($Sqlite3)) {
  $Sqlite3 = Resolve-FirstLeaf @(
    (Get-Command sqlite3.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\sqlite3.exe')
  )
}

$repositoryRoot = Split-Path $PSScriptRoot -Parent
$productionDataDir = Join-Path $env:LOCALAPPDATA 'AgentNotify\data'
$openCodePluginPath = Join-Path $env:USERPROFILE '.config\opencode\plugins\agent-notify.ts'
$previewDesktopExe = Resolve-FirstLeaf @(
  (Join-Path $env:LOCALAPPDATA 'Programs\AgentNotify-Rust-Preview\agentnotify-desktop.exe'),
  'D:\app\AgentNotify-Rust-Preview\agentnotify-desktop.exe'
)

function Get-IsolatedRoots {
  param([string]$RequestedDataDir)

  $dataDir = $env:AGENT_NOTIFY_DATA_DIR
  if ([string]::IsNullOrWhiteSpace($dataDir)) { $dataDir = $RequestedDataDir }
  if ([string]::IsNullOrWhiteSpace($dataDir)) {
    throw 'Prepare 模式需要隔离根目录：先设置 AGENT_NOTIFY_DATA_DIR，或传入 -DataDir。'
  }

  $resolvedData = [IO.Path]::GetFullPath($dataDir)
  $root = Split-Path $resolvedData -Parent
  $configDir = $env:AGENT_NOTIFY_CONFIG_DIR
  if ([string]::IsNullOrWhiteSpace($configDir)) { $configDir = Join-Path $root 'config' }
  $logDir = $env:AGENT_NOTIFY_LOG_DIR
  if ([string]::IsNullOrWhiteSpace($logDir)) { $logDir = Join-Path $root 'logs' }
  $spoolDir = $env:AGENT_NOTIFY_SPOOL_DIR
  if ([string]::IsNullOrWhiteSpace($spoolDir)) { $spoolDir = Join-Path $root 'spool' }

  return [pscustomobject]@{
    Root = $root
    Config = [IO.Path]::GetFullPath($configDir)
    Data = $resolvedData
    Log = [IO.Path]::GetFullPath($logDir)
    Spool = [IO.Path]::GetFullPath($spoolDir)
  }
}

# 插件模板把 ingress 绝对路径烘焙成 JS 字面量，这里反解出来与当前 ingress 对比。
function Get-BakedPluginIngress {
  param([string]$Path)

  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return '' }
  $match = Select-String -LiteralPath $Path -Pattern '^const BAKED_INGRESS = "(.*)"\s*;?\s*$' | Select-Object -First 1
  if (-not $match) { return '' }
  return $match.Matches[0].Groups[1].Value.Replace('\\', '\')
}

if ($Mode -eq 'Prepare') {
  $roots = Get-IsolatedRoots -RequestedDataDir $DataDir
  $isProduction = $roots.Data.TrimEnd('\').Equals($productionDataDir.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
  if ($isProduction -and -not $UseProductionDataDir) {
    throw "拒绝把生产数据目录当作隔离验收根目录：$productionDataDir。请先把 AGENT_NOTIFY_DATA_DIR 指向 D:\Temp 下的独立目录，确需使用生产目录时显式加 -UseProductionDataDir。"
  }

  foreach ($directory in @($roots.Config, $roots.Data, $roots.Log, $roots.Spool)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
  }

  if ($InstallPlugin) {
    $pluginInstaller = Join-Path $repositoryRoot 'tools\hooks\install-opencode-v2.ps1'
    $pluginSource = Join-Path $repositoryRoot 'plugin\rust\agent-notify.ts'
    if (-not (Test-Path -LiteralPath $pluginInstaller -PathType Leaf)) { throw "找不到插件安装助手：$pluginInstaller" }
    if (-not (Test-Path -LiteralPath $pluginSource -PathType Leaf)) { throw "找不到插件模板：$pluginSource" }
    if ([string]::IsNullOrWhiteSpace($IngressPath)) { throw '安装插件需要 -IngressPath 指向 Rust ingress。' }
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $pluginInstaller `
      -Source $pluginSource `
      -Destination $openCodePluginPath `
      -Ingress $IngressPath
    if ($LASTEXITCODE -ne 0) { throw "安装 OpenCode 插件失败 exit=$LASTEXITCODE" }
  }

  $pluginIngress = Get-BakedPluginIngress -Path $openCodePluginPath
  $runningDesktop = @(Get-Process -Name 'agentnotify-desktop' -ErrorAction SilentlyContinue)
  $isolatedDb = Join-Path $roots.Data 'state.db'

  Write-Output 'prepare=READY'
  Write-Output "acceptanceRoot=$($roots.Root)"
  Write-Output "isolated=$(-not $isProduction)"
  Write-Output "configDir=$($roots.Config)"
  Write-Output "dataDir=$($roots.Data)"
  Write-Output "logDir=$($roots.Log)"
  Write-Output "spoolDir=$($roots.Spool)"
  Write-Output "stateDbExists=$(Test-Path -LiteralPath $isolatedDb -PathType Leaf)"
  Write-Output "ingressPath=$IngressPath"
  if (Test-Path -LiteralPath $IngressPath -PathType Leaf) {
    Write-Output "ingressSha256=$((Get-FileHash -Algorithm SHA256 -LiteralPath $IngressPath).Hash)"
  }
  if ($previewDesktopExe) {
    Write-Output "desktopExe=$previewDesktopExe"
    Write-Output "desktopSha256=$((Get-FileHash -Algorithm SHA256 -LiteralPath $previewDesktopExe).Hash)"
  } else {
    Write-Output 'desktopExe=MISSING'
  }
  Write-Output "pluginPath=$openCodePluginPath"
  if ($pluginIngress) { Write-Output "pluginBakedIngress=$pluginIngress" } else { Write-Output 'pluginBakedIngress=UNKNOWN' }
  Write-Output "pluginMatchesIngress=$(($pluginIngress -ne '') -and $pluginIngress.Equals($IngressPath, [StringComparison]::OrdinalIgnoreCase))"
  Write-Output "runningDesktopInstances=$($runningDesktop.Count)"
  Write-Output 'env:'
  Write-Output "  `$env:AGENT_NOTIFY_CONFIG_DIR = '$($roots.Config)'"
  Write-Output "  `$env:AGENT_NOTIFY_DATA_DIR = '$($roots.Data)'"
  Write-Output "  `$env:AGENT_NOTIFY_LOG_DIR = '$($roots.Log)'"
  Write-Output "  `$env:AGENT_NOTIFY_SPOOL_DIR = '$($roots.Spool)'"
  Write-Output 'next=1) 退出正在运行的 OpenCode 桌面端与生产预览实例；'
  Write-Output 'next=2) 在同一个已设置上述环境变量的窗口里启动 OpenCode 与预览桌面程序，否则插件会把事件写进生产 spool；'
  Write-Output 'next=3) 在 Channels 中扫码绑定另一个微信用户账号（换 Bot 或重绑同一账号无效）并先发一条消息建立会话；'
  Write-Output 'next=4) 在 OpenCode 中开两个会话，分别作为 target 与 control；'
  Write-Output 'next=5) 执行 -Mode Send，微信确认收到后引用回复，再用 -Mode VerifyReply -OtherSessionId 验收。'
  exit 0
}

$stateDb = Join-Path $DataDir 'state.db'
if (-not (Test-Path -LiteralPath $stateDb -PathType Leaf)) {
  throw "找不到 AgentNotify SQLite：$stateDb"
}
if (-not (Test-Path -LiteralPath $IngressPath -PathType Leaf)) {
  throw "找不到 agentnotify-ingress.exe：$IngressPath"
}
if (-not (Test-Path -LiteralPath $OpenCodeCli -PathType Leaf)) {
  throw "找不到 opencode-cli.exe：$OpenCodeCli"
}
if (-not (Test-Path -LiteralPath $Sqlite3 -PathType Leaf)) {
  throw "找不到 sqlite3.exe；可通过 -Sqlite3 显式指定。"
}

function Quote-Sql {
  param([string]$Value)
  return $Value.Replace("'", "''")
}

function Invoke-StateQuery {
  param([string]$Sql)
  $rows = @(& $Sqlite3 -readonly -noheader -separator "`t" $stateDb $Sql)
  if ($LASTEXITCODE -ne 0) {
    throw "读取 AgentNotify SQLite 失败 exit=$LASTEXITCODE"
  }
  return $rows
}

function Invoke-IngressEvent {
  param([string]$Json)
  $node = Get-Command node.exe -ErrorAction SilentlyContinue
  if (-not $node) {
    throw '找不到 node.exe，无法可靠地向 GUI 子系统的 ingress 写入 UTF-8 stdin。'
  }
  $previousIngress = $env:AGENT_NOTIFY_E2E_INGRESS
  $previousEvent = $env:AGENT_NOTIFY_E2E_EVENT
  $bridgeRoot = 'D:\Temp\agentnotify-e2e'
  New-Item -ItemType Directory -Force -Path $bridgeRoot | Out-Null
  $bridgePath = Join-Path $bridgeRoot ('ingress-' + [guid]::NewGuid().ToString('N') + '.cjs')
  try {
    $env:AGENT_NOTIFY_E2E_INGRESS = $IngressPath
    $env:AGENT_NOTIFY_E2E_EVENT = $Json
    $bridge = @'
const { spawnSync } = require("node:child_process");
const result = spawnSync(process.env.AGENT_NOTIFY_E2E_INGRESS, [], {
  input: process.env.AGENT_NOTIFY_E2E_EVENT,
  encoding: "utf8",
  windowsHide: true,
  timeout: 30000,
});
if (result.error) {
  process.stderr.write(String(result.error.message));
  process.exit(1);
}
if (result.stderr) process.stderr.write(result.stderr);
process.exit(result.status === null ? 1 : result.status);
'@
    [IO.File]::WriteAllText($bridgePath, $bridge, (New-Object Text.UTF8Encoding($false)))
    & $node.Source $bridgePath
    if ($LASTEXITCODE -ne 0) {
      throw "ingress 提交失败 exit=$LASTEXITCODE"
    }
  } finally {
    Remove-Item -LiteralPath $bridgePath -Force -ErrorAction SilentlyContinue
    # 只在本目录已空时删掉，避免递归删除或以清理为名误删他物。
    if ((Test-Path -LiteralPath $bridgeRoot) -and -not (Get-ChildItem -LiteralPath $bridgeRoot -Force -ErrorAction SilentlyContinue)) {
      [IO.Directory]::Delete($bridgeRoot, $false)
    }
    if ($null -eq $previousIngress) { Remove-Item Env:AGENT_NOTIFY_E2E_INGRESS -ErrorAction SilentlyContinue } else { $env:AGENT_NOTIFY_E2E_INGRESS = $previousIngress }
    if ($null -eq $previousEvent) { Remove-Item Env:AGENT_NOTIFY_E2E_EVENT -ErrorAction SilentlyContinue } else { $env:AGENT_NOTIFY_E2E_EVENT = $previousEvent }
  }
}

function Get-HashText {
  param([string]$Value)
  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Value)
    return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
  } finally {
    $sha.Dispose()
  }
}

# session export 含完整正文，这里只在内存里匹配，不把正文写入输出或磁盘。
$script:sessionExportFailures = @()

function Test-OpenCodeSessionContains {
  param([string]$TargetSessionId, [string]$Needle, [switch]$Tolerant)

  $export = @(& $OpenCodeCli session export $TargetSessionId 2>&1)
  if ($LASTEXITCODE -ne 0) {
    # 批量反查时单个会话导出失败不能中断整轮扫描，但必须记录失败会话。
    if ($Tolerant) {
      $script:sessionExportFailures += (Get-HashText $TargetSessionId)
      return $false
    }
    throw "导出 OpenCode 会话失败 sessionHash=$(Get-HashText $TargetSessionId) exit=$LASTEXITCODE"
  }
  return (($export -join "`n") -match [regex]::Escape($Needle))
}

function Wait-ForState {
  param(
    [scriptblock]$Query,
    [scriptblock]$IsTerminal,
    [string]$Label
  )
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $row = & $Query
    if ($row -and (& $IsTerminal $row)) {
      return $row
    }
    Start-Sleep -Seconds 2
  } while ((Get-Date) -lt $deadline)
  throw "等待 $Label 超过 $TimeoutSeconds 秒仍未完成"
}

function Get-LatestNotification {
  param([string]$Id)
  $escaped = Quote-Sql $Id
  $rows = @(Invoke-StateQuery @"
select n.notification_id, n.session_id, n.ingest_key, d.state, coalesce(d.external_message_id, ''), d.updated_at,
  coalesce(d.channel_id, ''), coalesce(d.account_id, '')
from notifications n
left join deliveries d on d.notification_id = n.notification_id
where n.session_id = '$escaped'
order by n.rowid desc
limit 1;
"@)
  if ($rows.Count -eq 0) { return $null }
  return ($rows[0] -split "`t")
}

# 计划 Step 3 要求 ReplyRoute 用平台返回的稳定 message ID，而不是标题或正文；
# 这里按 (channel_id, account_id, external_message_id) 精确复算路由是否落在投递收到的 ID 上。
function Get-RouteForDelivery {
  param([string[]]$DeliveryRow)

  if ($null -eq $DeliveryRow -or $DeliveryRow.Count -lt 8) { return $null }
  $messageId = $DeliveryRow[4]
  $channelId = $DeliveryRow[6]
  $accountId = $DeliveryRow[7]
  if ([string]::IsNullOrWhiteSpace($messageId) -or [string]::IsNullOrWhiteSpace($channelId) -or [string]::IsNullOrWhiteSpace($accountId)) { return $null }

  $rows = @(Invoke-StateQuery @"
select session_id, agent_id, created_at, expires_at
from reply_routes
where channel_id = '$(Quote-Sql $channelId)'
  and account_id = '$(Quote-Sql $accountId)'
  and external_message_id = '$(Quote-Sql $messageId)'
order by rowid desc
limit 1;
"@)
  if ($rows.Count -eq 0) { return $null }
  return ($rows[0] -split "`t")
}

function Get-LatestRoute {
  param([string]$Id)
  $escaped = Quote-Sql $Id
  $rows = @(Invoke-StateQuery @"
select channel_id, account_id, external_message_id, created_at, expires_at
from reply_routes
where session_id = '$escaped'
order by rowid desc
limit 1;
"@)
  if ($rows.Count -eq 0) { return $null }
  return ($rows[0] -split "`t")
}

# 引用回复可能晚于投递数小时：按时间窗取候选会话，供 LocateReply 逐个反查。
function Get-RecentRoutes {
  param([int]$Hours, [int]$Limit)

  $rows = @(Invoke-StateQuery @"
select session_id, channel_id, account_id, created_at, expires_at, external_message_id
from reply_routes
where julianday(created_at) >= julianday('now') - ($Hours / 24.0)
order by created_at desc
limit $Limit;
"@)
  return $rows
}

# Claim 只记录回复消息自身的 ID，不记录被引用消息 ID；
# 因此用渠道账号加路由创建时间窗关联，再用唯一回复文本坐实落到目标会话。
function Get-CompletedClaimSince {
  param([string]$ChannelId, [string]$AccountId, [string]$Since)
  $escapedChannel = Quote-Sql $ChannelId
  $escapedAccount = Quote-Sql $AccountId
  $escapedSince = Quote-Sql $Since
  $rows = @(Invoke-StateQuery @"
select state, coalesce(error_code, ''), updated_at
from inbound_claims
where channel_id = '$escapedChannel' and account_id = '$escapedAccount'
  and state = 'Completed'
  and julianday(updated_at) >= julianday('$escapedSince')
order by updated_at desc
limit 1;
"@)
  if ($rows.Count -eq 0) { return $null }
  return ($rows[0] -split "`t")
}

function Get-LatestClaimSince {
  param([string]$ChannelId, [string]$AccountId, [string]$Since)
  $escapedChannel = Quote-Sql $ChannelId
  $escapedAccount = Quote-Sql $AccountId
  $escapedSince = Quote-Sql $Since
  $rows = @(Invoke-StateQuery @"
select state, coalesce(error_code, ''), updated_at
from inbound_claims
where channel_id = '$escapedChannel' and account_id = '$escapedAccount'
  and julianday(updated_at) >= julianday('$escapedSince')
order by updated_at desc
limit 1;
"@)
  if ($rows.Count -eq 0) { return $null }
  return ($rows[0] -split "`t")
}

if ($Mode -eq 'Send') {
  if ([string]::IsNullOrWhiteSpace($SessionId)) {
    throw 'Send 模式必须提供 -SessionId。'
  }

  $requestId = [guid]::NewGuid().ToString()
  $idempotencyKey = 'opencode:{0}:real-e2e:{1}' -f $SessionId, $requestId
  $metadata = @{
    acceptance = 'real-opencode-clawbot'
  }
  if (-not [string]::IsNullOrWhiteSpace($TargetAccountId)) {
    $metadata.targetAccountId = $TargetAccountId
  }
  $event = @{
    protocolVersion = 1
    kind = 'agent.event'
    requestId = $requestId
    agentId = 'opencode'
    payload = @{
      eventType = 'session.completed'
      idempotencyKey = $idempotencyKey
      occurredAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
      sessionId = $SessionId
      title = '【opencode】AgentNotify 真实闭环验收'
      body = "请在微信中引用本条消息并回复：$ReplyText"
      metadata = $metadata
    }
  }
  $json = $event | ConvertTo-Json -Compress -Depth 8
  Invoke-IngressEvent -Json $json

  $row = Wait-ForState `
    -Label '推送投递终态' `
    -Query {
      $escaped = Quote-Sql $idempotencyKey
      $rows = @(Invoke-StateQuery @"
select n.notification_id, n.session_id, d.state, coalesce(d.external_message_id, ''), d.updated_at
from notifications n
left join deliveries d on d.notification_id = n.notification_id
where n.ingest_key = '$escaped'
order by n.rowid desc
limit 1;
"@)
      if ($rows.Count -eq 0) { return $null }
      return $rows[0]
    } `
    -IsTerminal {
      param($value)
      $state = ($value -split "`t")[2]
      return $state -in @('Sent', 'Failed', 'Unknown', 'Skipped')
    }

  $parts = $row -split "`t"
  if ($parts[2] -ne 'Sent') {
    throw "推送没有被 ClawBot 受理，状态=$($parts[2])"
  }
  # ClawBot 的 Sent 仅代表平台受理并返回消息 ID，不能证明微信端已展示。
  $routeRow = Wait-ForState `
    -Label 'ReplyRoute 建立' `
    -Query {
      $current = Get-LatestNotification $SessionId
      if (-not $current) { return $null }
      return (Get-RouteForDelivery -DeliveryRow $current)
    } `
    -IsTerminal {
      param($value)
      return $null -ne $value
    }

  # 计划 Step 3 第 4 条：路由必须挂在 ClawBot 返回的稳定 message ID 上，而不是标题或正文。
  if ($routeRow[0] -ne $parts[1]) {
    throw "ReplyRoute 未落在目标会话：routeSessionHash=$(Get-HashText $routeRow[0]) expectedSessionHash=$(Get-HashText $parts[1])"
  }
  if ($routeRow[1] -ne 'opencode') {
    throw "ReplyRoute 的 agent 不是 opencode：agent=$($routeRow[1])"
  }
  $routeExpiresAt = [datetime]::MinValue
  if (-not [datetime]::TryParse($routeRow[3], [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$routeExpiresAt)) {
    throw "无法解析 ReplyRoute 过期时间：$($routeRow[3])"
  }
  if ($routeExpiresAt -le [datetime]::UtcNow) {
    throw "ReplyRoute 已过期，无法用于引用回复验收：expiresAt=$($routeRow[3])"
  }

  Write-Output "push=PLATFORM_ACCEPTED"
  Write-Output "notification=$($parts[0])"
  Write-Output "sessionHash=$(Get-HashText $parts[1])"
  Write-Output "deliveryState=$($parts[2])"
  Write-Output "externalMessageIdHash=$(Get-HashText $parts[3])"
  Write-Output 'routeMatchesDelivery=true'
  Write-Output "routeSessionHash=$(Get-HashText $routeRow[0])"
  Write-Output "routeCreatedAt=$($routeRow[2])"
  Write-Output "routeExpiresAt=$($routeRow[3])"
  Write-Output "pushVisibility=manual_confirmation_required"
  Write-Output "next=请先在微信端确认收到通知，再引用它回复：$ReplyText"
  exit 0
}

# 验收人可能不知道命中的是哪个 OpenCode 会话：用唯一回复文本在最近的路由候选里反查。
# 本模式只负责定位，不作结论；是否通过仍由 VerifyReply 判定。
if ($Mode -eq 'LocateReply') {
  if ([string]::IsNullOrWhiteSpace($ReplyText)) {
    throw 'LocateReply 模式必须提供 -ReplyText。'
  }

  # 同一会话可能对应多条路由（每次通知一条），先按会话去重再限制候选数。
  $recent = @(Get-RecentRoutes -Hours $IntervalHours -Limit 200)
  $candidates = @()
  $seenSessions = @{}
  foreach ($row in $recent) {
    $sessionId = ($row -split "`t")[0]
    if ($seenSessions.ContainsKey($sessionId)) { continue }
    $seenSessions[$sessionId] = $true
    $candidates += ,$row
    if ($candidates.Count -ge $MaxCandidates) { break }
  }
  if ($candidates.Count -eq 0) {
    throw "最近 $IntervalHours 小时内没有 ReplyRoute；请先执行 -Mode Send 并等投递成功。"
  }

  $hitRoutes = @()
  $otherRoutes = @()
  foreach ($row in $candidates) {
    $parts = $row -split "`t"
    if (Test-OpenCodeSessionContains -TargetSessionId $parts[0] -Needle $ReplyText -Tolerant) {
      $hitRoutes += ,$parts
    } else {
      $otherRoutes += ,$parts
    }
  }

  if ($hitRoutes.Count -eq 0) {
    $failed = ''
    if ($script:sessionExportFailures.Count -gt 0) {
      $failed = "；其中 $($script:sessionExportFailures.Count) 个会话导出失败（$($script:sessionExportFailures -join ',')）"
    }
    throw "最近 $IntervalHours 小时内的 $($candidates.Count) 个候选会话都没有出现验收回复文本$failed。请确认微信引用回复已发出，且 AgentNotify 实例当时在运行。"
  }
  if ($hitRoutes.Count -gt 1) {
    $hashes = @($hitRoutes | ForEach-Object { Get-HashText (($_ -split "`t")[0]) })
    throw "回复文本命中多个会话（$($hashes -join ',')），无法唯一定位；请改用更独特的 -ReplyText 重试。"
  }

  $hit = $hitRoutes[0]
  Write-Output 'locate=FOUND'
  Write-Output "sessionId=$($hit[0])"
  Write-Output "sessionHash=$(Get-HashText $hit[0])"
  Write-Output "routeCreatedAt=$($hit[3])"
  Write-Output "routeExpiresAt=$($hit[4])"
  Write-Output "externalMessageIdHash=$(Get-HashText $hit[5])"
  Write-Output "candidateCount=$($candidates.Count)"
  Write-Output "exportFailedCount=$($script:sessionExportFailures.Count)"
  if ($otherRoutes.Count -gt 0) {
    $control = $otherRoutes[0]
    Write-Output "controlSessionId=$($control[0])"
    Write-Output "controlSessionHash=$(Get-HashText $control[0])"
    Write-Output "next=powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\real-opencode-clawbot.ps1 -Mode VerifyReply -SessionId $($hit[0]) -OtherSessionId $($control[0]) -ReplyText $ReplyText"
  } else {
    Write-Output 'controlSessionId=MISSING'
    Write-Output 'next=没有可用于对照的其他会话；请先在 OpenCode 里另开一个会话，再用 -Mode VerifyReply -OtherSessionId 验收。'
  }
  exit 0
}

if ($Mode -eq 'VerifyReply') {
  if ([string]::IsNullOrWhiteSpace($SessionId)) {
    throw 'VerifyReply 模式必须提供 -SessionId。'
  }

  if ([string]::IsNullOrWhiteSpace($ReplyText)) {
    throw 'VerifyReply 模式必须提供 -ReplyText。'
  }

  $hasControlSession = -not [string]::IsNullOrWhiteSpace($OtherSessionId)
  if ($hasControlSession -and $OtherSessionId -eq $SessionId) {
    throw '对照会话必须与目标会话不同：-OtherSessionId 不能等于 -SessionId。'
  }

  $messageId = $ExternalMessageId
  if ([string]::IsNullOrWhiteSpace($messageId)) {
    $route = Get-LatestRoute $SessionId
    if (-not $route) {
      throw '没有找到该会话的 ReplyRoute；请先执行 Send 并等待推送成功。'
    }
    $messageId = $route[2]
  }

  if (-not $route) {
    $route = Get-LatestRoute $SessionId
    if (-not $route) {
      throw '没有找到该会话的 ReplyRoute；请先执行 Send 并等待推送成功。'
    }
  }

  try {
    $claim = Wait-ForState `
      -Label '引用回复 Claim 终态' `
      -Query { Get-CompletedClaimSince $route[0] $route[1] $route[3] } `
      -IsTerminal {
        param($value)
        return $null -ne $value
      }
  } catch {
    $latest = Get-LatestClaimSince $route[0] $route[1] $route[3]
    if ($latest) {
      throw "引用回复未完成：路由建立后最新 Claim state=$($latest[0]) error=$($latest[1]) updated=$($latest[2])。$($_.Exception.Message)"
    }
    throw
  }

  $claimParts = $claim -split "`t"
  if ($claimParts[0] -ne 'Completed') {
    throw "引用回复未完成，state=$($claimParts[0]) error=$($claimParts[1])"
  }

  # --sanitize 会把正文替换成占位符，无法证明回复落入会话；这里仅在内存里做未脱敏匹配。
  if (-not (Test-OpenCodeSessionContains -TargetSessionId $SessionId -Needle $ReplyText)) {
    throw 'Claim 已完成，但 OpenCode 会话导出中没有找到验收回复文本。'
  }

  # 只有对照会话不含同一回复文本，才能证明引用路由没有串到其他会话。
  $controlContainsReply = $false
  if ($hasControlSession) {
    $controlContainsReply = Test-OpenCodeSessionContains -TargetSessionId $OtherSessionId -Needle $ReplyText
    if ($controlContainsReply) {
      throw '对照会话同样包含验收回复文本，无法证明引用回复精确命中目标会话。'
    }
  }

  Write-Output 'reply=PASS'
  Write-Output "sessionHash=$(Get-HashText $SessionId)"
  Write-Output "externalMessageIdHash=$(Get-HashText $messageId)"
  Write-Output "claimState=$($claimParts[0])"
  Write-Output "claimUpdatedAt=$($claimParts[2])"
  Write-Output "replyTextHash=$(Get-HashText $ReplyText)"
  Write-Output 'targetSessionContainsReply=true'
  Write-Output "controlSessionChecked=$hasControlSession"
  if ($hasControlSession) {
    Write-Output "controlSessionHash=$(Get-HashText $OtherSessionId)"
    Write-Output "controlSessionContainsReply=$controlContainsReply"
  } else {
    Write-Output 'controlSessionContainsReply=UNKNOWN'
    Write-Output 'next=正式验收必须同时提供 -OtherSessionId，用于证明回复没有串入其他会话。'
  }
  exit 0
}

$notification = $null
$route = $null
$claim = $null
if (-not [string]::IsNullOrWhiteSpace($SessionId)) {
  $notification = Get-LatestNotification $SessionId
  $route = Get-LatestRoute $SessionId
  if ($route) {
    $claim = Get-CompletedClaimSince $route[0] $route[1] $route[3]
  }
}

Write-Output 'AgentNotify real OpenCode + ClawBot status'
if ($notification) {
  Write-Output "notification=$($notification[0])"
  Write-Output "sessionHash=$(Get-HashText $notification[1])"
  Write-Output "deliveryState=$($notification[3])"
  Write-Output "externalMessageIdHash=$(Get-HashText $notification[4])"
  Write-Output "deliveryUpdatedAt=$($notification[5])"
  if ([string]::IsNullOrWhiteSpace($notification[4])) {
    Write-Output 'routeMatchesDelivery=UNKNOWN'
  } elseif (Get-RouteForDelivery -DeliveryRow $notification) {
    Write-Output 'routeMatchesDelivery=true'
  } else {
    Write-Output 'routeMatchesDelivery=false'
  }
} else {
  Write-Output 'notification=none'
}
if ($route) {
  Write-Output "routeMessageIdHash=$(Get-HashText $route[2])"
  Write-Output "routeCreatedAt=$($route[3])"
  Write-Output "routeExpiresAt=$($route[4])"
} else {
  Write-Output 'route=none'
}
if ($claim) {
  Write-Output "claimState=$($claim[0])"
  Write-Output "claimError=$($claim[1])"
  Write-Output "claimUpdatedAt=$($claim[2])"
} else {
  Write-Output 'claim=none'
}
