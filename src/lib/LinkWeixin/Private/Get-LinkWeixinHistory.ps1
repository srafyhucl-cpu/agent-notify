function Get-LinkWeixinHistory {
  <#
  .SYNOPSIS
    读取推送日志，按时间倒序返回最近的历史推送记录。
  #>
  [CmdletBinding()]
  param(
    [int]$Limit = 50,
    [string]$LogPath = ''
  )

  if ([string]::IsNullOrWhiteSpace($LogPath)) {
    $paths = Get-LinkWeixinPaths
    $LogPath = $paths.PushLog
  }

  if (-not (Test-Path $LogPath)) {
    return @()
  }

  $lines = @(Get-Content -Path $LogPath -Encoding UTF8 -ErrorAction SilentlyContinue)
  if ($lines.Count -eq 0) { return @() }

  $results = New-Object System.Collections.ArrayList
  for ($i = $lines.Count - 1; $i -ge 0; $i--) {
    $line = $lines[$i]
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $m = [regex]::Match($line, '^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?)\s+(.*)$')
    if (-not $m.Success) { continue }

    $timeStr = $m.Groups[1].Value
    $rest = $m.Groups[2].Value
    $localTime = $timeStr
    try {
      $dt = [datetime]::Parse($timeStr, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind).ToLocalTime()
      $localTime = $dt.ToString('yyyy-MM-dd HH:mm:ss')
    } catch { }

    $title = ''
    $summary = ''
    $channel = 'PushPlus'
    $status = '成功'

    if ($rest -match 'title=([^|]+)') {
      $title = $Matches[1].Trim()
    }
    if ($rest -match 'summary=([^|]+)') {
      $summary = $Matches[1].Trim()
    }
    if ($rest -match 'channels=([^|]+)') {
      $channel = $Matches[1].Trim()
    }
    if ($rest -match 'status=([^|]+)') {
      $status = $Matches[1].Trim()
    }

    if ([string]::IsNullOrWhiteSpace($title)) {
      $title = $rest.Trim()
    }

    [void]$results.Add([PSCustomObject]@{
      Time      = $localTime
      RawTime   = $timeStr
      Title     = $title
      Summary   = $summary
      Channels  = $channel
      Status    = $status
      Raw       = $line
    })

    if ($results.Count -ge $Limit) { break }
  }

  return @($results)
}
