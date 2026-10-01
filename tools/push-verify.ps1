#Requires -Version 5.1
<#
.SYNOPSIS
  各 Agent 推送验证（本地发版门禁）：对全部已安装 Agent 各触发一次「任务完成 → 微信推送」，
  校验通知落库与 ClawBot 投递为 Sent，输出报告。

.DESCRIPTION
  用途：发版前（候选构建）与发布后（正式包）各跑一次；未安装/已停用的 Agent 自动跳过并记录。
  验证口径（每个 Agent 三条，前两条脚本自动核，第三条人工确认）：
    1) state.db 新增该 Agent 的通知行（agent_id 正确）；
    2) 对应 deliveries 状态为 Sent；
    3) 人工在微信里确认收到（报告会提示一次性确认）。
  触发方式（2026-10-01 调研实测沉淀）：
    - opencode：opencode-cli run（必须显式指定模型；默认模型可能因余额不足失败）；
    - codex：codex exec --skip-git-repo-check（非 git 目录必需该参数）；
    - antigravity：hook 注入（stdin JSON：fullyIdle=true + 非空 conversationId）；
    - commandcode：Node 加载已安装 mod，调用 __test.pushRunEnd 模拟一次 run_end；
    - devin：存在 devin.off 标记（用户停用）时跳过；否则 hook 注入。
  失败排查入口：docs\TROUBLESHOOTING.md 对应 Agent 的「任务结束不推送」章节，
  以及报告目录下每个 Agent 的 *-trigger.log。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\push-verify.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\push-verify.ps1 -OpencodeModel opencode-go/deepseek-v4.1-flash
#>
param(
  [string]$OpencodeModel = 'opencode-go/deepseek-v4.1-flash',
  [int]$TimeoutSeconds = 180,
  [string]$ReportDir = ''
)

$ErrorActionPreference = 'Stop'
# 5.1 下按 UTF-8 解码原生命令输出（sqlite3 会输出中文标题）。
try { [Console]::OutputEncoding = [Text.Encoding]::UTF8 } catch { }

# ---- 常量 ----
$script:PollIntervalSeconds = 3
$script:DeliveryGraceSeconds = 15
$script:TestMessage = '只回复：OK'
$script:TitlePrefix = 'push-verify'
$script:AppProcessName = 'agentnotify-desktop'
$script:AppInstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Agent-notify'
$script:IngressExe = Join-Path $script:AppInstallDir 'agentnotify-ingress.exe'
$script:MinNodeMajor = 22
$script:MinNodeMinor = 18

function Resolve-Sqlite3 {
  if ($env:AGENT_NOTIFY_SQLITE3 -and (Test-Path -LiteralPath $env:AGENT_NOTIFY_SQLITE3 -PathType Leaf)) {
    return $env:AGENT_NOTIFY_SQLITE3
  }
  $onPath = Get-Command sqlite3.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  $candidates = @(
    (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\sqlite3.exe'),
    (Join-Path $env:ProgramData 'chocolatey\bin\sqlite3.exe')
  )
  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
  }
  throw '找不到 sqlite3.exe：请把它放进 PATH，或用环境变量 AGENT_NOTIFY_SQLITE3 指定路径'
}

function Resolve-StateDb {
  $db = Join-Path $env:LOCALAPPDATA 'AgentNotify\data\state.db'
  if (-not (Test-Path -LiteralPath $db -PathType Leaf)) {
    throw "找不到应用数据库：$db（请先安装并启动 AgentNotify）"
  }
  return $db
}

function Get-AppVersion {
  $exe = Join-Path $script:AppInstallDir 'agentnotify-desktop.exe'
  if (Test-Path -LiteralPath $exe -PathType Leaf) {
    return (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
  }
  return '未知'
}

function Invoke-DbQuery {
  param([string]$Query)
  # 原生 stderr 在 $ErrorActionPreference=Stop 下会变成终止性错误；这里局部降级并按退出码判断。
  $previousPreference = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $output = & $script:Sqlite3 $script:StateDb $Query 2>$null
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }
  if ($exitCode -ne 0) { throw "sqlite3 查询失败：$Query" }
  return ($output | Out-String).Trim()
}

