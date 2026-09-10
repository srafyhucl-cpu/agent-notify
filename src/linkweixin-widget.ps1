#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 悬浮窗入口：单实例 + 装配 + 消息循环（界面/状态/动作在 widget\ 下）。

.DESCRIPTION
  常驻启动（控制台从不存在，Windows Terminal 也拦截不到）：
    pythonw.exe "C:\Users\你\bin\widget-detached.py"      （装了 Python 时，install 自动选）
    wscript.exe "C:\Users\你\bin\run-hidden.vbs" "C:\Users\你\bin\linkweixin-widget.ps1"
  （不要直接双击 ps1 / 用 powershell 拉：Win11 默认终端下会留黑窗口/页签。）
  install.ps1 会建 shell:startup 开机快捷方式 + 桌面快捷方式（都无需管理员）。
  无边框窗体，拖标题区移动。右上角 × / — 是最小化到托盘（首次有气泡提示），
  双击托盘图标恢复，右键托盘菜单可开关推送或彻底退出。
  再打开方式：双击托盘图标 / 桌面“linkWeixin 悬浮窗” / 上面那条手动命令。
  内容只有状态显示 + 翻 marker，不做 token/时段输入框。
  进程名已在本机实测：opencode 侧 'OpenCode*'（桌面）/'opencode*'（cli/service），
  codex 侧 'codex*'（codex-plus-plus* 是无关软件 Codex++，已排除）。轮询 5 秒一次，
  开关点击即时刷新；插件版本启动查一次、之后 10 分钟复查，心跳约 30 秒写一次，
  常驻开销只有内存（一个 hidden powershell），不阻止系统睡眠。
  上次推送时间读 notify-push.log 尾行（与插件同路径约定）。

  文件拆分（本文件 + widget\ 三个部件，同一作用域 dot-source，共享 $ctx）：
    widget\widget-form.ps1    窗体与托盘构建
    widget\widget-state.ps1   轮询状态刷新
    widget\widget-actions.ps1 窗口动作与事件接线
#>
param(
  [string]$MarkerPath
)

$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# 共享逻辑模块（marker 读写/路径/版本等都在里面）。加载失败直接可见地退出，
# 并落盘日志，不允许“静默死亡”。
try {
  Import-Module (Join-Path $PSScriptRoot 'lib\LinkWeixin\LinkWeixin.psd1') -ErrorAction Stop
} catch {
  $err = $_.Exception
  try {
    New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP 'opencode') | Out-Null
    "$(Get-Date -Format o) [module] $($err | Out-String)" | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-error.log') -Append -Encoding utf8
  } catch { }
  try { [System.Windows.Forms.MessageBox]::Show("linkWeixin 模块加载失败：$($err.Message)", 'linkWeixin') | Out-Null } catch { }
  exit 1
}

# 路径与错误日志（先于一切部件定义，供全局兜底与部件调用）。
$paths = Get-LinkWeixinPaths
if (-not $MarkerPath) { $MarkerPath = $paths.OpenCodeMarker }
$errLog = $paths.WidgetErrorLog
$aliveFile = $paths.WidgetAliveFile

function Write-WidgetError {
  param([string]$Where, [object]$Ex)
  try { "$(Get-Date -Format o) [$Where] $($Ex | Out-String)" | Out-File -FilePath $errLog -Append -Encoding utf8 } catch { }
}

# 全局兜底：UI 线程/未处理异常全部落盘。WinForms 事件里的漏网异常走这里，
# 否则就是“静默死亡、无日志”，上次丢进程就是这么查不出来的。
try {
  [System.Windows.Forms.Application]::SetUnhandledExceptionMode([System.Windows.Forms.UnhandledExceptionMode]::CatchException)
  [System.Windows.Forms.Application]::Add_ThreadException({
    param($s, $e)
    Write-WidgetError 'ui-thread' $e.Exception
  })
  [System.AppDomain]::CurrentDomain.add_UnhandledException({
    param($s, $e)
    Write-WidgetError 'fatal' $e.ExceptionObject
  })
} catch { }

