#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 悬浮窗：opencode/codex 独立开关 + 运行灯 + 上次推送时间 + 托盘常驻。

.DESCRIPTION
  常驻启动（控制台从不存在，Windows Terminal 也拦截不到）：
    wscript.exe "C:\Users\你\bin\run-hidden.vbs" "C:\Users\你\bin\linkweixin-widget.ps1"
  （不要直接双击 ps1 / 用 powershell 拉：Win11 默认终端下会留黑窗口/页签。）
  install.ps1 会建 shell:startup 开机快捷方式 + 桌面快捷方式（都无需管理员）。
  无边框窗体，拖标题区移动。右上角 × / — 是最小化到托盘（首次有气泡提示），
  双击托盘图标恢复，右键托盘菜单可开关推送或彻底退出。
  再打开方式：双击托盘图标 / 桌面“linkWeixin 悬浮窗” / 上面那条手动命令。
  内容只有状态显示 + 翻 marker，不做 token/时段输入框。
  进程名已在本机实测：opencode 侧 'OpenCode*'（桌面）/'opencode*'（cli/service），
  codex 侧 'codex*'（codex-plus-plus* 是无关软件 Codex++，已排除）。轮询 5 秒一次，
  开关点击即时刷新；插件版本启动查一次、之后 10 分钟复查，心跳约 30 秒写一次，
  常驻开销只有内存（一个 hidden powershell），不阻止系统睡眠。
  上次推送时间读 notify-push.log 尾行（与插件同路径约定）。
#>
param(
  [string]$MarkerPath = (Join-Path $env:USERPROFILE '.config\opencode\notify-pushplus.off')
)

$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# 共享逻辑模块（marker 读写/路径/版本等都在里面）。加载失败直接可见地退出，
# 并落盘日志，不允许“静默死亡”。
try {
  Import-Module (Join-Path $PSScriptRoot 'lib\LinkWeixin\LinkWeixin.psd1') -ErrorAction Stop
} catch {
  $err = $_.Exception
  try {
    New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP 'opencode') | Out-Null
    "$(Get-Date -Format o) [module] $($err | Out-String)" | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-error.log') -Append -Encoding utf8
  } catch { }
  try { [System.Windows.Forms.MessageBox]::Show("linkWeixin 模块加载失败：$($err.Message)", 'linkWeixin') | Out-Null } catch { }
  exit 1
}

# 全局兜底：UI 线程/未处理异常全部落盘。WinForms 事件里的漏网异常走这里，
# 否则就是“静默死亡、无日志”，上次丢进程就是这么查不出来的。
try {
  [System.Windows.Forms.Application]::SetUnhandledExceptionMode([System.Windows.Forms.UnhandledExceptionMode]::CatchException)
  [System.Windows.Forms.Application]::Add_ThreadException({
    param($s, $e)
    try {
      "$(Get-Date -Format o) [ui-thread] $($e.Exception | Out-String)" | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-error.log') -Append -Encoding utf8
    } catch { }
  })
  [System.AppDomain]::CurrentDomain.add_UnhandledException({
    param($s, $e)
    try {
      "$(Get-Date -Format o) [fatal] $($e.ExceptionObject | Out-String)" | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-error.log') -Append -Encoding utf8
    } catch { }
  })
} catch { }

# 单实例：互斥锁 + 带等待的清扫，任何时刻最多一个，且绝不出现“旧的被杀、
# 新的又退出、最后谁都不剩”的真空（之前就是这么把自己玩没的）。
# 桌面双击 = 新实例接管（开关状态全在 marker 文件里，不丢）。
# 锁必须持有到进程退出（不释放），实在抢不到且谁都没杀才安静退出。
$mtx = $null
$killedAny = $false
function Test-OldWidget {
  param([int]$MyPid, [int]$MyParent)
  try {
    return @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" -ErrorAction Stop |
      Where-Object {
        # 只认 -File 直跑本脚本的宿主：纯子串会误伤命令行里带脚本名的调用方。
        ($_.CommandLine -match '\-File\s+"[^"]*linkweixin-widget\.ps1"') -and
        ($_.ProcessId -ne $MyPid) -and ($_.ProcessId -ne $MyParent)
      })
  } catch { return @() }
}
try {
  $myPid = $PID
  $myParent = (Get-CimInstance Win32_Process -Filter "ProcessId=$myPid" -ErrorAction SilentlyContinue).ParentProcessId
  $mtx = New-Object System.Threading.Mutex($false, 'Global\LinkWeixinWidgetSingleInstance')
  $owns = $false
  try { $owns = $mtx.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owns = $true }
  if (-not $owns) { Start-Sleep -Seconds 2 }  # 等对方稳定（或确认它是 hung 的尸体）
  for ($i = 0; $i -lt 6; $i++) {
    foreach ($p in (Test-OldWidget $myPid $myParent)) {
      try { Stop-Process -Id $p.ProcessId -Force -ErrorAction Stop; $killedAny = $true } catch { }
    }
    Start-Sleep -Seconds 2  # 等被杀的进程彻底退出、锁释放（杀锁主不等于锁立即可用）
    try { $owns = $mtx.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owns = $true }
    if ($owns -and @(Test-OldWidget $myPid $myParent).Count -eq 0) { break }
  }
  if (-not $owns -and -not $killedAny) { exit 0 }  # 对方健康活着，我安静退出
} catch { }

