# 架构与设计

面向维护者。用户使用说明见 [README](../README.md)，故障排查见 [TROUBLESHOOTING](TROUBLESHOOTING.md)。

## 组件总览

```text
OpenCode
  └─ plugin/agent-notify.ts
        └─ agent-notify.exe notify --agent opencode

Codex
  └─ config.toml notify
        └─ agent-notify.exe codex turn-ended
              ├─ 透传 codex-computer-use.exe
              └─ 发送 ClawBot 消息

Antigravity
  └─ ~/.gemini/config/hooks.json
        └─ .\agent-notify-hook.cmd antigravity stop
              ├─ 仅 fullyIdle=true 时运行
              └─ transcript 尾部提取摘要

Devin
  └─ %APPDATA%/devin/config.json hooks.Stop
        └─ agent-notify.exe devin stop
              ├─ 跳过 stop_hook_active
              └─ last_assistant_message 提取摘要

命令行 / 脚本
  └─ agent-notify.exe notify

所有发送链路
  └─ internal/notify → internal/clawbot → ClawBot API → 微信

微信引用回复
  └─ widget → clawbot session poll → internal/reply
        ├─ Codex 持久线程: codex queue --thread=<thread-id> --message=<text>
        └─ OpenCode: local spool → session prompt
        ├─ Antigravity: language_server agentapi send-message
        └─ Devin: local spool → Devin extension → 桌面端 ACP stdin（旧 Cascade 走聊天动作）
```

常驻与运维命令：

- `agent-notify.exe widget`：原生 Windows 悬浮窗、托盘和 ClawBot 会话轮询。
- `agent-notify.exe login`：ClawBot 扫码登录，默认继续等待首条微信消息。
- `agent-notify.exe sync`：等待首条微信消息，建立主动推送会话。
- `agent-notify.exe doctor`：配置、凭据、会话、网络和接入自检。
- `agent-notify.exe status`：显示四类推送开关和路由文件位置；`doctor` 同时检查各 Agent 接入与 Codex queue 能力。
- `agent-notify.exe watch`：仅在 Codex notify 指向 `codex-computer-use.exe` 时恢复 AgentNotify。
- `install.ps1` / `uninstall.ps1`：部署和清理，不携带运行时业务逻辑。

## 代码职责

| 路径 | 职责 | 关键约束 |
|---|---|---|
| `installer/agent-notify.iss` | Inno Setup 当前用户安装器、快捷方式、标准卸载与启动项 | 默认安装到 `%LOCALAPPDATA%\Programs\Agent-notify`；不要求管理员权限；升级复用固定 AppId |
| `cmd/agent-notify/main.go` | CLI 命令、控制台处理、交互输出 | 发布版使用 GUI 子系统；仅在实际无可用 stdout 时绑定控制台 |
| `internal/setup` | 首次启动状态检查、隐藏 PowerShell 配置和失败重试 | 不重复执行已完成版本；失败不写完成状态；CLI 子命令不触发 |
| `internal/agent` | OpenCode、Codex、Antigravity、Devin、toggle、Codex 配置恢复 | Hook 路径避免阻塞；每个 Stop Hook 都输出兼容的继续语义；Codex 先透传再推送 |
| `internal/clawbot` | 二维码登录、凭据与会话状态、消息轮询、ClawBot API | 账号隔离；context token 持久化；`-14` 停止重试 |
| `internal/notify` | 协议块解析、消息渲染、发送、JSONL 历史 | 默认不限长；显式 `MaxChars` 计入标题、正文和页脚 |
| `internal/reply` | 引用路由、入站去重、按 Agent 注册的 `ReplySender` 分发 | 精确消息 ID、账号隔离、至多一次 Claim、无最近会话回退；Antigravity 调用官方 agentapi，OpenCode / Devin 复用本地 spool |
| `internal/config` | 配置默认值、校验、原子保存、路径解析 | 所有路径可由 `AGENT_NOTIFY_*` 隔离 |
| `internal/marker` | 四个 Agent 的 `.off` 开关 | 文件存在即暂停；不读取旧 marker |
| `internal/ui` | 原生 Win32 悬浮窗、设置、登录、历史、托盘 | 单实例、DPI 感知、双缓冲、会话状态实时刷新、`windowsgui` 发布模式 |
| `plugin/agent-notify.ts` | OpenCode V2 插件 | 安装器写入 `BAKED_BIN`；发送通知并维护引用回复心跳、收件箱和 `session.prompt` / `promptAsync` 兼容投递 |
| `plugin/devin-extension` | Devin 桌面端回复扩展 | ACP 会话直接向桌面端 `devin.exe acp` 子进程写 `session/prompt`（旧 Cascade 保留精确直发/聊天面板回退），维护心跳、持久收件箱、至多一次认领与结果回写 |
| `install.ps1` / `uninstall.ps1` | 文件分发、安装记录、快捷方式，以及 Codex / Antigravity / Devin 接入 | 共享 `tools/hook-config.ps1`；只改 AgentNotify 自己的 Hook，保留其他 JSON 配置 |

