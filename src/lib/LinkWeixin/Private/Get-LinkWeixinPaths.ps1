function Get-LinkWeixinPaths {
  <#
  .SYNOPSIS
    集中解析运行期路径（marker / 临时目录 / 日志 / 插件），环境变量在调用时读取。

  .DESCRIPTION
    与插件侧（plugins/notify-pushplus.ts）保持同一路径约定：
    - OpenCodeMarker：OPENCODE_NOTIFY_MARKER_FILE 覆盖，默认 ~\.config\opencode\notify-pushplus.off
    - CodexMarker：CODEX_NOTIFY_MARKER_FILE 覆盖，默认 ~\.config\opencode\codex-notify.off
    - PushLog：OPENCODE_NOTIFY_LOG_FILE 覆盖，默认 %TEMP%\opencode\notify-push.log
    - TempDir / PluginFile / WidgetErrorLog / WidgetAliveFile：默认路径
  #>
  $userProfile = $env:USERPROFILE
  $configDir = Join-Path $userProfile '.config\opencode'
  $tempDir = Join-Path $env:TEMP 'opencode'
  return @{
    ConfigDir       = $configDir
    TempDir         = $tempDir
    OpenCodeMarker  = if ($env:OPENCODE_NOTIFY_MARKER_FILE) { $env:OPENCODE_NOTIFY_MARKER_FILE } else { Join-Path $configDir 'notify-pushplus.off' }
    CodexMarker     = if ($env:CODEX_NOTIFY_MARKER_FILE) { $env:CODEX_NOTIFY_MARKER_FILE } else { Join-Path $configDir 'codex-notify.off' }
    PushLog         = if ($env:OPENCODE_NOTIFY_LOG_FILE) { $env:OPENCODE_NOTIFY_LOG_FILE } else { Join-Path $tempDir 'notify-push.log' }
    PluginFile      = Join-Path $configDir 'plugins\notify-pushplus.ts'
    WidgetErrorLog  = Join-Path $tempDir 'widget-error.log'
    WidgetAliveFile = Join-Path $tempDir 'widget-alive.txt'
  }
}
