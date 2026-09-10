#Requires -Version 5.1
<#
.SYNOPSIS
  AI 任务完成推送（PushPlus -> 微信），opencode / codex 共用入口。

.DESCRIPTION
  渲染、发送与默认文案都在 LinkWeixin 模块（lib\LinkWeixin）里实现；
  本入口只做参数绑定、stdin 兜底与调用。摘要优先级：
  1. -Summary 参数；2. 管道 stdin；3. 模块内默认文案（带时间戳）。
  密钥只从环境变量 PUSHPLUS_TOKEN 读，不落盘。任何失败都静默，
  永远 exit 0，不卡住 agent。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-ai.ps1 -Title "【AI任务】重构完成" -Summary "改了 3 个文件，测试通过"
  echo "小结文本" | powershell -NoProfile -ExecutionPolicy Bypass -File notify-ai.ps1 -Title "【AI任务】跑完了"
  powershell -NoProfile -ExecutionPolicy Bypass -File notify-ai.ps1 -DryRun -Summary "只打印 payload，不真推"
#>
[CmdletBinding()]
param(
  [string]$Title = "【AI任务】跑完了",
  [string]$Summary = "",
  [int]$MaxChars = 500,
  [switch]$DryRun,
  [switch]$NoStdin
)

$ErrorActionPreference = 'SilentlyContinue'

try {
  Import-Module (Join-Path $PSScriptRoot 'lib\LinkWeixin\LinkWeixin.psd1') -ErrorAction Stop
} catch {
  [Console]::Error.WriteLine('[notify-ai] LinkWeixin 模块加载失败：' + $_.Exception.Message)
  exit 0
}

# 摘要为空且允许读管道时，从 stdin 读原文（插件侧还会传 -NoStdin 双保险）。
if ([string]::IsNullOrWhiteSpace($Summary) -and -not $NoStdin -and [Console]::IsInputRedirected) {
  $Summary = [Console]::In.ReadToEnd()
}

$json = Send-PushPlusNotification -Title $Title -Summary $Summary -MaxChars $MaxChars -DryRun:$DryRun
if ($DryRun -and $json) { $json }
exit 0