# 原生命令统一入口：$ErrorActionPreference=Stop 下 stderr 会变成终止性错误，这里局部降级并按退出码判断。
function Invoke-NativeCapture {
  param([scriptblock]$Command)
  $previousPreference = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $output = & $Command 2>&1
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }
  return [pscustomobject]@{ ExitCode = $exitCode; Output = (($output | Out-String).Trim()) }
}

function Get-AgentMaxNotificationRowId {
  param([string]$AgentId)
  $value = Invoke-DbQuery "SELECT COALESCE(MAX(rowid),0) FROM notifications WHERE agent_id='$AgentId';"
  return [int]$value
}

function Wait-NewNotification {
  param([string]$AgentId, [int]$BaselineRowId, [int]$TimeoutSeconds)
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $row = Invoke-DbQuery "SELECT rowid || '|' || REPLACE(title, '|', '/') FROM notifications WHERE agent_id='$AgentId' AND rowid > $BaselineRowId ORDER BY rowid LIMIT 1;"
    if ($row) {
      $parts = $row.Split('|', 2)
      return [pscustomobject]@{ RowId = [int]$parts[0]; Title = $parts[1] }
    }
    Start-Sleep -Seconds $script:PollIntervalSeconds
  }
  return $null
}

function Wait-Delivery {
  param([int]$NotificationRowId, [int]$TimeoutSeconds)
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $row = Invoke-DbQuery "SELECT d.state || '|' || COALESCE(d.error_code,'') FROM deliveries d JOIN notifications n ON n.notification_id = d.notification_id WHERE n.rowid = $NotificationRowId LIMIT 1;"
    if ($row) {
      $parts = $row.Split('|', 2)
      return [pscustomobject]@{ State = $parts[0]; ErrorCode = $parts[1] }
    }
    Start-Sleep -Seconds $script:PollIntervalSeconds
  }
  return $null
}

# 触发后统一核验：等通知 → 等投递 → 组装结果行。
function Complete-AgentCheck {
  param([string]$AgentId, [int]$BaselineRowId, [pscustomobject]$Trigger)
  $note = ''
  if ($Trigger.ExitCode -ne 0) { $note = "触发命令退出码 $($Trigger.ExitCode)（详见 $AgentId-trigger.log）" }
  $notification = Wait-NewNotification -AgentId $AgentId -BaselineRowId $BaselineRowId -TimeoutSeconds $TimeoutSeconds
  if (-not $notification) {
    if ($note) { $note = "$note；" }
    $note = "$note" + "触发后 $TimeoutSeconds 秒内没有新通知（排查：docs\TROUBLESHOOTING.md 对应章节）"
    return [pscustomobject]@{ Agent = $AgentId; Status = 'FAIL'; NotificationRowId = 0; Title = ''; DeliveryState = ''; DeliveryError = ''; Note = $note }
  }
  $delivery = Wait-Delivery -NotificationRowId $notification.RowId -TimeoutSeconds $script:DeliveryGraceSeconds
  if (-not $delivery) {
    return [pscustomobject]@{ Agent = $AgentId; Status = 'FAIL'; NotificationRowId = $notification.RowId; Title = $notification.Title; DeliveryState = ''; DeliveryError = ''; Note = '通知已落库但没有投递记录' }
  }
  $status = 'PASS'
  if ($delivery.State -ne 'Sent') {
    $status = 'FAIL'
    $deliveryNote = "投递状态 $($delivery.State)"
    if ($delivery.ErrorCode) { $deliveryNote = "$deliveryNote（$($delivery.ErrorCode)）" }
    return [pscustomobject]@{ Agent = $AgentId; Status = $status; NotificationRowId = $notification.RowId; Title = $notification.Title; DeliveryState = $delivery.State; DeliveryError = $delivery.ErrorCode; Note = $deliveryNote }
  }
  if ($notification.Title -match '任务失败') { $note = "$note；触发回合失败（标题含「任务失败」，推送链路已通过）" }
  if ($note -and $note.StartsWith('；')) { $note = $note.Substring(1) }
  return [pscustomobject]@{ Agent = $AgentId; Status = $status; NotificationRowId = $notification.RowId; Title = $notification.Title; DeliveryState = $delivery.State; DeliveryError = $delivery.ErrorCode; Note = $note }
}

