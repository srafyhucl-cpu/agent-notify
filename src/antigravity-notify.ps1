#Requires -Version 5.1
<#
.SYNOPSIS
  Antigravity notify 钩子拦截器：接收 Stop 钩子事件并触发多通道推送。

.DESCRIPTION
  Antigravity 的 hooks.json 中配置 Stop 钩子调用本脚本。
  本脚本从 stdin 接收上下文 JSON：
  - fullyIdle == true 守卫；
  - 检查 antigravity-notify.off 标记；
  - 检查免打扰时段与会话防刷（冷却）；
  - 读取 transcriptPath 流式解析首行任务标题（前缀【Antigravity】）与末尾模型回复摘要；
  - 调用 Send-PushPlusNotification 发送通知；
  - 向 stdout 始终返回 {}，退出码 0。
#>
param(
  [string]$PayloadJson = '',
  [switch]$DryRun
)

$ErrorActionPreference = 'SilentlyContinue'

function Write-AntigravityDebug {
  param([string]$Line)
  if ($env:ANTIGRAVITY_NOTIFY_DEBUG -eq '0') { return }
  try {
    $tempDir = Join-Path $env:TEMP 'opencode'
    if (-not (Test-Path $tempDir)) { New-Item -ItemType Directory -Force -Path $tempDir | Out-Null }
    "$(Get-Date -Format o) $Line" | Out-File -FilePath "$tempDir\antigravity-notify-debug.log" -Append -Encoding utf8
  } catch { }
}

function Test-QuietHoursNow {
  param([string]$QuietRaw)
  if ([string]::IsNullOrWhiteSpace($QuietRaw)) { return $false }
  $m = [regex]::Match($QuietRaw, '^\s*(\d{1,2})\s*-\s*(\d{1,2})\s*$')
  if (-not $m.Success) { return $false }
  $s = [int]$m.Groups[1].Value
  $e = [int]$m.Groups[2].Value
  if ($s -lt 0 -or $s -gt 23 -or $e -lt 0 -or $e -gt 23 -or $s -eq $e) { return $false }
  $h = (Get-Date).Hour
  if ($s -lt $e) { return ($h -ge $s -and $h -lt $e) }
  return ($h -ge $s -or $h -lt $e)
}

function Get-TranscriptLineText {
  param([object]$Obj)
  if ($null -eq $Obj) { return '' }
  $val = $Obj.content
  if ($null -eq $val -and $Obj.message) { $val = $Obj.message.content }
  if ($null -eq $val -and $Obj.parts) { $val = $Obj.parts }
  if ($null -eq $val -and $Obj.message -and $Obj.message.parts) { $val = $Obj.message.parts }

  if ($val -is [string]) {
    return $val
  } elseif ($val -is [System.Collections.IEnumerable] -and $val -isnot [string]) {
    $texts = New-Object System.Collections.ArrayList
    foreach ($item in $val) {
      if ($item -is [string]) {
        [void]$texts.Add($item)
      } elseif ($item.type -eq 'text' -and $item.text) {
        [void]$texts.Add($item.text)
      } elseif ($item.text -and -not $item.type) {
        [void]$texts.Add($item.text)
      }
    }
    if ($texts.Count -gt 0) {
      return ($texts -join "`n")
    }
  }

  if ($val -and $val.text -is [string]) { return $val.text }
  if ($Obj.text -is [string]) { return $Obj.text }
  if ($Obj.prompt -is [string]) { return $Obj.prompt }
  if ($Obj.response -is [string]) { return $Obj.response }
  if ($Obj.message -and $Obj.message.text -is [string]) { return $Obj.message.text }
  return ''
}

function Test-IsAssistantTurn {
  param([object]$Obj)
  if ($null -eq $Obj) { return $false }
  $role = $Obj.role
  if (-not $role -and $Obj.message) { $role = $Obj.message.role }
  if (-not $role) { $role = $Obj.type }
  if (-not $role) { $role = $Obj.sender }
  if (-not $role) { $role = $Obj.author }
  if ($role -is [string] -and ($role -match '^(assistant|model|agent)$')) {
    return $true
  }
  return $false
}

try {
  Import-Module (Join-Path $PSScriptRoot 'lib\LinkWeixin\LinkWeixin.psd1') -ErrorAction Stop
} catch {
  Write-AntigravityDebug "module-load-failed: $($_.Exception.Message)"
  Write-Output '{}'
  exit 0
}

