#Requires -Version 5.1
<#
.SYNOPSIS
  把 Codex 的 notify 行接到 agentnotify-codex-hook.exe，只改写 notify 行。

.DESCRIPTION
  默认路径：
  - Hook：%USERPROFILE%\bin\agentnotify-codex-hook.exe
  - Codex 配置：%USERPROFILE%\.codex\config.toml
  - 事件入口：%USERPROFILE%\bin\agentnotify-ingress.exe

  保护规则：
  - 只修改 notify 行；修改前备份到 config.toml.bak-notify-wrapper。
  - 链上已有 AgentNotify（旧 agent-notify.exe 或新 Hook）时只替换链内路径，
    保留 codex-computer-use.exe 包装与用户自定义的 --previous-notify 载荷。
  - notify 指向 codex-computer-use.exe 时改写为 Hook 直连，并保留原有 --previous-notify 载荷。
  - notify 是多行 TOML 数组时拒绝改写并报错，保证 config.toml 不被损坏。
  - notify 是其他自定义程序时保持原样。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-codex-v2.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-codex-v2.ps1 -HookPath D:\app\agentnotify-codex-hook.exe -ConfigPath D:\Temp\codex\config.toml
#>
param(
  [string]$HookPath = '',
  [string]$ConfigPath = '',
  [string]$Ingress = ''
)

$ErrorActionPreference = 'Stop'

# 旧 AgentNotify 与新 Hook 都视为“链上已有 AgentNotify”，安装器负责把路径换成本次 Hook。
$AgentNotifyPattern = '(?i)(agent-notify\.exe|agentnotify-codex-hook\.exe)'
$CuaPattern = '(?i)codex-computer-use\.exe'
$IngressExeName = 'agentnotify-ingress.exe'
# 识别多行 notify 时最多读取的字符数，防止异常配置把识别过程拖长。
$NotifyBlockMaxChars = 4096

if ([string]::IsNullOrWhiteSpace($HookPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $HookPath = Join-Path $homeDirectory 'bin\agentnotify-codex-hook.exe'
}
if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $ConfigPath = Join-Path $homeDirectory '.codex\config.toml'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Ingress = Join-Path $homeDirectory "bin\$IngressExeName"
}