## OpenCode 数据流

1. 插件订阅 `session.execution.succeeded`。
2. 取会话标题和最近一条 assistant 文本；两项读取各自默认 10 秒超时，超时退回默认标题或空摘要，不影响推送。
3. 检查 `AGENT_NOTIFY_OFF`、`opencode.off`、内存冷却与跨进程状态冷却。
4. 调用 `agent-notify.exe notify --agent opencode --title ... --summary ... --session ... --no-stdin`。
5. CLI 解析并移除协议块，检查开关、标题勿扰标记和时段勿扰，然后渲染标题、正文与本地时间页脚。
6. 成功发送、发送失败或策略跳过都写入 `%TEMP%\agent-notify\push.log`。
7. 插件每 5 秒更新引用回复心跳，并从本地收件箱原子认领任务；成功后优先调用已有会话的 `session.prompt`，旧版环境回退到 `promptAsync`，提交默认 30 秒未返回时按状态未知上报且不重试。

插件侧与 CLI 侧都做开关和冷却检查，避免不同 OpenCode location 重复拉起进程。

桌面端插件上下文契约（按本机 OpenCode 桌面端 2.0.3 的加载器核对）：

- 加载器调用 `setup(ctx)`；本机 2.0.3 实测上下文中 `ctx.event.subscribe({ signal })` 是可迭代事件流，`ctx.session.prompt` 存在。插件按 `ctx.session.prompt` → `ctx.client.session.promptAsync` → `ctx.session.promptAsync` 顺序探测投递能力，并把结果写进心跳的 `ready`，因此不依赖单一版本的上下文形状。
- 当前版 `ctx.session.prompt({ sessionID, text, delivery })` 的参数形状与桌面端内部 session 服务一致：内部 `PromptInput` 是 `{ text, files, agents, skills }`，`session.prompt` 把这些字段平铺在请求里，所以正文必须放在 `text`；已发布 SDK 1.18.15 的 `{ prompt: { text } }` 只是 HTTP 端点包装，不是插件上下文签名。
- 三个分支都是“已接纳”语义：内部 `session.prompt` 在入力持久接纳并调度 agent-loop 后返回；v1 `/session/{id}/prompt_async` 返回 204 已接受；v2 `promptAsync({ sessionID, parts })` 文档说明会在需要时启动会话并立即返回。插件不等待整轮任务完成，也不会把超时当成“任务仍在跑”。
- 因此 30 秒超时只表示投递状态未确认，不重试：它既不能证明消息未到达，也不能用重发来补偿。
- 旧版桌面端只有 HTTP 形状时，`promptAsync({ path: { id }, body: { parts } })` 与 `promptAsync({ sessionID, parts })` 两个回退分支继续保留；两者都带 `throwOnError`，错误会写回微信。

## Codex 数据流

1. Codex 完成一轮后调用 `agent-notify.exe codex turn-ended <json>`。
2. `HandleCodex` 读取 stdin，并动态查找最新 `codex-computer-use.exe`。
3. 将原始参数与 stdin 透传给上游程序。
4. 检查 `codex.off`；关闭时只跳过推送，不影响上游透传。
5. 摘要取 `last-assistant-message`；标题按 `threads.name → threads.title → threads.first_user_message → session_index.jsonl → payload 首条消息 → 跑完了` 顺序解析。
6. 从 `thread-id`（兼容 `thread_id`）提取线程 ID；成功发送且 ID 非空时写入 30 天引用路由。
7. 渲染 Markdown 通知（加粗标题行 `**🟢 Codex｜会话名**`、正文、`—` 分隔与页脚）后发送并写入结构化历史；引用回复命中路由后执行 `codex queue`，成功后回一条送达确认（`replyConfirmation` 可关），消息可写入尚未在前台打开的持久化线程。

