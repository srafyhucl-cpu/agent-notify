#Requires -Version 5.1
<#
.SYNOPSIS
  Agent-notify 冒烟测试：脚本语法 + CLI DryRun + 开关 marker + 沙箱安装/卸载。不联网，不碰真实配置。
#>
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $RepoRoot

function Assert-True {
  param([bool]$Condition, [string]$Message)
  if (-not $Condition) { throw $Message }
}

function Get-PESubsystem {
  param([string]$Path)
  $stream = [IO.File]::OpenRead($Path)
  try {
    $reader = New-Object IO.BinaryReader($stream)
    $stream.Position = 0x3c
    $peOffset = $reader.ReadInt32()
    $stream.Position = $peOffset + 0x5c
    return $reader.ReadUInt16()
  } finally {
    $stream.Dispose()
  }
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

# 0. PowerShell 语法解析
foreach ($file in @('install.ps1', 'uninstall.ps1', 'tools\build-release.ps1', 'tools\test.ps1', 'tools\lint.ps1', 'tests\smoke.ps1')) {
  $tokens = $null
  $errors = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $RepoRoot $file), [ref]$tokens, [ref]$errors)
  if ($errors.Count -gt 0) { throw "$file 语法失败：$($errors[0].Message)" }
  Write-Output "[ok] syntax $file"
}

# 1. 插件静态约束：只依赖新的 CLI，不再出现旧品牌/旧运行时
$pluginRaw = [IO.File]::ReadAllText((Join-Path $RepoRoot 'plugin\agent-notify.ts'))
foreach ($needle in @('agent-notify.exe', 'AGENT_NOTIFY_BIN', 'notify",', 'PromptCurrentInput', 'promptAsync', 'writeReplyFileAtomic', 'cleanupStaleReplyArtifacts', 'REPLY_HEARTBEAT_DIR', 'clearReplyHeartbeat')) {
  Assert-True ($pluginRaw -match [regex]::Escape($needle)) "插件缺少新协议标记：$needle"
}
$legacyNames = @(('link' + 'Weixin'), ('link' + 'weixin'), ('PUSH' + 'PLUS'), ('Push' + 'Plus'), ('power' + 'shell.exe'), ('notify' + '-ai.ps1'), ('anti' + 'gravity'))
foreach ($legacy in $legacyNames) {
  Assert-True (-not ($pluginRaw -match [regex]::Escape($legacy))) "插件仍残留旧实现：$legacy"
}
Write-Output '[ok] plugin only targets agent-notify.exe'

# 1b. 快捷方式必须两套安装体系同名，否则升级后桌面/启动项会出现两份。
$installRaw = [IO.File]::ReadAllText((Join-Path $RepoRoot 'install.ps1'))
$issRaw = [IO.File]::ReadAllText((Join-Path $RepoRoot 'installer\agent-notify.iss'))
Assert-True ($installRaw.Contains("Agent-notify.lnk")) 'install.ps1 未使用统一快捷方式名 Agent-notify.lnk'
Assert-True ($issRaw.Contains('{userdesktop}\Agent-notify"')) '安装器桌面快捷方式不是 Agent-notify.lnk'
Assert-True ($issRaw.Contains('{userstartup}\Agent-notify"')) '安装器启动项快捷方式不是 Agent-notify.lnk'
Assert-True ($issRaw.Contains('Agent-notify 悬浮窗.lnk')) '安装器缺少旧快捷方式清理项'
Write-Output '[ok] shortcuts share one name'

# 2. 编译 CLI（缓存放仓库所在磁盘）
$goExe = Resolve-GoCommand
if (-not $goExe) { throw '找不到 go.exe' }
$driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
$cacheRoot = Join-Path $driveRoot 'Temp\agent-notify-go'
if ([string]::IsNullOrWhiteSpace($env:GOPATH)) { $env:GOPATH = $cacheRoot }
if ([string]::IsNullOrWhiteSpace($env:GOMODCACHE)) { $env:GOMODCACHE = Join-Path $cacheRoot 'pkg\mod' }
if ([string]::IsNullOrWhiteSpace($env:GOCACHE)) { $env:GOCACHE = Join-Path $cacheRoot 'build' }

$smokeRoot = Join-Path $driveRoot ('Temp\agent-notify-smoke-' + [guid]::NewGuid().ToString('N'))
$binDir = Join-Path $smokeRoot 'bin'
$pluginDir = Join-Path $smokeRoot 'plugins'
$configDir = Join-Path $smokeRoot 'config'
$antigravityConfigDir = Join-Path $smokeRoot 'antigravity-config'
$devinConfigDir = Join-Path $smokeRoot 'devin-config'
New-Item -ItemType Directory -Force -Path $binDir, $pluginDir, $configDir | Out-Null
New-Item -ItemType Directory -Force -Path $antigravityConfigDir, $devinConfigDir | Out-Null

$exePath = Join-Path $binDir 'agent-notify.exe'
& $goExe build -ldflags '-s -w' -trimpath -o $exePath '.\cmd\agent-notify\'
if ($LASTEXITCODE -ne 0) { throw "go build 失败 exit=$LASTEXITCODE" }
Write-Output '[ok] go build'

