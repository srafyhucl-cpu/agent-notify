#Requires -Version 5.1
<#
.SYNOPSIS
  生成和验证 Agent-notify ZIP 发布清单。

.DESCRIPTION
  清单列出 ZIP 发布根目录中的全部普通文件，使用 SHA-256 校验内容，并用 detached
  CMS/PKCS#7 签名认证。Rust 客户端和本模块读取 config/release-manifest-contract.json，
  避免构建脚本与客户端维护两套文件名和上限。
#>

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-ReleaseManifestContract {
  param([Parameter(Mandatory = $true)][string]$RepoRoot)

  $path = Join-Path $RepoRoot 'config\release-manifest-contract.json'
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "找不到发布清单合同：$path"
  }
  $contract = [IO.File]::ReadAllText($path) | ConvertFrom-Json
  $required = @('schemaVersion', 'product', 'manifestFileName', 'signatureFileName', 'hashAlgorithm', 'maxManifestBytes', 'maxSignatureBytes', 'maxFileEntries')
  $actual = @($contract.PSObject.Properties.Name)
  if ($actual.Count -ne $required.Count) { throw '发布清单合同包含未知字段或字段数量不正确' }
  foreach ($name in $required) {
    if (-not ($actual -ccontains $name)) { throw "发布清单合同缺少字段：$name" }
  }
  foreach ($name in $actual) {
    if ($required -cnotcontains $name) { throw "发布清单合同包含未知字段：$name" }
  }
  if ([int]$contract.schemaVersion -ne 1 -or [string]::IsNullOrWhiteSpace([string]$contract.product) -or [string]$contract.hashAlgorithm -ne 'sha256') {
    throw '发布清单合同的产品、版本或哈希算法无效'
  }
  if ([int64]$contract.maxManifestBytes -le 0 -or [int64]$contract.maxSignatureBytes -le 0 -or [int]$contract.maxFileEntries -le 0) {
    throw '发布清单合同的大小或文件数上限无效'
  }
  if ([string]$contract.manifestFileName -eq [string]$contract.signatureFileName) {
    throw '发布清单合同的控制文件名不能相同'
  }
  foreach ($name in @([string]$contract.manifestFileName, [string]$contract.signatureFileName)) {
    if (-not (Test-ReleaseManifestPath -Path $name)) {
      throw "发布清单合同控制文件名无效：$name"
    }
  }
  return $contract
}

function Test-ReleaseManifestPath {
  param([Parameter(Mandatory = $true)][string]$Path)

  if ([string]::IsNullOrWhiteSpace($Path) -or $Path.Contains('\') -or $Path.Contains([char]0) -or $Path.Contains(':') -or $Path.StartsWith('/') -or $Path.EndsWith('/') -or $Path.Contains('//')) {
    return $false
  }
  if ($Path -match '[\x00-\x1f]') { return $false }
  foreach ($part in $Path.Split('/')) {
    if ([string]::IsNullOrWhiteSpace($part) -or $part -eq '.' -or $part -eq '..') { return $false }
  }
  return $true
}

function Get-ReleaseManifestRelativePath {
  param(
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][string]$Path
  )

  $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
  $pathFull = [IO.Path]::GetFullPath($Path)
  if (-not $pathFull.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase)) {
    throw "发布文件不在 staging 根目录内：$Path"
  }
  return $pathFull.Substring($rootFull.Length).Replace('\', '/')
}