# 单实例：互斥锁 + 带等待的清扫，任何时刻最多一个，且绝不出现“旧的被杀、
# 新的又退出、最后谁都不剩”的真空（之前就是这么把自己玩没的）。
# 桌面双击 = 新实例接管（开关状态全在 marker 文件里，不丢）。
# 锁必须持有到进程退出（不释放），实在抢不到且谁都没杀才安静退出。
$mtx = $null
$killedAny = $false
function Test-OldWidget {
  param([int]$MyPid, [int]$MyParent)
  try {
    return @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" -ErrorAction Stop |
      Where-Object {
        # 只认 -File 直跑本脚本的宿主：纯子串会误伤命令行里带脚本名的调用方。
        # 路径可能带引号（wscript/vbs 拉起）也可能不带（pythonw 拉起），两种都要认。
        ($_.CommandLine -match '\-File\s+"?[^"]*linkweixin-widget\.ps1') -and
        ($_.ProcessId -ne $MyPid) -and ($_.ProcessId -ne $MyParent)
      })
  } catch { return @() }
}
try {
  $myPid = $PID
  $myParent = (Get-CimInstance Win32_Process -Filter "ProcessId=$myPid" -ErrorAction SilentlyContinue).ParentProcessId
  $mtx = New-Object System.Threading.Mutex($false, 'Global\LinkWeixinWidgetSingleInstance')
  $owns = $false
  try { $owns = $mtx.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owns = $true }
  if (-not $owns) { Start-Sleep -Seconds 2 }  # 等对方稳定（或确认它是 hung 的尸体）
  for ($i = 0; $i -lt 6; $i++) {
    foreach ($p in (Test-OldWidget $myPid $myParent)) {
      try { Stop-Process -Id $p.ProcessId -Force -ErrorAction Stop; $killedAny = $true } catch { }
    }
    Start-Sleep -Seconds 2  # 等被杀的进程彻底退出、锁释放（杀锁主不等于锁立即可用）
    try { $owns = $mtx.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owns = $true }
    if ($owns -and @(Test-OldWidget $myPid $myParent).Count -eq 0) { break }
  }
  if (-not $owns -and -not $killedAny) { exit 0 }  # 对方健康活着，我安静退出
} catch { }

# 拆分部件：同一作用域 dot-source，函数与 $ctx 互通。
. (Join-Path $PSScriptRoot 'widget\widget-form.ps1')
. (Join-Path $PSScriptRoot 'widget\widget-state.ps1')
. (Join-Path $PSScriptRoot 'widget\widget-actions.ps1')

# 共享上下文（部件间只经 $ctx 读写）。
$ctx = @{
  Paths       = $paths
  MarkerPath  = $MarkerPath
  CodexMarker = $paths.CodexMarker
  AliveFile   = $aliveFile
  AllowExit   = $false
  LastOnState = $null
  Tick        = 0
  PlugVer     = $null
  TaskVer     = $null
  IconBmps    = New-Object System.Collections.ArrayList
  Drag        = @{ On = $false; X = 0; Y = 0 }
  Colors      = @{
    BG     = [System.Drawing.Color]::FromArgb(31, 31, 35)
    CardBG = [System.Drawing.Color]::FromArgb(42, 42, 47)
    FG     = [System.Drawing.Color]::FromArgb(240, 240, 240)
    DIM    = [System.Drawing.Color]::FromArgb(150, 150, 155)
    GREEN  = [System.Drawing.Color]::FromArgb(46, 160, 67)
    RED    = [System.Drawing.Color]::FromArgb(200, 60, 60)
    DotOn  = [System.Drawing.Color]::FromArgb(63, 216, 96)
  }
  Fonts       = @{
    Base = New-Object System.Drawing.Font('Microsoft YaHei', 10)
    Bold = New-Object System.Drawing.Font('Microsoft YaHei', 10, [System.Drawing.FontStyle]::Bold)
    Mid  = New-Object System.Drawing.Font('Microsoft YaHei', 11, [System.Drawing.FontStyle]::Bold)
    Hint = New-Object System.Drawing.Font('Microsoft YaHei', 8.5)
    Foot = New-Object System.Drawing.Font('Microsoft YaHei', 8)
  }
}

New-WidgetForm -Ctx $ctx
Register-WidgetEvents -Ctx $ctx

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 5000
$timer.Add_Tick({
    try {
      Update-WidgetState -Ctx $ctx
      # 心跳 ~30 秒写一次：够定位“崩/杀/退”，不值得每轮写盘。
      if (($ctx.Tick % 6) -eq 0) {
        (Get-Date -Format o) | Out-File -FilePath $ctx.AliveFile -Encoding utf8 -Force
      }
      # 托盘图标自愈：Explorer 重启/托盘区抽风会丢图标（进程活着但图标没了），
      # 每 ~5 分钟重新 Visible 一次把它顶回去，无闪烁感，有问题进日志。
      if (($ctx.Tick % 60) -eq 0) {
        try { $ctx.Notify.Visible = $false; $ctx.Notify.Visible = $true } catch { Write-WidgetError 'repulse' $_ }
      }
    } catch { Write-WidgetError 'tick' $_ }
  })
$timer.Start()
'boot ok ' + $PID + ' ' + (Get-Date -Format o) | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-boot.log') -Encoding utf8 -Force

$ctx.Form.Add_Shown({
    try { Update-WidgetState -Ctx $ctx } catch { Write-WidgetError 'shown' $_ }
    try { $ctx.Notify.ShowBalloonTip(3000, 'linkWeixin', '悬浮窗已启动。× 藏到托盘（^ 里找绿/红点，可拖出来），双击恢复；右下角红字可彻底退出。', [System.Windows.Forms.ToolTipIcon]::Info) } catch { Write-WidgetError 'tip' $_ }
  })
try {
  [void]$ctx.Form.ShowDialog()
} catch {
  Write-WidgetError 'show' $_
}
exit 0
