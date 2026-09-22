#Requires -Version 5.1
<#
.SYNOPSIS
  使用 Inno Setup 构建 AgentNotify 标准 Windows 安装器（Tauri 桌面版）。

.DESCRIPTION
  版本号来自仓库根 VERSION（唯一来源），并校验调用方传入的 -Version 与之一致。
  需要传入桌面端、ingress 与阶段 D 的三个 Hook 可执行文件；ISCC 路径可通过
  AGENT_NOTIFY_ISCC 覆盖；设置 AGENT_NOTIFY_SIGNTOOL 后启用签名，并强制校验指纹
  等于内置信任指纹。
#>
param(
  [string]$Version,
  [string]$OutDir,
  [string]$ExePath,
  [string]$IngressPath,
  [string]$CodexHookPath,
  [string]$AntigravityHookPath,
  [string]$DevinHookPath
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'signature-common.ps1')
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }

$OutDir = [IO.Path]::GetFullPath($OutDir)
$issPath = Join-Path $RepoRoot 'installer\agent-notify.iss'
$versionPath = Join-Path $RepoRoot 'VERSION'

if (-not (Test-Path -LiteralPath $issPath -PathType Leaf)) {
  throw "找不到 Inno Setup 脚本：$issPath"
}
if (-not (Test-Path -LiteralPath $versionPath -PathType Leaf)) {
  throw "找不到版本文件：$versionPath"
}

$fileVersion = (Get-Content -LiteralPath $versionPath -Raw -Encoding utf8).Trim()
if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = $fileVersion
}
$Version = $Version.Trim().TrimStart('v')
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "版本号格式无效：$Version（期望 x.y.z）"
}
if ($fileVersion -ne $Version) {
  throw "VERSION 与调用方版本不一致：VERSION=$fileVersion，Version=$Version"
}

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  throw "缺少 -ExePath：正式安装器必须打包 agentnotify-desktop.exe"
}
if ([string]::IsNullOrWhiteSpace($IngressPath)) {
  throw "缺少 -IngressPath：正式安装器必须打包 agentnotify-ingress.exe"
}
# 阶段 D 的三个 Hook 也必须随包分发：安装器按任务调用对应接入脚本，缺文件就装不出可用 Hook。
$hookParameters = [ordered]@{
  '-CodexHookPath'       = $CodexHookPath
  '-AntigravityHookPath' = $AntigravityHookPath
  '-DevinHookPath'       = $DevinHookPath
}
foreach ($entry in $hookParameters.GetEnumerator()) {
  if ([string]::IsNullOrWhiteSpace($entry.Value)) {
    throw "缺少 $($entry.Key)：正式安装器必须打包阶段 D 的 Hook 可执行文件"
  }
}
$ExePath = [IO.Path]::GetFullPath($ExePath)
$IngressPath = [IO.Path]::GetFullPath($IngressPath)
$CodexHookPath = [IO.Path]::GetFullPath($CodexHookPath)
$AntigravityHookPath = [IO.Path]::GetFullPath($AntigravityHookPath)
$DevinHookPath = [IO.Path]::GetFullPath($DevinHookPath)
foreach ($leaf in @($ExePath, $IngressPath, $CodexHookPath, $AntigravityHookPath, $DevinHookPath)) {
  if (-not (Test-Path -LiteralPath $leaf -PathType Leaf)) {
    throw "找不到待打包的可执行文件：$leaf"
  }
}

# Windows 文件版本必须是四段数字。
$versionParts = $Version.Split('.')
$VersionInfo = "$($versionParts[0]).$($versionParts[1]).$($versionParts[2]).0"

function Resolve-Iscc {
  if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_ISCC)) {
    if (-not (Test-Path -LiteralPath $env:AGENT_NOTIFY_ISCC -PathType Leaf)) {
      throw "AGENT_NOTIFY_ISCC 指向的文件不存在：$env:AGENT_NOTIFY_ISCC"
    }
    return (Resolve-Path -LiteralPath $env:AGENT_NOTIFY_ISCC).Path
  }

  $command = Get-Command iscc.exe -ErrorAction SilentlyContinue
  if ($command) { return $command.Source }

  $candidates = @(
    'D:\Temp\InnoSetup\ISCC.exe',
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
  )
  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }
  return $null
}

$iscc = Resolve-Iscc
if (-not $iscc) {
  throw '找不到 ISCC.exe。请安装 Inno Setup 6，或设置 AGENT_NOTIFY_ISCC。'
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$installer = Join-Path $OutDir "Agent-notify-Setup-v$Version.exe"
if (Test-Path -LiteralPath $installer -PathType Leaf) {
  [IO.File]::Delete($installer)
}

$isccArgs = @(
  "/DAppVersion=$Version",
  "/DVersionInfo=$VersionInfo",
  "/DRepoRoot=$RepoRoot",
  "/DOutputDir=$OutDir",
  "/DExePath=$ExePath",
  "/DIngressPath=$IngressPath",
  "/DCodexHookPath=$CodexHookPath",
  "/DAntigravityHookPath=$AntigravityHookPath",
  "/DDevinHookPath=$DevinHookPath"
)

if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_SIGNTOOL)) {
  $signTool = $env:AGENT_NOTIFY_SIGNTOOL.Trim()
  if (Test-Path -LiteralPath $signTool -PathType Leaf) {
    $signTool = (Resolve-Path -LiteralPath $signTool).Path
  } else {
    $signCommand = Get-Command $signTool -ErrorAction SilentlyContinue
    if (-not $signCommand) {
      throw "找不到签名工具：$signTool"
    }
    $signTool = $signCommand.Source
  }
  $isccArgs += '/DSignToolCommand=1'
  $isccArgs += ('/Sagentnotify="{0}" sign "$f"' -f $signTool)
}

& $iscc @isccArgs $issPath
if ($LASTEXITCODE -ne 0) {
  throw "Inno Setup 构建失败 exit=$LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
  throw "安装器未生成：$installer"
}

# 配了签名工具就必须真的签上，且指纹必须等于客户端内置的信任指纹，否则直接失败，
# 避免发出未签名包，或发出客户端会判为"签名者不匹配"的包。
if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_SIGNTOOL)) {
  $expectedThumbprint = Get-ExpectedSignatureThumbprint -RepoRoot $RepoRoot
  $actualThumbprint = Get-VerifiedSignatureThumbprint -Path $installer -ExpectedThumbprint $expectedThumbprint
  Write-Output "[installer] 安装器签名校验通过（$actualThumbprint）"
}

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\installer-smoke.ps1') -Installer $installer -ExpectRust
if ($LASTEXITCODE -ne 0) {
  throw "安装器结构检查失败 exit=$LASTEXITCODE"
}

Write-Output "[installer] $installer"
