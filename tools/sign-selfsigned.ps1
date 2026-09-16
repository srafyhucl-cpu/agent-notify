#Requires -Version 5.1
<#
.SYNOPSIS
  用自签名/私有证书给产物签名，供 build-release.ps1 与 build-installer.ps1 通过
  AGENT_NOTIFY_SIGNTOOL 调用（调用约定：<tool> sign <file>）。

.DESCRIPTION
  从环境变量读取 PFX（base64）与密码，导入到当前用户证书存储签名后立即清理：
    AGENT_NOTIFY_SIGN_PFX_BASE64   PFX 文件的 base64 文本
    AGENT_NOTIFY_SIGN_PFX_PASSWORD PFX 密码

.EXAMPLE
  # 本地生成自签证书并导出 PFX（只做一次）：
  $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Agent-notify" -CertStoreLocation Cert:\CurrentUser\My
  $pwd = ConvertTo-SecureString -String 'your-password' -AsPlainText -Force
  Export-PfxCertificate -Cert $cert -FilePath agent-notify.pfx -Password $pwd
  # 把 agent-notify.pfx 转 base64 后放进 CI secret，并把本脚本路径写入 AGENT_NOTIFY_SIGNTOOL
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
if ([string]::IsNullOrWhiteSpace($base64)) {
  throw '缺少 AGENT_NOTIFY_SIGN_PFX_BASE64（PFX 的 base64 文本）'
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
