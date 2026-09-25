#Requires -Version 5.1
<#
.SYNOPSIS
  校验发布产物的 Authenticode 签名者指纹与客户端内置的信任指纹一致。

.DESCRIPTION
  客户端把 hosts/desktop-tauri/src/update/verify.rs 的 DEFAULT_SIGNATURE_THUMBPRINT 当作唯一信任锚：
  产物未签名、签名状态异常或指纹不一致时，客户端都会拒绝更新。发布前用本模块校验，
  可在构建阶段就挡住两类会把用户卡死的风险：
    1) 签名 secret 丢失/改名导致“静默发出未签名包”；
    2) 轮换证书只改了 secret、忘了同步内置指纹，导致新包对老客户端“签名者不匹配”。
#>

# Get-ExpectedSignatureThumbprint 从 2.0 Rust 客户端读取信任指纹；旧 Go 工具链仅作兼容回退。
function Get-ExpectedSignatureThumbprint {
  param([Parameter(Mandatory = $true)][string]$RepoRoot)

  $rustPath = Join-Path $RepoRoot 'hosts\desktop-tauri\src\update\verify.rs'
  if (Test-Path -LiteralPath $rustPath -PathType Leaf) {
    $source = [IO.File]::ReadAllText($rustPath)
    $match = [regex]::Match($source, 'DEFAULT_SIGNATURE_THUMBPRINT\s*:\s*&str\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
      throw "无法从 $rustPath 读取 DEFAULT_SIGNATURE_THUMBPRINT"
    }
    $thumbprint = $match.Groups[1].Value.Trim()
    if ([string]::IsNullOrWhiteSpace($thumbprint)) {
      throw "DEFAULT_SIGNATURE_THUMBPRINT 为空，无法校验签名：$rustPath"
    }
    return $thumbprint.Replace(':', '').Replace(' ', '').ToUpperInvariant()
  }

  $legacyPath = Join-Path $RepoRoot 'internal\update\signature.go'
  if (-not (Test-Path -LiteralPath $legacyPath -PathType Leaf)) {
    throw "找不到内置指纹来源文件：$rustPath 或 $legacyPath"
  }
  $source = [IO.File]::ReadAllText($legacyPath)
  $match = [regex]::Match($source, 'defaultSignatureThumbprint\s*=\s*"([^"]+)"')
  if (-not $match.Success) {
    throw "无法从 $legacyPath 读取 defaultSignatureThumbprint"
  }
  $thumbprint = $match.Groups[1].Value.Trim()
  if ([string]::IsNullOrWhiteSpace($thumbprint)) {
    throw "defaultSignatureThumbprint 为空，无法校验签名：$legacyPath"
  }
  return $thumbprint.Replace(':', '').Replace(' ', '').ToUpperInvariant()
}

# Get-VerifiedSignatureThumbprint 校验单个产物：未签名、状态异常或指纹不一致都直接抛错；
# 通过时返回实际签名者指纹。
function Get-VerifiedSignatureThumbprint {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$ExpectedThumbprint
  )

  $expected = $ExpectedThumbprint.Replace(':', '').Replace(' ', '').ToUpperInvariant()
  $signature = Get-AuthenticodeSignature -LiteralPath $Path
  if (-not $signature.SignerCertificate) {
    throw "产物未签名或无法读取签名者：$Path（状态 $($signature.Status)）。发布必须用内置信任指纹对应的证书签名。"
  }
  # 自签名/私有证书的链路状态是 UnknownError/NotTrusted，指纹匹配即放行；其它状态一律拒绝。
  if ($signature.Status -notin @('Valid', 'UnknownError', 'NotTrusted')) {
    throw "产物签名状态异常：$($signature.Status)（$Path）"
  }
  $actual = $signature.SignerCertificate.Thumbprint.Replace(':', '').Replace(' ', '').ToUpperInvariant()
  if ($actual -ne $expected) {
    throw "签名者指纹与客户端内置信任指纹不一致：实际 $actual，期望 $expected（$Path）。请先更新 hosts/desktop-tauri/src/update/verify.rs 的 DEFAULT_SIGNATURE_THUMBPRINT 并与本版本一起发布，再轮换证书，否则客户端会拒绝更新。"
  }
  return $actual
}
