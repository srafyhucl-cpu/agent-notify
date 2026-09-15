#Requires -Version 5.1
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [string]$RepoRoot
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

Write-Output '[installer-smoke] 安装器结构检查通过'
Write-Output '[installer-smoke] VERSION 安装清单检查通过'
