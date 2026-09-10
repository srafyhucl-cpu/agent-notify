# 悬浮窗 · 窗口动作与事件接线（由入口脚本 dot-source）。
# — 最小化到任务栏；× 藏到托盘并弹气泡；退出写标记（看守任务据分歧路）；
# 位置记忆、悬停反馈、托盘菜单、测试推送。

function Save-WidgetPosition {
  param([hashtable]$Ctx)
  try { "$($Ctx.Form.Location.X),$($Ctx.Form.Location.Y)" | Out-File -FilePath $Ctx.PosFile -Encoding UTF8 -Force } catch { }
}

function Show-WidgetWindow {
  param([hashtable]$Ctx)
  $Ctx.Form.WindowState = 'Normal'
  $Ctx.Form.Show()
  $Ctx.Form.Activate()
  $Ctx.MiShow.Text = '隐藏悬浮窗'
}

function Hide-WidgetWindow {
  param([hashtable]$Ctx, [switch]$Balloon)
  Save-WidgetPosition -Ctx $Ctx
  # 藏到托盘：任务栏无按钮，靠托盘图标 / 桌面快捷方式（单实例接管）回来。
  $Ctx.Form.Hide()
  $Ctx.MiShow.Text = '显示悬浮窗'
  if ($Balloon) {
    try {
      $Ctx.Notify.ShowBalloonTip(3000, 'linkWeixin 已藏到托盘', '双击托盘图标恢复（图标可能在 ^ 溢出区，可拖出来钉住）；找不到就双击桌面「linkWeixin 悬浮窗」。', [System.Windows.Forms.ToolTipIcon]::Info)
    } catch { Write-WidgetError 'balloon' $_ }
  }
}

