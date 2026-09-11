#Requires -Version 5.1
<#
  LinkWeixin 模块单元测试（Pester 5+，运行环境 PowerShell 5.1）。
  覆盖：摘要渲染、codex 事件解析、marker 语义、路径解析、模块清单。
#>

BeforeAll {
  $RepoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  Import-Module (Join-Path $RepoRoot 'src\lib\LinkWeixin\LinkWeixin.psd1') -Force
}

Describe 'Format-NotifySummary' {
  It '标题行加粗' {
    Format-NotifySummary -Text '# 标题' -Max 500 | Should -Be '<b>标题</b>'
  }
  It '无序与有序列表转圆点' {
    Format-NotifySummary -Text "- 甲`n1. 乙" -Max 500 | Should -Be '• 甲<br>• 乙'
  }
  It '引用行按历史行为处理（转义先于行内规则，> 输出为 &gt;）' {
    Format-NotifySummary -Text '> 引用' -Max 500 | Should -Be '&gt; 引用'
  }
  It '行内加粗' {
    Format-NotifySummary -Text '**加粗**' -Max 500 | Should -Be '<b>加粗</b>'
  }
  It '行内代码去反引号' {
    Format-NotifySummary -Text '`x`' -Max 500 | Should -Be 'x'
  }
  It 'HTML 转义' {
    Format-NotifySummary -Text 'a < b & c' -Max 500 | Should -Be 'a &lt; b &amp; c'
  }
  It '代码块整段剔除' {
    $text = @'
前
```secret```
后
'@
    $r = Format-NotifySummary -Text $text -Max 500
    $r | Should -Be '前<br><br>后'
    $r | Should -Not -Match 'secret'
  }
  It '按句截断：句号在 100 字之后时切在句末' {
    $text = ('a' * 110) + '。' + ('b' * 50)
    Format-NotifySummary -Text $text -Max 120 | Should -Be (('a' * 110) + '。…')
  }
  It '按句截断：无边界时硬切' {
    Format-NotifySummary -Text ('a' * 200) -Max 120 | Should -Be (('a' * 120) + '…')
  }
  It '连续空行折叠为两行' {
    Format-NotifySummary -Text "a`n`n`n`nb" -Max 500 | Should -Be 'a<br><br>b'
  }
  It '--- 分隔行剔除' {
    Format-NotifySummary -Text "a`n---`nb" -Max 500 | Should -Be 'a<br>b'
  }
  It '首尾空白裁掉' {
    Format-NotifySummary -Text "`n`na`n`n" -Max 500 | Should -Be 'a'
  }
  It '普通文本原样输出' {
    Format-NotifySummary -Text 'hello' -Max 500 | Should -Be 'hello'
  }
}

Describe 'ConvertFrom-CodexNotifyEventArgs' {
  It '标题取 input-messages[0]，摘要取 last-assistant-message' {
    $json = '{"last-assistant-message":"hello **world** smoke","input-messages":["帮我写个脚本测试一下"]}'
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @('turn-ended', $json)
    $r.Title | Should -Be '【codex】帮我写个脚本测试一下'
    $r.Summary | Should -Be 'hello **world** smoke'
  }
  It '标题超过 30 字截断加省略号' {
    $json = '{"input-messages":["' + ('字' * 40) + '"]}'
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @($json)
    $r.Title | Should -Be ('【codex】' + ('字' * 30) + '…')
  }
  It '标题压平空白' {
    $json = @{ 'input-messages' = @("a  `n b") } | ConvertTo-Json -Compress
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @($json)
    $r.Title | Should -Be '【codex】a b'
  }
  It '摘要超过 2000 字截断' {
    $json = @{ 'last-assistant-message' = ('x' * 2500) } | ConvertTo-Json -Compress
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @($json)
    $r.Summary.Length | Should -Be 2000
  }
  It '坏 JSON 参数跳过，继续解析后面的' {
    $good = '{"last-assistant-message":"ok","input-messages":["任务"]}'
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @('turn-ended', '{bad json', $good)
    $r.Title | Should -Be '【codex】任务'
    $r.Summary | Should -Be 'ok'
  }
  It '无参数时用默认标题、空摘要' {
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @()
    $r.Title | Should -Be '【codex】跑完了'
    $r.Summary | Should -Be ''
  }
  It '取到摘要即停止（优先第一个带摘要的事件）' {
    $first = '{"last-assistant-message":"第一","input-messages":["甲"]}'
    $second = '{"last-assistant-message":"第二","input-messages":["乙"]}'
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @($first, $second)
    $r.Summary | Should -Be '第一'
    $r.Title | Should -Be '【codex】甲'
  }
  It '无 input-messages 时用默认标题' {
    $r = ConvertFrom-CodexNotifyEventArgs -Arguments @('{"last-assistant-message":"hi"}')
    $r.Title | Should -Be '【codex】跑完了'
    $r.Summary | Should -Be 'hi'
  }
}

