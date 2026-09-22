#Requires -Version 5.1
<#
.SYNOPSIS
  构建 AgentNotify 正式发布产物：桌面端 + ingress 的 ZIP 与安装器，并生成 SHA256SUMS.txt。

.DESCRIPTION
  版本号唯一来源是仓库根 VERSION（可用 -Version 覆盖，发版时传 tag）。
  产物内容为 Tauri 桌面版：agentnotify-desktop.exe、agentnotify-ingress.exe、阶段 D 的
  三个 Hook（agentnotify-codex-hook.exe / agentnotify-antigravity-hook.exe /
  agentnotify-devin-hook.exe）、OpenCode V2 插件模板与四个 Agent 接入助手，
  以及 Devin V2 回复扩展与 Command Code V2 mod 的源文件；
  不再包含旧 Go 版 agent-notify.exe 与旧 Win32 UI。
  构建缓存与临时目录固定在项目所在盘的 Temp 下，不使用 C 盘。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-release.ps1 -Version 2.0.0
#>
param(
  [string]$Version,
  [string]$OutDir,
  [switch]$SkipInstaller
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'signature-common.ps1')
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }

$versionPath = Join-Path $RepoRoot 'VERSION'
if (-not (Test-Path -LiteralPath $versionPath -PathType Leaf)) {
  throw "找不到版本文件：$versionPath"
}
$fileVersion = [IO.File]::ReadAllText($versionPath).Trim()
if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = $fileVersion
}
$Version = $Version.Trim().TrimStart('v')
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "版本号格式无效：$Version（期望 x.y.z）"
}
if ($fileVersion -ne $Version) {
  throw "VERSION 与调用方版本不一致：VERSION=$fileVersion，Version=$Version"
}

$driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
$target = 'x86_64-pc-windows-msvc'
$env:CARGO_HOME = if ($env:CARGO_HOME -like 'D:\*') { $env:CARGO_HOME } else { 'D:\Tools\cargo' }
$env:RUSTUP_HOME = if ($env:RUSTUP_HOME -like 'D:\*') { $env:RUSTUP_HOME } else { 'D:\Tools\rustup' }
$env:CARGO_TARGET_DIR = if ($env:CARGO_TARGET_DIR -like 'D:\*') { $env:CARGO_TARGET_DIR } else { Join-Path $driveRoot 'Temp\agentnotify-rust-target' }
$env:npm_config_cache = if ($env:npm_config_cache -like 'D:\*') { $env:npm_config_cache } else { Join-Path $driveRoot 'Temp\npm-cache' }
$env:TEMP = if ($env:TEMP -like 'D:\*') { $env:TEMP } else { Join-Path $driveRoot 'Temp\agentnotify-temp' }
$env:TMP = $env:TEMP
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR, $env:TEMP, $env:npm_config_cache | Out-Null

$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
  throw "找不到 cargo：$cargo。请按 tools\rust\gate.ps1 的约定准备 D 盘 Rust 工具链。"
}

# 预检安装器工具链：缺 ISCC 时立刻失败，不要先花几分钟构建再报错。
if (-not $SkipInstaller) {
  $isccCandidates = @(
    $env:AGENT_NOTIFY_ISCC,
    (Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    'D:\Temp\InnoSetup\ISCC.exe',
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
  )
  $isccFound = $false
  foreach ($candidate in $isccCandidates) {
    if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { $isccFound = $true; break }
  }
  if (-not $isccFound) {
    throw '找不到 ISCC.exe（Inno Setup 6），无法构建正式安装器。请安装 Inno Setup 6 或设置 AGENT_NOTIFY_ISCC；只构建便携 ZIP 时可加 -SkipInstaller。'
  }
}

# 只有本机确实装了 cargo-xwin + LLVM 时才启用回退：CI 运行器没有 link.exe 的 PATH，
# 但装有 Visual Studio，rustc 能自行定位 MSVC，不能因此误触发回退。
if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
  $llvmHome = if ($env:AGENTNOTIFY_LLVM_HOME) { $env:AGENTNOTIFY_LLVM_HOME } else { 'D:\Tools\LLVM-23.1.1\LLVM' }
  $xwinEnv = Join-Path $PSScriptRoot 'rust\xwin-env.ps1'
  if ((Test-Path -LiteralPath (Join-Path $llvmHome 'bin\clang-cl.exe')) -and (Test-Path -LiteralPath $xwinEnv)) {
    . $xwinEnv
    Write-Warning '未找到 MSVC link.exe，使用本地 cargo-xwin 回退构建。'
  }
}

