function Format-CutSentence {
  param([string]$Text, [int]$Max)
  if ($Text.Length -le $Max) { return $Text }
  $window = $Text.Substring(0, $Max)
  $idx = $window.LastIndexOfAny(@('。', '！', '？', '!', '?', "`n"))
  if ($idx -ge 100) { return $window.Substring(0, $idx + 1) + "…" }
  return $window + "…"
}

function Format-NotifySummary {
  <#
  .SYNOPSIS
    摘要渲染（纯函数）：去代码块、按句截断、HTML 转义，再输出 PushPlus html。

  .DESCRIPTION
    渲染顺序与历史实现逐字一致：
    1. 去代码块（整段删），行内代码只留内容；
    2. 按句截断（在 HTML 转义之前，保证不断半个标签）；
    3. 转义后再做行内排版（此时插入的 <b>/<br> 不会被转义）。
  #>
  param([string]$Text, [int]$Max)

  # 1. 去代码块（整段删），行内代码只留内容。
  $t = [regex]::Replace($Text, '(?s)```.*?```', '')
  # 2. 按句截断（在 HTML 转义之前）。
  $t = Format-CutSentence $t.Trim() $Max
  # 3. 转义后做行内排版。
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
