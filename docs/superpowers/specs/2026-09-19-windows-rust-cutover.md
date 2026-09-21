# Windows 正式入口切换到 Rust 桌面版

面向维护者。记录 `2.0.0` 把 Windows 正式交付从 Go 单文件切换到 Tauri 桌面版的边界、回退路径与回滚窗口。

- 状态：**代码与本地构建已完成；正式 Release 待打 tag**
- 记录日期：2026-09-21
- 对应计划：`docs/superpowers/plans/2026-09-19-production-loop-migration-cutover-implementation.md` Task 10 / Task 11

## 1. 切换内容

| 项 | 切换前 | 切换后 |
| --- | --- | --- |
| 安装入口 | `agent-notify.exe`（Go，Win32 自绘悬浮窗） | `agentnotify-desktop.exe`（Tauri + React）+ `agentnotify-ingress.exe` |
| 界面 | 400×570 悬浮窗（Win32） | 工作台式主窗口：总览 / Agents / Channels / History / Diagnostics / Settings |
| 状态存储 | 多个 JSONL | SQLite WAL + 事务型 Outbox |
| OpenCode 接入 | `plugin/agent-notify.ts`（V2 插件，调用 `notify` 子命令） | `plugin/rust/agent-notify.ts`（V2 插件，调用 `agentnotify-ingress.exe`） |
| 版本事实来源 | `internal/app/version.go` | 仓库根 `VERSION` |
| 安装器 AppId | `{{E7A4419F-499D-4A21-BD12-6C2D1F6B31A4}` | **不变**（升级落回原安装目录） |

切换提交：`2166623 release: 切换 Windows 正式入口到 Tauri 桌面版`（版本同步与门禁见同一提交）。

