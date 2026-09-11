#Requires -Version 5.1
<#
  悬浮窗 · 通道配置与系统诊断自愈中心（由入口脚本 dot-source）。
#>

function Repair-WatchTaskHidden {
  [CmdletBinding()]
  param()
  try {
    $upBin = Join-Path $env:USERPROFILE 'bin'
    $watch = Join-Path $upBin 'codex-notify-watch.ps1'
    $launcher = Join-Path $upBin 'run-hidden.vbs'
    $wshExe = Join-Path $env:SystemRoot 'System32\wscript.exe'
    if (-not (Test-Path $wshExe)) { $wshExe = 'wscript.exe' }

    if (-not (Test-Path $watch) -or -not (Test-Path $launcher)) {
      return @{ Success = $false; Message = '未找到 ~/bin 中的看守脚本，请重新运行 install.ps1。' }
    }

    $argStr = '"' + $launcher + '" "' + $watch + '"'
    $taskAction = New-ScheduledTaskAction -Execute $wshExe -Argument $argStr
    $taskT1 = New-ScheduledTaskTrigger -AtLogOn
    $taskT2 = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5)
    Register-ScheduledTask -TaskName 'CodexNotifyWatch' -Action $taskAction -Trigger @($taskT1, $taskT2) -Force | Out-Null
    return @{ Success = $true; Message = '看守任务已成功升级为隐藏无闪版本！' }
  } catch {
    try {
      $cmd = "powershell -NoProfile -ExecutionPolicy Bypass -Command `"Register-ScheduledTask -TaskName 'CodexNotifyWatch' -Action (New-ScheduledTaskAction -Execute '$env:SystemRoot\System32\wscript.exe' -Argument '`\`"$env:USERPROFILE\bin\run-hidden.vbs`\`" `\`"$env:USERPROFILE\bin\codex-notify-watch.ps1`\`"') -Trigger @((New-ScheduledTaskTrigger -AtLogOn),(New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5))) -Force`""
      Start-Process powershell.exe -ArgumentList "-NoProfile -Command `"$cmd`"" -Verb RunAs -WindowStyle Hidden -Wait
      return @{ Success = $true; Message = '提权看守任务升级命令已触发！' }
    } catch {
      return @{ Success = $false; Message = '修复失败（需管理员权限）：' + $_.Exception.Message }
    }
  }
}

