#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 冒烟测试：语法检查 + DryRun 渲染 + watcher 幂等性。不真推，不碰真实配置。
#>
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $RepoRoot

$files = @(
  'src\notify-ai.ps1',
  'src\codex-notify.ps1',
  'src\codex-notify-watch.ps1',
  'src\notify-toggle.ps1',
  'src\linkweixin-widget.ps1',
  'install.ps1',
  'uninstall.ps1'
) + @(Get-ChildItem -Path (Join-Path $RepoRoot 'src\lib') -Recurse -File -Include *.ps1, *.psm1, *.psd1 |
    ForEach-Object { $_.FullName.Substring($RepoRoot.Length + 1) })
foreach ($f in $files) {
  $tokens = $null
  $errs = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $RepoRoot $f), [ref]$tokens, [ref]$errs)
  if ($errs.Count -gt 0) { throw "$f 语法失败：$($errs[0].Message)" }
  Write-Output "[ok] syntax $f"
}

# DryRun：不需要 token，验证渲染链路
$dry = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-ai.ps1 -DryRun -Title 'smoke' -Summary 'hello **bold** smoke' 2>&1
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
  & powershell -NoProfile -ExecutionPolicy Bypass -File src\codex-notify-watch.ps1
  $after1 = Get-Content $cfg -Raw
  if ($after1 -notmatch 'codex-notify\.ps1') { throw 'watcher 未改写 exe->wrapper' }
  if (-not (Test-Path "$cfg.bak-notify-wrapper")) { throw 'watcher 未备份' }
  & powershell -NoProfile -ExecutionPolicy Bypass -File src\codex-notify-watch.ps1
  if ((Get-Content $cfg -Raw) -ne $after1) { throw 'watcher 不幂等' }
  Write-Output '[ok] watcher patch + idempotent'
  Set-Content -Path $cfg -Value 'notify = [ "my-custom-tool" ]' -Encoding utf8
  Remove-Item "$cfg.bak-notify-wrapper" -Force -ErrorAction SilentlyContinue
  & powershell -NoProfile -ExecutionPolicy Bypass -File src\codex-notify-watch.ps1
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
    & (Join-Path $repo 'src\codex-notify.ps1') 'turn-ended' $json
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

# 防大小写撞车回归：PS 变量不分大小写，$CARD/$card 这类同名不同写
# 会静默覆盖（曾导致悬浮窗卡片颜色失效 + 启动 try 连带跳过 ShowDialog 秒退）
$wraw = [IO.File]::ReadAllText((Join-Path $RepoRoot 'src\linkweixin-widget.ps1'))
$vars = [regex]::Matches($wraw, '\$[A-Za-z][A-Za-z0-9_]*') | ForEach-Object { $_.Value } | Sort-Object -Unique
$dupes = $vars | Group-Object { $_.ToLower() } | Where-Object { ($_.Group | Sort-Object -Unique).Count -gt 1 }
if ($dupes) { throw ("widget 变量大小写撞车：" + (($dupes | ForEach-Object { $_.Group -join '/' }) -join '; ')) }
Write-Output '[ok] widget no case-collision vars'
# 防闪屏回归：两处拉起子 powershell 必须带 -WindowStyle Hidden（读原文跨行匹配）
foreach ($f in @('plugin\notify-pushplus.ts', 'src\codex-notify.ps1')) {
  $raw = [IO.File]::ReadAllText((Join-Path $RepoRoot $f))
  if ($raw -notmatch '(?s)-WindowStyle.\s*,?\s*.Hidden') {
    throw "$f 缺 -WindowStyle Hidden，任务完成时会闪命令行窗口"
  }
  Write-Output "[ok] no-flash $f"
}
# watcher 自隐藏：已注册的旧任务动作改不动（要管理员），靠脚本启动自藏窗口
$watchRaw = [IO.File]::ReadAllText((Join-Path $RepoRoot 'src\codex-notify-watch.ps1'))
if ($watchRaw -notmatch 'GetConsoleWindow') { throw 'watcher 缺自隐藏，计划任务每 5 分钟闪窗口' }
Write-Output '[ok] no-flash src\codex-notify-watch.ps1'
# 无窗口中转：.lnk/计划任务必须经 run-hidden.vbs 拉（Win11 默认终端 WT 下
# 直接拉 powershell 必闪，-WindowStyle Hidden 都盖不住第一帧）
$vbs = Join-Path $RepoRoot 'src\run-hidden.vbs'
if (-not (Test-Path $vbs)) { throw '缺 src\run-hidden.vbs' }
$vbsRaw = [IO.File]::ReadAllText($vbs)
if ($vbsRaw -notmatch 'Run.*, 0, False') { throw 'run-hidden.vbs 必须以后台方式(0, False)拉起' }
$instRaw = [IO.File]::ReadAllText((Join-Path $RepoRoot 'install.ps1'))
if ($instRaw -notmatch 'run-hidden\.vbs') { throw 'install.ps1 快捷方式/任务必须经 run-hidden.vbs 中转' }
Write-Output '[ok] no-flash run-hidden.vbs + install wiring'
# pythonw 脱离启动器：无控制台、无 WT 页签，关不掉宿主才杀不死窗体
$py = Join-Path $RepoRoot 'src\widget-detached.py'
if (-not (Test-Path $py)) { throw '缺 src\widget-detached.py' }
$pyRaw = [IO.File]::ReadAllText($py)
if ($pyRaw -notmatch '0x08000000' -or $pyRaw -notmatch 'DEVNULL') { throw 'widget-detached.py 必须 CREATE_NO_WINDOW + 重定向标准句柄' }
Write-Output '[ok] widget-detached.py present'

