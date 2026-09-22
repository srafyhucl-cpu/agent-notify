#Requires -Version 5.1
<#
.SYNOPSIS
  发布/补发前的产物门禁：校验 ZIP 内的必需程序与 SHA256SUMS.txt 的覆盖情况。

.DESCRIPTION
  tools/publish-release.ps1（手动补发与 release workflow 的镜像步骤）必须保证发布的每个可执行
  文件都带客户端内置信任指纹对应的签名：客户端会校验安装包与 ZIP 内程序的 Authenticode 签名者
  指纹，也会用 SHA256SUMS.txt 校验下载的产物，任一不符都会让用户卡在升级失败。

  本模块只做编排，签名校验沿用 tools/signature-common.ps1 的 Get-VerifiedSignatureThumbprint，
  不另写一套校验实现。
#>

$script:ReleaseArchiveEntryPrefix = 'Agent-notify/bin/'
# ZIP 内 AgentNotify 自己的可执行文件：主程序 + 阶段 D 的三个 Hook。缺一个都不允许发布。
$script:ReleaseArchiveExecutables = @(
  'agentnotify-desktop.exe',
  'agentnotify-ingress.exe',
  'agentnotify-codex-hook.exe',
  'agentnotify-antigravity-hook.exe',
  'agentnotify-devin-hook.exe'
)

function Get-ReleaseSumEntry {
  param(
    [Parameter(Mandatory = $true)][string]$SumsPath,
    [Parameter(Mandatory = $true)][string]$ArtifactName
  )

  if (-not (Test-Path -LiteralPath $SumsPath -PathType Leaf)) {
    throw "校验文件不存在：$SumsPath"
  }
  # 与客户端 internal/update/checksum.go 相同的解析规则：<sha256>  <文件名>，文件名允许 * 前缀。
  foreach ($line in [IO.File]::ReadAllLines($SumsPath)) {
    $fields = @($line.Split([char[]]@(' ', "`t"), [StringSplitOptions]::RemoveEmptyEntries))
    if ($fields.Count -lt 2) { continue }
    $name = $fields[$fields.Count - 1].TrimStart('*')
    if ($name -ne $ArtifactName) { continue }
    $checksum = $fields[0].ToLowerInvariant()
    if ($checksum -notmatch '^[0-9a-f]{64}$') {
      throw "SHA256SUMS.txt 里 $ArtifactName 的校验值无效：$checksum（$SumsPath）"
    }
    return $checksum
  }
  throw "SHA256SUMS.txt 未覆盖 $ArtifactName（$SumsPath）。发布的产物必须与校验值同源，请用 tools\build-release.ps1 重新生成后再发布。"
}

function Assert-SumsCoversArtifact {
  param(
    [Parameter(Mandatory = $true)][string]$SumsPath,
    [Parameter(Mandatory = $true)][string]$ArtifactPath
  )

  if (-not (Test-Path -LiteralPath $ArtifactPath -PathType Leaf)) {
    throw "产物不存在：$ArtifactPath"
  }
  $name = [IO.Path]::GetFileName($ArtifactPath)
  $expected = Get-ReleaseSumEntry -SumsPath $SumsPath -ArtifactName $name
  $actual = (Get-FileHash -LiteralPath $ArtifactPath -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $expected) {
    throw "SHA256SUMS.txt 里 $name 的校验值与文件不一致：记录 $expected，实际 $actual（$ArtifactPath）。客户端的 SHA256 校验会拒绝这个包，请重新生成 SHA256SUMS.txt。"
  }
  return $actual
}

function Assert-ArchiveExecutables {
  param(
    [Parameter(Mandatory = $true)][string]$ZipPath,
    [Parameter(Mandatory = $true)][string]$ExpectedThumbprint,
    [string[]]$ExecutableNames = $script:ReleaseArchiveExecutables,
    [string]$EntryPrefix = $script:ReleaseArchiveEntryPrefix
  )

  if (-not (Test-Path -LiteralPath $ZipPath -PathType Leaf)) {
    throw "发布包不存在：$ZipPath"
  }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $extractRoot = Join-Path ([IO.Path]::GetTempPath()) ('agent-notify-release-gate-' + [guid]::NewGuid().ToString('N'))
  try {
    $archive = [IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
      $entries = @{}
      $missing = @()
      foreach ($leaf in $ExecutableNames) {
        $entryName = "$EntryPrefix$leaf"
        $entry = $archive.Entries | Where-Object { $_.FullName -eq $entryName } | Select-Object -First 1
        if (-not $entry) {
          $missing += $entryName
          continue
        }
        $entries[$leaf] = $entry
      }
      # 先报缺文件再校验签名：缺 Hook 时报错必须指向 Hook，而不是先撞上主程序的签名问题。
      if ($missing.Count -gt 0) {
        throw "发布包缺少必需程序：$($missing -join '、')（$ZipPath）。请用 tools\build-release.ps1 重新构建，不要手工拼包。"
      }
      New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
      $verified = @()
      foreach ($leaf in $ExecutableNames) {
        $extractedExe = Join-Path $extractRoot $leaf
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[$leaf], $extractedExe, $true)
        $thumbprint = Get-VerifiedSignatureThumbprint -Path $extractedExe -ExpectedThumbprint $ExpectedThumbprint
        $verified += [pscustomobject]@{ Name = $leaf; Thumbprint = $thumbprint }
      }
      return $verified
    } finally {
      $archive.Dispose()
    }
  } finally {
    if (Test-Path -LiteralPath $extractRoot) { Remove-Item -LiteralPath $extractRoot -Recurse -Force }
  }
}
