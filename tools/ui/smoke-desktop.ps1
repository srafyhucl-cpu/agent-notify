#Requires -Version 5.1
<#
.SYNOPSIS
  执行 Rust 桌面预览包 smoke。

.DESCRIPTION
  默认只做可重复的安装器结构检查；传入 -Execute 后会在 D:\Temp 隔离安装并启动真实宿主。
#>
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [switch]$Execute,
  [switch]$KeepInstall
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$arguments = @(
  '-NoProfile',
  '-ExecutionPolicy', 'Bypass',
  '-File', (Join-Path $RepoRoot 'tests\desktop-installer-smoke.ps1'),
  '-Installer', $Installer
)
if ($Execute) { $arguments += '-Execute' }
if ($KeepInstall) { $arguments += '-KeepInstall' }

& powershell.exe @arguments
exit $LASTEXITCODE
