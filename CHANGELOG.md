# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与
[语义化版本](https://semver.org/lang/zh-CN/)。版本号唯一来源是
`src/lib/LinkWeixin/LinkWeixin.psd1` 的 `ModuleVersion`。

## [Unreleased]

### Fixed

- opencode 插件目录修正为 V2 约定 `~/.config/opencode/plugins/`（复数）。
  安装器 / 卸载器 / 悬浮窗插件检查此前误用 V1 单数 `plugin\`，导致悬浮窗误报
  "插件未安装"、并可能向错误目录反复写入；现改为复数路径，装/卸时自动清理旧目录残留

## [0.1.0] - 2026-09-10

第一个正式版本。工程化改造完成；对外契约（安装位置、入口文件名、参数、环境变量、
marker 路径、推送行为）与历史版本保持一致，老用户重装即平滑升级。

### Added

- `LinkWeixin` PowerShell 模块（`src/lib/LinkWeixin/`）：路径解析、摘要渲染、
  PushPlus 发送、codex 事件解析、codex-computer-use 定位、marker 读写；
  入口脚本瘦身为薄封装
- 测试体系：Pester 单测（33 项）、仓库 BOM/CRLF 硬门禁、冒烟沙箱安装/卸载用例；
  统一入口 `tools/test.ps1`
- 工程门禁：PSScriptAnalyzer 配置（含 PS 5.1 兼容检查）与 `tools/lint.ps1`、
  opencode 插件 TypeScript 类型检查（`tsc --noEmit`）
- 安装记录 `linkweixin-install.json`：整树安装、按记录精准卸载、自动清理旧版本残留文件
- 悬浮窗 `pythonw` 启动链（GUI 子系统，无控制台、无 Windows Terminal 页签防误杀）；
  系统没有 Python 时自动回退 `run-hidden.vbs`
- 悬浮窗底栏显示版本号；安装记录记录实际启动方式（python / vbs）
- 治理与文档：CONTRIBUTING / SECURITY / Issue 与 PR 模板 / ARCHITECTURE / TROUBLESHOOTING

### Changed

- 目录结构：`scripts/` → `src/`，`opencode-plugin/` → `plugin/`（安装到 `~/bin` 的路径不变）
- 悬浮窗由单文件 484 行拆分为入口 + `widget/` 三个部件（同一作用域协作）
- PushPlus 请求超时 10 秒 → 20 秒（慢代理链路实测单次 9~12 秒，10 秒会偶发失败）
- 插件关闭子进程 stdin 改为显式 `child.stdin.end()`（原 `execFile` 的 `input` 选项
  无效，此前仅靠脚本侧 `-NoStdin` 兜底）

### Fixed

- `widget-detached.py` 接线补全：纳入安装/卸载清单与快捷方式构建，不再是孤儿文件
- 悬浮窗单实例接管兼容 `pythonw` 启动（命令行路径不带引号的形态）

### 已知行为（非缺陷）

- `Format-NotifySummary` 的 `>` 引用行分支为历史死代码（HTML 转义先于行内规则），
  实际输出 `&gt; …`；0.1.0 按行为等价保留，单测已固化现状
