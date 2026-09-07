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
  'scripts\notify-toggle.ps1',
  'scripts\linkweixin-widget.ps1',
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

# codex-notify：用 Start-Job 隔离跑，桩脚本捕获调参。不联网、不碰真实配置。
# 注意：不能用 powershell.exe -File 直接调——PS5.1 调原生命令会剥掉/拆散 JSON
# 内嵌双引号（上已实测），真机上 codex 用 argv 数组 spawn 则无此问题；
# 且 wrapper 尾部 exit 0 不能跑在本进程。Job 传参走对象序列化，字符串一字不差。
$tmp2 = Join-Path $env:TEMP 'linkweixin-smoke2'
New-Item -ItemType Directory -Force -Path $tmp2 | Out-Null
try {
  $stub = Join-Path $tmp2 'stub.ps1'
  Set-Content -Path $stub -Value '"$args" | Out-File -FilePath "$env:SMOKE_GOT" -Encoding utf8' -Encoding utf8
  $gotPath = Join-Path $tmp2 'got.txt'
  $nolocal = Join-Path $tmp2 'nolocal'
  $sample = '{"last-assistant-message":"hello **world** smoke","input-messages":["帮我写个脚本测试一下"]}'
  $job = Start-Job -ScriptBlock {
    param($repo, $stubPath, $gotFile, $fakeLocal, $json)
    $env:SMOKE_GOT = $gotFile
    $env:NOTIFY_AI_SCRIPT = $stubPath
    $env:LOCALAPPDATA = $fakeLocal
    & (Join-Path $repo 'scripts\codex-notify.ps1') 'turn-ended' $json
  } -ArgumentList $RepoRoot, $stub, $gotPath, $nolocal, $sample
  $job | Wait-Job | Out-Null
  $jout = Receive-Job $job
  Remove-Job $job -Force -ErrorAction SilentlyContinue
  if ($job.State -ne 'Completed') { throw "codex-notify job 未正常结束：$($job.State) $jout" }
  if (-not (Test-Path $gotPath)) { throw 'codex-notify 未调到桩脚本' }
  $got = Get-Content $gotPath -Raw
  if ($got -notmatch '【codex】帮我写个脚本测试一下') { throw "codex-notify 标题不对：$got" }
  if ($got -notmatch 'hello \*\*world\*\* smoke') { throw "codex-notify 摘要未原文透传：$got" }
  Write-Output '[ok] codex-notify passthru + args'
} finally {
  Remove-Item $tmp2 -Recurse -Force -ErrorAction SilentlyContinue
}

# 防闪屏回归：两处拉起子 powershell 必须带 -WindowStyle Hidden（读原文跨行匹配）
foreach ($f in @('opencode-plugin\notify-pushplus.ts', 'scripts\codex-notify.ps1')) {
  $raw = [IO.File]::ReadAllText((Join-Path $RepoRoot $f))
  if ($raw -notmatch '(?s)-WindowStyle.\s*,?\s*.Hidden') {
    throw "$f 缺 -WindowStyle Hidden，任务完成时会闪命令行窗口"
  }
  Write-Output "[ok] no-flash $f"
}

# notify-toggle：临时 -MarkerPath 隔离，断言 off->on->off + 回显。不碰真实 marker。
$tmp3 = Join-Path $env:TEMP 'linkweixin-smoke-toggle'
New-Item -ItemType Directory -Force -Path $tmp3 | Out-Null
try {
  $marker = Join-Path $tmp3 'notify-pushplus.off'
  Remove-Item $marker -Force -ErrorAction SilentlyContinue
  $t1 = & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\notify-toggle.ps1 -MarkerPath $marker 2>&1
  if ($LASTEXITCODE -ne 0) { throw "toggle 翻转 exit 非 0：$LASTEXITCODE" }
  if ("$t1" -notmatch 'OFF') { throw "toggle 翻转回显不对（期望 OFF）：$t1" }
  if (-not (Test-Path $marker)) { throw 'toggle 翻转未建 marker' }
  Write-Output '[ok] toggle flip -> OFF'
  $t2 = & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\notify-toggle.ps1 -MarkerPath $marker 2>&1
  if ("$t2" -notmatch 'ON') { throw "toggle 翻转回显不对（期望 ON）：$t2" }
  if (Test-Path $marker) { throw 'toggle 翻转未删 marker' }
  Write-Output '[ok] toggle flip -> ON'
  $t3 = & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\notify-toggle.ps1 -MarkerPath $marker -Off 2>&1
  if ("$t3" -notmatch 'OFF' -or -not (Test-Path $marker)) { throw "toggle -Off 不对：$t3" }
  $t4 = & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\notify-toggle.ps1 -MarkerPath $marker -On 2>&1
  if ("$t4" -notmatch 'ON' -or (Test-Path $marker)) { throw "toggle -On 不对：$t4" }
  Write-Output '[ok] toggle -On/-Off'
} finally {
  Remove-Item $tmp3 -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output 'SMOKE ALL GREEN'
