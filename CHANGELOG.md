# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与
[语义化版本](https://semver.org/lang/zh-CN/)。版本号唯一来源是
`internal/app/version.go` 的 `Version`。

## [1.0.0] - 2026-09-12

### Added

- 新增 Go 单文件运行时 `agent-notify.exe`。
- 新增 ClawBot 扫码登录、凭据保存、状态查询和有限重试发送。
- 新增 OpenCode 全局插件，监听任务完成事件并提取最新 assistant 摘要。
- 新增 Codex notify 接入，保留 `codex-computer-use.exe` 原始参数和 stdin 透传。
- 新增原生 Win32 悬浮窗、托盘、OpenCode / Codex 开关、勿扰设置、推送历史和测试推送。
- 新增 JSON Lines 推送历史，记录成功、失败、未登录和跳过状态。
- 新增 `doctor`、`watch`、`status --json`、`toggle` 等运维命令。
- 新增 Go 单测、OpenCode 插件类型检查、PowerShell 静态检查和隔离安装 smoke。
- 新增 GitHub Actions CI 与版本包发布流程。

### Changed

- 安装模型改为发布包中的 `bin/agent-notify.exe` 加 `plugin/agent-notify.ts`。
- 配置与凭据统一保存到 `%USERPROFILE%\.config\agent-notify`。
- 日志与去重状态统一保存到 `%TEMP%\agent-notify`。
- 环境变量统一使用 `AGENT_NOTIFY_*` 前缀。
- marker 统一为 `opencode.off` 和 `codex.off`。
- Codex 配置看护改由悬浮窗定时执行，只在 notify 行仍直指上游程序时恢复。
- 发布包名改为 `Agent-notify-v<版本>.zip`。

### Fixed

- GUI 子系统程序在 PowerShell 或管道重定向时不再把 stdout 覆盖为控制台设备。
- Codex DryRun 只输出一份 JSON，不再重复打印。
- 安装和卸载 smoke 使用明确文件路径清理，避免误删沙箱外内容。

### Removed

- 删除所有旧运行时、包装脚本、模块加载和兼容入口。
- 删除旧品牌命名、旧配置文件、旧 marker、旧日志和旧环境变量。
- 删除旧安装记录、旧快捷方式及旧发布包命名。
- 不提供旧版本配置迁移或别名；v1.0.0 只使用本文档中的新契约。
