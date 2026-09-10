# 悬浮窗 · 窗口动作与事件接线（由入口脚本 dot-source）。
# 隐藏/显示/彻底退出、按钮与托盘菜单事件、无边框拖动、关闭拦截。

function Show-WidgetWindow {
  param([hashtable]$Ctx)
  $Ctx.Form.WindowState = 'Normal'
  $Ctx.Form.Show()
  $Ctx.Form.Activate()
  $Ctx.MiShow.Text = '隐藏悬浮窗'
}

function Hide-WidgetWindow {
  param([hashtable]$Ctx)
  # 藏到托盘：任务栏无按钮，靠托盘图标 / 桌面快捷方式（单实例接管）回来。
  $Ctx.Form.Hide()
  $Ctx.MiShow.Text = '显示悬浮窗'
}

function Close-Widget {
  param([hashtable]$Ctx)
  $Ctx.AllowExit = $true
  try { 'user-exit ' + (Get-Date -Format o) | Out-File -FilePath $Ctx.AliveFile -Encoding utf8 -Force } catch { }
  $Ctx.Notify.Visible = $false
  $Ctx.Notify.Dispose()
  $Ctx.Form.Close()
}

function Register-WidgetEvents {
  param([hashtable]$Ctx)

  $Ctx.BtnMin.Add_Click({ try { Hide-WidgetWindow -Ctx $Ctx } catch { Write-WidgetError 'min' $_ } })
  $Ctx.BtnX.Add_Click({ try { Hide-WidgetWindow -Ctx $Ctx } catch { Write-WidgetError 'x' $_ } })
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
  $Ctx.MiShow.Add_Click({
      try {
        if ($Ctx.Form.Visible) { Hide-WidgetWindow -Ctx $Ctx } else { Show-WidgetWindow -Ctx $Ctx }
      } catch { Write-WidgetError 'miShow' $_ }
    })
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
  $Ctx.MiExit.Add_Click({ try { Close-Widget -Ctx $Ctx } catch { Write-WidgetError 'miExit' $_ } })
  $Ctx.Notify.Add_DoubleClick({
      try {
        if ($Ctx.Form.Visible) { Hide-WidgetWindow -Ctx $Ctx } else { Show-WidgetWindow -Ctx $Ctx }
      } catch { Write-WidgetError 'dblclick' $_ }
    })
  $Ctx.Menu.Add_Opening({
      try {
        $Ctx.MiShow.Text = if ($Ctx.Form.Visible) { '隐藏悬浮窗' } else { '显示悬浮窗' }
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

  # 拖动：按住标题栏移动无边框窗体
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
  $upH = { try { $Ctx.Drag.On = $false } catch { Write-WidgetError 'dragup' $_ } }
  foreach ($c in @($Ctx.Bar, $Ctx.Title)) {
    $c.Add_MouseDown($downH); $c.Add_MouseMove($moveH); $c.Add_MouseUp($upH)
  }
}
