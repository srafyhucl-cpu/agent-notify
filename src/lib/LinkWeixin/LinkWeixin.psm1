#Requires -Version 5.1
<#
  linkWeixin 共享逻辑模块（加载器）。

  按文件名顺序 dot-source Private\*.ps1，再显式导出函数（与 psd1 的
  FunctionsToExport 保持一致）。所有函数都在调用时才读环境变量，
  便于测试覆写与运行时切换；不要在模块加载期求值。
#>
$ErrorActionPreference = 'Stop'

foreach ($f in @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'Private') -Filter '*.ps1' -File -ErrorAction Stop | Sort-Object Name)) {
  . $f.FullName
}

Export-ModuleMember -Function @(
  'Get-LinkWeixinPaths',
  'Get-LinkWeixinConfig',
  'Set-LinkWeixinConfig',
  'Get-LinkWeixinHistory',
  'Format-NotifySummary',
  'Send-PushPlusNotification',
  'ConvertFrom-CodexNotifyEventArgs',
  'Get-CodexComputerUseExe',
  'Set-NotifyMarker',
  'Test-NotifyMarker'
)