`agent-notify watch` 与悬浮窗只会在 notify 行仍指向 `codex-computer-use.exe` 时恢复配置；自定义 notify 程序始终保留。

## Antigravity 数据流

1. Antigravity 在 execution loop 停止时向 `antigravity stop` 写入 JSON stdin。
2. `HandleAntigravityStop` 只接受 `fullyIdle=true` 且 `conversationId` 非空的事件。
3. `readAntigravityTranscriptSummary` 读取 `transcriptPath` 最后 512 KiB，容忍逐行 JSON schema 差异，并优先选择 assistant 文本；读取失败时退化为空摘要。
4. `resolveAntigravityTitle` 优先读取 `%USERPROFILE%\.gemini\antigravity\annotations\<conversationId>.pbtxt`；文件尚未生成时降级到 transcript 的首条用户请求，仍不可用时使用默认标题。
5. 共享 `sendWithAgentPolicy` 统一处理 marker、勿扰时段、勿扰标题和 DryRun，再写入引用路由。
6. CLI wrapper 无论解析或发送结果如何都只输出 `{}`，不让通知故障阻塞 Antigravity。

## Devin 数据流

1. Devin 在 Stop 事件向 `devin stop` 写入 JSON stdin。
2. `HandleDevinStop` 跳过 `stop_hook_active=true`，并在 `hook_event_name` 存在时要求其为 `Stop`。
3. 通知正文取 `last_assistant_message`，引用目标取稳定字段 `session_id`；不使用当前工作目录或会话列表。
4. 共享策略和 GUI wrapper 与 Antigravity 相同，Hook 失败始终以 `{}` 继续。

Antigravity 回复由 `agentAPIProcess` 调用当前运行语言服务的官方 `agentapi`。发现顺序覆盖 LocalAppData 与 AppData 下的 Antigravity 安装，按进程安装目录和 PID 过滤，再从命令行读取本次启动的 CSRF token、从该 PID 的监听端口构造候选端点；先用 `get-conversation-metadata` 精确确认 `conversationId`，确认后才调用 `send-message`，且只尝试 HTTP 端口。

Devin 回复通过 `devin-reply-inbox/{pending,processing,results}` 单向投递，扩展在 Devin 桌面端提供 `devin.sendChatActionMessage` 或本窗口存在 ACP 通道时写入就绪心跳。Go 侧先用 `session_id` 从桌面端状态库（`%APPDATA%\devin\User\globalStorage\state.vscdb`）读出桌面端内部 Cascade 标识 `acp/devin-cli/<session_id>`，作业随正文一起下发该标识；扩展对 `acp/` 前缀的会话直接向该窗口的 `devin.exe acp` 子进程写 `session/prompt` NDJSON，聊天面板只做尽力激活，避免新开对话或落入 Ask 模式；旧 Cascade 会话仍走 `openCascadeIdInChatPanel` + `sendCascadeInput`。查不到登记记录时直接报错，不回退到 CLI 会话号或最近会话。成功后由 `Stop` Hook 推送本轮结果。Go 侧最多等待 10 秒同步结果，超时后按持久队列已接收处理并继续观察，任务过期或处理中断都不会自动重放。

Antigravity 的 CSRF token 和端口每次启动都会变化，因此不做缓存；Devin 回复全程由桌面端自己处理，不读取 CLI 登录状态、不检查会话锁、也不启动第二个 Agent 进程。

## ClawBot 2.4.6 契约

默认入口固定为 `https://ilinkai.weixin.qq.com`。登录成功后，业务 API 使用响应中的 `baseurl`。

| 阶段 | 请求 | 作用 |
|---|---|---|
| 获取二维码 | `POST /ilink/bot/get_bot_qrcode?bot_type=3`，携带 `local_token_list` 与 `base_info` | 启动扫码登录 |
| 扫码状态 | `GET /ilink/bot/get_qrcode_status?qrcode=...`，需要时追加 `verify_code` | 处理扫码、配对码、跳转和过期 |
| 收消息 | `POST /ilink/bot/getupdates` | 长轮询消息并取得 `context_token` 与新游标 |
| 发消息 | `POST /ilink/bot/sendmessage` | 使用持久化的 `context_token` 主动推送 |
| 生命周期 | `POST /ilink/bot/msg/notifystart` / `notifystop` | 最佳努力通知服务端客户端上下线 |

