#Requires -Version 5.1
<#
  悬浮窗 · 界面构建（由入口脚本 dot-source）。
  无边框窗体、现代暗色 Fluent 质感、平滑圆角、状态卡片、底栏工具栏与托盘。
#>

function New-RoundedRegion {
  param([int]$width, [int]$height, [int]$radius)
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $radius * 2
  $path.AddArc(0, 0, $d, $d, 180, 90)
  $path.AddArc($width - $d, 0, $d, $d, 270, 90)
  $path.AddArc($width - $d, $height - $d, $d, $d, 0, 90)
  $path.AddArc(0, $height - $d, $d, $d, 90, 90)
  $path.CloseFigure()
  $region = New-Object System.Drawing.Region($path)
  $path.Dispose()
  return $region
}

function Get-LighterColor {
  param([System.Drawing.Color]$color)
  return [System.Drawing.Color]::FromArgb(
    [Math]::Min(255, $color.R + 25),
    [Math]::Min(255, $color.G + 25),
    [Math]::Min(255, $color.B + 25))
}

function New-DotIcon {
  param([hashtable]$ctx, [System.Drawing.Color]$color)
  try {
    $ErrorActionPreference = 'Stop'
    $bmp = New-Object System.Drawing.Bitmap(16, 16)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)
    $b = New-Object System.Drawing.SolidBrush($color)
    $g.FillEllipse($b, 1, 1, 14, 14)
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, 1)
    $g.DrawEllipse($pen, 1, 1, 14, 14)
    $b.Dispose(); $pen.Dispose(); $g.Dispose()
    $ico = [System.Drawing.Icon]::FromHandle($bmp.GetHicon())
    [void]$ctx.IconBmps.Add($bmp)
    return $ico
  } catch {
    Write-WidgetError 'icon' $_
    return $null
  }
}

function Add-WidgetRow {
  param([hashtable]$ctx, [System.Windows.Forms.Panel]$card, [int]$y, [string]$name)
  $dot = New-Object System.Windows.Forms.Label
  $dot.Text = '●'
  $dot.Font = $ctx.Fonts.Bold
  $dot.Location = New-Object System.Drawing.Point(14, $y)
  $dot.Size = New-Object System.Drawing.Size(22, 30)
  $dot.TextAlign = 'MiddleCenter'
  $card.Controls.Add($dot)

  $txt = New-Object System.Windows.Forms.Label
  $txt.Font = $ctx.Fonts.Base
  $txt.ForeColor = $ctx.Colors.FG
  $txt.BackColor = $ctx.Colors.CardBG
  $txt.Location = New-Object System.Drawing.Point(40, $y)
  $txt.Size = New-Object System.Drawing.Size(280, 30)
  $txt.TextAlign = 'MiddleLeft'
  $card.Controls.Add($txt)
  return @{ Dot = $dot; Txt = $txt; Name = $name }
}

