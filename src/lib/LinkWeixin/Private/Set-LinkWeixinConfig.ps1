function Set-LinkWeixinConfig {
  <#
  .SYNOPSIS
    持久化 linkWeixin 用户配置到 config.json。
  #>
  [CmdletBinding()]
  param(
    [Parameter(Mandatory = $true)]
    [hashtable]$Config,
    [string]$ConfigPath = ''
  )

  if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $paths = Get-LinkWeixinPaths
    $ConfigPath = $paths.AppConfigFile
  }

  $parentDir = Split-Path $ConfigPath -Parent
  if (-not (Test-Path $parentDir)) {
    New-Item -ItemType Directory -Force -Path $parentDir | Out-Null
  }

  $json = $Config | ConvertTo-Json -Depth 6
  $utf8Encoding = New-Object System.Text.UTF8Encoding($false)
  [IO.File]::WriteAllText($ConfigPath, $json, $utf8Encoding)
  return $ConfigPath
}
