# 架构与设计

面向维护者。用户使用说明见 [README](../README.md)，故障排查见 [TROUBLESHOOTING](TROUBLESHOOTING.md)。

## 组件总览

2.0.0 的正式入口是两个可执行文件：Tauri 桌面端 `agentnotify-desktop.exe`（工作台式主窗口 + 常驻运行时）
和内部事件入口 `agentnotify-ingress.exe`。Go 版 `agent-notify.exe`、Win32 悬浮窗与管理 CLI 只保留在
源码树里，不再随正式包发布；回滚时安装上一稳定 Release 的安装器。

```text
Agent 客户端
  ├─ OpenCode        plugin/rust/agent-notify.ts（V2 插件，直接提交 ingress）
  ├─ Codex           config.toml notify → agentnotify-codex-hook.exe "codex" "turn-ended"
  │                     ├─ 先透传上游 codex-computer-use.exe（保留 --previous-notify 载荷）
  │                     └─ 再提交 agent.event
  ├─ Antigravity     ~/.gemini/config/hooks.json → .\agent-notify-hook.cmd antigravity stop
  │                     └─ agentnotify-antigravity-hook.exe → agent.event
  ├─ Devin           %APPDATA%\devin\config.json hooks.Stop → agentnotify-devin-hook.exe "devin" "stop"
  │                     └─ agent.event
  └─ Command Code    ~/.commandcode/mods/agent-notify.ts（V2 mod，直接提交 ingress）

所有事件
  └─ agentnotify-ingress.exe
        ├─ 优先：命名管道 \\.\pipe\agentnotify-v1-<SID 哈希>（仅当前用户可连接）
        └─ 回退：%LOCALAPPDATA%\AgentNotify\spool（核心未运行或不可达时持久化）

桌面端 agentnotify-desktop.exe（Tauri 宿主）
  ├─ bridge（类型化 commands/events）→ apps/desktop-ui（React 工作台）
  ├─ agentnotify-runtime（运行时编排、ClawBot 会话循环、管道服务、spool 消费、迁移）
  │     ├─ agentnotify-application（ingest / delivery / reply / policy / status）
  │     ├─ agentnotify-storage-sqlite（SQLite WAL 状态库）
  │     └─ agentnotify-channel-clawbot（扫码登录、长轮询、主动推送）
  └─ update（查询 / 下载 / 校验 / 安装）

微信引用回复
  └─ 运行时轮询入站私聊 → 结构化入站消息 → 精确路由 → AgentAdapter.resume
        ├─ Codex：codex queue（持久线程）
        ├─ OpenCode：本地收件箱 → V2 插件 session.prompt / promptAsync
        ├─ Antigravity：本机 language_server agentapi
        ├─ Devin：本地收件箱 → V2 扩展 → 桌面端 ACP（旧 Cascade 走聊天面板）
        └─ Command Code：本地收件箱 → V2 mod 回复窗口
```

桌面端只提供图形界面：没有 `status` / `doctor` / `notify` / `sync` / `history` 等管理子命令。
`agentnotify-ingress.exe` 无参数时是 stdin 事件协议；面向用户的能力只有只读自检
`--doctor` / `--ping`（不提交事件、不写盘，退出码 0 正常、1 异常），供无头环境探活。
Go 版源码的保留边界与删除条件见 [CONTRIBUTING](../CONTRIBUTING.md) 的「Go 1.x 遗留代码（回滚窗口保留）」。

## 代码职责