# notify-toggle：临时 -MarkerPath 隔离，断言 off->on->off + 回显。不碰真实 marker。
$tmp3 = Join-Path $env:TEMP 'linkweixin-smoke-toggle'
New-Item -ItemType Directory -Force -Path $tmp3 | Out-Null
try {
  $marker = Join-Path $tmp3 'notify-pushplus.off'
  Remove-Item $marker -Force -ErrorAction SilentlyContinue
  $t1 = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -MarkerPath $marker 2>&1
  if ($LASTEXITCODE -ne 0) { throw "toggle 翻转 exit 非 0：$LASTEXITCODE" }
  if ("$t1" -notmatch 'OFF') { throw "toggle 翻转回显不对（期望 OFF）：$t1" }
  if (-not (Test-Path $marker)) { throw 'toggle 翻转未建 marker' }
  Write-Output '[ok] toggle flip -> OFF'
  $t2 = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -MarkerPath $marker 2>&1
  if ("$t2" -notmatch 'ON') { throw "toggle 翻转回显不对（期望 ON）：$t2" }
  if (Test-Path $marker) { throw 'toggle 翻转未删 marker' }
  Write-Output '[ok] toggle flip -> ON'
  $t3 = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -MarkerPath $marker -Off 2>&1
  if ("$t3" -notmatch 'OFF' -or -not (Test-Path $marker)) { throw "toggle -Off 不对：$t3" }
  $t4 = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -MarkerPath $marker -On 2>&1
  if ("$t4" -notmatch 'ON' -or (Test-Path $marker)) { throw "toggle -On 不对：$t4" }
  Write-Output '[ok] toggle -On/-Off'
  # -Agent Codex：独立 marker，翻转 + 回显（-CodexMarker 隔离，不碰真实文件）
  $cxMarker = Join-Path $tmp3 'codex-notify.off'
  Remove-Item $cxMarker -Force -ErrorAction SilentlyContinue
  $c1 = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -Agent Codex -CodexMarker $cxMarker 2>&1
  if ("$c1" -notmatch 'OFF' -or -not (Test-Path $cxMarker)) { throw "toggle codex 翻转不对：$c1" }
  $c2 = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -Agent Codex -CodexMarker $cxMarker 2>&1
  if ("$c2" -notmatch 'ON' -or (Test-Path $cxMarker)) { throw "toggle codex 翻转不对：$c2" }
  Write-Output '[ok] toggle -Agent Codex'
  # -Agent All：两边各翻各的，各回显一行
  Remove-Item $marker -Force -ErrorAction SilentlyContinue
  Remove-Item $cxMarker -Force -ErrorAction SilentlyContinue
  $al = & powershell -NoProfile -ExecutionPolicy Bypass -File src\notify-toggle.ps1 -MarkerPath $marker -CodexMarker $cxMarker -Off 2>&1
  if (("$al" -notmatch 'opencode: OFF') -or ("$al" -notmatch 'codex: OFF')) { throw "toggle All 回显不对：$al" }
  if (-not (Test-Path $marker) -or -not (Test-Path $cxMarker)) { throw 'toggle All 未建齐 marker' }
  Write-Output '[ok] toggle -Agent All'
} finally {
  Remove-Item $tmp3 -Recurse -Force -ErrorAction SilentlyContinue
}

