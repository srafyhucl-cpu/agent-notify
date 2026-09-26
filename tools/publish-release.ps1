#Requires -Version 5.1
<#
.SYNOPSIS
  Publish an already-built Agent-notify archive to the public binary-only repository.

.DESCRIPTION
  上传前强制门禁：安装器签名、SHA256SUMS.txt 覆盖情况与哈希一致性、ZIP 签名清单、
  ZIP 内主程序与三个 Hook 的签名者指纹（编排见 tools\release-gate.ps1）。
  Release 先保持 Draft，上传后重新下载资产验收；已发布 Release 拒绝自动覆盖。
  任一项不符直接失败，避免补发出客户端会拒绝的包。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\publish-release.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\publish-release.ps1 -Version 1.13.2
#>
param(
  [string]$Version,
  [string]$Repository = 'srafyhucl-cpu/agent-notify-releases',
  [string]$DistDir,
  # 默认拒绝覆盖已发布的 Release（workflow 与默认人工补发都走这条）；确需覆盖时显式加
  # -AllowPublished，并自行保留审计输出。
  [switch]$AllowPublished
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'signature-common.ps1')
. (Join-Path $PSScriptRoot 'release-gate.ps1')
if (-not $DistDir) { $DistDir = Join-Path $RepoRoot 'dist' }

if (-not $Version) {
  $versionPath = Join-Path $RepoRoot 'VERSION'
  if (-not (Test-Path -LiteralPath $versionPath -PathType Leaf)) { throw "找不到版本文件：$versionPath" }
  $Version = ([IO.File]::ReadAllText($versionPath)).Trim()
  if ([string]::IsNullOrWhiteSpace($Version)) { throw "VERSION 为空：$versionPath" }
}
$Version = $Version.Trim().TrimStart('v')

$tag = "v$Version"
$zipPath = Join-Path $DistDir "Agent-notify-$tag.zip"
$setupPath = Join-Path $DistDir "Agent-notify-Setup-$tag.exe"
$sumsPath = Join-Path $DistDir 'SHA256SUMS.txt'
if (-not (Test-Path -LiteralPath $zipPath -PathType Leaf)) {
  throw "Release archive does not exist: $zipPath"
}
if (-not (Test-Path -LiteralPath $setupPath -PathType Leaf)) {
  throw "Release installer does not exist: $setupPath"
}
if (-not (Test-Path -LiteralPath $sumsPath -PathType Leaf)) {
  throw "Checksum file does not exist: $sumsPath"
}

# 手动补发同样必须签名，且签名者指纹等于客户端内置信任指纹，否则客户端会拒绝安装。
# 自动镜像步骤（release.yml）也走本脚本，因此这一步同时是发布前的纵深防御。
$expectedThumbprint = Get-ExpectedSignatureThumbprint -RepoRoot $RepoRoot
$installerThumbprint = Get-VerifiedSignatureThumbprint -Path $setupPath -ExpectedThumbprint $expectedThumbprint
Write-Output "[publish] 安装器签名校验通过（$installerThumbprint）"

# SHA256SUMS.txt 必须覆盖安装器与 ZIP 且哈希一致：客户端下载后按这份校验值验收，过期或不一致的
# 校验值会让用户直接升级失败。
$setupSha = Assert-SumsCoversArtifact -SumsPath $sumsPath -ArtifactPath $setupPath
Write-Output "[publish] SHA256SUMS.txt 覆盖安装器（$setupSha）"
$zipSha = Assert-SumsCoversArtifact -SumsPath $sumsPath -ArtifactPath $zipPath
Write-Output "[publish] SHA256SUMS.txt 覆盖 ZIP（$zipSha）"

# ZIP 内的主程序与阶段 D 的三个 Hook 都要校验：缺文件、未签名或指纹不符都不允许补发。
# 只有主程序过门、Hook 漏检时，用户会在新版本里收到"Hook 无法启动"的静默失效。
# Assert-ArchiveExecutables 会先验证签名清单覆盖 ZIP 全部普通文件，再检查各个程序的 Authenticode。
foreach ($verified in @(Assert-ArchiveExecutables -ZipPath $zipPath -ExpectedThumbprint $expectedThumbprint -ExpectedManifestThumbprint $expectedThumbprint -RepoRoot $RepoRoot -ExpectedVersion $Version)) {
  Write-Output "[publish] ZIP 内 $($verified.Name) 签名校验通过（$($verified.Thumbprint)）"
}

$gh = Get-Command gh.exe -ErrorAction SilentlyContinue
if (-not $gh) {
  throw 'gh.exe was not found. Install and authenticate GitHub CLI first.'
}

