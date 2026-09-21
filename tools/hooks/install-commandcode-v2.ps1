#Requires -Version 5.1
<#
.SYNOPSIS
  安装 Command Code V2 mod：把事件入口写进 mod，原子部署到用户级 mods 目录。

.DESCRIPTION
  默认路径：
  - mod 源文件：<仓库>\plugin\commandcode-v2\agent-notify.ts
  - mod 目标：%USERPROFILE%\.commandcode\mods\agent-notify.ts
  - 事件入口：%USERPROFILE%\bin\agentnotify-ingress.exe

  保护规则：
  - 目标文件不含 AgentNotify 归属标识（agent-notify-commandcode-mod）时拒绝覆盖，绝不动其他 mod。
  - 覆盖前备份为 agent-notify.ts.bak-agent-notify。
  - 只写这一个文件；不读写 Command Code 的 config.json、auth.json 或会话数据。
  - 目标内容已是最新时不写盘（幂等）。
  - 失败一律抛错，不猜路径、不静默跳过。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-commandcode-v2.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\hooks\install-commandcode-v2.ps1 -ModPath D:\Temp\sandbox\mods\agent-notify.ts -Ingress D:\Temp\app\agentnotify-ingress.exe
#>
param(
  [string]$ModPath = '',
  [string]$Source = '',
  [string]$Ingress = ''
)

$ErrorActionPreference = 'Stop'

# 归属标识：V1 与 V2 mod 共用，升级时据此确认文件由 AgentNotify 创建。
$ModMarker = 'agent-notify-commandcode-mod'
# 安装器把事件入口绝对路径写进这个占位符；占位符缺失说明 mod 源文件已改版。
$BakedIngressPlaceholder = 'const BAKED_INGRESS = ""'
$BackupSuffix = '.bak-agent-notify'
$IngressExeName = 'agentnotify-ingress.exe'

if ([string]::IsNullOrWhiteSpace($ModPath)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $ModPath = Join-Path $homeDirectory '.commandcode\mods\agent-notify.ts'
}
if ([string]::IsNullOrWhiteSpace($Source)) {
  $repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $Source = Join-Path $repoRoot 'plugin\commandcode-v2\agent-notify.ts'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Ingress = Join-Path $homeDirectory "bin\$IngressExeName"
}

$modPathFull = [IO.Path]::GetFullPath($ModPath)
$sourcePathFull = [IO.Path]::GetFullPath($Source)
$ingressPathFull = [IO.Path]::GetFullPath($Ingress)

if (-not (Test-Path -LiteralPath $sourcePathFull -PathType Leaf)) {
  throw "Command Code mod source not found: $sourcePathFull"
}
if (-not (Test-Path -LiteralPath $ingressPathFull -PathType Leaf)) {
  throw "Ingress executable not found: $ingressPathFull"
}
if ([IO.Path]::GetExtension($ingressPathFull) -ine '.exe') {
  throw "Ingress executable must be an .exe file: $ingressPathFull"
}

$sourceContent = [IO.File]::ReadAllText($sourcePathFull)
if (-not $sourceContent.Contains($BakedIngressPlaceholder)) {
  throw "Command Code mod 缺少 BAKED_INGRESS 占位符，安装器不会猜测路径：$sourcePathFull"
}
$escapedIngress = $ingressPathFull.Replace('\', '\\').Replace('"', '\"')
$content = $sourceContent.Replace($BakedIngressPlaceholder, "const BAKED_INGRESS = `"$escapedIngress`"")

if (Test-Path -LiteralPath $modPathFull -PathType Container) {
  throw "Command Code mod 目标路径已被目录占用：$modPathFull"
}

$existed = Test-Path -LiteralPath $modPathFull -PathType Leaf
if ($existed) {
  $existing = [IO.File]::ReadAllText($modPathFull)
  if (-not $existing.Contains($ModMarker)) {
    throw "Command Code mods 目录里的 agent-notify.ts 不是 AgentNotify 部署的 mod（缺少归属标识 $ModMarker），已拒绝覆盖：$modPathFull"
  }
  if ($existing -eq $content) {
    Write-Output "[hook] Command Code V2 mod 已是最新，无需改动：$modPathFull"
    Write-Output "[hook] 事件入口：$ingressPathFull"
    exit 0
  }
}

$modDirectory = Split-Path -Parent $modPathFull
if ([string]::IsNullOrWhiteSpace($modDirectory)) {
  throw "Command Code mod 路径缺少父目录：$modPathFull"
}
New-Item -ItemType Directory -Force -Path $modDirectory | Out-Null

if ($existed) {
  Copy-Item -LiteralPath $modPathFull -Destination "$modPathFull$BackupSuffix" -Force
}

# 原子替换：先写同目录临时文件，再 Move-Item -Force，避免半成品 mod 被 Command Code 加载。
$temporary = Join-Path $modDirectory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
try {
  [IO.File]::WriteAllText($temporary, $content, (New-Object Text.UTF8Encoding($false)))
  Move-Item -LiteralPath $temporary -Destination $modPathFull -Force
} finally {
  if (Test-Path -LiteralPath $temporary) {
    Remove-Item -LiteralPath $temporary -Force
  }
}

if ($existed) {
  Write-Output "[hook] 已更新 Command Code V2 mod：$modPathFull（备份：$modPathFull$BackupSuffix）"
} else {
  Write-Output "[hook] 已安装 Command Code V2 mod：$modPathFull"
}
Write-Output "[hook] 事件入口：$ingressPathFull"
Write-Output '[hook] 回复窗口默认关闭（commandCodeReplyWindowSec = 0）；需要引用回复时再设为 1–600 秒并重启 Command Code。'
