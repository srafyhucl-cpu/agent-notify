param(
  [string]$Source = '',
  [string]$Destination = '',
  [string]$Ingress = ''
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Source)) {
  $Source = Join-Path $PSScriptRoot '..\..\plugin\rust\agent-notify.ts'
}
if ([string]::IsNullOrWhiteSpace($Destination)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Destination = Join-Path $homeDirectory '.config\opencode\plugins\agent-notify.ts'
}
if ([string]::IsNullOrWhiteSpace($Ingress)) {
  $homeDirectory = [Environment]::GetFolderPath('UserProfile')
  $Ingress = Join-Path $homeDirectory 'bin\agentnotify-ingress.exe'
}

$sourcePath = [IO.Path]::GetFullPath($Source)
$destinationPath = [IO.Path]::GetFullPath($Destination)
$ingressPath = [IO.Path]::GetFullPath($Ingress)

if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
  throw "OpenCode plugin source not found: $sourcePath"
}
if (-not (Test-Path -LiteralPath $ingressPath -PathType Leaf)) {
  throw "Ingress executable not found: $ingressPath"
}
if ([IO.Path]::GetExtension($ingressPath) -ine '.exe') {
  throw "Ingress executable must be an .exe file: $ingressPath"
}

$destinationDirectory = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Force -Path $destinationDirectory | Out-Null

$content = [IO.File]::ReadAllText($sourcePath)
$escapedIngress = $ingressPath.Replace('\', '\\').Replace('"', '\"')
$pattern = 'const BAKED_INGRESS = ""'
if (-not $content.Contains($pattern)) {
  throw "OpenCode plugin does not contain the expected ingress marker."
}
$content = $content.Replace($pattern, "const BAKED_INGRESS = `"$escapedIngress`"")

$temporary = Join-Path $destinationDirectory ('.agent-notify.' + [guid]::NewGuid().ToString('N') + '.tmp')
try {
  [IO.File]::WriteAllText(
    $temporary,
    $content,
    (New-Object Text.UTF8Encoding($false))
  )
  Move-Item -LiteralPath $temporary -Destination $destinationPath -Force
} finally {
  if (Test-Path -LiteralPath $temporary) {
    Remove-Item -LiteralPath $temporary -Force
  }
}

Write-Output "Installed OpenCode V2 plugin: $destinationPath"