| 路径 | 职责 | 关键约束 |
|---|---|---|
| `VERSION` | 应用版本唯一来源 | `tools/sync-version.ps1` 同步到 Tauri 配置、Cargo workspace 与扩展 |
| `crates/agentnotify-domain` | 通知、投递、路由、Claim、时间等纯领域类型 | 不依赖运行时与 IO |
| `crates/agentnotify-application` | 用例编排：ingest、delivery、reply、policy、status | 只通过端口（trait）访问存储与外部系统 |
| `crates/agentnotify-agent-sdk` | `AgentAdapter` 契约、`AgentRegistry` 与共享契约测试 | 新增 Agent 只加 crate + 接入 + 注册项，不改页面分支 |
| `crates/agentnotify-agent-opencode` | OpenCode 适配器：事件标准化、标题/摘要、收件箱投递 | 心跳不新鲜或插件不支持投递时拒绝任务 |
| `crates/agentnotify-agent-codex` | Codex 适配器：事件、标题链、`codex queue` | 标题链降级写进正文提示；queue 失败给可照做的错误 |
| `crates/agentnotify-agent-antigravity` | Antigravity 适配器：Stop 事件、transcript 摘要、annotations 标题、官方 agentapi | 只接受 `fullyIdle=true` + `conversationId`；不缓存 token |
| `crates/agentnotify-agent-devin` | Devin 适配器：Stop 事件、桌面端会话元数据、扩展收件箱 | 只按显式 Cascade 标识投递，无回退 |
| `crates/agentnotify-agent-commandcode` | Command Code 适配器：`run_end` 事件、标题、回复窗口与收件箱 | 窗口开关由界面配置；每个非 Ready 状态都有独立错误 |
| `crates/agentnotify-channel-sdk` / `channel-clawbot` | 渠道契约与 ClawBot 实现（登录、轮询、发送、渲染） | 凭据只进凭据管理器；账号作用域隔离 |
| `crates/agentnotify-storage-sqlite` | SQLite WAL 仓储与旧数据只读导入 | 迁移不写回旧文件；导入状态入库，重复启动不重复迁移 |
| `crates/agentnotify-runtime` | 运行时组装、事件总线、管道服务、spool 消费、迁移检查、日志 | 单实例运行时锁；离线事件先消费 spool |
| `apps/ingress` | 入口协议、命名管道客户端、spool 写入、只读自检 | 只接受 `protocolVersion=1` 的 `agent.event`；`--doctor` / `--ping` 只读不写 |
| `apps/hooks/{codex,antigravity,devin}` | 三个 Stop/notify Hook 的可执行文件 | 只提交事件与写诊断，绝不改变上游退出语义 |
| `hosts/desktop-tauri` | Tauri 宿主：bridge 命令、生命周期（托盘/单实例/自启动）、生产组合根、更新 | 命令只返回脱敏 DTO；宿主初始化前命令等待而不是立刻失败 |
| `apps/desktop-ui` | React 工作台（总览 / Agents / Channels / History / Diagnostics / Settings） | 页面由 descriptor 与 JSON Schema 驱动，不写死 Agent 分支 |
| `plugin/rust/agent-notify.ts` | OpenCode V2 插件 | 安装器写入 `BAKED_INGRESS`；失败全部吞掉 |
| `plugin/devin-extension-v2` | Devin 桌面端回复扩展 | 按显式 `targetID` 投递，ACP 优先、聊天面板回退 |
| `plugin/commandcode-v2` | Command Code V2 mod | 单文件 TypeScript；回复窗口用 `onStop` 的 `reason` 送达正文 |
| `tools/hooks/install-*-v2.ps1` | 五个 Agent 的接入脚本 | 只改 AgentNotify 自己的配置项；冲突时明确报错，不猜路径 |
| `installer/agent-notify.iss` | Inno Setup 当前用户安装器 | 复用固定 AppId 与安装目录；接入失败不阻断安装 |
| `internal/`、`cmd/` | Go 版实现（仅回滚，不参与 2.0 发布） | 不再接收新功能 |

## Agent 适配器

五个适配器都实现 `AgentAdapter`（`crates/agentnotify-agent-sdk`）：

- `descriptor()`：稳定 ID、显示名、说明与 `config_schema`；界面按它渲染配置表单，所以新增 Agent 不需要改页面分支。
- `capabilities()`：`notify` / `resume` / `session_title` / `hook_installer` / `reply_window`。
- `parse_event(envelope)`：把版本化事件标准化为 `NormalizedAgentEvent`；未知事件返回 `InvalidEvent`，
  策略跳过（未空闲、缺会话 ID、hook 重入等）返回 `Ignored`。
- `resume(session_id, text)`：把引用回复送回原会话；只返回"已接纳"回执，不表示回答已完成。
- `inspect()`：接入健康快照，只包含可展示的脱敏信息。

`AgentRegistry` 在启动时按数据库里的 Agent 配置构造全部适配器；`update_agent_config` 会先用合并后的
配置重建注册表校验，再落库并整体替换、重启运行时。Command Code 的回复窗口值在同一层原子写入
mod 读取的 `window.json`，保证界面值与实际生效值同源。

接入方式互相独立，失败不阻塞上游：

| Agent | 事件来源 | 引用回复通道 | 失败诊断 |
|---|---|---|---|---|
| OpenCode | V2 插件订阅 `session.idle` / `session.error` / `session.execution.failed`（兼容旧事件名） | 本地收件箱 → 插件 `session.prompt` / `promptAsync` | `%TEMP%\agent-notify\opencode-debug.log`（`AGENT_NOTIFY_DEBUG=1`） |
| Codex | notify Hook（先透传上游，后提交事件） | `codex queue` | `%TEMP%\agent-notify\codex-notify-debug.log`（仅失败） |
| Antigravity | Stop Hook（启动器 `.cmd` 调 Hook exe） | 官方 `language_server.exe agentapi` | `%TEMP%\agent-notify\antigravity-notify-debug.log`（仅失败） |
| Devin | Stop Hook | 本地收件箱 → 桌面扩展 → ACP / 聊天面板 | `%TEMP%\agent-notify\devin-notify-debug.log`（仅失败） |
| Command Code | V2 mod 的 `run_end` | 本地收件箱 → mod 回复窗口 | `%TEMP%\agent-notify\commandcode-debug.log`（`AGENT_NOTIFY_DEBUG=1`） |

Hook 的硬约束：