所有业务请求都带 `base_info.channel_version = "2.4.6"` 与 `base_info.bot_agent`，并使用 `iLink-App-Id`、`iLink-App-ClientVersion` 和随机 `X-WECHAT-UIN` 请求头。发送请求的 `from_user_id` 必须为空，`to_user_id` 使用绑定的 `ilink_user_id`。

`sendmessage` 响应可能把消息 ID 放在顶层、`msg` 或 `data` 中，字段名兼容 `message_id`、`msg_id` 和 `msgid`；客户端同时保留请求生成的 `client_id`。`getupdates` 的引用结构兼容顶层 `referenced_msg_id`、`ref_msg.msg_id`、`ref_msg.referenced_msg_id` 和 `ref_msg.message_item.msg_id`，数字与字符串都归一化为字符串。

如果同一条入站消息暴露多个不一致的引用 ID，系统不会选择任意一个，而是拒绝路由。P0 调试通过 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 分别记录发送请求生成的 `client_id`、脱敏后的 `sendmessage` 响应和解析结果，以及 `getupdates` 中解析出的引用 ID；token 类字段和消息正文不会落盘。
调试记录同时保存 `has_reference`，用于区分普通消息与“结构上引用了消息、但平台没有返回可路由 ID”的异常样本；后者在 P0 中直接判为失败。

每条发送与引用诊断还带 `account_scope`，它是 bot ID 与绑定用户 ID 的不可逆 SHA-256 前缀；入站记录额外保存 `private` 与 `bound_sender`。`reply-check` 只统计当前登录凭据对应 scope 的私聊、绑定发送者样本，跨账号、群聊、陌生发送者和升级前的旧格式记录会被计入 `ignoredSends` / `ignoredQuotes` 且不参与结论，避免换账号或历史日志把 P0 对照“凑通过”。

`agent-notify reply-check` 以只读方式复用同一套解析：`internal/clawbot` 提供类型化的调试日志读取与字段访问器，`internal/reply.EvaluateReplyGateForAccountWithSends` 合并 scoped `sendmessage-result` 与当前账号未过期的 `RouteStore.ListActive` 路由作为发送证据，再把发送 ID 与引用 ID 精确对照，并通过与分发器相同的 `RouteStore.Find` 校验路由。其他账号、过期的本地路由不得进入证据集；只有全部引用样本都精确匹配并成功解析到路由时才给出“P0 对照通过”；任一未匹配、引用 ID 冲突、引用标记缺少 ID，或路由缺失、过期、无法读取，都会判为未通过。

二维码状态机覆盖：

- `wait`、`scaned`：继续轮询。
- `need_verifycode`：读取数字配对码，并在下一次状态请求中携带。
- `verify_code_blocked`、`expired`：按上限刷新二维码。
- `scaned_but_redirect`：后续状态轮询切换到 `redirect_host`。
- `binded_redirect`：只有本机存在有效旧凭据时才算成功，否则重新登录。
- `confirmed`：保存 token、bot id、用户 id 和业务 `baseurl`。

扫码成功只代表登录完成，不代表可以主动发送。主动发送依赖 `getupdates` 返回的每条会话 `context_token`，因此用户还必须先给 ClawBot 发送一条消息。此规则不能由旧状态或猜测补齐。

## 会话与凭据状态

收到 `getupdates` 响应后，客户端原子持久化：

```json
{
  "bot_token": "...",
  "ilink_bot_id": "...",
  "baseurl": "https://ilinkai.weixin.qq.com",
  "ilink_user_id": "...",
  "context_token": "...",
  "context_user_id": "...",
  "get_updates_buf": "...",
  "stale_at": ""
}
```

- `context_token` 和游标只允许在同一 bot 账号、同一绑定用户下复用。
- 切换账号会清空旧账号的游标和上下文。
- `ret=-14` 或 `errcode=-14` 表示 token 失效；立即停止轮询，清空上下文与游标，写入 `stale_at`，等待用户重新扫码。
- 未建立会话时，领取试发送返回 `会话未建立`，不会消耗无意义的重试。
- `sendmessage` 返回 `ret=-2 prepare failed` 时保留登录与消息游标，但清除已失效的上下文并显示“会话未建立”；用户再次给 ClawBot 发消息即可重建会话。
- 悬浮窗和 `login` 都通过同一个 `RunSessionLoop` 或一次性 `sync` 路径获取上下文，不维护第二套协议实现。

