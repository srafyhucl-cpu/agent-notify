#Requires -Version 5.1
<#
.SYNOPSIS
  校验 tools/signature-common.ps1 的发布签名门禁本身没有被改坏。

.DESCRIPTION
  门禁的危险失效方向是 fail-open（放行未签名或指纹不符的产物）。这里用系统已签名的可执行
  文件当合法样本、用自造 MZ 文件当未签名样本，并用临时篡改过的 signature.go 验证“指纹来自
  源码而非硬编码”，因此不需要签名证书也能在 CI 覆盖。
#>
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $RepoRoot 'tools\signature-common.ps1')

$failures = New-Object System.Collections.Generic.List[string]
function Add-Failure { param([string]$Message) $script:failures.Add($Message) }

function Assert-Throws {
  param([string]$Name, [scriptblock]$Action)
  $threw = $false
  try { & $Action | Out-Null } catch { $threw = $true }
  if ($threw) {
    Write-Output "[signature-gate] ok（拒绝）: $Name"
  } else {
    Add-Failure "$Name：期望抛错，实际放行"
  }
}

function Assert-Passes {
  param([string]$Name, [scriptblock]$Action)
  try {
    & $Action | Out-Null
    Write-Output "[signature-gate] ok（放行）: $Name"
  } catch {
    Add-Failure "$Name：期望通过，实际报错：$($_.Exception.Message)"
  }
}

function Get-SignedFixture {
  foreach ($candidate in @(
      (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'),
      (Join-Path $env:SystemRoot 'System32\notepad.exe')
    )) {
    if (Test-Path -LiteralPath $candidate) {
      $signature = Get-AuthenticodeSignature -LiteralPath $candidate
      if ($signature.SignerCertificate) { return $candidate }
    }
  }
  return $null
}

$tempBase = [IO.Path]::GetTempPath()

# 1) 内置指纹必须是可用的 40 位十六进制值。
$expected = Get-ExpectedSignatureThumbprint -RepoRoot $RepoRoot
if ($expected -notmatch '^[0-9A-F]{40}$') {
  Add-Failure "Get-ExpectedSignatureThumbprint 不是 40 位十六进制指纹：$expected"
} else {
  Write-Output "[signature-gate] ok: 内置指纹 $expected"
}

# 2) 指纹来自源码：篡改临时副本后必须读到新值，且与真实值不同。
$tamperRoot = Join-Path $tempBase ('agent-notify-siggate-' + [guid]::NewGuid().ToString('N'))
$tamperedValue = '1111111111111111111111111111111111111111'
try {
  $tamperDir = Join-Path $tamperRoot 'internal\update'
  New-Item -ItemType Directory -Force -Path $tamperDir | Out-Null
  $fakeSource = "package update`n`nconst defaultSignatureThumbprint = `"$tamperedValue`"`n"
  [IO.File]::WriteAllText((Join-Path $tamperDir 'signature.go'), $fakeSource, (New-Object Text.UTF8Encoding($false)))

  $tampered = Get-ExpectedSignatureThumbprint -RepoRoot $tamperRoot
  if ($tampered -ne $tamperedValue) {
    Add-Failure "篡改后的 signature.go 未被读取：得到 $tampered"
  } elseif ($tampered -eq $expected) {
    Add-Failure '篡改前后指纹相同，说明指纹可能被硬编码'
  } else {
    Write-Output "[signature-gate] ok: 指纹随源码变化（$tampered）"
  }

  # 3) 已签名样本：正确指纹放行，非内置指纹拒绝（模拟证书轮换未同步内置指纹）。
  $fixture = Get-SignedFixture
  if (-not $fixture) {
    Write-Warning '[signature-gate] 未找到已签名的系统样本，跳过签名放行/拒绝断言'
  } else {
    $actual = (Get-AuthenticodeSignature -LiteralPath $fixture).SignerCertificate.Thumbprint
    Assert-Passes '已签名样本 + 正确指纹' { Get-VerifiedSignatureThumbprint -Path $fixture -ExpectedThumbprint $actual }
    Assert-Throws '已签名样本 + 非内置指纹（模拟内置指纹未同步）' { Get-VerifiedSignatureThumbprint -Path $fixture -ExpectedThumbprint $tamperedValue }
  }
} finally {
  if (Test-Path -LiteralPath $tamperRoot) { Remove-Item -LiteralPath $tamperRoot -Recurse -Force }
}

# 4) 未签名样本必须被拒绝。
$unsigned = Join-Path $tempBase ('agent-notify-unsigned-' + [guid]::NewGuid().ToString('N') + '.exe')
try {
  [IO.File]::WriteAllBytes($unsigned, [byte[]](0x4D, 0x5A))
  Assert-Throws '未签名样本' { Get-VerifiedSignatureThumbprint -Path $unsigned -ExpectedThumbprint $expected }
} finally {
  if (Test-Path -LiteralPath $unsigned) { Remove-Item -LiteralPath $unsigned -Force }
}

if ($failures.Count -gt 0) {
  foreach ($failure in $failures) { Write-Output "[signature-gate] 失败：$failure" }
  exit 1
}
Write-Output '[signature-gate] 通过'
exit 0