Describe 'Notify marker 读写' {
  BeforeEach {
    $script:markerDir = Join-Path $env:TEMP ('linkweixin-unit-' + [guid]::NewGuid().ToString('N'))
    $script:marker = Join-Path $script:markerDir 'notify.off'
  }
  AfterEach {
    Remove-Item $script:markerDir -Recurse -Force -ErrorAction SilentlyContinue
  }
  It 'Flip：不存在则建文件并返回 OFF（自动建父目录）' {
    $r = Set-NotifyMarker -Path $script:marker -Mode Flip
    $r | Should -Be 'OFF'
    Test-Path $script:marker | Should -BeTrue
    Test-NotifyMarker -Path $script:marker | Should -BeTrue
  }
  It 'Flip：存在则删文件并返回 ON' {
    Set-NotifyMarker -Path $script:marker -Mode Off | Out-Null
    $r = Set-NotifyMarker -Path $script:marker -Mode Flip
    $r | Should -Be 'ON'
    Test-Path $script:marker | Should -BeFalse
    Test-NotifyMarker -Path $script:marker | Should -BeFalse
  }
  It 'On：删文件（幂等）' {
    Set-NotifyMarker -Path $script:marker -Mode Off | Out-Null
    Set-NotifyMarker -Path $script:marker -Mode On | Should -Be 'ON'
    Set-NotifyMarker -Path $script:marker -Mode On | Should -Be 'ON'
    Test-Path $script:marker | Should -BeFalse
  }
  It 'Off：建文件（幂等）' {
    Set-NotifyMarker -Path $script:marker -Mode Off | Should -Be 'OFF'
    Set-NotifyMarker -Path $script:marker -Mode Off | Should -Be 'OFF'
    Test-Path $script:marker | Should -BeTrue
  }
}

Describe 'Get-LinkWeixinPaths' {
  It '默认路径符合约定' {
    $p = Get-LinkWeixinPaths
    $p.OpenCodeMarker | Should -Be (Join-Path $env:USERPROFILE '.config\opencode\notify-pushplus.off')
    $p.CodexMarker | Should -Be (Join-Path $env:USERPROFILE '.config\opencode\codex-notify.off')
    $p.AntigravityMarker | Should -Be (Join-Path $env:USERPROFILE '.config\opencode\antigravity-notify.off')
    $p.PushLog | Should -Be (Join-Path $env:TEMP 'opencode\notify-push.log')
    $p.PluginFile | Should -Be (Join-Path $env:USERPROFILE '.config\opencode\plugins\notify-pushplus.ts')
    $p.WidgetPosFile | Should -Be (Join-Path $env:TEMP 'opencode\widget-pos.txt')
    $p.WidgetExitMarker | Should -Be (Join-Path $env:TEMP 'opencode\widget-exit.txt')
  }
  It '环境变量覆盖生效' {
    $oldO = $env:OPENCODE_NOTIFY_MARKER_FILE
    $oldC = $env:CODEX_NOTIFY_MARKER_FILE
    $oldA = $env:ANTIGRAVITY_NOTIFY_MARKER_FILE
    $oldL = $env:OPENCODE_NOTIFY_LOG_FILE
    try {
      $env:OPENCODE_NOTIFY_MARKER_FILE = 'X:\oc.off'
      $env:CODEX_NOTIFY_MARKER_FILE = 'X:\cx.off'
      $env:ANTIGRAVITY_NOTIFY_MARKER_FILE = 'X:\ag.off'
      $env:OPENCODE_NOTIFY_LOG_FILE = 'X:\push.log'
      $p = Get-LinkWeixinPaths
      $p.OpenCodeMarker | Should -Be 'X:\oc.off'
      $p.CodexMarker | Should -Be 'X:\cx.off'
      $p.AntigravityMarker | Should -Be 'X:\ag.off'
      $p.PushLog | Should -Be 'X:\push.log'
    } finally {
      $env:OPENCODE_NOTIFY_MARKER_FILE = $oldO
      $env:CODEX_NOTIFY_MARKER_FILE = $oldC
      $env:ANTIGRAVITY_NOTIFY_MARKER_FILE = $oldA
      $env:OPENCODE_NOTIFY_LOG_FILE = $oldL
    }
  }
}

