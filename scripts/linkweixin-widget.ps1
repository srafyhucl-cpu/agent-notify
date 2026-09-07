#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 悬浮窗：大开关 + opencode/codex 运行灯 + 上次推送时间。

.DESCRIPTION
  常驻启动（控制台藏掉，只留窗体）：
    powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File linkweixin-widget.ps1
  install.ps1 会在 shell:startup 建开机快捷方式（无需管理员），重启后自启。
  无边框窗体，拖标题区移动；右上角 × 退出。
  内容只有状态显示 + 翻 marker，不做 token/时段输入框。
  进程名已在本机实测：opencode 侧 'OpenCode*'（桌面）/'opencode*'（cli/service），
  codex 侧 'codex*'（codex / codex-code-mode-host / …）。
  上次推送时间读 notify-push.log 尾行（与插件同路径约定）。
#>
param(
  [string]$MarkerPath = (Join-Path $env:USERPROFILE '.config\opencode\notify-pushplus.off')
)

$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$pushLog = if ($env:OPENCODE_NOTIFY_LOG_FILE) { $env:OPENCODE_NOTIFY_LOG_FILE } else { Join-Path $env:TEMP 'opencode\notify-push.log' }
$pluginPath = Join-Path $env:USERPROFILE '.config\opencode\plugin\notify-pushplus.ts'

# 装上去的插件是不是带三道闸的新版：旧版不认 marker，关了也照推，
# 悬浮窗直接提示，避免静默失效。
function Test-PluginGate {
  try {
    if (-not (Test-Path $pluginPath)) { return '未安装' }
    if (Select-String -Path $pluginPath -Pattern 'markerOff' -SimpleMatch -Quiet) { return '新版' }
    return '旧版'
  } catch { return '未知' }
}

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

function Get-LastPushText {
  try {
    if (-not (Test-Path $pushLog)) { return '暂无推送' }
    $tail = Get-Content $pushLog -Tail 1 -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($tail)) { return '暂无推送' }
    $m = [regex]::Match($tail, '(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})')
    if ($m.Success) { return "$($m.Groups[1].Value) $($m.Groups[2].Value)" }
    return $tail.Trim()
  } catch { return '暂无推送' }
}

$form = New-Object Windows.Forms.Form
$form.Text = 'linkWeixin'
$form.Size = New-Object Drawing.Size(248, 232)
$form.FormBorderStyle = 'None'
$form.TopMost = $true
$form.ShowInTaskbar = $false
$form.StartPosition = 'Manual'
$form.BackColor = [Drawing.Color]::FromArgb(45, 45, 48)
$form.ForeColor = [Drawing.Color]::WhiteSmoke
try {
  $wa = [Windows.Forms.Screen]::PrimaryScreen.WorkingArea
  $form.Location = New-Object Drawing.Point(($wa.Right - 268), ($wa.Bottom - 252))
} catch { }

# 标题区（拖这里移动）+ 关闭按钮
$title = New-Object Windows.Forms.Label
$title.Text = '  linkWeixin 推送'
$title.Dock = 'Top'
$title.Height = 26
$title.BackColor = [Drawing.Color]::FromArgb(30, 30, 30)
$title.ForeColor = [Drawing.Color]::WhiteSmoke
$title.TextAlign = 'MiddleLeft'
$form.Controls.Add($title)

$close = New-Object Windows.Forms.Label
$close.Text = '×'
$close.Size = New-Object Drawing.Size(30, 26)
$close.Location = New-Object Drawing.Point(218, 0)
$close.Anchor = 'Top, Right'
$close.TextAlign = 'MiddleCenter'
$close.Cursor = 'Hand'
$close.ForeColor = [Drawing.Color]::WhiteSmoke
$close.Add_Click({ $form.Close() })
$form.Controls.Add($close)
$close.BringToFront()

# 大开关
$btn = New-Object Windows.Forms.Button
$btn.Size = New-Object Drawing.Size(208, 56)
$btn.Location = New-Object Drawing.Point(20, 38)
$btn.Font = New-Object Drawing.Font('Microsoft YaHei', 14, [Drawing.FontStyle]::Bold)
$btn.FlatStyle = 'Flat'
$btn.Cursor = 'Hand'
$btn.Add_Click({ Set-NotifyOn (-not (Get-NotifyOn)); Refresh-UI })
$form.Controls.Add($btn)

