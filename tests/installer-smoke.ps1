#Requires -Version 5.1
<#
.SYNOPSIS
  安装器结构检查：校验安装器是 PE、带产品版本信息，并核对 Inno 脚本的打包契约。

.DESCRIPTION
  默认只做通用检查。加 -ExpectRust 时额外断言「正式包已是 Rust 桌面版」：
  安装两个可执行文件、保留旧 AppId 与安装目录、自启动指向新桌面程序、
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

  # 升级与卸载只允许删除程序文件：不得涉及用户数据、旧迁移源或迁移报告。
  foreach ($pattern in @('AgentNotify\\data', 'AgentNotify\\logs', 'AgentNotify\\spool', 'state\.db', '\.config\\agent-notify')) {
    if ($issueScript -match $pattern) {
      throw "正式包脚本涉及用户数据路径（$pattern），升级或卸载可能删除用户数据"
    }
  }
}

Write-Output '[installer-smoke] 安装器结构检查通过'
Write-Output '[installer-smoke] VERSION 安装清单检查通过'
if ($ExpectRust) {
  Write-Output '[installer-smoke] Rust 正式包契约检查通过'
}
