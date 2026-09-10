function ConvertFrom-CodexNotifyEventArgs {
  <#
  .SYNOPSIS
    解析 codex notify 收到的参数数组：标题取 input-messages[0]（30 字截断），
    摘要取 last-assistant-message（2000 字截断）。

  .DESCRIPTION
    只处理形如 JSON 的参数（TrimStart 后以 { 开头），解析失败的参数跳过。
    摘要一旦取到即停止扫描；标题随每个成功解析的参数更新（与历史行为一致）。
    返回 @{ Title; Summary }，标题已带【codex】前缀；取不到时标题为「【codex】跑完了」。
  #>
  [CmdletBinding()]
  param([string[]]$Arguments)

  $summary = ''
  $taskName = ''
  foreach ($a in @($Arguments)) {
    if ($a -isnot [string] -or -not $a.TrimStart().StartsWith('{')) { continue }
    try {
      $evt = $a | ConvertFrom-Json -ErrorAction Stop
      $msg = $evt.'last-assistant-message'
      # 传原文（只去首尾空）：排版和截断由 notify-ai 统一做。
      if ($msg -is [string] -and $msg.Trim().Length -gt 0) {
        $summary = $msg.Trim()
        if ($summary.Length -gt 2000) { $summary = $summary.Substring(0, 2000) }
      }
      $first = $evt.'input-messages'
      if ($first -is [array] -and $first.Count -gt 0 -and $first[0] -is [string] -and $first[0].Trim().Length -gt 0) {
        $taskName = ($first[0] -replace '\s+', ' ').Trim()
        if ($taskName.Length -gt 30) { $taskName = $taskName.Substring(0, 30) + "…" }
      }
      if ($summary -ne '') { break }
    } catch { }
  }

  $title = if ($taskName -ne '') { "【codex】$taskName" } else { '【codex】跑完了' }
  return @{ Title = $title; Summary = $summary }
}
