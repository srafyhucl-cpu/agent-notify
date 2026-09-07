#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 悬浮窗：大开关 + opencode/codex 运行灯 + 上次推送时间 + 托盘常驻。

.DESCRIPTION
  常驻启动（控制台藏掉，只留窗体）：
    powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File linkweixin-widget.ps1
  install.ps1 会建 shell:startup 开机快捷方式 + 桌面快捷方式（都无需管理员）。
  无边框窗体，拖标题区移动。右上角 × / — 是最小化到托盘（首次有气泡提示），
  双击托盘图标恢复，右键托盘菜单可开关推送或彻底退出。
  再打开方式：双击托盘图标 / 桌面“linkWeixin 悬浮窗” / 上面那条手动命令。
  内容只有状态显示 + 翻 marker，不做 token/时段输入框。
  进程名已在本机实测：opencode 侧 'OpenCode*'（桌面）/'opencode*'（cli/service），
  codex 侧 'codex*'（codex-plus-plus* 是无关软件 Codex++，已排除）。
  上次推送时间读 notify-push.log 尾行（与插件同路径约定）。
#>
param(
  [string]$MarkerPath = (Join-Path $env:USERPROFILE '.config\opencode\notify-pushplus.off')
)

$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# 单实例：桌面快捷方式双击 = 可靠重开。先接管（杀掉）旧实例再起新窗体；
# 开关状态在 marker 文件里，新实例自动继承。排除自己和父进程，
# 父进程的命令行里也可能带本脚本名（比如从终端手动启动时），不能误杀。
try {
  $myParent = (Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction SilentlyContinue).ParentProcessId
  Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" -ErrorAction Stop |
    Where-Object {
      ($_.CommandLine -like '*linkweixin-widget.ps1*') -and
      ($_.ProcessId -ne $PID) -and ($_.ProcessId -ne $myParent)
    } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 500
} catch { }

$pushLog = if ($env:OPENCODE_NOTIFY_LOG_FILE) { $env:OPENCODE_NOTIFY_LOG_FILE } else { Join-Path $env:TEMP 'opencode\notify-push.log' }
$pluginPath = Join-Path $env:USERPROFILE '.config\opencode\plugin\notify-pushplus.ts'

$script:allowExit = $false
$script:lastOn = $null
$script:trayTipped = $false

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

function Get-NotifyOn {
  return -not (Test-Path $MarkerPath)
}

function Set-NotifyOn {
  param([bool]$TurnOn)
  if ($TurnOn) {
    Remove-Item $MarkerPath -Force -ErrorAction SilentlyContinue
  } else {
    New-Item -ItemType Directory -Force -Path (Split-Path $MarkerPath -Parent) | Out-Null
    "off $((Get-Date).ToString('o'))" | Out-File -FilePath $MarkerPath -Encoding utf8 -Force
  }
}

# 装上去的插件是不是带三道闸的新版：旧版不认 marker，关了也照推，
# 悬浮窗直接提示，避免静默失效。
function Test-PluginGate {
  try {
    if (-not (Test-Path $pluginPath)) { return '未安装' }
    if (Select-String -Path $pluginPath -Pattern 'markerOff' -SimpleMatch -Quiet) { return '新版' }
    return '旧版'
  } catch { return '未知' }
}

function Get-LastPushText {
  try {
    if (-not (Test-Path $pushLog)) { return '暂无推送' }
    $tail = Get-Content $pushLog -Tail 1 -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($tail)) { return '暂无推送' }
    $m = [regex]::Match($tail, '(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})')
    if ($m.Success) { return "$($m.Groups[1].Value) $($m.Groups[2].Value)" }
    $t = $tail.Trim()
    if ($t.Length -gt 24) { return $t.Substring(0, 24) + '…' }
    return $t
  } catch { return '暂无推送' }
}

