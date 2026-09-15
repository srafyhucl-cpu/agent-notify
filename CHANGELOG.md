# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与
[语义化版本](https://semver.org/lang/zh-CN/)。版本号唯一来源是
`internal/app/version.go` 的 `Version`。

## [Unreleased]

## [1.4.0] - 2026-09-15

### Added

- 悬浮窗新增“升级”按钮：自动检查官方 GitHub Release、下载 `Agent-notify-v<版本>.zip`、校验 `SHA256SUMS.txt`，随后调用新版本安装器更新程序并重启悬浮窗。
- 新增只分发编译产物的公开 Release 仓库，源码仓库保持私有；桌面端默认从公开仓库检查更新。

## [1.3.0] - 2026-09-15

### Added

- 新增 `integration-status`，并让安装器、`status`、`doctor` 与悬浮窗共用同一套 Agent 接入检测；悬浮窗提供“检查接入”入口，可安全恢复被改回的 Codex notify，但不会自动重启任何 Agent。
- 新增首次安装无凭据时自动打开微信扫码，并在安装结束时逐项显示 OpenCode、Codex、Antigravity、Devin 的真实接入状态。

### Fixed

- 修复悬浮窗沿用越界坐标导致右侧被裁切的问题；启动和 DPI 变化时均按目标显示器工作区重新校正窗口边界。
- 修复悬浮窗把“开关开启”误显示为“监听中”的问题；现在区分未接入、待重启、接入异常和已接入，OpenCode / Devin 使用真实加载心跳，Codex 与 Antigravity 校验有效配置和目标程序。
- 修复安装器被 Codex 嵌套 `--previous-notify` 误导而跳过接入的问题，并确保安装结束在 Windows PowerShell 5.1 下正确显示四个 Agent 的真实状态。

## [1.2.0] - 2026-09-15

### Added

- 新增 Antigravity 全局 `Stop` Hook：仅在 `fullyIdle=true` 且存在 `conversationId` 时发送通知，并从 transcript 尾部提取摘要；Hook 始终返回 `{}`，通知失败不会阻塞 Antigravity。
- 新增 Devin 用户级 `Stop` Hook：使用 `session_id` 和 `last_assistant_message`，跳过 `stop_hook_active=true` 的重入事件；Hook 失败不会改变 Devin 的停止决策。
- 新增 Antigravity / Devin 微信引用回复：Antigravity 使用桌面端官方 `language_server.exe agentapi`；Devin 使用随 Agent-notify 安装的桌面扩展，直接向桌面端 `devin.exe acp` 子进程写入 `session/prompt` 续写原会话；两者都不依赖对应 CLI 登录。
- `status`、`doctor`、`toggle` 和悬浮窗统一支持四个 Agent；新增 `antigravity.off` / `devin.off` marker 与对应环境变量覆盖。
- 安装和卸载脚本新增共享 `tools/hook-config.ps1`，以原子替换方式只维护 Agent-notify 自己的 Hook，保留其他 JSON 配置和 handler。

### Changed

- Agent 标识、标题、页脚、推送开关和可回复判定统一收敛到 `internal/agentmeta`，避免四套重复定义。
- OpenCode 与 Devin 复用通用本地 spool 队列，共享原子写入、心跳校验、同步结果等待、异步失败观察和禁止自动重放语义。
- Devin 回复不再启动第二个 Agent 进程：ACP 会话复用桌面端自己拉起的 ACP 子进程，因此不受会话锁、工作区信任和 CLI 登录状态影响。
- Devin 回复扩展安装与卸载纳入正式安装器、发布包和冒烟测试；卸载只删除归属校验通过的扩展文件。
- Antigravity / Devin 安装器只在已有配置或父目录存在时写入 Hook，不会为未安装的客户端创建配置目录。

### Fixed

- Antigravity Hook 改为调用同目录无空格启动器，避免 Windows `cmd /c` 对 exe 外层引号的转义导致 Stop Hook 实际未执行。
- Antigravity 推送优先读取真实会话标题；首次通知时标题文件尚未生成，则从 transcript 首条用户请求生成可识别标题，不再退回“跑完了”。

- Devin 引用回复改为直接写桌面端常驻的 `devin.exe acp` 子进程 stdin（NDJSON `session/prompt`），不再经聊天面板提交，修掉引用回复新开对话、落入 Ask 模式以及误报 CLI 未登录的问题；目标通道缺失、存在多个候选或写入失败都会返回微信可读错误，不向 CLI 或新会话回退。
- Devin 引用回复改用桌面端内部 Cascade 标识：Go 侧按 `session_id` 从 `%APPDATA%\devin\User\globalStorage\state.vscdb` 精确解析后随作业下发，修掉直接把 CLI 会话号当作桌面端标识导致的“目标 Devin 会话不存在”。
- Antigravity 回复只使用当前语言服务实际监听的 HTTP 端口和本次启动的 CSRF token，并在发送前验证目标会话存在。
- release 校验新增 `antigravity.off` / `devin.off`，并对 `tools/hook-config.ps1` 进行打包检查。

## [1.1.0] - 2026-09-13

### Added

- 新增微信引用通知回复：按平台消息 ID 精确关联已记录通知，Codex 通过 `codex queue` 向对应线程续聊，首版默认关闭。
- Codex 冷持久化线程由本地持久队列承接，恢复同一线程后执行；归档、已删除或不存在的线程返回可见错误，不静默切换会话。
- Codex CLI 在 PATH 不可用的开机自启场景下会继续从 `%LOCALAPPDATA%\OpenAI\Codex\bin` 自动发现最新安装，避免引用回复因环境变量差异失效。
- 新增引用路由、入站去重、账号隔离、30 天过期和可见失败提示。
- 路由与去重 JSONL 达到阈值后原子压缩，回收已过期或损坏记录，避免文件无限增长。
- 新增 OpenCode 本地回复收件箱、插件心跳和 `session.prompt` 投递路径，兼容旧版 `promptAsync`，等待第二阶段真实验收。
- 新增 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 脱敏协议诊断，以及 `doctor` 的 `codex queue` 检查。
- 新增只读命令 `agent-notify reply-check`：自动核对引用 ID 与发送记录、本地路由的对应关系，作为开启“引用回复”前的 P0 闸门（退出码 `0` 通过、`1` 失败、`2` 证据不足）。
- P0 发送证据合并 scoped `sendmessage-result` 与当前账号未过期的本地路由，兼容调试开关未覆盖发送进程的情况，同时继续按账号和过期时间隔离。
- P0 诊断区分普通消息与引用了消息但未返回 ID 的异常样本；引用结构存在、路由缺失、过期或无法读取时一律不通过。
- P0 诊断写入 `account_scope`、`private` 与 `bound_sender`；`reply-check` 只采信当前登录账号绑定用户的私聊样本，跨账号、群聊和升级前旧日志被忽略并计数。
- 核对并记录 OpenCode 桌面端插件投递契约：`session.prompt({ sessionID, text, delivery })` 在入力被持久接纳后返回，不等待整轮任务完成。
- Codex 通知使用只读 SQLite 解析真实会话名，按状态库字段、会话索引和 payload 逐级降级；标题失败时在原通知中显示提示，且不影响引用路由。
- 新增 `codex-title.log` 诊断，记录标题来源、失败阶段、SQLite 错误码、重试次数和降级来源。
- 通知新增协议块解析、本地时间页脚和默认不限长策略；正常心跳静默，异常或未知心跳不会被漏报。

### Changed

- `codex queue` 超时或调用取消按“投递未确认”处理，不自动重试，避免重复发送。
- ClawBot 发送接口返回平台消息 ID 和客户端 ID；历史记录增加可选的 `messageID`、`clientID`。
- Codex notify 提取并保存 `thread-id`（兼容 `thread_id`）；缺失线程 ID 时只发送普通通知。
- 设置界面增加“引用回复”开关，配置文件增加 `replyEnabled`。
- Codex 与 OpenCode 标题统一为 `【codex】会话名` / `【opencode】会话标题`；显式 `--max-chars` 现在按渲染后的完整消息计算。

## [1.0.3] - 2026-09-12

### Changed

- 移除源码安装与构建脚本中的开发机专用 Go 路径；`go.exe` 现在仅从 `AGENT_NOTIFY_GO` 或系统 `PATH` 查找。

### Added

- 增加任意用户目录默认路径回归测试，并确认发布包中的插件模板不携带任何人的绝对安装路径。

## [1.0.2] - 2026-09-12

### Fixed

- 推送时不再拉起可见的控制台窗口：Codex 通知钩子转发上游程序时显式使用 `CREATE_NO_WINDOW` 并隐藏子进程窗口。
- ClawBot 返回 `ret=-2 prepare failed` 时识别为主动推送会话失效，清除本地过期上下文并在状态中重新提示发送微信消息，避免继续误报“会话就绪”。


## [1.0.1] - 2026-09-12

### Changed

- 悬浮窗、设置、历史、登录窗口字号整体放大，并集中到 `internal/ui/ui_fonts.go` 统一按 DPI 缩放。
- 设置窗口的勿扰时段输入框新增格式占位提示。

### Fixed

- “最近推送”卡片的时间与标题不再被截断，标题区域扩展到整卡宽度。
- 悬浮窗改为托盘型工具窗，不再额外占用任务栏按钮，桌面端启动后不会再出现“两个图标”。
- 新增文本宽度回归测试，覆盖 96/144/192 DPI 下关键文案不溢出。

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
- 新增 `history --json` 机器可读输出。
- 新增 `go vet ./...` 门禁，插件类型检查提升到 TypeScript `strict`。
- 源码安装注入 `Version`、`Commit`、`BuildTime`，`status` 与 `doctor` 显示真实构建信息。

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
- 安装时把 `$InstallDir` 中的绝对路径写进插件 `BAKED_BIN`，插件按 `BAKED_BIN`、`AGENT_NOTIFY_BIN`、默认目录、`PATH` 顺序解析运行程序。

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
- 修正自定义安装目录下 OpenCode 插件仍去找 `%USERPROFILE%\bin\agent-notify.exe` 导致任务完成不推送的问题。
- 配置与凭据保存改为直接原子替换，写入失败时不再先删掉上一份可用文件。

### Removed

- 删除所有旧运行时、包装脚本、模块加载和兼容入口。
- 删除旧品牌命名、旧配置文件、旧 marker、旧日志和旧环境变量。
- 删除旧安装记录、旧快捷方式及旧发布包命名。
- 不提供旧版本配置迁移或别名；v1.0.0 只使用本文档中的新契约。
