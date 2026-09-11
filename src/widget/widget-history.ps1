#Requires -Version 5.1
<#
  悬浮窗 · 推送历史与详情查看器（由入口脚本 dot-source）。
#>

function Show-HistoryDialog {
  param([hashtable]$ctx)

  try {
    $histForm = New-Object System.Windows.Forms.Form
    $histForm.Text = 'linkWeixin - 推送历史'
    $histForm.Size = New-Object System.Drawing.Size(560, 440)
    $histForm.StartPosition = 'CenterScreen'
    $histForm.BackColor = $ctx.Colors.BG
    $histForm.ForeColor = $ctx.Colors.FG
    $histForm.FormBorderStyle = 'FixedDialog'
    $histForm.MaximizeBox = $false
    $histForm.MinimizeBox = $false
    $histForm.ShowInTaskbar = $false
    $histForm.TopMost = $true

    $lblHeader = New-Object System.Windows.Forms.Label
    $lblHeader.Text = '最近推送记录（点击行查看完整摘要）：'
    $lblHeader.Location = New-Object System.Drawing.Point(16, 12)
    $lblHeader.Size = New-Object System.Drawing.Size(350, 24)
    $lblHeader.Font = $ctx.Fonts.Bold
    $lblHeader.ForeColor = $ctx.Colors.FG
    $histForm.Controls.Add($lblHeader)

    $btnRefresh = New-Object System.Windows.Forms.Button
    $btnRefresh.Text = '刷新'
    $btnRefresh.Location = New-Object System.Drawing.Point(448, 8)
    $btnRefresh.Size = New-Object System.Drawing.Size(80, 26)
    $btnRefresh.FlatStyle = 'Flat'
    $btnRefresh.BackColor = $ctx.Colors.CardBG
    $btnRefresh.ForeColor = $ctx.Colors.FG
    $btnRefresh.Cursor = [System.Windows.Forms.Cursors]::Hand
    $histForm.Controls.Add($btnRefresh)

    $grid = New-Object System.Windows.Forms.DataGridView
    $grid.Location = New-Object System.Drawing.Point(16, 42)
    $grid.Size = New-Object System.Drawing.Size(512, 190)
    $grid.BackgroundColor = $ctx.Colors.CardBG
    $grid.ForeColor = $ctx.Colors.FG
    $grid.DefaultCellStyle.BackColor = $ctx.Colors.CardBG
    $grid.DefaultCellStyle.ForeColor = $ctx.Colors.FG
    $grid.DefaultCellStyle.SelectionBackColor = [System.Drawing.Color]::FromArgb(40, 80, 140)
    $grid.DefaultCellStyle.SelectionForeColor = [System.Drawing.Color]::White
    $grid.RowHeadersVisible = $false
    $grid.SelectionMode = 'FullRowSelect'
    $grid.MultiSelect = $false
    $grid.ReadOnly = $true
    $grid.AllowUserToAddRows = $false
    $grid.AllowUserToDeleteRows = $false
    $grid.BorderStyle = 'None'

    [void]$grid.Columns.Add('time', '时间')
    $grid.Columns['time'].Width = 140
    [void]$grid.Columns.Add('title', '标题')
    $grid.Columns['title'].Width = 170
    [void]$grid.Columns.Add('channel', '通道')
    $grid.Columns['channel'].Width = 100
    [void]$grid.Columns.Add('status', '状态')
    $grid.Columns['status'].Width = 80
    $histForm.Controls.Add($grid)

    $lblDetail = New-Object System.Windows.Forms.Label
    $lblDetail.Text = '摘要详情：'
    $lblDetail.Location = New-Object System.Drawing.Point(16, 238)
    $lblDetail.Size = New-Object System.Drawing.Size(120, 20)
    $lblDetail.Font = $ctx.Fonts.Bold
    $lblDetail.ForeColor = $ctx.Colors.DIM
    $histForm.Controls.Add($lblDetail)

    $txtDetail = New-Object System.Windows.Forms.TextBox
    $txtDetail.Location = New-Object System.Drawing.Point(16, 260)
    $txtDetail.Size = New-Object System.Drawing.Size(512, 90)
    $txtDetail.Multiline = $true
    $txtDetail.ReadOnly = $true
    $txtDetail.ScrollBars = 'Vertical'
    $txtDetail.BackColor = $ctx.Colors.CardBG
    $txtDetail.ForeColor = $ctx.Colors.FG
    $txtDetail.BorderStyle = 'FixedSingle'
    $histForm.Controls.Add($txtDetail)

    $btnCopy = New-Object System.Windows.Forms.Button
    $btnCopy.Text = '复制摘要'
    $btnCopy.Location = New-Object System.Drawing.Point(16, 360)
    $btnCopy.Size = New-Object System.Drawing.Size(90, 28)
    $btnCopy.FlatStyle = 'Flat'
    $btnCopy.BackColor = $ctx.Colors.CardBG
    $btnCopy.ForeColor = $ctx.Colors.FG
    $btnCopy.Cursor = [System.Windows.Forms.Cursors]::Hand
    $histForm.Controls.Add($btnCopy)

    $btnClear = New-Object System.Windows.Forms.Button
    $btnClear.Text = '清空日志'
    $btnClear.Location = New-Object System.Drawing.Point(116, 360)
    $btnClear.Size = New-Object System.Drawing.Size(90, 28)
    $btnClear.FlatStyle = 'Flat'
    $btnClear.BackColor = $ctx.Colors.CardBG
    $btnClear.ForeColor = [System.Drawing.Color]::FromArgb(220, 100, 100)
    $btnClear.Cursor = [System.Windows.Forms.Cursors]::Hand
    $histForm.Controls.Add($btnClear)

    $btnClose = New-Object System.Windows.Forms.Button
    $btnClose.Text = '关闭'
    $btnClose.Location = New-Object System.Drawing.Point(438, 360)
    $btnClose.Size = New-Object System.Drawing.Size(90, 28)
    $btnClose.FlatStyle = 'Flat'
    $btnClose.BackColor = $ctx.Colors.GREEN
    $btnClose.ForeColor = [System.Drawing.Color]::White
    $btnClose.Cursor = [System.Windows.Forms.Cursors]::Hand
    $histForm.Controls.Add($btnClose)

    $historyData = New-Object System.Collections.ArrayList

    $loadData = {
      $grid.Rows.Clear()
      $historyData.Clear()
      $records = @(Get-LinkWeixinHistory -Limit 100)
      foreach ($r in $records) {
        [void]$historyData.Add($r)
        [void]$grid.Rows.Add($r.Time, $r.Title, $r.Channels, $r.Status)
      }
      if ($grid.Rows.Count -gt 0) {
        $grid.Rows[0].Selected = $true
        $txtDetail.Text = $records[0].Summary
      } else {
        $txtDetail.Text = '暂无推送历史记录。'
      }
    }

    $grid.Add_SelectionChanged({
      if ($grid.SelectedRows.Count -gt 0) {
        $idx = $grid.SelectedRows[0].Index
        if ($idx -ge 0 -and $idx -lt $historyData.Count) {
          $item = $historyData[$idx]
          $txtDetail.Text = "【$($item.Title)】`r`n时间：$($item.Time) ($($item.Channels) - $($item.Status))`r`n----------------------------------------`r`n$($item.Summary)"
        }
      }
    })

    $btnRefresh.Add_Click({ & $loadData })
    $btnCopy.Add_Click({
      if (-not [string]::IsNullOrWhiteSpace($txtDetail.Text)) {
        [System.Windows.Forms.Clipboard]::SetText($txtDetail.Text)
        [System.Windows.Forms.MessageBox]::Show('已复制到剪贴板。', 'linkWeixin') | Out-Null
      }
    })
    $btnClear.Add_Click({
      $choice = [System.Windows.Forms.MessageBox]::Show('确定清空所有推送历史日志吗？', 'linkWeixin', [System.Windows.Forms.MessageBoxButtons]::YesNo, [System.Windows.Forms.MessageBoxIcon]::Question)
      if ($choice -eq [System.Windows.Forms.DialogResult]::Yes) {
        $paths = Get-LinkWeixinPaths
        if (Test-Path $paths.PushLog) {
          Remove-Item $paths.PushLog -Force -ErrorAction SilentlyContinue
        }
        & $loadData
      }
    })
    $btnClose.Add_Click({ $histForm.Close() })

    & $loadData
    [void]$histForm.ShowDialog($ctx.Form)
  } catch {
    Write-WidgetError 'history-dialog' $_
  }
}