# 隔离环境：所有状态都落在沙箱里，绝不碰真实用户配置
$env:AGENT_NOTIFY_CONFIG_DIR = $configDir
$env:AGENT_NOTIFY_TEMP_DIR = (Join-Path $smokeRoot 'state')
$env:AGENT_NOTIFY_CONFIG_FILE = Join-Path $configDir 'config.json'
$env:AGENT_NOTIFY_CREDENTIAL_FILE = Join-Path $configDir 'clawbot.json'
$env:AGENT_NOTIFY_PLUGIN_FILE = Join-Path $pluginDir 'agent-notify.ts'
$env:AGENT_NOTIFY_CODEX_CONFIG = Join-Path $smokeRoot 'codex-config\config.toml'
$env:AGENT_NOTIFY_OPENCODE_REPLY_DIR = Join-Path $smokeRoot 'opencode-reply'
$env:AGENT_NOTIFY_ANTIGRAVITY_HOOKS = Join-Path $antigravityConfigDir 'hooks.json'
$env:AGENT_NOTIFY_ANTIGRAVITY_LAUNCHER = Join-Path $antigravityConfigDir 'agent-notify-hook.cmd'
$env:AGENT_NOTIFY_DEVIN_CONFIG = Join-Path $devinConfigDir 'config.json'
$env:AGENT_NOTIFY_DEVIN_EXTENSION_DIR = Join-Path $smokeRoot 'devin-extension'
$env:AGENT_NOTIFY_DEVIN_REPLY_DIR = Join-Path $smokeRoot 'devin-reply'