`status` 与悬浮窗只显示脱敏后的用户标识，不输出 token 或 context token。

## 引用路由状态

`internal/notify` 在通知发送成功后写入 `reply-routes.jsonl`。每条路由包含平台消息 ID、发送 `client_id`、bot ID、绑定用户 ID、Agent、线程或会话 ID、创建时间和过期时间。默认有效期为 30 天，账号作用域不匹配的旧路由不会被复用。

`internal/reply.StateStore` 对入站消息先写入 `claimed`，再执行命令。Claim 与路由同样默认保留 30 天，并按 bot ID 和绑定用户隔离；即使进程在命令执行中崩溃，游标重放或长轮询重投也不会再次提交。没有入站 `msg_id` 时使用 `seq + 引用 ID + 文本哈希` 生成确定性回退键。

路由与去重文件采用 JSON Lines、`0600` 权限和内核级文件锁。Windows 使用 `LockFileEx`，其他平台使用 `flock`，避免并发 CLI 或悬浮窗同时写入时丢记录。损坏的单行会被忽略，不影响其余路由。

两类文件达到大小阈值后会在持锁状态下检查过期比例；当可回收记录足够多时，先写入同目录临时文件并 `fsync`，再用原子替换压缩，避免长期只追加导致无限增长。

Codex 分发使用参数数组直接启动隐藏窗口的 `codex queue`，超时 30 秒，不经过 shell，也不添加任何审批或沙箱绕过参数。命令超时或调用方取消时，系统无法证明消息尚未到达 Codex，因此按“投递未确认”处理，不自动重试并提示用户先检查目标会话。

Codex 可执行文件解析顺序为：显式注入或 `AGENT_NOTIFY_CODEX_BIN`、当前 PATH、`%LOCALAPPDATA%\OpenAI\Codex\bin` 下更新时间最新的本地安装。这样由启动项拉起的悬浮窗不会依赖 Codex 桌面端仅对子进程临时注入的 PATH。

OpenCode 使用 `opencode-reply-inbox/{pending,processing,results}` 单向投递，插件心跳不新鲜或不支持 `session.prompt` / `promptAsync` 时拒绝提交；处理中断的 `processing` 任务只报告失败，绝不自动重放。

Dispatcher 只负责校验、去重和精确路由查找，实际投递统一交给按 Agent 注册的 `ReplySender`；新增 Agent 时无需修改分发分支。

Antigravity 与 Devin 都只把 `Route.SessionID` 作为目标参数。Antigravity 通过官方 agentapi 先验会话再发送；Devin 先按该 ID 解析桌面端 Cascade 标识，再由扩展向桌面端 ACP 子进程写 `session/prompt`（旧 Cascade 会话回退到聊天面板提交）。两者的明确失败都会同步回写微信，超过同步窗口则按持久队列已接收处理并异步观察，避免 dispatcher 超时诱发重复发送。

本机 Codex CLI 的 `queue` 会把消息写入目标线程的本地持久队列，因此已保存但未在前台打开的线程也能精确接收回复；恢复该线程后，Codex 消费队列中的消息。归档线程会拒绝入队并提示先运行 `codex unarchive`，已删除或不存在的线程同样返回可见错误。`codex exec resume` 会直接运行一个完整新回合，不能在 30 秒内确认“已入队”并在超时时安全退出，因此不作为静默回退。

OpenCode 每个插件实例在 `opencode-reply-inbox/heartbeats` 下维护独立心跳租约；正常退出只删除自己的租约，异常退出的租约最多在 30 秒后失效，不会因一个实例退出而误判其他 location 实例离线。

处理中的任务会记录认领它的插件实例；只要该实例心跳仍新鲜，其他实例就不会把长时间运行的会话投递当作崩溃任务回收。实例心跳过期后才记录处理中断并停止重试，保持至多一次语义。

OpenCode 的 Go 侧最多等待 10 秒同步结果；窗口内拿到失败会立即回微信，超时则视为本地队列已接收。该窗口覆盖插件 5 秒心跳周期。超时后会在任务有效期内继续观察结果：后续会话投递失败或最终未确认会回写微信，但不会自动重放 Agent 任务。

## 路径与隔离

