<#
.SYNOPSIS
  报告（并可选清理）本机构建缓存占用。

.DESCRIPTION
  默认只读：列出 Rust target、npm 缓存、Playwright 浏览器与测试临时目录的占用，
  并单独列出各 target 布局下的 incremental 目录（纯中间产物）。

  -PruneIncremental  删除各 target 布局下的 incremental 目录（最安全，回收最多）。
  -RemoveRustTarget  删除整个 Rust target 目录（等同 cargo clean，最彻底，下次全量重编）。

  路径遵循仓库约定：CARGO_TARGET_DIR / npm_config_cache / PLAYWRIGHT_BROWSERS_PATH 优先，
  未设置时回落到 D 盘默认位置。
#>
param(
    [switch]$PruneIncremental,
    [switch]$RemoveRustTarget
)

$ErrorActionPreference = 'Stop'

function Get-UsedGB {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if (-not $sum) { return 0 }
    return [math]::Round($sum / 1GB, 2)
}

$rustTarget = $env:CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($rustTarget)) { $rustTarget = 'D:\Temp\agentnotify-rust-target' }
$npmCache = $env:npm_config_cache
if ([string]::IsNullOrWhiteSpace($npmCache)) { $npmCache = 'D:\Temp\npm-cache' }
$browsers = $env:PLAYWRIGHT_BROWSERS_PATH
if ([string]::IsNullOrWhiteSpace($browsers)) { $browsers = 'D:\Tools\playwright-browsers' }
$testTemp = 'D:\Temp\agentnotify-temp'

Write-Output '[cache] 当前占用：'
foreach ($item in @(
        @{ Name = 'Rust target 目录'; Path = $rustTarget },
        @{ Name = 'npm 缓存'; Path = $npmCache },
        @{ Name = 'Playwright 浏览器'; Path = $browsers },
        @{ Name = '测试临时目录'; Path = $testTemp }
    )) {
    Write-Output ('  {0,-18} {1,8:N2} GB   {2}' -f $item.Name, (Get-UsedGB $item.Path), $item.Path)
}

$layouts = @(Get-ChildItem -LiteralPath $rustTarget -Directory -Force -ErrorAction SilentlyContinue)
foreach ($layout in $layouts) {
    $incremental = Join-Path $layout.FullName 'debug\incremental'
    if (Test-Path -LiteralPath $incremental) {
        Write-Output ('  incremental({0}) {1,8:N2} GB' -f $layout.Name, (Get-UsedGB $incremental))
    }
}

if ($RemoveRustTarget) {
    if (Test-Path -LiteralPath $rustTarget) {
        Write-Output "[cache] 删除整个 Rust target 目录：$rustTarget"
        Remove-Item -LiteralPath $rustTarget -Recurse -Force
    }
    Write-Output '[cache] 完成（下次构建会全量重编）。'
    return
}

if ($PruneIncremental) {
    $freed = 0.0
    foreach ($layout in $layouts) {
        $incremental = Join-Path $layout.FullName 'debug\incremental'
        if (Test-Path -LiteralPath $incremental) {
            $freed += Get-UsedGB $incremental
            Write-Output "[cache] 删除 $incremental"
            Remove-Item -LiteralPath $incremental -Recurse -Force
        }
    }
    Write-Output ('[cache] 已回收约 {0:N2} GB（incremental 中间产物，下次编译会重建）。' -f $freed)
}