try {
  # 3. notify DryRun 渲染
  $dry = & $exePath notify --dry-run --title '【smoke】' --summary 'hello **bold** smoke' --no-stdin 2>&1
  Assert-True ($LASTEXITCODE -eq 0) "notify DryRun exit=$LASTEXITCODE"
  $dryText = "$dry"
  Assert-True ($dryText -match 'bold') "notify DryRun 未渲染摘要：$dryText"
  Assert-True ($dryText -match 'smoke') "notify DryRun 未渲染标题：$dryText"
  Write-Output '[ok] notify dry-run'

  # 4. status JSON
  $status = & $exePath status --json 2>&1
  Assert-True ($LASTEXITCODE -eq 0) "status exit=$LASTEXITCODE"
  $statusJson = "$status" | ConvertFrom-Json
  Assert-True ($statusJson.openCodeEnabled -eq $true) 'status 初始 OpenCode 开关应为开启'
  Assert-True ($statusJson.codexEnabled -eq $true) 'status 初始 Codex 开关应为开启'
  Assert-True ($statusJson.antigravityEnabled -eq $true) 'status 初始 Antigravity 开关应为开启'
  Assert-True ($statusJson.devinEnabled -eq $true) 'status 初始 Devin 开关应为开启'
  Assert-True ($statusJson.replyEnabled -eq $false) 'status 初始引用回复开关应默认关闭'
  Assert-True (-not [string]::IsNullOrWhiteSpace($statusJson.replyRouteFile)) 'status 缺少引用路由文件路径'
  Write-Output '[ok] status json'

  # 4b. history JSON：空历史也要输出合法 JSON，脚本才不用区分文本提示
  $historyRaw = "$(& $exePath history --limit 5 --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 0) "history --json exit=$LASTEXITCODE"
  Assert-True ($historyRaw -eq '[]') "history --json 空历史应为 []：$historyRaw"
  Write-Output '[ok] history json'

  # 4c. reply-check 只读闸门：无诊断日志时必须报告证据不足
  $gateRaw = "$(& $exePath reply-check --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 2) "reply-check 无证据应返回 2，实际 $LASTEXITCODE：$gateRaw"
  $gateJson = $gateRaw | ConvertFrom-Json
  Assert-True ($gateJson.gate.status -eq 'awaiting-send') "reply-check 状态应为 awaiting-send：$gateRaw"
  Assert-True ($gateJson.replyEnabled -eq $false) 'reply-check 不应改变引用回复开关'
  Write-Output '[ok] reply-check no evidence'

  # 4d. reply-check 正/反向路径：只在沙箱写诊断日志，不触网
  $debugLog = Join-Path $env:AGENT_NOTIFY_TEMP_DIR 'clawbot-debug.log'
  New-Item -ItemType Directory -Force -Path $env:AGENT_NOTIFY_TEMP_DIR | Out-Null
  $utf8NoBom = New-Object Text.UTF8Encoding($false)
  $credentialsPath = Join-Path $configDir 'clawbot.json'
  $routeFile = Join-Path $configDir 'reply-routes.jsonl'
  [IO.File]::WriteAllText($credentialsPath, '{"bot_token":"token","ilink_bot_id":"bot-1","ilink_user_id":"user-1"}', $utf8NoBom)
	  $now = [DateTime]::UtcNow
	  $scopeBytes = [Text.Encoding]::UTF8.GetBytes("bot-1`0user-1")
	  $sha256 = [Security.Cryptography.SHA256]::Create()
	  try { $scopeHash = $sha256.ComputeHash($scopeBytes) } finally { $sha256.Dispose() }
	  $scope = ([BitConverter]::ToString($scopeHash).Replace('-', '').ToLowerInvariant()).Substring(0, 32)
	  $routeJson = @{
    messageID = 'platform-1'
    clientID = 'client-1'
    botID = 'bot-1'
    userID = 'user-1'
    agent = 'codex'
    sessionID = 'thread-1'
    createdAt = $now.ToString('o')
    expiresAt = $now.AddDays(1).ToString('o')
  } | ConvertTo-Json -Compress
  [IO.File]::WriteAllText($routeFile, $routeJson + [Environment]::NewLine, $utf8NoBom)
  [IO.File]::WriteAllLines($debugLog, @(
    ('2026-09-13T07:00:00+08:00 sendmessage-result data={"account_scope":"' + $scope + '","message_id":"platform-1","client_id":"client-1"}')
    ('2026-09-13T07:00:01+08:00 getupdates-result data=[{"msg_id":"reply-1","has_reference":true,"referenced_msg_ids":["platform-1"],"account_scope":"' + $scope + '","private":true,"bound_sender":true}]')
  ), $utf8NoBom)
  $gatePassRaw = "$(& $exePath reply-check --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 0) "reply-check 匹配时应返回 0，实际 $LASTEXITCODE：$gatePassRaw"
  $gatePass = $gatePassRaw | ConvertFrom-Json
  Assert-True ($gatePass.gate.status -eq 'passed') "reply-check 应通过：$gatePassRaw"
  Remove-Item -LiteralPath $routeFile -Force
  $routeFailRaw = "$(& $exePath reply-check --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 1) "reply-check 路由缺失时应返回 1，实际 $LASTEXITCODE：$routeFailRaw"
  $routeFail = $routeFailRaw | ConvertFrom-Json
  Assert-True ($routeFail.gate.status -eq 'failed') "reply-check 应判路由未通过：$routeFailRaw"
  Assert-True ($routeFail.gate.routeFailures -eq 1) "reply-check 应记录 1 条路由失败：$routeFailRaw"
  [IO.File]::WriteAllLines($debugLog, @(
    ('2026-09-13T07:01:00+08:00 sendmessage-result data={"account_scope":"' + $scope + '","message_id":"platform-1","client_id":"client-1"}')
    ('2026-09-13T07:01:01+08:00 getupdates-result data=[{"msg_id":"reply-2","has_reference":true,"referenced_msg_ids":["platform-9"],"account_scope":"' + $scope + '","private":true,"bound_sender":true}]')
  ), $utf8NoBom)
  $gateFailRaw = "$(& $exePath reply-check --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 1) "reply-check 未匹配时应返回 1，实际 $LASTEXITCODE：$gateFailRaw"
  $gateFail = $gateFailRaw | ConvertFrom-Json
  Assert-True ($gateFail.gate.status -eq 'failed') "reply-check 应判未通过：$gateFailRaw"
  [IO.File]::WriteAllLines($debugLog, @(
    ('2026-09-13T07:02:00+08:00 sendmessage-result data={"account_scope":"' + $scope + '","message_id":"platform-1","client_id":"client-1"}')
    ('2026-09-13T07:02:01+08:00 getupdates-result data=[{"msg_id":"reply-3","has_reference":true,"referenced_msg_ids":[],"account_scope":"' + $scope + '","private":true,"bound_sender":true}]')
  ), $utf8NoBom)
  $missingIDRaw = "$(& $exePath reply-check --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 1) "reply-check 引用缺少 ID 时应返回 1，实际 $LASTEXITCODE：$missingIDRaw"
  $missingID = $missingIDRaw | ConvertFrom-Json
  Assert-True ($missingID.gate.status -eq 'failed') "缺少引用 ID 应判未通过：$missingIDRaw"
  Assert-True ($missingID.gate.failedQuotes -eq 1) "缺少引用 ID 应记录 1 条失败：$missingIDRaw"
  Assert-True (-not [string]::IsNullOrWhiteSpace($missingID.gate.quotes[0].referenceError)) '缺少引用 ID 应包含明确原因'
  Write-Output '[ok] reply-check gate'

  # 5. toggle 开关 marker
  & $exePath toggle --agent all --off 2>&1 | Out-Null
  Assert-True ($LASTEXITCODE -eq 0) "toggle off exit=$LASTEXITCODE"
  Assert-True (Test-Path (Join-Path $configDir 'opencode.off')) 'toggle off 未建 OpenCode marker'
  Assert-True (Test-Path (Join-Path $configDir 'codex.off')) 'toggle off 未建 Codex marker'
  Assert-True (Test-Path (Join-Path $configDir 'antigravity.off')) 'toggle off 未建 Antigravity marker'
  Assert-True (Test-Path (Join-Path $configDir 'devin.off')) 'toggle off 未建 Devin marker'
  $offJson = "$(& $exePath status --json 2>&1)" | ConvertFrom-Json
  Assert-True ($offJson.openCodeEnabled -eq $false) 'toggle off 后 OpenCode 应为关闭'
  Assert-True ($offJson.antigravityEnabled -eq $false) 'toggle off 后 Antigravity 应为关闭'
  Assert-True ($offJson.devinEnabled -eq $false) 'toggle off 后 Devin 应为关闭'
  & $exePath toggle --agent all --on 2>&1 | Out-Null
  Assert-True (-not (Test-Path (Join-Path $configDir 'opencode.off'))) 'toggle on 未删 OpenCode marker'
  Assert-True (-not (Test-Path (Join-Path $configDir 'codex.off'))) 'toggle on 未删 Codex marker'
  Assert-True (-not (Test-Path (Join-Path $configDir 'antigravity.off'))) 'toggle on 未删 Antigravity marker'
  Assert-True (-not (Test-Path (Join-Path $configDir 'devin.off'))) 'toggle on 未删 Devin marker'
  Write-Output '[ok] toggle markers'

  $integrationJson = "$(& $exePath integration-status --json 2>&1)" | ConvertFrom-Json
  Assert-True ($LASTEXITCODE -eq 0) "integration-status exit=$LASTEXITCODE"
  Assert-True ($integrationJson.Count -eq 4) "integration-status 应返回 4 个 Agent，实际 $($integrationJson.Count)"
  Write-Output '[ok] integration status contract'

  # 6. Codex 事件解析（DryRun，不发送）
  $payload = '{"last-assistant-message":"hello **world** smoke","input-messages":["帮我写个脚本测试一下"]}'
  $codexDry = & $exePath codex turn-ended $payload -dry-run 2>&1
  Assert-True ($LASTEXITCODE -eq 0) "codex dry-run exit=$LASTEXITCODE"
  $codexText = "$codexDry"
  Assert-True ($codexText -match '【codex】帮我写个脚本测试一下') "codex 标题解析失败：$codexText"
  Write-Output '[ok] codex dry-run passthru'
  $codexJson = $codexText | ConvertFrom-Json
  Assert-True ($codexJson.title -match '【codex】帮我写个脚本测试一下') "codex 标题解析失败：$codexText"
  Assert-True ($codexJson.message -match 'hello world smoke') "codex 摘要未透传：$codexText"

  # 6b. Antigravity / Devin Stop hook 契约（DryRun，不发送）
  $env:AGENT_NOTIFY_ANTIGRAVITY_DRYRUN = '1'
  $antiPayload = '{"conversationId":"anti-smoke","fullyIdle":true}'
  $antiDry = (($antiPayload | & $exePath antigravity stop 2>&1) -join "`n").Trim()
  Assert-True ($LASTEXITCODE -eq 0) "antigravity dry-run exit=$LASTEXITCODE：$antiDry"
  Assert-True ($antiDry -eq '{}') "antigravity hook 必须静默返回空 JSON：$antiDry"
  $env:AGENT_NOTIFY_DEVIN_DRYRUN = '1'
  $devinPayload = '{"session_id":"devin-smoke","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"devin smoke"}'
  $devinDry = (($devinPayload | & $exePath devin stop 2>&1) -join "`n").Trim()
  Assert-True ($LASTEXITCODE -eq 0) "devin dry-run exit=$LASTEXITCODE：$devinDry"
  Assert-True ($devinDry -eq '{}') "devin hook 必须静默返回空 JSON：$devinDry"
  Write-Output '[ok] antigravity/devin stop hooks'

  # 7. 沙箱安装/卸载：只应落盘 exe + 插件 + 安装记录
  $repoBin = Join-Path $RepoRoot 'bin'
  New-Item -ItemType Directory -Force -Path $repoBin | Out-Null
  Copy-Item $exePath (Join-Path $repoBin 'agent-notify.exe') -Force

$sandboxInstall = Join-Path $smokeRoot 'install-bin'
$sandboxPlugins = Join-Path $smokeRoot 'install-plugins'
$sandboxDevinExtension = Join-Path $smokeRoot 'devin-extension'
$antigravityHooks = Join-Path $antigravityConfigDir 'hooks.json'
$devinConfig = Join-Path $devinConfigDir 'config.json'
$sandboxCodexConfig = Join-Path $smokeRoot 'codex-config\config.toml'
New-Item -ItemType Directory -Force -Path $sandboxInstall, $sandboxPlugins, (Split-Path $sandboxCodexConfig -Parent) | Out-Null
  $antigravityFixture = [ordered]@{
    'linkweixin-notify' = [ordered]@{
      Stop = [ordered]@{ type = 'command'; command = 'other.exe antigravity' }
    }
    hooks = [ordered]@{
      Stop = [ordered]@{ type = 'command'; command = 'legacy.exe antigravity'; timeout = 30 }
    }
    keep = [ordered]@{ value = 42 }
    'agent-notify' = [ordered]@{
      Stop = [ordered]@{ type = 'command'; command = '"C:\old\agent-notify.exe" antigravity stop'; timeout = 1 }
    }
  }
  $devinFixture = [ordered]@{
    version = 1
    permissions = [ordered]@{ allow = @('Exec(ls)') }
    hooks = [ordered]@{
      Stop = @(
        [ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = 'other.exe devin'; timeout = 10 }) },
        [ordered]@{
          matcher = ''
          hooks = @(
            [ordered]@{ type = 'command'; command = '"C:\old\agent-notify.exe" devin stop'; timeout = 1 },
            [ordered]@{ type = 'command'; command = 'keep.exe' }
          )
        }
      )
      SessionStart = @([ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = 'session-start.exe' }) })
    }
  }
  [IO.File]::WriteAllText($antigravityHooks, ($antigravityFixture | ConvertTo-Json -Depth 20), $utf8NoBom)
  [IO.File]::WriteAllText($devinConfig, ($devinFixture | ConvertTo-Json -Depth 20), $utf8NoBom)
  $codexFixture = 'notify = [ "C:\\old\\codex-computer-use.exe", "turn-ended", "--previous-notify", "[\"C:/old/agent-notify.exe\",\"codex\",\"turn-ended\"]" ]'
  [IO.File]::WriteAllText($sandboxCodexConfig, $codexFixture, $utf8NoBom)