$hookPathFull = [IO.Path]::GetFullPath($HookPath)
$configPathFull = [IO.Path]::GetFullPath($ConfigPath)
$ingressPathFull = [IO.Path]::GetFullPath($Ingress)
# notify 行统一写正斜杠，避免 TOML 转义差异（与旧安装器一致）。
$hookSlash = $hookPathFull.Replace('\', '/')

if (-not (Test-Path -LiteralPath $hookPathFull -PathType Leaf)) {
  throw "Codex Hook executable not found: $hookPathFull"
}
if ([IO.Path]::GetExtension($hookPathFull) -ine '.exe') {
  throw "Codex Hook must be an .exe file: $hookPathFull"
}
if (-not (Test-Path -LiteralPath $configPathFull -PathType Leaf)) {
  throw "Codex config not found: $configPathFull"
}
if (-not (Test-Path -LiteralPath $ingressPathFull -PathType Leaf)) {
  throw "Ingress executable not found: $ingressPathFull"
}

function Unescape-TomlString {
  param([string]$Quoted)
  if ($Quoted.Length -le 2) { return '' }
  try {
    return [regex]::Unescape($Quoted.Substring(1, $Quoted.Length - 2))
  } catch {
    return $Quoted
  }
}

# 原样取出 --previous-notify 后面那个带引号的 JSON 载荷；没有时返回空串。
function Get-PreviousNotifyToken {
  param([string]$NotifyLine)
  $items = [regex]::Matches($NotifyLine, '"(?:\\.|[^"])*"')
  for ($index = 0; $index -lt ($items.Count - 1); $index++) {
    if ((Unescape-TomlString $items[$index].Value) -ieq '--previous-notify') {
      return $items[$index + 1].Value
    }
  }
  return ''
}

# 取 notify 赋值块：单行时就是该行；多行时读到方括号闭合处（仅用于识别与拒绝，不改写）。
function Get-NotifyBlock {
  param([string]$Text)
  $block = ''
  $open = 0
  $close = 0
  foreach ($line in ($Text -split "`r?`n")) {
    if ($block.Length -gt 0) { $block += "`n" }
    $block += $line
    $open += ([regex]::Matches($line, '\[')).Count
    $close += ([regex]::Matches($line, '\]')).Count
    if ($open -gt 0 -and $open -eq $close) { break }
    if ($block.Length -ge $NotifyBlockMaxChars) { break }
  }
  return $block
}

# TOML 允许数组跨多行；安装器只改写单行 notify，多行时拒绝并保持配置零改动。
function Assert-NotifyLineIsSingleLine {
  param(
    [string]$NotifyLine,
    [string]$NotifyBlock,
    [string]$ConfigPath
  )
  if ([string]::IsNullOrWhiteSpace($NotifyLine)) { return }
  $open = ([regex]::Matches($NotifyBlock, '\[')).Count
  $close = ([regex]::Matches($NotifyBlock, '\]')).Count
  $singleLine = $NotifyBlock -notmatch "`n"
  if ($singleLine -and ($open -eq $close) -and $NotifyBlock.TrimEnd().EndsWith(']')) { return }
  throw "Codex notify 是多行 TOML 数组，安装器只支持单行写法，本次不会改动配置：$ConfigPath。请先把 notify 合并为单行（行尾注释请移到单独一行）后重试。当前行：$NotifyLine"
}

# 把链上指向 AgentNotify 的路径替换成 $NewPath，其余项（CUA、第三方）原样保留。
function Update-AgentNotifyPathInNotifyLine {
  param(
    [string]$NotifyLine,
    [string]$NewPath
  )
  $items = [regex]::Matches($NotifyLine, '"(?:\\.|[^"])*"')
  if ($items.Count -eq 0) { return $NotifyLine }

  $changed = $false
  $kept = New-Object System.Collections.Generic.List[string]
  foreach ($item in $items) {
    $raw = $item.Value
    $value = Unescape-TomlString $raw

    if ($value -match ($AgentNotifyPattern + '\s*$')) {
      $kept.Add('"' + $NewPath + '"')
      $changed = $true
      continue
    }

    if ($value -match $AgentNotifyPattern) {
      # --previous-notify 的载荷是内嵌 JSON 数组，逐项替换后重新转义。
      $innerFixed = $null
      try {
        $inner = ConvertFrom-Json -InputObject $value
        if ($inner -is [System.Array]) {
          $innerChanged = $false
          for ($index = 0; $index -lt $inner.Count; $index++) {
            if ([string]$inner[$index] -match ($AgentNotifyPattern + '\s*$')) {
              $inner[$index] = $NewPath
              $innerChanged = $true
            }
          }
          if ($innerChanged) {
            $innerFixed = (ConvertTo-Json -InputObject @($inner) -Compress).Replace('"', '\"')
          }
        }
      } catch { }

      if (-not $innerFixed) {
        # 兜底：历史或手写的不规范转义会让 JSON 解析失败，此时按路径片段替换。
        $pattern = '(?i)[^"\[\],\s]*(agent-notify\.exe|agentnotify-codex-hook\.exe)'
        $replaced = [regex]::Replace($value, $pattern, [System.Text.RegularExpressions.MatchEvaluator] { param($match) $NewPath })
        if ($replaced -ne $value) {
          $innerFixed = $replaced.Replace('\', '\\').Replace('"', '\"')
        }
      }

      if ($innerFixed) {
        $kept.Add('"' + $innerFixed + '"')
        $changed = $true
        continue
      }
    }

    $kept.Add($raw)
  }

  if (-not $changed) { return $NotifyLine }
  return 'notify = [ ' + ($kept -join ', ') + ' ]'
}

# 直连 Hook；用户原有的 --previous-notify 载荷原样保留，由 Hook 透传给上游。
function New-DirectNotifyLine {
  param(
    [string]$NewPath,
    [string]$PreviousNotifyToken
  )
  $items = New-Object System.Collections.Generic.List[string]
  $items.Add('"' + $NewPath + '"')
  $items.Add('"codex"')
  $items.Add('"turn-ended"')
  if (-not [string]::IsNullOrWhiteSpace($PreviousNotifyToken)) {
    $items.Add('"--previous-notify"')
    $items.Add($PreviousNotifyToken)
  }
  return 'notify = [ ' + ($items -join ', ') + ' ]'
}

function Write-ConfigAtomically {
  param(
    [string]$Path,
    [string]$Content
  )
  $directory = Split-Path -Parent $Path
  $temporary = Join-Path $directory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
  try {
    [IO.File]::WriteAllText($temporary, $Content, (New-Object Text.UTF8Encoding($false)))
    Move-Item -LiteralPath $temporary -Destination $Path -Force
  } finally {
    if (Test-Path -LiteralPath $temporary) {
      Remove-Item -LiteralPath $temporary -Force
    }
  }
}

$content = [IO.File]::ReadAllText($configPathFull)
$notifyMatch = [regex]::Match($content, '(?m)^notify\s*=.*$')
$notifyLine = $notifyMatch.Value
# 多行数组只截到闭合处用于识别；真正改写前会再次校验并拒绝多行写法。
$notifyBlock = if ($notifyMatch.Success) { Get-NotifyBlock -Text $content.Substring($notifyMatch.Index) } else { '' }

if ($notifyBlock -match $AgentNotifyPattern) {
  Assert-NotifyLineIsSingleLine -NotifyLine $notifyLine -NotifyBlock $notifyBlock -ConfigPath $configPathFull
  $updatedLine = Update-AgentNotifyPathInNotifyLine -NotifyLine $notifyLine -NewPath $hookSlash
  if ($updatedLine -ne $notifyLine) {
    Copy-Item -LiteralPath $configPathFull -Destination "$configPathFull.bak-notify-wrapper" -Force
    $lineMatch = [regex]::Match($content, '(?m)^notify\s*=.*$')
    $updated = $content.Substring(0, $lineMatch.Index) + $updatedLine + $content.Substring($lineMatch.Index + $lineMatch.Length)
    Write-ConfigAtomically -Path $configPathFull -Content $updated
    Write-Output "[hook] Codex notify 链上的 AgentNotify 已替换为 Hook（备份：$configPathFull.bak-notify-wrapper）"
    Write-Output "[hook] $updatedLine"
  } elseif ($notifyLine -match [regex]::Escape($hookSlash)) {
    Write-Output "[hook] Codex notify 已指向本 Hook，无需改动。"
  } else {
    throw "Codex notify 行含有 AgentNotify 但无法自动替换，请手动改为：$hookSlash（当前：$notifyLine）"
  }
} elseif ($notifyBlock -match $CuaPattern) {
  Assert-NotifyLineIsSingleLine -NotifyLine $notifyLine -NotifyBlock $notifyBlock -ConfigPath $configPathFull
  $previousNotify = Get-PreviousNotifyToken -NotifyLine $notifyLine
  $updatedLine = New-DirectNotifyLine -NewPath $hookSlash -PreviousNotifyToken $previousNotify
  Copy-Item -LiteralPath $configPathFull -Destination "$configPathFull.bak-notify-wrapper" -Force
  $updated = [regex]::Replace($content, '(?m)^notify\s*=.*$', [System.Text.RegularExpressions.MatchEvaluator] { param($match) $updatedLine })
  Write-ConfigAtomically -Path $configPathFull -Content $updated
  Write-Output "[hook] Codex notify 已接管为 Hook 直连（备份：$configPathFull.bak-notify-wrapper）"
  Write-Output "[hook] $updatedLine"
  if (-not [string]::IsNullOrWhiteSpace($previousNotify)) {
    Write-Output '[hook] 已保留原有 --previous-notify 载荷，Hook 会随参数透传给 codex-computer-use.exe。'
  }
} elseif ([string]::IsNullOrWhiteSpace($notifyLine)) {
  Copy-Item -LiteralPath $configPathFull -Destination "$configPathFull.bak-notify-wrapper" -Force
  $updatedLine = New-DirectNotifyLine -NewPath $hookSlash -PreviousNotifyToken ''
  $updated = $content.TrimEnd() + "`r`n" + $updatedLine + "`r`n"
  Write-ConfigAtomically -Path $configPathFull -Content $updated
  Write-Output "[hook] Codex notify 已写入配置（备份：$configPathFull.bak-notify-wrapper）"
  Write-Output "[hook] $updatedLine"
} else {
  Write-Output "[hook] Codex notify 是自定义程序，保持原样；如需接入请手动改为：$hookSlash"
}

# Hook 运行时按 env → 同目录 → 正式安装目录查找 ingress，这里提前提醒不可达的情况。
$hookDirectory = Split-Path -Parent $hookPathFull
$ingressCandidates = @(
  (Join-Path $hookDirectory $IngressExeName),
  (Join-Path (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\Agent-notify') $IngressExeName)
)
$ingressReachable = $false
foreach ($candidate in $ingressCandidates) {
  if (Test-Path -LiteralPath $candidate -PathType Leaf) { $ingressReachable = $true; break }
}
if (-not $ingressReachable) {
  Write-Output "[hook] 警告：Hook 同目录与正式安装目录都没有 $IngressExeName，事件无法提交；请把 Hook 放到 ingress 同目录，或设置 AGENT_NOTIFY_INGRESS。"
}
