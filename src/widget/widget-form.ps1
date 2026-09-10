# 悬浮窗 · 界面构建（由入口脚本 dot-source，函数通过 $Ctx 共享状态）。
# 无边框窗体（Win11 DWM 圆角）、状态色条、双开关按钮、状态卡片、底栏提示、
# 托盘图标与菜单、工具提示、位置记忆、图标失败兜底。

function New-RoundedRegion {
  param([int]$Width, [int]$Height, [int]$Radius)
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $Radius * 2
  $path.AddArc(0, 0, $d, $d, 180, 90)
  $path.AddArc($Width - $d, 0, $d, $d, 270, 90)
  $path.AddArc($Width - $d, $Height - $d, $d, $d, 0, 90)
  $path.AddArc(0, $Height - $d, $d, $d, 90, 90)
  $path.CloseFigure()
  $region = New-Object System.Drawing.Region($path)
  $path.Dispose()
  return $region
}

function Get-LighterColor {
  param([System.Drawing.Color]$Color)
  return [System.Drawing.Color]::FromArgb(
    [Math]::Min(255, $Color.R + 30),
    [Math]::Min(255, $Color.G + 30),
    [Math]::Min(255, $Color.B + 30))
}

function New-DotIcon {
  param([hashtable]$Ctx, [System.Drawing.Color]$Color)
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
    [void]$Ctx.IconBmps.Add($bmp)
    return $ico
  } catch {
    Write-WidgetError 'icon' $_
    return $null
  }
}

function Add-WidgetRow {
  param([hashtable]$Ctx, [System.Windows.Forms.Panel]$Card, [int]$Y, [string]$Name)
  $dot = New-Object System.Windows.Forms.Label
  $dot.Text = '●'
  $dot.Font = $Ctx.Fonts.Bold
  $dot.Location = New-Object System.Drawing.Point(12, $Y)
  $dot.Size = New-Object System.Drawing.Size(20, 28)
  $dot.TextAlign = 'MiddleCenter'
  $Card.Controls.Add($dot)
  $txt = New-Object System.Windows.Forms.Label
  $txt.Font = $Ctx.Fonts.Base
  $txt.ForeColor = $Ctx.Colors.FG
  $txt.BackColor = $Ctx.Colors.CardBG
  $txt.Location = New-Object System.Drawing.Point(34, $Y)
  $txt.Size = New-Object System.Drawing.Size(210, 28)
  $txt.TextAlign = 'MiddleLeft'
  $Card.Controls.Add($txt)
  return @{ Dot = $dot; Txt = $txt; Name = $Name }
}