$releaseDir = Join-Path $env:CARGO_TARGET_DIR "$target\release"
$desktopExe = Join-Path $releaseDir 'agentnotify-desktop.exe'
$ingressExe = Join-Path $releaseDir 'agentnotify-ingress.exe'
# 阶段 D 的三个 Hook：与桌面端、ingress 共用同一 target 与 --locked 约定。
$hookBuilds = @(
  [pscustomobject]@{ Package = 'agentnotify-codex-hook'; ExeName = 'agentnotify-codex-hook.exe' },
  [pscustomobject]@{ Package = 'agentnotify-antigravity-hook'; ExeName = 'agentnotify-antigravity-hook.exe' },
  [pscustomobject]@{ Package = 'agentnotify-devin-hook'; ExeName = 'agentnotify-devin-hook.exe' }
)
$buildRoot = Join-Path $driveRoot ('Temp\agentnotify-release-' + [guid]::NewGuid().ToString('N'))
$stagedDesktop = Join-Path $buildRoot 'agentnotify-desktop.exe'
$stagedIngress = Join-Path $buildRoot 'agentnotify-ingress.exe'
$stagedHooks = @($hookBuilds | ForEach-Object { Join-Path $buildRoot $_.ExeName })
New-Item -ItemType Directory -Force -Path $buildRoot | Out-Null