[IO.File]::WriteAllText((Join-Path $sandboxInstall 'agent-notify.exe'), 'old-install')
[IO.File]::WriteAllText((Join-Path $sandboxPlugins 'agent-notify.ts'), 'old-plugin')
New-Item -ItemType Directory -Force -Path $sandboxDevinExtension | Out-Null
[IO.File]::WriteAllText((Join-Path $sandboxDevinExtension 'keep.txt'), 'keep-extension-data')
Write-Output '[ok] install upgrade fixtures'
  $installOutput = @(& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'install.ps1') `
    -InstallDir $sandboxInstall -PluginDir $sandboxPlugins -AntigravityHooks $antigravityHooks -DevinConfig $devinConfig `
    -DevinExtensionDir $sandboxDevinExtension -CodexConfig $sandboxCodexConfig `
    -SkipShortcuts -SkipWidgetLaunch -SkipLoginLaunch 2>&1)
  Assert-True ($LASTEXITCODE -eq 0) "沙箱安装 exit=$LASTEXITCODE"
  $installText = $installOutput -join "`n"
  Assert-True ($installText -match '当前 Agent 接入状态') "安装器未输出 Agent 接入状态：$installText"
  Assert-True ($installText -match 'Codex: 已接入') "安装器未正确解析并输出 Codex 接入状态：$installText"
  Assert-True (Test-Path (Join-Path $sandboxInstall 'agent-notify.exe')) '沙箱安装缺 exe'
  Assert-True ((Get-PESubsystem (Join-Path $sandboxInstall 'agent-notify.exe')) -eq 2) '安装后的 exe 不是 Windows GUI 子系统'
  Assert-True (Test-Path (Join-Path $sandboxInstall 'agent-notify-install.json')) '沙箱安装缺安装记录'
  Assert-True ((Get-PESubsystem (Join-Path $RepoRoot 'bin\agent-notify.exe')) -eq 2) '安装器没有把 Console 构建重建为 Windows GUI 子系统'
  Assert-True (Test-Path (Join-Path $sandboxPlugins 'agent-notify.ts')) '沙箱安装缺插件'
  Assert-True (Test-Path (Join-Path $sandboxDevinExtension 'package.json')) '沙箱安装缺 Devin 扩展 package.json'
  Assert-True (Test-Path (Join-Path $sandboxDevinExtension 'extension.js')) '沙箱安装缺 Devin 扩展入口'
  Assert-True (Test-Path (Join-Path $sandboxDevinExtension 'acp-bridge.js')) '沙箱安装缺 Devin ACP 通道模块'
  $installedDevinManifest = Get-Content -LiteralPath (Join-Path $sandboxDevinExtension 'package.json') -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True ($installedDevinManifest.name -eq 'agent-notify-reply' -and $installedDevinManifest.publisher -eq 'agent-notify') 'Devin 扩展归属信息异常'
  $atomicLeftovers = @(Get-ChildItem -Path $sandboxInstall, $sandboxPlugins -File | Where-Object { $_.Name -like '*.new-*' })
  Assert-True ($atomicLeftovers.Count -eq 0) '原子安装残留替换临时文件'
  $installedPluginText = [IO.File]::ReadAllText((Join-Path $sandboxPlugins 'agent-notify.ts'))
  $expectedBaked = (Join-Path $sandboxInstall 'agent-notify.exe').Replace('\', '\\')
  Assert-True ($installedPluginText.Contains('const BAKED_BIN = "' + $expectedBaked + '"')) "安装后的插件没有指向沙箱 exe：$expectedBaked"
  Assert-True ($pluginRaw.Contains('const BAKED_BIN = ""')) '仓库内的插件副本应保持可移植的空 BAKED_BIN'
  $installedCodexLine = [regex]::Match([IO.File]::ReadAllText($sandboxCodexConfig), '(?m)^notify\s*=.*$').Value
  $expectedCodexTarget = (Join-Path $sandboxInstall 'agent-notify.exe').Replace('\', '/')
  Assert-True ($installedCodexLine -match '(?i)codex-computer-use\.exe') "安装器不应拆掉 Codex computer-use 包装链：$installedCodexLine"
  Assert-True ($installedCodexLine.Contains($expectedCodexTarget)) "安装器未把链内 agent-notify 路径更新到沙箱 exe：$installedCodexLine"
  Assert-True (Test-Path -LiteralPath "$sandboxCodexConfig.bak-notify-wrapper" -PathType Leaf) '更新 Codex notify 链前未备份 config.toml'
  Write-Output '[ok] install baked plugin path'
  $installedFiles = @(Get-ChildItem $sandboxInstall -File | Select-Object -ExpandProperty Name | Sort-Object)
  $expectedInstalledFiles = @('VERSION', 'agent-notify-install.json', 'agent-notify.exe', 'install.ps1', 'uninstall.ps1') | Sort-Object
  Assert-True (($installedFiles -join ',') -eq ($expectedInstalledFiles -join ',')) "安装目录文件意外：$($installedFiles -join ',')"
  foreach ($relative in @(
      'install.ps1',
      'uninstall.ps1',
      'VERSION',
      'tools\hook-config.ps1',
      'plugin\agent-notify.ts',
      'plugin\devin-extension\package.json',
      'plugin\devin-extension\extension.js',
      'plugin\devin-extension\acp-bridge.js'
    )) {
    Assert-True (Test-Path -LiteralPath (Join-Path $sandboxInstall $relative) -PathType Leaf) "安装目录缺自举文件：$relative"
  }
  $record = Get-Content (Join-Path $sandboxInstall 'agent-notify-install.json') -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True ($record.name -eq 'Agent-notify') "安装记录 name 异常：$($record.name)"
  Assert-True (@($record.files) -contains 'agent-notify.exe') '安装记录缺 exe'
  Assert-True (@($record.files) -contains 'install.ps1') '安装记录缺自举文件 install.ps1'
  Assert-True (@($record.files) -contains 'tools/hook-config.ps1') '安装记录缺自举文件 tools/hook-config.ps1'
  Assert-True (@($record.files).Count -eq 9) "安装记录 files 数量异常：$(@($record.files).Count)"
  Write-Output '[ok] install sandbox files + record'

  # 7b. 仅配置模式只更新用户级接入，不替换已安装 exe。
  $configureOnlyInstall = Join-Path $smokeRoot 'configure-only-app'
  $configureOnlyPlugins = Join-Path $smokeRoot 'configure-only-plugins'
  $configureOnlyDevinExtension = Join-Path $smokeRoot 'configure-only-devin-extension'
  $configureOnlySourcePlugin = Join-Path $configureOnlyInstall 'plugin'
  $configureOnlyTools = Join-Path $configureOnlyInstall 'tools'
  New-Item -ItemType Directory -Force -Path $configureOnlySourcePlugin, (Join-Path $configureOnlySourcePlugin 'devin-extension'), $configureOnlyTools | Out-Null
  Copy-Item -LiteralPath $exePath -Destination (Join-Path $configureOnlyInstall 'agent-notify.exe') -Force
  Copy-Item -LiteralPath (Join-Path $RepoRoot 'install.ps1') -Destination (Join-Path $configureOnlyInstall 'install.ps1') -Force
  Copy-Item -LiteralPath (Join-Path $RepoRoot 'VERSION') -Destination (Join-Path $configureOnlyInstall 'VERSION') -Force
  Copy-Item -LiteralPath (Join-Path $RepoRoot 'tools\hook-config.ps1') -Destination (Join-Path $configureOnlyTools 'hook-config.ps1') -Force
  Copy-Item -LiteralPath (Join-Path $RepoRoot 'plugin\agent-notify.ts') -Destination (Join-Path $configureOnlySourcePlugin 'agent-notify.ts') -Force
  foreach ($name in @('package.json', 'extension.js', 'acp-bridge.js')) {
    Copy-Item -LiteralPath (Join-Path $RepoRoot "plugin\devin-extension\$name") -Destination (Join-Path $configureOnlySourcePlugin "devin-extension\$name") -Force
  }
  $configureOnlyOutput = @(& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $configureOnlyInstall 'install.ps1') `
    -ConfigureOnly `
    -InstallDir $configureOnlyInstall `
    -PluginDir $configureOnlyPlugins `
    -DevinExtensionDir $configureOnlyDevinExtension `
    -CodexConfig (Join-Path $smokeRoot 'configure-only-codex\config.toml') `
    -AntigravityHooks (Join-Path $smokeRoot 'configure-only-antigravity\hooks.json') `
    -DevinConfig (Join-Path $smokeRoot 'configure-only-devin\config.json') `
    -SkipShortcuts `
    -SkipWidgetLaunch `
    -SkipLoginLaunch 2>&1)
  Assert-True ($LASTEXITCODE -eq 0) "ConfigureOnly 安装失败：$($configureOnlyOutput -join [Environment]::NewLine)"
  Assert-True (Test-Path -LiteralPath (Join-Path $configureOnlyPlugins 'agent-notify.ts')) 'ConfigureOnly 未安装 OpenCode 插件'
  Assert-True (Test-Path -LiteralPath (Join-Path $configureOnlyDevinExtension 'package.json')) 'ConfigureOnly 未安装 Devin 扩展'
  Assert-True (Test-Path -LiteralPath (Join-Path $configureOnlyInstall 'agent-notify-install.json')) 'ConfigureOnly 未写安装记录'
  Write-Output '[ok] configure-only install'

  # 8. Hook 安装只替换 Agent-notify 自己的 handler，保留其他配置。
  $installedAntigravity = Get-Content -LiteralPath $antigravityHooks -Raw -Encoding utf8 | ConvertFrom-Json
  $installedDevin = Get-Content -LiteralPath $devinConfig -Raw -Encoding utf8 | ConvertFrom-Json
  $expectedAntigravityCommand = '.\agent-notify-hook.cmd antigravity stop'
  $expectedAntigravityLauncher = Join-Path $antigravityConfigDir 'agent-notify-hook.cmd'
  $expectedDevinCommand = '"' + (Join-Path $sandboxInstall 'agent-notify.exe') + '" devin stop'
  Assert-True ($installedAntigravity.keep.value -eq 42) '安装后 Antigravity 丢失无关配置'
  Assert-True (@($installedAntigravity.'linkweixin-notify'.Stop)[0].command -eq 'other.exe antigravity') '安装后 Antigravity 丢失其他 Hook'
  Assert-True (@($installedAntigravity.'linkweixin-notify'.Stop).Count -eq 1) '安装后旧版 Antigravity Stop 未规范为数组'
  Assert-True (@($installedAntigravity.hooks.Stop)[0].command -eq 'legacy.exe antigravity') '安装后 hooks.Stop 未规范为数组或丢失命令'
  Assert-True (@($installedAntigravity.hooks.Stop).Count -eq 1) '安装后 hooks.Stop 数量异常'
  Assert-True (@($installedAntigravity.'agent-notify'.Stop).Count -eq 1) 'Antigravity Agent-notify Stop 数量异常'
  Assert-True ($installedAntigravity.'agent-notify'.Stop[0].command -eq $expectedAntigravityCommand) 'Antigravity Hook 未指向沙箱 exe'
  Assert-True ($installedAntigravity.'agent-notify'.Stop[0].timeout -eq 60) 'Antigravity Hook timeout 应为 60 秒'
  Assert-True (Test-Path -LiteralPath $expectedAntigravityLauncher -PathType Leaf) 'Antigravity Hook 启动器缺失'
  $launcherText = [IO.File]::ReadAllText($expectedAntigravityLauncher)
  Assert-True ($launcherText.Contains('@rem agent-notify-antigravity-launcher')) 'Antigravity Hook 启动器缺少归属标记'
  Assert-True ($launcherText.Contains('"' + (Join-Path $sandboxInstall 'agent-notify.exe') + '" antigravity stop')) 'Antigravity Hook 启动器未指向沙箱 exe'
  $previousLocation = (Get-Location).Path
  try {
    Set-Location -LiteralPath $antigravityConfigDir
    $env:AGENT_NOTIFY_ANTIGRAVITY_DRYRUN = '1'
    $launcherOutput = (($antiPayload | & cmd.exe /d /c $expectedAntigravityCommand 2>&1) -join "`n").Trim()
    Assert-True ($LASTEXITCODE -eq 0) "Antigravity 启动器执行失败 exit=$LASTEXITCODE：$launcherOutput"
    Assert-True ($launcherOutput -eq '{}') "Antigravity 启动器输出异常：$launcherOutput"
  } finally {
    Set-Location -LiteralPath $previousLocation
  }
  Assert-True (@($installedDevin.hooks.Stop).Count -eq 3) 'Devin Stop 组数量异常'
  $devinCommands = @($installedDevin.hooks.Stop | ForEach-Object { @($_.hooks) | ForEach-Object { [string]$_.command } })
  Assert-True (@($devinCommands | Where-Object { $_ -eq $expectedDevinCommand }).Count -eq 1) 'Devin Hook 未指向沙箱 exe'
  Assert-True ($devinCommands -contains 'other.exe devin') '安装后 Devin 丢失其他 Stop Hook'
  Assert-True ($devinCommands -contains 'keep.exe') '安装后 Devin 丢失同组其他 handler'
  Assert-True (@($installedDevin.hooks.SessionStart).Count -eq 1) '安装后 Devin 丢失其他事件 Hook'
  Assert-True (@($installedDevin.permissions.allow) -contains 'Exec(ls)') '安装后 Devin 丢失权限配置'
  $hookAtomicLeftovers = @(Get-ChildItem -Path $antigravityConfigDir, $devinConfigDir -File | Where-Object { $_.Name -like '*.new-*' })
  Assert-True ($hookAtomicLeftovers.Count -eq 0) 'Hook 配置残留原子替换临时文件'
  Write-Output '[ok] install preserves hook config'

  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'uninstall.ps1') `
    -InstallDir $sandboxInstall -PluginDir $sandboxPlugins -AntigravityHooks $antigravityHooks -DevinConfig $devinConfig `
    -DevinExtensionDir $sandboxDevinExtension -CodexConfig $sandboxCodexConfig `
    -SkipShortcuts -SkipProcessStop | Out-Null
  Assert-True ($LASTEXITCODE -eq 0) "沙箱卸载 exit=$LASTEXITCODE"
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'agent-notify.exe'))) '沙箱卸载残留 exe'
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'agent-notify-install.json'))) '沙箱卸载残留安装记录'
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'install.ps1'))) '沙箱卸载残留 install.ps1'
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'plugin\agent-notify.ts'))) '沙箱卸载残留安装目录内的插件副本'
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'tools\hook-config.ps1'))) '沙箱卸载残留 hook-config.ps1'
  Assert-True (-not (Test-Path (Join-Path $sandboxPlugins 'agent-notify.ts'))) '沙箱卸载残留插件'
  Assert-True (-not (Test-Path (Join-Path $sandboxDevinExtension 'package.json'))) '沙箱卸载残留 Devin 扩展 package.json'
  Assert-True (-not (Test-Path (Join-Path $sandboxDevinExtension 'extension.js'))) '沙箱卸载残留 Devin 扩展入口'
  Assert-True (-not (Test-Path (Join-Path $sandboxDevinExtension 'acp-bridge.js'))) '沙箱卸载残留 Devin ACP 通道模块'
  Assert-True (Test-Path (Join-Path $sandboxDevinExtension 'keep.txt')) '卸载误删 Devin 扩展目录中的其他文件'
  $uninstalledAntigravity = Get-Content -LiteralPath $antigravityHooks -Raw -Encoding utf8 | ConvertFrom-Json
  $uninstalledDevin = Get-Content -LiteralPath $devinConfig -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True ($null -eq $uninstalledAntigravity.PSObject.Properties['agent-notify']) '卸载后残留 Antigravity Hook'
  Assert-True (-not (Test-Path -LiteralPath $expectedAntigravityLauncher)) '卸载后残留 Antigravity Hook 启动器'
  Assert-True ($uninstalledAntigravity.keep.value -eq 42) '卸载后 Antigravity 丢失无关配置'
  Assert-True (@($uninstalledAntigravity.'linkweixin-notify'.Stop)[0].command -eq 'other.exe antigravity') '卸载后 Antigravity 丢失其他 Hook'
  Assert-True (@($uninstalledAntigravity.'linkweixin-notify'.Stop).Count -eq 1) '卸载后旧版 Antigravity Stop 未保持数组'
  Assert-True (@($uninstalledAntigravity.hooks.Stop)[0].command -eq 'legacy.exe antigravity') '卸载后 hooks.Stop 未保持数组或丢失命令'
  $devinCommandsAfter = @($uninstalledDevin.hooks.Stop | ForEach-Object { @($_.hooks) | ForEach-Object { [string]$_.command } })
  Assert-True (@($devinCommandsAfter | Where-Object { $_ -match 'agent-notify.*devin stop' }).Count -eq 0) '卸载后残留 Devin Hook'
  Assert-True ($devinCommandsAfter -contains 'other.exe devin') '卸载后 Devin 丢失其他 Stop Hook'
  Assert-True ($devinCommandsAfter -contains 'keep.exe') '卸载后 Devin 丢失同组其他 handler'
  Assert-True (@($uninstalledDevin.hooks.SessionStart).Count -eq 1) '卸载后 Devin 丢失其他事件 Hook'
  Assert-True (@($uninstalledDevin.permissions.allow) -contains 'Exec(ls)') '卸载后 Devin 丢失权限配置'
  Write-Output '[ok] uninstall preserves hook config'
  Write-Output '[ok] uninstall sandbox clean'
} finally {
  $env:AGENT_NOTIFY_CONFIG_DIR = $null
  $env:AGENT_NOTIFY_TEMP_DIR = $null
  $env:AGENT_NOTIFY_CONFIG_FILE = $null
  $env:AGENT_NOTIFY_CREDENTIAL_FILE = $null
  $env:AGENT_NOTIFY_ANTIGRAVITY_DRYRUN = $null
  $env:AGENT_NOTIFY_DEVIN_DRYRUN = $null

  # 清理沙箱：只逐个删除明确的文件路径，再逐个删除已空目录
  $explicitFiles = @(
    (Join-Path $smokeRoot 'bin\agent-notify.exe'),
    (Join-Path $configDir 'config.json'),
    (Join-Path $configDir 'clawbot.json'),
    (Join-Path $configDir 'reply-routes.jsonl.lock'),
    (Join-Path $configDir 'reply-routes.jsonl'),
    (Join-Path $configDir 'opencode.off'),
    (Join-Path $configDir 'codex.off'),
    (Join-Path $configDir 'antigravity.off'),
    (Join-Path $configDir 'devin.off'),
    (Join-Path $smokeRoot 'install-bin\agent-notify.exe'),
    (Join-Path $smokeRoot 'install-bin\agent-notify-install.json'),
    (Join-Path $smokeRoot 'install-bin\install.ps1'),
    (Join-Path $smokeRoot 'install-bin\uninstall.ps1'),
    (Join-Path $smokeRoot 'install-bin\VERSION'),
    (Join-Path $smokeRoot 'install-bin\tools\hook-config.ps1'),
    (Join-Path $smokeRoot 'install-bin\plugin\agent-notify.ts'),
    (Join-Path $smokeRoot 'install-bin\plugin\devin-extension\package.json'),
    (Join-Path $smokeRoot 'install-bin\plugin\devin-extension\extension.js'),
    (Join-Path $smokeRoot 'install-bin\plugin\devin-extension\acp-bridge.js'),
    (Join-Path $smokeRoot 'install-plugins\agent-notify.ts'),
    (Join-Path $smokeRoot 'configure-only-app\agent-notify.exe'),
    (Join-Path $smokeRoot 'configure-only-app\install.ps1'),
    (Join-Path $smokeRoot 'configure-only-app\VERSION'),
    (Join-Path $smokeRoot 'configure-only-app\agent-notify-install.json'),
    (Join-Path $smokeRoot 'configure-only-app\plugin\agent-notify.ts'),
    (Join-Path $smokeRoot 'configure-only-app\plugin\devin-extension\package.json'),
    (Join-Path $smokeRoot 'configure-only-app\plugin\devin-extension\extension.js'),
    (Join-Path $smokeRoot 'configure-only-app\plugin\devin-extension\acp-bridge.js'),
    (Join-Path $smokeRoot 'configure-only-app\tools\hook-config.ps1'),
    (Join-Path $smokeRoot 'configure-only-plugins\agent-notify.ts'),
    (Join-Path $smokeRoot 'configure-only-devin-extension\package.json'),
    (Join-Path $smokeRoot 'configure-only-devin-extension\extension.js'),
    (Join-Path $smokeRoot 'configure-only-devin-extension\acp-bridge.js'),
    (Join-Path $smokeRoot 'devin-extension\package.json'),
    (Join-Path $smokeRoot 'devin-extension\extension.js'),
    (Join-Path $smokeRoot 'devin-extension\acp-bridge.js'),
    (Join-Path $smokeRoot 'devin-extension\keep.txt'),
    (Join-Path $smokeRoot 'antigravity-config\hooks.json'),
    (Join-Path $smokeRoot 'devin-config\config.json'),
    (Join-Path $smokeRoot 'codex-config\config.toml'),
    (Join-Path $smokeRoot 'codex-config\config.toml.bak-notify-wrapper'),
    (Join-Path $smokeRoot 'state\push.log'),
    (Join-Path $smokeRoot 'state\codex-notify-debug.log'),
    (Join-Path $smokeRoot 'state\opencode-sent.json'),
    (Join-Path $smokeRoot 'state\clawbot-debug.log')
  )
  foreach ($file in $explicitFiles) {
    if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue }
  }
  foreach ($dir in @(
      (Join-Path $smokeRoot 'state'),
      $configDir,
      $antigravityConfigDir,
      $devinConfigDir,
      (Join-Path $smokeRoot 'codex-config'),
      $pluginDir,
      $binDir,
      (Join-Path $smokeRoot 'install-bin\plugin\devin-extension'),
      (Join-Path $smokeRoot 'install-bin\plugin'),
      (Join-Path $smokeRoot 'install-bin\tools'),
      (Join-Path $smokeRoot 'install-bin'),
      (Join-Path $smokeRoot 'install-plugins'),
      (Join-Path $smokeRoot 'configure-only-app\plugin\devin-extension'),
      (Join-Path $smokeRoot 'configure-only-app\plugin'),
      (Join-Path $smokeRoot 'configure-only-app\tools'),
      (Join-Path $smokeRoot 'configure-only-app'),
      (Join-Path $smokeRoot 'configure-only-plugins'),
      (Join-Path $smokeRoot 'configure-only-devin-extension'),
      $sandboxDevinExtension,
      $smokeRoot
    )) {
    if (Test-Path -LiteralPath $dir) {
      try { [IO.Directory]::Delete($dir, $false) } catch { Write-Warning "清理空目录失败：$dir - $($_.Exception.Message)" }
    }
  }
}

Write-Output 'SMOKE ALL GREEN'