# ---- Agent 探测 ----
function Resolve-OpencodeCli {
  $cliRoot = Join-Path $env:APPDATA 'ai.opencode.desktop\cli'
  if (Test-Path -LiteralPath $cliRoot) {
    $found = Get-ChildItem -LiteralPath $cliRoot -Recurse -Filter 'opencode-cli.exe' -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($found) { return $found.FullName }
  }
  $onPath = Get-Command opencode.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  return $null
}

function Resolve-CodexCli {
  $binRoot = Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\bin'
  if (Test-Path -LiteralPath $binRoot) {
    $found = Get-ChildItem -LiteralPath $binRoot -Recurse -Filter 'codex.exe' -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($found) { return $found.FullName }
  }
  $onPath = Get-Command codex.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  return $null
}

function Test-NodeSupportsTypeScript {
  $node = Get-Command node.exe -ErrorAction SilentlyContinue
  if (-not $node) { return $null }
  $versionText = (& $node.Source --version) 2>$null
  if ($versionText -notmatch '^v(\d+)\.(\d+)') { return $null }
  $major = [int]$Matches[1]
  $minor = [int]$Matches[2]
  if ($major -gt $script:MinNodeMajor -or ($major -eq $script:MinNodeMajor -and $minor -ge $script:MinNodeMinor)) {
    return $node.Source
  }
  return $null
}

# ---- 各 Agent 触发 ----
function Invoke-OpencodeRun {
  param([string]$Cli, [string]$WorkDir)
  Push-Location -LiteralPath $WorkDir
  try {
    return Invoke-NativeCapture { & $Cli run --title "$($script:TitlePrefix)-opencode" --auto --model $OpencodeModel $script:TestMessage }
  } finally { Pop-Location }
}

function Invoke-CodexRun {
  param([string]$Cli, [string]$WorkDir)
  Push-Location -LiteralPath $WorkDir
  try {
    # codex exec 在 stdin 是未关闭的管道时会一直等 EOF；显式给空输入并关闭，避免挂起。
    return Invoke-NativeCapture { '' | & $Cli exec --skip-git-repo-check "$($script:TitlePrefix)-codex：$($script:TestMessage)" }
  } finally { Pop-Location }
}

function Invoke-AntigravityHook {
  param([string]$Launcher)
  $payload = '{"fullyIdle":true,"conversationId":"' + $script:TitlePrefix + '-antigravity"}'
  return Invoke-NativeCapture { $payload | & $Launcher antigravity stop }
}

function Invoke-CommandCodeMod {
  param([string]$NodeExe, [string]$ModPath, [string]$WorkDir)
  $harnessPath = Join-Path $WorkDir 'commandcode-run-end.mjs'
  $modUri = 'file:///' + ((Resolve-Path -LiteralPath $ModPath).Path -replace '\\', '/')
  $harness = @"
// 由 tools\push-verify.ps1 生成：加载已安装的 Command Code mod，模拟一次 run_end 提交。
const mod = await import('$modUri');
const t = mod.__test;
await t.loadFs();
const state = t.createInstanceState();
state.sessionId = '$($script:TitlePrefix)-commandcode';
state.title = '$($script:TitlePrefix)-commandcode';
await t.pushRunEnd(state, '$($script:TestMessage)', state.sessionId);
console.log('submitted');
"@
  [IO.File]::WriteAllText($harnessPath, $harness, (New-Object Text.UTF8Encoding($false)))
  return Invoke-NativeCapture { & $NodeExe $harnessPath }
}

function Invoke-DevinHook {
  param([string]$HookExe)
  $payload = '{"session_id":"' + $script:TitlePrefix + '-devin","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"OK"}'
  return Invoke-NativeCapture { $payload | & $HookExe devin stop }
}

