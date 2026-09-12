#Requires -Version 5.1
<#
.SYNOPSIS
  测试入口（本地/CI 共用）：Go 单测 + 插件类型检查 + PowerShell 冒烟测试，任一失败整体非零退出。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1 -SkipTypeScript
#>
param(
  [switch]$SkipTypeScript
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent

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

$goExe = Resolve-GoCommand
if (-not $goExe) { throw '找不到 go.exe，无法运行 Go 测试' }

$driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
$cacheRoot = Join-Path $driveRoot 'Temp\agent-notify-go'
if ([string]::IsNullOrWhiteSpace($env:GOPATH)) { $env:GOPATH = $cacheRoot }
if ([string]::IsNullOrWhiteSpace($env:GOMODCACHE)) { $env:GOMODCACHE = Join-Path $cacheRoot 'pkg\mod' }
if ([string]::IsNullOrWhiteSpace($env:GOCACHE)) { $env:GOCACHE = Join-Path $cacheRoot 'build' }

Push-Location $RepoRoot
try {
  & $goExe test ./...
  if ($LASTEXITCODE -ne 0) { throw "Go 单测失败 exit=$LASTEXITCODE" }
  & $goExe vet ./...
  if ($LASTEXITCODE -ne 0) { throw "Go vet 失败 exit=$LASTEXITCODE" }
  Write-Output '[test] Go 单测与 vet 通过'
} finally {
  Pop-Location
}

if (-not $SkipTypeScript) {
  $tsc = Join-Path $RepoRoot 'node_modules\.bin\tsc.cmd'
  if (-not (Test-Path $tsc)) {
    throw '缺少 node_modules 里的 tsc，请先在仓库根目录执行 npm ci'
  }
  Push-Location $RepoRoot
  try {
    & $tsc --noEmit
    if ($LASTEXITCODE -ne 0) { throw "插件类型检查失败 exit=$LASTEXITCODE" }
    Write-Output '[test] 插件类型检查通过'
  } finally {
    Pop-Location
  }
}

& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\smoke.ps1')
if ($LASTEXITCODE -ne 0) {
  throw "冒烟测试失败 exit=$LASTEXITCODE"
}

Write-Output '[test] 单测 + 类型检查 + 冒烟全绿'
exit 0