function New-DotIcon {
  param([System.Drawing.Color]$Color)
  $bmp = New-Object System.Drawing.Bitmap(16, 16)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.Clear([System.Drawing.Color]::Transparent)
  $b = New-Object System.Drawing.SolidBrush($Color)
  $g.FillEllipse($b, 1, 1, 14, 14)
  $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, 1)
  $g.DrawEllipse($pen, 1, 1, 14, 14)
  $b.Dispose(); $pen.Dispose(); $g.Dispose()
  $ico = [System.Drawing.Icon]::FromHandle($bmp.GetHicon())
  $bmp.Dispose()
  return $ico
}

$BG = [System.Drawing.Color]::FromArgb(31, 31, 35)
$CARD = [System.Drawing.Color]::FromArgb(42, 42, 47)
$FG = [System.Drawing.Color]::FromArgb(240, 240, 240)
$DIM = [System.Drawing.Color]::FromArgb(150, 150, 155)
$GREEN = [System.Drawing.Color]::FromArgb(46, 160, 67)
$RED = [System.Drawing.Color]::FromArgb(200, 60, 60)
$DOT_ON = [System.Drawing.Color]::FromArgb(63, 216, 96)
$YAFONT = New-Object System.Drawing.Font('Microsoft YaHei', 10)
$YAFONT_B = New-Object System.Drawing.Font('Microsoft YaHei', 10, [System.Drawing.FontStyle]::Bold)
$BIGFONT = New-Object System.Drawing.Font('Microsoft YaHei', 15, [System.Drawing.FontStyle]::Bold)

$form = New-Object System.Windows.Forms.Form
$form.Text = 'linkWeixin'
$form.Size = New-Object System.Drawing.Size(288, 352)
$form.FormBorderStyle = 'None'
$form.TopMost = $true
$form.ShowInTaskbar = $false
$form.StartPosition = 'Manual'
$form.BackColor = $BG
$form.ForeColor = $FG
try {
  $wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
  $form.Location = New-Object System.Drawing.Point(($wa.Right - 308), ($wa.Bottom - 372))
} catch { }

# 顶部状态色条
$strip = New-Object System.Windows.Forms.Panel
$strip.Dock = 'Top'
$strip.Height = 5
$form.Controls.Add($strip)

# 标题栏
$bar = New-Object System.Windows.Forms.Panel
$bar.Dock = 'Top'
$bar.Height = 34
$bar.BackColor = $BG
$form.Controls.Add($bar)

$title = New-Object System.Windows.Forms.Label
$title.Text = '  🔔 linkWeixin'
$title.Font = $YAFONT_B
$title.ForeColor = $FG
$title.BackColor = $BG
$title.Location = New-Object System.Drawing.Point(4, 0)
$title.Size = New-Object System.Drawing.Size(200, 34)
$title.TextAlign = 'MiddleLeft'
$bar.Controls.Add($title)

$btnMin = New-Object System.Windows.Forms.Label
$btnMin.Text = '—'
$btnMin.Font = $YAFONT
$btnMin.ForeColor = $DIM
$btnMin.Size = New-Object System.Drawing.Size(36, 34)
$btnMin.Location = New-Object System.Drawing.Point(216, 0)
$btnMin.TextAlign = 'MiddleCenter'
$btnMin.Cursor = 'Hand'
$bar.Controls.Add($btnMin)

$btnX = New-Object System.Windows.Forms.Label
$btnX.Text = '✕'
$btnX.Font = $YAFONT
$btnX.ForeColor = $DIM
$btnX.Size = New-Object System.Drawing.Size(36, 34)
$btnX.Location = New-Object System.Drawing.Point(252, 0)
$btnX.TextAlign = 'MiddleCenter'
$btnX.Cursor = 'Hand'
$bar.Controls.Add($btnX)