function New-WidgetForm {
  param([hashtable]$ctx)
  $colors = $ctx.Colors
  $fonts = $ctx.Fonts

  $form = New-Object System.Windows.Forms.Form
  $form.Text = 'linkWeixin'
  # 宽适现代桌面卡片尺寸：380 x 450，充足留白与呼吸感
  $form.Size = New-Object System.Drawing.Size(380, 450)
  $form.FormBorderStyle = 'None'
  $form.TopMost = $true
  $form.ShowInTaskbar = $true
  $form.MinimizeBox = $true
  $form.StartPosition = 'Manual'
  $form.BackColor = $colors.BG
  $form.ForeColor = $colors.FG

  # Win11 圆角与任务栏最小化样式支持
  try {
    Add-Type -MemberDefinition '[DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(System.IntPtr hwnd, int attr, ref int value, int size);' -Name DwmCorner -Namespace LinkWeixin -ErrorAction Stop
    $pref = 2
    [void][LinkWeixin.DwmCorner]::DwmSetWindowAttribute($form.Handle, 33, [ref]$pref, 4)
  } catch { }
  try {
    $GWL_STYLE = -16
    $WS_MINIMIZEBOX = 0x00020000
    $style = [LinkWeixin.DpiAwareness]::GetWindowLong($form.Handle, $GWL_STYLE)
    [void][LinkWeixin.DpiAwareness]::SetWindowLong($form.Handle, $GWL_STYLE, ($style -bor $WS_MINIMIZEBOX))
  } catch { }

  # 位置记忆
  $restored = $false
  try {
    if ($ctx.PosFile -and (Test-Path $ctx.PosFile)) {
      $xy = (Get-Content $ctx.PosFile -Raw -Encoding UTF8).Trim() -split ','
      if ($xy.Count -eq 2) {
        $x = [int]$xy[0]; $y = [int]$xy[1]
        $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
        if ($x -ge $vs.Left -and $x -le ($vs.Right - 100) -and $y -ge $vs.Top -and $y -le ($vs.Bottom - 100)) {
          $form.Location = New-Object System.Drawing.Point($x, $y)
          $restored = $true
        }
      }
    }
  } catch { }
  if (-not $restored) {
    try {
      $wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
      $form.Location = New-Object System.Drawing.Point(($wa.Right - 400), ($wa.Bottom - 460))
    } catch { }
  }

  # 顶部状态色带
  $strip = New-Object System.Windows.Forms.Panel
  $strip.Dock = 'Top'
  $strip.Height = 4
  $form.Controls.Add($strip)

  # 标题栏
  $bar = New-Object System.Windows.Forms.Panel
  $bar.Dock = 'Top'
  $bar.Height = 38
  $bar.BackColor = $colors.BG
  $form.Controls.Add($bar)

  # 标题栏下微弱分隔线
  $sep = New-Object System.Windows.Forms.Panel
  $sep.Location = New-Object System.Drawing.Point(0, 42)
  $sep.Size = New-Object System.Drawing.Size(380, 1)
  $sep.BackColor = [System.Drawing.Color]::FromArgb(40, 40, 46)
  $form.Controls.Add($sep)

  $title = New-Object System.Windows.Forms.Label
  $title.Text = '  linkWeixin 推送'
  $title.Font = $fonts.Bold
  $title.ForeColor = $colors.FG
  $title.BackColor = $colors.BG
  $title.Location = New-Object System.Drawing.Point(8, 0)
  $title.Size = New-Object System.Drawing.Size(260, 38)
  $title.TextAlign = 'MiddleLeft'
  $bar.Controls.Add($title)

  $btnMin = New-Object System.Windows.Forms.Label
  $btnMin.Text = '—'
  $btnMin.Font = $fonts.Base
  $btnMin.ForeColor = $colors.DIM
  $btnMin.Size = New-Object System.Drawing.Size(40, 38)
  $btnMin.Location = New-Object System.Drawing.Point(298, 0)
  $btnMin.TextAlign = 'MiddleCenter'
  $btnMin.Cursor = 'Hand'
  $bar.Controls.Add($btnMin)

  $btnX = New-Object System.Windows.Forms.Label
  $btnX.Text = '✕'
  $btnX.Font = $fonts.Base
  $btnX.ForeColor = $colors.DIM
  $btnX.Size = New-Object System.Drawing.Size(40, 38)
  $btnX.Location = New-Object System.Drawing.Point(338, 0)
  $btnX.TextAlign = 'MiddleCenter'
  $btnX.Cursor = 'Hand'
  $bar.Controls.Add($btnX)

  # 三独立大开关（宽 106px，高 72px，间距 11px，左右边距对称各 20px）
  $btnOc = New-Object System.Windows.Forms.Button
  $btnOc.Location = New-Object System.Drawing.Point(20, 56)
  $btnOc.Size = New-Object System.Drawing.Size(106, 72)
  $btnOc.Font = $fonts.Mid
  $btnOc.FlatStyle = 'Flat'
  $btnOc.FlatAppearance.BorderSize = 0
  $btnOc.TabStop = $false
  $btnOc.Cursor = 'Hand'
  $btnOc.Region = New-RoundedRegion -Width 106 -Height 72 -Radius 10
  $form.Controls.Add($btnOc)

  $btnCx = New-Object System.Windows.Forms.Button
  $btnCx.Location = New-Object System.Drawing.Point(137, 56)
  $btnCx.Size = New-Object System.Drawing.Size(106, 72)
  $btnCx.Font = $fonts.Mid
  $btnCx.FlatStyle = 'Flat'
  $btnCx.FlatAppearance.BorderSize = 0
  $btnCx.TabStop = $false
  $btnCx.Cursor = 'Hand'
  $btnCx.Region = New-RoundedRegion -Width 106 -Height 72 -Radius 10
  $form.Controls.Add($btnCx)

  $btnAg = New-Object System.Windows.Forms.Button
  $btnAg.Location = New-Object System.Drawing.Point(254, 56)
  $btnAg.Size = New-Object System.Drawing.Size(106, 72)
  $btnAg.Font = $fonts.Mid
  $btnAg.FlatStyle = 'Flat'
  $btnAg.FlatAppearance.BorderSize = 0
  $btnAg.TabStop = $false
  $btnAg.Cursor = 'Hand'
  $btnAg.Region = New-RoundedRegion -Width 106 -Height 72 -Radius 10
  $form.Controls.Add($btnAg)

  # 状态监控卡片（四行卡片，尺寸 340 x 148，高度舒适，行间距均匀）
  $card = New-Object System.Windows.Forms.Panel
  $card.Location = New-Object System.Drawing.Point(20, 142)
  $card.Size = New-Object System.Drawing.Size(340, 148)
  $card.BackColor = $colors.CardBG
  $card.Region = New-RoundedRegion -Width 340 -Height 148 -Radius 10
  $form.Controls.Add($card)

  $rowOc = Add-WidgetRow -Ctx $ctx -Card $card -Y 8 -Name 'opencode'
  $rowCx = Add-WidgetRow -Ctx $ctx -Card $card -Y 42 -Name 'codex'
  $rowAg = Add-WidgetRow -Ctx $ctx -Card $card -Y 76 -Name 'antigravity'
  $rowLast = Add-WidgetRow -Ctx $ctx -Card $card -Y 110 -Name 'last'
  $rowLast.Dot.Text = '•'
  $rowLast.Dot.ForeColor = $colors.DIM
  $rowLast.Txt.Size = New-Object System.Drawing.Size(200, 30)

  # 历史详情小链接（彻底消除整行文字截断，给用户明确点击提示）
  $lnkHistory = New-Object System.Windows.Forms.Label
  $lnkHistory.Text = '历史详情 ›'
  $lnkHistory.Font = $fonts.Base
  $lnkHistory.ForeColor = [System.Drawing.Color]::FromArgb(96, 165, 250)
  $lnkHistory.BackColor = $colors.CardBG
  $lnkHistory.Location = New-Object System.Drawing.Point(244, 110)
  $lnkHistory.Size = New-Object System.Drawing.Size(84, 30)
  $lnkHistory.TextAlign = 'MiddleRight'
  $lnkHistory.Cursor = 'Hand'
  $card.Controls.Add($lnkHistory)

  # 提示与自愈条（单行舒展显示，彻底解决单字“条”掉行）
  $hint = New-Object System.Windows.Forms.Label
  $hint.Location = New-Object System.Drawing.Point(20, 298)
  $hint.Size = New-Object System.Drawing.Size(340, 36)
  $hint.Font = $fonts.Hint
  $hint.ForeColor = $colors.DIM
  $hint.TextAlign = 'MiddleLeft'
  $form.Controls.Add($hint)

  # 底部分隔线
  $sepFoot = New-Object System.Windows.Forms.Panel
  $sepFoot.Location = New-Object System.Drawing.Point(20, 348)
  $sepFoot.Size = New-Object System.Drawing.Size(340, 1)
  $sepFoot.BackColor = [System.Drawing.Color]::FromArgb(40, 40, 46)
  $form.Controls.Add($sepFoot)

  # 底部工具栏（充足底部边距，杜绝贴底压迫）
  $foot = New-Object System.Windows.Forms.Label
  $foot.Location = New-Object System.Drawing.Point(20, 362)
  $foot.Size = New-Object System.Drawing.Size(70, 32)
  $foot.Font = $fonts.Foot
  $foot.ForeColor = [System.Drawing.Color]::FromArgb(120, 120, 130)
  $foot.Text = if ($ctx.AppVersion) { "v$($ctx.AppVersion)" } else { "v0.3.0" }
  $foot.TextAlign = 'MiddleLeft'
  $form.Controls.Add($foot)

  # 底部操作按钮（无焦点边框，现代微圆角质感）
  $btnHistory = New-Object System.Windows.Forms.Button
  $btnHistory.Text = '历史'
  $btnHistory.Location = New-Object System.Drawing.Point(96, 362)
  $btnHistory.Size = New-Object System.Drawing.Size(58, 32)
  $btnHistory.Font = $fonts.Foot
  $btnHistory.FlatStyle = 'Flat'
  $btnHistory.FlatAppearance.BorderSize = 0
  $btnHistory.TabStop = $false
  $btnHistory.BackColor = $colors.CardBG
  $btnHistory.ForeColor = $colors.FG
  $btnHistory.Cursor = 'Hand'
  $btnHistory.Region = New-RoundedRegion -Width 58 -Height 32 -Radius 6
  $form.Controls.Add($btnHistory)

  $btnSettings = New-Object System.Windows.Forms.Button
  $btnSettings.Text = '设置'
  $btnSettings.Location = New-Object System.Drawing.Point(162, 362)
  $btnSettings.Size = New-Object System.Drawing.Size(58, 32)
  $btnSettings.Font = $fonts.Foot
  $btnSettings.FlatStyle = 'Flat'
  $btnSettings.FlatAppearance.BorderSize = 0
  $btnSettings.TabStop = $false
  $btnSettings.BackColor = $colors.CardBG
  $btnSettings.ForeColor = $colors.FG
  $btnSettings.Cursor = 'Hand'
  $btnSettings.Region = New-RoundedRegion -Width 58 -Height 32 -Radius 6
  $form.Controls.Add($btnSettings)

  $btnTestPush = New-Object System.Windows.Forms.Button
  $btnTestPush.Text = '测试'
  $btnTestPush.Location = New-Object System.Drawing.Point(228, 362)
  $btnTestPush.Size = New-Object System.Drawing.Size(58, 32)
  $btnTestPush.Font = $fonts.Foot
  $btnTestPush.FlatStyle = 'Flat'
  $btnTestPush.FlatAppearance.BorderSize = 0
  $btnTestPush.TabStop = $false
  $btnTestPush.BackColor = $colors.CardBG
  $btnTestPush.ForeColor = $colors.FG
  $btnTestPush.Cursor = 'Hand'
  $btnTestPush.Region = New-RoundedRegion -Width 58 -Height 32 -Radius 6
  $form.Controls.Add($btnTestPush)

  $btnQuit = New-Object System.Windows.Forms.Button
  $btnQuit.Text = '退出'
  $btnQuit.Location = New-Object System.Drawing.Point(294, 362)
  $btnQuit.Size = New-Object System.Drawing.Size(66, 32)
  $btnQuit.Font = $fonts.Foot
  $btnQuit.FlatStyle = 'Flat'
  $btnQuit.FlatAppearance.BorderSize = 0
  $btnQuit.TabStop = $false
  $btnQuit.BackColor = [System.Drawing.Color]::FromArgb(45, 30, 32)
  $btnQuit.ForeColor = [System.Drawing.Color]::FromArgb(240, 100, 100)
  $btnQuit.Cursor = 'Hand'
  $btnQuit.Region = New-RoundedRegion -Width 66 -Height 32 -Radius 6
  $form.Controls.Add($btnQuit)

  # 托盘
  $iconOn = New-DotIcon -Ctx $ctx -Color $colors.DotOn
  $iconMid = New-DotIcon -Ctx $ctx -Color ([System.Drawing.Color]::FromArgb(255, 170, 60))
  $iconOff = New-DotIcon -Ctx $ctx -Color $colors.RED
  if ($null -eq $iconOn -or $null -eq $iconMid -or $null -eq $iconOff) {
    try {
      $fb = [System.Drawing.SystemIcons]::Application
      if ($null -eq $iconOn) { $iconOn = $fb }
      if ($null -eq $iconMid) { $iconMid = $fb }
      if ($null -eq $iconOff) { $iconOff = $fb }
    } catch { Write-WidgetError 'icon-fb' $_ }
  }
  $notify = New-Object System.Windows.Forms.NotifyIcon
  $notify.Text = 'linkWeixin 推送'
  $notify.Icon = $iconOn
  $notify.Visible = $true
  $menu = New-Object System.Windows.Forms.ContextMenuStrip
  $miShow = $menu.Items.Add('隐藏悬浮窗')
  $miOc = $menu.Items.Add('关闭 opencode 推送')
  $miCx = $menu.Items.Add('关闭 codex 推送')
  $miAg = $menu.Items.Add('关闭 antigravity 推送')
  [void]$menu.Items.Add('-')
  $miHistory = $menu.Items.Add('推送历史记录')
  $miSettings = $menu.Items.Add('通道与偏好设置')
  $miTest = $menu.Items.Add('发送测试推送')
  $miShare = $menu.Items.Add('复制推荐名片 / 分享')
  [void]$menu.Items.Add('-')
  $miExit = $menu.Items.Add('退出')
  $notify.ContextMenuStrip = $menu
  $form.Icon = $iconOn

  # 工具提示
  $tip = New-Object System.Windows.Forms.ToolTip
  $tip.SetToolTip($btnMin, '最小化到任务栏')
  $tip.SetToolTip($btnX, '藏到托盘（双击托盘图标或桌面快捷方式恢复）')
  $tip.SetToolTip($btnOc, 'OpenCode 推送开关（点击切换）')
  $tip.SetToolTip($btnCx, 'Codex 推送开关（点击切换）')
  $tip.SetToolTip($btnAg, 'Antigravity 推送开关（点击切换）')
  $tip.SetToolTip($btnHistory, '查看推送历史日志与摘要详情')
  $tip.SetToolTip($btnSettings, '配置多推送通道与系统自检')
  $tip.SetToolTip($btnTestPush, '立即向已启用的通道发送一条测试消息')
  $tip.SetToolTip($btnQuit, '彻底退出 linkWeixin')
  $tip.SetToolTip($lnkHistory, '点击查看完整推送历史记录')

  $ctx.Form = $form
  $ctx.Strip = $strip
  $ctx.Bar = $bar
  $ctx.Sep = $sep
  $ctx.Title = $title
  $ctx.BtnMin = $btnMin
  $ctx.BtnX = $btnX
  $ctx.BtnOc = $btnOc
  $ctx.BtnCx = $btnCx
  $ctx.BtnAg = $btnAg
  $ctx.Card = $card
  $ctx.RowOc = $rowOc
  $ctx.RowCx = $rowCx
  $ctx.RowAg = $rowAg
  $ctx.RowLast = $rowLast
  $ctx.LnkHistory = $lnkHistory
  $ctx.Hint = $hint
  $ctx.Foot = $foot
  $ctx.BtnHistory = $btnHistory
  $ctx.BtnSettings = $btnSettings
  $ctx.BtnTestPush = $btnTestPush
  $ctx.BtnQuit = $btnQuit
  $ctx.Notify = $notify
  $ctx.Menu = $menu
  $ctx.MiShow = $miShow
  $ctx.MiOc = $miOc
  $ctx.MiCx = $miCx
  $ctx.MiAg = $miAg
  $ctx.MiHistory = $miHistory
  $ctx.MiSettings = $miSettings
  $ctx.MiTest = $miTest
  $ctx.MiShare = $miShare
  $ctx.MiExit = $miExit
  $ctx.Tip = $tip
  $ctx.IconOn = $iconOn
  $ctx.IconMid = $iconMid
  $ctx.IconOff = $iconOff
}
