#Requires -Version 5.1
<#
.SYNOPSIS
  校验版本号在全部发布位置保持一致：以 VERSION 为唯一来源，也可用 -Version 指定（发版时传 tag）。

.DESCRIPTION
  VERSION 自 2026-09-21 起成为唯一版本来源（此前是 internal/app/version.go）。检查：
  VERSION、hosts/desktop-tauri/tauri.conf.json、Cargo.toml 的 [workspace.package] version、
  README 徽章、plugin/devin-extension/package.json 与 plugin/devin-extension-v2/package.json、CHANGELOG，以及安装包名规则。
  本地与 CI 共用同一入口：tools\lint.ps1 会调用它，release workflow 用 tag 调用它。

  为什么不再检查 internal/app/version.go：Go 版 UI 已不再作为发布入口（Task 10 切换），
  它保留在仓库中只为回滚窗口，版本号不再随产品发布变化。Cargo 版本直接读 Cargo.toml
  而不调用 cargo metadata，是为了让 lint 不依赖 Rust 工具链、也不触发锁文件改写。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools/check-version.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools/check-version.ps1 -Version 2.0.1
#>
param(
  [string]$Version
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
$failures = New-Object System.Collections.Generic.List[string]

function Read-TextFile {
  param([string]$RelativePath)
  $path = Join-Path $RepoRoot $RelativePath
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    $failures.Add("缺少文件：$RelativePath")
    return ''
  }
  return [IO.File]::ReadAllText($path)
}

$versionFileContent = Read-TextFile 'VERSION'
if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = $versionFileContent.Trim()
}
$Version = $Version.Trim().TrimStart('v')
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "版本号格式无效：$Version（期望 x.y.z）"
}
$escaped = [regex]::Escape($Version)

if ($versionFileContent.Trim() -ne $Version) {
  $failures.Add("VERSION 不是 $Version")
}

$tauri = Read-TextFile 'hosts\desktop-tauri\tauri.conf.json'
if ($tauri -notmatch ('(?m)^\s*"version"\s*:\s*"' + $escaped + '"\s*,?\s*$')) {
  $failures.Add("hosts/desktop-tauri/tauri.conf.json 的顶层 version 不是 $Version")
}

$cargo = Read-TextFile 'Cargo.toml'
$cargoSection = [regex]::Match($cargo, '(?ms)^\[workspace\.package\][^\[]*')
if (-not $cargoSection.Success) {
  $failures.Add('Cargo.toml 缺少 [workspace.package] 段')
} elseif ($cargoSection.Value -notmatch ('(?m)^\s*version\s*=\s*"' + $escaped + '"\s*$')) {
  $failures.Add("Cargo.toml 的 [workspace.package] version 不是 $Version（CARGO_PKG_VERSION 由它决定）")
}

$readme = Read-TextFile 'README.md'
if ($readme -notmatch ('badge/version-' + $escaped + '-')) {
  $failures.Add("README 版本徽章不是 $Version")
}

# Devin 扩展两代并存：V1（Go 版遗留）与 V2（桌面版）都随安装包分发，版本必须与产品一致。
foreach ($extensionRelative in @('plugin\devin-extension\package.json', 'plugin\devin-extension-v2\package.json')) {
  $package = Read-TextFile $extensionRelative
  if ($package -notmatch ('(?m)^\s*"version"\s*:\s*"' + $escaped + '"\s*,?\s*$')) {
    $failures.Add("Devin 扩展 $extensionRelative 版本不是 $Version")
  }
}

# 阶段 D 的其余分发物没有独立产品版本字段，无需在这里校验：
# - 三个 Hook exe（agentnotify-codex-hook / -antigravity-hook / -devin-hook）的版本来自
#   Cargo.toml 的 [workspace.package] version，已由上面的 CARGO_PKG_VERSION 检查覆盖；
# - plugin\commandcode-v2\agent-notify.ts 是单文件 mod，只有协议版本 PROTOCOL_VERSION=1，
#   没有产品版本字段（Command Code 也不读版本），不为了过门禁往 mod 里塞无用字段。

if ((Read-TextFile 'CHANGELOG.md') -notmatch ('(?m)^## \[' + $escaped + '\]')) {
  $failures.Add("CHANGELOG 缺少 $Version 段落")
}

# 安装包名必须仍然由 AppVersion 决定，发布产物才可被 update 模块按版本精确匹配。
$installer = Read-TextFile 'installer\agent-notify.iss'
if ($installer -notmatch '(?m)^OutputBaseFilename=Agent-notify-Setup-v\{#AppVersion\}\s*$') {
  $failures.Add('安装器脚本的 OutputBaseFilename 不是 Agent-notify-Setup-v{#AppVersion}')
}

if ($failures.Count -gt 0) {
  foreach ($item in $failures) { Write-Output "[version] $item" }
  exit 1
}

Write-Output "[version] $Version 在所有发布位置一致"
exit 0