# 大开关
$btn = New-Object System.Windows.Forms.Button
$btn.Location = New-Object System.Drawing.Point(16, 50)
$btn.Size = New-Object System.Drawing.Size(256, 64)
$btn.Font = $BIGFONT
$btn.FlatStyle = 'Flat'
$btn.FlatAppearance.BorderSize = 0
$btn.Cursor = 'Hand'
$form.Controls.Add($btn)

# 状态卡片
$card = New-Object System.Windows.Forms.Panel
$card.Location = New-Object System.Drawing.Point(16, 124)
$card.Size = New-Object System.Drawing.Size(256, 128)
$card.BackColor = $CARD
$form.Controls.Add($card)

function Add-Row {
  param([int]$Y, [string]$Name)
  $dot = New-Object System.Windows.Forms.Label
  $dot.Text = '●'
  $dot.Font = $YAFONT_B
  $dot.Location = New-Object System.Drawing.Point(12, $Y)
  $dot.Size = New-Object System.Drawing.Size(20, 28)
  $dot.TextAlign = 'MiddleCenter'
  $card.Controls.Add($dot)
  $txt = New-Object System.Windows.Forms.Label
  $txt.Font = $YAFONT
  $txt.ForeColor = $FG
  $txt.BackColor = $CARD
  $txt.Location = New-Object System.Drawing.Point(34, $Y)
  $txt.Size = New-Object System.Drawing.Size(210, 28)
  $txt.TextAlign = 'MiddleLeft'
  $card.Controls.Add($txt)
  return @{ Dot = $dot; Txt = $txt; Name = $Name }
}
$rowOc = Add-Row 8 'opencode'
$rowCx = Add-Row 46 'codex'
$rowLast = Add-Row 84 'last'
$rowLast.Dot.Text = '🕒'
$rowLast.Dot.ForeColor = $DIM

# 底栏提示
$hint = New-Object System.Windows.Forms.Label
$hint.Location = New-Object System.Drawing.Point(16, 258)
$hint.Size = New-Object System.Drawing.Size(256, 44)
$hint.Font = New-Object System.Drawing.Font('Microsoft YaHei', 8.5)
$hint.ForeColor = $DIM
$form.Controls.Add($hint)

$foot = New-Object System.Windows.Forms.Label
$foot.Location = New-Object System.Drawing.Point(16, 304)
$foot.Size = New-Object System.Drawing.Size(256, 20)
$foot.Font = New-Object System.Drawing.Font('Microsoft YaHei', 8)
$foot.ForeColor = [System.Drawing.Color]::FromArgb(110, 110, 115)
$foot.Text = '× 最小化到托盘 · 双击托盘图标恢复'
$form.Controls.Add($foot)

# 托盘
$iconOn = New-DotIcon $DOT_ON
$iconOff = New-DotIcon $RED
$notify = New-Object System.Windows.Forms.NotifyIcon
$notify.Text = 'linkWeixin 推送'
$notify.Visible = $true
$menu = New-Object System.Windows.Forms.ContextMenuStrip
$miShow = $menu.Items.Add('隐藏悬浮窗')
$miToggle = $menu.Items.Add('关闭推送')
[void]$menu.Items.Add('-')
$miExit = $menu.Items.Add('退出')
$notify.ContextMenuStrip = $menu

function Show-Window {
  $form.Show()
  $form.Activate()
  $miShow.Text = '隐藏悬浮窗'
}
function Hide-Window {
  $form.Hide()
  $miShow.Text = '显示悬浮窗'
  if (-not $script:trayTipped) {
    $script:trayTipped = $true
    $notify.ShowBalloonTip(3000, 'linkWeixin', '已最小化到托盘，双击图标可恢复。右键托盘有退出。', [System.Windows.Forms.ToolTipIcon]::Info)
  }
}
function Toggle-Window {
  if ($form.Visible) { Hide-Window } else { Show-Window }
}
function Real-Exit {
  $script:allowExit = $true
  $notify.Visible = $false
  $notify.Dispose()
  $form.Close()
}

