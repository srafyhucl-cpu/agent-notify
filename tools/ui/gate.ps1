$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$env:npm_config_cache = if ($env:npm_config_cache -like 'D:\*') { $env:npm_config_cache } else { 'D:\Temp\npm-cache' }
$env:TEMP = if ($env:TEMP -like 'D:\*') { $env:TEMP } else { 'D:\Temp\agentnotify-temp' }
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:npm_config_cache,$env:TEMP | Out-Null

Push-Location (Join-Path $root 'apps\desktop-ui')
try {
    npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    npm run typecheck
    if ($LASTEXITCODE -ne 0) { throw 'TypeScript check failed' }
    npm run test -- --run
    if ($LASTEXITCODE -ne 0) { throw 'Vitest failed' }
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'Vite build failed' }
}
finally {
    Pop-Location
}

powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'tools\rust\gate.ps1')
