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
    Prepare      建立隔离验收目录并输出启动参数，不读取也不修改生产数据。
#>
[CmdletBinding()]
param(
  [ValidateSet('Prepare', 'Status', 'Send', 'VerifyReply')]
  [string]$Mode = 'Status',
  [string]$SessionId = '',
  [string]$ReplyText = 'AGENT_NOTIFY_REPLY_OK',
  [string]$ExternalMessageId = '',
  [string]$DataDir = '',
  [string]$IngressPath = '',
  [string]$OpenCodeCli = '',
  [string]$Sqlite3 = '',
  [string]$TargetAccountId = '',
  [switch]$InstallPlugin,
  [switch]$UseProductionDataDir,
  [int]$TimeoutSeconds = 180
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

if ([string]::IsNullOrWhiteSpace($IngressPath)) {
  $IngressPath = Resolve-FirstLeaf @(
    (Join-Path $env:USERPROFILE 'bin\agentnotify-ingress.exe'),
    'D:\app\AgentNotify-Rust-Preview\agentnotify-ingress.exe'
  )
}
if ([string]::IsNullOrWhiteSpace($OpenCodeCli)) {
  $OpenCodeCli = Resolve-FirstLeaf @(
    (Join-Path $env:APPDATA 'ai.opencode.desktop\cli\2.0.11\opencode-cli.exe'),
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
  Write-Output 'next=1) 退出正在运行的生产预览实例；2) 用上面的环境变量启动预览桌面程序；3) 在 Channels 中扫码绑定独立测试账号并先发一条消息建立会话；4) 再执行 -Mode Send。'
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
select n.notification_id, n.session_id, n.ingest_key, d.state, coalesce(d.external_message_id, ''), d.updated_at
from notifications n
left join deliveries d on d.notification_id = n.notification_id
where n.session_id = '$escaped'
order by n.rowid desc
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
  Write-Output "push=PLATFORM_ACCEPTED"
  Write-Output "notification=$($parts[0])"
  Write-Output "sessionHash=$(Get-HashText $parts[1])"
  Write-Output "deliveryState=$($parts[2])"
  Write-Output "externalMessageIdHash=$(Get-HashText $parts[3])"
  Write-Output "pushVisibility=manual_confirmation_required"
  Write-Output "next=请先在微信端确认收到通知，再引用它回复：$ReplyText"
  exit 0
}

if ($Mode -eq 'VerifyReply') {
  if ([string]::IsNullOrWhiteSpace($SessionId)) {
    throw 'VerifyReply 模式必须提供 -SessionId。'
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
  $export = @(& $OpenCodeCli session export $SessionId 2>&1)
  if ($LASTEXITCODE -ne 0) {
    throw "导出 OpenCode 会话失败 exit=$LASTEXITCODE"
  }
  $exportText = $export -join "`n"
  if ($exportText -notmatch [regex]::Escape($ReplyText)) {
    throw 'Claim 已完成，但 OpenCode 会话导出中没有找到验收回复文本。'
  }

  Write-Output 'reply=PASS'
  Write-Output "sessionHash=$(Get-HashText $SessionId)"
  Write-Output "externalMessageIdHash=$(Get-HashText $messageId)"
  Write-Output "claimState=$($claimParts[0])"
  Write-Output "claimUpdatedAt=$($claimParts[2])"
  Write-Output "replyTextHash=$(Get-HashText $ReplyText)"
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
