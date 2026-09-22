#Requires -Version 5.1
<#
.SYNOPSIS
  安装器结构检查：校验安装器是 PE、带产品版本信息，并核对 Inno 脚本的打包契约。

.DESCRIPTION
  默认做通用检查，以及应用内一键升级依赖的静默安装契约：
  Inno 脚本里不得有裸 MsgBox（静默安装会被弹窗卡死），[Run] 段不得带 skipifsilent
  （否则静默安装完成后不会自动重新打开桌面端）。
  加 -ExpectRust 时额外断言「正式包已是 Rust 桌面版」：
  安装桌面端、ingress 与阶段 D 的三个 Hook，保留旧 AppId 与安装目录、自启动指向新桌面程序、
  四个 Agent 适配器按任务接入、卸载会清理自己写入的 Hook / 扩展 / mod，
  不再把旧 Win32 UI 作为启动入口，且安装/卸载都不触碰用户数据。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\installer-smoke.ps1 -Installer .\dist\Agent-notify-Setup-v2.0.0.exe -ExpectRust
#>
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [string]$RepoRoot,
  [switch]$ExpectRust
)

$ErrorActionPreference = 'Stop'
if (-not $RepoRoot) {
  $RepoRoot = Split-Path $PSScriptRoot -Parent
}

if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) {
  throw "安装器不存在：$Installer"
}

$stream = [IO.File]::OpenRead($Installer)
try {
  $reader = New-Object IO.BinaryReader($stream)
  if ($reader.ReadByte() -ne 0x4d -or $reader.ReadByte() -ne 0x5a) {
    throw '安装器不是 Windows PE 文件'
  }
} finally {
  $stream.Dispose()
}

$versionInfo = (Get-Item -LiteralPath $Installer).VersionInfo
if ([string]::IsNullOrWhiteSpace($versionInfo.ProductName)) {
  throw '安装器缺少产品版本信息'
}
if ([string]::IsNullOrWhiteSpace($versionInfo.ProductVersion)) {
  throw '安装器缺少产品版本号'
}

$issPath = Join-Path $RepoRoot 'installer\agent-notify.iss'
if (-not (Test-Path -LiteralPath $issPath -PathType Leaf)) {
  throw "安装器脚本不存在：$issPath"
}
$issueScript = Get-Content -LiteralPath $issPath -Raw -Encoding utf8
$requiredVersionEntry = 'Source: "{#RepoRoot}\VERSION"; DestDir: "{app}"; Flags: ignoreversion'
if (-not $issueScript.Contains($requiredVersionEntry)) {
  throw '安装器脚本未把仓库根 VERSION 安装到 {app}\VERSION'
}

# 真实条目（排除 ; 与 // 注释行）：下面几条结构断言只看会被 Inno 编译的内容。
$issueEntries = (($issueScript -split "\r?\n") | Where-Object { $_ -notmatch '^\s*(;|//)' }) -join "`n"

# 静默安装（应用内一键升级用 /SILENT 拉起）不能被弹窗卡死：Inno 的 MsgBox 在静默安装下
# 依然会显示并等待点击，只有 SuppressibleMsgBox 才可能被抑制，所以脚本里不允许再出现裸 MsgBox。
$bareMsgBox = [regex]::Match($issueEntries, '(?<![\w.])MsgBox\s*\(')
if ($bareMsgBox.Success) {
  throw "安装器脚本仍在使用裸 MsgBox（静默安装会卡在弹窗），请改用 SuppressibleMsgBox：$($bareMsgBox.Value)"
}

# 安装结束必须自动重启桌面端：静默安装时 [Run] 带 skipifsilent 就不会重新打开应用。
$runSection = [regex]::Match($issueScript, '(?ms)^\[Run\](?<body>.*?)(?=^\[|\z)')
if (-not $runSection.Success) {
  throw '安装器脚本缺少 [Run] 段，安装完成后不会自动启动 AgentNotify'
}
if ($runSection.Groups['body'].Value -match 'skipifsilent') {
  throw '[Run] 段带 skipifsilent：静默安装完成后不会自动启动 AgentNotify，应用内一键升级会停在没有界面的状态'
}

