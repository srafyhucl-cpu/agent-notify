# 悬浮窗 · 状态轮询与刷新（由入口脚本 dot-source）。
# 运行灯（5 秒轮询）、上次推送时间、插件/看守任务版本检查、色条与托盘图标同步。

function Test-AppRunning {
  param([string[]]$Patterns, [string[]]$Exclude = @())
  try {
    $hits = Get-Process -Name $Patterns -ErrorAction SilentlyContinue | Where-Object {
      $p = $_
      $skip = $false
      foreach ($x in $Exclude) { if ($p.ProcessName -like $x) { $skip = $true } }
      -not $skip
    }
    return [bool]$hits
  } catch { }
  return $false
}

function Test-PluginGate {
  # 装上去的插件是不是带三道闸的新版：旧版不认 marker，关了也照推，
  # 悬浮窗直接提示，避免静默失效。
  param([hashtable]$Ctx)
  try {
    $pluginPath = $Ctx.Paths.PluginFile
    if (-not (Test-Path $pluginPath)) { return '未安装' }
    if (Select-String -Path $pluginPath -Pattern 'markerOff' -SimpleMatch -Quiet) { return '新版' }
    return '旧版'
  } catch { return '未知' }
}

function Test-WatchTask {
  # 看守任务是不是隐藏版：旧版 install 注册的动作不带 -WindowStyle Hidden，
  # 每 5 分钟闪一次窗口。读任务定义不需要管理员权限，查出来就提示管理员重跑。
  try {
    $xml = schtasks /query /tn CodexNotifyWatch /xml 2>$null | Out-String
    if ([string]::IsNullOrWhiteSpace($xml)) { return '新版' } # 没装看守就不报警
    if ($xml -match 'WindowStyle\s+Hidden') { return '新版' }
    return '旧版'
  } catch { return '未知' }
}

function Get-LastPushText {
  param([hashtable]$Ctx)
  try {
    $pushLog = $Ctx.Paths.PushLog
    if (-not (Test-Path $pushLog)) { return '暂无推送' }
    # 日志是无 BOM UTF-8（插件写入）：PS 5.1 必须显式 -Encoding UTF8，否则中文乱码
    $tail = Get-Content $pushLog -Tail 1 -Encoding UTF8 -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($tail)) { return '暂无推送' }
    $m = [regex]::Match($tail, '(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})')
    if ($m.Success) { return "$($m.Groups[1].Value) $($m.Groups[2].Value)" }
    $t = $tail.Trim()
    if ($t.Length -gt 24) { return $t.Substring(0, 24) + '…' }
    return $t
  } catch { return '暂无推送' }
}

function Update-WidgetState {
  param([hashtable]$Ctx)
  $onOc = -not (Test-NotifyMarker -Path $Ctx.MarkerPath)
  $onCx = -not (Test-NotifyMarker -Path $Ctx.CodexMarker)
  $Ctx.Tick = [int]$Ctx.Tick + 1
  $Ctx.BtnOc.Text = if ($onOc) { "opencode`n● ON" } else { "opencode`n○ OFF" }
  $Ctx.BtnOc.BackColor = if ($onOc) { $Ctx.Colors.GREEN } else { $Ctx.Colors.RED }
  $Ctx.BtnOc.ForeColor = [System.Drawing.Color]::White
  $Ctx.BtnCx.Text = if ($onCx) { "codex`n● ON" } else { "codex`n○ OFF" }
  $Ctx.BtnCx.BackColor = if ($onCx) { $Ctx.Colors.GREEN } else { $Ctx.Colors.RED }
  $Ctx.BtnCx.ForeColor = [System.Drawing.Color]::White
  # 色条/托盘：两边都开绿，都关红，一开一关橙
  $state = if ($onOc -and $onCx) { 2 } elseif (-not $onOc -and -not $onCx) { 0 } else { 1 }
  $Ctx.Strip.BackColor = if ($state -eq 2) { $Ctx.Colors.GREEN } elseif ($state -eq 0) { $Ctx.Colors.RED } else { [System.Drawing.Color]::FromArgb(200, 130, 30) }
  if ($Ctx.LastOnState -ne $state) {
    $Ctx.LastOnState = $state
    $Ctx.Notify.Icon = if ($state -eq 2) { $Ctx.IconOn } elseif ($state -eq 0) { $Ctx.IconOff } else { $Ctx.IconMid }
    $Ctx.Form.Icon = $Ctx.Notify.Icon
  }
  $oc = Test-AppRunning @('OpenCode*', 'opencode*')
  # codex-plus-plus* 是无关常驻进程（Codex++，另一个软件），必须排除，
  # 否则关掉 Codex 桌面灯也不会灭。
  $cx = Test-AppRunning @('codex*') @('codex-plus-plus*')
  $Ctx.RowOc.Txt.Text = if ($oc) { 'opencode  运行中' } else { 'opencode  未运行' }
  $Ctx.RowOc.Dot.ForeColor = if ($oc) { $Ctx.Colors.DotOn } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }
  $Ctx.RowCx.Txt.Text = if ($cx) { 'codex  运行中' } else { 'codex  未运行' }
  $Ctx.RowCx.Dot.ForeColor = if ($cx) { $Ctx.Colors.DotOn } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }
  $Ctx.RowLast.Txt.Text = '上次推送 ' + (Get-LastPushText -Ctx $Ctx)
  # 插件/任务版本几乎不变：启动查一次，之后每 10 分钟复查，不再每轮读文件。
  if ($null -eq $Ctx.PlugVer -or ($Ctx.Tick % 120) -eq 1) { $Ctx.PlugVer = Test-PluginGate -Ctx $Ctx }
  if ($null -eq $Ctx.TaskVer -or ($Ctx.Tick % 120) -eq 1) { $Ctx.TaskVer = Test-WatchTask }
  $pv = $Ctx.PlugVer
  if ($pv -eq '新版') {
    $Ctx.Hint.Text = '两个开关独立，各管一边。'
    $Ctx.Hint.ForeColor = $Ctx.Colors.DIM
  } else {
    $Ctx.Hint.Text = "⚠ 插件$pv：开关不生效，重跑 install 后重启桌面。"
    $Ctx.Hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
  }
  # 任务旧版另起提示（别覆盖插件报警，插件问题更严重）。
  if ($pv -eq '新版' -and $Ctx.TaskVer -eq '旧版') {
    $Ctx.Hint.Text = '⚠ 看守任务旧版：每5分钟闪窗口，管理员重跑 install.ps1。'
    $Ctx.Hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
  }
}