**正式 Release 尚未生成。** 需要打 tag `v2.0.0` 并推送，Release workflow 会校验 tag 与 `VERSION` 一致、强制签名后发布，并镜像到二进制仓库 `srafyhucl-cpu/agent-notify-releases`。本机 `dist\` 里的产物**未签名，仅用于本地验证，不可对外发布**。

## 2. 回退版本与校验和

回退目标固定为上一稳定 Go 版：

| 产物 | SHA256 |
| --- | --- |
| `Agent-notify-Setup-v1.17.0.exe` | `5712c124ffc5d99cd50cb5300d10349ffafd8d5a6b9b3b68f42f1c4fe10e536b` |
| `Agent-notify-v1.17.0.zip` | `50b6c35b068dbe8bb9659853e042eb26bc5b2b8f960e8880a817e0a5f9e9f807` |

来源：<https://github.com/srafyhucl-cpu/agent-notify-releases/releases/tag/v1.17.0>

回退步骤：运行上一稳定安装器，选择同一安装目录（`%LOCALAPPDATA%\Programs\Agent-notify`）。安装器保留 AppId，因此会覆盖回旧版程序；用户配置、凭据、历史与 SQLite 均不删除。

回退后旧程序只读取旧文件，不读 SQLite；SQLite 与迁移报告保留在磁盘上，供再次升级或排查使用。

## 3. 本地验证构建（未签名，仅存档）

2026-09-21 本地构建产物校验和，用于对照重跑是否一致：

```
9abe2f5e4a5268d13fb303a3b65a4cd69f49d6063fd22af1e767b6676ff92125  Agent-notify-v2.0.0.zip
aeb6fa94ed93aa44cbbbd005eb482a516790acaf80e57f4a89fbc0d528239e4d  Agent-notify-Setup-v2.0.0.exe
```

正式发布件的校验和以 Release 附件中的 `SHA256SUMS.txt` 为准。

## 4. 数据位置与迁移报告

| 内容 | 位置 |
| --- | --- |
| SQLite 状态库 | `%LOCALAPPDATA%\AgentNotify\data\state.db` |
| **迁移报告** | `%LOCALAPPDATA%\AgentNotify\data\legacy-import-report.json` |
| 运行日志 | `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` |
| 离线事件 spool | `%LOCALAPPDATA%\AgentNotify\spool` |
| 旧配置 / 凭据 / 路由 / Claim | `%USERPROFILE%\.config\agent-notify\`（**只读**，迁移不改写） |
| 旧推送历史 | `%TEMP%\agent-notify\push.log`（**只读**） |

迁移是**只读**的：升级后旧文件的 SHA256 保持不变（由 `tests\rollback-smoke.ps1` 断言，含真实源文件与隔离副本两侧）。重复启动不会重复迁移，依据是 SQLite `settings` 中的 `legacyImportV1` 标记。

## 5. 进程锁与单实例策略

- **Tauri 单实例**：同一应用标识只允许一个桌面端进程，第二次启动聚焦已有窗口。
- **命名管道按当前用户 SID 派生**：同一用户下同时只能有一个桌面实例在监听。
- **运行时锁**：`data\AgentNotify.runtime.lock` 标记运行中的核心。
- **退出路径唯一**：托盘「退出」与 Bridge `quit_app` 走同一条 `shutdown_for_quit`，先停运行时并完成 SQLite WAL checkpoint 再退出。
- **跨版本不同时运行**：新安装器 `CloseApplications=yes` 并在 `[InstallDelete]` 移除旧程序；升级时先停旧进程再替换文件。回滚时上一稳定安装器同样会停止安装目录内的旧进程。

## 6. 升级时对旧接入的处置

新安装器在 `ssPostInstall` 调用 `uninstall.ps1 -HooksOnly`，清理旧版写入的接入点（这些接入在 2.0.0 尚未实现）：

- Codex `config.toml` 的 `notify` 行（按备份定点还原或移除 AgentNotify 链）
- Antigravity `~/.gemini/config/hooks.json` 的顶层 `agent-notify` Hook
- Devin `%APPDATA%\devin\config.json` 的 `hooks.Stop` 中 AgentNotify handler
- Devin 回复扩展目录与 Command Code mod

清理失败只提示、不阻断安装。用户配置、凭据、历史与 SQLite 一律保留。

## 7. 已知问题与不支持范围

- 2.0.0 **只支持 OpenCode + ClawBot/微信**。
- Codex、Antigravity、Devin、Command Code 的通知与引用回复**尚未在 Rust 版实现**（扩展生态阶段），升级后其旧接入会被清理。
- 飞书等其它渠道、外部适配器协议未实现。
- macOS 与 HarmonyOS PC 只有 `PlatformHost` 边界，无实现、无排期。
- 不提供面向用户或 AI 的管理 CLI，也不提供 MCP；唯一命令行入口是只接收版本化 Agent 事件的 `agentnotify-ingress.exe`。
- ClawBot 下行投递曾被平台侧风控静默丢弃（接口返回成功但微信不可见），故障跟随微信用户账号；2026-09-21 17:15 已自行恢复。风控可能复发，验收记录见 `docs\superpowers\specs\2026-09-19-opencode-clawbot-acceptance.md`。

## 8. 回滚窗口

- 窗口覆盖**一个正式小版本**（即 `2.0.0` 之后的第一个小版本发布前）。
- 窗口内保留上一稳定 Go 版安装器与 ZIP，随时可回退。
- 窗口结束前收集真实使用反馈：迁移是否成功、推送是否可见、引用回复是否精确落到原会话、升级与卸载是否干净。
- 观察期内出现阻断级迁移、推送或回复问题时，停止扩展生态开发，优先修复或按第 2 节回退。

### 结束回滚窗口的条件

只有同时满足以下条件，后续版本才能删除旧 Go UI 源代码与迁移兼容层：

1. `2.0.0` 至少稳定运行一个发布周期。
2. 没有阻断级迁移、推送或回复问题。
3. 真实验收证据完整（见验收记录与 `tests\rollback-smoke.ps1` 的连续通过）。
4. `rollback-smoke.ps1` 连续通过。
5. 用户明确同意结束回滚窗口。

**本任务不删除旧源码。** 删除旧 UI 是回滚窗口结束后的独立变更，必须重新评审，并同时提供迁移兼容层的移除方案。

## 9. 回滚 smoke 执行记录

`tests\rollback-smoke.ps1`（`-From 2.0.0 -To 1.17.0`）在隔离根中执行，2026-09-21 首次通过：

| 断言组 | 结果 |
| --- | --- |
| 首次升级：SQLite、迁移报告、`legacyImportV1` 标记 | 通过（导入 374 条真实历史通知） |
| 旧文件零改动 | 通过（隔离副本与**真实源文件**两侧，迁移后与 Go 版运行后各测一轮） |
| 回滚后旧版可用 | 通过（Go v1.17.0 `status` exit=0 且有输出；SQLite 与报告仍在） |
| 再次升级不重复导入 | 通过（通知 374→374、路由 250→250、Claim 0→0） |
| 单实例 | 通过（同一时刻仅 1 个桌面端进程） |
| 防串号护栏 | 通过（存在 Claim 但缺少 ClawBot 凭据时 `legacy_import_failed`，不写入任何通知） |

该脚本的输入刻意不含 `clawbot.json`（凭据）与 `reply-state.jsonl`（Claim）：隔离实例若拿到真实凭据会轮询平台，且迁移会把凭据写入共享的 Windows 凭据管理器（账号 ID 与真实账号相同）而覆盖真实登录。凭据导入路径由真实验收覆盖（迁移导入 2 个账号与 52 条 Claim），后者由上面的护栏用例反向保障。

日期：2026-09-21