# ---- 清理 ----
function Remove-OpencodeTestSessions {
  param([string]$Cli, [string]$WorkDir)
  $removed = New-Object System.Collections.Generic.List[string]
  Push-Location -LiteralPath $WorkDir
  try {
    $listResult = Invoke-NativeCapture { & $Cli session list }
    foreach ($line in ($listResult.Output -split "`r?`n")) {
      if ($line -match ('^(ses_\S+)\s+' + [regex]::Escape($script:TitlePrefix))) {
        $sessionId = $Matches[1]
        Invoke-NativeCapture { & $Cli session delete $sessionId } | Out-Null
        $removed.Add($sessionId)
      }
    }
  } finally { Pop-Location }
  return $removed
}

# ---- 前置检查 ----
if (-not (Get-Process -Name $script:AppProcessName -ErrorAction SilentlyContinue)) {
  throw "AgentNotify 未运行：请先启动 $script:AppProcessName 再跑本脚本（推送管道需要宿主进程）"
}
$script:Sqlite3 = Resolve-Sqlite3
$script:StateDb = Resolve-StateDb
$appVersion = Get-AppVersion

$paused = Invoke-DbQuery "SELECT value_json FROM settings WHERE key='notificationsPaused';"
if ($paused -match 'true') {
  throw '通知已被暂停（notificationsPaused=true）：请在设置里恢复后再跑本脚本'
}
$quietHours = Invoke-DbQuery "SELECT value_json FROM settings WHERE key='notification.quietHours';"
if ($quietHours -and $quietHours -ne '""') {
  Write-Warning "检测到勿扰时段配置（$quietHours）：可能吞掉测试推送，建议临时清空"
}

if ([string]::IsNullOrWhiteSpace($ReportDir)) {
  $ReportDir = Join-Path ([IO.Path]::GetTempPath()) 'agent-notify-push-verify'
}
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null
$workRoot = Join-Path $ReportDir 'work'
foreach ($name in @('opencode', 'codex', 'commandcode')) {
  New-Item -ItemType Directory -Force -Path (Join-Path $workRoot $name) | Out-Null
}

$results = New-Object System.Collections.Generic.List[object]
$skipped = New-Object System.Collections.Generic.List[object]
$channelBroken = $false
$opencodeCli = $null

function Write-TriggerLog {
  param([string]$AgentId, [pscustomobject]$Trigger)
  $logPath = Join-Path $ReportDir "$AgentId-trigger.log"
  [IO.File]::WriteAllText($logPath, ("exit=$($Trigger.ExitCode)`r`n`r`n" + $Trigger.Output + "`r`n"), (New-Object Text.UTF8Encoding($false)))
}

# ---- 1) opencode（同时充当 canary）----
$opencodeCli = Resolve-OpencodeCli
if (-not $opencodeCli) {
  $skipped.Add([pscustomobject]@{ Agent = 'opencode'; Reason = '未找到 opencode-cli.exe / opencode.exe' })
} else {
  $baseline = Get-AgentMaxNotificationRowId 'opencode'
  $trigger = Invoke-OpencodeRun -Cli $opencodeCli -WorkDir (Join-Path $workRoot 'opencode')
  Write-TriggerLog -AgentId 'opencode' -Trigger $trigger
  $result = Complete-AgentCheck -AgentId 'opencode' -BaselineRowId $baseline -Trigger $trigger
  $results.Add($result)
  if ($result.DeliveryState -eq 'Skipped' -and $result.DeliveryError -eq 'session_missing') { $channelBroken = $true }
}

# ---- 2) codex ----
if (-not $channelBroken) {
  $codexCli = Resolve-CodexCli
  if (-not $codexCli) {
    $skipped.Add([pscustomobject]@{ Agent = 'codex'; Reason = '未找到 codex.exe' })
  } else {
    $baseline = Get-AgentMaxNotificationRowId 'codex'
    $trigger = Invoke-CodexRun -Cli $codexCli -WorkDir (Join-Path $workRoot 'codex')
    Write-TriggerLog -AgentId 'codex' -Trigger $trigger
    $result = Complete-AgentCheck -AgentId 'codex' -BaselineRowId $baseline -Trigger $trigger
    $results.Add($result)
    if ($result.DeliveryState -eq 'Skipped' -and $result.DeliveryError -eq 'session_missing') { $channelBroken = $true }
  }
}

