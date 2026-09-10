@{
  RootModule        = 'LinkWeixin.psm1'
  ModuleVersion     = '0.1.0'
  GUID              = '56a32fa0-7bf9-40a8-aa4d-05704807c513'
  Author            = 'linkWeixin contributors'
  Copyright         = '(c) linkWeixin contributors. MIT License.'
  Description       = 'linkWeixin 共享逻辑模块：路径解析、摘要渲染、PushPlus 推送、codex 事件解析、marker 读写。'
  PowerShellVersion = '5.1'

  FunctionsToExport = @(
    'Get-LinkWeixinPaths',
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
      Tags = @('linkWeixin', 'PushPlus', 'notification', 'opencode', 'codex')
    }
  }
}
