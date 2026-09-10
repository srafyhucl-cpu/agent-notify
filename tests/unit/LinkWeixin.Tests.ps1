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
    $p.PushLog | Should -Be (Join-Path $env:TEMP 'opencode\notify-push.log')
    $p.PluginFile | Should -Be (Join-Path $env:USERPROFILE '.config\opencode\plugins\notify-pushplus.ts')
    $p.WidgetPosFile | Should -Be (Join-Path $env:TEMP 'opencode\widget-pos.txt')
    $p.WidgetExitMarker | Should -Be (Join-Path $env:TEMP 'opencode\widget-exit.txt')
  }
  It '环境变量覆盖生效' {
    $oldO = $env:OPENCODE_NOTIFY_MARKER_FILE
    $oldC = $env:CODEX_NOTIFY_MARKER_FILE
    $oldL = $env:OPENCODE_NOTIFY_LOG_FILE
    try {
      $env:OPENCODE_NOTIFY_MARKER_FILE = 'X:\oc.off'
      $env:CODEX_NOTIFY_MARKER_FILE = 'X:\cx.off'
      $env:OPENCODE_NOTIFY_LOG_FILE = 'X:\push.log'
      $p = Get-LinkWeixinPaths
      $p.OpenCodeMarker | Should -Be 'X:\oc.off'
      $p.CodexMarker | Should -Be 'X:\cx.off'
      $p.PushLog | Should -Be 'X:\push.log'
    } finally {
      $env:OPENCODE_NOTIFY_MARKER_FILE = $oldO
      $env:CODEX_NOTIFY_MARKER_FILE = $oldC
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
    $m.Version.ToString() | Should -Be '0.2.0'
    $m.PowerShellVersion.ToString() | Should -Be '5.1'
  }
  It 'FunctionsToExport 里每个函数都存在' {
    $expected = @(
      'Get-LinkWeixinPaths',
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
