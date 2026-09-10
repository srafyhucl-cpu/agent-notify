<#
.SYNOPSIS
  AI 任务完成推送（PushPlus -> 微信），dsh / opencode / codex 共用。

.DESCRIPTION
  各 agent 的 Stop/finish hook 都调这一个脚本。摘要优先级：
  1. -Summary 参数；2. 管道 stdin；3. 都没有则推一句默认文案。
  密钥只从环境变量 PUSHPLUS_TOKEN 读，不落盘。任何失败都静默，
  永远 exit 0，不卡住 agent。
  摘要渲染（结构化）：去代码块、标题加粗、列表分行、按句截断，
  输出 PushPlus html。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-ai.ps1 -Title "【AI任务】重构完成" -Summary "改了 3 个文件，测试通过"
  echo "小结文本" | powershell -NoProfile -ExecutionPolicy Bypass -File notify-ai.ps1 -Title "【AI任务】跑完了"
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-ai.ps1 -DryRun -Summary "只打印 payload，不真推"
#>
[CmdletBinding()]
param(
  [string]$Title = "【AI任务】跑完了",
  [string]$Summary = "",
  [int]$MaxChars = 500,
  [switch]$DryRun,
  [switch]$NoStdin
)

$ErrorActionPreference = 'SilentlyContinue'

function Cut-SentenceAware {
  param([string]$Text, [int]$Max)
  if ($Text.Length -le $Max) { return $Text }
  $window = $Text.Substring(0, $Max)
  $idx = $window.LastIndexOfAny(@('。', '！', '？', '!', '?', "`n"))
  if ($idx -ge 100) { return $window.Substring(0, $idx + 1) + "…" }
  return $window + "…"
}

function Format-Summary {
  param([string]$Text, [int]$Max)
  # 1. 去代码块（整段删），行内代码只留内容。
  $t = [regex]::Replace($Text, '(?s)```.*?```', '')
  # 2. 按句截断（在 HTML 转义之前，保证不断半个标签）。
  $t = Cut-SentenceAware $t.Trim() $Max
  # 3. 转义后再做行内排版（此时插入的 <b>/<br> 不会被转义）。
  $t = [System.Net.WebUtility]::HtmlEncode($t)
  $lines = $t -split "`n" | ForEach-Object { $_.TrimEnd() } | Where-Object { $_ -ne '---' }
  $out = foreach ($line in $lines) {
    # 先定块级类型并取出内文，再做行内排版，这样同一行可同时加粗+列表。
    $l = $line
    $wrap = ''
    if ($l -match '^(#{1,4})\s+(.*)$') { $wrap = 'b'; $l = $Matches[2] }
    elseif ($l -match '^>\s?(.*)$') { $l = $Matches[1] }
    elseif ($l -match '^(\d+[.)]|[-*])\s+(.*)$') { $wrap = 'li'; $l = $Matches[2] }
    $l = $l -replace '`([^`]+)`', '$1'
    $l = [regex]::Replace($l, '\*\*(.+?)\*\*', '<b>$1</b>')
    if ($wrap -eq 'b') { '<b>' + $l + '</b>' }
    elseif ($wrap -eq 'li') { '• ' + $l }
    else { $l }
  }
  $html = ($out -join '<br>')
  $html = [regex]::Replace($html, '(<br>\s*){3,}', '<br><br>')
  # 只去掉首尾成串的 <br>，不要用 Trim(char[])（会吃掉合法的 <b> 等字符）。
  $html = [regex]::Replace($html, '^(<br>\s*)+|(<br>\s*)+$', '')
  $html.Trim()
}

if ([string]::IsNullOrWhiteSpace($Summary) -and -not $NoStdin -and [Console]::IsInputRedirected) {
  $Summary = [Console]::In.ReadToEnd()
}
if ([string]::IsNullOrWhiteSpace($Summary)) {
  # 默认文案必须每次唯一：PushPlus 会拒收重复内容（code=999），
  # 固定文案第二次起就发不出去了，所以带上时间戳。
  $Summary = "任务完成，上线查看详情。[" + (Get-Date -Format "MM-dd HH:mm:ss") + "]"
}
$rendered = Format-Summary $Summary $MaxChars
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
  $shown | ConvertTo-Json -Compress
  exit 0
}

if ([string]::IsNullOrWhiteSpace($token)) {
  [Console]::Error.WriteLine('[notify-ai] PUSHPLUS_TOKEN 为空，跳过推送。')
  exit 0
}

try {
  $body = $payload | ConvertTo-Json -Compress
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
  $res = Invoke-RestMethod -Uri 'https://www.pushplus.plus/send' -Method Post `
    -ContentType 'application/json; charset=utf-8' -Body $bytes -TimeoutSec 10
  if ($res.code -ne 200) {
    [Console]::Error.WriteLine("[notify-ai] PushPlus 返回 code=$($res.code) msg=$($res.msg)")
  }
} catch {
  [Console]::Error.WriteLine('[notify-ai] 推送失败（已忽略）: ' + $_.Exception.Message)
}
exit 0
