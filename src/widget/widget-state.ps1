#Requires -Version 5.1
<#
  悬浮窗 · 状态轮询与刷新（由入口脚本 dot-source）。
#>

function Test-AppRunning {
  param([string[]]$patterns, [string[]]$exclude = @())
  try {
    $hits = Get-Process -Name $patterns -ErrorAction SilentlyContinue | Where-Object {
      $p = $_
      $skip = $false
      foreach ($x in $exclude) { if ($p.ProcessName -like $x) { $skip = $true } }
      -not $skip
    }
    return [bool]$hits
  } catch { }
  return $false
}

function Test-PluginGate {
  param([hashtable]$ctx)
  try {
    $pluginPath = $ctx.Paths.PluginFile
    if (-not (Test-Path $pluginPath)) { return '未安装' }
    if (Select-String -Path $pluginPath -Pattern 'markerOff' -SimpleMatch -Quiet) { return '新版' }
    return '旧版'
  } catch { return '未知' }
}

function Test-WatchTask {
  try {
    $xml = schtasks /query /tn CodexNotifyWatch /xml 2>$null | Out-String
    if ([string]::IsNullOrWhiteSpace($xml)) { return '新版' }
    if ($xml -match 'run-hidden\.vbs' -or $xml -match 'wscript' -or $xml -match 'WindowStyle\s+Hidden') { return '新版' }
    return '旧版'
  } catch { return '未知' }
}

function Test-QuietNow {
  try {
    $cfg = Get-LinkWeixinConfig
    $raw = if ($cfg.quietHours) { $cfg.quietHours } else { $env:OPENCODE_NOTIFY_QUIET }
    $m = [regex]::Match($raw, '^\s*(\d{1,2})\s*-\s*(\d{1,2})\s*$')
    if (-not $m.Success) { return $false }
    $s = [int]$m.Groups[1].Value
    $e = [int]$m.Groups[2].Value
    if ($s -lt 0 -or $s -gt 23 -or $e -lt 0 -or $e -gt 23 -or $s -eq $e) { return $false }
    $h = (Get-Date).Hour
    if ($s -lt $e) { return ($h -ge $s -and $h -lt $e) }
    return ($h -ge $s -or $h -lt $e)
  } catch { return $false }
}

function Get-TodayPushCount {
  param([hashtable]$ctx)
  try {
    $log = $ctx.Paths.PushLog
    if (-not (Test-Path $log)) { return 0 }
    $today = (Get-Date).Date
    $count = 0
    foreach ($line in Get-Content $log -Encoding UTF8 -ErrorAction Stop) {
      $m = [regex]::Match($line, '^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?)')
      if (-not $m.Success) { continue }
      try {
        $dt = [datetime]::Parse($m.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind).ToLocalTime()
        if ($dt.Date -eq $today) { $count++ }
      } catch { }
    }
    return $count
  } catch { return 0 }
}

function Get-LastPushText {
  param([hashtable]$ctx)
  try {
    $pushLog = $ctx.Paths.PushLog
    if (-not (Test-Path $pushLog)) { return '暂无推送' }
    $tail = Get-Content $pushLog -Tail 1 -Encoding UTF8 -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($tail)) { return '暂无推送' }
    $m = [regex]::Match($tail, '(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?)')
    if ($m.Success) {
      try {
        $dt = [datetime]::Parse($m.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind).ToLocalTime()
        $now = Get-Date
        $delta = $now - $dt
        if ($delta.TotalSeconds -lt 60) { return '刚刚' }
        if ($delta.TotalMinutes -lt 60) { return "$([int]$delta.TotalMinutes) 分钟前" }
        if ($dt.Date -eq $now.Date) { return $dt.ToString('HH:mm') }
        if ($dt.Date -eq $now.Date.AddDays(-1)) { return '昨天 ' + $dt.ToString('HH:mm') }
        return $dt.ToString('MM-dd HH:mm')
      } catch { }
    }
    $t = $tail.Trim()
    if ($t.Length -gt 16) { return $t.Substring(0, 16) + '…' }
    return $t
  } catch { return '暂无推送' }
}