| 类型 | 默认位置 |
|---|---|
| 配置 | `%USERPROFILE%\.config\agent-notify\config.json` |
| 凭据与会话 | `%USERPROFILE%\.config\agent-notify\clawbot.json` |
| marker | `%USERPROFILE%\.config\agent-notify\{opencode,codex,antigravity,devin}.off` |
| 历史 | `%TEMP%\agent-notify\push.log` |
| 引用路由与去重 | `%USERPROFILE%\.config\agent-notify\reply-{routes,state}.jsonl` |
| OpenCode 回复收件箱 | `%USERPROFILE%\.config\agent-notify\opencode-reply-inbox` |
| Devin 回复收件箱 | `%USERPROFILE%\.config\agent-notify\devin-reply-inbox` |
| 运行日志 | `%TEMP%\agent-notify\*.log` |
| 首次接入状态 | `%USERPROFILE%\.config\agent-notify\setup-state.json` |
| 首次接入失败日志 | `%TEMP%\agent-notify\setup.log` |
| Codex 标题诊断 | `%TEMP%\agent-notify\codex-title.log` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |
| Devin 回复扩展 | `%USERPROFILE%\.devin\extensions\agent-notify` |
| Antigravity Hook | `%USERPROFILE%\.gemini\config\hooks.json` |
| Antigravity 会话标题 | `%USERPROFILE%\.gemini\antigravity\annotations\<conversationId>.pbtxt` |
| Devin Hook | `%APPDATA%\devin\config.json` |
| Devin 桌面端会话元数据（只读） | `%APPDATA%\devin\User\globalStorage\state.vscdb` |

插件副本的 `BAKED_BIN` 指向安装目录里的 exe，安装到自定义目录时不需要额外环境变量。手动移动 exe 后需重跑 `install.ps1`，或用 `AGENT_NOTIFY_BIN` 覆盖。

测试与便携部署可覆盖 `AGENT_NOTIFY_CONFIG_DIR`、`AGENT_NOTIFY_TEMP_DIR`、`AGENT_NOTIFY_CONFIG_FILE`、`AGENT_NOTIFY_CREDENTIAL_FILE`、`AGENT_NOTIFY_LOG_FILE`、`AGENT_NOTIFY_SETUP_STATE_FILE`、`AGENT_NOTIFY_SETUP_LOG_FILE`、`AGENT_NOTIFY_ANTIGRAVITY_HOOKS`、`AGENT_NOTIFY_ANTIGRAVITY_BIN`、`AGENT_NOTIFY_ANTIGRAVITY_ANNOTATIONS_DIR`、`AGENT_NOTIFY_DEVIN_CONFIG`、`AGENT_NOTIFY_DEVIN_REPLY_DIR`、`AGENT_NOTIFY_DEVIN_DESKTOP_DB` 和相关 marker 路径。

## 安装、首次接入与卸载

标准发布同时提供安装器、ZIP 和校验文件：

```text
dist/
├── Agent-notify-Setup-vX.Y.Z.exe
├── Agent-notify-vX.Y.Z.zip
└── SHA256SUMS.txt
```

`Agent-notify-Setup-vX.Y.Z.exe` 由 `installer/agent-notify.iss` 生成，是普通用户的主安装入口。它按当前用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，复制运行程序、插件、扩展和配置脚本，创建开始菜单、可选桌面快捷方式和可选开机启动项，并注册标准卸载入口。安装完成页默认启动 `agent-notify.exe widget`。

ZIP 保留给便携、开发和旧版更新兼容，内部结构仍为：

```text
Agent-notify/
├── bin/agent-notify.exe
├── plugin/agent-notify.ts
├── plugin/devin-extension/{package.json,extension.js,acp-bridge.js}
├── VERSION
├── install.ps1
├── uninstall.ps1
└── tools/hook-config.ps1
```

标准安装版首次执行 `widget` 时，`internal/setup` 会在显示悬浮窗前做用户级配置：

1. 读取 `%USERPROFILE%\.config\agent-notify\setup-state.json`。文件中的版本与当前版本一致时直接跳过；文件缺失、损坏或版本不一致时继续初始化。
2. 通过隐藏窗口的 `powershell.exe` 调用安装目录中的 `install.ps1 -ConfigureOnly -InstallDir <安装目录> -SkipWidgetLaunch -SkipLoginLaunch -SkipShortcuts`，不显示命令行。
3. `-ConfigureOnly` 不替换 exe、不创建快捷方式、不主动启动登录，只安装或更新 OpenCode 插件、Devin 扩展、Antigravity / Devin Hook 和 Codex notify，并写入安装记录。
4. 成功后原子写入 `setup-state.json` 的版本与完成时间；失败时不写完成状态，把 PowerShell 输出写到 `%TEMP%\agent-notify\setup.log`。
5. 失败时悬浮窗仍可打开，顶部连接卡和健康状态显示“首次接入失败”或“接入异常”。点击“检查修复”会强制重跑初始化，再执行现有可恢复的 Codex notify 检查。