# ---- 3) antigravity（hook 注入）----
if (-not $channelBroken) {
  $launcher = Join-Path $env:USERPROFILE '.gemini\config\agent-notify-hook.cmd'
  $hooksFile = Join-Path $env:USERPROFILE '.gemini\config\hooks.json'
  $hookExe = Join-Path $script:AppInstallDir 'agentnotify-antigravity-hook.exe'
  if (-not (Test-Path -LiteralPath $launcher -PathType Leaf) -or
      -not (Test-Path -LiteralPath $hookExe -PathType Leaf) -or
      -not ((Get-Content -LiteralPath $hooksFile -Raw -ErrorAction SilentlyContinue) -match 'agent-notify')) {
    $skipped.Add([pscustomobject]@{ Agent = 'antigravity'; Reason = '未接入（缺少 agent-notify-hook.cmd / hooks.json 无 agent-notify）' })
  } else {
    $baseline = Get-AgentMaxNotificationRowId 'antigravity'
    $trigger = Invoke-AntigravityHook -Launcher $launcher
    Write-TriggerLog -AgentId 'antigravity' -Trigger $trigger
    $result = Complete-AgentCheck -AgentId 'antigravity' -BaselineRowId $baseline -Trigger $trigger
    $results.Add($result)
    if ($result.DeliveryState -eq 'Skipped' -and $result.DeliveryError -eq 'session_missing') { $channelBroken = $true }
  }
}

# ---- 4) commandcode（mod 注入）----
if (-not $channelBroken) {
  $modPath = Join-Path $env:USERPROFILE '.commandcode\mods\agent-notify.ts'
  $nodeExe = Test-NodeSupportsTypeScript
  if (-not (Test-Path -LiteralPath $modPath -PathType Leaf)) {
    $skipped.Add([pscustomobject]@{ Agent = 'commandcode'; Reason = '未接入（缺少 mod：~\.commandcode\mods\agent-notify.ts）' })
  } elseif (-not $nodeExe) {
    $skipped.Add([pscustomobject]@{ Agent = 'commandcode'; Reason = "未找到可用的 node.exe（需要 >= $($script:MinNodeMajor).$($script:MinNodeMinor) 以直接运行 .ts）" })
  } else {
    $baseline = Get-AgentMaxNotificationRowId 'commandcode'
    $trigger = Invoke-CommandCodeMod -NodeExe $nodeExe -ModPath $modPath -WorkDir (Join-Path $workRoot 'commandcode')
    Write-TriggerLog -AgentId 'commandcode' -Trigger $trigger
    $result = Complete-AgentCheck -AgentId 'commandcode' -BaselineRowId $baseline -Trigger $trigger
    $results.Add($result)
    if ($result.DeliveryState -eq 'Skipped' -and $result.DeliveryError -eq 'session_missing') { $channelBroken = $true }
  }
}

# ---- 5) devin（默认停用；启用时 hook 注入）----
if (-not $channelBroken) {
  $devinOff = Join-Path $env:USERPROFILE '.config\agent-notify\devin.off'
  $devinHook = Join-Path $script:AppInstallDir 'agentnotify-devin-hook.exe'
  $devinConfig = Join-Path $env:APPDATA 'devin\config.json'
  if (Test-Path -LiteralPath $devinOff -PathType Leaf) {
    $skipped.Add([pscustomobject]@{ Agent = 'devin'; Reason = '已停用（devin.off）' })
  } elseif (-not (Test-Path -LiteralPath $devinHook -PathType Leaf) -or
            -not ((Get-Content -LiteralPath $devinConfig -Raw -ErrorAction SilentlyContinue) -match 'agentnotify-devin-hook')) {
    $skipped.Add([pscustomobject]@{ Agent = 'devin'; Reason = '未接入（缺少 devin hook / config.json 无 handler）' })
  } else {
    $baseline = Get-AgentMaxNotificationRowId 'devin'
    $trigger = Invoke-DevinHook -HookExe $devinHook
    Write-TriggerLog -AgentId 'devin' -Trigger $trigger
    $result = Complete-AgentCheck -AgentId 'devin' -BaselineRowId $baseline -Trigger $trigger
    $results.Add($result)
    if ($result.DeliveryState -eq 'Skipped' -and $result.DeliveryError -eq 'session_missing') { $channelBroken = $true }
  }
}

