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

$driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
$cacheRoot = Join-Path $driveRoot 'Temp\agent-notify-go'
$testTempRoot = Join-Path $driveRoot 'Temp\agent-notify-test'
$previousTemp = $env:TEMP
$previousTmp = $env:TMP
$goTempRoot = Join-Path $cacheRoot 'tmp'
New-Item -ItemType Directory -Force -Path $goTempRoot | Out-Null
New-Item -ItemType Directory -Force -Path $testTempRoot | Out-Null
$env:TEMP = $testTempRoot
$env:TMP = $testTempRoot

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

try {
$goExe = Resolve-GoCommand
if (-not $goExe) { throw '找不到 go.exe，无法运行 Go 测试' }

$env:GOPATH = $cacheRoot
$env:GOMODCACHE = Join-Path $cacheRoot 'pkg\mod'
$env:GOCACHE = Join-Path $cacheRoot 'build'
$env:GOTMPDIR = $goTempRoot

Push-Location $RepoRoot
try {
  & $goExe test ./...
  if ($LASTEXITCODE -ne 0) { throw "Go 单测失败 exit=$LASTEXITCODE" }
  & $goExe vet ./...
  if ($LASTEXITCODE -ne 0) { throw "Go vet 失败 exit=$LASTEXITCODE" }
  $gofmt = Join-Path (Split-Path $goExe -Parent) 'gofmt.exe'
  if (-not (Test-Path -LiteralPath $gofmt)) {
    $gofmtCommand = Get-Command gofmt.exe -ErrorAction SilentlyContinue
    if (-not $gofmtCommand) { throw '找不到 gofmt.exe，无法检查 Go 格式' }
    $gofmt = $gofmtCommand.Source
  }
  $goFiles = @(Get-ChildItem -Path $RepoRoot -Recurse -File -Filter *.go |
      Where-Object { $_.FullName -notmatch '[\\/](\.git|node_modules|dist|bin)[\\/]' } |
      Select-Object -ExpandProperty FullName)
  $unformatted = @(& $gofmt -l $goFiles)
  if ($LASTEXITCODE -ne 0) { throw "gofmt 检查失败 exit=$LASTEXITCODE" }
  if ($unformatted.Count -gt 0) { throw "以下 Go 文件未格式化：$($unformatted -join ', ')" }
  Write-Output '[test] Go 单测、vet 与格式检查通过'
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
    $node = Get-Command node.exe -ErrorAction SilentlyContinue
    if (-not $node) { throw '找不到 node.exe，无法运行插件测试' }
    & $node.Source --test (Join-Path $RepoRoot 'tests\plugin-reply.test.cjs')
    if ($LASTEXITCODE -ne 0) { throw "OpenCode 插件测试失败 exit=$LASTEXITCODE" }
    & $node.Source --test (Join-Path $RepoRoot 'tests\devin-extension.test.cjs')
    if ($LASTEXITCODE -ne 0) { throw "Devin 扩展测试失败 exit=$LASTEXITCODE" }
    Write-Output '[test] 插件状态机测试通过'
  } finally {
    Pop-Location
  }
}

& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\signature-gate.tests.ps1')
if ($LASTEXITCODE -ne 0) {
  throw "签名门禁回归测试失败 exit=$LASTEXITCODE"
}

& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\smoke.ps1')
if ($LASTEXITCODE -ne 0) {
  throw "冒烟测试失败 exit=$LASTEXITCODE"
}

Write-Output '[test] Go 单测 + 插件类型/状态机 + 冒烟全绿'
} finally {
  $env:TEMP = $previousTemp
  $env:TMP = $previousTmp
  if (Test-Path -LiteralPath $testTempRoot) {
    $knownEmptyDir = Join-Path $testTempRoot 'agent-notify'
    if (Test-Path -LiteralPath $knownEmptyDir) {
      try {
        [IO.Directory]::Delete($knownEmptyDir, $false)
      } catch {
        Write-Warning "测试临时子目录仍有残留，请检查：$knownEmptyDir"
      }
    }
    try {
      [IO.Directory]::Delete($testTempRoot, $false)
    } catch {
      Write-Warning "测试临时目录仍有残留，请检查：$testTempRoot"
    }
  }
}

exit 0