$btnMin.Add_Click({ Hide-Window })
$btnX.Add_Click({ Hide-Window })
$btn.Add_Click({ Set-NotifyOn (-not (Get-NotifyOn)); Refresh-UI })
$miShow.Add_Click({ Toggle-Window })
$miToggle.Add_Click({ Set-NotifyOn (-not (Get-NotifyOn)); Refresh-UI })
$miExit.Add_Click({ Real-Exit })
$notify.Add_DoubleClick({ Toggle-Window })
$menu.Add_Opening({
  $miShow.Text = if ($form.Visible) { '隐藏悬浮窗' } else { '显示悬浮窗' }
  $miToggle.Text = if (Get-NotifyOn) { '关闭推送' } else { '开启推送' }
})
$form.Add_FormClosing({
  param($s, $e)
  if (-not $script:allowExit) { $e.Cancel = $true; Hide-Window }
})

# 拖动：按住标题栏移动无边框窗体
$drag = @{ On = $false; X = 0; Y = 0 }
$moveH = {
  if ($drag.On) {
    $form.Location = New-Object System.Drawing.Point(
      ([System.Windows.Forms.Cursor]::Position.X - $drag.X),
      ([System.Windows.Forms.Cursor]::Position.Y - $drag.Y))
  }
}
$downH = {
  $drag.On = $true
  $drag.X = [System.Windows.Forms.Cursor]::Position.X - $form.Location.X
  $drag.Y = [System.Windows.Forms.Cursor]::Position.Y - $form.Location.Y
}
$upH = { $drag.On = $false }
foreach ($c in @($bar, $title)) {
  $c.Add_MouseDown($downH); $c.Add_MouseMove($moveH); $c.Add_MouseUp($upH)
}

function Refresh-UI {
  $on = Get-NotifyOn
  $btn.Text = if ($on) { '●  推送开启' } else { '○  推送关闭' }
  $btn.BackColor = if ($on) { $GREEN } else { $RED }
  $btn.ForeColor = [System.Drawing.Color]::White
  $strip.BackColor = if ($on) { $GREEN } else { $RED }
  if ($script:lastOn -ne $on) {
    $script:lastOn = $on
    $notify.Icon = if ($on) { $iconOn } else { $iconOff }
  }
  $oc = Test-AppRunning @('OpenCode*', 'opencode*')
  # codex-plus-plus* 是无关常驻进程（Codex++，另一个软件），必须排除，
  # 否则关掉 Codex 桌面灯也不会灭。
  $cx = Test-AppRunning @('codex*') @('codex-plus-plus*')
  $rowOc.Txt.Text = if ($oc) { 'opencode  运行中' } else { 'opencode  未运行' }
  $rowOc.Dot.ForeColor = if ($oc) { $DOT_ON } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }
  $rowCx.Txt.Text = if ($cx) { 'codex  运行中' } else { 'codex  未运行' }
  $rowCx.Dot.ForeColor = if ($cx) { $DOT_ON } else { [System.Drawing.Color]::FromArgb(100, 100, 105) }
  $rowLast.Txt.Text = '上次推送 ' + (Get-LastPushText)
  $pv = Test-PluginGate
  if ($pv -eq '新版') {
    $hint.Text = '只管 opencode 侧推送，codex 侧不受影响。'
    $hint.ForeColor = $DIM
  } else {
    $hint.Text = "⚠ 插件$pv：开关不生效，重跑 install 后重启桌面。"
    $hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
  }
}

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 3000
$timer.Add_Tick({ Refresh-UI })
$timer.Start()

$form.Add_Shown({
  Refresh-UI
  $notify.ShowBalloonTip(3000, 'linkWeixin', '悬浮窗已启动。× 最小化到托盘（任务栏 ^ 里找绿/红点，可拖出来），双击托盘图标恢复。', [System.Windows.Forms.ToolTipIcon]::Info)
})
[void]$form.ShowDialog()
exit 0
