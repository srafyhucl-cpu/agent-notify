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

$moduleOk = $true
try {
  Import-Module (Join-Path $PSScriptRoot 'lib\LinkWeixin\LinkWeixin.psd1') -ErrorAction Stop
} catch {
  $moduleOk = $false
}

try { New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP 'opencode') | Out-Null } catch { }

function Write-CodexNotifyDebug {
  param([string]$Line)
  if ($env:CODEX_NOTIFY_DEBUG -ne '1') { return }
  try { "$(Get-Date -Format o) $Line" | Out-File -FilePath "$env:TEMP\opencode\codex-notify-debug.log" -Append -Encoding utf8 } catch { }
}

Write-CodexNotifyDebug "args=$($Passthru -join ' | ')"
if (-not $moduleOk) { Write-CodexNotifyDebug 'module-import-failed; push disabled' }

# 先透传原电脑操控集成（即便模块挂了也要保住原行为；找不到 exe 就跳过）。
try {
  $ORIGINAL = $null
  if ($moduleOk) {
    $ORIGINAL = Get-CodexComputerUseExe
  } else {
    try {
      $ORIGINAL = Get-ChildItem "$env:LOCALAPPDATA\OpenAI\Codex\runtimes\cua_node\*\bin\node_modules\@oai\sky\bin\windows\codex-computer-use.exe" -ErrorAction Stop |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1 -ExpandProperty FullName
    } catch { $ORIGINAL = $null }
  }
  if ($ORIGINAL) {
    if ([Console]::IsInputRedirected) {
      [Console]::In.ReadToEnd() | & $ORIGINAL @Passthru
    } else {
      & $ORIGINAL @Passthru
    }
  }
} catch { }

try {
  if (-not $moduleOk) { exit 0 }
  # marker 存在 = 只跳过推送（透传已在上面执行完，不受影响）。
  $paths = Get-LinkWeixinPaths
  if (Test-NotifyMarker -Path $paths.CodexMarker) {
    Write-CodexNotifyDebug 'marker-off skip push'
    exit 0
  }

  $t0 = Get-Date
  $parsed = ConvertFrom-CodexNotifyEventArgs -Arguments $Passthru
  $title = $parsed.Title
  $summary = $parsed.Summary

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
  if ($summary -ne '') { $ppArgs += @("-Summary", $summary) }
  & powershell @ppArgs
  $code = $LASTEXITCODE
  $secs = [math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
  Write-CodexNotifyDebug "push exit=$code secs=$secs summarylen=$($summary.Length)"
} catch {
  Write-CodexNotifyDebug "push throw=$($_.Exception.Message)"
}

exit 0
