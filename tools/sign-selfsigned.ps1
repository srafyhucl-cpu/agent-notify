#Requires -Version 5.1
<#
.SYNOPSIS
  用自签名/私有证书给产物签名，供 build-release.ps1 与 build-installer.ps1 通过
  AGENT_NOTIFY_SIGNTOOL 调用（调用约定：<tool> sign <file>）。

.DESCRIPTION
  通过同目录的 sign-selfsigned.cmd 调用（Inno Setup 无法直接执行 .ps1）。
  从环境变量读取 PFX（base64）与密码，导入到当前用户证书存储签名后立即清理：
    AGENT_NOTIFY_SIGN_PFX_BASE64   PFX 文件的 base64 文本
    AGENT_NOTIFY_SIGN_PFX_PASSWORD PFX 密码

.EXAMPLE
  # 本地生成自签证书并导出 PFX（只做一次）：
  $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Agent-notify" -CertStoreLocation Cert:\CurrentUser\My
  $pwd = ConvertTo-SecureString -String 'your-password' -AsPlainText -Force
  Export-PfxCertificate -Cert $cert -FilePath agent-notify.pfx -Password $pwd
  # 把中文说明与 base64 放进 CI secret，并把 tools\sign-selfsigned.cmd 的绝对路径写入 AGENT_NOTIFY_SIGNTOOL。
#>
param(
  [Parameter(Mandatory = $true)][string]$Command,
  [Parameter(Mandatory = $true)][string]$File
)

$ErrorActionPreference = 'Stop'

if ($Command -ne 'sign') {
  throw "不支持的签名命令：$Command（仅支持 sign <file>）"
}
if (-not (Test-Path -LiteralPath $File -PathType Leaf)) {
  throw "待签名文件不存在：$File"
}

$base64 = [string]$env:AGENT_NOTIFY_SIGN_PFX_BASE64
$password = [string]$env:AGENT_NOTIFY_SIGN_PFX_PASSWORD
# CI secret 可能带上尾随换行（例如用管道写入），这里统一去掉。
$password = $password.TrimEnd("`r", "`n")
if ([string]::IsNullOrWhiteSpace($base64)) {
  throw '缺少 AGENT_NOTIFY_SIGN_PFX_BASE64（PFX 的 base64 文本）'
}
# 去掉可能的换行/空白，并在解码前给出明确错误，避免 CI 里只看到 FromBase64String 的泛化报错。
$base64 = -join ($base64 -split '\s+')
if ($base64 -notmatch '^[A-Za-z0-9+/]+={0,2}$') {
  throw "AGENT_NOTIFY_SIGN_PFX_BASE64 不是合法 base64（长度 $($base64.Length)）"
}

$pfxPath = Join-Path ([IO.Path]::GetTempPath()) ('agent-notify-sign-' + [guid]::NewGuid().ToString('N') + '.pfx')
[IO.File]::WriteAllBytes($pfxPath, [Convert]::FromBase64String($base64))

$imported = $null
try {
  # 用 SecureString 构造密码，避免 PSUseSecureString 规则（密码本身来自 CI secret）。
  $securePassword = New-Object System.Security.SecureString
  foreach ($character in $password.ToCharArray()) { $securePassword.AppendChar($character) }
  $securePassword.MakeReadOnly()
  $imported = Import-PfxCertificate -FilePath $pfxPath -CertStoreLocation Cert:\CurrentUser\My -Password $securePassword
  $signature = Set-AuthenticodeSignature -FilePath $File -Certificate $imported
  if ($signature.Status -ne 'Valid' -and $signature.Status -ne 'UnknownError' -and $signature.Status -ne 'NotTrusted') {
    throw "签名失败：$($signature.Status) - $($signature.StatusMessage)"
  }
  Write-Output "[sign] 已签名：$File（$($imported.Thumbprint)）"
} finally {
  if (Test-Path -LiteralPath $pfxPath) { Remove-Item -LiteralPath $pfxPath -Force }
  if ($imported) {
    Remove-Item -LiteralPath ("Cert:\CurrentUser\My\" + $imported.Thumbprint) -Force -ErrorAction SilentlyContinue
  }
}