function Update-WidgetState {
  param([hashtable]$ctx, [switch]$Light)
  $onOc = -not (Test-NotifyMarker -Path $ctx.MarkerPath)
  $onCx = -not (Test-NotifyMarker -Path $ctx.CodexMarker)
  $onAg = -not (Test-NotifyMarker -Path $ctx.AntigravityMarker)
  $ctx.Tick = [int]$ctx.Tick + 1

  $ctx.BtnOc.Text = if ($onOc) { "OpenCode`n● 监听中" } else { "OpenCode`n○ 已暂停" }
  if (-not $ctx.HoverOc) { $ctx.BtnOc.BackColor = if ($onOc) { $ctx.Colors.GREEN } else { $ctx.Colors.RED } }
  $ctx.BtnOc.ForeColor = [System.Drawing.Color]::White

  $ctx.BtnCx.Text = if ($onCx) { "Codex`n● 监听中" } else { "Codex`n○ 已暂停" }
  if (-not $ctx.HoverCx) { $ctx.BtnCx.BackColor = if ($onCx) { $ctx.Colors.GREEN } else { $ctx.Colors.RED } }
  $ctx.BtnCx.ForeColor = [System.Drawing.Color]::White

  $ctx.BtnAg.Text = if ($onAg) { "Antigravity`n● 监听中" } else { "Antigravity`n○ 已暂停" }
  if (-not $ctx.HoverAg) { $ctx.BtnAg.BackColor = if ($onAg) { $ctx.Colors.GREEN } else { $ctx.Colors.RED } }
  $ctx.BtnAg.ForeColor = [System.Drawing.Color]::White

  $onCount = [int]$onOc + [int]$onCx + [int]$onAg
  $state = if ($onCount -eq 3) { 2 } elseif ($onCount -eq 0) { 0 } else { 1 }
  $ctx.Strip.BackColor = if ($state -eq 2) { $ctx.Colors.GREEN } elseif ($state -eq 0) { $ctx.Colors.RED } else { [System.Drawing.Color]::FromArgb(200, 130, 30) }
  if ($ctx.LastOnState -ne $state) {
    $ctx.LastOnState = $state
    $ctx.Notify.Icon = if ($state -eq 2) { $ctx.IconOn } elseif ($state -eq 0) { $ctx.IconOff } else { $ctx.IconMid }
    $ctx.Form.Icon = $ctx.Notify.Icon
  }

  $oc = Test-AppRunning @('OpenCode*', 'opencode*')
  $cx = Test-AppRunning @('codex*') @('codex-plus-plus*')
  $ag = Test-AppRunning @('Antigravity*', 'language_server*')

  $ctx.RowOc.Txt.Text = if ($oc) { 'OpenCode  运行中' } else { 'OpenCode  未运行' }
  $ctx.RowOc.Dot.ForeColor = if ($oc) { $ctx.Colors.DotOn } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }

  $ctx.RowCx.Txt.Text = if ($cx) { 'Codex  运行中' } else { 'Codex  未运行' }
  $ctx.RowCx.Dot.ForeColor = if ($cx) { $ctx.Colors.DotOn } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }

  $ctx.RowAg.Txt.Text = if ($ag) { 'Antigravity  运行中' } else { 'Antigravity  未运行' }
  $ctx.RowAg.Dot.ForeColor = if ($ag) { $ctx.Colors.DotOn } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }

  $ctx.RowLast.Txt.Text = '上次推送  ' + (Get-LastPushText -Ctx $ctx)

  # 首屏秒开：Light 模式跳过重型检测（插件文件扫描与 schtasks 外部命令），
  # 仅用默认就绪态绘制，留给窗口呈现后的首个异步 Tick 做深度诊断。
  if (-not $Light) {
    if ($null -eq $ctx.PlugVer -or ($ctx.Tick % 120) -eq 1) {
      $ctx.PlugVer = Test-PluginGate -Ctx $ctx
      $ctx.TodayCount = Get-TodayPushCount -Ctx $ctx
    }
    if ($null -eq $ctx.TaskVer -or ($ctx.Tick % 120) -eq 1) { $ctx.TaskVer = Test-WatchTask }
  }
  $pv = $ctx.PlugVer
  if ($Light -and $null -eq $pv) { $pv = '新版' }
  $taskVerShown = $ctx.TaskVer
  if ($Light -and $null -eq $taskVerShown) { $taskVerShown = '新版' }

  if ($pv -eq '新版') {
    $quiet = Test-QuietNow
    $bits = @()
    if ($quiet) { $bits += '时段静默中' }
    if ($null -ne $ctx.TodayCount) { $bits += "今日已推 $($ctx.TodayCount) 条" }
    $ctx.Hint.Text = if ($bits.Count -gt 0) { '三开关独立各管一边 · ' + ($bits -join ' · ') } else { '三开关独立各管一边 · 推送通道已就绪' }
    $ctx.Hint.ForeColor = if ($quiet) { [System.Drawing.Color]::FromArgb(200, 170, 90) } else { $ctx.Colors.DIM }
    $ctx.Hint.Cursor = [System.Windows.Forms.Cursors]::Default
  } else {
    $ctx.Hint.Text = "⚠ 插件$pv：开关不生效，请重跑 install 后重启桌面。"
    $ctx.Hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
    $ctx.Hint.Cursor = [System.Windows.Forms.Cursors]::Default
  }

  if ($pv -eq '新版' -and $taskVerShown -eq '旧版') {
    $ctx.Hint.Text = '⚡ 检测到旧版看守任务 (每5分钟闪屏)：点击修复'
    $ctx.Hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
    $ctx.Hint.Cursor = [System.Windows.Forms.Cursors]::Hand
  }
}