Describe 'Send-PushPlusNotification DryRun' {
  It '输出 payload JSON，屏蔽 token，标题加前缀' {
    $old = $env:PUSHPLUS_TOKEN
    try {
      $env:PUSHPLUS_TOKEN = 'unit-test-token'
      $json = Send-PushPlusNotification -Title '单测' -Summary '内容' -DryRun
      $obj = $json | ConvertFrom-Json
      $obj.token | Should -Be '****'
      $obj.title | Should -Be '【AI任务】单测'
      $obj.content | Should -Be '内容'
      $obj.template | Should -Be 'html'
    } finally {
      $env:PUSHPLUS_TOKEN = $old
    }
  }
  It '摘要为空时用带时间戳的默认文案' {
    $json = Send-PushPlusNotification -Title 't' -Summary '' -DryRun
    $obj = $json | ConvertFrom-Json
    $obj.content | Should -Match '任务完成，上线查看详情。\['
  }
}

Describe '模块清单' {
  It '版本与最低 PowerShell 版本正确' {
    $m = Get-Module LinkWeixin
    $m.Version.ToString() | Should -Be '0.3.0'
    $m.PowerShellVersion.ToString() | Should -Be '5.1'
  }
  It 'FunctionsToExport 里每个函数都存在' {
    $expected = @(
      'Get-LinkWeixinPaths',
      'Get-LinkWeixinConfig',
      'Set-LinkWeixinConfig',
      'Get-LinkWeixinHistory',
      'Format-NotifySummary',
      'Send-PushPlusNotification',
      'ConvertFrom-CodexNotifyEventArgs',
      'Get-CodexComputerUseExe',
      'Set-NotifyMarker',
      'Test-NotifyMarker'
    )
    foreach ($n in $expected) {
      Get-Command -Module LinkWeixin -Name $n -ErrorAction SilentlyContinue | Should -Not -BeNullOrEmpty
    }
  }
}

Describe 'Get-LinkWeixinConfig & Set-LinkWeixinConfig' {
  BeforeEach {
    $script:cfgDir = Join-Path $env:TEMP ('linkweixin-cfg-' + [guid]::NewGuid().ToString('N'))
    $script:cfgFile = Join-Path $script:cfgDir 'config.json'
  }
  AfterEach {
    Remove-Item $script:cfgDir -Recurse -Force -ErrorAction SilentlyContinue
  }
  It '默认配置结构完整' {
    $cfg = Get-LinkWeixinConfig -ConfigPath $script:cfgFile
    $cfg.channels.pushplus.enabled | Should -BeTrue
    $cfg.cooldownMin | Should -Be 10
  }
  It '写入配置并正确读取' {
    $c = @{
      channels = @{
        pushplus = @{ enabled = $false; token = 'test-token' }
        wecom    = @{ enabled = $true; webhook = 'https://qyapi.weixin.qq.com/...' }
      }
      quietHours = '22-7'
      cooldownMin = 15
    }
    Set-LinkWeixinConfig -Config $c -ConfigPath $script:cfgFile | Out-Null
    Test-Path $script:cfgFile | Should -BeTrue
    $read = Get-LinkWeixinConfig -ConfigPath $script:cfgFile
    $read.channels.pushplus.enabled | Should -BeFalse
    $read.channels.pushplus.token | Should -Be 'test-token'
    $read.channels.wecom.enabled | Should -BeTrue
    $read.quietHours | Should -Be '22-7'
    $read.cooldownMin | Should -Be 15
  }
}

