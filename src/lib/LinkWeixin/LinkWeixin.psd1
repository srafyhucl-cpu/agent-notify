@{
  RootModule        = 'LinkWeixin.psm1'
  ModuleVersion     = '0.3.0'
  GUID              = '56a32fa0-7bf9-40a8-aa4d-05704807c513'
  Author            = 'linkWeixin contributors'
  Copyright         = '(c) linkWeixin contributors. MIT License.'
  Description       = 'linkWeixin 共享逻辑模块：多通道配置、路径解析、摘要渲染、多渠道推送、codex 事件解析、marker 读写、推送历史查询。'
  PowerShellVersion = '5.1'

  FunctionsToExport = @(
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
  CmdletsToExport   = @()
  VariablesToExport = @()
  AliasesToExport   = @()

  PrivateData = @{
    PSData = @{
      Tags = @('linkWeixin', 'PushPlus', 'notification', 'opencode', 'codex', 'wecom', 'feishu', 'dingtalk')
    }
  }
}