if ($ExpectRust) {
  # 正式包必须是 Rust 桌面版：两个可执行文件都要在包里。
  foreach ($needle in @('agentnotify-desktop.exe', 'agentnotify-ingress.exe')) {
    if (-not $issueScript.Contains($needle)) {
      throw "正式包缺少 Rust 可执行文件：$needle"
    }
  }

  # 升级必须落回原安装目录：AppId 与安装目录都要与旧版一致。
  if (-not $issueScript.Contains('AppId={{E7A4419F-499D-4A21-BD12-6C2D1F6B31A4}')) {
    throw '正式包未保留旧 AppId，升级不会落回原安装目录'
  }
  if (-not $issueScript.Contains('DefaultDirName={localappdata}\Programs\Agent-notify')) {
    throw '正式包的标准安装目录与旧版不一致'
  }

  # 旧 Win32 UI 不得再作为启动入口。
  if ($issueScript -match '(?m)^\s*Source:.*agent-notify\.exe') {
    throw '正式包仍把旧 agent-notify.exe 作为安装内容'
  }
  if ($issueScript.Contains('Parameters: "widget"')) {
    throw '正式包仍以 widget 子命令启动旧 Win32 UI'
  }
  if ($issueScript -match 'agent-notify\.exe"?\s+notify') {
    throw '正式包仍引用旧 notify 命令'
  }

  # 自启动必须指向新的桌面程序。
  $startupIcon = [regex]::Match($issueScript, '(?m)^\s*Name:\s*"\{userstartup\}[^"]*";\s*Filename:\s*"([^"]+)"')
  if (-not $startupIcon.Success) {
    throw '正式包缺少开机自启动项'
  }
  if ($startupIcon.Groups[1].Value -notmatch 'agentnotify-desktop\.exe$') {
    throw "自启动未指向 agentnotify-desktop.exe：$($startupIcon.Groups[1].Value)"
  }

  # OpenCode 插件必须绑定 ingress，不能指向旧 notify 命令。
  if (-not $issueScript.Contains('install-opencode-v2.ps1')) {
    throw '正式包未携带 OpenCode V2 插件安装脚本'
  }
  if (-not $issueScript.Contains('-Ingress')) {
    throw 'OpenCode 插件安装未绑定 ingress 可执行文件'
  }

  # Inno 没有 {userprofile} 常量：用户目录必须用 {%USERPROFILE}，写错会在安装末尾抛异常。
  # 只检查真实条目：Inno 的 ; 注释与 [Code] 的 // 注释里可以提到这个名字。
  if ($issueEntries -match '\{userprofile\}') {
    throw '安装器使用了不存在的 {userprofile} 常量，用户目录应写 {%USERPROFILE}'
  }

  # 阶段 D 的四个适配器必须随正式包分发：三个 Hook exe、四个接入脚本、Devin V2 扩展、Command Code V2 mod。
  $adapterPayloads = @(
    'Source: "{#CodexHookPath}"; DestDir: "{app}"; DestName: "agentnotify-codex-hook.exe"',
    'Source: "{#AntigravityHookPath}"; DestDir: "{app}"; DestName: "agentnotify-antigravity-hook.exe"',
    'Source: "{#DevinHookPath}"; DestDir: "{app}"; DestName: "agentnotify-devin-hook.exe"',
    'Source: "{#RepoRoot}\plugin\devin-extension-v2\package.json"; DestDir: "{app}\plugin\devin-extension-v2"',
    'Source: "{#RepoRoot}\plugin\devin-extension-v2\extension.js"; DestDir: "{app}\plugin\devin-extension-v2"',
    'Source: "{#RepoRoot}\plugin\devin-extension-v2\acp-bridge.js"; DestDir: "{app}\plugin\devin-extension-v2"',
    'Source: "{#RepoRoot}\plugin\commandcode-v2\agent-notify.ts"; DestDir: "{app}\plugin\commandcode-v2"'
  )
  foreach ($needle in $adapterPayloads) {
    if (-not $issueScript.Contains($needle)) {
      throw "正式包缺少阶段 D 适配器产物：$needle"
    }
  }
  foreach ($scriptName in @('install-codex-v2.ps1', 'install-antigravity-v2.ps1', 'install-devin-v2.ps1', 'install-commandcode-v2.ps1')) {
    $expectedEntry = 'Source: "{#RepoRoot}\tools\hooks\' + $scriptName + '"; DestDir: "{app}\tools\hooks"'
    if (-not $issueScript.Contains($expectedEntry)) {
      throw "正式包未携带 Agent 接入脚本：$scriptName"
    }
  }

  # 每个 Agent 一个接入任务：默认勾选（与 Go 版"不传 -Skip* 就接入全部"的语义一致），并真的按任务调用。
  foreach ($task in @('opencode', 'codex', 'antigravity', 'devin', 'commandcode')) {
    if ($issueScript -notmatch ('(?m)^Name:\s*"' + $task + '";')) {
      throw "正式包缺少 $task 接入任务"
    }
    if (-not $issueScript.Contains("WizardIsTaskSelected('" + $task + "')")) {
      throw "正式包没有按任务接入 $task"
    }
    if ($issueScript -match ('(?m)^Name:\s*"' + $task + '";[^\r\n]*Flags:\s*unchecked')) {
      throw "接入任务 $task 不应默认取消勾选：Go 版语义是默认接入全部 Agent"
    }
  }

  # 接入调用只允许出现在 [Code] 段，并且必须绑定本次安装目录里的 Hook / 接入脚本。
  $codeIndex = $issueScript.IndexOf('[Code]')
  if ($codeIndex -lt 0) {
    throw '正式包安装器脚本缺少 [Code] 段'
  }
  $codeSection = $issueScript.Substring($codeIndex)
  $integrationBindings = @(
    @{ Script = 'install-codex-v2.ps1'; Hook = 'agentnotify-codex-hook.exe' },
    @{ Script = 'install-antigravity-v2.ps1'; Hook = 'agentnotify-antigravity-hook.exe' },
    @{ Script = 'install-devin-v2.ps1'; Hook = 'agentnotify-devin-hook.exe' }
  )
  foreach ($binding in $integrationBindings) {
    $hookPattern = [regex]::Escape($binding.Script) + '[\s\S]{0,400}?' + [regex]::Escape($binding.Hook)
    if ($codeSection -notmatch $hookPattern) {
      throw "正式包没有把 $($binding.Script) 绑定到 $($binding.Hook)"
    }
    $ingressPattern = [regex]::Escape($binding.Script) + '[\s\S]{0,600}?-Ingress'
    if ($codeSection -notmatch $ingressPattern) {
      throw "正式包没有给 $($binding.Script) 指定 -Ingress"
    }
  }
  foreach ($binding in @(
      @{ Script = 'install-devin-v2.ps1'; Argument = '-ExtensionSource' },
      @{ Script = 'install-commandcode-v2.ps1'; Argument = '-Source' }
    )) {
    $argumentPattern = [regex]::Escape($binding.Script) + '[\s\S]{0,400}?' + [regex]::Escape($binding.Argument)
    if ($codeSection -notmatch $argumentPattern) {
      throw "正式包没有给 $($binding.Script) 指定 $($binding.Argument)"
    }
  }

  # 升级时先清旧 Hook 再写新 Hook：顺序反了会把本次刚写入的配置当成旧 Hook 清掉。
  $curStepMatch = [regex]::Match($codeSection, '(?s)procedure CurStepChanged\(CurStep: TSetupStep\);.*?\r?\nend;')
  if (-not $curStepMatch.Success) {
    throw '正式包安装器脚本缺少 CurStepChanged 过程'
  }
  $curStepBody = $curStepMatch.Value
  $cleanupIndex = $curStepBody.IndexOf('RunLegacyHookCleanup();')
  $codexIndex = $curStepBody.IndexOf("WizardIsTaskSelected('codex')")
  if ($cleanupIndex -lt 0 -or $codexIndex -lt 0) {
    throw '正式包没有在安装后清理旧 Hook 并按任务接入 Codex'
  }
  if ($cleanupIndex -gt $codexIndex) {
    throw '旧 Hook 清理必须在新 Hook 接入之前执行'
  }

  # 卸载必须清掉本次新增的可分发物：程序文件由 Inno 删除，用户目录产物交给既有清理脚本与 [UninstallDelete]。
  if ($issueScript -notmatch '(?m)^\[UninstallRun\]') {
    throw '正式包缺少 [UninstallRun]，卸载不会移除 Hook / 扩展 / mod'
  }
  if (-not $issueScript.Contains('uninstall.ps1"" -HooksOnly')) {
    throw '正式包卸载没有调用既有 Hook 清理脚本（-HooksOnly）'
  }
  if ($issueScript -notmatch '(?m)^\[UninstallDelete\]') {
    throw '正式包缺少 [UninstallDelete]，Devin V2 回复扩展会在卸载后残留'
  }
  foreach ($name in @('package.json', 'extension.js', 'acp-bridge.js')) {
    if (-not $issueScript.Contains('agent-notify-reply-v2\' + $name)) {
      throw "卸载清理缺少 Devin V2 扩展文件：$name"
    }
  }

  # 升级与卸载只允许删除程序文件：不得涉及用户数据、旧迁移源或迁移报告。
  foreach ($pattern in @('AgentNotify\\data', 'AgentNotify\\logs', 'AgentNotify\\spool', 'state\.db', '\.config\\agent-notify')) {
    if ($issueScript -match $pattern) {
      throw "正式包脚本涉及用户数据路径（$pattern），升级或卸载可能删除用户数据"
    }
  }
}

Write-Output '[installer-smoke] 安装器结构检查通过'
Write-Output '[installer-smoke] VERSION 安装清单检查通过'
Write-Output '[installer-smoke] 静默安装契约检查通过（无裸 MsgBox，[Run] 不带 skipifsilent）'
if ($ExpectRust) {
  Write-Output '[installer-smoke] Rust 正式包契约检查通过'
}
