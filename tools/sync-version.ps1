#Requires -Version 5.1
<#
.SYNOPSIS
  把 VERSION 的值同步到所有需要字面版本号的发布位置。

.DESCRIPTION
  VERSION 是唯一的版本来源。本脚本把它的值写入：
    - hosts/desktop-tauri/tauri.conf.json 的顶层 version
    - Cargo.toml 的 [workspace.package] version（决定 CARGO_PKG_VERSION 与桌面端显示的版本）
    - plugin/devin-extension/package.json 与 plugin/devin-extension-v2/package.json 的 version（随安装包一起分发）
  幂等：值已一致时不改写文件，也不产生无意义的时间戳变化。

  不在构建过程中自动改写：改 Cargo 版本会让 Cargo.lock 变脏，因此发版时先跑本脚本并提交，
  一致性由 tools/check-version.ps1 在门禁里校验（tools/lint.ps1 会调用它）。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools/sync-version.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools/sync-version.ps1 -Version 2.0.1
#>
param(
  [string]$Version
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent

if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = ([IO.File]::ReadAllText((Join-Path $RepoRoot 'VERSION'))).Trim()
} else {
  $Version = $Version.Trim().TrimStart('v')
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "版本号格式无效：$Version（期望 x.y.z）"
}

$changed = New-Object System.Collections.Generic.List[string]
$utf8NoBom = New-Object Text.UTF8Encoding($false)

function Save-IfChanged {
  param([string]$RelativePath, [string]$Content)
  $path = Join-Path $RepoRoot $RelativePath
  if ([IO.File]::ReadAllText($path) -eq $Content) { return }
  [IO.File]::WriteAllText($path, $Content, $utf8NoBom)
  [void]$changed.Add($RelativePath)
}

# VERSION 自身：仅在显式传 -Version 且内容不同时更新，保持原有的行尾风格。
$versionPath = Join-Path $RepoRoot 'VERSION'
if ([IO.File]::ReadAllText($versionPath).Trim() -ne $Version) {
  [IO.File]::WriteAllText($versionPath, "$Version`n", $utf8NoBom)
  [void]$changed.Add('VERSION')
}

# tauri.conf.json：只替换顶层 version，避免误伤其他同名字段。
$tauriRelative = 'hosts\desktop-tauri\tauri.conf.json'
$tauriText = [IO.File]::ReadAllText((Join-Path $RepoRoot $tauriRelative))
$tauriUpdated = [regex]::Replace($tauriText, '(?m)^(\s*"version"\s*:\s*")[^"]*(")', ('${1}' + $Version + '${2}'), 1)
if ($tauriUpdated -notmatch ('"version"\s*:\s*"' + [regex]::Escape($Version) + '"')) {
  throw "$tauriRelative 未找到可替换的顶层 version 字段"
}
Save-IfChanged $tauriRelative $tauriUpdated

# Cargo.toml：只替换 [workspace.package] 段内的 version。
$cargoRelative = 'Cargo.toml'
$cargoText = [IO.File]::ReadAllText((Join-Path $RepoRoot $cargoRelative))
$section = [regex]::Match($cargoText, '(?ms)^\[workspace\.package\][^\[]*')
if (-not $section.Success) {
  throw "$cargoRelative 缺少 [workspace.package] 段"
}
# 先确认字段存在，再替换：替换后无变化也可能只是值已经正确，不能当成缺字段。
if (-not [regex]::IsMatch($section.Value, '(?m)^\s*version\s*=\s*"[^"]*"')) {
  throw "$cargoRelative 的 [workspace.package] 段未找到 version 字段"
}
$updatedSection = [regex]::Replace($section.Value, '(?m)^(\s*version\s*=\s*")[^"]*(")', ('${1}' + $Version + '${2}'), 1)
$cargoUpdated = $cargoText.Substring(0, $section.Index) + $updatedSection + $cargoText.Substring($section.Index + $section.Length)
Save-IfChanged $cargoRelative $cargoUpdated

# Devin 扩展两代并存：V1（Go 版遗留）与 V2（桌面版）都随安装包分发，版本与产品保持一致。
foreach ($extensionRelative in @('plugin\devin-extension\package.json', 'plugin\devin-extension-v2\package.json')) {
  $extensionText = [IO.File]::ReadAllText((Join-Path $RepoRoot $extensionRelative))
  $extensionUpdated = [regex]::Replace($extensionText, '(?m)^(\s*"version"\s*:\s*")[^"]*(")', ('${1}' + $Version + '${2}'), 1)
  Save-IfChanged $extensionRelative $extensionUpdated
}

# 阶段 D 的其余分发物没有独立产品版本字段，无需在这里同步：
# 三个 Hook exe 的版本来自 Cargo.toml 的 [workspace.package] version（上面已同步），
# plugin\commandcode-v2\agent-notify.ts 只有协议版本 PROTOCOL_VERSION，没有产品版本字段。

if ($changed.Count -eq 0) {
  Write-Output "[version] 已是 $Version，无需改动"
} else {
  foreach ($item in $changed) { Write-Output "[version] 已更新 $item → $Version" }
}
Write-Output "[version] 提示：改过 Cargo.toml 后需重新生成 Cargo.lock（cargo metadata 或任意 cargo 命令即可）"