function Minimize-WidgetWindow {
  # Minimize 非核准动词，但对 WindowState 语义最准确；核准动词替代名都更晦涩，定向豁免。
  [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseApprovedVerbs', '', Justification = 'Minimize 对 WinForms WindowState 语义最准确；已评估核准动词替代名均更晦涩')]
  param([hashtable]$Ctx)
  # 最小化到任务栏：任务栏按钮常驻，随时点得回来。
  Save-WidgetPosition -Ctx $Ctx
  $Ctx.Form.WindowState = 'Minimized'
}

function Switch-WidgetWindow {
  param([hashtable]$Ctx)
  $shown = $Ctx.Form.Visible -and $Ctx.Form.WindowState -ne [System.Windows.Forms.FormWindowState]::Minimized
  if ($shown) { Hide-WidgetWindow -Ctx $Ctx } else { Show-WidgetWindow -Ctx $Ctx }
}

function Close-Widget {
  param([hashtable]$Ctx)
  $Ctx.AllowExit = $true
  # 主动退出标记：看守任务看到它就不会自动拉起（区分“崩了”与“用户退出”）。
  try { (Get-Date -Format o) | Out-File -FilePath $Ctx.ExitMarker -Encoding utf8 -Force } catch { }
  Save-WidgetPosition -Ctx $Ctx
  try { 'user-exit ' + (Get-Date -Format o) | Out-File -FilePath $Ctx.AliveFile -Encoding utf8 -Force } catch { }
  $Ctx.Notify.Visible = $false
  $Ctx.Notify.Dispose()
  $Ctx.Form.Close()
}

function Register-WidgetEvents {
  param([hashtable]$Ctx)

  $Ctx.BtnMin.Add_Click({ try { Minimize-WidgetWindow -Ctx $Ctx } catch { Write-WidgetError 'min' $_ } })
  $Ctx.BtnX.Add_Click({ try { Hide-WidgetWindow -Ctx $Ctx -Balloon } catch { Write-WidgetError 'x' $_ } })
  $Ctx.BtnQuit.Add_Click({ try { Close-Widget -Ctx $Ctx } catch { Write-WidgetError 'quit' $_ } })
  $Ctx.BtnOc.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $Ctx.MarkerPath)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $Ctx.MarkerPath -Mode $mode)
        Update-WidgetState -Ctx $Ctx
      } catch { Write-WidgetError 'btnOc' $_ }
    })
  $Ctx.BtnCx.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $Ctx.CodexMarker)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $Ctx.CodexMarker -Mode $mode)
        Update-WidgetState -Ctx $Ctx
      } catch { Write-WidgetError 'btnCx' $_ }
    })
  $Ctx.MiShow.Add_Click({ try { Switch-WidgetWindow -Ctx $Ctx } catch { Write-WidgetError 'miShow' $_ } })
  $Ctx.MiOc.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $Ctx.MarkerPath)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $Ctx.MarkerPath -Mode $mode)
        Update-WidgetState -Ctx $Ctx
      } catch { Write-WidgetError 'miOc' $_ }
    })
  $Ctx.MiCx.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $Ctx.CodexMarker)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $Ctx.CodexMarker -Mode $mode)
        Update-WidgetState -Ctx $Ctx
      } catch { Write-WidgetError 'miCx' $_ }
    })
  $Ctx.MiTest.Add_Click({
      try {
        $script = Join-Path $Ctx.ScriptDir 'notify-ai.ps1'
        if (Test-Path $script) {
          Start-Process 'powershell.exe' -ArgumentList @(
            '-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass',
            '-File', $script, '-Title', '【测试】linkWeixin', '-Summary', '来自悬浮窗的测试推送。', '-NoStdin'
          ) -WindowStyle Hidden
        }
      } catch { Write-WidgetError 'testpush' $_ }
    })
  $Ctx.MiExit.Add_Click({ try { Close-Widget -Ctx $Ctx } catch { Write-WidgetError 'miExit' $_ } })
  $Ctx.Notify.Add_DoubleClick({ try { Switch-WidgetWindow -Ctx $Ctx } catch { Write-WidgetError 'dblclick' $_ } })
  $Ctx.Menu.Add_Opening({
      try {
        $shown = $Ctx.Form.Visible -and $Ctx.Form.WindowState -ne [System.Windows.Forms.FormWindowState]::Minimized
        $Ctx.MiShow.Text = if ($shown) { '隐藏悬浮窗' } else { '显示悬浮窗' }
        $onOc = -not (Test-NotifyMarker -Path $Ctx.MarkerPath)
        $onCx = -not (Test-NotifyMarker -Path $Ctx.CodexMarker)
        $Ctx.MiOc.Text = if ($onOc) { '关闭 opencode 推送' } else { '开启 opencode 推送' }
        $Ctx.MiCx.Text = if ($onCx) { '关闭 codex 推送' } else { '开启 codex 推送' }
      } catch { Write-WidgetError 'opening' $_ }
    })
  $Ctx.Form.Add_FormClosing({
      param($s, $e)
      $null = $s  # sender：事件签名要求，实际不用
      try {
        if (-not $Ctx.AllowExit) { $e.Cancel = $true; Hide-WidgetWindow -Ctx $Ctx }
      } catch { Write-WidgetError 'closing' $_ }
    })

  # 悬停反馈：开关按钮变亮；标注按钮变白（离开时状态刷新会还原开关色）。
  $Ctx.BtnOc.Add_MouseEnter({ try { $Ctx.HoverOc = $true; $Ctx.BtnOc.BackColor = Get-LighterColor $Ctx.BtnOc.BackColor } catch { } })
  $Ctx.BtnOc.Add_MouseLeave({ try { $Ctx.HoverOc = $false; Update-WidgetState -Ctx $Ctx } catch { } })
  $Ctx.BtnCx.Add_MouseEnter({ try { $Ctx.HoverCx = $true; $Ctx.BtnCx.BackColor = Get-LighterColor $Ctx.BtnCx.BackColor } catch { } })
  $Ctx.BtnCx.Add_MouseLeave({ try { $Ctx.HoverCx = $false; Update-WidgetState -Ctx $Ctx } catch { } })
  $Ctx.BtnMin.Add_MouseEnter({ try { $Ctx.BtnMin.ForeColor = [System.Drawing.Color]::White } catch { } })
  $Ctx.BtnMin.Add_MouseLeave({ try { $Ctx.BtnMin.ForeColor = $Ctx.Colors.DIM } catch { } })
  $Ctx.BtnX.Add_MouseEnter({ try { $Ctx.BtnX.ForeColor = [System.Drawing.Color]::White } catch { } })
  $Ctx.BtnX.Add_MouseLeave({ try { $Ctx.BtnX.ForeColor = $Ctx.Colors.DIM } catch { } })
  $Ctx.BtnQuit.Add_MouseEnter({ try { $Ctx.BtnQuit.ForeColor = [System.Drawing.Color]::FromArgb(255, 130, 130) } catch { } })
  $Ctx.BtnQuit.Add_MouseLeave({ try { $Ctx.BtnQuit.ForeColor = [System.Drawing.Color]::FromArgb(200, 100, 100) } catch { } })

  # 拖动：按住标题栏移动无边框窗体；松手记住位置。
  $Ctx.Drag = @{ On = $false; X = 0; Y = 0 }
  $moveH = {
    try {
      if ($Ctx.Drag.On) {
        $Ctx.Form.Location = New-Object System.Drawing.Point(
          ([System.Windows.Forms.Cursor]::Position.X - $Ctx.Drag.X),
          ([System.Windows.Forms.Cursor]::Position.Y - $Ctx.Drag.Y))
      }
    } catch { Write-WidgetError 'drag' $_ }
  }
  $downH = {
    try {
      $Ctx.Drag.On = $true
      $Ctx.Drag.X = [System.Windows.Forms.Cursor]::Position.X - $Ctx.Form.Location.X
      $Ctx.Drag.Y = [System.Windows.Forms.Cursor]::Position.Y - $Ctx.Form.Location.Y
    } catch { Write-WidgetError 'dragdown' $_ }
  }
  $upH = { try { $Ctx.Drag.On = $false; Save-WidgetPosition -Ctx $Ctx } catch { Write-WidgetError 'dragup' $_ } }
  foreach ($c in @($Ctx.Bar, $Ctx.Title)) {
    $c.Add_MouseDown($downH); $c.Add_MouseMove($moveH); $c.Add_MouseUp($upH)
  }
}