# ---- 清理 ----
$removedSessions = New-Object System.Collections.Generic.List[string]
if ($opencodeCli -and -not $channelBroken) {
  $removedSessions = Remove-OpencodeTestSessions -Cli $opencodeCli -WorkDir (Join-Path $workRoot 'opencode')
}

# ---- 报告 ----
$failCount = @($results | Where-Object { $_.Status -ne 'PASS' }).Count
if ($channelBroken) {
  $conclusion = '推送会话已断（session_missing）：请在微信里给 ClawBot 发一条任意消息后重新运行本脚本'
} elseif ($failCount -gt 0) {
  $conclusion = "存在失败项（$failCount 个）"
} else {
  $conclusion = '全部通过'
}

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add('# Agent 推送验证报告（push-verify）')
$lines.Add('')
$lines.Add(('- 时间：{0}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')))
$lines.Add(('- 机器：{0}；应用版本：{1}' -f $env:COMPUTERNAME, $appVersion))
$lines.Add(('- 数据库：{0}' -f $script:StateDb))
$lines.Add(('- opencode 模型：{0}' -f $OpencodeModel))
$lines.Add(('- 结论：{0}' -f $conclusion))
$lines.Add('')
$lines.Add('## 结果')
$lines.Add('| Agent | 状态 | 通知 rowid | 标题 | 投递 | 备注 |')
$lines.Add('| --- | --- | --- | --- | --- | --- |')
foreach ($item in $results) {
  $lines.Add(('| {0} | {1} | {2} | {3} | {4} | {5} |' -f $item.Agent, $item.Status, $item.NotificationRowId, $item.Title, $item.DeliveryState, $item.Note))
}
if ($skipped.Count -gt 0) {
  $lines.Add('')
  $lines.Add('## 跳过（未安装/未启用）')
  foreach ($item in $skipped) {
    $lines.Add(('- {0}：{1}' -f $item.Agent, $item.Reason))
  }
}
$lines.Add('')
$lines.Add('## 人工确认')
$lines.Add(('- 请在微信里确认收到 {0} 条 `{1}-*` 测试推送（内容「{2}」）。' -f $results.Count, $script:TitlePrefix, $script:TestMessage))
$lines.Add('')
$lines.Add('## 清理')
$lines.Add(('- opencode 测试会话已删除：{0} 个' -f $removedSessions.Count))
$lines.Add(('- codex 会留下一个测试线程（工作目录：{0}），可自行归档。' -f (Join-Path $workRoot 'codex')))
$lines.Add(('- 触发命令原始输出：{0}\<agent>-trigger.log' -f $ReportDir))

$reportPath = Join-Path $ReportDir ('push-verify-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.md')
[IO.File]::WriteAllLines($reportPath, $lines.ToArray(), (New-Object Text.UTF8Encoding($false)))

# ---- 汇总输出 ----
Write-Output ''
Write-Output '[push-verify] 结果：'
foreach ($item in $results) {
  $suffix = ''
  if ($item.Note) { $suffix = " — $($item.Note)" }
  Write-Output ('  - {0}: {1}{2}' -f $item.Agent, $item.Status, $suffix)
}
foreach ($item in $skipped) {
  Write-Output ('  - {0}: 跳过 — {1}' -f $item.Agent, $item.Reason)
}
Write-Output ('[push-verify] 报告：{0}' -f $reportPath)

if ($channelBroken) {
  Write-Output '[push-verify] 推送会话已断：请在微信里给 ClawBot 发一条任意消息后重新运行本脚本'
  exit 2
}
if ($failCount -gt 0) {
  Write-Output '[push-verify] 存在失败项'
  exit 1
}
Write-Output '[push-verify] 全部通过'
exit 0
