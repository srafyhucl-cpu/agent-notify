#Requires -Version 5.1
<#
.SYNOPSIS
  linkWeixin 悬浮窗入口：单实例 + 装配 + 消息循环（界面/状态/动作/历史/设置在 widget\ 下）。
#>
param(
  [string]$MarkerPath
)

$ErrorActionPreference = 'SilentlyContinue'

# DPI 感知与任务栏窗口样式（Per-Monitor V2 + Taskbar Minimize）
try {
  Add-Type -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(System.IntPtr value);
[DllImport("user32.dll", EntryPoint = "GetWindowLong")] public static extern int GetWindowLong(System.IntPtr hWnd, int nIndex);
[DllImport("user32.dll", EntryPoint = "SetWindowLong")] public static extern int SetWindowLong(System.IntPtr hWnd, int nIndex, int dwNewLong);
'@ -Name DpiAwareness -Namespace LinkWeixin -ErrorAction Stop
  [void][LinkWeixin.DpiAwareness]::SetProcessDpiAwarenessContext([IntPtr](-4))
} catch { }

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

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

$paths = Get-LinkWeixinPaths
if (-not $MarkerPath) { $MarkerPath = $paths.OpenCodeMarker }
$errLog = $paths.WidgetErrorLog
$aliveFile = $paths.WidgetAliveFile
$exitMarker = $paths.WidgetExitMarker
$posFile = $paths.WidgetPosFile

if (Test-Path $exitMarker) { Remove-Item $exitMarker -Force -ErrorAction SilentlyContinue }

$appVersion = ''
try { $appVersion = (Get-Module LinkWeixin).Version.ToString() } catch { }

function Write-WidgetError {
  param([string]$Where, [object]$Ex)
  try { "$(Get-Date -Format o) [$Where] $($Ex | Out-String)" | Out-File -FilePath $errLog -Append -Encoding utf8 } catch { }
}

[System.AppDomain]::CurrentDomain.add_ProcessExit({
  try { "exit pid=$PID at=$(Get-Date -Format o)" | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-exit.log') -Append -Encoding utf8 } catch { }
})

try {
  [System.Windows.Forms.Application]::SetUnhandledExceptionMode([System.Windows.Forms.UnhandledExceptionMode]::CatchException)
  [System.Windows.Forms.Application]::Add_ThreadException({
    param($s, $e)
    $null = $s
    Write-WidgetError 'ui-thread' $e.Exception
  })
  [System.AppDomain]::CurrentDomain.add_UnhandledException({
    param($s, $e)
    $null = $s
    Write-WidgetError 'fatal' $e.ExceptionObject
  })
} catch { }

# 单实例互斥与旧进程清理（极速冷启动：快速通道 0ms 放行，仅冲突时轻量恢复）
$mtx = $null
$killedAny = $false
function Test-OldWidget {
  param([int]$myPid, [int]$myParent)
  try {
    return @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" -ErrorAction Stop |
      Where-Object {
        ($_.CommandLine -match '\-File\s+"?[^"]*linkweixin-widget\.ps1') -and
        ($_.ProcessId -ne $myPid) -and ($_.ProcessId -ne $myParent)
      })
  } catch { return @() }
}
try {
  $mtx = New-Object System.Threading.Mutex($false, 'Global\LinkWeixinWidgetSingleInstance')
  $owns = $false
  try { $owns = $mtx.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owns = $true }
  if (-not $owns) {
    $myPid = $PID
    $myParent = 0
    try { $myParent = (Get-CimInstance Win32_Process -Filter "ProcessId=$myPid" -ErrorAction SilentlyContinue).ParentProcessId } catch { }
    for ($i = 0; $i -lt 8; $i++) {
      foreach ($p in (Test-OldWidget $myPid $myParent)) {
        try { Stop-Process -Id $p.ProcessId -Force -ErrorAction Stop; $killedAny = $true } catch { }
      }
      Start-Sleep -Milliseconds 100
      try { $owns = $mtx.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $owns = $true }
      if ($owns) { break }
    }
    if (-not $owns -and -not $killedAny) { exit 0 }
  }
} catch { }

# 拆分部件加载
. (Join-Path $PSScriptRoot 'widget\widget-history.ps1')
. (Join-Path $PSScriptRoot 'widget\widget-settings.ps1')
. (Join-Path $PSScriptRoot 'widget\widget-form.ps1')
. (Join-Path $PSScriptRoot 'widget\widget-state.ps1')
. (Join-Path $PSScriptRoot 'widget\widget-actions.ps1')

$ctx = @{
  Paths             = $paths
  ScriptDir         = $PSScriptRoot
  MarkerPath        = $MarkerPath
  CodexMarker       = $paths.CodexMarker
  AntigravityMarker = $paths.AntigravityMarker
  AliveFile         = $aliveFile
  PosFile           = $posFile
  ExitMarker        = $exitMarker
  AppVersion        = $appVersion
  AllowExit         = $false
  LastOnState       = $null
  PlugVer           = $null
  TaskVer           = '新版'
  TodayCount        = $null
  HoverOc           = $false
  HoverCx           = $false
  HoverAg           = $false
  Tick              = 0
  IconBmps          = New-Object System.Collections.ArrayList
  Drag        = @{ On = $false; X = 0; Y = 0 }
  Colors      = @{
    BG     = [System.Drawing.Color]::FromArgb(24, 24, 27)
    CardBG = [System.Drawing.Color]::FromArgb(39, 39, 44)
    FG     = [System.Drawing.Color]::FromArgb(244, 244, 245)
    DIM    = [System.Drawing.Color]::FromArgb(161, 161, 170)
    GREEN  = [System.Drawing.Color]::FromArgb(16, 185, 129)
    RED    = [System.Drawing.Color]::FromArgb(220, 53, 69)
    DotOn  = [System.Drawing.Color]::FromArgb(52, 211, 153)
  }
  Fonts       = @{
    Base = New-Object System.Drawing.Font('Microsoft YaHei', 9.5)
    Bold = New-Object System.Drawing.Font('Microsoft YaHei', 9.5, [System.Drawing.FontStyle]::Bold)
    Mid  = New-Object System.Drawing.Font('Microsoft YaHei', 10, [System.Drawing.FontStyle]::Bold)
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
      if (($ctx.Tick % 6) -eq 0) {
        (Get-Date -Format o) | Out-File -FilePath $ctx.AliveFile -Encoding utf8 -Force
      }
      if (($ctx.Tick % 60) -eq 0) {
        try { $ctx.Notify.Visible = $false; $ctx.Notify.Visible = $true } catch { Write-WidgetError 'repulse' $_ }
      }
    } catch { Write-WidgetError 'tick' $_ }
  })
$timer.Start()
'boot ok ' + $PID + ' ' + (Get-Date -Format o) | Out-File -FilePath (Join-Path $env:TEMP 'opencode\widget-boot.log') -Encoding utf8 -Force

$ctx.Form.Add_Shown({
    try { $ctx.Form.Opacity = 1 } catch { }
    try { Update-WidgetState -Ctx $ctx -Light } catch { Write-WidgetError 'shown' $_ }
  })

try {
  $appContext = New-Object System.Windows.Forms.ApplicationContext
  $ctx.AppContext = $appContext
  $ctx.Form.Show()
  [System.Windows.Forms.Application]::Run($appContext)
} catch {
  Write-WidgetError 'run' $_
}
exit 0