- Codex Hook 先运行上游、再提交 ingress；上游找不到、超时或失败只写诊断，退出码原样返回上游结果。
- Antigravity / Devin Hook 无论解析或提交结果如何都输出 `{}`，通知故障不得阻塞 Agent。
- 三个 Hook 都只写失败日志（512 KiB 上限，超限清空重写），日志只含错误码与原因，不含令牌与正文。

## 内部事件入口

`apps/ingress` 定义版本化事件协议；五个接入方都通过它提交事件：

```json
{
  "protocolVersion": 1,
  "kind": "agent.event",
  "requestId": "<UUID>",
  "agentId": "codex",
  "payload": { "...": "原始事件字段" }
}
```

- 大小上限：整包 320 KiB、`payload` 256 KiB、`payload.body` 64 KiB；超出整包拒收并返回协议错误。
- `protocolVersion` 必须是 1，`kind` 必须是 `agent.event`，`requestId` 必须是 UUID。
- Windows 上优先连接命名管道 `\\.\pipe\agentnotify-v1-<当前用户 SID 的 SHA-256 前 8 字节>`，
  安全描述符只授予当前用户；连接超时 150 ms，读写超时 5 秒。
- 管道帧格式为 4 字节长度前缀 + JSON；核心用单字节 ACK 回应：`0` 已接收、`1` 请重试（核心侧暂时
  不可用）、`2` 协议无效。只有收到 `0` 才算提交成功。
- 管道不存在、连接失败或收到非 `0` 时，事件写入 `%LOCALAPPDATA%\AgentNotify\spool`（原子 rename），
  核心下次启动时按批次消费；无效事件移入 `spool\quarantine` 并附 `.error` 原因，7 天前的遗留事件清理。
- spool 上限为 10,000 条 / 64 MiB / 单条 256 KiB，超限直接失败并写诊断，不静默丢弃。

## OpenCode 数据流

1. 插件订阅 `session.idle` / `session.error` / `session.execution.failed`，兼容旧版
   `session.execution.succeeded`。
2. 取会话标题和最近一条 assistant 文本；失败或没有正文时只写诊断、不提交。
3. 同一会话的重复终端事件在插件侧去重（256 条内存窗口）；失败后 2 秒内的 idle 被抑制。
4. 通过 `agentnotify-ingress.exe` 提交 `agent.event`，由核心按 Agent 开关、勿扰时段、标题勿扰标记和
   冷却决定投递或跳过。
5. 插件每 5 秒写心跳（30 秒过期），并从 `opencode-reply-inbox` 原子认领回复任务；成功后调用
   `session.prompt`（`delivery: "steer"`），旧环境回退 `promptAsync`，提交默认 30 秒未返回时按状态未知
   上报且不重试。

桌面端插件上下文契约（按本机 OpenCode 桌面端 2.0.3 的加载器核对）：

- 加载器调用 `setup(ctx)`；`ctx.event.subscribe({ signal })` 是可迭代事件流，`ctx.session.prompt` 存在。
  插件按 `ctx.session.prompt` → `ctx.client.session.promptAsync` → `ctx.session.promptAsync` 顺序探测
  投递能力，并把结果写进心跳的 `ready`，因此不依赖单一版本的上下文形状。
- 当前版 `ctx.session.prompt({ sessionID, text, delivery })` 的参数形状与桌面端内部 session 服务一致：
  内部 `PromptInput` 是 `{ text, files, agents, skills }`，`session.prompt` 把这些字段平铺在请求里，
  所以正文必须放在 `text`；已发布 SDK 1.18.15 的 `{ prompt: { text } }` 只是 HTTP 端点包装，不是插件
  上下文签名。
- 三个分支都是"已接纳"语义：内部 `session.prompt` 在入力持久接纳并调度 agent-loop 后返回；v1
  `/session/{id}/prompt_async` 返回 204 已接受；v2 `promptAsync({ sessionID, parts })` 文档说明会在需要时
  启动会话并立即返回。插件不等待整轮任务完成，也不会把超时当成"任务仍在跑"。
- 因此 30 秒超时只表示投递状态未确认，不重试：它既不能证明消息未到达，也不能用重发来补偿。
- 旧版桌面端只有 HTTP 形状时，`promptAsync({ path: { id }, body: { parts } })` 与
  `promptAsync({ sessionID, parts })` 两个回退分支继续保留；两者都带 `throwOnError`，错误会写回微信。

## Codex 数据流

1. Codex 完成一轮后按 `notify` 行调用 `agentnotify-codex-hook.exe "codex" "turn-ended" <json>`。
2. Hook 有界读取 stdin（上限 256 KiB / 2 秒），把参数与 stdin 透传给上游 `codex-computer-use.exe`
   （扫描 `%LOCALAPPDATA%\OpenAI\Codex\runtimes\cua_node` 下最新的运行时；可用
   `AGENT_NOTIFY_CODEX_UPSTREAM` 显式指定），上游总时长上限 30 秒。