function Get-ReleaseManifestActualFiles {
  param(
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)]$Contract
  )

  $rootItem = Get-Item -LiteralPath $Root -Force -ErrorAction Stop
  if (-not $rootItem.PSIsContainer -or (($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    throw "发布清单根目录无效或包含链接：$Root"
  }
  $paths = New-Object 'System.Collections.Generic.List[string]'
  $pending = New-Object 'System.Collections.Generic.Stack[System.IO.DirectoryInfo]'
  $pending.Push([IO.DirectoryInfo]$rootItem)
  while ($pending.Count -gt 0) {
    $directory = $pending.Pop()
    foreach ($item in @(Get-ChildItem -LiteralPath $directory.FullName -Force)) {
      if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "发布目录包含不支持的链接：$($item.FullName)"
      }
      if ($item.PSIsContainer) {
        $pending.Push([IO.DirectoryInfo]$item)
        continue
      }
      if (-not ($item -is [IO.FileInfo])) {
        throw "发布目录包含不支持的特殊文件：$($item.FullName)"
      }
      $relative = Get-ReleaseManifestRelativePath -Root $Root -Path $item.FullName
      if ($relative -ceq [string]$Contract.manifestFileName -or $relative -ceq [string]$Contract.signatureFileName) {
        continue
      }
      if (-not (Test-ReleaseManifestPath -Path $relative)) {
        throw "发布目录包含不安全的相对路径：$relative"
      }
      $paths.Add($relative)
    }
  }
  $paths.Sort([StringComparer]::Ordinal)
  if ($paths.Count -gt [int]$Contract.maxFileEntries) {
    throw "发布文件数量超过上限：$($paths.Count)"
  }

  $entries = New-Object 'System.Collections.Generic.List[object]'
  foreach ($relative in $paths) {
    $full = Join-Path $Root ($relative.Replace('/', '\'))
    $hash = (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant()
    $entries.Add([pscustomobject][ordered]@{
      path = $relative
      size = [int64](Get-Item -LiteralPath $full).Length
      sha256 = $hash
    })
  }
  return $entries.ToArray()
}

function New-ReleaseManifest {
  param(
    [Parameter(Mandatory = $true)][string]$RepoRoot,
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][string]$Version
  )

  $contract = Get-ReleaseManifestContract -RepoRoot $RepoRoot
  $entries = @(Get-ReleaseManifestActualFiles -Root $Root -Contract $contract)
  $manifest = [ordered]@{
    schemaVersion = [int]$contract.schemaVersion
    product = [string]$contract.product
    version = $Version
    hashAlgorithm = [string]$contract.hashAlgorithm
    files = $entries
  }
  $path = Join-Path $Root ([string]$contract.manifestFileName)
  $json = $manifest | ConvertTo-Json -Depth 8
  [IO.File]::WriteAllText($path, $json, (New-Object Text.UTF8Encoding($false)))
  return $path
}

function Get-ReleaseManifestSigningCertificate {
  param([Parameter(Mandatory = $true)][string]$ExpectedThumbprint)

  $base64 = [string]$env:AGENT_NOTIFY_SIGN_PFX_BASE64
  $password = [string]$env:AGENT_NOTIFY_SIGN_PFX_PASSWORD
  if ([string]::IsNullOrWhiteSpace($base64) -or [string]::IsNullOrWhiteSpace($password)) {
    throw '缺少发布清单签名所需的 PFX 环境变量'
  }
  $base64 = -join ($base64 -split '\s+')
  if ($base64 -notmatch '^[A-Za-z0-9+/]+={0,2}$') { throw '发布签名 PFX base64 格式无效' }
  $securePassword = New-Object System.Security.SecureString
  foreach ($character in $password.TrimEnd("`r", "`n").ToCharArray()) { $securePassword.AppendChar($character) }
  $securePassword.MakeReadOnly()
  $pfxBytes = [Convert]::FromBase64String($base64)
  $certificate = $null
  try {
    $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet
    $certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($pfxBytes, $securePassword, $flags)
    if (-not $certificate.HasPrivateKey) { throw '发布签名证书没有私钥' }
    $actual = ([string]$certificate.Thumbprint).Replace(':', '').Replace(' ', '').ToUpperInvariant()
    $expected = ([string]$ExpectedThumbprint).Replace(':', '').Replace(' ', '').ToUpperInvariant()
    if ($actual -ne $expected) { throw "发布签名证书指纹不匹配：实际 $actual，期望 $expected" }
    $now = [DateTime]::UtcNow
    if ($now -lt $certificate.NotBefore.ToUniversalTime() -or $now -gt $certificate.NotAfter.ToUniversalTime()) {
      throw '发布签名证书不在有效期内'
    }
    return $certificate
  } catch {
    if ($certificate) { $certificate.Dispose() }
    throw
  } finally {
    [Array]::Clear($pfxBytes, 0, $pfxBytes.Length)
    $securePassword.Dispose()
  }
}

function Protect-ReleaseManifest {
  param(
    [Parameter(Mandatory = $true)][string]$RepoRoot,
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$ExpectedThumbprint
  )

  $contract = Get-ReleaseManifestContract -RepoRoot $RepoRoot
  $manifestPath = Join-Path $Root ([string]$contract.manifestFileName)
  if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw "发布清单不存在：$manifestPath" }
  $signaturePath = Join-Path $Root ([string]$contract.signatureFileName)
  $manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
  if ($manifestBytes.Length -gt [int64]$contract.maxManifestBytes) { throw '发布清单超过大小上限' }
  $certificate = Get-ReleaseManifestSigningCertificate -ExpectedThumbprint $ExpectedThumbprint
  try {
    Add-Type -AssemblyName System.Security
    $content = [System.Security.Cryptography.Pkcs.ContentInfo]::new($manifestBytes)
    $cms = [System.Security.Cryptography.Pkcs.SignedCms]::new($content, $true)
    $signer = [System.Security.Cryptography.Pkcs.CmsSigner]::new($certificate)
    $signer.DigestAlgorithm = [System.Security.Cryptography.Oid]::new('2.16.840.1.101.3.4.2.1')
    $signer.IncludeOption = [System.Security.Cryptography.X509Certificates.X509IncludeOption]::EndCertOnly
    $cms.ComputeSignature($signer)
    $signatureBytes = $cms.Encode()
    if ($signatureBytes.Length -gt [int64]$contract.maxSignatureBytes) { throw '发布清单签名超过大小上限' }
    [IO.File]::WriteAllBytes($signaturePath, $signatureBytes)
  } finally {
    $certificate.Dispose()
  }
  return $signaturePath
}

function Test-ReleaseManifestCms {
  param(
    [Parameter(Mandatory = $true)][byte[]]$ManifestBytes,
    [Parameter(Mandatory = $true)][byte[]]$SignatureBytes,
    [Parameter(Mandatory = $true)][string]$ExpectedThumbprint
  )

  Add-Type -AssemblyName System.Security
  $content = [System.Security.Cryptography.Pkcs.ContentInfo]::new($ManifestBytes)
  $cms = [System.Security.Cryptography.Pkcs.SignedCms]::new($content, $true)
  try {
    $cms.Decode($SignatureBytes)
    $cms.CheckSignature($true)
    $signers = @($cms.SignerInfos)
    if ($signers.Count -ne 1) { throw '发布清单签名者数量不是 1' }
    $signer = $signers[0]
    if ([string]$signer.DigestAlgorithm.Value -ne '2.16.840.1.101.3.4.2.1') { throw '发布清单签名摘要算法不是 SHA-256' }
    $certificate = $signer.Certificate
    if (-not $certificate) { throw '发布清单签名缺少证书' }
    $actual = ([string]$certificate.Thumbprint).Replace(':', '').Replace(' ', '').ToUpperInvariant()
    $expected = ([string]$ExpectedThumbprint).Replace(':', '').Replace(' ', '').ToUpperInvariant()
    if ($actual -ne $expected) { throw "发布清单签名者指纹不匹配：实际 $actual，期望 $expected" }
    $now = [DateTime]::UtcNow
    if ($now -lt $certificate.NotBefore.ToUniversalTime() -or $now -gt $certificate.NotAfter.ToUniversalTime()) { throw '发布清单签名证书不在有效期内' }
    return $actual
  } catch {
    throw "发布清单 CMS 签名校验失败：$($_.Exception.Message)"
  }
}

function Test-ReleaseManifest {
  param(
    [Parameter(Mandatory = $true)][string]$RepoRoot,
    [Parameter(Mandatory = $true)][string]$Root,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$ExpectedThumbprint
  )

  $contract = Get-ReleaseManifestContract -RepoRoot $RepoRoot
  $manifestPath = Join-Path $Root ([string]$contract.manifestFileName)
  $signaturePath = Join-Path $Root ([string]$contract.signatureFileName)
  foreach ($path in @($manifestPath, $signaturePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "发布清单控制文件不存在：$path" }
    $controlItem = Get-Item -LiteralPath $path -Force
    if (($controlItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "发布清单控制文件不能是链接：$path" }
  }
  $manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
  $signatureBytes = [IO.File]::ReadAllBytes($signaturePath)
  if ($manifestBytes.Length -gt [int64]$contract.maxManifestBytes) { throw '发布清单超过大小上限' }
  if ($signatureBytes.Length -gt [int64]$contract.maxSignatureBytes) { throw '发布清单签名超过大小上限' }
  $thumbprint = Test-ReleaseManifestCms -ManifestBytes $manifestBytes -SignatureBytes $signatureBytes -ExpectedThumbprint $ExpectedThumbprint

  try { $manifest = [Text.Encoding]::UTF8.GetString($manifestBytes) | ConvertFrom-Json } catch { throw '发布清单不是有效 JSON' }
  if ($null -eq $manifest -or $null -eq $manifest.PSObject) { throw '发布清单不是有效对象' }
  $allowed = @('schemaVersion', 'product', 'version', 'hashAlgorithm', 'files')
  $actualProperties = @($manifest.PSObject.Properties.Name)
  if ($actualProperties.Count -ne $allowed.Count) { throw '发布清单顶层字段数量不正确' }
  foreach ($property in $actualProperties) {
    if ($allowed -cnotcontains $property) { throw "发布清单包含未知字段：$property" }
  }
  foreach ($property in $allowed) {
    if ($actualProperties -cnotcontains $property) { throw "发布清单缺少字段：$property" }
  }
  if ([int]$manifest.schemaVersion -ne [int]$contract.schemaVersion -or [string]$manifest.product -cne [string]$contract.product -or [string]$manifest.version -cne $Version -or [string]$manifest.hashAlgorithm -cne [string]$contract.hashAlgorithm) {
    throw '发布清单的版本、产品或哈希算法不匹配'
  }
  $files = @($manifest.files)
  if ($files.Count -eq 0 -or $files.Count -gt [int]$contract.maxFileEntries) { throw '发布清单文件数量无效' }

  $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
  $previous = $null
  $fileFields = @('path', 'size', 'sha256')
  foreach ($file in $files) {
    $fileProperties = @($file.PSObject.Properties.Name)
    if ($fileProperties.Count -ne $fileFields.Count) { throw '发布清单文件条目字段数量不正确' }
    foreach ($field in $fileProperties) {
      if ($fileFields -cnotcontains $field) { throw "发布清单文件条目包含未知字段：$field" }
    }
    $path = [string]$file.path
    if (-not (Test-ReleaseManifestPath -Path $path) -or -not $seen.Add($path)) { throw "发布清单路径重复或无效：$path" }
    if ($null -ne $previous -and [StringComparer]::Ordinal.Compare($previous, $path) -ge 0) { throw "发布清单路径未按 ordinal 顺序排列：$path" }
    $size = 0L
    $sizeText = [string]$file.size
    if ($sizeText -notmatch '^[0-9]+$' -or -not [int64]::TryParse($sizeText, [ref]$size) -or $size -lt 0 -or ([string]$file.sha256 -notmatch '^[0-9a-f]{64}$')) { throw "发布清单文件校验字段无效：$path" }
    $previous = $path
  }

  $actual = @(Get-ReleaseManifestActualFiles -Root $Root -Contract $contract)
  if ($actual.Count -ne $files.Count) { throw '发布清单文件集合与 ZIP 实际文件集合不一致' }
  $actualByPath = New-Object 'System.Collections.Generic.Dictionary[string,object]' ([StringComparer]::Ordinal)
  foreach ($file in $actual) {
    $actualByPath.Add([string]$file.path, $file)
    if (-not $seen.Contains([string]$file.path)) { throw "发布包包含清单未声明的文件：$($file.path)" }
  }
  foreach ($file in $files) {
    $entry = $actualByPath[[string]$file.path]
    if ($null -eq $entry -or [int64]$entry.size -ne [int64]$file.size -or [string]$entry.sha256 -ne [string]$file.sha256) {
      throw "发布包文件与清单不一致：$($file.path)"
    }
  }
  return [pscustomobject]@{ Version = $Version; Thumbprint = $thumbprint; FileCount = $files.Count }
}

function Get-ReleaseArchiveEntryPath {
  param([Parameter(Mandatory = $true)][string]$EntryName)

  $name = [string]$EntryName
  if ([string]::IsNullOrWhiteSpace($name) -or $name.Contains('\') -or $name.Contains([char]0) -or $name.StartsWith('/') -or $name -match '^[A-Za-z]:' -or $name.Contains('//') -or $name -match '[\x00-\x1f]') {
    throw "发布 ZIP 路径不安全：$EntryName"
  }
  $isDirectory = $name.EndsWith('/')
  $canonical = $name.TrimEnd('/')
  if ([string]::IsNullOrWhiteSpace($canonical) -or $canonical.Contains('//')) {
    throw "发布 ZIP 路径不安全：$EntryName"
  }
  $parts = @($canonical.Split('/'))
  if ($parts.Count -eq 0 -or $parts[0] -cne 'Agent-notify') {
    throw "发布 ZIP 条目不在 Agent-notify 根目录内：$EntryName"
  }
  $relativeParts = @()
  if ($parts.Count -gt 1) { $relativeParts = @($parts[1..($parts.Count - 1)]) }
  foreach ($part in $relativeParts) {
    if ([string]::IsNullOrWhiteSpace($part) -or $part -eq '.' -or $part -eq '..' -or $part.Contains(':')) {
      throw "发布 ZIP 路径不安全：$EntryName"
    }
  }
  if (-not $isDirectory -and $relativeParts.Count -eq 0) {
    throw "发布 ZIP 根目录不能是文件：$EntryName"
  }
  $relative = ($relativeParts -join '/')
  if (-not [string]::IsNullOrWhiteSpace($relative) -and -not (Test-ReleaseManifestPath -Path $relative)) {
    throw "发布 ZIP 路径不安全：$EntryName"
  }
  return [pscustomobject]@{ Canonical = $canonical; Relative = $relative; IsDirectory = $isDirectory }
}

function Assert-ArchiveReleaseManifest {
  param(
    [Parameter(Mandatory = $true)][string]$RepoRoot,
    [Parameter(Mandatory = $true)][string]$ZipPath,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$ExpectedThumbprint
  )

  if (-not (Test-Path -LiteralPath $ZipPath -PathType Leaf)) { throw "发布 ZIP 不存在：$ZipPath" }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
  $tempBase = Join-Path $driveRoot 'Temp'
  New-Item -ItemType Directory -Force -Path $tempBase | Out-Null
  $extractRoot = Join-Path $tempBase ('agent-notify-manifest-gate-' + [guid]::NewGuid().ToString('N'))
  try {
    New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
    $archive = [IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
      $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
      $extractPrefix = [IO.Path]::GetFullPath($extractRoot).TrimEnd('\') + '\'
      foreach ($entry in $archive.Entries) {
        $entryPath = Get-ReleaseArchiveEntryPath -EntryName ([string]$entry.FullName)
        if (-not $seen.Add($entryPath.Canonical)) { throw "发布 ZIP 包含重复条目：$($entry.FullName)" }
        $target = Join-Path $extractRoot ($entryPath.Canonical.Replace('/', '\'))
        $targetFull = [IO.Path]::GetFullPath($target)
        if (-not $targetFull.StartsWith($extractPrefix, [StringComparison]::OrdinalIgnoreCase)) {
          throw "发布 ZIP 条目越过临时目录：$($entry.FullName)"
        }
        if ($entryPath.IsDirectory) {
          New-Item -ItemType Directory -Force -Path $target | Out-Null
          continue
        }
        $parent = Split-Path -Parent $target
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
      }
    } finally { $archive.Dispose() }
    $root = Join-Path $extractRoot 'Agent-notify'
    return Test-ReleaseManifest -RepoRoot $RepoRoot -Root $root -Version $Version -ExpectedThumbprint $ExpectedThumbprint
  } finally {
    if (Test-Path -LiteralPath $extractRoot) { Remove-Item -LiteralPath $extractRoot -Recurse -Force }
  }
}