`install.ps1` 的完整模式：

1. 停止安装目录内的 AgentNotify 进程。
2. 复制或构建 `agent-notify.exe`。
3. 复制 `plugin/agent-notify.ts`。
4. 复制 `plugin/devin-extension` 到 Devin 用户扩展目录。
5. 写入 `agent-notify-install.json`，记录版本和安装目录内文件。
6. 按安装记录清理已不再分发的旧文件。
7. 使用共享 `tools/hook-config.ps1` 写入/替换 Antigravity 顶层 `agent-notify` Hook，并合并 Devin `hooks.Stop` handler。
8. 在安全条件下接管 Codex notify。
9. 创建开机启动和桌面快捷方式，并启动悬浮窗。

标准安装版卸载入口由 Inno Setup 注册，卸载时隐藏调用 `uninstall.ps1`。卸载器只删除安装记录中的程序文件、固定插件和 AgentNotify 快捷方式，移除 AgentNotify 自己写入的 Antigravity / Devin Hook 与 Codex notify 配置，并保留其他 Hook、用户配置和凭据。ClawBot 登录凭据、`config.json`、推送历史和引用路由默认保留在 `%USERPROFILE%\.config\agent-notify`。

卸载 Devin 扩展前会校验 `package.json` 的 `name` 与 `publisher`；只删除明确的扩展文件，目录中其他内容不会被递归清理。

## 无窗口与终端行为

- 发布二进制使用 `-H windowsgui`，避免 agent hook 和开机启动时闪现控制台。
- `attachConsole` 只有在标准 stdout 不可用时才绑定 `CONOUT$`；重定向输出不会被覆盖。
- 悬浮窗由同一 exe 的 `widget` 子命令启动，托盘和窗口消息都由 Go/Win32 管理。
- `notify` 不主动分配控制台；后台 hook 调用不会弹窗。

## UI 线程与 DPI

- 所有窗口都在锁定到操作系统线程的 UI 消息循环中创建和销毁。
- 进程声明 Per-Monitor V2 DPI 感知；窗口按当前显示器 DPI 调整尺寸，命中测试把物理坐标还原为逻辑坐标。
- 绘制统一使用逻辑坐标，由 `internal/ui/scale.go` 缩放；`WM_PAINT` 先画到内存 DC，再一次性拷贝到窗口 DC。
- 设置窗通过 `ShowLoginDialog` 打开二维码登录；扫码轮询在后台 goroutine 中运行。
- 悬浮窗启动后会运行 ClawBot 会话循环，并通过窗口消息刷新连接状态；设置窗使用定时重绘同步四类状态。
- 设置与历史以模态弹窗运行，关闭时恢复父窗口的启用状态和 DPI 状态。

## 配置与去重

- `quietHours`：`23-8` 表示 23:00 到次日 08:00，结束时间不包含。
- `cooldownMin`：OpenCode 同一会话默认 10 分钟去重，范围是 1 到 1440。
- `replyEnabled`：控制引用回复，默认 `false`；只影响入站引用分发，不改变原有推送行为。
- `replyConfirmation`：引用回复成功后是否回一条“✅ 已送达 …”确认，默认 `true`。
- 标题包含 `🔕` 或 `[勿扰]` 时跳过推送。
- marker 在冷却记账前检查，被暂停的会话不会消耗冷却窗口。
- 引用路由和入站 Claim 分别使用账号作用域与消息级持久化，不把“最近通知”当作回退目标。

## 日志

`push.log` 每行是一条 JSON 对象：

```json
{"timestamp":"2026-09-12T09:00:00+08:00","agent":"opencode","session":"...","title":"**🟢 OpenCode｜任务**","summary":"摘要","status":"成功"}
```

失败会额外记录 `error`，成功发送会额外记录可选的 `messageID` 与 `clientID`。历史列表按时间倒序读取，损坏的单行会被跳过，不影响其余记录。

协议调试日志 `clawbot-debug.log` 只在 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 时写入，并对所有 token、secret、authorization、cookie 和 credential 类字段做递归脱敏。链路错误写入 `reply-debug.log`，不记录用户回复正文。