function Show-SettingsDialog {
  param([hashtable]$ctx)

  try {
    $cfgForm = New-Object System.Windows.Forms.Form
    $cfgForm.Text = 'linkWeixin - 配置与诊断中心'
    $cfgForm.Size = New-Object System.Drawing.Size(520, 560)
    $cfgForm.StartPosition = 'CenterScreen'
    $cfgForm.BackColor = $ctx.Colors.BG
    $cfgForm.ForeColor = $ctx.Colors.FG
    $cfgForm.FormBorderStyle = 'FixedDialog'
    $cfgForm.MaximizeBox = $false
    $cfgForm.MinimizeBox = $false
    $cfgForm.ShowInTaskbar = $false
    $cfgForm.TopMost = $true

    $curCfg = Get-LinkWeixinConfig

    # 分组 1：推送通道
    $grpChannels = New-Object System.Windows.Forms.GroupBox
    $grpChannels.Text = '推送通道配置（支持多通道联动）'
    $grpChannels.Location = New-Object System.Drawing.Point(16, 12)
    $grpChannels.Size = New-Object System.Drawing.Size(472, 230)
    $grpChannels.ForeColor = $ctx.Colors.FG
    $cfgForm.Controls.Add($grpChannels)

    # PushPlus
    $chkPp = New-Object System.Windows.Forms.CheckBox
    $chkPp.Text = 'PushPlus (微信服务号)'
    $chkPp.Location = New-Object System.Drawing.Point(16, 24)
    $chkPp.Size = New-Object System.Drawing.Size(180, 24)
    $chkPp.Checked = [bool]$curCfg.channels.pushplus.enabled
    $grpChannels.Controls.Add($chkPp)

    $txtPpToken = New-Object System.Windows.Forms.TextBox
    $txtPpToken.Location = New-Object System.Drawing.Point(196, 24)
    $txtPpToken.Size = New-Object System.Drawing.Size(190, 22)
    $txtPpToken.Text = [string]$curCfg.channels.pushplus.token
    $txtPpToken.UseSystemPasswordChar = $true
    $grpChannels.Controls.Add($txtPpToken)

    $btnPpShow = New-Object System.Windows.Forms.Button
    $btnPpShow.Text = '👁'
    $btnPpShow.Location = New-Object System.Drawing.Point(392, 23)
    $btnPpShow.Size = New-Object System.Drawing.Size(30, 24)
    $btnPpShow.FlatStyle = 'Flat'
    $btnPpShow.Cursor = [System.Windows.Forms.Cursors]::Hand
    $grpChannels.Controls.Add($btnPpShow)
    $btnPpShow.Add_Click({ $txtPpToken.UseSystemPasswordChar = -not $txtPpToken.UseSystemPasswordChar })

    # 企业微信 Webhook
    $chkWx = New-Object System.Windows.Forms.CheckBox
    $chkWx.Text = '企业微信群机器人'
    $chkWx.Location = New-Object System.Drawing.Point(16, 60)
    $chkWx.Size = New-Object System.Drawing.Size(180, 24)
    $chkWx.Checked = [bool]$curCfg.channels.wecom.enabled
    $grpChannels.Controls.Add($chkWx)

    $txtWxHook = New-Object System.Windows.Forms.TextBox
    $txtWxHook.Location = New-Object System.Drawing.Point(196, 60)
    $txtWxHook.Size = New-Object System.Drawing.Size(260, 22)
    $txtWxHook.Text = [string]$curCfg.channels.wecom.webhook
    $grpChannels.Controls.Add($txtWxHook)

    # 飞书 Webhook
    $chkFs = New-Object System.Windows.Forms.CheckBox
    $chkFs.Text = '飞书群机器人'
    $chkFs.Location = New-Object System.Drawing.Point(16, 96)
    $chkFs.Size = New-Object System.Drawing.Size(180, 24)
    $chkFs.Checked = [bool]$curCfg.channels.feishu.enabled
    $grpChannels.Controls.Add($chkFs)

    $txtFsHook = New-Object System.Windows.Forms.TextBox
    $txtFsHook.Location = New-Object System.Drawing.Point(196, 96)
    $txtFsHook.Size = New-Object System.Drawing.Size(260, 22)
    $txtFsHook.Text = [string]$curCfg.channels.feishu.webhook
    $grpChannels.Controls.Add($txtFsHook)

    # 钉钉 Webhook
    $chkDd = New-Object System.Windows.Forms.CheckBox
    $chkDd.Text = '钉钉群机器人'
    $chkDd.Location = New-Object System.Drawing.Point(16, 132)
    $chkDd.Size = New-Object System.Drawing.Size(180, 24)
    $chkDd.Checked = [bool]$curCfg.channels.dingtalk.enabled
    $grpChannels.Controls.Add($chkDd)

    $txtDdHook = New-Object System.Windows.Forms.TextBox
    $txtDdHook.Location = New-Object System.Drawing.Point(196, 132)
    $txtDdHook.Size = New-Object System.Drawing.Size(260, 22)
    $txtDdHook.Text = [string]$curCfg.channels.dingtalk.webhook
    $grpChannels.Controls.Add($txtDdHook)

    # 自定义 Webhook
    $chkCu = New-Object System.Windows.Forms.CheckBox
    $chkCu.Text = '自定义 Webhook'
    $chkCu.Location = New-Object System.Drawing.Point(16, 168)
    $chkCu.Size = New-Object System.Drawing.Size(180, 24)
    $chkCu.Checked = [bool]$curCfg.channels.custom.enabled
    $grpChannels.Controls.Add($chkCu)

    $txtCuHook = New-Object System.Windows.Forms.TextBox
    $txtCuHook.Location = New-Object System.Drawing.Point(196, 168)
    $txtCuHook.Size = New-Object System.Drawing.Size(260, 22)
    $txtCuHook.Text = [string]$curCfg.channels.custom.webhook
    $grpChannels.Controls.Add($txtCuHook)

    $lblTipChan = New-Object System.Windows.Forms.Label
    $lblTipChan.Text = '提示：通道凭据安全保存在用户配置目录中，多个通道可同时接收推送。'
    $lblTipChan.Location = New-Object System.Drawing.Point(16, 202)
    $lblTipChan.Size = New-Object System.Drawing.Size(440, 20)
    $lblTipChan.Font = $ctx.Fonts.Foot
    $lblTipChan.ForeColor = $ctx.Colors.DIM
    $grpChannels.Controls.Add($lblTipChan)

    # 分组 2：免打扰与偏好
    $grpPref = New-Object System.Windows.Forms.GroupBox
    $grpPref.Text = '偏好与免打扰'
    $grpPref.Location = New-Object System.Drawing.Point(16, 250)
    $grpPref.Size = New-Object System.Drawing.Size(472, 90)
    $grpPref.ForeColor = $ctx.Colors.FG
    $cfgForm.Controls.Add($grpPref)

    $lblQuiet = New-Object System.Windows.Forms.Label
    $lblQuiet.Text = '勿扰时段 (起-止小时)：'
    $lblQuiet.Location = New-Object System.Drawing.Point(16, 26)
    $lblQuiet.Size = New-Object System.Drawing.Size(150, 24)
    $grpPref.Controls.Add($lblQuiet)

    $txtQuiet = New-Object System.Windows.Forms.TextBox
    $txtQuiet.Location = New-Object System.Drawing.Point(170, 24)
    $txtQuiet.Size = New-Object System.Drawing.Size(80, 22)
    $txtQuiet.Text = [string]$curCfg.quietHours
    $grpPref.Controls.Add($txtQuiet)

    $lblQuietExp = New-Object System.Windows.Forms.Label
    $lblQuietExp.Text = '例如 23-8 表示 23:00 至次日 08:00 静默'
    $lblQuietExp.Location = New-Object System.Drawing.Point(260, 26)
    $lblQuietExp.Size = New-Object System.Drawing.Size(200, 20)
    $lblQuietExp.ForeColor = $ctx.Colors.DIM
    $lblQuietExp.Font = $ctx.Fonts.Foot
    $grpPref.Controls.Add($lblQuietExp)

    $lblCd = New-Object System.Windows.Forms.Label
    $lblCd.Text = '同一会话冷却时间：'
    $lblCd.Location = New-Object System.Drawing.Point(16, 56)
    $lblCd.Size = New-Object System.Drawing.Size(150, 24)
    $grpPref.Controls.Add($lblCd)

    $numCd = New-Object System.Windows.Forms.NumericUpDown
    $numCd.Location = New-Object System.Drawing.Point(170, 54)
    $numCd.Size = New-Object System.Drawing.Size(80, 22)
    $numCd.Minimum = 1
    $numCd.Maximum = 120
    $numCd.Value = [int]$curCfg.cooldownMin
    $grpPref.Controls.Add($numCd)

    $lblCdExp = New-Object System.Windows.Forms.Label
    $lblCdExp.Text = '分钟（防止密集输出连续刷屏推送）'
    $lblCdExp.Location = New-Object System.Drawing.Point(260, 56)
    $lblCdExp.Size = New-Object System.Drawing.Size(200, 20)
    $lblCdExp.ForeColor = $ctx.Colors.DIM
    $lblCdExp.Font = $ctx.Fonts.Foot
    $grpPref.Controls.Add($lblCdExp)

    # 分组 3：系统健康诊断与自愈
    $grpDiag = New-Object System.Windows.Forms.GroupBox
    $grpDiag.Text = '系统健康与自愈诊断'
    $grpDiag.Location = New-Object System.Drawing.Point(16, 348)
    $grpDiag.Size = New-Object System.Drawing.Size(472, 110)
    $grpDiag.ForeColor = $ctx.Colors.FG
    $cfgForm.Controls.Add($grpDiag)

    $lblDiagPlug = New-Object System.Windows.Forms.Label
    $lblDiagPlug.Text = '• OpenCode 插件：检测中...'
    $lblDiagPlug.Location = New-Object System.Drawing.Point(16, 24)
    $lblDiagPlug.Size = New-Object System.Drawing.Size(260, 20)
    $grpDiag.Controls.Add($lblDiagPlug)

    $lblDiagWatch = New-Object System.Windows.Forms.Label
    $lblDiagWatch.Text = '• 看守任务：检测中...'
    $lblDiagWatch.Location = New-Object System.Drawing.Point(16, 50)
    $lblDiagWatch.Size = New-Object System.Drawing.Size(260, 20)
    $grpDiag.Controls.Add($lblDiagWatch)

    $btnFixWatch = New-Object System.Windows.Forms.Button
    $btnFixWatch.Text = '⚡ 一键修复闪屏任务'
    $btnFixWatch.Location = New-Object System.Drawing.Point(280, 46)
    $btnFixWatch.Size = New-Object System.Drawing.Size(160, 28)
    $btnFixWatch.FlatStyle = 'Flat'
    $btnFixWatch.BackColor = [System.Drawing.Color]::FromArgb(200, 130, 30)
    $btnFixWatch.ForeColor = [System.Drawing.Color]::White
    $btnFixWatch.Cursor = [System.Windows.Forms.Cursors]::Hand
    $btnFixWatch.Visible = $false
    $grpDiag.Controls.Add($btnFixWatch)

    $btnFixWatch.Add_Click({
      $res = Repair-WatchTaskHidden
      if ($res.Success) {
        [System.Windows.Forms.MessageBox]::Show($res.Message, 'linkWeixin 自愈成功') | Out-Null
        $lblDiagWatch.Text = '• 看守任务：✅ 已修复为隐藏无闪版本'
        $btnFixWatch.Visible = $false
        Update-WidgetState -Ctx $ctx
      } else {
        [System.Windows.Forms.MessageBox]::Show($res.Message, 'linkWeixin 自愈提示') | Out-Null
      }
    })

    $pv = Test-PluginGate -Ctx $ctx
    $lblDiagPlug.Text = if ($pv -eq '新版') { '• OpenCode 插件：✅ 已就绪 (带完整闸控)' } else { "• OpenCode 插件：⚠️ $pv" }

    $tv = Test-WatchTask
    if ($tv -eq '新版') {
      $lblDiagWatch.Text = '• 看守任务：✅ 运行正常 (无闪烁)'
    } elseif ($tv -eq '旧版') {
      $lblDiagWatch.Text = '• 看守任务：⚠️ 旧版命令行 (每5分钟闪屏)'
      $btnFixWatch.Visible = $true
    } else {
      $lblDiagWatch.Text = "• 看守任务：$tv"
    }

    $lblDiagNet = New-Object System.Windows.Forms.Label
    $lblDiagNet.Text = '• 推送架构：多端统一渲染 + 智能排版'
    $lblDiagNet.Location = New-Object System.Drawing.Point(16, 78)
    $lblDiagNet.Size = New-Object System.Drawing.Size(430, 20)
    $lblDiagNet.ForeColor = $ctx.Colors.DIM
    $grpDiag.Controls.Add($lblDiagNet)

    # 底部操作按钮
    $btnSave = New-Object System.Windows.Forms.Button
    $btnSave.Text = '保存配置'
    $btnSave.Location = New-Object System.Drawing.Point(286, 474)
    $btnSave.Size = New-Object System.Drawing.Size(96, 32)
    $btnSave.FlatStyle = 'Flat'
    $btnSave.BackColor = $ctx.Colors.GREEN
    $btnSave.ForeColor = [System.Drawing.Color]::White
    $btnSave.Cursor = [System.Windows.Forms.Cursors]::Hand
    $cfgForm.Controls.Add($btnSave)

    $btnCancel = New-Object System.Windows.Forms.Button
    $btnCancel.Text = '取消'
    $btnCancel.Location = New-Object System.Drawing.Point(392, 474)
    $btnCancel.Size = New-Object System.Drawing.Size(96, 32)
    $btnCancel.FlatStyle = 'Flat'
    $btnCancel.BackColor = $ctx.Colors.CardBG
    $btnCancel.ForeColor = $ctx.Colors.FG
    $btnCancel.Cursor = [System.Windows.Forms.Cursors]::Hand
    $cfgForm.Controls.Add($btnCancel)

    $btnSave.Add_Click({
      $newCfg = @{
        channels = @{
          pushplus = @{ enabled = $chkPp.Checked; token = $txtPpToken.Text.Trim() }
          wecom    = @{ enabled = $chkWx.Checked; webhook = $txtWxHook.Text.Trim() }
          feishu   = @{ enabled = $chkFs.Checked; webhook = $txtFsHook.Text.Trim() }
          dingtalk = @{ enabled = $chkDd.Checked; webhook = $txtDdHook.Text.Trim() }
          custom   = @{ enabled = $chkCu.Checked; webhook = $txtCuHook.Text.Trim() }
        }
        quietHours  = $txtQuiet.Text.Trim()
        cooldownMin = [int]$numCd.Value
      }
      Set-LinkWeixinConfig -Config $newCfg | Out-Null
      [System.Windows.Forms.MessageBox]::Show('配置已保存！', 'linkWeixin') | Out-Null
      Update-WidgetState -Ctx $ctx
      $cfgForm.Close()
    })

    $btnCancel.Add_Click({ $cfgForm.Close() })

    [void]$cfgForm.ShowDialog($ctx.Form)
  } catch {
    Write-WidgetError 'settings-dialog' $_
  }
}
