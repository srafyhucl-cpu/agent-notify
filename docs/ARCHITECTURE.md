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

命令行 / 脚本
  └─ agent-notify.exe notify

所有发送链路
  └─ internal/notify → internal/clawbot → ClawBot API → 微信

微信引用回复
  └─ widget → clawbot session poll → internal/reply
        ├─ Codex 持久线程: codex queue --thread=<thread-id> --message=<text>
        └─ OpenCode: local spool → session prompt
```

常驻与运维命令：

- `agent-notify.exe widget`：原生 Windows 悬浮窗、托盘和 ClawBot 会话轮询。
- `agent-notify.exe login`：ClawBot 扫码登录，默认继续等待首条微信消息。
- `agent-notify.exe sync`：等待首条微信消息，建立主动推送会话。
- `agent-notify.exe doctor`：配置、凭据、会话、网络和接入自检。
- `agent-notify.exe status`：显示引用回复开关和路由文件位置；`doctor` 同时检查 `codex queue` 能力。
- `agent-notify.exe watch`：仅在 Codex notify 指向 `codex-computer-use.exe` 时恢复 Agent-notify。
- `install.ps1` / `uninstall.ps1`：部署和清理，不携带运行时业务逻辑。

## 代码职责

| 路径 | 职责 | 关键约束 |
|---|---|---|
| `cmd/agent-notify/main.go` | CLI 命令、控制台处理、交互输出 | 发布版使用 GUI 子系统；仅在实际无可用 stdout 时绑定控制台 |
| `internal/agent` | OpenCode、Codex、toggle、Codex 配置恢复 | hook 路径避免阻塞；Codex 先透传再推送；只读解析真实会话名 |
| `internal/clawbot` | 二维码登录、凭据与会话状态、消息轮询、ClawBot API | 账号隔离；context token 持久化；`-14` 停止重试 |
| `internal/notify` | 协议块解析、消息渲染、发送、JSONL 历史 | 默认不限长；显式 `MaxChars` 计入标题、正文和页脚 |
| `internal/reply` | 引用路由、入站去重、按 Agent 注册的 `ReplySender` 分发 | 精确消息 ID、账号隔离、至多一次 Claim、无最近会话回退 |
| `internal/config` | 配置默认值、校验、原子保存、路径解析 | 所有路径可由 `AGENT_NOTIFY_*` 隔离 |
| `internal/marker` | `opencode.off` / `codex.off` 开关 | 文件存在即暂停；不读取旧 marker |
| `internal/ui` | 原生 Win32 悬浮窗、设置、登录、历史、托盘 | 单实例、DPI 感知、双缓冲、会话状态实时刷新、`windowsgui` 发布模式 |
| `plugin/agent-notify.ts` | OpenCode V2 插件 | 安装器写入 `BAKED_BIN`；发送通知并维护引用回复心跳、收件箱和 `session.prompt` / `promptAsync` 兼容投递 |
| `install.ps1` / `uninstall.ps1` | 文件分发、安装记录、快捷方式、Codex 接管 | 不安装业务运行时；卸载按安装记录清理 |

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
7. 渲染 `【codex】会话名`、正文和 `Codex · yyyy/MM/dd HH:mm` 页脚后发送并写入结构化历史；引用回复命中路由后执行 `codex queue`，消息可写入尚未在前台打开的持久化线程。

`agent-notify watch` 与悬浮窗只会在 notify 行仍指向 `codex-computer-use.exe` 时恢复配置；自定义 notify 程序始终保留。

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

本机 Codex CLI 的 `queue` 会把消息写入目标线程的本地持久队列，因此已保存但未在前台打开的线程也能精确接收回复；恢复该线程后，Codex 消费队列中的消息。归档线程会拒绝入队并提示先运行 `codex unarchive`，已删除或不存在的线程同样返回可见错误。`codex exec resume` 会直接运行一个完整新回合，不能在 30 秒内确认“已入队”并在超时时安全退出，因此不作为静默回退。

OpenCode 每个插件实例在 `opencode-reply-inbox/heartbeats` 下维护独立心跳租约；正常退出只删除自己的租约，异常退出的租约最多在 30 秒后失效，不会因一个实例退出而误判其他 location 实例离线。

处理中的任务会记录认领它的插件实例；只要该实例心跳仍新鲜，其他实例就不会把长时间运行的会话投递当作崩溃任务回收。实例心跳过期后才记录处理中断并停止重试，保持至多一次语义。

OpenCode 的 Go 侧最多等待 10 秒同步结果；窗口内拿到失败会立即回微信，超时则视为本地队列已接收。该窗口覆盖插件 5 秒心跳周期。超时后会在任务有效期内继续观察结果：后续会话投递失败或最终未确认会回写微信，但不会自动重放 Agent 任务。

## 路径与隔离

| 类型 | 默认位置 |
|---|---|
| 配置 | `%USERPROFILE%\.config\agent-notify\config.json` |
| 凭据与会话 | `%USERPROFILE%\.config\agent-notify\clawbot.json` |
| marker | `%USERPROFILE%\.config\agent-notify\{opencode,codex}.off` |
| 历史 | `%TEMP%\agent-notify\push.log` |
| 引用路由与去重 | `%USERPROFILE%\.config\agent-notify\reply-{routes,state}.jsonl` |
| OpenCode 回复收件箱 | `%USERPROFILE%\.config\agent-notify\opencode-reply-inbox` |
| 运行日志 | `%TEMP%\agent-notify\*.log` |
| Codex 标题诊断 | `%TEMP%\agent-notify\codex-title.log` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |

插件副本的 `BAKED_BIN` 指向安装目录里的 exe，安装到自定义目录时不需要额外环境变量。手动移动 exe 后需重跑 `install.ps1`，或用 `AGENT_NOTIFY_BIN` 覆盖。

测试与便携部署可覆盖 `AGENT_NOTIFY_CONFIG_DIR`、`AGENT_NOTIFY_TEMP_DIR`、`AGENT_NOTIFY_CONFIG_FILE`、`AGENT_NOTIFY_CREDENTIAL_FILE`、`AGENT_NOTIFY_LOG_FILE` 和相关 marker 路径。

## 安装模型

发布包结构：

```text
Agent-notify/
├── bin/agent-notify.exe
├── plugin/agent-notify.ts
├── VERSION
├── install.ps1
├── uninstall.ps1
├── README.md
├── CHANGELOG.md
├── LICENSE
└── .env.example
```

`install.ps1`：

1. 停止安装目录内的 Agent-notify 进程。
2. 复制或构建 `agent-notify.exe`。
3. 复制 `plugin/agent-notify.ts`。
4. 写入 `agent-notify-install.json`，记录版本和安装目录内文件。
5. 按安装记录清理已不再分发的旧文件。
6. 在安全条件下接管 Codex notify。
7. 创建开机启动和桌面快捷方式，并启动悬浮窗。

`uninstall.ps1` 只删除安装记录中的文件、固定插件和 Agent-notify 快捷方式，保留用户配置与凭据。

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
- 标题包含 `🔕` 或 `[勿扰]` 时跳过推送。
- marker 在冷却记账前检查，被暂停的会话不会消耗冷却窗口。
- 引用路由和入站 Claim 分别使用账号作用域与消息级持久化，不把“最近通知”当作回退目标。

## 日志

`push.log` 每行是一条 JSON 对象：

```json
{"timestamp":"2026-09-12T09:00:00+08:00","agent":"opencode","session":"...","title":"【opencode】任务","summary":"摘要","status":"成功"}
```

失败会额外记录 `error`，成功发送会额外记录可选的 `messageID` 与 `clientID`。历史列表按时间倒序读取，损坏的单行会被跳过，不影响其余记录。

协议调试日志 `clawbot-debug.log` 只在 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 时写入，并对所有 token、secret、authorization、cookie 和 credential 类字段做递归脱敏。链路错误写入 `reply-debug.log`，不记录用户回复正文。

`codex-title.log` 记录线程 ID、标题来源、失败阶段、SQLite 错误码、重试次数和最终降级来源，不记录通知正文。标题读取失败会在原通知正文后追加简短提示，引用路由仍使用原 `thread-id`。

## 版本与发布

- 应用版本唯一来源：`internal/app/version.go` 的 `Version`。
- 本地或 CI 使用 `tools/build-release.ps1` 生成 `Agent-notify-v<版本>.zip` 和 `SHA256SUMS.txt`。
- Release workflow 校验 tag `v<版本>` 与 Go 源码版本一致。
- 发布包解压后包含 `bin/agent-notify.exe`，最终用户不需要安装 Go。

## 不可破坏的契约

1. 安装入口名与位置：`agent-notify.exe`、`agent-notify.ts`、`agent-notify-install.json`。
2. 所有用户可覆盖项统一使用 `AGENT_NOTIFY_*`。
3. marker 文件名为 `opencode.off` 与 `codex.off`，存在即暂停。
4. ClawBot 登录、`context_token` 建立、凭据字段和发送消息结构。
5. Codex notify 透传顺序：先上游，后推送；推送失败不得影响透传。
6. `push.log` 保持稳定的 JSON Lines 结构。
7. 发布 exe 保持 Windows GUI 子系统，悬浮窗与 hook 不闪控制台。
8. v1.0.0 不读取旧品牌名称、旧模块、旧脚本、旧环境变量或命令别名。
9. 引用回复只允许精确消息 ID 路由；禁止标题、正文、最近会话或跨账号回退。
10. 引用回复默认关闭；OpenCode 插件不支持 `session.prompt` 或 `promptAsync` 时必须拒绝任务并返回可见错误。
