$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$env:npm_config_cache = if ($env:npm_config_cache -like 'D:\*') { $env:npm_config_cache } else { 'D:\Temp\npm-cache' }
$env:TEMP = if ($env:TEMP -like 'D:\*') { $env:TEMP } else { 'D:\Temp\agentnotify-temp' }
$env:TMP = $env:TEMP
$env:PLAYWRIGHT_BROWSERS_PATH = if ($env:PLAYWRIGHT_BROWSERS_PATH -like 'D:\*') { $env:PLAYWRIGHT_BROWSERS_PATH } else { 'D:\Tools\playwright-browsers' }
New-Item -ItemType Directory -Force -Path $env:npm_config_cache,$env:TEMP | Out-Null

Push-Location (Join-Path $root 'apps\desktop-ui')
try {
    npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    $stableBindingsBefore = (Get-FileHash -LiteralPath 'src\bridge\types.ts' -Algorithm SHA256).Hash
    npm run bridge:generate
    if ($LASTEXITCODE -ne 0) { throw 'HostBridge generation failed' }
    $stableBindingsAfter = (Get-FileHash -LiteralPath 'src\bridge\types.ts' -Algorithm SHA256).Hash
    if ($stableBindingsBefore -ne $stableBindingsAfter) { throw 'HostBridge bindings are stale' }
    npm run typecheck
    if ($LASTEXITCODE -ne 0) { throw 'TypeScript check failed' }
    npm run test -- --run
    if ($LASTEXITCODE -ne 0) { throw 'Vitest failed' }
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'Vite build failed' }
    # 本地约定把浏览器装在 D:\Tools\playwright-browsers；CI 与全新机器上没有这份缓存，
    # 缺失时按需安装一次（PLAYWRIGHT_BROWSERS_PATH 已在上方解析为当前机器的目标目录）。
    $chromiumReady = @(Get-ChildItem -LiteralPath $env:PLAYWRIGHT_BROWSERS_PATH -Directory -Filter 'chromium-*' -ErrorAction SilentlyContinue).Count -gt 0
    if (-not $chromiumReady) {
        npx playwright install chromium
        if ($LASTEXITCODE -ne 0) { throw 'Playwright browser install failed' }
    }
    npm run test:e2e
    if ($LASTEXITCODE -ne 0) { throw 'Playwright UI checks failed' }
}
finally {
    Pop-Location
}

powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'tools\rust\gate.ps1')
