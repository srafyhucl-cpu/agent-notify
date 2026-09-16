#Requires -Version 5.1
<#
.SYNOPSIS
  Publish an already-built Agent-notify archive to the public binary-only repository.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\publish-release.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\publish-release.ps1 -Version 1.11.0
#>
param(
  [string]$Version,
  [string]$Repository = 'srafyhucl-cpu/agent-notify-releases',
  [string]$DistDir
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
if (-not $DistDir) { $DistDir = Join-Path $RepoRoot 'dist' }

if (-not $Version) {
  $versionFile = Join-Path $RepoRoot 'internal\app\version.go'
  $match = Select-String -Path $versionFile -Pattern 'Version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $match) { throw "Could not read Version from $versionFile" }
  $Version = $match.Matches[0].Groups[1].Value
}

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

  $previousErrorAction = $ErrorActionPreference
  $ErrorActionPreference = 'SilentlyContinue'
  & $gh.Source release view $tag --repo $Repository *> $null
  $releaseExists = $LASTEXITCODE -eq 0
  $ErrorActionPreference = $previousErrorAction
  if ($releaseExists) {
    & $gh.Source release upload $tag $setupPath $zipPath $sumsPath --repo $Repository --clobber
    if ($LASTEXITCODE -ne 0) { throw "gh release upload failed: exit=$LASTEXITCODE" }
    & $gh.Source release edit $tag --repo $Repository --title "Agent-notify $tag" --notes-file $notesPath --latest
    if ($LASTEXITCODE -ne 0) { throw "gh release edit failed: exit=$LASTEXITCODE" }
    Write-Output "[publish] updated public release: $Repository $tag"
  } else {
    & $gh.Source release create $tag $setupPath $zipPath $sumsPath --repo $Repository --title "Agent-notify $tag" --notes-file $notesPath --latest
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed: exit=$LASTEXITCODE" }
    Write-Output "[publish] created public release: $Repository $tag"
  }
} finally {
  if (Test-Path -LiteralPath $notesPath) {
    Remove-Item -LiteralPath $notesPath -Force
  }
}
