#Requires -Version 5.1
<#
.SYNOPSIS
  PSScriptAnalyzer 全量扫描（本地与 CI 共用入口）。有 Error/Warning 非零退出。

.DESCRIPTION
  按仓库根目录 PSScriptAnalyzerSettings.psd1 扫描全部 ps1/psm1/psd1；
  本地缺 PSScriptAnalyzer 时自动装到 CurrentUser（CI 同样适用）。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\lint.ps1
#>
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent

if (-not (Get-Module PSScriptAnalyzer -ListAvailable)) {
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Install-Module PSScriptAnalyzer -Force -Scope CurrentUser -Confirm:$false
}
Import-Module PSScriptAnalyzer

$settings = Join-Path $RepoRoot 'PSScriptAnalyzerSettings.psd1'
$results = @(Invoke-ScriptAnalyzer -Path $RepoRoot -Recurse -Settings $settings -Severity Error, Warning |
    Where-Object { $_.ScriptPath -notmatch '[\\/]\.git[\\/]|[\\/]node_modules[\\/]|[\\/]dist[\\/]' })

if ($results.Count -gt 0) {
  foreach ($r in $results) {
    "{0}  {1}:{2}  {3}" -f $r.Severity, $r.ScriptName, $r.Line, $r.Message
  }
  Write-Output "[lint] 有 $($results.Count) 处 Error/Warning，请修复或按理由加入 PSScriptAnalyzerSettings.psd1 排除"
  exit 1
}
Write-Output '[lint] PSScriptAnalyzer 全绿'
exit 0
