#Requires -Version 5.1
<#
.SYNOPSIS
  测试入口（本地/CI 共用）：Pester 单测 + 冒烟测试，任一失败整体非零退出。

.DESCRIPTION
  缺 Pester 5+ 时自动装到 CurrentUser。冒烟测试以 powershell.exe（5.1）
  子进程方式运行，保证与真实运行环境一致。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1
#>
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent

$pester = Get-Module Pester -ListAvailable | Sort-Object Version -Descending | Select-Object -First 1
if (-not $pester -or $pester.Version.Major -lt 5) {
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Install-Module Pester -MinimumVersion 5.5.0 -Force -SkipPublisherCheck -Scope CurrentUser -Confirm:$false
}
Import-Module Pester

Push-Location $RepoRoot
try {
  $result = Invoke-Pester -Path (Join-Path $RepoRoot 'tests\unit') -Output Detailed -PassThru
  if ($result.FailedCount -gt 0) {
    throw "Pester 单测失败 $($result.FailedCount) 项"
  }
  Write-Output "[test] Pester 单测通过 $($result.PassedCount) 项"
} finally {
  Pop-Location
}

# 冒烟用 5.1 子进程（真实运行环境），失败即抛。
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\smoke.ps1')
if ($LASTEXITCODE -ne 0) {
  throw "冒烟测试失败 exit=$LASTEXITCODE"
}

Write-Output '[test] 单测 + 冒烟全绿'
exit 0
