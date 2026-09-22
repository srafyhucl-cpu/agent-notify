#Requires -Version 5.1
<#
.SYNOPSIS
  卸载脚本的 V2 接入清理回归测试：在隔离沙箱里验证识别范围、保留策略与无备份时的提示。

.DESCRIPTION
  不触碰真实用户目录：把 uninstall.ps1 与 tools\hook-config.ps1 按安装器的目录布局复制到
  %TEMP% 下的 GUID 沙箱，所有配置路径都用显式参数指向沙箱，验证三件事：

  1. V2 接入被清理：Codex notify 行（agentnotify-codex-hook.exe）、Devin hooks.Stop handler、
     Antigravity 顶层 agent-notify 键与启动器、Devin V2 回复扩展、Command Code mod。
  2. 只清理 AgentNotify 自己的条目：第三方 notify 程序、自定义 matcher、同组其他 handler、
     其他顶层键与目录里的其他文件都原样保留；归属校验不通过时保持原样。
  3. 先清理再安装（安装器升级顺序）不会留下残局：清完旧 Hook 后，接入脚本仍能按当前安装目录
     重新写回 Codex / Antigravity / Devin 的接入配置。
  4. 没有 .bak-notify-wrapper 可还原时打印可照做的说明，不静默删除、不猜测原值。
#>
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
$utf8NoBom = New-Object Text.UTF8Encoding($false)

function Assert-True {
  param([bool]$Condition, [string]$Message)
  if (-not $Condition) { throw $Message }
}

function Assert-Contains {
  param([string]$Text, [string]$Needle, [string]$Message)
  if (-not $Text.Contains($Needle)) {
    throw "$Message（输出里没有：$Needle）`n--- 卸载输出 ---`n$Text"
  }
}

function Assert-NotContains {
  param([string]$Text, [string]$Needle, [string]$Message)
  if ($Text.Contains($Needle)) {
    throw "$Message（输出里不应出现：$Needle）`n--- 卸载输出 ---`n$Text"
  }
}

function Write-JsonFile {
  param([string]$Path, $Value)
  $parent = Split-Path -Parent $Path
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
  [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 32), $utf8NoBom)
}

function Get-NotifyLine {
  param([string]$Path)
  return [regex]::Match([IO.File]::ReadAllText($Path), '(?m)^notify\s*=.*$').Value.Trim()
}

function Get-DevinCommands {
  param([string]$Path)
  $config = Get-Content -LiteralPath $Path -Raw -Encoding utf8 | ConvertFrom-Json
  return @($config.hooks.Stop | ForEach-Object { @($_.hooks) | ForEach-Object { [string]$_.command } })
}

# 运行沙箱里的卸载脚本；$ExtraArguments 用来覆盖沙箱路径与开关。
function Invoke-SandboxUninstall {
  param(
    [Parameter(Mandatory = $true)][string]$ScriptPath,
    [Parameter(Mandatory = $true)][hashtable]$Paths,
    [string[]]$ExtraArguments = @()
  )
  $arguments = @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $ScriptPath,
    '-InstallDir', $Paths.InstallDir,
    '-PluginDir', $Paths.PluginDir,
    '-DevinExtensionDir', $Paths.DevinExtensionDir,
    '-DevinExtensionV2Dir', $Paths.DevinExtensionV2Dir,
    '-CommandCodeModDir', $Paths.CommandCodeModDir,
    '-CodexConfig', $Paths.CodexConfig,
    '-AntigravityHooks', $Paths.AntigravityHooks,
    '-DevinConfig', $Paths.DevinConfig,
    '-SkipShortcuts', '-SkipProcessStop'
  ) + $ExtraArguments
  $text = @(& powershell @arguments 2>&1) -join "`n"
  $exitCode = $LASTEXITCODE
  if ($exitCode -ne 0) {
    throw "沙箱卸载失败 exit=$exitCode`n$text"
  }
  return $text
}