`codex-title.log` 记录线程 ID、标题来源、失败阶段、SQLite 错误码、重试次数和最终降级来源，不记录通知正文。标题读取失败会在原通知正文后追加简短提示，引用路由仍使用原 `thread-id`。

## 版本与发布

- 应用版本唯一来源：`internal/app/version.go` 的 `Version`。
- 本地或 CI 使用 `tools/build-release.ps1` 生成 `Agent-notify-Setup-vX.Y.Z.exe`、`Agent-notify-vX.Y.Z.zip` 和同时包含两者哈希的 `SHA256SUMS.txt`。
- Release workflow 校验 tag `v<版本>` 与 Go 源码版本一致。
- 发布同步使用 `tools/publish-release.ps1` 将构建产物上传到公开的 `agent-notify-releases` 仓库，源码仓库无需公开。
- `tools/build-installer.ps1` 校验 `VERSION` 与 Go 源码版本一致，再调用 Inno Setup 生成安装器。
- 最终用户不需要安装 Go；ZIP 解压后包含 `bin/agent-notify.exe`，标准安装器直接复制已构建的程序。

## 自动更新

悬浮窗的“升级”按钮调用 `internal/update`：

1. 从 GitHub `releases/latest` 读取稳定版本，要求版本严格高于当前 SemVer。
2. 优先接受与版本对应的 `Agent-notify-Setup-vX.Y.Z.exe`；Release 缺少安装器时回退到 `Agent-notify-vX.Y.Z.zip`，两种情况都要求同名 `SHA256SUMS.txt`，不接受模糊文件名或旁路下载地址。
3. 默认更新源是只分发编译产物的公开仓库 `srafyhucl-cpu/agent-notify-releases`，源码仓库保持私有。
4. 安装器和校验文件先下载到 `%TEMP%\agent-notify\updates`；下载后校验 SHA256，并检查安装器是否为 Windows PE 文件。
5. 安装器校验通过后以 `/SILENT /NORESTART` 直接启动，由 Inno Setup 原地覆盖现有安装并重新启动悬浮窗。校验失败时删除下载内容，不启动安装程序。
6. ZIP 兼容路径继续校验 SHA256，并拒绝越界路径、符号链接、超限文件、版本不一致或缺少安装脚本的包；校验通过后由隐藏 PowerShell 进程调用新版本自带的 `install.ps1`。
7. 两种路径都继承用户当前的安装目录与 Agent 配置路径，并保留 ClawBot 凭据、配置、历史和引用路由。
8. 更新只重启 Agent-notify 悬浮窗，不启动、关闭或重启任何 Agent。

## 不可破坏的契约

1. 安装入口名与位置：`agent-notify.exe`、`agent-notify.ts`、`agent-notify-install.json`。
2. 所有用户可覆盖项统一使用 `AGENT_NOTIFY_*`。
3. marker 文件名为 `opencode.off`、`codex.off`、`antigravity.off` 与 `devin.off`，存在即暂停。
4. ClawBot 登录、`context_token` 建立、凭据字段和发送消息结构。
5. Codex notify 透传顺序：先上游，后推送；推送失败不得影响透传。
6. `push.log` 保持稳定的 JSON Lines 结构。
7. 发布 exe 保持 Windows GUI 子系统，悬浮窗与 hook 不闪控制台。
8. v1.0.0 不读取旧品牌名称、旧模块、旧脚本、旧环境变量或命令别名。
9. 引用回复只允许精确消息 ID 路由；禁止标题、正文、最近会话或跨账号回退。
10. 引用回复默认关闭；OpenCode 插件不支持 `session.prompt` 或 `promptAsync` 时必须拒绝任务并返回可见错误。
11. Antigravity Hook 使用独立顶层键 `agent-notify`；Devin 只修改 `hooks.Stop` 中的 AgentNotify handler，安装与卸载不得覆盖其他 JSON 配置。
12. Antigravity 使用同目录无空格启动器调用安装目录中的 exe，避免其 Windows `cmd /c` 参数转义破坏带引号和空格的命令。
13. Antigravity / Devin Stop wrapper 必须始终输出 `{}`，通知故障不得阻塞 agent。
14. Antigravity / Devin 回复只允许使用通知携带的稳定会话 ID；禁止工作目录、最近会话或标题回退。