# 运行状态灯
$lblOc = New-Object Windows.Forms.Label
$lblOc.Size = New-Object Drawing.Size(208, 22)
$lblOc.Location = New-Object Drawing.Point(20, 104)
$lblOc.Font = New-Object Drawing.Font('Microsoft YaHei', 10)
$form.Controls.Add($lblOc)

$lblCx = New-Object Windows.Forms.Label
$lblCx.Size = New-Object Drawing.Size(208, 22)
$lblCx.Location = New-Object Drawing.Point(20, 128)
$lblCx.Font = New-Object Drawing.Font('Microsoft YaHei', 10)
$form.Controls.Add($lblCx)

# 上次推送
$lblLast = New-Object Windows.Forms.Label
$lblLast.Size = New-Object Drawing.Size(208, 22)
$lblLast.Location = New-Object Drawing.Point(20, 152)
$lblLast.Font = New-Object Drawing.Font('Microsoft YaHei', 9)
$lblLast.ForeColor = [Drawing.Color]::Silver
$form.Controls.Add($lblLast)

$hint = New-Object Windows.Forms.Label
$hint.Size = New-Object Drawing.Size(208, 20)
$hint.Location = New-Object Drawing.Point(20, 176)
$hint.Font = New-Object Drawing.Font('Microsoft YaHei', 8)
$hint.ForeColor = [Drawing.Color]::Gray
$hint.Text = '拖标题区移动 · 只管 opencode 侧'
$form.Controls.Add($hint)

function Refresh-UI {
  $on = Get-NotifyOn
  $btn.Text = if ($on) { '推送：ON' } else { '推送：OFF' }
  $btn.BackColor = if ($on) { [Drawing.Color]::FromArgb(56, 142, 60) } else { [Drawing.Color]::FromArgb(198, 40, 40) }
  $btn.ForeColor = [Drawing.Color]::White
  $oc = Test-AppRunning @('OpenCode*', 'opencode*')
  # codex-plus-plus* 是无关常驻进程（Codex++，另一个软件），必须排除，
  # 否则关掉 Codex 桌面灯也不会灭。
  $cx = Test-AppRunning @('codex*') @('codex-plus-plus*')
  $lblOc.Text = if ($oc) { '● opencode 运行中' } else { '○ opencode 未运行' }
  $lblOc.ForeColor = if ($oc) { [Drawing.Color]::LightGreen } else { [Drawing.Color]::Gray }
  $lblCx.Text = if ($cx) { '● codex 运行中' } else { '○ codex 未运行' }
  $lblCx.ForeColor = if ($cx) { [Drawing.Color]::LightGreen } else { [Drawing.Color]::Gray }
  $lblLast.Text = '上次推送：' + (Get-LastPushText)
  $pv = Test-PluginGate
  $hint.Text = if ($pv -eq '新版') { '拖标题区移动 · 只管 opencode 侧' } else { "⚠插件$pv：重跑 install+重启桌面" }
  $hint.ForeColor = if ($pv -eq '新版') { [Drawing.Color]::Gray } else { [Drawing.Color]::Orange }
}

# 拖动：按住标题区移动无边框窗体
$drag = @{ On = $false; X = 0; Y = 0 }
$title.Add_MouseDown({
  $drag.On = $true
  $drag.X = [Windows.Forms.Cursor]::Position.X - $form.Location.X
  $drag.Y = [Windows.Forms.Cursor]::Position.Y - $form.Location.Y
})
$title.Add_MouseMove({
  if ($drag.On) {
    $form.Location = New-Object Drawing.Point(
      ([Windows.Forms.Cursor]::Position.X - $drag.X),
      ([Windows.Forms.Cursor]::Position.Y - $drag.Y))
  }
})
$title.Add_MouseUp({ $drag.On = $false })

$timer = New-Object Windows.Forms.Timer
$timer.Interval = 3000
$timer.Add_Tick({ Refresh-UI })
$timer.Start()

$form.Add_Shown({ Refresh-UI })
[void]$form.ShowDialog()
exit 0