$sandboxRoot = Join-Path ([IO.Path]::GetTempPath()) ('agent-notify-uninstall-v2-' + [guid]::NewGuid().ToString('N'))
$appDir = Join-Path $sandboxRoot 'app'
$tempDir = Join-Path $sandboxRoot 'temp'
$sandboxScript = Join-Path $appDir 'uninstall.ps1'
New-Item -ItemType Directory -Force -Path (Join-Path $appDir 'tools'), $tempDir, (Join-Path $tempDir 'agent-notify') | Out-Null
Copy-Item -LiteralPath (Join-Path $RepoRoot 'uninstall.ps1') -Destination $sandboxScript -Force
Copy-Item -LiteralPath (Join-Path $RepoRoot 'tools\hook-config.ps1') -Destination (Join-Path $appDir 'tools\hook-config.ps1') -Force
# 卸载脚本第 7 步清理退出标记：把 TEMP 指到沙箱，避免碰真实 %TEMP%。
$previousTemp = $env:TEMP
$previousTmp = $env:TMP
$env:TEMP = $tempDir
$env:TMP = $tempDir

function New-ScenarioDir {
  param([string]$Name)
  $dir = Join-Path $sandboxRoot $Name
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  return $dir
}

function New-SandboxPaths {
  param([Parameter(Mandatory = $true)][string]$Scenario)
  $paths = @{
    InstallDir          = Join-Path $Scenario 'install-bin'
    PluginDir           = Join-Path $Scenario 'opencode-plugins'
    DevinExtensionDir   = Join-Path $Scenario 'devin-extension-v1'
    DevinExtensionV2Dir = Join-Path $Scenario 'devin-extension-v2'
    CommandCodeModDir   = Join-Path $Scenario 'commandcode-mods'
    CodexConfig         = Join-Path $Scenario 'codex\config.toml'
    AntigravityHooks    = Join-Path $Scenario 'gemini\hooks.json'
    DevinConfig         = Join-Path $Scenario 'devin\config.json'
  }
  foreach ($key in @('InstallDir', 'PluginDir', 'DevinExtensionDir', 'DevinExtensionV2Dir', 'CommandCodeModDir')) {
    New-Item -ItemType Directory -Force -Path $paths[$key] | Out-Null
  }
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $paths.CodexConfig) | Out-Null
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $paths.AntigravityHooks) | Out-Null
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $paths.DevinConfig) | Out-Null
  return $paths
}

$hooksOnly = @('-HooksOnly')