$pushLog = if ($env:OPENCODE_NOTIFY_LOG_FILE) { $env:OPENCODE_NOTIFY_LOG_FILE } else { Join-Path $env:TEMP 'opencode\notify-push.log' }
$pluginPath = Join-Path $env:USERPROFILE '.config\opencode\plugin\notify-pushplus.ts'
# codex 侧独立 marker（与 notify-toggle -Agent Codex、codex wrapper 同路径约定）。
$codexMarker = if ($env:CODEX_NOTIFY_MARKER_FILE) { $env:CODEX_NOTIFY_MARKER_FILE } else { Join-Path $env:USERPROFILE '.config\opencode\codex-notify.off' }
$errLog = Join-Path $env:TEMP 'opencode\widget-error.log'
$aliveFile = Join-Path $env:TEMP 'opencode\widget-alive.txt'
function Log-Err {
  param([string]$Where, [object]$Ex)
  try { "$(Get-Date -Format o) [$Where] $($Ex | Out-String)" | Out-File -FilePath $errLog -Append -Encoding utf8 } catch { }
}

$script:allowExit = $false
$script:lastOn = $null
$script:tickN = 0
$script:plugVer = $null
$script:taskVer = $null
$script:iconBmps = @()

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
  # opencode 侧开关（codex 侧另有独立开关，见下）。marker 存在 = 关。
  return -not (Test-NotifyMarker -Path $MarkerPath)
}

function Set-NotifyOn {
  param([bool]$TurnOn)
  $mode = if ($TurnOn) { 'On' } else { 'Off' }
  [void](Set-NotifyMarker -Path $MarkerPath -Mode $mode)
}

function Get-CodexNotifyOn {
  return -not (Test-NotifyMarker -Path $codexMarker)
}