Describe 'Get-LinkWeixinHistory' {
  BeforeEach {
    $script:histDir = Join-Path $env:TEMP ('linkweixin-hist-' + [guid]::NewGuid().ToString('N'))
    $script:logFile = Join-Path $script:histDir 'notify-push.log'
    New-Item -ItemType Directory -Force -Path $script:histDir | Out-Null
  }
  AfterEach {
    Remove-Item $script:histDir -Recurse -Force -ErrorAction SilentlyContinue
  }
  It '空日志或无日志文件安全返回空数组' {
    $h = Get-LinkWeixinHistory -LogPath $script:logFile
    $h.Count | Should -Be 0
  }
  It '正确解析结构化日志行' {
    $line1 = "2026-09-10T12:00:00.000Z push title=任务1 | channels=PushPlus | status=成功 | summary=摘要1`r`n"
    $line2 = "2026-09-10T12:05:00.000Z push title=任务2 | channels=企业微信 | status=成功 | summary=摘要2`r`n"
    [IO.File]::WriteAllText($script:logFile, $line1 + $line2, [System.Text.Encoding]::UTF8)
    $h = Get-LinkWeixinHistory -LogPath $script:logFile
    $h.Count | Should -Be 2
    # 倒序：第一条是任务2
    $h[0].Title | Should -Be '任务2'
    $h[0].Channels | Should -Be '企业微信'
    $h[1].Title | Should -Be '任务1'
  }
}

Describe 'Antigravity notify 逻辑' {
  BeforeEach {
    $script:agDir = Join-Path $env:TEMP ('linkweixin-ag-test-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $script:agDir | Out-Null
  }
  AfterEach {
    Remove-Item $script:agDir -Recurse -Force -ErrorAction SilentlyContinue
  }

  It '正确提取加粗 **Task**: 标题与末尾 Markdown 摘要' {
    $transcriptFile = Join-Path $script:agDir 'transcript.jsonl'
    $line1 = '{"role":"user","content":"<USER_REQUEST>\n**Task**: 升级三开关 UI 并适配 Antigravity\n</USER_REQUEST>"}'
    $line2 = '{"role":"model","parts":[{"text":"已完成阶段 1 核心功能开发，悬浮窗已升级三开关。"}]}'
    Set-Content -Path $transcriptFile -Value @($line1, $line2) -Encoding UTF8
    $escaped = ($transcriptFile -replace '\\', '\\')

    $scriptPath = Join-Path $PSScriptRoot '..\..\src\antigravity-notify.ps1'
    $payload = "{`"fullyIdle`":true,`"transcriptPath`":`"$escaped`",`"conversationId`":`"unit-conv-1`"}"
    $out = $payload | & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -DryRun
    $out | Should -Match '【Antigravity】升级三开关 UI 并适配 Antigravity'
    $out | Should -Not -Match '\*\*Task\*\*'
    $out | Should -Match '已完成阶段 1 核心功能开发'
  }

  It 'fullyIdle 为字符串 false 时必须安全跳过并返回空 JSON' {
    $scriptPath = Join-Path $PSScriptRoot '..\..\src\antigravity-notify.ps1'
    $payload = '{"fullyIdle":"false","transcriptPath":"non-existent","conversationId":"unit-conv-2"}'
    $out = $payload | & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath
    $out.Trim() | Should -Be '{}'
  }

  It '日志文件处于并发写入（FileShare.ReadWrite）状态时安全读取首行与尾部' {
    $transcriptFile = Join-Path $script:agDir 'transcript.jsonl'
    $line1 = '{"role":"user","content":"Task: 并发读取验证"}'
    $line2 = '{"role":"assistant","content":"写入测试成功"}'
    Set-Content -Path $transcriptFile -Value @($line1, $line2) -Encoding UTF8
    $escaped = ($transcriptFile -replace '\\', '\\')

    # 保持写句柄处于打开状态模拟并发
    $writer = [System.IO.File]::Open($transcriptFile, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)
    try {
      $scriptPath = Join-Path $PSScriptRoot '..\..\src\antigravity-notify.ps1'
      $payload = "{`"fullyIdle`":true,`"transcriptPath`":`"$escaped`",`"conversationId`":`"unit-conv-3`"}"
      $out = $payload | & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -DryRun
      $out | Should -Match '【Antigravity】并发读取验证'
      $out | Should -Match '写入测试成功'
    } finally {
      $writer.Dispose()
    }
  }
}
