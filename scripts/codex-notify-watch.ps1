#Requires -Version 5.1
<#
.SYNOPSIS
  看住 codex 的 notify 配置：codex 桌面启动/更新时会把 config.toml 的
  notify 改回直调 codex-computer-use.exe，本脚本把它改回 wrapper。

.DESCRIPTION
  只在当前 notify 行指向 codex-computer-use.exe 且没经过 wrapper 时才改写，
  其他自定义配置一律不动。改写前备份。设计为计划任务每 5 分钟跑一次，
  也可手动跑。codex 侧如在改写前已启动，需重启 codex 桌面才生效。
#>
param(
  [string]$ConfigPath = $(if ($env:CODEX_CONFIG) { $env:CODEX_CONFIG } else { Join-Path $env:USERPROFILE '.codex\config.toml' }),
  [string]$WrapperPath = $(if ($env:CODEX_NOTIFY_WRAPPER) { $env:CODEX_NOTIFY_WRAPPER } else { Join-Path $PSScriptRoot 'codex-notify.ps1' })
)

$ErrorActionPreference = 'SilentlyContinue'

# 防闪屏：本脚本被计划任务每 5 分钟拉起，旧注册动作不带 -WindowStyle Hidden
#（改任务要管理员权限），所以启动第一时间自己藏窗口。只在父进程是任务引擎
#（svchost/taskeng）时藏，手动跑不影响自己的终端。新注册见 install.ps1（已带 Hidden）。
try {
  $ppid = (Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction Stop).ParentProcessId
  $pname = (Get-CimInstance Win32_Process -Filter "ProcessId=$ppid" -ErrorAction Stop).Name
  if ($pname -match '^(svchost|taskeng)(\.exe)?$') {
    Add-Type -Name WinHide -Namespace LinkWeixin -MemberDefinition '[DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow(); [DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr hWnd, int nCmdShow);' -ErrorAction Stop
    [LinkWeixin.WinHide]::ShowWindow([LinkWeixin.WinHide]::GetConsoleWindow(), 0) | Out-Null
  }
} catch { }
try { New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP 'opencode') | Out-Null } catch { }
$cfg = $ConfigPath
$wrapperSlash = ($WrapperPath -replace '\\', '/')
$want = "notify = [ `"powershell.exe`", `"-NoProfile`", `"-ExecutionPolicy`", `"Bypass`", `"-File`", `"$wrapperSlash`", `"turn-ended`" ]"
try {
  $t = [IO.File]::ReadAllText($cfg)
  if ($t -match 'codex-notify\.ps1') { exit 0 }
  if ($t -notmatch '(?m)^notify\s*=.*codex-computer-use\.exe') { exit 0 }
  Copy-Item $cfg "$cfg.bak-notify-wrapper" -Force
  $t2 = [regex]::Replace($t, '(?m)^notify\s*=.*$', $want)
  if ($t2 -ne $t) {
    [IO.File]::WriteAllText($cfg, $t2)
    "repatched $(Get-Date -Format o)" | Out-File -FilePath "$env:TEMP\opencode\codex-watch.log" -Append -Encoding utf8
  }
} catch { }
exit 0
