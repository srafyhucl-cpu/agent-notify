function Get-LinkWeixinConfig {
  <#
  .SYNOPSIS
    读取 linkWeixin 用户配置（~/.config/linkweixin/config.json），支持与环境变量智能融合。
  #>
  [CmdletBinding()]
  param(
    [string]$ConfigPath = ''
  )

  if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $paths = Get-LinkWeixinPaths
    $ConfigPath = $paths.AppConfigFile
  }

  $defaultConfig = [ordered]@{
    channels = [ordered]@{
      pushplus = [ordered]@{
        enabled = $true
        token   = if ($env:PUSHPLUS_TOKEN) { $env:PUSHPLUS_TOKEN } else { '' }
      }
      wecom    = [ordered]@{
        enabled = [bool]$env:WECOM_WEBHOOK_URL
        webhook = if ($env:WECOM_WEBHOOK_URL) { $env:WECOM_WEBHOOK_URL } else { '' }
      }
      feishu   = [ordered]@{
        enabled = [bool]$env:FEISHU_WEBHOOK_URL
        webhook = if ($env:FEISHU_WEBHOOK_URL) { $env:FEISHU_WEBHOOK_URL } else { '' }
      }
      dingtalk = [ordered]@{
        enabled = [bool]$env:DINGTALK_WEBHOOK_URL
        webhook = if ($env:DINGTALK_WEBHOOK_URL) { $env:DINGTALK_WEBHOOK_URL } else { '' }
      }
      custom   = [ordered]@{
        enabled = [bool]$env:LINKWEIXIN_WEBHOOK_URL
        webhook = if ($env:LINKWEIXIN_WEBHOOK_URL) { $env:LINKWEIXIN_WEBHOOK_URL } else { '' }
      }
    }
    quietHours  = if ($env:OPENCODE_NOTIFY_QUIET) { $env:OPENCODE_NOTIFY_QUIET } else { '' }
    cooldownMin = if ($env:OPENCODE_NOTIFY_COOLDOWN_MIN) { [int]$env:OPENCODE_NOTIFY_COOLDOWN_MIN } else { 10 }
  }

  if (Test-Path $ConfigPath) {
    try {
      $raw = [IO.File]::ReadAllText($ConfigPath, [System.Text.Encoding]::UTF8)
      $cfg = ConvertFrom-Json $raw
      if ($cfg.channels) {
        if ($cfg.channels.pushplus) {
          if ($null -ne $cfg.channels.pushplus.enabled) { $defaultConfig.channels.pushplus.enabled = [bool]$cfg.channels.pushplus.enabled }
          if (-not [string]::IsNullOrWhiteSpace($cfg.channels.pushplus.token)) { $defaultConfig.channels.pushplus.token = [string]$cfg.channels.pushplus.token }
        }
        if ($cfg.channels.wecom) {
          if ($null -ne $cfg.channels.wecom.enabled) { $defaultConfig.channels.wecom.enabled = [bool]$cfg.channels.wecom.enabled }
          if (-not [string]::IsNullOrWhiteSpace($cfg.channels.wecom.webhook)) { $defaultConfig.channels.wecom.webhook = [string]$cfg.channels.wecom.webhook }
        }
        if ($cfg.channels.feishu) {
          if ($null -ne $cfg.channels.feishu.enabled) { $defaultConfig.channels.feishu.enabled = [bool]$cfg.channels.feishu.enabled }
          if (-not [string]::IsNullOrWhiteSpace($cfg.channels.feishu.webhook)) { $defaultConfig.channels.feishu.webhook = [string]$cfg.channels.feishu.webhook }
        }
        if ($cfg.channels.dingtalk) {
          if ($null -ne $cfg.channels.dingtalk.enabled) { $defaultConfig.channels.dingtalk.enabled = [bool]$cfg.channels.dingtalk.enabled }
          if (-not [string]::IsNullOrWhiteSpace($cfg.channels.dingtalk.webhook)) { $defaultConfig.channels.dingtalk.webhook = [string]$cfg.channels.dingtalk.webhook }
        }
        if ($cfg.channels.custom) {
          if ($null -ne $cfg.channels.custom.enabled) { $defaultConfig.channels.custom.enabled = [bool]$cfg.channels.custom.enabled }
          if (-not [string]::IsNullOrWhiteSpace($cfg.channels.custom.webhook)) { $defaultConfig.channels.custom.webhook = [string]$cfg.channels.custom.webhook }
        }
      }
      if ($null -ne $cfg.quietHours) { $defaultConfig.quietHours = [string]$cfg.quietHours }
      if ($null -ne $cfg.cooldownMin -and [int]$cfg.cooldownMin -gt 0) { $defaultConfig.cooldownMin = [int]$cfg.cooldownMin }
    } catch { }
  }

  return $defaultConfig
}