try {
  # 1) 前端资源必须先就绪：直接调用 cargo 不会执行 Tauri 的 beforeBuildCommand。
  $uiRoot = Join-Path $RepoRoot 'apps\desktop-ui'
  Push-Location $uiRoot
  try {
    & npm ci
    if ($LASTEXITCODE -ne 0) { throw "npm ci 失败 exit=$LASTEXITCODE" }
    & npm run build
    if ($LASTEXITCODE -ne 0) { throw "前端构建失败 exit=$LASTEXITCODE" }
  } finally {
    Pop-Location
  }
  $frontendDist = Join-Path $uiRoot 'dist\index.html'
  if (-not (Test-Path -LiteralPath $frontendDist -PathType Leaf)) {
    throw "前端产物缺失：$frontendDist"
  }

  # 2) 构建可执行文件。custom-protocol 让 Tauri 嵌入前端资源而不是读 devUrl。
  Push-Location $RepoRoot
  try {
    & $cargo build -p agentnotify-desktop --release --locked --target $target --features tauri/custom-protocol
    if ($LASTEXITCODE -ne 0) { throw "桌面端构建失败 exit=$LASTEXITCODE" }
    & $cargo build -p agentnotify-ingress --release --locked --target $target
    if ($LASTEXITCODE -ne 0) { throw "ingress 构建失败 exit=$LASTEXITCODE" }
    foreach ($hook in $hookBuilds) {
      & $cargo build -p $hook.Package --release --locked --target $target
      if ($LASTEXITCODE -ne 0) { throw "$($hook.Package) 构建失败 exit=$LASTEXITCODE" }
    }
  } finally {
    Pop-Location
  }
  foreach ($leaf in @($desktopExe, $ingressExe)) {
    if (-not (Test-Path -LiteralPath $leaf -PathType Leaf)) {
      throw "构建产物缺失：$leaf"
    }
  }
  Copy-Item -LiteralPath $desktopExe -Destination $stagedDesktop -Force
  Copy-Item -LiteralPath $ingressExe -Destination $stagedIngress -Force
  foreach ($hook in $hookBuilds) {
    $hookExe = Join-Path $releaseDir $hook.ExeName
    if (-not (Test-Path -LiteralPath $hookExe -PathType Leaf)) {
      throw "构建产物缺失：$hookExe"
    }
    Copy-Item -LiteralPath $hookExe -Destination (Join-Path $buildRoot $hook.ExeName) -Force
  }

  # 3) 与安装器共用同一签名工具约定（<tool> sign <file>）；设置 AGENT_NOTIFY_SIGNTOOL 后
  #    所有可执行文件（桌面端、ingress、三个 Hook）都会签名，并强制校验指纹等于客户端内置的信任指纹。
  if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_SIGNTOOL)) {
    $signTool = $env:AGENT_NOTIFY_SIGNTOOL.Trim()
    if (Test-Path -LiteralPath $signTool -PathType Leaf) {
      $signTool = (Resolve-Path -LiteralPath $signTool).Path
    } else {
      $signCommand = Get-Command $signTool -ErrorAction SilentlyContinue
      if (-not $signCommand) { throw "找不到签名工具：$signTool" }
      $signTool = $signCommand.Source
    }
    $expectedThumbprint = Get-ExpectedSignatureThumbprint -RepoRoot $RepoRoot
    foreach ($leaf in @($stagedDesktop, $stagedIngress) + $stagedHooks) {
      & $signTool sign $leaf
      if ($LASTEXITCODE -ne 0) { throw "签名失败（$leaf）exit=$LASTEXITCODE" }
      $actualThumbprint = Get-VerifiedSignatureThumbprint -Path $leaf -ExpectedThumbprint $expectedThumbprint
      Write-Output "[release] 已签名 $([IO.Path]::GetFileName($leaf))（$actualThumbprint）"
    }
  }

  # 4) ZIP：便携与开发用途，内容与正式安装器一致，不含旧 Go 产物与用户状态。
  $pluginSource = Join-Path $RepoRoot 'plugin\rust\agent-notify.ts'
  $pluginSourceText = [IO.File]::ReadAllText($pluginSource)
  if (-not [regex]::IsMatch($pluginSourceText, '(?m)^const BAKED_INGRESS = ""\s*$')) {
    throw '发布用插件必须保持 BAKED_INGRESS 为空，由安装器绑定到目标机器。'
  }
  $commandCodeSource = Join-Path $RepoRoot 'plugin\commandcode-v2\agent-notify.ts'
  if (-not [regex]::IsMatch([IO.File]::ReadAllText($commandCodeSource), '(?m)^const BAKED_INGRESS = ""\s*$')) {
    throw '发布用 Command Code mod 必须保持 BAKED_INGRESS 为空，由安装器绑定到目标机器。'
  }
  $devinExtensionFiles = @('package.json', 'extension.js', 'acp-bridge.js')
  $hookInstallerScripts = @(
    'install-opencode-v2.ps1',
    'install-codex-v2.ps1',
    'install-antigravity-v2.ps1',
    'install-devin-v2.ps1',
    'install-commandcode-v2.ps1'
  )

  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  $zipPath = Join-Path $OutDir "Agent-notify-v$Version.zip"
  if (Test-Path -LiteralPath $zipPath) { [IO.File]::Delete($zipPath) }

  Add-Type -AssemblyName System.IO.Compression.FileSystem
  Add-Type -AssemblyName System.IO.Compression
  $archive = [IO.Compression.ZipFile]::Open($zipPath, [IO.Compression.ZipArchiveMode]::Create)

  function Add-ReleaseFile {
    param(
      [IO.Compression.ZipArchive]$Zip,
      [string]$Source,
      [string]$EntryName
    )
    if (-not (Test-Path -LiteralPath $Source)) {
      throw "Release source is missing: $Source"
    }
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
      $Zip,
      $Source,
      $EntryName,
      [IO.Compression.CompressionLevel]::Optimal
    )
  }

  try {
    Add-ReleaseFile $archive $stagedDesktop 'Agent-notify/bin/agentnotify-desktop.exe'
    Add-ReleaseFile $archive $stagedIngress 'Agent-notify/bin/agentnotify-ingress.exe'
    foreach ($hook in $hookBuilds) {
      Add-ReleaseFile $archive (Join-Path $buildRoot $hook.ExeName) "Agent-notify/bin/$($hook.ExeName)"
    }
    Add-ReleaseFile $archive $pluginSource 'Agent-notify/plugin/agent-notify.ts'
    foreach ($name in $devinExtensionFiles) {
      Add-ReleaseFile $archive (Join-Path $RepoRoot "plugin\devin-extension-v2\$name") "Agent-notify/plugin/devin-extension-v2/$name"
    }
    Add-ReleaseFile $archive $commandCodeSource 'Agent-notify/plugin/commandcode-v2/agent-notify.ts'
    foreach ($name in $hookInstallerScripts) {
      Add-ReleaseFile $archive (Join-Path $RepoRoot "tools\hooks\$name") "Agent-notify/tools/hooks/$name"
    }
    # VERSION 由解析后的版本生成，包内元数据不可能与刚构建的程序不一致。
    $versionEntry = $archive.CreateEntry('Agent-notify/VERSION', [IO.Compression.CompressionLevel]::Optimal)
    $versionWriter = New-Object IO.StreamWriter($versionEntry.Open(), (New-Object Text.UTF8Encoding($false)))
    try {
      $versionWriter.Write($Version)
    } finally {
      $versionWriter.Dispose()
    }
    foreach ($name in @('README.md', 'CHANGELOG.md', 'SECURITY.md', 'CONTRIBUTING.md', 'LICENSE', '.env.example')) {
      Add-ReleaseFile $archive (Join-Path $RepoRoot $name) "Agent-notify/$name"
    }
    foreach ($name in @('ARCHITECTURE.md', 'TROUBLESHOOTING.md')) {
      Add-ReleaseFile $archive (Join-Path $RepoRoot "docs\$name") "Agent-notify/docs/$name"
    }
  } finally {
    $archive.Dispose()
  }

  $forbiddenReleaseFiles = @(
    'clawbot.json',
    'config.json',
    'opencode.off',
    'codex.off',
    'antigravity.off',
    'devin.off',
    'commandcode.off',
    'push.log',
    'opencode-sent.json',
    'agent-notify-install.json',
    'agent-notify.exe'
  )
  $verificationArchive = [IO.Compression.ZipFile]::OpenRead($zipPath)
  try {
    $entryNames = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $verificationArchive.Entries) {
      [void]$entryNames.Add($entry.FullName)
      $leaf = [IO.Path]::GetFileName($entry.FullName)
      if ($forbiddenReleaseFiles -contains $leaf -or $leaf.EndsWith('.log', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Release archive contains forbidden entry: $($entry.FullName)"
      }
    }
    # 阶段 D 的四个适配器产物必须真的进包：漏一个，安装器按任务接入时就会失败。
    $requiredReleaseEntries = @(
      'Agent-notify/bin/agentnotify-desktop.exe',
      'Agent-notify/bin/agentnotify-ingress.exe'
    )
    foreach ($hook in $hookBuilds) { $requiredReleaseEntries += "Agent-notify/bin/$($hook.ExeName)" }
    $requiredReleaseEntries += 'Agent-notify/plugin/agent-notify.ts'
    foreach ($name in $devinExtensionFiles) { $requiredReleaseEntries += "Agent-notify/plugin/devin-extension-v2/$name" }
    $requiredReleaseEntries += 'Agent-notify/plugin/commandcode-v2/agent-notify.ts'
    foreach ($name in $hookInstallerScripts) { $requiredReleaseEntries += "Agent-notify/tools/hooks/$name" }
    $requiredReleaseEntries += 'Agent-notify/VERSION'
    foreach ($required in $requiredReleaseEntries) {
      if (-not $entryNames.Contains($required)) {
        throw "Release archive is missing required entry: $required"
      }
    }
  } finally {
    $verificationArchive.Dispose()
  }

  if (-not $SkipInstaller) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\build-installer.ps1') `
      -Version $Version `
      -OutDir $OutDir `
      -ExePath $stagedDesktop `
      -IngressPath $stagedIngress `
      -CodexHookPath (Join-Path $buildRoot 'agentnotify-codex-hook.exe') `
      -AntigravityHookPath (Join-Path $buildRoot 'agentnotify-antigravity-hook.exe') `
      -DevinHookPath (Join-Path $buildRoot 'agentnotify-devin-hook.exe')
    if ($LASTEXITCODE -ne 0) { throw "安装器构建失败 exit=$LASTEXITCODE" }
  }

  $zipArtifact = Get-Item -LiteralPath $zipPath -ErrorAction Stop
  $artifacts = @($zipArtifact)
  if (-not $SkipInstaller) {
    $artifacts += Get-Item -LiteralPath (Join-Path $OutDir "Agent-notify-Setup-v$Version.exe") -ErrorAction Stop
  }
  $lines = foreach ($artifact in $artifacts) {
    $hash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash.ToLower()
    "$hash  $($artifact.Name)"
  }
  $sumPath = Join-Path $OutDir 'SHA256SUMS.txt'
  [IO.File]::WriteAllLines($sumPath, $lines, [Text.Encoding]::ASCII)
  Write-Output "[release] archive: $zipPath"
  if (-not $SkipInstaller) {
    Write-Output "[release] installer: $($artifacts[1].FullName)"
  }
  Write-Output "[release] sums:    $sumPath"
} finally {
  if (Test-Path -LiteralPath $buildRoot) { [IO.Directory]::Delete($buildRoot, $true) }
}
