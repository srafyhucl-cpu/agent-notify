# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与
[语义化版本](https://semver.org/lang/zh-CN/)。版本号唯一来源是
`internal/app/version.go` 的 `Version`。

## [Unreleased]

### Added

- `tools\test.ps1` 增加 `go vet ./...` 门禁，OpenCode 插件类型检查提升到 TypeScript `strict`。
- 源码安装会注入 `Version`、`Commit`、`BuildTime`，`status` 与 `doctor` 能显示真实构建信息。
- OpenCode 插件副本记录真实安装路径，自定义 `-InstallDir` 不再依赖 `%USERPROFILE%\bin`。

### Changed

- 安装时把 `$InstallDir` 中的绝对路径写进插件 `BAKED_BIN`，插件按 `BAKED_BIN`、`AGENT_NOTIFY_BIN`、默认目录、`PATH` 顺序解析运行程序。

### Fixed

- 修正自定义安装目录下 OpenCode 插件仍去找 `%USERPROFILE%\bin\agent-notify.exe` 导致任务完成不推送的问题。
- 配置与凭据保存改为直接原子替换，写入失败时不再先删掉上一份可用文件。

## [1.0.0] - 2026-09-12

### Added

- 新增 Go 单文件运行时 `agent-notify.exe`。
- 新增 ClawBot 2.4.6 二维码登录、配对码、节点跳转、凭据保存、状态查询和有限重试发送。
- 新增首条微信消息建立主动推送会话的 `sync` 命令与会话上下文持久化。
- 新增 OpenCode 全局插件，监听任务完成事件并提取最新 assistant 摘要。
- 新增 Codex notify 接入，保留 `codex-computer-use.exe` 原始参数和 stdin 透传。
- 新增原生 Win32 悬浮窗、托盘、OpenCode / Codex 开关、勿扰设置、推送历史和测试推送。
- 新增设置窗内 ClawBot 二维码登录、重新登录、退出登录和四类连接状态展示。
- 新增 DPI 感知、双缓冲绘制和可滚动的历史详情面板，统一悬浮窗与弹窗视觉语言。
- 新增 JSON Lines 推送历史，区分成功、失败、未登录、会话未建立和跳过状态。
- 新增 `doctor`、`watch`、`status --json`、`toggle` 等运维命令。
- 新增 Go 单测、OpenCode 插件类型检查、PowerShell 静态检查和隔离安装 smoke。
- 新增 GitHub Actions CI 与版本包发布流程。

### Changed

- 安装模型改为发布包中的 `bin/agent-notify.exe` 加 `plugin/agent-notify.ts`。
- 配置、凭据和会话上下文统一保存到 `%USERPROFILE%\.config\agent-notify`。
- 日志与去重状态统一保存到 `%TEMP%\agent-notify`。
- 环境变量统一使用 `AGENT_NOTIFY_*` 前缀。
- marker 统一为 `opencode.off` 和 `codex.off`。
- Codex 配置看护改由悬浮窗定时执行，只在 notify 行仍直指上游程序时恢复。
- 发布包名改为 `Agent-notify-v<版本>.zip`。
- 悬浮窗关闭与最小化只隐藏到托盘，完全退出改由托盘菜单执行。
- ClawBot 成为唯一微信推送通道；设置、登录、历史窗口不再调用旧运行时或外部脚本。
- 扫码登录与主动推送会话明确拆成两个阶段；只有登录不再被视为可发送。
- `ret/errcode=-14` 会将登录标记为失效并停止轮询，避免继续高压重试。

### Fixed

- 统一放大悬浮窗与弹窗的正文、辅助文字和图标字号，高缩放显示器上的文字不再细小难读。
- 修正 Codex 配置看护和安装器错误地把项目路径中的 `agent-notify` 当作已完成接管的问题；现在只检查实际 `notify` 行，旧 `codex-computer-use.exe` 包装链会被替换。
- 安装器会校验并重建 Windows GUI 子系统二进制，避免把 Console 构建安装后同时出现 Windows Terminal 空白窗口和悬浮窗。
- GUI 子系统程序在 PowerShell 或管道重定向时不再把 stdout 覆盖为控制台设备。
- Codex DryRun 只输出一份 JSON，不再重复打印。
- 安装和卸载 smoke 使用明确文件路径清理，避免误删沙箱外内容。
- 重新登录不会再把失效 token 作为可复用 `local_token_list`。
- 切换 ClawBot 账号时会清空旧账号的游标和会话上下文。
- 会话循环退出时会尽力发送 `notifystop`。

### Removed

- 删除所有旧运行时、包装脚本、模块加载和兼容入口。
- 删除旧品牌命名、旧配置文件、旧 marker、旧日志和旧环境变量。
- 删除旧安装记录、旧快捷方式及旧发布包命名。
- 不提供旧版本配置迁移或别名；v1.0.0 只使用本文档中的新契约。
