#Requires -Version 5.1
<#
  悬浮窗 · 窗口动作与事件接线（由入口脚本 dot-source）。
#>

function Save-WidgetPosition {
  param([hashtable]$ctx)
  try {
    if ($ctx.Form.WindowState -eq [System.Windows.Forms.FormWindowState]::Normal) {
      "$($ctx.Form.Location.X),$($ctx.Form.Location.Y)" | Out-File -FilePath $ctx.PosFile -Encoding UTF8 -Force
    }
  } catch { }
}

function Show-WidgetWindow {
  param([hashtable]$ctx)
  $ctx.Form.WindowState = [System.Windows.Forms.FormWindowState]::Normal
  $ctx.Form.Show()
  $ctx.Form.Activate()
  $ctx.Form.BringToFront()
  $ctx.MiShow.Text = '隐藏悬浮窗'
}

function Hide-WidgetWindow {
  param([hashtable]$ctx)
  Save-WidgetPosition -Ctx $ctx
  $ctx.Form.Hide()
  $ctx.Form.WindowState = [System.Windows.Forms.FormWindowState]::Normal
  $ctx.MiShow.Text = '显示悬浮窗'
}

function Minimize-WidgetWindow {
  [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseApprovedVerbs', '', Justification = 'Minimize 对 WinForms WindowState 语义最准确')]
  param([hashtable]$ctx)
  Hide-WidgetWindow -Ctx $ctx
}

function Switch-WidgetWindow {
  param([hashtable]$ctx)
  $shown = $ctx.Form.Visible -and $ctx.Form.WindowState -ne [System.Windows.Forms.FormWindowState]::Minimized
  if ($shown) { Hide-WidgetWindow -Ctx $ctx } else { Show-WidgetWindow -Ctx $ctx }
}

function Close-Widget {
  param([hashtable]$ctx)
  $ctx.AllowExit = $true
  try { (Get-Date -Format o) | Out-File -FilePath $ctx.ExitMarker -Encoding utf8 -Force } catch { }
  Save-WidgetPosition -Ctx $ctx
  try { 'user-exit ' + (Get-Date -Format o) | Out-File -FilePath $ctx.AliveFile -Encoding utf8 -Force } catch { }
  $ctx.Notify.Visible = $false
  $ctx.Notify.Dispose()
  if ($ctx.AppContext) {
    try { $ctx.AppContext.ExitThread() } catch { }
  }
  $ctx.Form.Close()
}

function Start-TestPush {
  param([hashtable]$ctx)
  try {
    $script = Join-Path $ctx.ScriptDir 'notify-ai.ps1'
    if (Test-Path $script) {
      Start-Process 'powershell.exe' -ArgumentList @(
        '-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass',
        '-File', $script, '-Title', '【测试】linkWeixin', '-Summary', '来自悬浮窗的测试推送，通道与渲染工作正常。', '-NoStdin'
      ) -WindowStyle Hidden
      [System.Windows.Forms.MessageBox]::Show('已触发测试推送，请留意手机通知。', 'linkWeixin') | Out-Null
    }
  } catch { Write-WidgetError 'testpush' $_ }
}

function Copy-ShareCard {
  try {
    $shareText = "🤖 linkWeixin v0.3.0 - 多 Agent AI 任务推送助手`r`n支持 OpenCode / Codex / Antigravity 任务完成后自动推送微信、企微、飞书、钉钉！`r`n✨ 统一排版渲染 · 零闪烁无窗口 · 毫秒自愈 · 桌面悬浮窗`r`n开源地址：https://github.com/srafyhucl-cpu/linkWeixin"
    [System.Windows.Forms.Clipboard]::SetText($shareText)
    [System.Windows.Forms.MessageBox]::Show("推荐名片文案已复制到剪贴板，可直接粘贴分享到微信/技术群！`r`n`r`n感谢对 linkWeixin 的支持！", 'linkWeixin 分享') | Out-Null
  } catch { }
}

function Register-WidgetEvents {
  param([hashtable]$ctx)

  $ctx.Form.Add_Resize({
    try {
      if ($ctx.Form.WindowState -eq [System.Windows.Forms.FormWindowState]::Minimized) {
        Hide-WidgetWindow -Ctx $ctx
      }
    } catch { }
  })

  $ctx.BtnMin.Add_Click({ try { Minimize-WidgetWindow -Ctx $ctx } catch { Write-WidgetError 'min' $_ } })
  $ctx.BtnX.Add_Click({ try { Hide-WidgetWindow -Ctx $ctx } catch { Write-WidgetError 'x' $_ } })
  $ctx.BtnQuit.Add_Click({ try { Close-Widget -Ctx $ctx } catch { Write-WidgetError 'quit' $_ } })

  $ctx.BtnHistory.Add_Click({ try { Show-HistoryDialog -Ctx $ctx } catch { Write-WidgetError 'btnHist' $_ } })
  $ctx.BtnSettings.Add_Click({ try { Show-SettingsDialog -Ctx $ctx } catch { Write-WidgetError 'btnSet' $_ } })
  $ctx.BtnTestPush.Add_Click({ try { Start-TestPush -Ctx $ctx } catch { Write-WidgetError 'btnTest' $_ } })
  $ctx.LnkHistory.Add_Click({ try { Show-HistoryDialog -Ctx $ctx } catch { Write-WidgetError 'lnkHist' $_ } })

  $ctx.Hint.Add_Click({
    try {
      if ($ctx.TaskVer -eq '旧版') {
        $res = Repair-WatchTaskHidden
        if ($res.Success) {
          [System.Windows.Forms.MessageBox]::Show($res.Message, 'linkWeixin 自愈成功') | Out-Null
          $ctx.TaskVer = '新版'
          Update-WidgetState -Ctx $ctx
        } else {
          [System.Windows.Forms.MessageBox]::Show($res.Message, 'linkWeixin 提示') | Out-Null
        }
      }
    } catch { Write-WidgetError 'hintClick' $_ }
  })

  $ctx.BtnOc.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $ctx.MarkerPath)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $ctx.MarkerPath -Mode $mode)
        Update-WidgetState -Ctx $ctx
      } catch { Write-WidgetError 'btnOc' $_ }
    })
  $ctx.BtnCx.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $ctx.CodexMarker)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $ctx.CodexMarker -Mode $mode)
        Update-WidgetState -Ctx $ctx
      } catch { Write-WidgetError 'btnCx' $_ }
    })
  $ctx.BtnAg.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $ctx.AntigravityMarker)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $ctx.AntigravityMarker -Mode $mode)
        Update-WidgetState -Ctx $ctx
      } catch { Write-WidgetError 'btnAg' $_ }
    })

  $ctx.MiShow.Add_Click({ try { Switch-WidgetWindow -Ctx $ctx } catch { Write-WidgetError 'miShow' $_ } })
  $ctx.MiOc.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $ctx.MarkerPath)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $ctx.MarkerPath -Mode $mode)
        Update-WidgetState -Ctx $ctx
      } catch { Write-WidgetError 'miOc' $_ }
    })
  $ctx.MiCx.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $ctx.CodexMarker)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $ctx.CodexMarker -Mode $mode)
        Update-WidgetState -Ctx $ctx
      } catch { Write-WidgetError 'miCx' $_ }
    })
  $ctx.MiAg.Add_Click({
      try {
        $on = -not (Test-NotifyMarker -Path $ctx.AntigravityMarker)
        $mode = if ($on) { 'Off' } else { 'On' }
        [void](Set-NotifyMarker -Path $ctx.AntigravityMarker -Mode $mode)
        Update-WidgetState -Ctx $ctx
      } catch { Write-WidgetError 'miAg' $_ }
    })

  $ctx.MiHistory.Add_Click({ try { Show-HistoryDialog -Ctx $ctx } catch { Write-WidgetError 'miHist' $_ } })
  $ctx.MiSettings.Add_Click({ try { Show-SettingsDialog -Ctx $ctx } catch { Write-WidgetError 'miSet' $_ } })
  $ctx.MiTest.Add_Click({ try { Start-TestPush -Ctx $ctx } catch { Write-WidgetError 'miTest' $_ } })
  $ctx.MiShare.Add_Click({ try { Copy-ShareCard } catch { } })
  $ctx.MiExit.Add_Click({ try { Close-Widget -Ctx $ctx } catch { Write-WidgetError 'miExit' $_ } })
  $ctx.Notify.Add_DoubleClick({ try { Show-WidgetWindow -Ctx $ctx } catch { Write-WidgetError 'dblclick' $_ } })

  $ctx.Menu.Add_Opening({
      try {
        $shown = $ctx.Form.Visible -and $ctx.Form.WindowState -ne [System.Windows.Forms.FormWindowState]::Minimized
        $ctx.MiShow.Text = if ($shown) { '隐藏悬浮窗' } else { '显示悬浮窗' }
        $onOc = -not (Test-NotifyMarker -Path $ctx.MarkerPath)
        $onCx = -not (Test-NotifyMarker -Path $ctx.CodexMarker)
        $onAg = -not (Test-NotifyMarker -Path $ctx.AntigravityMarker)
        $ctx.MiOc.Text = if ($onOc) { '关闭 opencode 推送' } else { '开启 opencode 推送' }
        $ctx.MiCx.Text = if ($onCx) { '关闭 codex 推送' } else { '开启 codex 推送' }
        $ctx.MiAg.Text = if ($onAg) { '关闭 antigravity 推送' } else { '开启 antigravity 推送' }
      } catch { Write-WidgetError 'opening' $_ }
    })
  $ctx.Form.Add_FormClosing({
      param($s, $e)
      $null = $s
      try {
        if (-not $ctx.AllowExit) { $e.Cancel = $true; Hide-WidgetWindow -Ctx $ctx }
      } catch { Write-WidgetError 'closing' $_ }
    })

  $ctx.BtnOc.Add_MouseEnter({ try { $ctx.HoverOc = $true; $ctx.BtnOc.BackColor = Get-LighterColor $ctx.BtnOc.BackColor } catch { } })
  $ctx.BtnOc.Add_MouseLeave({ try { $ctx.HoverOc = $false; Update-WidgetState -Ctx $ctx } catch { } })
  $ctx.BtnCx.Add_MouseEnter({ try { $ctx.HoverCx = $true; $ctx.BtnCx.BackColor = Get-LighterColor $ctx.BtnCx.BackColor } catch { } })
  $ctx.BtnCx.Add_MouseLeave({ try { $ctx.HoverCx = $false; Update-WidgetState -Ctx $ctx } catch { } })
  $ctx.BtnAg.Add_MouseEnter({ try { $ctx.HoverAg = $true; $ctx.BtnAg.BackColor = Get-LighterColor $ctx.BtnAg.BackColor } catch { } })
  $ctx.BtnAg.Add_MouseLeave({ try { $ctx.HoverAg = $false; Update-WidgetState -Ctx $ctx } catch { } })
  $ctx.BtnMin.Add_MouseEnter({ try { $ctx.BtnMin.ForeColor = [System.Drawing.Color]::White } catch { } })
  $ctx.BtnMin.Add_MouseLeave({ try { $ctx.BtnMin.ForeColor = $ctx.Colors.DIM } catch { } })
  $ctx.BtnX.Add_MouseEnter({ try { $ctx.BtnX.ForeColor = [System.Drawing.Color]::White } catch { } })
  $ctx.BtnX.Add_MouseLeave({ try { $ctx.BtnX.ForeColor = $ctx.Colors.DIM } catch { } })
  $ctx.LnkHistory.Add_MouseEnter({ try { $ctx.LnkHistory.ForeColor = [System.Drawing.Color]::White } catch { } })
  $ctx.LnkHistory.Add_MouseLeave({ try { $ctx.LnkHistory.ForeColor = [System.Drawing.Color]::FromArgb(96, 165, 250) } catch { } })
  $ctx.BtnQuit.Add_MouseEnter({ try { $ctx.BtnQuit.BackColor = [System.Drawing.Color]::FromArgb(60, 30, 35) } catch { } })
  $ctx.BtnQuit.Add_MouseLeave({ try { $ctx.BtnQuit.BackColor = [System.Drawing.Color]::FromArgb(45, 30, 32) } catch { } })

  $ctx.Drag = @{ On = $false; X = 0; Y = 0 }
  $moveH = {
    try {
      if ($ctx.Drag.On) {
        $ctx.Form.Location = New-Object System.Drawing.Point(
          ([System.Windows.Forms.Cursor]::Position.X - $ctx.Drag.X),
          ([System.Windows.Forms.Cursor]::Position.Y - $ctx.Drag.Y))
      }
    } catch { Write-WidgetError 'drag' $_ }
  }
  $downH = {
    try {
      $ctx.Drag.On = $true
      $ctx.Drag.X = [System.Windows.Forms.Cursor]::Position.X - $ctx.Form.Location.X
      $ctx.Drag.Y = [System.Windows.Forms.Cursor]::Position.Y - $ctx.Form.Location.Y
    } catch { Write-WidgetError 'dragdown' $_ }
  }
  $upH = { try { $ctx.Drag.On = $false; Save-WidgetPosition -Ctx $ctx } catch { Write-WidgetError 'dragup' $_ } }
  foreach ($c in @($ctx.Bar, $ctx.Title)) {
    $c.Add_MouseDown($downH); $c.Add_MouseMove($moveH); $c.Add_MouseUp($upH)
  }
}