3. 再把事件提交给 ingress（上限 10 秒）；失败只写 `codex-notify-debug.log`，不影响上游退出语义。
4. 适配器从 payload 提取 `thread-id`（兼容 `thread_id`）；标题按
   `threads.name → threads.title → threads.first_user_message → session_index.jsonl → payload 首条消息
   → 跑完了` 顺序解析，状态库不可读时在正文里追加降级提示。
5. 核心按策略投递并把通知映射为「渠道 + 账号 + 渠道消息 ID」的引用路由；命中路由后执行
   `codex queue --thread=<id> --message=<text>`（超时 30 秒，不经过 shell），成功后回一条送达确认
   （可在 Settings → 回复 里关闭）。
6. 线程归档、已删除、临时会话、队列不可用或 app-server 状态冲突都会返回可读错误，绝不回退到其他会话。

## Antigravity 数据流

1. Antigravity 在 execution loop 停止时向 `agent-notify-hook.cmd antigravity stop` 写入 JSON stdin，
   启动器调用 `agentnotify-antigravity-hook.exe`。
2. Hook 把原始 Stop 事件原样放进 `payload` 提交给 ingress，并始终输出 `{}`。
3. 适配器只接受 `fullyIdle=true` 且 `conversationId` 非空的 payload；其余返回 `Ignored`。
4. 摘要读取 `transcriptPath` 尾部（容忍逐行 JSON schema 差异）；标题优先读
   `%USERPROFILE%\.gemini\antigravity\annotations\<conversationId>.pbtxt`，文件尚未生成时降级到 transcript
   首条用户请求，仍不可用时使用默认标题，降级会写进正文提示。
5. 引用回复由适配器调用当前运行语言服务的官方 `agentapi`：发现顺序覆盖 LocalAppData 与 AppData 下的
   Antigravity 安装，按进程安装目录和 PID 过滤，再从命令行读取本次启动的 CSRF token、从该 PID 的监听
   端口构造候选端点；先用 `get-conversation-metadata` 精确确认 `conversationId`，确认后才调用
   `send-message`，且只尝试 HTTP 端口。令牌与端口不做缓存。

## Devin 数据流

1. Devin 在 Stop 事件向 `agentnotify-devin-hook.exe "devin" "stop"` 写入 JSON stdin。
2. Hook 把原始事件放进 `payload` 提交给 ingress，并始终输出 `{}`。
3. 适配器跳过 `stop_hook_active=true`，`hook_event_name` 存在时要求为 `Stop`；必须包含非空
   `session_id`，`last_assistant_message` 为空时使用默认正文。
4. 标题只读 `%APPDATA%\devin\cli\sessions.db`（可用 `sessionsDatabase` 覆盖）；查不到时用默认标题并在
   正文里给出降级提示。
5. 回复经 `devin-reply-inbox/{pending,processing,results}` 单向投递，扩展在 Devin 桌面端写就绪心跳。
   核心先用 `session_id` 从桌面端状态库（`%APPDATA%\devin\User\globalStorage\state.vscdb`）读出桌面端
   内部 Cascade 标识 `acp/devin-cli/<session_id>`，作业随正文一起下发该标识；扩展对 `acp/` 前缀的会话
   直接向该窗口的 `devin.exe acp` 子进程写 `session/prompt` NDJSON，聊天面板只做尽力激活，旧 Cascade
   会话仍走 `openCascadeIdInChatPanel` + `sendCascadeInput`。查不到登记记录时直接报错，不回退到 CLI
   会话号或最近会话。
6. 同步等待结果上限 10 秒；超时按持久队列已接收处理并继续观察，任务过期或处理中断都不会自动重放。

## ClawBot 2.4.6 契约

默认入口固定为 `https://ilinkai.weixin.qq.com`。登录成功后，业务 API 使用响应中的 `baseurl`。

| 阶段 | 请求 | 作用 |
|---|---|---|
| 获取二维码 | `POST /ilink/bot/get_bot_qrcode?bot_type=3`，携带 `local_token_list` 与 `base_info` | 启动扫码登录 |
| 扫码状态 | `GET /ilink/bot/get_qrcode_status?qrcode=...`，需要时追加 `verify_code` | 处理扫码、配对码、跳转和过期 |
| 收消息 | `POST /ilink/bot/getupdates` | 长轮询消息并取得 `context_token` 与新游标 |
| 发消息 | `POST /ilink/bot/sendmessage` | 使用持久化的 `context_token` 主动推送 |
| 生命周期 | `POST /ilink/bot/msg/notifystart` / `notifystop` | 最佳努力通知服务端客户端上下线 |

所有业务请求都带 `base_info.channel_version = "2.4.6"` 与 `base_info.bot_agent`，并使用 `iLink-App-Id`、
`iLink-App-ClientVersion` 和随机 `X-WECHAT-UIN` 请求头。发送请求的 `from_user_id` 必须为空，
`to_user_id` 使用绑定的 `ilink_user_id`。