$notesPath = Join-Path $DistDir "release-notes-$Version.md"
try {
  $lines = Get-Content (Join-Path $RepoRoot 'CHANGELOG.md') -Encoding UTF8
  $start = -1
  $end = $lines.Count
  for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($start -lt 0 -and $lines[$i] -match "^## \[$([regex]::Escape($Version))\]") {
      $start = $i
      continue
    }
    if ($start -ge 0 -and $i -gt $start -and $lines[$i] -match '^## \[') {
      $end = $i
      break
    }
  }
  if ($start -lt 0) { throw "CHANGELOG.md does not contain version $Version" }
  $notes = ($lines[($start + 1)..($end - 1)] -join "`n").Trim()
  if ([string]::IsNullOrWhiteSpace($notes)) { throw "CHANGELOG.md version $Version has no release notes" }
  [IO.File]::WriteAllText($notesPath, $notes, (New-Object Text.UTF8Encoding($false)))

  # gh 对"按 tag 查草稿"是原生可解析的（裸 API 的 /releases/tags/{tag} 对草稿才返回 404），
  # 所以状态判断直接用 `release view`（退出码 + isDraft），不再自己解析 releases 列表。
  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  $view = & $gh.Source release view $tag --repo $Repository --json isDraft 2>$null
  $viewExit = $LASTEXITCODE
  $ErrorActionPreference = $previousErrorAction

  if ($viewExit -eq 0) {
    $isDraft = (([string]$view).Trim() -match '"isDraft"\s*:\s*true')
    if (-not $isDraft) {
      if (-not $AllowPublished) {
        throw "已存在已发布的 Release，未加 -AllowPublished 时拒绝覆盖：$Repository $tag"
      }
      Write-Warning "已发布的 Release 将被覆盖（-AllowPublished）：$Repository $tag"
    }
  } else {
    # 先创建为草稿（草稿对客户端不可见），把全部资产传完后再发布为 Latest。
    # 曾出现的真实问题：gh release create 逐个上传资产，Release 在 Setup 传完前就已可见且被置为 Latest，
    # 客户端若在窗口期内查询，会只看到 ZIP 而选错升级路径（压缩包分支要求旧版布局，必然失败）。
    & $gh.Source release create $tag --repo $Repository --title "Agent-notify $tag" --notes-file $notesPath --draft
    if ($LASTEXITCODE -ne 0) { throw "gh release create (draft) failed: exit=$LASTEXITCODE" }
  }
  & $gh.Source release upload $tag $setupPath $zipPath $sumsPath --repo $Repository --clobber
  if ($LASTEXITCODE -ne 0) { throw "gh release upload failed: exit=$LASTEXITCODE" }

  # Draft 阶段重新下载资产做一次完整验收；验收失败时 Release 仍保持 Draft。
  $verifyDir = Join-Path $DistDir ('.release-verify-' + [guid]::NewGuid().ToString('N'))
  try {
    New-Item -ItemType Directory -Force -Path $verifyDir | Out-Null
    & $gh.Source release download $tag --repo $Repository --pattern "Agent-notify-Setup-$tag.exe" --pattern "Agent-notify-$tag.zip" --pattern 'SHA256SUMS.txt' --dir $verifyDir
    if ($LASTEXITCODE -ne 0) { throw "gh release download verification failed: exit=$LASTEXITCODE" }
    $downloadedSetup = Join-Path $verifyDir "Agent-notify-Setup-$tag.exe"
    $downloadedZip = Join-Path $verifyDir "Agent-notify-$tag.zip"
    $downloadedSums = Join-Path $verifyDir 'SHA256SUMS.txt'
    foreach ($path in @($downloadedSetup, $downloadedZip, $downloadedSums)) {
      if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "下载后的 Release 资产缺失：$path" }
    }
    Get-VerifiedSignatureThumbprint -Path $downloadedSetup -ExpectedThumbprint $expectedThumbprint | Out-Null
    Assert-SumsCoversArtifact -SumsPath $downloadedSums -ArtifactPath $downloadedSetup | Out-Null
    Assert-SumsCoversArtifact -SumsPath $downloadedSums -ArtifactPath $downloadedZip | Out-Null
    Assert-ArchiveExecutables -ZipPath $downloadedZip -ExpectedThumbprint $expectedThumbprint -ExpectedManifestThumbprint $expectedThumbprint -RepoRoot $RepoRoot -ExpectedVersion $Version | Out-Null
  } finally {
    if (Test-Path -LiteralPath $verifyDir) { Remove-Item -LiteralPath $verifyDir -Recurse -Force }
  }

  # gh 原生发布草稿（与 view 一样能解析草稿）：只翻 draft/latest，不重写已通过验收的标题/说明/资产。
  & $gh.Source release edit $tag --repo $Repository --title "Agent-notify $tag" --notes-file $notesPath --draft=false --latest
  if ($LASTEXITCODE -ne 0) { throw "gh release edit (publish) failed: exit=$LASTEXITCODE" }
  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  $published = & $gh.Source release view $tag --repo $Repository --json isDraft 2>$null
  $publishedExit = $LASTEXITCODE
  $ErrorActionPreference = $previousErrorAction
  if ($publishedExit -ne 0 -or (([string]$published).Trim() -match '"isDraft"\s*:\s*true')) {
    throw "Release 未发布成功：$Repository $tag"
  }
  Write-Output "[publish] published release with all assets: $Repository $tag"
} finally {
  if (Test-Path -LiteralPath $notesPath) {
    Remove-Item -LiteralPath $notesPath -Force
  }
}
