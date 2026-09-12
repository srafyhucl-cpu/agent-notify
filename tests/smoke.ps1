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
foreach ($needle in @('agent-notify.exe', 'AGENT_NOTIFY_BIN', 'notify",')) {
  Assert-True ($pluginRaw -match [regex]::Escape($needle)) "插件缺少新协议标记：$needle"
}
$legacyNames = @(('link' + 'Weixin'), ('link' + 'weixin'), ('PUSH' + 'PLUS'), ('Push' + 'Plus'), ('power' + 'shell.exe'), ('notify' + '-ai.ps1'), ('anti' + 'gravity'))
foreach ($legacy in $legacyNames) {
  Assert-True (-not ($pluginRaw -match [regex]::Escape($legacy))) "插件仍残留旧实现：$legacy"
}
Write-Output '[ok] plugin only targets agent-notify.exe'

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
New-Item -ItemType Directory -Force -Path $binDir, $pluginDir, $configDir | Out-Null

$exePath = Join-Path $binDir 'agent-notify.exe'
& $goExe build -ldflags '-s -w' -trimpath -o $exePath '.\cmd\agent-notify\'
if ($LASTEXITCODE -ne 0) { throw "go build 失败 exit=$LASTEXITCODE" }
Write-Output '[ok] go build'

# 隔离环境：所有状态都落在沙箱里，绝不碰真实用户配置
$env:AGENT_NOTIFY_CONFIG_DIR = $configDir
$env:AGENT_NOTIFY_TEMP_DIR = (Join-Path $smokeRoot 'state')
$env:AGENT_NOTIFY_CONFIG_FILE = Join-Path $configDir 'config.json'
$env:AGENT_NOTIFY_CREDENTIAL_FILE = Join-Path $configDir 'clawbot.json'

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
  Write-Output '[ok] status json'

  # 4b. history JSON：空历史也要输出合法 JSON，脚本才不用区分文本提示
  $historyRaw = "$(& $exePath history --limit 5 --json 2>&1)".Trim()
  Assert-True ($LASTEXITCODE -eq 0) "history --json exit=$LASTEXITCODE"
  Assert-True ($historyRaw -eq '[]') "history --json 空历史应为 []：$historyRaw"
  Write-Output '[ok] history json'

  # 5. toggle 开关 marker
  & $exePath toggle --agent all --off 2>&1 | Out-Null
  Assert-True ($LASTEXITCODE -eq 0) "toggle off exit=$LASTEXITCODE"
  Assert-True (Test-Path (Join-Path $configDir 'opencode.off')) 'toggle off 未建 OpenCode marker'
  Assert-True (Test-Path (Join-Path $configDir 'codex.off')) 'toggle off 未建 Codex marker'
  $offJson = "$(& $exePath status --json 2>&1)" | ConvertFrom-Json
  Assert-True ($offJson.openCodeEnabled -eq $false) 'toggle off 后 OpenCode 应为关闭'
  & $exePath toggle --agent all --on 2>&1 | Out-Null
  Assert-True (-not (Test-Path (Join-Path $configDir 'opencode.off'))) 'toggle on 未删 OpenCode marker'
  Assert-True (-not (Test-Path (Join-Path $configDir 'codex.off'))) 'toggle on 未删 Codex marker'
  Write-Output '[ok] toggle markers'

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

  # 7. 沙箱安装/卸载：只应落盘 exe + 插件 + 安装记录
  $repoBin = Join-Path $RepoRoot 'bin'
  New-Item -ItemType Directory -Force -Path $repoBin | Out-Null
  Copy-Item $exePath (Join-Path $repoBin 'agent-notify.exe') -Force

  $sandboxInstall = Join-Path $smokeRoot 'install-bin'
  $sandboxPlugins = Join-Path $smokeRoot 'install-plugins'
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'install.ps1') `
    -InstallDir $sandboxInstall -PluginDir $sandboxPlugins -SkipCodexConfig -SkipShortcuts -SkipWidgetLaunch | Out-Null
  Assert-True ($LASTEXITCODE -eq 0) "沙箱安装 exit=$LASTEXITCODE"
  Assert-True (Test-Path (Join-Path $sandboxInstall 'agent-notify.exe')) '沙箱安装缺 exe'
  Assert-True ((Get-PESubsystem (Join-Path $sandboxInstall 'agent-notify.exe')) -eq 2) '安装后的 exe 不是 Windows GUI 子系统'
  Assert-True (Test-Path (Join-Path $sandboxInstall 'agent-notify-install.json')) '沙箱安装缺安装记录'
  Assert-True ((Get-PESubsystem (Join-Path $RepoRoot 'bin\agent-notify.exe')) -eq 2) '安装器没有把 Console 构建重建为 Windows GUI 子系统'
  Assert-True (Test-Path (Join-Path $sandboxPlugins 'agent-notify.ts')) '沙箱安装缺插件'
  $installedPluginText = [IO.File]::ReadAllText((Join-Path $sandboxPlugins 'agent-notify.ts'))
  $expectedBaked = (Join-Path $sandboxInstall 'agent-notify.exe').Replace('\', '\\')
  Assert-True ($installedPluginText.Contains('const BAKED_BIN = "' + $expectedBaked + '"')) "安装后的插件没有指向沙箱 exe：$expectedBaked"
  Assert-True ($pluginRaw.Contains('const BAKED_BIN = ""')) '仓库内的插件副本应保持可移植的空 BAKED_BIN'
  Write-Output '[ok] install baked plugin path'
  $installedFiles = @(Get-ChildItem $sandboxInstall -File | Select-Object -ExpandProperty Name | Sort-Object)
  Assert-True (($installedFiles -join ',') -eq 'agent-notify.exe,agent-notify-install.json') "安装目录文件意外：$($installedFiles -join ',')"
  $record = Get-Content (Join-Path $sandboxInstall 'agent-notify-install.json') -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True ($record.name -eq 'Agent-notify') "安装记录 name 异常：$($record.name)"
  Assert-True (@($record.files) -contains 'agent-notify.exe') '安装记录缺 exe'
  Write-Output '[ok] install sandbox files + record'

  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'uninstall.ps1') `
    -InstallDir $sandboxInstall -PluginDir $sandboxPlugins -SkipCodexConfig -SkipShortcuts -SkipProcessStop | Out-Null
  Assert-True ($LASTEXITCODE -eq 0) "沙箱卸载 exit=$LASTEXITCODE"
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'agent-notify.exe'))) '沙箱卸载残留 exe'
  Assert-True (-not (Test-Path (Join-Path $sandboxInstall 'agent-notify-install.json'))) '沙箱卸载残留安装记录'
  Assert-True (-not (Test-Path (Join-Path $sandboxPlugins 'agent-notify.ts'))) '沙箱卸载残留插件'
  Write-Output '[ok] uninstall sandbox clean'
} finally {
  $env:AGENT_NOTIFY_CONFIG_DIR = $null
  $env:AGENT_NOTIFY_TEMP_DIR = $null
  $env:AGENT_NOTIFY_CONFIG_FILE = $null
  $env:AGENT_NOTIFY_CREDENTIAL_FILE = $null

  # 清理沙箱：只逐个删除明确的文件路径，再逐个删除已空目录
  $explicitFiles = @(
    (Join-Path $smokeRoot 'bin\agent-notify.exe'),
    (Join-Path $configDir 'config.json'),
    (Join-Path $configDir 'clawbot.json'),
    (Join-Path $configDir 'opencode.off'),
    (Join-Path $configDir 'codex.off'),
    (Join-Path $smokeRoot 'install-bin\agent-notify.exe'),
    (Join-Path $smokeRoot 'install-bin\agent-notify-install.json'),
    (Join-Path $smokeRoot 'install-plugins\agent-notify.ts'),
    (Join-Path $smokeRoot 'state\push.log'),
    (Join-Path $smokeRoot 'state\opencode-sent.json')
  )
  foreach ($file in $explicitFiles) {
    if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue }
  }
  foreach ($dir in @(
      (Join-Path $smokeRoot 'state'),
      $configDir,
      $pluginDir,
      $binDir,
      (Join-Path $smokeRoot 'install-bin'),
      (Join-Path $smokeRoot 'install-plugins'),
      $smokeRoot
    )) {
    if (Test-Path -LiteralPath $dir) {
      try { [IO.Directory]::Delete($dir, $false) } catch { Write-Warning "清理空目录失败：$dir - $($_.Exception.Message)" }
    }
  }
}

Write-Output 'SMOKE ALL GREEN'