function New-WidgetForm {
  param([hashtable]$Ctx)
  $colors = $Ctx.Colors
  $fonts = $Ctx.Fonts

  $form = New-Object System.Windows.Forms.Form
  $form.Text = 'linkWeixin'
  $form.Size = New-Object System.Drawing.Size(288, 352)
  $form.FormBorderStyle = 'None'
  $form.TopMost = $true
  # 任务栏保留按钮（带状态圆点图标）：窗口最小化后必定找得回。
  $form.ShowInTaskbar = $true
  $form.StartPosition = 'Manual'
  $form.BackColor = $colors.BG
  $form.ForeColor = $colors.FG

  # Win11 圆角（Win10 上调用失败无副作用）：DWMWA_WINDOW_CORNER_PREFERENCE = 33，ROUND = 2。
  try {
    Add-Type -MemberDefinition '[DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(System.IntPtr hwnd, int attr, ref int value, int size);' -Name DwmCorner -Namespace LinkWeixin -ErrorAction Stop
    $pref = 2
    [void][LinkWeixin.DwmCorner]::DwmSetWindowAttribute($form.Handle, 33, [ref]$pref, 4)
  } catch { }

  # 位置记忆：优先恢复上次位置（需仍在虚拟屏幕内），否则右下角默认位。
  $restored = $false
  try {
    if ($Ctx.PosFile -and (Test-Path $Ctx.PosFile)) {
      $xy = (Get-Content $Ctx.PosFile -Raw -Encoding UTF8).Trim() -split ','
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
      $form.Location = New-Object System.Drawing.Point(($wa.Right - 308), ($wa.Bottom - 372))
    } catch { }
  }

  # 顶部状态色条
  $strip = New-Object System.Windows.Forms.Panel
  $strip.Dock = 'Top'
  $strip.Height = 5
  $form.Controls.Add($strip)

  # 标题栏
  $bar = New-Object System.Windows.Forms.Panel
  $bar.Dock = 'Top'
  $bar.Height = 34
  $bar.BackColor = $colors.BG
  $form.Controls.Add($bar)

  # 标题栏下细分隔线（层次感）
  $sep = New-Object System.Windows.Forms.Panel
  $sep.Location = New-Object System.Drawing.Point(0, 39)
  $sep.Size = New-Object System.Drawing.Size(288, 1)
  $sep.BackColor = [System.Drawing.Color]::FromArgb(48, 48, 54)
  $form.Controls.Add($sep)

  $title = New-Object System.Windows.Forms.Label
  $title.Text = '  linkWeixin 推送'
  $title.Font = $fonts.Bold
  $title.ForeColor = $colors.FG
  $title.BackColor = $colors.BG
  $title.Location = New-Object System.Drawing.Point(4, 0)
  $title.Size = New-Object System.Drawing.Size(200, 34)
  $title.TextAlign = 'MiddleLeft'
  $bar.Controls.Add($title)

  $btnMin = New-Object System.Windows.Forms.Label
  $btnMin.Text = '—'
  $btnMin.Font = $fonts.Base
  $btnMin.ForeColor = $colors.DIM
  $btnMin.Size = New-Object System.Drawing.Size(36, 34)
  $btnMin.Location = New-Object System.Drawing.Point(216, 0)
  $btnMin.TextAlign = 'MiddleCenter'
  $btnMin.Cursor = 'Hand'
  $bar.Controls.Add($btnMin)

  $btnX = New-Object System.Windows.Forms.Label
  $btnX.Text = '✕'
  $btnX.Font = $fonts.Base
  $btnX.ForeColor = $colors.DIM
  $btnX.Size = New-Object System.Drawing.Size(36, 34)
  $btnX.Location = New-Object System.Drawing.Point(252, 0)
  $btnX.TextAlign = 'MiddleCenter'
  $btnX.Cursor = 'Hand'
  $bar.Controls.Add($btnX)

  # 两个独立大开关：opencode / codex 各管一边，各翻各的 marker
  $btnOc = New-Object System.Windows.Forms.Button
  $btnOc.Location = New-Object System.Drawing.Point(16, 54)
  $btnOc.Size = New-Object System.Drawing.Size(124, 62)
  $btnOc.Font = $fonts.Mid
  $btnOc.FlatStyle = 'Flat'
  $btnOc.FlatAppearance.BorderSize = 0
  $btnOc.Cursor = 'Hand'
  $btnOc.Region = New-RoundedRegion -Width 124 -Height 62 -Radius 9
  $form.Controls.Add($btnOc)

  $btnCx = New-Object System.Windows.Forms.Button
  $btnCx.Location = New-Object System.Drawing.Point(148, 54)
  $btnCx.Size = New-Object System.Drawing.Size(124, 62)
  $btnCx.Font = $fonts.Mid
  $btnCx.FlatStyle = 'Flat'
  $btnCx.FlatAppearance.BorderSize = 0
  $btnCx.Cursor = 'Hand'
  $btnCx.Region = New-RoundedRegion -Width 124 -Height 62 -Radius 9
  $form.Controls.Add($btnCx)

  # 状态卡片（圆角）
  $card = New-Object System.Windows.Forms.Panel
  $card.Location = New-Object System.Drawing.Point(16, 128)
  $card.Size = New-Object System.Drawing.Size(256, 128)
  $card.BackColor = $colors.CardBG
  $card.Region = New-RoundedRegion -Width 256 -Height 128 -Radius 9
  $form.Controls.Add($card)

  $rowOc = Add-WidgetRow -Ctx $Ctx -Card $card -Y 8 -Name 'opencode'
  $rowCx = Add-WidgetRow -Ctx $Ctx -Card $card -Y 46 -Name 'codex'
  $rowLast = Add-WidgetRow -Ctx $Ctx -Card $card -Y 84 -Name 'last'
  $rowLast.Dot.Text = '•'
  $rowLast.Dot.ForeColor = $colors.DIM

  # 底栏提示
  $hint = New-Object System.Windows.Forms.Label
  $hint.Location = New-Object System.Drawing.Point(16, 262)
  $hint.Size = New-Object System.Drawing.Size(256, 44)
  $hint.Font = $fonts.Hint
  $hint.ForeColor = $colors.DIM
  $form.Controls.Add($hint)

  $foot = New-Object System.Windows.Forms.Label
  $foot.Location = New-Object System.Drawing.Point(16, 304)
  $foot.Size = New-Object System.Drawing.Size(208, 20)
  $foot.Font = $fonts.Foot
  $foot.ForeColor = [System.Drawing.Color]::FromArgb(110, 110, 115)
  $foot.Text = if ($Ctx.AppVersion) { "v$($Ctx.AppVersion) · × 藏到托盘 · 双击恢复" } else { '× 藏到托盘 · 双击托盘图标恢复' }
  $form.Controls.Add($foot)

  $btnQuit = New-Object System.Windows.Forms.Label
  $btnQuit.Text = '退出'
  $btnQuit.Font = $fonts.Foot
  $btnQuit.ForeColor = [System.Drawing.Color]::FromArgb(200, 100, 100)
  $btnQuit.BackColor = $colors.BG
  $btnQuit.Size = New-Object System.Drawing.Size(40, 20)
  $btnQuit.Location = New-Object System.Drawing.Point(232, 304)
  $btnQuit.TextAlign = 'MiddleCenter'
  $btnQuit.Cursor = 'Hand'
  $form.Controls.Add($btnQuit)

  # 托盘
  $iconOn = New-DotIcon -Ctx $Ctx -Color $colors.DotOn
  $iconMid = New-DotIcon -Ctx $Ctx -Color ([System.Drawing.Color]::FromArgb(255, 170, 60))
  $iconOff = New-DotIcon -Ctx $Ctx -Color $colors.RED
  # 兜底：自绘图标任一失败就用系统默认图标，保证任务栏/托盘一定有东西（丑但可见）。
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
  $miTest = $menu.Items.Add('测试推送')
  [void]$menu.Items.Add('-')
  $miExit = $menu.Items.Add('退出')
  $notify.ContextMenuStrip = $menu
  # 窗体图标也用状态圆点：任务栏按钮显示它，不再是 powershell 默认图标。
  $form.Icon = $iconOn

  # 工具提示
  $tip = New-Object System.Windows.Forms.ToolTip
  $tip.SetToolTip($btnMin, '最小化到任务栏')
  $tip.SetToolTip($btnX, '藏到托盘（双击托盘图标或桌面快捷方式恢复）')
  $tip.SetToolTip($btnOc, 'opencode 推送开关（点击切换）')
  $tip.SetToolTip($btnCx, 'codex 推送开关（点击切换）')
  $tip.SetToolTip($btnQuit, '彻底退出（桌面快捷方式可重开）')

  $Ctx.Form = $form
  $Ctx.Strip = $strip
  $Ctx.Bar = $bar
  $Ctx.Sep = $sep
  $Ctx.Title = $title
  $Ctx.BtnMin = $btnMin
  $Ctx.BtnX = $btnX
  $Ctx.BtnOc = $btnOc
  $Ctx.BtnCx = $btnCx
  $Ctx.Card = $card
  $Ctx.RowOc = $rowOc
  $Ctx.RowCx = $rowCx
  $Ctx.RowLast = $rowLast
  $Ctx.Hint = $hint
  $Ctx.Foot = $foot
  $Ctx.BtnQuit = $btnQuit
  $Ctx.Notify = $notify
  $Ctx.Menu = $menu
  $Ctx.MiShow = $miShow
  $Ctx.MiOc = $miOc
  $Ctx.MiCx = $miCx
  $Ctx.MiTest = $miTest
  $Ctx.MiExit = $miExit
  $Ctx.Tip = $tip
  $Ctx.IconOn = $iconOn
  $Ctx.IconMid = $iconMid
  $Ctx.IconOff = $iconOff
}