`sendmessage` 响应可能把消息 ID 放在顶层、`msg` 或 `data` 中，字段名兼容 `message_id`、`msg_id` 和
`msgid`；客户端同时保留请求生成的 `client_id`。`getupdates` 的引用结构兼容顶层 `referenced_msg_id`、
`ref_msg.msg_id`、`ref_msg.referenced_msg_id` 和 `ref_msg.message_item.msg_id`，数字与字符串都归一化为
字符串。

如果同一条入站消息暴露多个不一致的引用 ID，系统不会选择任意一个，而是拒绝路由。引用诊断由
`runtime.log` 承担：记录发送生成的 `client_id`、脱敏后的 `sendmessage` 结果与解析出的引用 ID；
token 类字段和消息正文不会落盘。

二维码状态机覆盖：

- `wait`、`scaned`：继续轮询。
- `need_verifycode`：读取数字配对码，并在下一次状态请求中携带。
- `verify_code_blocked`、`expired`：按上限刷新二维码。
- `scaned_but_redirect`：后续状态轮询切换到 `redirect_host`。
- `binded_redirect`：只有本机存在有效旧凭据时才算成功，否则重新登录。
- `confirmed`：保存 token、bot id、用户 id 和业务 `baseurl`。

扫码成功只代表登录完成，不代表可以主动发送。主动发送依赖 `getupdates` 返回的每条会话
`context_token`，因此用户还必须先给 ClawBot 发送一条消息。此规则不能由旧状态或猜测补齐。

## 会话与凭据状态

登录凭据（`bot_token`、`bot_id`、`user_id`、`base_url`）整体存入 Windows 凭据管理器；`context_token`
与绑定的平台用户 ID 成组存入凭据管理器。SQLite 里只保存可展示的状态与游标：

```json
{
  "bot_id_hint": "...",
  "user_id_hint": "...",
  "base_url": "https://ilinkai.weixin.qq.com",
  "stale_at": null,
  "session_established_at": "...",
  "session_alert_at": null,
  "get_updates_buf": "..."
}
```

- `context_token` 和游标只允许在同一 bot 账号、同一绑定用户下复用。
- 切换账号会清空旧账号的游标和上下文。
- `ret=-14` 或 `errcode=-14` 表示 token 失效；立即停止轮询，清空上下文与游标，写入 `stale_at`，
  等待用户重新扫码。
- 未建立会话时，投递返回「会话未建立」，不会消耗无意义的重试。
- `sendmessage` 返回 `ret=-2 prepare failed` 时保留登录与消息游标，但清除已失效的上下文并标记为
  未建立；用户再次给 ClawBot 发消息即可重建会话。
- 界面只显示脱敏后的用户标识，不输出 token 或 context token。

## 引用路由与入站 Claim

通知发送成功后写入「渠道 + 账号 + 渠道消息 ID → Agent + 会话 ID」的引用路由，默认有效期 24 小时
（Settings → 回复 的「路由有效期」可配 60–604800 秒），账号作用域不匹配的旧路由不会被复用。

入站引用回复先写入 Claim（`claimed`）再执行命令；Claim 与路由同样按 bot ID 和绑定用户隔离。
即使进程在命令执行中崩溃，游标重放或长轮询重投也不会再次提交。该至多一次语义的代价是：若进程在写入
`claimed` 之后、投递确认之前崩溃，这条入站引用回复在 TTL 内不会再被处理，且不会向微信回报错误
（进程已退出）。这是"宁可漏投一次也不重复执行"的取舍；用户重新发送一条新的引用回复会得到新的去重键。

路由与 Claim 都存在 SQLite，与通知、投递共用同一个 WAL 事务边界；渠道消息 ID 缺失时使用
`seq + 引用 ID + 文本哈希` 生成确定性回退键。

Dispatcher 只负责校验、去重和精确路由查找，实际投递交给 `AgentRegistry` 里对应 Agent 的
`AgentAdapter.resume`；新增 Agent 时无需修改分发分支。

- Codex 使用参数数组直接启动隐藏窗口的 `codex queue`，超时 30 秒，不经过 shell，也不添加任何审批或
  沙箱绕过参数。命令超时或调用方取消时，系统无法证明消息尚未到达 Codex，因此按"投递未确认"处理，
  不自动重试并提示用户先检查目标会话。
- Codex 可执行文件解析顺序为：显式注入或 `AGENT_NOTIFY_CODEX_BIN`、当前 PATH、
  `%LOCALAPPDATA%\OpenAI\Codex\bin` 下更新时间最新的本地安装。
- OpenCode 使用 `opencode-reply-inbox/{pending,processing,results}` 单向投递；插件心跳不新鲜或不支持
  `session.prompt` / `promptAsync` 时拒绝提交；处理中断的 `processing` 任务只报告失败，绝不自动重放。
- Antigravity 与 Devin 都只把 `Route.SessionID` 作为目标参数。Antigravity 通过官方 agentapi 先验会话再
  发送；Devin 先按该 ID 解析桌面端 Cascade 标识，再由扩展向桌面端 ACP 子进程写 `session/prompt`
  （旧 Cascade 会话回退到聊天面板提交）。两者的明确失败都会同步回写微信，超过同步窗口则按持久队列
  已接收处理并异步观察，避免 dispatcher 超时诱发重复发送。