try {
  # 1. V2 接入全量清理（卸载器实际使用的 -HooksOnly 路径）
  $scenario = New-ScenarioDir 'v2-removal'
  $paths = New-SandboxPaths -Scenario $scenario
  $codexHook = Join-Path $appDir 'agentnotify-codex-hook.exe'
  $devinHook = Join-Path $appDir 'agentnotify-devin-hook.exe'
  $antigravityHook = Join-Path $appDir 'agentnotify-antigravity-hook.exe'

  # Codex：V2 Hook 直连 + 备份里是用户自己的包装，卸载必须按备份原文还原。
  [IO.File]::WriteAllText(
    $paths.CodexConfig,
    "model = `"gpt-5`"`r`nnotify = [ `"$($codexHook.Replace('\', '/'))`", `"codex`", `"turn-ended`" ]`r`n",
    $utf8NoBom)
  $backupLine = 'notify = [ "C:/third/linkweixin-notify.exe", "turn-ended" ]'
  [IO.File]::WriteAllText("$($paths.CodexConfig).bak-notify-wrapper", "$backupLine`r`n", $utf8NoBom)

  # Devin：自定义 matcher / 同组其他 handler / 其他事件必须保留，只摘掉 V2 与旧版 hook。
  Write-JsonFile -Path $paths.DevinConfig -Value ([ordered]@{
      version     = 1
      permissions = [ordered]@{ allow = @('Exec(ls)') }
      hooks       = [ordered]@{
        Stop         = @(
          [ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = 'other.exe devin'; timeout = 10 }) },
          [ordered]@{
            matcher = 'custom-matcher'
            hooks   = @(
              [ordered]@{ type = 'command'; command = '"' + $devinHook + '" devin stop'; timeout = 60 },
              [ordered]@{ type = 'command'; command = 'keep.exe' }
            )
          },
          [ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = '"C:\old\agent-notify.exe" devin stop'; timeout = 1 }) }
        )
        SessionStart = @([ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = 'session-start.exe' }) })
      }
    })

  # Antigravity：顶层 agent-notify 键（启动器命令 + V2 Hook 直连各一条）与启动器要清掉，第三方顶层键保留。
  Write-JsonFile -Path $paths.AntigravityHooks -Value ([ordered]@{
      'linkweixin-notify' = [ordered]@{
        Stop = @([ordered]@{ type = 'command'; command = 'other.exe antigravity'; timeout = 30 })
      }
      keep                = [ordered]@{ value = 42 }
      'agent-notify'      = [ordered]@{
        Stop = @(
          [ordered]@{ type = 'command'; command = '.\agent-notify-hook.cmd antigravity stop'; timeout = 60 },
          [ordered]@{ type = 'command'; command = '"' + $antigravityHook + '" antigravity stop'; timeout = 60 }
        )
      }
    })
  $launcherPath = Join-Path (Split-Path -Parent $paths.AntigravityHooks) 'agent-notify-hook.cmd'
  [IO.File]::WriteAllText(
    $launcherPath,
    "@echo off`r`n@rem agent-notify-antigravity-launcher`r`n`"$antigravityHook`" antigravity stop`r`n",
    $utf8NoBom)

  # Devin V2 扩展：三个文件删掉，目录里的其他文件与目录本身保留。
  New-Item -ItemType Directory -Force -Path $paths.DevinExtensionV2Dir | Out-Null
  foreach ($name in @('package.json', 'extension.js', 'acp-bridge.js')) {
    Copy-Item -LiteralPath (Join-Path $RepoRoot "plugin\devin-extension-v2\$name") -Destination (Join-Path $paths.DevinExtensionV2Dir $name) -Force
  }
  [IO.File]::WriteAllText((Join-Path $paths.DevinExtensionV2Dir 'keep.txt'), 'keep-extension-data', $utf8NoBom)

  # Command Code mod：带归属标识的 agent-notify.ts 删除，同目录其他 mod 保留。
  New-Item -ItemType Directory -Force -Path $paths.CommandCodeModDir | Out-Null
  Copy-Item -LiteralPath (Join-Path $RepoRoot 'plugin\commandcode-v2\agent-notify.ts') -Destination (Join-Path $paths.CommandCodeModDir 'agent-notify.ts') -Force
  [IO.File]::WriteAllText((Join-Path $paths.CommandCodeModDir 'mine.ts'), '// 用户自己的 mod', $utf8NoBom)

  # 退出标记：沙箱里预置一份，卸载脚本应当删掉它。
  $exitMarker = Join-Path $tempDir 'agent-notify\widget-exit.txt'
  [IO.File]::WriteAllText($exitMarker, 'exit', $utf8NoBom)

  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly

  $restoredCodex = [IO.File]::ReadAllText($paths.CodexConfig)
  Assert-True ((Get-NotifyLine $paths.CodexConfig) -eq $backupLine) "卸载未按备份原文还原 Codex notify：$(Get-NotifyLine $paths.CodexConfig)"
  Assert-True ($restoredCodex -match '(?m)^model = "gpt-5"\s*$') 'Codex 备份还原破坏了 other 配置'
  Assert-True ($restoredCodex -notmatch '(?i)agentnotify') '卸载后 Codex notify 仍指向 V2 Hook'
  Assert-True (-not (Test-Path -LiteralPath "$($paths.CodexConfig).bak-notify-wrapper")) '卸载后残留 Codex 备份文件'

  $devinCommands = Get-DevinCommands -Path $paths.DevinConfig
  Assert-True (@($devinCommands | Where-Object { $_ -match '(?i)agentnotify-devin-hook\.exe|agent-notify\.exe' }).Count -eq 0) "卸载后残留 Devin Hook：$($devinCommands -join ' | ')"
  Assert-True ($devinCommands -contains 'other.exe devin') '卸载误删第三方 Devin handler'
  Assert-True ($devinCommands -contains 'keep.exe') '卸载误删同组其他 handler'
  $devinConfig = Get-Content -LiteralPath $paths.DevinConfig -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True (@($devinConfig.hooks.Stop).Count -eq 2) "卸载没有丢掉整个 AgentNotify 分组：$(@($devinConfig.hooks.Stop).Count)"
  Assert-True (@($devinConfig.hooks.Stop)[1].matcher -eq 'custom-matcher') '卸载改写了自定义 matcher'
  Assert-True (@($devinConfig.hooks.SessionStart).Count -eq 1) '卸载误删 Devin 其他事件 Hook'
  Assert-True (@($devinConfig.permissions.allow) -contains 'Exec(ls)') '卸载误删 Devin 权限配置'

  $antigravityConfig = Get-Content -LiteralPath $paths.AntigravityHooks -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True ($null -eq $antigravityConfig.PSObject.Properties['agent-notify']) '卸载后残留 Antigravity 顶层 agent-notify 键'
  Assert-True (@($antigravityConfig.'linkweixin-notify'.Stop)[0].command -eq 'other.exe antigravity') '卸载误删第三方 Antigravity Hook'
  Assert-True ($antigravityConfig.keep.value -eq 42) '卸载误删 Antigravity 无关配置'
  Assert-True (-not (Test-Path -LiteralPath $launcherPath)) '卸载后残留 Antigravity 启动器'

  foreach ($name in @('package.json', 'extension.js', 'acp-bridge.js')) {
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $paths.DevinExtensionV2Dir $name))) "卸载后残留 Devin V2 扩展文件：$name"
  }
  Assert-True (Test-Path -LiteralPath (Join-Path $paths.DevinExtensionV2Dir 'keep.txt')) '卸载误删 Devin V2 扩展目录里的其他文件'
  Assert-True (-not (Test-Path -LiteralPath (Join-Path $paths.CommandCodeModDir 'agent-notify.ts'))) '卸载后残留 Command Code mod'
  Assert-True (Test-Path -LiteralPath (Join-Path $paths.CommandCodeModDir 'mine.ts')) '卸载误删用户自己的 Command Code mod'
  Assert-True (-not (Test-Path -LiteralPath $exitMarker)) '卸载没有清理退出标记'
  Write-Output '[ok] V2 接入被清理，第三方条目保留'

  # 2. 归属校验不通过时保持原样：扩展与 mod 都不属于 AgentNotify。
  $scenario = New-ScenarioDir 'foreign-ownership'
  $paths = New-SandboxPaths -Scenario $scenario
  New-Item -ItemType Directory -Force -Path $paths.DevinExtensionV2Dir, $paths.CommandCodeModDir | Out-Null
  [IO.File]::WriteAllText((Join-Path $paths.DevinExtensionV2Dir 'package.json'), '{"name":"someone-else","publisher":"other"}', $utf8NoBom)
  [IO.File]::WriteAllText((Join-Path $paths.DevinExtensionV2Dir 'extension.js'), 'other', $utf8NoBom)
  [IO.File]::WriteAllText((Join-Path $paths.CommandCodeModDir 'agent-notify.ts'), '// 不是 AgentNotify 部署的 mod', $utf8NoBom)
  [IO.File]::WriteAllText($paths.CodexConfig, "notify = [ `"C:/third/linkweixin-notify.exe`" ]`r`n", $utf8NoBom)
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly

  Assert-Contains $output 'Devin V2 扩展归属校验失败，保持原样' '卸载未提示扩展归属校验失败'
  Assert-Contains $output 'Command Code mod 归属校验失败，保持原样' '卸载未提示 mod 归属校验失败'
  Assert-Contains $output 'Codex notify 未指向 AgentNotify，保持原样。' '卸载误判第三方 notify 行'
  Assert-True (Test-Path -LiteralPath (Join-Path $paths.DevinExtensionV2Dir 'package.json')) '卸载误删其他扩展的 package.json'
  Assert-True (Test-Path -LiteralPath (Join-Path $paths.DevinExtensionV2Dir 'extension.js')) '卸载误删其他扩展的入口文件'
  Assert-True (Test-Path -LiteralPath (Join-Path $paths.CommandCodeModDir 'agent-notify.ts')) '卸载误删用户自己的同名 mod'
  Assert-True ((Get-NotifyLine $paths.CodexConfig) -eq 'notify = [ "C:/third/linkweixin-notify.exe" ]') '卸载改写了第三方 notify 行'

  # 名字里带 agent-notify 的第三方程序不是 AgentNotify 的入口：识别必须精确，文件一个字都不能改。
  $foreignCodex = Join-Path $scenario 'codex\foreign-agent-notify.toml'
  $foreignContent = 'notify = [ "C:/tools/my-agent-notify-helper.exe", "codex", "turn-ended" ]' + "`r`n"
  [IO.File]::WriteAllText($foreignCodex, $foreignContent, $utf8NoBom)
  $foreignPaths = $paths.Clone()
  $foreignPaths.CodexConfig = $foreignCodex
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $foreignPaths -ExtraArguments $hooksOnly
  Assert-Contains $output 'Codex notify 未指向 AgentNotify，保持原样。' '卸载把名字里带 agent-notify 的第三方程序当成自己的入口'
  Assert-True ([IO.File]::ReadAllText($foreignCodex) -eq $foreignContent) '卸载改写了名字里带 agent-notify 的第三方 notify 行'
  Write-Output '[ok] 归属校验失败的扩展 / mod / notify 保持原样'

  # 3. 没有备份可还原时的三种情况：还原载荷、整行移除并给出说明、载荷无法解析时保留并警告。
  $scenario = New-ScenarioDir 'no-backup'
  $paths = New-SandboxPaths -Scenario $scenario

  # 3a. 载荷是用户自己的 notify 数组：按载荷原文还原。
  [IO.File]::WriteAllText(
    $paths.CodexConfig,
    'notify = [ "' + $codexHook.Replace('\', '/') + '", "codex", "turn-ended", "--previous-notify", "[\"C:/third/linkweixin-notify.exe\",\"turn-ended\"]" ]' + "`r`n",
    $utf8NoBom)
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly
  $restored = Get-NotifyLine $paths.CodexConfig
  Assert-True ($restored -eq 'notify = [ "C:/third/linkweixin-notify.exe", "turn-ended" ]') "无备份时未按载荷还原上游 notify：$restored"
  Assert-Contains $output '已按 --previous-notify 载荷还原原上游 notify' '无备份还原没有给出说明'
  Assert-Contains $output '没有可还原的备份' '无备份时缺少可照做的说明'

  # 3b. 直连 V2 Hook 且没有载荷：整行移除，并说明被移除的原文。
  [IO.File]::WriteAllText(
    $paths.CodexConfig,
    'model = "gpt-5"' + "`r`n" + 'notify = [ "' + $codexHook.Replace('\', '/') + '", "codex", "turn-ended" ]' + "`r`n",
    $utf8NoBom)
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly
  $text = [IO.File]::ReadAllText($paths.CodexConfig)
  Assert-True ($text -notmatch '(?m)^notify') '卸载后残留 AgentNotify notify 行'
  Assert-True ($text -match '(?m)^model = "gpt-5"\s*$') '整行移除破坏了其他配置'
  Assert-Contains $output '已移除 config.toml 里的 AgentNotify notify 行' '整行移除没有输出说明'
  Assert-Contains $output '没有可还原的备份' '无备份时缺少可照做的说明'
  Assert-Contains $output '被移除的原文是：notify = [ ' '无备份时没有给出被移除的原文'

  # 3c. 载荷含嵌套链，无法自动还原：原样保留并明确警告。
  [IO.File]::WriteAllText(
    $paths.CodexConfig,
    'notify = [ "' + $codexHook.Replace('\', '/') + '", "codex", "turn-ended", "--previous-notify", "[\"C:/tools/codex-computer-use.exe\",\"turn-ended\",\"--previous-notify\",\"[\"C:/third/x.exe\"]\"]" ]' + "`r`n",
    $utf8NoBom)
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly
  Assert-Contains $output '无法自动还原，已原样保留' '嵌套载荷没有给出人工处理说明'
  Assert-True ((Get-NotifyLine $paths.CodexConfig) -notmatch '(?i)agentnotify') '嵌套载荷情况下仍残留 V2 Hook'
  Write-Output '[ok] 无备份时给出可照做的说明'

  # 4. 升级顺序：先清理旧 Hook，再按当前安装目录接入新 Hook，不留下残局。
  $scenario = New-ScenarioDir 'upgrade-order'
  $paths = New-SandboxPaths -Scenario $scenario
  $hooksDir = Join-Path $scenario 'new-hooks'
  New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null
  foreach ($name in @('agentnotify-codex-hook.exe', 'agentnotify-antigravity-hook.exe', 'agentnotify-devin-hook.exe', 'agentnotify-ingress.exe')) {
    [IO.File]::WriteAllText((Join-Path $hooksDir $name), 'stub', $utf8NoBom)
  }

  # 模拟升级前的状态：V2 Hook 已接入（备份里是接入前的配置，没有 notify 行）。
  [IO.File]::WriteAllText(
    $paths.CodexConfig,
    'model = "gpt-5"' + "`r`n" + 'notify = [ "' + (Join-Path $hooksDir 'agentnotify-codex-hook.exe').Replace('\', '/') + '", "codex", "turn-ended" ]' + "`r`n",
    $utf8NoBom)
  [IO.File]::WriteAllText("$($paths.CodexConfig).bak-notify-wrapper", 'model = "gpt-5"' + "`r`n", $utf8NoBom)
  Write-JsonFile -Path $paths.DevinConfig -Value ([ordered]@{
      hooks = [ordered]@{
        Stop = @(
          [ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = 'other.exe devin' }) },
          [ordered]@{ matcher = ''; hooks = @([ordered]@{ type = 'command'; command = '"' + (Join-Path $hooksDir 'agentnotify-devin-hook.exe') + '" devin stop'; timeout = 60 }) }
        )
      }
    })
  Write-JsonFile -Path $paths.AntigravityHooks -Value ([ordered]@{
      'agent-notify' = [ordered]@{ Stop = @([ordered]@{ type = 'command'; command = '.\agent-notify-hook.cmd antigravity stop'; timeout = 60 }) }
    })
  [IO.File]::WriteAllText(
    (Join-Path (Split-Path -Parent $paths.AntigravityHooks) 'agent-notify-hook.cmd'),
    "@echo off`r`n@rem agent-notify-antigravity-launcher`r`n`"$(Join-Path $hooksDir 'agentnotify-antigravity-hook.exe')`" antigravity stop`r`n",
    $utf8NoBom)

  # 安装器顺序：RunLegacyHookCleanup（-HooksOnly）在前，接入脚本在后。
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly
  Assert-True ((Get-NotifyLine $paths.CodexConfig) -notmatch '(?i)agentnotify') '升级清理没有清掉旧的 V2 Hook 行'

  $installOutputs = @()
  $installOutputs += @(& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\hooks\install-codex-v2.ps1') `
      -HookPath (Join-Path $hooksDir 'agentnotify-codex-hook.exe') `
      -ConfigPath $paths.CodexConfig `
      -Ingress (Join-Path $hooksDir 'agentnotify-ingress.exe') 2>&1) -join "`n"
  Assert-True ($LASTEXITCODE -eq 0) "升级后 Codex 接入失败：$installOutputs"
  $installOutputs += @(& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\hooks\install-devin-v2.ps1') `
      -HookPath (Join-Path $hooksDir 'agentnotify-devin-hook.exe') `
      -ConfigPath $paths.DevinConfig `
      -ExtensionDir $paths.DevinExtensionV2Dir `
      -ExtensionSource (Join-Path $RepoRoot 'plugin\devin-extension-v2') `
      -Ingress (Join-Path $hooksDir 'agentnotify-ingress.exe') 2>&1) -join "`n"
  Assert-True ($LASTEXITCODE -eq 0) "升级后 Devin 接入失败：$installOutputs"
  $installOutputs += @(& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\hooks\install-antigravity-v2.ps1') `
      -HookPath (Join-Path $hooksDir 'agentnotify-antigravity-hook.exe') `
      -HooksPath $paths.AntigravityHooks `
      -Ingress (Join-Path $hooksDir 'agentnotify-ingress.exe') 2>&1) -join "`n"
  Assert-True ($LASTEXITCODE -eq 0) "升级后 Antigravity 接入失败：$installOutputs"

  $codexLine = Get-NotifyLine $paths.CodexConfig
  Assert-True ($codexLine -match [regex]::Escape((Join-Path $hooksDir 'agentnotify-codex-hook.exe').Replace('\', '/'))) "升级后 Codex notify 未指向新 Hook：$codexLine"
  Assert-True ($codexLine -eq ('notify = [ "' + (Join-Path $hooksDir 'agentnotify-codex-hook.exe').Replace('\', '/') + '", "codex", "turn-ended" ]')) "升级后 Codex notify 行不干净：$codexLine"
  $devinCommands = Get-DevinCommands -Path $paths.DevinConfig
  Assert-True (@($devinCommands | Where-Object { $_ -match '(?i)agentnotify-devin-hook\.exe' }).Count -eq 1) "升级后 Devin Hook 未写回：$($devinCommands -join ' | ')"
  Assert-True ($devinCommands -contains 'other.exe devin') '升级接入误删第三方 Devin handler'
  $antigravityConfig = Get-Content -LiteralPath $paths.AntigravityHooks -Raw -Encoding utf8 | ConvertFrom-Json
  Assert-True (@($antigravityConfig.'agent-notify'.Stop)[0].command -eq '.\agent-notify-hook.cmd antigravity stop') '升级后 Antigravity 顶层键未写回'
  Assert-True (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $paths.AntigravityHooks) 'agent-notify-hook.cmd')) '升级后 Antigravity 启动器未写回'
  foreach ($name in @('package.json', 'extension.js', 'acp-bridge.js')) {
    Assert-True (Test-Path -LiteralPath (Join-Path $paths.DevinExtensionV2Dir $name)) "升级后 Devin V2 扩展未写回：$name"
  }
  Write-Output '[ok] 先清理再安装，接入仍能写回且不丢用户配置'

  # 5. 升级前的 Codex 是「codex-computer-use + --previous-notify 载荷」：清理后重新接入必须
  #    保留用户原有的 --previous-notify 载荷（install-codex-v2.ps1 按设计把 CUA 换成 Hook 直连）。
  $scenario = New-ScenarioDir 'upgrade-cua-payload'
  $paths = New-SandboxPaths -Scenario $scenario
  $hooksDir = Join-Path $scenario 'new-hooks'
  New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null
  foreach ($name in @('agentnotify-codex-hook.exe', 'agentnotify-ingress.exe')) {
    [IO.File]::WriteAllText((Join-Path $hooksDir $name), 'stub', $utf8NoBom)
  }
  $cuaLine = 'notify = [ "C:/tools/codex-computer-use.exe", "turn-ended", "--previous-notify", "[\"C:/third/linkweixin-notify.exe\",\"turn-ended\"]" ]'
  [IO.File]::WriteAllText(
    $paths.CodexConfig,
    'notify = [ "' + (Join-Path $hooksDir 'agentnotify-codex-hook.exe').Replace('\', '/') + '", "codex", "turn-ended" ]' + "`r`n",
    $utf8NoBom)
  [IO.File]::WriteAllText("$($paths.CodexConfig).bak-notify-wrapper", "$cuaLine`r`n", $utf8NoBom)
  $output = Invoke-SandboxUninstall -ScriptPath $sandboxScript -Paths $paths -ExtraArguments $hooksOnly
  Assert-True ((Get-NotifyLine $paths.CodexConfig) -eq $cuaLine) '升级清理没有按备份还原用户的 CUA notify 行'
  $installOutput = @(& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\hooks\install-codex-v2.ps1') `
      -HookPath (Join-Path $hooksDir 'agentnotify-codex-hook.exe') `
      -ConfigPath $paths.CodexConfig `
      -Ingress (Join-Path $hooksDir 'agentnotify-ingress.exe') 2>&1) -join "`n"
  Assert-True ($LASTEXITCODE -eq 0) "升级后 Codex 接入失败：$installOutput"
  $codexLine = Get-NotifyLine $paths.CodexConfig
  Assert-True ($codexLine -match [regex]::Escape((Join-Path $hooksDir 'agentnotify-codex-hook.exe').Replace('\', '/'))) "升级后 Codex notify 未指向新 Hook：$codexLine"
  Assert-True ($codexLine.Contains('--previous-notify')) "升级后 Codex 丢失了用户原有的 --previous-notify 载荷：$codexLine"
  Assert-True ($codexLine.Contains('linkweixin-notify.exe')) "升级后 Codex 载荷里的第三方程序丢失：$codexLine"
  Write-Output '[ok] 升级保留用户原有的 --previous-notify 载荷'
} finally {
  $env:TEMP = $previousTemp
  $env:TMP = $previousTmp
  if (Test-Path -LiteralPath $sandboxRoot) {
    $resolved = [IO.Path]::GetFullPath($sandboxRoot)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $insideTemp = $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)
    $expectedName = (Split-Path $resolved -Leaf) -match '^agent-notify-uninstall-v2-[0-9a-f]{32}$'
    if ($insideTemp -and $expectedName) {
      Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    } else {
      Write-Warning "拒绝清理不符合命名约束的沙箱目录：$resolved"
    }
  }
}

Write-Output 'UNINSTALL V2 CLEANUP ALL GREEN'
