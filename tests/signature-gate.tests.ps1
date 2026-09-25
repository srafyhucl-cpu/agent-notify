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

# 与 Assert-Throws 的差别：额外断言失败原因里点名了哪个产物，避免"因为别的原因失败"被当成通过。
function Assert-FailsWith {
  param([string]$Name, [string]$ExpectedText, [scriptblock]$Action)
  try {
    & $Action | Out-Null
    Add-Failure "$Name：期望失败，实际放行"
  } catch {
    if ($_.Exception.Message.Contains($ExpectedText)) {
      Write-Output "[signature-gate] ok（拒绝）: $Name"
    } else {
      Add-Failure "$Name：失败原因没有点名 $ExpectedText，实际：$($_.Exception.Message)"
    }
  }
}

function Get-SignedFixture {
  # where.exe 体积最小（约 64 KB），补发门禁用例会把它复制进测试 ZIP，避免制造大临时文件。
  foreach ($candidate in @(
      (Join-Path $env:SystemRoot 'System32\where.exe'),
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

$tempBase = Join-Path ([IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')) 'Temp'
New-Item -ItemType Directory -Force -Path $tempBase | Out-Null

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
  $tamperDir = Join-Path $tamperRoot 'hosts\desktop-tauri\src\update'
  New-Item -ItemType Directory -Force -Path $tamperDir | Out-Null
  $fakeSource = "pub const DEFAULT_SIGNATURE_THUMBPRINT: &str = `"$tamperedValue`";`n"
  [IO.File]::WriteAllText((Join-Path $tamperDir 'verify.rs'), $fakeSource, (New-Object Text.UTF8Encoding($false)))

  $tampered = Get-ExpectedSignatureThumbprint -RepoRoot $tamperRoot
  if ($tampered -ne $tamperedValue) {
    Add-Failure "篡改后的 verify.rs 未被读取：得到 $tampered"
  } elseif ($tampered -eq $expected) {
    Add-Failure '篡改前后指纹相同，说明指纹可能被硬编码'
  } else {
    Write-Output "[signature-gate] ok: 指纹随 Rust 源码变化（$tampered）"
  }

  # 3) 旧 Go 常量在没有 Rust 源文件的兼容仓库中仍可读取。
  $legacyTamperRoot = Join-Path $tempBase ('agent-notify-siggate-legacy-' + [guid]::NewGuid().ToString('N'))
  try {
    $legacyDir = Join-Path $legacyTamperRoot 'internal\update'
    New-Item -ItemType Directory -Force -Path $legacyDir | Out-Null
    $legacyValue = '2222222222222222222222222222222222222222'
    $legacySource = "const defaultSignatureThumbprint = `"$legacyValue`"`n"
    [IO.File]::WriteAllText((Join-Path $legacyDir 'signature.go'), $legacySource, (New-Object Text.UTF8Encoding($false)))
    $legacyThumbprint = Get-ExpectedSignatureThumbprint -RepoRoot $legacyTamperRoot
    if ($legacyThumbprint -ne $legacyValue) {
      Add-Failure "旧 Go 指纹未被读取：得到 $legacyThumbprint"
    } else {
      Write-Output "[signature-gate] ok: 旧 Go 指纹兼容读取（$legacyThumbprint）"
    }
  } finally {
    if (Test-Path -LiteralPath $legacyTamperRoot) { Remove-Item -LiteralPath $legacyTamperRoot -Recurse -Force }
  }

  # 4) 已签名样本：正确指纹放行，非内置指纹拒绝（模拟证书轮换未同步）。
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

# 5) 未签名样本必须被拒绝。
$unsigned = Join-Path $tempBase ('agent-notify-unsigned-' + [guid]::NewGuid().ToString('N') + '.exe')
try {
  [IO.File]::WriteAllBytes($unsigned, [byte[]](0x4D, 0x5A))
  Assert-Throws '未签名样本' { Get-VerifiedSignatureThumbprint -Path $unsigned -ExpectedThumbprint $expected }
} finally {
  if (Test-Path -LiteralPath $unsigned) { Remove-Item -LiteralPath $unsigned -Force }
}

# 6) 补发/镜像路径的产物门禁（tools/release-gate.ps1）：
#    ZIP 内三个 Hook 缺一个、或 Hook 未签名/指纹不符，都必须在上传前失败。
. (Join-Path $RepoRoot 'tools\release-gate.ps1')
$publishScript = [IO.File]::ReadAllText((Join-Path $RepoRoot 'tools\publish-release.ps1'))
if (-not $publishScript.Contains('release-gate.ps1')) {
  Add-Failure 'tools/publish-release.ps1 没有引用 tools/release-gate.ps1，补发路径会绕过 Hook 校验'
} elseif (-not $publishScript.Contains('Assert-ArchiveExecutables')) {
  Add-Failure 'tools/publish-release.ps1 没有调用 Assert-ArchiveExecutables，ZIP 内 Hook 不会被校验'
} elseif (-not $publishScript.Contains('ExpectedManifestThumbprint')) {
  Add-Failure 'tools/publish-release.ps1 没有把签名清单绑定到发布信任锚'
} elseif (-not $publishScript.Contains('已存在已发布的 Release')) {
  Add-Failure 'tools/publish-release.ps1 没有拒绝覆盖已发布 Release'
} elseif (-not $publishScript.Contains('gh release download')) {
  Add-Failure 'tools/publish-release.ps1 没有在 Draft 阶段重新下载并验证资产'
} else {
  Write-Output '[signature-gate] ok: 补发脚本引用归档门禁'
}

function New-GateArchive {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [string[]]$SignedLeaves = @(),
    [string[]]$UnsignedLeaves = @(),
    [string[]]$ExtraFiles = @(),
    [switch]$OmitManifest,
    [switch]$OmitSignature,
    [switch]$TamperManifest,
    [switch]$TamperPayload
  )
  # 先生成包含全部条目的 staging，再用临时内存 PFX 签署清单，最后从 staging 打包。
  # 这样归档门禁测试本身也覆盖“清单声明文件集合”和 Authenticode 两层校验。
  $stagingRoot = Join-Path $gateRoot ('archive-staging-' + [guid]::NewGuid().ToString('N'))
  $payloadRoot = Join-Path $stagingRoot 'Agent-notify'
  try {
    New-Item -ItemType Directory -Force -Path (Join-Path $payloadRoot 'bin') | Out-Null
    $fixture = Get-SignedFixture
    foreach ($leaf in $SignedLeaves) {
      Copy-Item -LiteralPath $fixture -Destination (Join-Path $payloadRoot "bin\$leaf") -Force
    }
    foreach ($leaf in $UnsignedLeaves) {
      [IO.File]::WriteAllBytes((Join-Path $payloadRoot "bin\$leaf"), [byte[]](0x4D, 0x5A))
    }
    New-ReleaseManifest -RepoRoot $RepoRoot -Root $payloadRoot -Version $gateManifestVersion | Out-Null
    Protect-ReleaseManifest -RepoRoot $RepoRoot -Root $payloadRoot -Version $gateManifestVersion -ExpectedThumbprint $gateManifestThumbprint | Out-Null
    if ($OmitManifest) { Remove-Item -LiteralPath (Join-Path $payloadRoot 'RELEASE-MANIFEST.json') -Force }
    if ($OmitSignature) { Remove-Item -LiteralPath (Join-Path $payloadRoot 'RELEASE-MANIFEST.p7s') -Force }
    if ($TamperManifest) {
      $manifestPath = Join-Path $payloadRoot 'RELEASE-MANIFEST.json'
      $manifestBytes = [IO.File]::ReadAllBytes($manifestPath) + [byte[]](0x20)
      [IO.File]::WriteAllBytes($manifestPath, $manifestBytes)
    }
    if ($TamperPayload) {
      $payloadFile = Get-ChildItem -LiteralPath (Join-Path $payloadRoot 'bin') -File | Select-Object -First 1
      [IO.File]::WriteAllBytes($payloadFile.FullName, ([IO.File]::ReadAllBytes($payloadFile.FullName) + [byte[]](0x20)))
    }
    foreach ($relative in $ExtraFiles) {
      $extraPath = Join-Path $payloadRoot ($relative.Replace('/', '\'))
      New-Item -ItemType Directory -Force -Path (Split-Path -Parent $extraPath) | Out-Null
      [IO.File]::WriteAllText($extraPath, 'extra', (New-Object Text.UTF8Encoding($false)))
    }

    # PowerShell 的类型字面量只在已加载的程序集里查找：两个程序集都要显式加载。
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    Add-Type -AssemblyName System.IO.Compression
    $archive = [IO.Compression.ZipFile]::Open($Path, [IO.Compression.ZipArchiveMode]::Create)
    try {
      foreach ($file in @(Get-ChildItem -LiteralPath $payloadRoot -Recurse -File)) {
        $entryName = 'Agent-notify/' + (Get-ReleaseManifestRelativePath -Root $payloadRoot -Path $file.FullName)
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
          $archive, $file.FullName, $entryName, [IO.Compression.CompressionLevel]::Optimal)
      }
    } finally {
      $archive.Dispose()
    }
  } finally {
    if (Test-Path -LiteralPath $stagingRoot) { Remove-Item -LiteralPath $stagingRoot -Recurse -Force }
  }
}

$gateRoot = Join-Path $tempBase ('agent-notify-release-gate-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $gateRoot | Out-Null
$gateManifestVersion = ([IO.File]::ReadAllText((Join-Path $RepoRoot 'VERSION'))).Trim()
$gateManifestThumbprint = $null
$manifestCertificate = $null
$manifestPassword = $null
$manifestPfx = $null
$previousPfxBase64 = $env:AGENT_NOTIFY_SIGN_PFX_BASE64
$previousPfxPassword = $env:AGENT_NOTIFY_SIGN_PFX_PASSWORD
try {
  $gateFixture = Get-SignedFixture
  if (-not $gateFixture) {
    Write-Warning '[signature-gate] 未找到已签名的系统样本，跳过补发门禁的归档断言'
  } else {
    $manifestCertificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=AgentNotify Release Manifest Test' -CertStoreLocation 'Cert:\CurrentUser\My' -NotBefore (Get-Date).AddYears(-1) -NotAfter (Get-Date).AddYears(2)
    $manifestPassword = New-Object System.Security.SecureString
    foreach ($character in 'agentnotify-test-password'.ToCharArray()) { $manifestPassword.AppendChar($character) }
    $manifestPassword.MakeReadOnly()
    $manifestPfx = $manifestCertificate.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Pfx, $manifestPassword)
    $env:AGENT_NOTIFY_SIGN_PFX_BASE64 = [Convert]::ToBase64String($manifestPfx)
    $env:AGENT_NOTIFY_SIGN_PFX_PASSWORD = 'agentnotify-test-password'
    $gateManifestThumbprint = $manifestCertificate.Thumbprint
  $fixtureThumbprint = (Get-AuthenticodeSignature -LiteralPath $gateFixture).SignerCertificate.Thumbprint
  $allLeaves = @(
    'agentnotify-desktop.exe', 'agentnotify-ingress.exe',
    'agentnotify-codex-hook.exe', 'agentnotify-antigravity-hook.exe', 'agentnotify-devin-hook.exe'
  )

  # 6a) 只有主程序、三个 Hook 全缺：必须点名缺的 Hook 并失败。
  $missingHooksZip = Join-Path $gateRoot 'missing-hooks.zip'
  New-GateArchive -Path $missingHooksZip -SignedLeaves @('agentnotify-desktop.exe', 'agentnotify-ingress.exe', 'agentnotify-antigravity-hook.exe')
  Assert-FailsWith 'ZIP 内缺少 Codex Hook' 'agentnotify-codex-hook.exe' {
    Assert-ArchiveExecutables -ZipPath $missingHooksZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }

  # 6b) Hook 存在但未签名：必须点名该 Hook 并失败（证明 Hook 真的进了签名校验循环）。
  $unsignedHookZip = Join-Path $gateRoot 'unsigned-hook.zip'
  New-GateArchive -Path $unsignedHookZip `
    -SignedLeaves @('agentnotify-desktop.exe', 'agentnotify-ingress.exe', 'agentnotify-codex-hook.exe', 'agentnotify-antigravity-hook.exe') `
    -UnsignedLeaves @('agentnotify-devin-hook.exe')
  Assert-FailsWith 'ZIP 内 Devin Hook 未签名' 'agentnotify-devin-hook.exe' {
    Assert-ArchiveExecutables -ZipPath $unsignedHookZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }

  # 6c) 全部齐备且指纹匹配：放行，并且逐个返回校验结果。
  $okZip = Join-Path $gateRoot 'all-present.zip'
  New-GateArchive -Path $okZip -SignedLeaves $allLeaves
  try {
    $verified = @(Assert-ArchiveExecutables -ZipPath $okZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion)
    $verifiedNames = @($verified | ForEach-Object { $_.Name })
    if ($verified.Count -ne $allLeaves.Count) {
      Add-Failure "归档门禁返回的产物数量不对：$($verified.Count)（期望 $($allLeaves.Count)）"
    } elseif (@($allLeaves | Where-Object { $verifiedNames -notcontains $_ }).Count -gt 0) {
      Add-Failure "归档门禁没有校验全部产物：$($verifiedNames -join ', ')"
    } else {
      Write-Output "[signature-gate] ok（放行）: ZIP 内主程序与三个 Hook（$($verified.Count) 个）"
    }
  } catch {
    Add-Failure "产物齐备的 ZIP 被拒绝：$($_.Exception.Message)"
  }

  # 6d) 清单本身的缺失、篡改、额外文件和签名者错误也必须 fail-closed。
  $missingManifestZip = Join-Path $gateRoot 'missing-manifest.zip'
  New-GateArchive -Path $missingManifestZip -SignedLeaves $allLeaves -OmitManifest
  Assert-FailsWith 'ZIP 缺少清单' 'RELEASE-MANIFEST.json' {
    Assert-ArchiveExecutables -ZipPath $missingManifestZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }
  $missingSignatureZip = Join-Path $gateRoot 'missing-signature.zip'
  New-GateArchive -Path $missingSignatureZip -SignedLeaves $allLeaves -OmitSignature
  Assert-FailsWith 'ZIP 缺少清单签名' 'RELEASE-MANIFEST.p7s' {
    Assert-ArchiveExecutables -ZipPath $missingSignatureZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }
  $tamperedManifestZip = Join-Path $gateRoot 'tampered-manifest.zip'
  New-GateArchive -Path $tamperedManifestZip -SignedLeaves $allLeaves -TamperManifest
  Assert-FailsWith '清单篡改' 'CMS 签名校验失败' {
    Assert-ArchiveExecutables -ZipPath $tamperedManifestZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }
  $tamperedPayloadZip = Join-Path $gateRoot 'tampered-payload.zip'
  New-GateArchive -Path $tamperedPayloadZip -SignedLeaves $allLeaves -TamperPayload
  Assert-FailsWith '清单声明文件被篡改' '发布包文件与清单不一致' {
    Assert-ArchiveExecutables -ZipPath $tamperedPayloadZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }
  $extraFileZip = Join-Path $gateRoot 'extra-file.zip'
  New-GateArchive -Path $extraFileZip -SignedLeaves $allLeaves -ExtraFiles @('extra.dll')
  Assert-FailsWith '清单未声明额外文件' '文件集合与 ZIP 实际文件集合不一致' {
    Assert-ArchiveExecutables -ZipPath $extraFileZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint $gateManifestThumbprint -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }
  $wrongManifestSignerZip = Join-Path $gateRoot 'wrong-manifest-signer.zip'
  New-GateArchive -Path $wrongManifestSignerZip -SignedLeaves $allLeaves
  Assert-FailsWith '清单签名者错误' '签名者指纹不匹配' {
    Assert-ArchiveExecutables -ZipPath $wrongManifestSignerZip -ExpectedThumbprint $fixtureThumbprint -ExpectedManifestThumbprint ('0' * 40) -RepoRoot $RepoRoot -ExpectedVersion $gateManifestVersion
  }

  # 6e) SHA256SUMS.txt：必须覆盖产物、哈希一致。
  $sumsArtifact = Join-Path $gateRoot 'artifact.bin'
  [IO.File]::WriteAllBytes($sumsArtifact, [byte[]](1, 2, 3, 4, 5))
  $artifactHash = (Get-FileHash -LiteralPath $sumsArtifact -Algorithm SHA256).Hash.ToLowerInvariant()
  $sumsPath = Join-Path $gateRoot 'SHA256SUMS.txt'
  [IO.File]::WriteAllLines($sumsPath, @("$artifactHash  other.zip"), [Text.Encoding]::ASCII)
  Assert-FailsWith 'SHA256SUMS.txt 未覆盖产物' 'SHA256SUMS.txt 未覆盖 artifact.bin' {
    Assert-SumsCoversArtifact -SumsPath $sumsPath -ArtifactPath $sumsArtifact
  }
  $wrongHash = '0' * 64
  [IO.File]::WriteAllLines($sumsPath, @("$wrongHash  artifact.bin"), [Text.Encoding]::ASCII)
  Assert-FailsWith 'SHA256SUMS.txt 哈希不一致' 'SHA256SUMS.txt 里 artifact.bin 的校验值与文件不一致' {
    Assert-SumsCoversArtifact -SumsPath $sumsPath -ArtifactPath $sumsArtifact
  }
  [IO.File]::WriteAllLines($sumsPath, @("$artifactHash  artifact.bin"), [Text.Encoding]::ASCII)
  Assert-Passes 'SHA256SUMS.txt 覆盖且哈希一致' {
    Assert-SumsCoversArtifact -SumsPath $sumsPath -ArtifactPath $sumsArtifact
  }
  }
} finally {
  if ($null -eq $previousPfxBase64) {
    Remove-Item Env:AGENT_NOTIFY_SIGN_PFX_BASE64 -ErrorAction SilentlyContinue
  } else {
    $env:AGENT_NOTIFY_SIGN_PFX_BASE64 = $previousPfxBase64
  }
  if ($null -eq $previousPfxPassword) {
    Remove-Item Env:AGENT_NOTIFY_SIGN_PFX_PASSWORD -ErrorAction SilentlyContinue
  } else {
    $env:AGENT_NOTIFY_SIGN_PFX_PASSWORD = $previousPfxPassword
  }
  if ($manifestPfx) { [Array]::Clear($manifestPfx, 0, $manifestPfx.Length) }
  if ($manifestPassword) { $manifestPassword.Dispose() }
  if ($manifestCertificate) {
    Remove-Item -LiteralPath ("Cert:\CurrentUser\My\" + $manifestCertificate.Thumbprint) -Force -ErrorAction SilentlyContinue
  }
}
if (Test-Path -LiteralPath $gateRoot) { Remove-Item -LiteralPath $gateRoot -Recurse -Force }

if ($failures.Count -gt 0) {
  foreach ($failure in $failures) { Write-Output "[signature-gate] 失败：$failure" }
  exit 1
}
Write-Output '[signature-gate] 通过'
exit 0
