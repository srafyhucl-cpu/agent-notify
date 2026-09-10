function Send-PushPlusNotification {
  <#
  .SYNOPSIS
    组装 payload 并调用 PushPlus（html 模板）。失败静默（写 stderr），不抛异常。

  .DESCRIPTION
    - 摘要为空时用带时间戳的默认文案（PushPlus 拒收重复内容 code=999）；
    - token 只从环境变量 PUSHPLUS_TOKEN 读，不落盘；为空时跳过并写 stderr；
    - -DryRun 返回屏蔽 token 的 payload JSON（不联网、不打印，由调用方输出）；
    - 超时 20 秒：慢代理链路实测要 9~12 秒；调用方（插件）硬超时 25 秒，仍有余量。
  #>
  [CmdletBinding()]
  param(
    [string]$Title = '【AI任务】跑完了',
    [string]$Summary = '',
    [int]$MaxChars = 500,
    [switch]$DryRun
  )

  if ([string]::IsNullOrWhiteSpace($Summary)) {
    # 默认文案必须每次唯一：固定文案第二次起发不出去（PushPlus code=999），所以带时间戳。
    $Summary = "任务完成，上线查看详情。[" + (Get-Date -Format "MM-dd HH:mm:ss") + "]"
  }
  $rendered = Format-NotifySummary $Summary $MaxChars
  if ($Title -notmatch '^【') {
    $Title = "【AI任务】$Title"
  }

  $token = $env:PUSHPLUS_TOKEN

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

  if ([string]::IsNullOrWhiteSpace($token)) {
    [Console]::Error.WriteLine('[notify-ai] PUSHPLUS_TOKEN 为空，跳过推送。')
    return
  }

  try {
    $body = $payload | ConvertTo-Json -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $res = Invoke-RestMethod -Uri 'https://www.pushplus.plus/send' -Method Post `
      -ContentType 'application/json; charset=utf-8' -Body $bytes -TimeoutSec 20
    if ($res.code -ne 200) {
      [Console]::Error.WriteLine("[notify-ai] PushPlus 返回 code=$($res.code) msg=$($res.msg)")
    }
  } catch {
    [Console]::Error.WriteLine('[notify-ai] 推送失败（已忽略）: ' + $_.Exception.Message)
  }
}
