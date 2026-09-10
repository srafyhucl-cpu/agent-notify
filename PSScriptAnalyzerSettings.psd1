# PSScriptAnalyzer 配置（linkWeixin）
# 默认规则全开 + PS 5.1 语法兼容检查；排除项均附理由，改动需说明原因。
@{
  IncludeDefaultRules = $true

  ExcludeRules = @(
    # 静默失败是核心契约：推送链路任何失败都不得打扰 agent（成功也 exit 0）。
    # 全项目空 catch 均为有意为之，逐个加 Suppress 只会产生噪音。
    'PSAvoidUsingEmptyCatchBlock'

    # 非交互式工具脚本（后台推送/悬浮窗/安装器），不存在 WhatIf/Confirm 语义。
    'PSUseShouldProcessForStateChangingFunctions'

    # 领域命名且是模块公开 API（Get-LinkWeixinPaths / Register-WidgetEvents /
    # ConvertFrom-CodexNotifyEventArgs）；改名破坏兼容，集合语义本身也更准确。
    'PSUseSingularNouns'

    # 冒烟测试用 Start-Job -ScriptBlock { param(...) } -ArgumentList 显式传参，
    # 这正是推荐写法；规则不识别 param 块，属误报。
    'PSUseUsingScopeModifierInNewRunspaces'
  )

  Rules = @{
    # 全项目必须兼容 Windows PowerShell 5.1（最低支持运行时）。
    PSUseCompatibleSyntax = @{
      Enable         = $true
      TargetVersions = @('5.1')
    }
  }
}