function Set-CodexNotifyOn {
  param([bool]$TurnOn)
  $mode = if ($TurnOn) { 'On' } else { 'Off' }
  [void](Set-NotifyMarker -Path $codexMarker -Mode $mode)
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

# 看守任务是不是隐藏版：旧版 install 注册的动作不带 -WindowStyle Hidden，
# 每 5 分钟闪一次窗口。读任务定义不需要管理员权限，查出来就提示用户管理员重跑。
function Test-WatchTask {
  try {
    $xml = schtasks /query /tn CodexNotifyWatch /xml 2>$null | Out-String
    if ([string]::IsNullOrWhiteSpace($xml)) { return '新版' } # 没装看守就不报警
    if ($xml -match 'WindowStyle\s+Hidden') { return '新版' }
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
  # 图标创建失败必须留痕：曾经出现过静默 $null（任务栏变回 powershell 图标、
  # 托盘无图标、零报错），原因是构造期语句级错误被 SilentlyContinue 吞掉。
  try {
    $ErrorActionPreference = 'Stop'
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
    # 注意：$bmp 绝不能 Dispose/被回收——HICON 底片依赖它活着。
    # 之前 Dispose 后托盘因 Explorer 建时缓存了像素所以显示正常，
    # 但任务栏每次重绘都读 live 句柄，位图一死就回退成 powershell 默认图标。
    # 3 张 16x16 常驻，内存忽略不计。
    $script:iconBmps += $bmp
    return $ico
  } catch {
    Log-Err 'icon' $_
    return $null
  }
}

$BG = [System.Drawing.Color]::FromArgb(31, 31, 35)
$CardBG = [System.Drawing.Color]::FromArgb(42, 42, 47)
$FG = [System.Drawing.Color]::FromArgb(240, 240, 240)
$DIM = [System.Drawing.Color]::FromArgb(150, 150, 155)
$GREEN = [System.Drawing.Color]::FromArgb(46, 160, 67)
$RED = [System.Drawing.Color]::FromArgb(200, 60, 60)
$DOT_ON = [System.Drawing.Color]::FromArgb(63, 216, 96)
$YAFONT = New-Object System.Drawing.Font('Microsoft YaHei', 10)
$YAFONT_B = New-Object System.Drawing.Font('Microsoft YaHei', 10, [System.Drawing.FontStyle]::Bold)
$BIGFONT = New-Object System.Drawing.Font('Microsoft YaHei', 15, [System.Drawing.FontStyle]::Bold)
$MIDFONT = New-Object System.Drawing.Font('Microsoft YaHei', 11, [System.Drawing.FontStyle]::Bold)

$form = New-Object System.Windows.Forms.Form
$form.Text = 'linkWeixin'
$form.Size = New-Object System.Drawing.Size(288, 352)
$form.FormBorderStyle = 'None'
$form.TopMost = $true
# 任务栏不留按钮：只活在托盘 + 桌面快捷方式（单实例接管）。
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
$title.Text = '  linkWeixin 推送'
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

# 两个独立大开关：opencode / codex 各管一边，各翻各的 marker
$btnOc = New-Object System.Windows.Forms.Button
$btnOc.Location = New-Object System.Drawing.Point(16, 50)
$btnOc.Size = New-Object System.Drawing.Size(124, 62)
$btnOc.Font = $MIDFONT
$btnOc.FlatStyle = 'Flat'
$btnOc.FlatAppearance.BorderSize = 0
$btnOc.Cursor = 'Hand'
$form.Controls.Add($btnOc)

$btnCx = New-Object System.Windows.Forms.Button
$btnCx.Location = New-Object System.Drawing.Point(148, 50)
$btnCx.Size = New-Object System.Drawing.Size(124, 62)
$btnCx.Font = $MIDFONT
$btnCx.FlatStyle = 'Flat'
$btnCx.FlatAppearance.BorderSize = 0
$btnCx.Cursor = 'Hand'
$form.Controls.Add($btnCx)

# 状态卡片
$card = New-Object System.Windows.Forms.Panel
$card.Location = New-Object System.Drawing.Point(16, 124)
$card.Size = New-Object System.Drawing.Size(256, 128)
$card.BackColor = $CardBG
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
  $txt.BackColor = $CardBG
  $txt.Location = New-Object System.Drawing.Point(34, $Y)
  $txt.Size = New-Object System.Drawing.Size(210, 28)
  $txt.TextAlign = 'MiddleLeft'
  $card.Controls.Add($txt)
  return @{ Dot = $dot; Txt = $txt; Name = $Name }
}
$rowOc = Add-Row 8 'opencode'
$rowCx = Add-Row 46 'codex'
$rowLast = Add-Row 84 'last'
$rowLast.Dot.Text = '•'
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
$foot.Size = New-Object System.Drawing.Size(208, 20)
$foot.Font = New-Object System.Drawing.Font('Microsoft YaHei', 8)
$foot.ForeColor = [System.Drawing.Color]::FromArgb(110, 110, 115)
$foot.Text = '× 藏到托盘 · 双击托盘图标恢复'
$form.Controls.Add($foot)

$btnQuit = New-Object System.Windows.Forms.Label
$btnQuit.Text = '退出'
$btnQuit.Font = New-Object System.Drawing.Font('Microsoft YaHei', 8)
$btnQuit.ForeColor = [System.Drawing.Color]::FromArgb(200, 100, 100)
$btnQuit.BackColor = $BG
$btnQuit.Size = New-Object System.Drawing.Size(40, 20)
$btnQuit.Location = New-Object System.Drawing.Point(232, 304)
$btnQuit.TextAlign = 'MiddleCenter'
$btnQuit.Cursor = 'Hand'
$form.Controls.Add($btnQuit)

# 托盘
$iconOn = New-DotIcon $DOT_ON
$iconMid = New-DotIcon ([System.Drawing.Color]::FromArgb(255, 170, 60))
$iconOff = New-DotIcon $RED
# 兜底：自绘图标任一失败就用系统默认图标，保证任务栏/托盘一定有东西（丑但可见）。
if ($null -eq $iconOn -or $null -eq $iconMid -or $null -eq $iconOff) {
  try {
    $fb = [System.Drawing.SystemIcons]::Application
    if ($null -eq $iconOn) { $iconOn = $fb }
    if ($null -eq $iconMid) { $iconMid = $fb }
    if ($null -eq $iconOff) { $iconOff = $fb }
  } catch { Log-Err 'icon-fb' $_ }
}
$notify = New-Object System.Windows.Forms.NotifyIcon
$notify.Text = 'linkWeixin 推送'
$notify.Icon = $iconOn
$notify.Visible = $true
$menu = New-Object System.Windows.Forms.ContextMenuStrip
$miShow = $menu.Items.Add('隐藏悬浮窗')
$miOc = $menu.Items.Add('关闭 opencode 推送')
$miCx = $menu.Items.Add('关闭 codex 推送')
[void]$menu.Items.Add('-')
$miExit = $menu.Items.Add('退出')
$notify.ContextMenuStrip = $menu
# 窗体图标也用状态圆点：任务栏按钮显示它，不再是 powershell 默认图标。
$form.Icon = $iconOn

function Show-Window {
  $form.WindowState = 'Normal'
  $form.Show()
  $form.Activate()
  $miShow.Text = '隐藏悬浮窗'
}
function Hide-Window {
  # 藏到托盘：任务栏无按钮，靠托盘图标 / 桌面快捷方式（单实例接管）回来。
  $form.Hide()
  $miShow.Text = '显示悬浮窗'
}
function Toggle-Window {
  if ($form.Visible) { Hide-Window } else { Show-Window }
}
function Real-Exit {
  $script:allowExit = $true
  try { 'user-exit ' + (Get-Date -Format o) | Out-File -FilePath $aliveFile -Encoding utf8 -Force } catch { }
  $notify.Visible = $false
  $notify.Dispose()
  $form.Close()
}

$btnMin.Add_Click({ try { Hide-Window } catch { Log-Err 'min' $_ } })
$btnX.Add_Click({ try { Hide-Window } catch { Log-Err 'x' $_ } })
$btnQuit.Add_Click({ try { Real-Exit } catch { Log-Err 'quit' $_ } })
$btnOc.Add_Click({ try { Set-NotifyOn (-not (Get-NotifyOn)); Refresh-UI } catch { Log-Err 'btnOc' $_ } })
$btnCx.Add_Click({ try { Set-CodexNotifyOn (-not (Get-CodexNotifyOn)); Refresh-UI } catch { Log-Err 'btnCx' $_ } })
$miShow.Add_Click({ try { Toggle-Window } catch { Log-Err 'miShow' $_ } })
$miOc.Add_Click({ try { Set-NotifyOn (-not (Get-NotifyOn)); Refresh-UI } catch { Log-Err 'miOc' $_ } })
$miCx.Add_Click({ try { Set-CodexNotifyOn (-not (Get-CodexNotifyOn)); Refresh-UI } catch { Log-Err 'miCx' $_ } })
$miExit.Add_Click({ try { Real-Exit } catch { Log-Err 'miExit' $_ } })
$notify.Add_DoubleClick({ try { Toggle-Window } catch { Log-Err 'dblclick' $_ } })
$menu.Add_Opening({ try {
  $miShow.Text = if ($form.Visible) { '隐藏悬浮窗' } else { '显示悬浮窗' }
  $miOc.Text = if (Get-NotifyOn) { '关闭 opencode 推送' } else { '开启 opencode 推送' }
  $miCx.Text = if (Get-CodexNotifyOn) { '关闭 codex 推送' } else { '开启 codex 推送' }
} catch { Log-Err 'opening' $_ } })
$form.Add_FormClosing({
  param($s, $e)
  try {
    if (-not $script:allowExit) { $e.Cancel = $true; Hide-Window }
  } catch { Log-Err 'closing' $_ }
})

# 拖动：按住标题栏移动无边框窗体
$drag = @{ On = $false; X = 0; Y = 0 }
$moveH = {
  try {
    if ($drag.On) {
      $form.Location = New-Object System.Drawing.Point(
        ([System.Windows.Forms.Cursor]::Position.X - $drag.X),
        ([System.Windows.Forms.Cursor]::Position.Y - $drag.Y))
    }
  } catch { Log-Err 'drag' $_ }
}
$downH = {
  try {
    $drag.On = $true
    $drag.X = [System.Windows.Forms.Cursor]::Position.X - $form.Location.X
    $drag.Y = [System.Windows.Forms.Cursor]::Position.Y - $form.Location.Y
  } catch { Log-Err 'dragdown' $_ }
}
$upH = { try { $drag.On = $false } catch { Log-Err 'dragup' $_ } }
foreach ($c in @($bar, $title)) {
  $c.Add_MouseDown($downH); $c.Add_MouseMove($moveH); $c.Add_MouseUp($upH)
}

function Refresh-UI {
  $onOc = Get-NotifyOn
  $onCx = Get-CodexNotifyOn
  $script:tickN++
  $btnOc.Text = if ($onOc) { "opencode`n● ON" } else { "opencode`n○ OFF" }
  $btnOc.BackColor = if ($onOc) { $GREEN } else { $RED }
  $btnOc.ForeColor = [System.Drawing.Color]::White
  $btnCx.Text = if ($onCx) { "codex`n● ON" } else { "codex`n○ OFF" }
  $btnCx.BackColor = if ($onCx) { $GREEN } else { $RED }
  $btnCx.ForeColor = [System.Drawing.Color]::White
  # 色条/托盘：两边都开绿，都关红，一开一关橙
  $state = if ($onOc -and $onCx) { 2 } elseif (-not $onOc -and -not $onCx) { 0 } else { 1 }
  $strip.BackColor = if ($state -eq 2) { $GREEN } elseif ($state -eq 0) { $RED } else { [System.Drawing.Color]::FromArgb(200, 130, 30) }
  if ($script:lastOn -ne $state) {
    $script:lastOn = $state
    $notify.Icon = if ($state -eq 2) { $iconOn } elseif ($state -eq 0) { $iconOff } else { $iconMid }
    $form.Icon = $notify.Icon
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
  # 插件/任务版本几乎不变：启动查一次，之后每 10 分钟复查，不再每轮读文件。
  if ($null -eq $script:plugVer -or ($script:tickN % 120) -eq 1) { $script:plugVer = Test-PluginGate }
  if ($null -eq $script:taskVer -or ($script:tickN % 120) -eq 1) { $script:taskVer = Test-WatchTask }
  $pv = $script:plugVer
  if ($pv -eq '新版') {
    $hint.Text = '两个开关独立，各管一边。'
    $hint.ForeColor = $DIM
  } else {
    $hint.Text = "⚠ 插件$pv：开关不生效，重跑 install 后重启桌面。"
    $hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
  }
  # 任务旧版另起提示（别覆盖插件报警，插件问题更严重）。
  if ($pv -eq '新版' -and $script:taskVer -eq '旧版') {
    $hint.Text = '⚠ 看守任务旧版：每5分钟闪窗口，管理员重跑 install.ps1。'
    $hint.ForeColor = [System.Drawing.Color]::FromArgb(255, 170, 60)
  }
}

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 5000
$timer.Add_Tick({
  try {
    Refresh-UI
    # 心跳 ~30 秒写一次：够定位“崩/杀/退”，不值得每轮写盘。
    if (($script:tickN % 6) -eq 0) {
      (Get-Date -Format o) | Out-File -FilePath $aliveFile -Encoding utf8 -Force
    }
    # 托盘图标自愈：Explorer 重启/托盘区抽风会丢图标（进程活着但图标没了），
    # 每 ~5 分钟重新 Visible 一次把它顶回去，无闪烁感，有问题进日志。
    if (($script:tickN % 60) -eq 0) {
      try { $notify.Visible = $false; $notify.Visible = $true } catch { Log-Err 'repulse' $_ }
    }
  } catch { Log-Err 'tick' $_ }
})
$timer.Start()
'boot ok ' + $PID + ' ' + (Get-Date -Format o) | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-boot.log') -Encoding utf8 -Force

$form.Add_Shown({
  try { Refresh-UI } catch { Log-Err 'shown' $_ }
  try { $notify.ShowBalloonTip(3000, 'linkWeixin', '悬浮窗已启动。× 藏到托盘（^ 里找绿/红点，可拖出来），双击恢复；右下角红字可彻底退出。', [System.Windows.Forms.ToolTipIcon]::Info) } catch { Log-Err 'tip' $_ }
})
try {
  [void]$form.ShowDialog()
} catch {
  Log-Err 'show' $_
}
exit 0
