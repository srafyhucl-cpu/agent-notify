#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin opencode 推送总开关：翻转 marker 文件有无（随用随开）。

.DESCRIPTION
  marker 存在 = 推送关（OFF），不存在 = 开（ON）。
  与 opencode 插件同路径约定，默认 marker：
  %USERPROFILE%\.config\opencode\notify-pushplus.off
  可用 -MarkerPath 覆盖（冒烟测试即用临时路径隔离）。
  注意：开关两边都管（opencode 插件与 codex wrapper 看同一 marker，
  codex 只跳推送，透传原电脑操控不受影响）。
  任何情况都 exit 0，不卡住调用方。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-toggle.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-toggle.ps1 -On
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-toggle.ps1 -Off
#>
param(
  [switch]$On,
  [switch]$Off,
  [string]$MarkerPath = (Join-Path $env:USERPROFILE '.config\opencode\notify-pushplus.off')
)

$ErrorActionPreference = 'SilentlyContinue'

function Set-MarkerOff {
  param([string]$Path)
  New-Item -ItemType Directory -Force -Path (Split-Path $Path -Parent) | Out-Null
  "off $((Get-Date).ToString('o'))" | Out-File -FilePath $Path -Encoding utf8 -Force
}

try {
  if ($Off) {
    # -On -Off 同时给时按 -Off 算（关比开安全）。
    Set-MarkerOff $MarkerPath
    Write-Output 'OFF'
  } elseif ($On) {
    Remove-Item $MarkerPath -Force -ErrorAction SilentlyContinue
    Write-Output 'ON'
  } elseif (Test-Path $MarkerPath) {
    Remove-Item $MarkerPath -Force -ErrorAction SilentlyContinue
    Write-Output 'ON'
  } else {
    Set-MarkerOff $MarkerPath
    Write-Output 'OFF'
  }
} catch {
  # 失败也按 marker 有无回显当前状态，保证调用方总有回显。
  if (Test-Path $MarkerPath) { Write-Output 'OFF' } else { Write-Output 'ON' }
}
exit 0
