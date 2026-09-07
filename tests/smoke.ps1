#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 冒烟测试：语法检查 + DryRun 渲染 + watcher 幂等性。不真推，不碰真实配置。
#>
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $RepoRoot

$files = @(
  'scripts\notify-ai.ps1',
  'scripts\codex-notify.ps1',
  'scripts\codex-notify-watch.ps1',
  'install.ps1',
  'uninstall.ps1'
)
foreach ($f in $files) {
  $tokens = $null
  $errs = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $RepoRoot $f), [ref]$tokens, [ref]$errs)
  if ($errs.Count -gt 0) { throw "$f 语法失败：$($errs[0].Message)" }
  Write-Output "[ok] syntax $f"
}

# DryRun：不需要 token，验证渲染链路
$dry = & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\notify-ai.ps1 -DryRun -Title 'smoke' -Summary 'hello **bold** smoke' 2>&1
if ($dry -notmatch 'bold') { throw "DryRun 渲染失败：$dry" }
Write-Output '[ok] notify-ai DryRun'

# watcher：临时 config，验证 exe->wrapper 改写、幂等、自定义不动
$tmp = Join-Path $env:TEMP 'linkweixin-smoke'
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  $cfg = Join-Path $tmp 'config.toml'
  Set-Content -Path $cfg -Value ('model = "x"' + "`n" + 'notify = [ "old", "codex-computer-use.exe", "turn-ended" ]') -Encoding utf8
  $env:CODEX_CONFIG = $cfg
  $env:CODEX_NOTIFY_WRAPPER = 'C:/smoke-test/bin/codex-notify.ps1'
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\codex-notify-watch.ps1
  $after1 = Get-Content $cfg -Raw
  if ($after1 -notmatch 'codex-notify\.ps1') { throw 'watcher 未改写 exe->wrapper' }
  if (-not (Test-Path "$cfg.bak-notify-wrapper")) { throw 'watcher 未备份' }
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\codex-notify-watch.ps1
  if ((Get-Content $cfg -Raw) -ne $after1) { throw 'watcher 不幂等' }
  Write-Output '[ok] watcher patch + idempotent'
  Set-Content -Path $cfg -Value 'notify = [ "my-custom-tool" ]' -Encoding utf8
  Remove-Item "$cfg.bak-notify-wrapper" -Force -ErrorAction SilentlyContinue
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\codex-notify-watch.ps1
  if ((Get-Content $cfg -Raw) -notmatch 'my-custom-tool') { throw 'watcher 误改自定义配置' }
  Write-Output '[ok] watcher custom untouched'
} finally {
  $env:CODEX_CONFIG = $null
  $env:CODEX_NOTIFY_WRAPPER = $null
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output 'SMOKE ALL GREEN'
