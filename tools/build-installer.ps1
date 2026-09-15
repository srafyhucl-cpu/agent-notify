#Requires -Version 5.1
<#
.SYNOPSIS
  使用 Inno Setup 构建 Agent-notify 标准 Windows 安装器。

.DESCRIPTION
  版本号默认读取 internal/app/version.go，并校验仓库根目录 VERSION 一致。
  ISCC 路径可通过 AGENT_NOTIFY_ISCC 覆盖；设置 AGENT_NOTIFY_SIGNTOOL 后启用签名。
#>
param(
  [string]$Version,
  [string]$OutDir,
  [string]$ExePath
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }
if (-not $ExePath) { $ExePath = Join-Path $RepoRoot 'bin\agent-notify.exe' }

$OutDir = [IO.Path]::GetFullPath($OutDir)
$ExePath = [IO.Path]::GetFullPath($ExePath)
$issPath = Join-Path $RepoRoot 'installer\agent-notify.iss'
$versionPath = Join-Path $RepoRoot 'VERSION'

if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
  throw "找不到待打包的 agent-notify.exe：$ExePath"
}
if (-not (Test-Path -LiteralPath $issPath -PathType Leaf)) {
  throw "找不到 Inno Setup 脚本：$issPath"
}
if (-not (Test-Path -LiteralPath $versionPath -PathType Leaf)) {
  throw "找不到版本文件：$versionPath"
}

$fileVersion = (Get-Content -LiteralPath $versionPath -Raw -Encoding utf8).Trim()
if (-not $Version) {
  $versionFile = Join-Path $RepoRoot 'internal\app\version.go'
  $match = Select-String -Path $versionFile -Pattern 'Version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $match) { throw "无法从 $versionFile 读取应用版本" }
  $Version = $match.Matches[0].Groups[1].Value
}
if ([string]::IsNullOrWhiteSpace($Version)) {
  throw '应用版本不能为空'
}
if ($fileVersion -ne $Version) {
  throw "VERSION 与应用版本不一致：VERSION=$fileVersion，Version=$Version"
}

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
  "/DRepoRoot=$RepoRoot",
  "/DOutputDir=$OutDir",
  "/DExePath=$ExePath"
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

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\installer-smoke.ps1') -Installer $installer
if ($LASTEXITCODE -ne 0) {
  throw "安装器结构检查失败 exit=$LASTEXITCODE"
}

Write-Output "[installer] $installer"
