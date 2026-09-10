function Set-NotifyMarker {
  <#
  .SYNOPSIS
    marker 读写：文件存在 = 该边推送关（OFF），不存在 = 开（ON）。
    -Mode On / Off / Flip（默认 Flip：有则删、无则建），返回 'ON' / 'OFF'。
  #>
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [ValidateSet('On', 'Off', 'Flip')][string]$Mode = 'Flip'
  )
  $isOff = Test-Path $Path
  $turnOn = if ($Mode -eq 'On') { $true } elseif ($Mode -eq 'Off') { $false } else { $isOff }
  if ($turnOn) {
    Remove-Item $Path -Force -ErrorAction SilentlyContinue
    return 'ON'
  } else {
    New-Item -ItemType Directory -Force -Path (Split-Path $Path -Parent) | Out-Null
    "off $((Get-Date).ToString('o'))" | Out-File -FilePath $Path -Encoding utf8 -Force
    return 'OFF'
  }
}

function Test-NotifyMarker {
  <#
  .SYNOPSIS
    返回 $true 表示 marker 存在（该边推送为关）。
  #>
  param([Parameter(Mandatory = $true)][string]$Path)
  return (Test-Path $Path)
}
