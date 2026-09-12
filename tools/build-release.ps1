#Requires -Version 5.1
<#
.SYNOPSIS
  Build Agent-notify-v<version>.zip without a directory staging tree.

.DESCRIPTION
  Version source: internal/app/version.go.
  The archive always contains a top-level Agent-notify directory.
#>
param(
  [string]$Version,
  [string]$OutDir
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }

if (-not $Version) {
  $versionFile = Join-Path $RepoRoot 'internal\app\version.go'
  $match = Select-String -Path $versionFile -Pattern 'Version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $match) { throw "Could not read Version from $versionFile" }
  $Version = $match.Matches[0].Groups[1].Value
}

function Resolve-GoCommand {
  $candidates = @()
  if (-not [string]::IsNullOrWhiteSpace($env:AGENT_NOTIFY_GO)) { $candidates += $env:AGENT_NOTIFY_GO }
  $onPath = Get-Command go.exe -ErrorAction SilentlyContinue
  if ($onPath) { $candidates += $onPath.Source }
  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path $candidate)) { return $candidate }
  }
  return $null
}

$goExe = Resolve-GoCommand
if (-not $goExe) { throw 'go.exe was not found' }

$driveRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($RepoRoot)).TrimEnd('\')
$cacheRoot = Join-Path $driveRoot 'Temp\agent-notify-go'
if ([string]::IsNullOrWhiteSpace($env:GOPATH)) { $env:GOPATH = $cacheRoot }
if ([string]::IsNullOrWhiteSpace($env:GOMODCACHE)) { $env:GOMODCACHE = Join-Path $cacheRoot 'pkg\mod' }
if ([string]::IsNullOrWhiteSpace($env:GOCACHE)) { $env:GOCACHE = Join-Path $cacheRoot 'build' }

$buildRoot = Join-Path $driveRoot ('Temp\agent-notify-build-' + [guid]::NewGuid().ToString('N'))
$tempExe = Join-Path $buildRoot 'agent-notify.exe'
New-Item -ItemType Directory -Force -Path $buildRoot | Out-Null

try {
  $commit = 'unknown'
  try {
    $commit = (& git rev-parse --short HEAD 2>$null).Trim()
    if (-not $commit) { $commit = 'unknown' }
  } catch {
    $commit = 'unknown'
  }
  $buildTime = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  $module = 'github.com/srafyhucl-cpu/agent-notify/internal/app'
  $ldflags = "-H windowsgui -s -w -X $module.Version=$Version -X $module.Commit=$commit -X $module.BuildTime=$buildTime"

  Push-Location $RepoRoot
  try {
    & $goExe build -ldflags $ldflags -trimpath -o $tempExe '.\cmd\agent-notify\'
    if ($LASTEXITCODE -ne 0) { throw "go build failed: exit=$LASTEXITCODE" }
  } finally {
    Pop-Location
  }

  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  $zipName = "Agent-notify-v$Version.zip"
  $zipPath = Join-Path $OutDir $zipName
  if (Test-Path -LiteralPath $zipPath) { [IO.File]::Delete($zipPath) }

  $pluginSource = Join-Path $RepoRoot 'plugin\agent-notify.ts'
  $pluginSourceText = [IO.File]::ReadAllText($pluginSource)
  if (-not [regex]::IsMatch($pluginSourceText, '(?m)^const BAKED_BIN = ""\s*$')) {
    throw 'Release plugin must keep BAKED_BIN empty so the installer can bind it to the target machine.'
  }

	Add-Type -AssemblyName System.IO.Compression.FileSystem
	Add-Type -AssemblyName System.IO.Compression
	$archive = [IO.Compression.ZipFile]::Open(
    $zipPath,
    [IO.Compression.ZipArchiveMode]::Create
  )

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
    Add-ReleaseFile $archive $tempExe 'Agent-notify/bin/agent-notify.exe'
    Add-ReleaseFile $archive $pluginSource 'Agent-notify/plugin/agent-notify.ts'
    # VERSION is generated from the resolved --Version so packaged metadata can
    # never drift from the executable that was just built.
    $versionEntry = $archive.CreateEntry('Agent-notify/VERSION', [IO.Compression.CompressionLevel]::Optimal)
    $versionWriter = New-Object IO.StreamWriter($versionEntry.Open(), (New-Object Text.UTF8Encoding($false)))
    try {
      $versionWriter.Write($Version)
    } finally {
      $versionWriter.Dispose()
    }
    foreach ($name in @('install.ps1', 'uninstall.ps1', 'README.md', 'CHANGELOG.md', 'SECURITY.md', 'CONTRIBUTING.md', 'LICENSE', '.env.example')) {
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
    'push.log',
    'opencode-sent.json',
    'agent-notify-install.json'
  )
  $verificationArchive = [IO.Compression.ZipFile]::OpenRead($zipPath)
  try {
    foreach ($entry in $verificationArchive.Entries) {
      $leaf = [IO.Path]::GetFileName($entry.FullName)
      if ($forbiddenReleaseFiles -contains $leaf -or $leaf.EndsWith('.log', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Release archive contains user state: $($entry.FullName)"
      }
    }
  } finally {
    $verificationArchive.Dispose()
  }

  $hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLower()
  $sumPath = Join-Path $OutDir 'SHA256SUMS.txt'
  [IO.File]::WriteAllText(
    $sumPath,
    "$hash  $zipName`r`n",
    [Text.Encoding]::ASCII
  )
  Write-Output "[release] archive: $zipPath"
  Write-Output "[release] sha256:  $hash"
  Write-Output "[release] sums:    $sumPath"
} finally {
  if (Test-Path -LiteralPath $tempExe) { [IO.File]::Delete($tempExe) }
  if (Test-Path -LiteralPath $buildRoot) { [IO.Directory]::Delete($buildRoot, $false) }
}
