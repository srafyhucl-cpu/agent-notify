#Requires -Version 5.1
<#
.SYNOPSIS
  校验版本号在全部发布位置保持一致：以 internal/app/version.go 为准，也可用 -Version 指定（发版时传 tag）。
.DESCRIPTION
  检查 VERSION、cmd/agent-notify/agent-notify.manifest、README 徽章、internal/clawbot/types.go 的
  BotAgent、plugin/devin-extension/package.json 以及 CHANGELOG 是否都包含该版本。
  本地与 CI 共用同一入口：tools\lint.ps1 会调用它，release workflow 用 tag 调用它。
.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\check-version.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\check-version.ps1 -Version 1.8.0
#>
param(
  [string]$Version
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
$failures = New-Object System.Collections.Generic.List[string]

function Read-VersionFile {
  param([string]$RelativePath)
  $path = Join-Path $RepoRoot $RelativePath
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    $failures.Add("缺少文件：$RelativePath")
    return ''
  }
  return [IO.File]::ReadAllText($path)
}

if ([string]::IsNullOrWhiteSpace($Version)) {
  $versionSource = Read-VersionFile 'internal\app\version.go'
  $match = [regex]::Match($versionSource, 'Version\s*=\s*"([^"]+)"')
  if (-not $match.Success) {
    throw '无法从 internal/app/version.go 读取 Version'
  }
  $Version = $match.Groups[1].Value
}
$Version = $Version.Trim().TrimStart('v')
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "版本号格式无效：$Version（期望 x.y.z）"
}
$escaped = [regex]::Escape($Version)

$versionSource = Read-VersionFile 'internal\app\version.go'
if ($versionSource -notmatch ('Version\s*=\s*"' + $escaped + '"')) {
  $failures.Add("internal/app/version.go 的 Version 不是 $Version")
}

if ((Read-VersionFile 'VERSION').Trim() -ne $Version) {
  $failures.Add("VERSION 不是 $Version")
}

$manifest = Read-VersionFile 'cmd\agent-notify\agent-notify.manifest'
if ($manifest -notmatch ('version="' + $escaped + '\.0"')) {
  $failures.Add("agent-notify.manifest 不是 $Version.0")
}

if ((Read-VersionFile 'README.md') -notmatch ('badge/version-' + $escaped + '-')) {
  $failures.Add("README 版本徽章不是 $Version")
}

$types = Read-VersionFile 'internal\clawbot\types.go'
if ($types -notmatch ('Agent-notify/' + $escaped + ' \(windows\)')) {
  $failures.Add("clawbot BotAgent 不是 Agent-notify/$Version (windows)")
}

$package = Read-VersionFile 'plugin\devin-extension\package.json'
if ($package -notmatch ('"version":\s*"' + $escaped + '"')) {
  $failures.Add("Devin 扩展 package.json 版本不是 $Version")
}

if ((Read-VersionFile 'CHANGELOG.md') -notmatch ('(?m)^## \[' + $escaped + '\]')) {
  $failures.Add("CHANGELOG 缺少 $Version 段落")
}

if ($failures.Count -gt 0) {
  foreach ($item in $failures) { Write-Output "[version] $item" }
  exit 1
}

Write-Output "[version] $Version 在所有发布位置一致"
exit 0
