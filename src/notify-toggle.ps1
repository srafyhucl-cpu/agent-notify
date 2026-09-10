#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 推送开关：翻转 marker 文件有无（随用随开，两边独立）。

.DESCRIPTION
  每个 agent 各一个 marker，各管一边：
  - opencode：%USERPROFILE%\.config\opencode\notify-pushplus.off
    （可用 OPENCODE_NOTIFY_MARKER_FILE 覆盖）
  - codex：%USERPROFILE%\.config\opencode\codex-notify.off
    （可用 CODEX_NOTIFY_MARKER_FILE 覆盖）
  文件存在 = 该边关（OFF），不存在 = 开（ON）。
  -Agent Opencode/Codex 只动一边；默认 All，两边各翻各的并各回显一行。
  -On/-Off 显式指定（同时给按 -Off 算）。-MarkerPath 是 -OpencodeMarker
  的别名（兼容老用法/测试）。codex 只跳推送，透传原电脑操控不受影响。
  任何情况都 exit 0，不卡住调用方。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-toggle.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-toggle.ps1 -Agent Opencode -Off
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-toggle.ps1 -Agent Codex -On
#>
param(
  [switch]$On,
  [switch]$Off,
  [ValidateSet('All', 'Opencode', 'Codex')][string]$Agent = 'All',
  [Alias('MarkerPath')][string]$OpencodeMarker,
  [string]$CodexMarker
)

$ErrorActionPreference = 'SilentlyContinue'

try {
  Import-Module (Join-Path $PSScriptRoot 'lib\LinkWeixin\LinkWeixin.psd1') -ErrorAction Stop
} catch {
  [Console]::Error.WriteLine('[notify-toggle] LinkWeixin 模块加载失败：' + $_.Exception.Message)
  exit 0
}

$paths = Get-LinkWeixinPaths
if (-not $PSBoundParameters.ContainsKey('OpencodeMarker')) { $OpencodeMarker = $paths.OpenCodeMarker }
if (-not $PSBoundParameters.ContainsKey('CodexMarker')) { $CodexMarker = $paths.CodexMarker }

try {
  $mode = if ($Off) { 'Off' } elseif ($On) { 'On' } else { 'Flip' }
  if ($Agent -eq 'All') {
    try { $a = Set-NotifyMarker -Path $OpencodeMarker -Mode $mode } catch { $a = if (Test-Path $OpencodeMarker) { 'OFF' } else { 'ON' } }
    try { $b = Set-NotifyMarker -Path $CodexMarker -Mode $mode } catch { $b = if (Test-Path $CodexMarker) { 'OFF' } else { 'ON' } }
    Write-Output "opencode: $a"
    Write-Output "codex: $b"
  } elseif ($Agent -eq 'Opencode') {
    Write-Output (Set-NotifyMarker -Path $OpencodeMarker -Mode $mode)
  } else {
    Write-Output (Set-NotifyMarker -Path $CodexMarker -Mode $mode)
  }
} catch {
  # 失败也按 marker 有无回显，保证调用方总有回显。
  Write-Output 'ON'
}
exit 0
