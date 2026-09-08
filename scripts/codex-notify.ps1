#Requires -Version 5.1
<#
.SYNOPSIS
  Codex notify 中转：先原样透传给电脑操控集成，再发微信推送。

.DESCRIPTION
  codex 的 `notify` 同时只能配一个程序，本脚本保住原有
  `codex-computer-use.exe turn-ended` 行为不变，再调 notify-ai.ps1。
  codex 追加的任何参数和 stdin 都原样透传；任何一步失败都静默，
  永远 exit 0，不影响 codex 运行。

  联调：$env:CODEX_NOTIFY_DEBUG = "1" 时把收到的参数记到临时文件。
#>
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Passthru)

$ErrorActionPreference = 'SilentlyContinue'

# 随用随开 marker（与 opencode 插件、notify-toggle 同路径约定，
# 测试用 OPENCODE_NOTIFY_MARKER_FILE 覆盖）：存在只跳过推送，
# 原电脑操控透传不受影响（透传在下面先执行）。
$MarkerFile = if ($env:OPENCODE_NOTIFY_MARKER_FILE) { $env:OPENCODE_NOTIFY_MARKER_FILE } else { Join-Path $env:USERPROFILE '.config\opencode\notify-pushplus.off' }

try { New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP 'opencode') | Out-Null } catch { }

# 原集成路径里的 cua_node 哈希目录会随更新变化，每次动态找最新的。
function Get-OriginalNotify {
  try {
    Get-ChildItem "$env:LOCALAPPDATA\OpenAI\Codex\runtimes\cua_node\*\bin\node_modules\@oai\sky\bin\windows\codex-computer-use.exe" -ErrorAction Stop |
      Sort-Object LastWriteTime -Descending |
      Select-Object -First 1 -ExpandProperty FullName
  } catch { $null }
}

if ($env:CODEX_NOTIFY_DEBUG -eq '1') {
  try {
    $log = "$env:TEMP\opencode\codex-notify-debug.log"
    "[$(Get-Date -Format o)] args=$($Passthru -join ' | ')" | Out-File -FilePath $log -Append -Encoding utf8
  } catch { }
}

try {
  $ORIGINAL = Get-OriginalNotify
  if ($ORIGINAL) {
    if ([Console]::IsInputRedirected) {
      [Console]::In.ReadToEnd() | & $ORIGINAL @Passthru
    } else {
      & $ORIGINAL @Passthru
    }
  }
} catch { }

try {
  # marker 存在 = 只跳过推送（透传已在上面执行完，不受影响）。
  if (Test-Path $MarkerFile) {
    if ($env:CODEX_NOTIFY_DEBUG -eq '1') {
      "marker-off skip push" | Out-File -FilePath "$env:TEMP\opencode\codex-notify-debug.log" -Append -Encoding utf8
    }
    exit 0
  }
  $t0 = Get-Date
  $summary = ""
  $taskName = ""
  foreach ($a in $Passthru) {
    if ($a -isnot [string] -or -not $a.TrimStart().StartsWith("{")) { continue }
    try {
      $evt = $a | ConvertFrom-Json -ErrorAction Stop
      $msg = $evt.'last-assistant-message'
      # 传原文（只去首尾空）：排版和截断由 notify-ai.ps1 统一做。
      if ($msg -is [string] -and $msg.Trim().Length -gt 0) {
        $summary = $msg.Trim()
        if ($summary.Length -gt 2000) { $summary = $summary.Substring(0, 2000) }
      }
      $first = $evt.'input-messages'
      if ($first -is [array] -and $first.Count -gt 0 -and $first[0] -is [string] -and $first[0].Trim().Length -gt 0) {
        $taskName = ($first[0] -replace '\s+', ' ').Trim()
        if ($taskName.Length -gt 30) { $taskName = $taskName.Substring(0, 30) + "…" }
      }
      if ($summary -ne "") { break }
    } catch { }
  }
  if ($taskName -ne "") { $title = "【codex】$taskName" } else { $title = "【codex】跑完了" }
  $NotifyScript = $env:NOTIFY_AI_SCRIPT
  if ([string]::IsNullOrWhiteSpace($NotifyScript)) {
    $NotifyScript = Join-Path $PSScriptRoot 'notify-ai.ps1'
  }
  $ppArgs = @(
    "-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass",
    "-File", $NotifyScript,
    "-Title", $title,
    "-NoStdin"
  )
  # 注意：-Summary 为空时必须整个省略，传空字符串会导致子进程参数绑定失败。
  if ($summary -ne "") { $ppArgs += @("-Summary", $summary) }
  & powershell @ppArgs
  $code = $LASTEXITCODE
  $secs = [math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
  if ($env:CODEX_NOTIFY_DEBUG -eq '1') {
    "push exit=$code secs=$secs summarylen=$($summary.Length)" | Out-File -FilePath "$env:TEMP\opencode\codex-notify-debug.log" -Append -Encoding utf8
  }
} catch {
  if ($env:CODEX_NOTIFY_DEBUG -eq '1') {
    "push throw=$($_.Exception.Message)" | Out-File -FilePath "$env:TEMP\opencode\codex-notify-debug.log" -Append -Encoding utf8
  }
}

exit 0
