function Send-PushPlusNotification {
  <#
  .SYNOPSIS
    组装 payload 并调用推送通知通道（PushPlus 及企业微信/飞书/钉钉等）。失败静默（写 stderr），不抛异常。

  .DESCRIPTION
    - 摘要为空时用带时间戳的默认文案（PushPlus 拒收重复内容 code=999）；
    - token 优先从配置文件或环境变量 PUSHPLUS_TOKEN 读；
    - 支持多通道联动（企业微信、飞书、钉钉机器人 Webhook）；
    - -DryRun 返回屏蔽 token 的 payload JSON（完全保持向后兼容）；
    - 超时 20 秒，失败静默记录，不阻塞主流程。
  #>
  [CmdletBinding()]
  param(
    [string]$Title = '【AI任务】跑完了',
    [string]$Summary = '',
    [int]$MaxChars = 500,
    [switch]$DryRun
  )

  if ([string]::IsNullOrWhiteSpace($Summary)) {
    $Summary = "任务完成，上线查看详情。[" + (Get-Date -Format "MM-dd HH:mm:ss") + "]"
  }
  $rendered = Format-NotifySummary $Summary $MaxChars
  if ($Title -notmatch '^【') {
    $Title = "【AI任务】$Title"
  }

  $cfg = Get-LinkWeixinConfig
  $token = ''
  if ($cfg.channels.pushplus.enabled -and -not [string]::IsNullOrWhiteSpace($cfg.channels.pushplus.token)) {
    $token = $cfg.channels.pushplus.token
  } elseif (-not [string]::IsNullOrWhiteSpace($env:PUSHPLUS_TOKEN)) {
    $token = $env:PUSHPLUS_TOKEN
  }

  $payload = @{
    title    = $Title
    content  = $rendered
    template = 'html'
  }
  if (-not [string]::IsNullOrWhiteSpace($token)) {
    $payload['token'] = $token
  }

  if ($DryRun) {
    $shown = $payload.Clone()
    if ($shown.ContainsKey('token')) { $shown['token'] = '****' }
    return ($shown | ConvertTo-Json -Compress)
  }

  $channelsPushed = New-Object System.Collections.ArrayList
  $channelsFailed = New-Object System.Collections.ArrayList

  # 1. PushPlus 微信通道
  if (-not [string]::IsNullOrWhiteSpace($token) -and $cfg.channels.pushplus.enabled) {
    try {
      $body = $payload | ConvertTo-Json -Compress
      $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
      $res = Invoke-RestMethod -Uri 'https://www.pushplus.plus/send' -Method Post `
        -ContentType 'application/json; charset=utf-8' -Body $bytes -TimeoutSec 20
      if ($res.code -eq 200) {
        [void]$channelsPushed.Add('PushPlus')
      } else {
        [void]$channelsFailed.Add('PushPlus')
        [Console]::Error.WriteLine("[notify-ai] PushPlus 返回 code=$($res.code) msg=$($res.msg)")
      }
    } catch {
      [void]$channelsFailed.Add('PushPlus')
      [Console]::Error.WriteLine('[notify-ai] PushPlus 失败: ' + $_.Exception.Message)
    }
  } elseif ([string]::IsNullOrWhiteSpace($token)) {
    [Console]::Error.WriteLine('[notify-ai] PUSHPLUS_TOKEN 为空，跳过 PushPlus 通道。')
  }

  # 2. 企业微信 Webhook 通道
  $wecomUrl = $cfg.channels.wecom.webhook
  if ($cfg.channels.wecom.enabled -and -not [string]::IsNullOrWhiteSpace($wecomUrl)) {
    try {
      $wecomBody = @{
        msgtype = 'markdown'
        markdown = @{
          content = "### $Title`r`n`r`n$Summary"
        }
      } | ConvertTo-Json -Depth 4 -Compress
      $res = Invoke-RestMethod -Uri $wecomUrl -Method Post -ContentType 'application/json; charset=utf-8' -Body ([System.Text.Encoding]::UTF8.GetBytes($wecomBody)) -TimeoutSec 15
      if ($res.errcode -eq 0) { [void]$channelsPushed.Add('企业微信') }
      else { [void]$channelsFailed.Add('企业微信') }
    } catch {
      [void]$channelsFailed.Add('企业微信')
      [Console]::Error.WriteLine('[notify-ai] 企业微信 Webhook 失败: ' + $_.Exception.Message)
    }
  }

  # 3. 飞书 Webhook 通道
  $feishuUrl = $cfg.channels.feishu.webhook
  if ($cfg.channels.feishu.enabled -and -not [string]::IsNullOrWhiteSpace($feishuUrl)) {
    try {
      $feishuBody = @{
        msg_type = 'interactive'
        card = @{
          header = @{ title = @{ tag = 'plain_text'; content = $Title }; template = 'blue' }
          elements = @(@{ tag = 'div'; text = @{ tag = 'lark_md'; content = $Summary } })
        }
      } | ConvertTo-Json -Depth 6 -Compress
      $res = Invoke-RestMethod -Uri $feishuUrl -Method Post -ContentType 'application/json; charset=utf-8' -Body ([System.Text.Encoding]::UTF8.GetBytes($feishuBody)) -TimeoutSec 15
      if ($res.code -eq 0) { [void]$channelsPushed.Add('飞书') }
      else { [void]$channelsFailed.Add('飞书') }
    } catch {
      [void]$channelsFailed.Add('飞书')
      [Console]::Error.WriteLine('[notify-ai] 飞书 Webhook 失败: ' + $_.Exception.Message)
    }
  }

  # 4. 钉钉 Webhook 通道
  $dingUrl = $cfg.channels.dingtalk.webhook
  if ($cfg.channels.dingtalk.enabled -and -not [string]::IsNullOrWhiteSpace($dingUrl)) {
    try {
      $dingBody = @{
        msgtype = 'markdown'
        markdown = @{
          title = $Title
          text = "### $Title`r`n`r`n$Summary"
        }
      } | ConvertTo-Json -Depth 4 -Compress
      $res = Invoke-RestMethod -Uri $dingUrl -Method Post -ContentType 'application/json; charset=utf-8' -Body ([System.Text.Encoding]::UTF8.GetBytes($dingBody)) -TimeoutSec 15
      if ($res.errcode -eq 0) { [void]$channelsPushed.Add('钉钉') }
      else { [void]$channelsFailed.Add('钉钉') }
    } catch {
      [void]$channelsFailed.Add('钉钉')
      [Console]::Error.WriteLine('[notify-ai] 钉钉 Webhook 失败: ' + $_.Exception.Message)
    }
  }

  # 5. 自定义 Webhook 通道
  $customUrl = $cfg.channels.custom.webhook
  if ($cfg.channels.custom.enabled -and -not [string]::IsNullOrWhiteSpace($customUrl)) {
    try {
      $customBody = @{
        title = $Title
        content = $Summary
        rendered = $rendered
        timestamp = (Get-Date -Format o)
      } | ConvertTo-Json -Depth 4 -Compress
      Invoke-RestMethod -Uri $customUrl -Method Post -ContentType 'application/json; charset=utf-8' -Body ([System.Text.Encoding]::UTF8.GetBytes($customBody)) -TimeoutSec 15 | Out-Null
      [void]$channelsPushed.Add('自定义Webhook')
    } catch {
      [void]$channelsFailed.Add('自定义Webhook')
      [Console]::Error.WriteLine('[notify-ai] 自定义 Webhook 失败: ' + $_.Exception.Message)
    }
  }

  # 统一落盘日志（供推送历史查看器与悬浮窗状态解析，保持时间戳在首列）
  try {
    $paths = Get-LinkWeixinPaths
    $logPath = $paths.PushLog
    $logDir = Split-Path $logPath -Parent
    if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
    $nowIso = [datetime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    $chanText = if ($channelsPushed.Count -gt 0) { $channelsPushed -join ',' } else { '无通道投递' }
    $statusText = if ($channelsFailed.Count -eq 0 -and $channelsPushed.Count -gt 0) { '成功' } elseif ($channelsPushed.Count -gt 0) { '部分成功' } else { '未发送' }
    $shortSummary = ($Summary -replace '[\r\n\t]+', ' ').Trim()
    if ($shortSummary.Length -gt 120) { $shortSummary = $shortSummary.Substring(0, 120) + "..." }
    $logEntry = "$nowIso push title=$Title | channels=$chanText | status=$statusText | summary=$shortSummary`r`n"
    [IO.File]::AppendAllText($logPath, $logEntry, (New-Object System.Text.UTF8Encoding($false)))
  } catch { }
}