- Command Code 每个非 Ready 状态都有独立错误码（窗口已过、会话未运行、mod 未运行、心跳无效），
  投递任务有效期 10 分钟，超时按未确认处理。

## 路径与隔离

| 类型 | 默认位置 |
|---|---|
| 主程序 / ingress / 三个 Hook | `%LOCALAPPDATA%\Programs\Agent-notify\` |
| 状态库（设置、账号、历史、路由、Claim） | `%LOCALAPPDATA%\AgentNotify\data\state.db` |
| 运行日志 | `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` |
| 离线事件 spool | `%LOCALAPPDATA%\AgentNotify\spool` |
| 更新临时目录 | `%LOCALAPPDATA%\AgentNotify\temp\updates` |
| 旧数据迁移报告 | `%LOCALAPPDATA%\AgentNotify\data\legacy-import-report.json` |
| OpenCode 回复收件箱 | `%USERPROFILE%\.config\agent-notify\opencode-reply-inbox` |
| Devin 回复收件箱 | `%USERPROFILE%\.config\agent-notify\devin-reply-inbox` |
| Command Code 回复收件箱与窗口 | `%USERPROFILE%\.config\agent-notify\commandcode-reply-inbox` |
| ClawBot 登录凭据 | Windows 凭据管理器 |
| 旧配置（只读迁移来源） | `%USERPROFILE%\.config\agent-notify\` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |
| Devin 回复扩展 | `%USERPROFILE%\.devin\extensions\agent-notify-reply-v2` |
| Antigravity Hook 与启动器 | `%USERPROFILE%\.gemini\config\hooks.json`、`agent-notify-hook.cmd` |
| Devin Hook | `%APPDATA%\devin\config.json` |
| Command Code mod | `%USERPROFILE%\.commandcode\mods\agent-notify.ts` |
| Hook 失败日志 | `%TEMP%\agent-notify\{codex,antigravity,devin}-notify-debug.log` |
| 插件 / mod 调试日志 | `%TEMP%\agent-notify\{opencode,commandcode}-debug.log` |

插件与 mod 副本的 `BAKED_INGRESS` 指向安装目录里的 `agentnotify-ingress.exe`；安装到自定义目录时不需要
额外环境变量。手动移动 exe 后需重跑接入脚本，或用 `AGENT_NOTIFY_INGRESS_BIN` 覆盖。

测试与便携部署可覆盖 `AGENT_NOTIFY_CONFIG_DIR`、`AGENT_NOTIFY_DATA_DIR`、`AGENT_NOTIFY_LOG_DIR`、
`AGENT_NOTIFY_SPOOL_DIR`、`AGENT_NOTIFY_TEMP_DIR`、`AGENT_NOTIFY_INGRESS`、
`AGENT_NOTIFY_INGRESS_BIN`、`AGENT_NOTIFY_CODEX_BIN`、`AGENT_NOTIFY_CODEX_UPSTREAM`、
`AGENT_NOTIFY_ANTIGRAVITY_BIN`、`AGENT_NOTIFY_ANTIGRAVITY_ANNOTATIONS_DIR`、
`AGENT_NOTIFY_DEVIN_REPLY_DIR`、`AGENT_NOTIFY_DEVIN_SESSIONS_DB`、`AGENT_NOTIFY_DEVIN_DESKTOP_DB`、
`AGENT_NOTIFY_COMMANDCODE_REPLY_DIR`、`AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC`、
`AGENT_NOTIFY_UPDATE_REPOSITORY`、`AGENT_NOTIFY_UPDATE_API_BASE` 与相关 marker 路径。

## 安装、首次启动与卸载

标准发布同时提供安装器、ZIP 和校验文件：

```text
dist/
├── Agent-notify-Setup-vX.Y.Z.exe
├── Agent-notify-vX.Y.Z.zip
└── SHA256SUMS.txt
```

`Agent-notify-Setup-vX.Y.Z.exe` 由 `installer/agent-notify.iss` 生成，是普通用户的主安装入口。它按当前
用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，复制桌面端、ingress、三个 Hook、OpenCode 插件模板、
Devin V2 扩展、Command Code V2 mod 与五个接入脚本，创建开始菜单/可选桌面快捷方式与可选开机启动项，
并注册标准卸载入口。安装前检查 WebView2 运行时；缺失时提示，静默安装直接中止并写日志。

安装向导的接入任务是可取消的（OpenCode / Codex / Antigravity / Devin / Command Code），由安装器调用
`tools\hooks\install-*-v2.ps1`，用本次安装目录的绝对路径写入各 Agent 配置。接入失败只记录并在结束时
统一提示，不影响程序本体安装；升级时先清理上一版本写入的旧 Go 版 Hook 与 V1 扩展，再装新接入。

ZIP 保留给便携、开发和旧版更新兼容，内部结构仍为：

```text
Agent-notify/
├── bin/agentnotify-desktop.exe
├── bin/agentnotify-ingress.exe
├── bin/agentnotify-{codex,antigravity,devin}-hook.exe
├── plugin/agent-notify.ts
├── plugin/devin-extension-v2/{package.json,extension.js,acp-bridge.js}
├── plugin/commandcode-v2/agent-notify.ts
├── tools/hooks/install-*-v2.ps1
└── VERSION
```

首次启动时 `agentnotify-runtime` 做一次**只读**旧数据迁移：

1. 检查旧源文件是否存在（`config.json`、`clawbot.json`、五个 `.off` marker、`push.log`、
   `reply-routes.jsonl`、`reply-state.jsonl`）；一个都没有时直接进入 `NotDetected`。
2. 先取得运行时独占锁；若旧版悬浮窗仍在运行（`%TEMP%\agent-notify\widget-alive.txt` 仍新鲜）则拒绝
   读取旧数据，进入只读诊断模式并提示先从旧版退出。
3. 解析旧配置为设置项、按旧版语义继承五个旧 Agent 的开关（有 marker = 停用，没有 marker = 启用）、
   把旧凭据写入凭据管理器、把推送历史导入通知与投递、把旧路由与 Claim 导入对应表；损坏记录跳过并写入警告。
4. 导入结果写入 `legacy-import-report.json`，并把 `legacyImportV1` 状态写进 SQLite；重复启动直接复用
   已有报告，不会重复迁移。
5. 迁移失败时启动"迁移诊断模式"：只允许查看诊断，不写入新数据；Diagnostics 页可备份旧目录后重试。

卸载入口由 Inno Setup 注册，卸载时先用 `uninstall.ps1 -HooksOnly` 移除 AgentNotify 自己写入的 Codex
notify、Antigravity Hook 与启动器、Devin handler 与 V2 扩展、Command Code mod，再删除程序文件与快捷
方式；SQLite 状态库、迁移报告、旧配置与 OpenCode 插件文件都保留。

## 桌面端宿主与界面

- 单实例：运行时锁保证只有一个 AgentNotify 进程持有状态库；重复启动只聚焦已有窗口。
- 托盘常驻：关闭主窗口只隐藏到托盘，托盘可显示窗口、暂停/恢复通知与退出。
- 宿主与界面通过类型化 bridge 通信：命令只返回脱敏 DTO，事件用于推送快照与状态变化；页面不直接
  读文件或数据库。
- 页面由 descriptor 与 JSON Schema 驱动：Agents 页按适配器 descriptor 渲染，Channels 页按渠道
  descriptor 渲染，所以新增 Agent/渠道只需要加适配器与注册项，不改页面分支。
- 宿主初始化完成前，命令会等待初始化（上限 20 秒）而不是立刻报错；初始化失败时返回具体原因
  （例如数据库损坏、迁移失败）。
- 界面由 WebView2 渲染；多显示器缩放交给 WebView2 处理，窗口状态与位置由 Tauri 管理。

## 配置与策略

设置存 SQLite，通过 Settings / Agents 页修改：

- 全局暂停（`notificationsPaused`）：暂停新的通知调度。
- `quietHours`：例如 `23-8` 表示 23:00 到次日 08:00，结束时间不包含。
- 通知冷却（`cooldownSeconds`）：0–3600 秒，0 表示不去重，默认 0；按「Agent + 会话」计算。
- 引用回复（`reply.enabled`）：默认关闭；只影响入站引用分发。
- 送达确认（`reply.confirmation`）：默认关闭。
- 路由有效期（`reply.routeTtlSeconds`）：默认 86400 秒，范围 60–604800。
- 标题包含 `🔕` 或 `[勿扰]` 时跳过推送；被停用的 Agent 直接跳过（事件仍会提交，跳过原因写入通知
  元数据与 `runtime.log`）。
- 旧的 `config.json` 与 `.off` marker 只在首次迁移时读取，之后设置以 SQLite 为准。

## 日志

`runtime.log` 是 tracing 的结构化文本日志：记录投递结果、引用回复结果、迁移检查与稳定错误码；
写入前对 `token`、`secret`、`authorization`、`cookie`、`context_token`、`body`、`message`、`prompt`、
`text` 等键做递归脱敏，超过 8 MiB 在下次启动时轮换为 `runtime.log.1`。

接入侧日志各自独立：

- 三个 Hook 只在失败时写 `*-notify-debug.log`，内容是错误码与原因（上游未找到、ingress 退出码、
  协议拒绝、超时等），不含通知正文。
- OpenCode 插件与 Command Code mod 的调试日志需要 `AGENT_NOTIFY_DEBUG=1`，记录提交目标、跳过原因与
  投递结果，同样不含正文。

## 版本与发布

- 应用版本唯一来源是仓库根 `VERSION`（2.0.0 起）；`tools/sync-version.ps1` 把它同步到 Tauri 配置、
  Cargo workspace 与 Devin 扩展，`tools/check-version.ps1` 校验一致性。
- 本地或 CI 使用 `tools/build-release.ps1` 构建桌面端、ingress、三个 Hook，生成
  `Agent-notify-Setup-vX.Y.Z.exe`、`Agent-notify-vX.Y.Z.zip` 和同时包含两者哈希的 `SHA256SUMS.txt`。
- 设置 `AGENT_NOTIFY_SIGNTOOL` 后，五个可执行文件与安装器都会签名，并在构建期校验签名者指纹等于
  客户端内置的信任指纹；Release workflow 强制要求签名 secret 存在。
- 发布与补发使用 `tools/publish-release.ps1`（编排在 `tools/release-gate.ps1`）：上传前校验安装器签名、
  `SHA256SUMS.txt` 覆盖安装器与 ZIP、以及 ZIP 内五个程序的签名指纹，任一项不符直接失败。
- 最终用户不需要安装 Rust/Go；安装器直接复制已构建的程序。

## 自动更新

Settings → 更新 的「检查更新 / 下载并安装」调用 `hosts/desktop-tauri/src/update`，分层如下：

| 模块 | 职责 | 关键约束 |
|---|---|---|
| `release.rs` | 查询 `srafyhucl-cpu/agent-notify-releases` 的 `/releases/latest`，解析版本与产物 | 只接受严格更高的 SemVer；草稿/预发布视为不稳定；API 不可用时回退 HTML 重定向（只支持 ZIP） |
| `download.rs` | 下载校验文件与产物，带重试与大小上限 | 只接受与版本同名的安装器或 ZIP；不跟随旁路下载地址 |
| `verify.rs` | 依次校验 SHA256、PE 头/位数、文件版本、Authenticode 签名者指纹 | 正式通道必须命中内置 `DEFAULT_SIGNATURE_THUMBPRINT`；篡改类状态一律拒绝 |
| `install.rs` | 拉起安装器（2 秒观察窗口）或解包 ZIP 并原地替换 | 安装器用 `/SILENT /NORESTART /LOG=<临时目录>\updates\last-update.log /DIR=<安装目录>`；ZIP 拒绝越界路径、符号链接与超限文件，替换失败回滚 |
| `service.rs` | 编排查询、下载、校验、安装；缓存最近一次检查结果 | 同一时间只允许一个安装任务；版本缓存安装前再比对一次 |

1. 安装器路径：校验通过后启动安装器，随后请求应用优雅退出（先让响应回到界面），安装器完成替换并按
   `[Run]` 段自动重新启动。
2. 安装器启动失败：回退下载 ZIP，校验 SHA256 与解包后的主程序签名，再原地替换安装目录文件
   （旧文件备份到 `updates\backup\<版本>-<随机>`），提示重启后生效。
3. 下载或校验失败不会触碰已安装文件；两条路径都继承用户当前的安装目录与配置路径，保留账号、设置、
   历史和引用路由。
4. 更新只重启 AgentNotify，不启动、关闭或重启任何 Agent。

## 不可破坏的契约

1. 安装入口名与位置：`agentnotify-desktop.exe`、`agentnotify-ingress.exe`、三个 Hook、
   `agent-notify.ts`（OpenCode 插件与 Command Code mod 同名不同目录）。
2. 所有用户可覆盖项统一使用 `AGENT_NOTIFY_*`。
3. 旧 marker 文件名 `opencode.off`、`codex.off`、`antigravity.off`、`devin.off`、`commandcode.off`
   只在首次迁移时读取；迁移后开关以 SQLite 为准。升级必须继承旧版开关（没有 marker 就是旧版开着），
   “新适配器默认关闭”只适用于全新安装。
4. ClawBot 登录、`context_token` 建立、凭据字段和发送消息结构。
5. Codex notify 透传顺序：先上游，后推送；推送失败不得影响透传。
6. 入口事件协议保持 `protocolVersion=1` 的 `agent.event` 形状与大小上限；spool 回退不得静默丢事件。
7. 发布 exe 保持 Windows GUI 子系统，Hook 与开机启动不闪控制台。
8. 引用回复只允许精确消息 ID 路由；禁止标题、正文、最近会话或跨账号回退。
9. 引用回复默认关闭；Agent 不支持 resume 时必须拒绝任务并返回可见错误。
10. Antigravity Hook 使用独立顶层键 `agent-notify`；Devin 只修改 `hooks.Stop` 中的 AgentNotify handler，
    安装与卸载不得覆盖其他 JSON 配置。
11. Antigravity 使用同目录无空格启动器调用安装目录中的 exe，避免其 Windows `cmd /c` 参数转义破坏
    带引号和空格的命令。
12. Antigravity / Devin Stop wrapper 必须始终输出 `{}`，通知故障不得阻塞 agent。
13. 发布门禁：五个可执行文件与安装器必须签名且指纹等于内置信任指纹，`SHA256SUMS.txt` 必须覆盖
    安装器与 ZIP。
14. 界面由 descriptor 与 JSON Schema 驱动，新增 Agent/渠道不得引入页面分支。