# codex-notify + marker：marker 存在只跳过推送（桩不被调），拿掉恢复。不碰真实 marker。
$tmp4 = Join-Path $env:TEMP 'linkweixin-smoke4'
New-Item -ItemType Directory -Force -Path $tmp4 | Out-Null
try {
  $stub4 = Join-Path $tmp4 'stub.ps1'
  Set-Content -Path $stub4 -Value '"$args" | Out-File -FilePath "$env:SMOKE_GOT4" -Encoding utf8' -Encoding utf8
  $got4 = Join-Path $tmp4 'got.txt'
  $marker4 = Join-Path $tmp4 'notify-pushplus.off'
  $nolocal4 = Join-Path $tmp4 'nolocal'
  "off smoke" | Out-File -FilePath $marker4 -Encoding utf8
  $sample4 = '{"last-assistant-message":"marker test","input-messages":["marker"]}'
  $runWrapper = {
    param($repo, $stubPath, $gotFile, $fakeLocal, $json, $markerPath)
    $env:SMOKE_GOT4 = $gotFile
    $env:NOTIFY_AI_SCRIPT = $stubPath
    $env:LOCALAPPDATA = $fakeLocal
    $env:CODEX_NOTIFY_MARKER_FILE = $markerPath
    & (Join-Path $repo 'src\codex-notify.ps1') 'turn-ended' $json
  }
  $job = Start-Job -ScriptBlock $runWrapper -ArgumentList $RepoRoot, $stub4, $got4, $nolocal4, $sample4, $marker4
  $job | Wait-Job | Out-Null
  Receive-Job $job | Out-Null
  Remove-Job $job -Force -ErrorAction SilentlyContinue
  if ($job.State -ne 'Completed') { throw "codex-notify marker job 未正常结束：$($job.State)" }
  if (Test-Path $got4) { throw 'codex-notify marker 存在时仍调了推送' }
  Write-Output '[ok] codex-notify marker-off skips push'
  Remove-Item $marker4 -Force
  $job = Start-Job -ScriptBlock $runWrapper -ArgumentList $RepoRoot, $stub4, $got4, $nolocal4, $sample4, (Join-Path $tmp4 'absent.off')
  $job | Wait-Job | Out-Null
  Receive-Job $job | Out-Null
  Remove-Job $job -Force -ErrorAction SilentlyContinue
  if (-not (Test-Path $got4)) { throw 'codex-notify marker 拿掉后未恢复推送' }
  Write-Output '[ok] codex-notify marker removed resumes push'
} finally {
  Remove-Item $tmp4 -Recurse -Force -ErrorAction SilentlyContinue
}

# 沙箱安装/卸载：临时目录完整跑一遍安装与卸载，不碰真实 ~/bin、任务、快捷方式、codex 配置。
# 安装记录 + 整树拷贝是部署模型的核心机制，这里做端到端兜底（顺带防"漏拷文件"类回归）。
$tmp5 = Join-Path $env:TEMP 'linkweixin-smoke-install'
Remove-Item $tmp5 -Recurse -Force -ErrorAction SilentlyContinue
$instDir = Join-Path $tmp5 'bin'
$plugDir = Join-Path $tmp5 'plugin'
try {
  New-Item -ItemType Directory -Force -Path $instDir | Out-Null
  & powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -InstallDir $instDir -PluginDir $plugDir `
    -SkipScheduledTask -SkipCodexConfig -SkipShortcuts -SkipWidgetLaunch | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "沙箱安装 exit=$LASTEXITCODE" }
  foreach ($f in @('notify-ai.ps1', 'codex-notify.ps1', 'codex-notify-watch.ps1', 'notify-toggle.ps1', 'linkweixin-widget.ps1', 'run-hidden.vbs', 'widget-detached.py', 'lib\LinkWeixin\LinkWeixin.psd1', 'lib\LinkWeixin\Private\Send-PushPlusNotification.ps1')) {
    if (-not (Test-Path (Join-Path $instDir $f))) { throw "沙箱安装缺文件：$f" }
  }
  if (-not (Test-Path (Join-Path $plugDir 'notify-pushplus.ts'))) { throw '沙箱安装缺插件' }
  $rec = Get-Content (Join-Path $instDir 'linkweixin-install.json') -Raw -Encoding utf8 | ConvertFrom-Json
  if ([string]::IsNullOrWhiteSpace($rec.version)) { throw '沙箱安装记录缺 version' }
  if (@('vbs', 'python') -notcontains $rec.launcher) { throw "沙箱安装记录 launcher 非法：$($rec.launcher)" }
  if ([string]::IsNullOrWhiteSpace($rec.installedAt)) { throw '沙箱安装记录缺 installedAt' }
  if (@($rec.files) -notcontains 'notify-ai.ps1' -or @($rec.files) -notcontains 'widget-detached.py' -or @($rec.files) -notcontains 'lib/LinkWeixin/LinkWeixin.psd1') { throw "沙箱安装记录 files 不完整：$(@($rec.files) -join ',')" }
  Write-Output '[ok] install sandbox files + record'

  & powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1 -InstallDir $instDir -PluginDir $plugDir `
    -SkipCodexConfig -SkipShortcuts -SkipScheduledTask | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "沙箱卸载 exit=$LASTEXITCODE" }
  if (Test-Path (Join-Path $instDir 'linkweixin-install.json')) { throw '沙箱卸载残留：安装记录' }
  foreach ($f in @('notify-ai.ps1', 'linkweixin-widget.ps1', 'widget-detached.py')) {
    if (Test-Path (Join-Path $instDir $f)) { throw "沙箱卸载残留：$f" }
  }
  if (Test-Path (Join-Path $plugDir 'notify-pushplus.ts')) { throw '沙箱卸载残留：插件' }
  $left = @(Get-ChildItem $instDir -Recurse -Force -ErrorAction SilentlyContinue)
  if ($left.Count -gt 0) { throw "沙箱卸载残留：$($left.FullName -join '; ')" }
  Write-Output '[ok] uninstall sandbox clean'
} finally {
  Remove-Item $tmp5 -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output 'SMOKE ALL GREEN'
