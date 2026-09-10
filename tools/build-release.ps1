#Requires -Version 5.1
<#
.SYNOPSIS
  构建发布包：linkWeixin-v<版本>.zip + SHA256SUMS.txt（本地与 CI 共用）。

.DESCRIPTION
  版本号默认读 src/lib/LinkWeixin/LinkWeixin.psd1 的 ModuleVersion（单一来源）。
  打包内容：install.ps1 / uninstall.ps1 / src/** / plugin/** / LICENSE /
  README.md / CHANGELOG.md / .env.example；解压后根目录即可安装。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-release.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-release.ps1 -Version 0.1.0 -OutDir dist
#>
param(
  [string]$Version,
  [string]$OutDir
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }

if (-not $Version) {
  $psd1 = Join-Path $RepoRoot 'src\lib\LinkWeixin\LinkWeixin.psd1'
  $m = Select-String -Path $psd1 -Pattern "ModuleVersion\s*=\s*'([^']+)'" | Select-Object -First 1
  if (-not $m) { throw "无法从 psd1 读出 ModuleVersion：$psd1" }
  $Version = $m.Matches[0].Groups[1].Value
}

# 组装发布目录（临时 staging），保持仓库的相对结构，解压即用。
$stage = Join-Path $env:TEMP ("linkweixin-release-" + [guid]::NewGuid().ToString('N'))
$stageRoot = Join-Path $stage 'linkWeixin'
New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null
try {
  Copy-Item (Join-Path $RepoRoot 'install.ps1') $stageRoot -Force
  Copy-Item (Join-Path $RepoRoot 'uninstall.ps1') $stageRoot -Force
  Copy-Item (Join-Path $RepoRoot 'src') (Join-Path $stageRoot 'src') -Recurse -Force
  Copy-Item (Join-Path $RepoRoot 'plugin') (Join-Path $stageRoot 'plugin') -Recurse -Force
  foreach ($f in @('LICENSE', 'README.md', 'CHANGELOG.md', '.env.example')) {
    $p = Join-Path $RepoRoot $f
    if (Test-Path $p) { Copy-Item $p $stageRoot -Force }
  }

  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  $zipName = "linkWeixin-v$Version.zip"
  $zipPath = Join-Path $OutDir $zipName
  Remove-Item $zipPath -Force -ErrorAction SilentlyContinue
  Compress-Archive -Path $stageRoot -DestinationPath $zipPath -CompressionLevel Optimal

  $hash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()
  $sumPath = Join-Path $OutDir 'SHA256SUMS.txt'
  # 标准 sha256sum 格式（hash + 两空格 + 文件名），ascii 保证校验工具兼容。
  "$hash  $zipName" | Out-File -FilePath $sumPath -Encoding ascii -Force
  Write-Output "[release] 已生成 $zipPath"
  Write-Output "[release] SHA256 $hash"
  Write-Output "[release] 校验文件 $sumPath"
} finally {
  Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