try {
  Write-AntigravityDebug "enter: PID=$PID IsInputRedirected=$([Console]::IsInputRedirected) PayloadJsonLen=$($PayloadJson.Length)"
  $raw = $PayloadJson
  if ([string]::IsNullOrWhiteSpace($raw) -and [Console]::IsInputRedirected) {
    $raw = [Console]::In.ReadToEnd()
  }
  Write-AntigravityDebug "received raw: len=$($raw.Length) preview=$(if ($raw.Length -gt 200) { $raw.Substring(0, 200) } else { $raw })"

  if ([string]::IsNullOrWhiteSpace($raw)) {
    Write-AntigravityDebug 'skip: empty payload'
    Write-Output '{}'
    exit 0
  }

  $hookContext = $null
  try {
    $hookContext = ConvertFrom-Json $raw
  } catch {
    Write-AntigravityDebug "bad json payload: $($_.Exception.Message)"
    Write-Output '{}'
    exit 0
  }

  if ($null -eq $hookContext) {
    Write-Output '{}'
    exit 0
  }

  # 守卫 1：fullyIdle 必须为 true
  $isFullyIdle = $false
  if ($hookContext.fullyIdle -is [bool]) {
    $isFullyIdle = $hookContext.fullyIdle
  } elseif ($hookContext.fullyIdle -is [string]) {
    $isFullyIdle = ($hookContext.fullyIdle.Trim().ToLower() -eq 'true' -or $hookContext.fullyIdle.Trim() -eq '1')
  } elseif ($hookContext.fullyIdle -is [int]) {
    $isFullyIdle = ($hookContext.fullyIdle -eq 1)
  }

  if (-not $isFullyIdle) {
    Write-AntigravityDebug "skip: fullyIdle is not true (fullyIdle=$($hookContext.fullyIdle))"
    Write-Output '{}'
    exit 0
  }

  # 守卫 2：marker 文件检查
  $paths = Get-LinkWeixinPaths
  $markerPath = $paths.AntigravityMarker
  if (Test-NotifyMarker -Path $markerPath) {
    Write-AntigravityDebug "skip: marker-off ($markerPath)"
    Write-Output '{}'
    exit 0
  }

  # 守卫 3：免打扰时段
  $cfg = Get-LinkWeixinConfig
  $quietSetting = if ($cfg.quietHours) { $cfg.quietHours } elseif ($env:ANTIGRAVITY_NOTIFY_QUIET) { $env:ANTIGRAVITY_NOTIFY_QUIET } else { $env:OPENCODE_NOTIFY_QUIET }
  if (Test-QuietHoursNow -QuietRaw $quietSetting) {
    Write-AntigravityDebug "skip: in quiet hours ($quietSetting)"
    Write-Output '{}'
    exit 0
  }

  # 守卫 4：会话防刷（冷却）
  $transcriptPath = [string]$hookContext.transcriptPath
  $sessionId = [string]$hookContext.conversationId
  if ([string]::IsNullOrWhiteSpace($sessionId)) { $sessionId = [string]$hookContext.sessionId }
  if ([string]::IsNullOrWhiteSpace($sessionId)) { $sessionId = [string]$hookContext.sessionID }
  if ([string]::IsNullOrWhiteSpace($sessionId) -and -not [string]::IsNullOrWhiteSpace($transcriptPath)) {
    $sessionId = [System.IO.Path]::GetFileName($transcriptPath)
  }
  if ([string]::IsNullOrWhiteSpace($sessionId)) { $sessionId = 'default-antigravity' }

  $cooldownMin = 10
  if ($cfg.cooldownMin -gt 0) { $cooldownMin = [int]$cfg.cooldownMin }
  if ($env:ANTIGRAVITY_NOTIFY_COOLDOWN_MIN) { $cooldownMin = [int]$env:ANTIGRAVITY_NOTIFY_COOLDOWN_MIN }

  $stateFile = if ($env:ANTIGRAVITY_NOTIFY_STATE_FILE) { $env:ANTIGRAVITY_NOTIFY_STATE_FILE } else { Join-Path $paths.TempDir 'antigravity-notify-sent.json' }
  $stateDir = Split-Path $stateFile -Parent
  if (-not (Test-Path $stateDir)) { New-Item -ItemType Directory -Force -Path $stateDir | Out-Null }

  $sentMap = @{}
  if (Test-Path $stateFile) {
    try {
      $rawSent = Get-Content $stateFile -Raw -Encoding UTF8
      $parsedSent = ConvertFrom-Json $rawSent
      foreach ($prop in $parsedSent.PSObject.Properties) {
        $sentMap[$prop.Name] = [double]$prop.Value
      }
    } catch { }
  }

  $nowEpoch = [double]([datetime]::UtcNow - [datetime]'1970-01-01').TotalMilliseconds
  $cooldownMs = [double]($cooldownMin * 60 * 1000)

  # 守卫 4：会话防刷（冷却，仅非 DryRun 模式下生效）
  $isDry = $DryRun -or ($env:ANTIGRAVITY_NOTIFY_DRYRUN -eq '1')
  if (-not $isDry) {
    if ($sentMap.ContainsKey($sessionId)) {
      $lastSent = [double]$sentMap[$sessionId]
      if (($nowEpoch - $lastSent) -lt $cooldownMs) {
        Write-AntigravityDebug "skip: cooldown sid=$sessionId"
        Write-Output '{}'
        exit 0
      }
    }

    $sentMap[$sessionId] = $nowEpoch
    $keysToRemove = @()
    foreach ($k in $sentMap.Keys) {
      if (($nowEpoch - $sentMap[$k]) -gt $cooldownMs) { $keysToRemove += $k }
    }
    foreach ($k in $keysToRemove) { $sentMap.Remove($k) }
    try {
      $sentMap | ConvertTo-Json -Compress | Out-File -FilePath $stateFile -Encoding UTF8 -Force
    } catch { }
  }

  # 提取逻辑：读取 transcript.jsonl
  $taskTitle = '任务完成'
  $modelSummary = ''

  if (-not [string]::IsNullOrWhiteSpace($transcriptPath) -and (Test-Path $transcriptPath)) {
    # 提取首行：用户任务标题（以 FileShare.ReadWrite 方式安全读取并发写的日志文件）
    try {
      $firstLine = ''
      $fs = [System.IO.File]::Open($transcriptPath, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
      $reader = New-Object System.IO.StreamReader($fs, [System.Text.Encoding]::UTF8)
      try {
        while (-not $reader.EndOfStream) {
          $line = $reader.ReadLine()
          if (-not [string]::IsNullOrWhiteSpace($line)) {
            $firstLine = $line
            break
          }
        }
      } finally {
        $reader.Dispose()
        $fs.Dispose()
      }

      if (-not [string]::IsNullOrWhiteSpace($firstLine)) {
        $firstObj = $null
        try { $firstObj = ConvertFrom-Json $firstLine } catch { }
        $userText = Get-TranscriptLineText $firstObj
        if ([string]::IsNullOrWhiteSpace($userText)) { $userText = $firstLine }

        # 优先提取 <USER_REQUEST> 内部用户指令，避免混入系统提示词
        if ($userText -match '(?si)<USER_REQUEST>(.*?)</USER_REQUEST>') {
          $userText = $Matches[1]
        }
        # 清除残留 XML / HTML 标签
        $userText = $userText -replace '(?s)<[^>]+>', ' '
        $userText = ($userText -replace '[\r\n\t]+', ' ').Trim()

        # 支持匹配 **Task**: / Task: / 任务: / **任务**:
        if ($userText -match '(?i)(?:\*{1,2})?(?:Task|任务)(?:\*{1,2})?[:：]\s*(.+)') {
          $userText = $Matches[1].Trim().Trim('*').Trim()
        }
        if ($userText.Length -gt 30) {
          $userText = $userText.Substring(0, 30) + '…'
        }
        if (-not [string]::IsNullOrWhiteSpace($userText)) {
          $taskTitle = $userText
        }
      }
    } catch {
      Write-AntigravityDebug "read first line error: $($_.Exception.Message)"
    }

    # 逆序提取尾部模型回复内容生成精炼 Markdown 摘要
    try {
      $tailLines = @(Get-Content -Path $transcriptPath -Tail 100 -Encoding UTF8 -ErrorAction SilentlyContinue)
      for ($i = $tailLines.Count - 1; $i -ge 0; $i--) {
        $l = $tailLines[$i]
        if ([string]::IsNullOrWhiteSpace($l)) { continue }
        $obj = $null
        try { $obj = ConvertFrom-Json $l } catch { }
        if ($null -ne $obj -and (Test-IsAssistantTurn $obj)) {
          $text = Get-TranscriptLineText $obj
          if (-not [string]::IsNullOrWhiteSpace($text)) {
            $trimmed = $text.Trim()
            if ($trimmed.Length -gt 2000) { $trimmed = $trimmed.Substring(0, 2000) }
            $modelSummary = $trimmed
            break
          }
        }
      }
      # 兜底：如果没匹配到标准角色行但有尾部内容
      if ([string]::IsNullOrWhiteSpace($modelSummary) -and $tailLines.Count -gt 1) {
        for ($i = $tailLines.Count - 1; $i -ge 1; $i--) {
          $cand = $tailLines[$i].Trim()
          if ($cand.Length -gt 0) {
            # 尝试从 JSON 对象提取纯文本，避免将原始 JSON 行作为摘要
            $candObj = $null
            try { $candObj = ConvertFrom-Json $cand } catch { }
            $candText = Get-TranscriptLineText $candObj
            if ([string]::IsNullOrWhiteSpace($candText)) { $candText = $cand }
            $candText = $candText.Trim()
            if ($candText.Length -gt 0) {
              if ($candText.Length -gt 2000) { $candText = $candText.Substring(0, 2000) }
              $modelSummary = $candText
              break
            }
          }
        }
      }
    } catch {
      Write-AntigravityDebug "read tail lines error: $($_.Exception.Message)"
    }
  }

  $title = "【Antigravity】$taskTitle"
  $isDry = $DryRun -or ($env:ANTIGRAVITY_NOTIFY_DRYRUN -eq '1')
  $res = Send-PushPlusNotification -Title $title -Summary $modelSummary -MaxChars 1000 -DryRun:$isDry
  Write-AntigravityDebug "push completed: title=$title summarylen=$($modelSummary.Length) dry=$isDry"

  if ($DryRun -and -not [string]::IsNullOrWhiteSpace($res)) {
    Write-Output $res
  } else {
    Write-Output '{}'
  }
} catch {
  Write-AntigravityDebug "unexpected error: $($_.Exception.Message)"
  Write-Output '{}'
}

exit 0
