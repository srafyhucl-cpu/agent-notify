# Agent-notify

<p align="center">
  <img src="https://img.shields.io/badge/version-1.2.0-blue.svg?style=flat-square" alt="Version" />
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6.svg?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/Go-1.25%2B-00ADD8.svg?style=flat-square" alt="Go" />
  <img src="https://img.shields.io/badge/License-MIT-green.svg?style=flat-square" alt="License" />
</p>

Agent-notify 是面向 OpenCode、Codex、Antigravity、Devin 与命令行长任务的 Windows 通知工具。任务完成后，它通过 ClawBot 把标题和摘要发送到微信，并在本机保留结构化推送历史。

v1.0.0 是一次彻底重构：运行时只有一个 `agent-notify.exe`，不再依赖旧脚本、旧模块、旧配置或旧环境变量，也不读取任何旧名称的别名。

## 功能

- ClawBot 扫码登录，并等待微信首条消息建立主动推送会话。
- 凭据和会话上下文只保存在本机。
- OpenCode 全局插件，监听任务完成事件并自动提取会话摘要。
- Codex `notify` 接入，保留上游 `codex-computer-use.exe` 事件透传。
- Antigravity 全局 `Stop` Hook，只在 `fullyIdle=true` 时推送，并从 transcript 尾部提取摘要。
- Devin 用户级 `Stop` Hook，读取 `last_assistant_message`，并跳过 `stop_hook_active` 防重入事件。
- Devin 推送标题从本机 `sessions.db` 按 `session_id` 精确读取；读取失败时使用默认标题并附带降级提示。
- Codex 通知使用真实会话名；OpenCode 使用插件读取的会话标题；四个 Agent 都带各自标识与本地时间页脚。
- 协议块不会出现在微信正文；正常心跳静默，异常或未知心跳保留正文并推送。
- 通知默认不截断；需要人工限制时可显式传入 `--max-chars`。
- 微信引用 Agent-notify 通知后可继续对应的 OpenCode、Codex、Antigravity 或 Devin 会话；目标只按原始平台消息 ID 和稳定会话 ID 精确匹配，不回退到最近会话。
- 通用 CLI，可在编译、测试、训练或爬虫结束后主动推送。
- 原生 Windows 悬浮窗：四个 Agent 的开关与运行状态、勿扰设置、推送历史、测试推送和托盘。
- 设置窗内置 ClawBot 扫码登录、重新登录和退出登录，不再切换到独立控制台。
- 悬浮窗、设置、登录和历史窗口均按 DPI 缩放并使用双缓冲绘制，支持多显示器 DPI 变化。
- 悬浮窗是托盘型工具窗，不占用任务栏按钮，也不进入 Alt+Tab；隐藏后从托盘图标恢复。
- JSON Lines 推送历史，区分成功、失败、未登录、会话未建立与跳过状态。
- 退出码与输出面向脚本友好；hook 调用失败不会阻塞 agent。

## 安装

从 Release 下载 `Agent-notify-v1.2.0.zip`，解压后运行：

发布包不包含任何账号凭据或绝对安装路径。安装器会在每台机器上按当前用户目录写入插件所需的实际可执行文件路径。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

默认安装结果：

| 内容 | 默认路径 |
|---|---|
| 运行程序 | `%USERPROFILE%\bin\agent-notify.exe` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |
| Devin 回复扩展 | `%USERPROFILE%\.devin\extensions\agent-notify` |
| Antigravity Hook | `%USERPROFILE%\.gemini\config\hooks.json` |
| Devin Hook | `%APPDATA%\devin\config.json` |
| 安装记录 | `%USERPROFILE%\bin\agent-notify-install.json` |

Antigravity 配置使用独立顶层 `agent-notify` Hook；Devin 只向 `hooks.Stop` 追加独立的 Agent-notify 组。安装和卸载都会保留其他顶层配置、事件和 handler，并采用同目录原子替换。

安装器只在 Codex `config.toml` 的 `notify` 行缺失或指向 `codex-computer-use.exe` 时接管，并在修改前创建 `config.toml.bak-notify-wrapper`。自定义 notify 程序不会被覆盖。

仅当 Antigravity 的 `hooks.json` 或 Devin 的 `config.json` 已存在（或其父目录已存在）时，安装器才写入对应 Hook；未安装这两个客户端的机器不会凭空创建配置目录。

自定义安装目录（例如 `-InstallDir D:\Tools\Agent-notify`）时，安装器会把该绝对路径写进插件副本的 `BAKED_BIN`，OpenCode 插件无需额外环境变量就能找到运行程序。若把 exe 手动挪到别处，需要用 `AGENT_NOTIFY_BIN` 覆盖或重跑 `install.ps1`。

也可以从源码安装：

```powershell
git clone https://github.com/srafyhucl-cpu/agent-notify.git
cd agent-notify
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

源码目录没有预编译程序时，`install.ps1` 会调用 Go 构建 `bin\agent-notify.exe`。

## 首次使用

`agent-notify.exe` 使用 Windows GUI 子系统，以避免 agent hook 调用时闪出控制台。PowerShell 中执行交互命令时使用 `Start-Process -Wait`：

```powershell
$exe = "$env:USERPROFILE\bin\agent-notify.exe"

# 扫码登录；登录后默认等待微信首条消息建立主动推送会话
Start-Process $exe -ArgumentList "login" -Wait

# 发送测试通知
Start-Process $exe -ArgumentList "test" -Wait

# 查看完整状态
Start-Process $exe -ArgumentList "status" -Wait
```

登录和建立主动推送会话是两个步骤：

1. 微信扫码确认登录 ClawBot；如果微信要求数字配对码，按窗口或终端提示输入。
2. 在微信中给 ClawBot 发送任意一条消息。
3. 收到首条消息并保存其 `context_token` 后，主动推送会话才算就绪。
4. 点击“发送测试”或运行 `agent-notify test`，确认微信能收到通知。
5. 如需继续 Agent 会话，在设置中先完成一次真实的引用 ID 对照验证，再开启“引用回复”。详细步骤见[微信引用回复](#微信引用回复)。

`login` 默认会等待首条消息。若选择 `agent-notify login --wait=false`，稍后运行 `agent-notify sync` 即可继续等待。悬浮窗运行时会自动维持会话轮询，因此扫码后在设置页保持悬浮窗运行也能完成第二步。

安装完成后也可以双击桌面上的 `Agent-notify 悬浮窗`，在“设置”里完成上述流程。悬浮窗的推荐首次流程：

1. 点击顶部连接卡或“设置”，打开设置窗。
2. 点击“扫码登录”，使用微信扫描窗口内二维码。
3. 看到“等待微信消息”后，在微信中给 ClawBot 发送一条消息。
4. 状态变为“ClawBot 已连接 · 主动推送会话已就绪”后，点击“发送测试”。
5. 最小化和关闭按钮只会隐藏悬浮窗；完全退出请右键托盘图标并选择“退出”。

## 命令行

```text
agent-notify login       微信扫码登录，默认等待首条消息建立会话
agent-notify sync        等待首条微信消息，建立主动推送会话
agent-notify logout      删除本机 ClawBot 凭据
agent-notify status      查看登录、会话、开关、路径与最近推送
agent-notify notify      发送一条通知
agent-notify test        发送测试通知
agent-notify doctor      检查配置、凭据、会话、网络与接入
agent-notify toggle      开启或暂停 OpenCode / Codex / Antigravity / Devin 推送
agent-notify watch       恢复被改回的 Codex notify 配置
agent-notify history     查看最近推送记录（`--json` 输出机器可读结果）
agent-notify widget      启动桌面悬浮窗
agent-notify version     查看版本与构建信息
```

常用参数：

```powershell
agent-notify.exe notify --title "构建完成" --summary "Release 已生成"
agent-notify.exe notify --title "数据任务完成" --no-stdin
agent-notify.exe notify --dry-run --title "预演" --summary "不会发送"
agent-notify.exe login --wait=false
agent-notify.exe sync --timeout 10m
agent-notify.exe toggle --agent all --off
agent-notify.exe toggle --agent opencode --on
agent-notify.exe toggle --agent antigravity --off
agent-notify.exe toggle --agent devin --on
agent-notify.exe status --json
agent-notify.exe notify --dry-run --title "长通知" --summary "完整正文" --max-chars 0
```

`notify` 未提供 `--summary` 且未指定 `--no-stdin` 时，会从标准输入读取摘要。发送前必须同时满足“已登录”和“主动推送会话已就绪”；仅扫码登录不会自动获得发送能力。

Codex 通知格式为 `【codex】会话名`，OpenCode 为 `【opencode】会话标题`，Antigravity 为 `【antigravity】会话标题`，Devin 为 `【devin】会话标题`（读取失败时使用各自默认标题）；非空正文后附本地时间页脚。`--max-chars 0` 表示不限长；正数会同时计入标题、正文和页脚。

## 文件与配置

| 项目 | 默认路径 |
|---|---|
| 配置目录 | `%USERPROFILE%\.config\agent-notify` |
| 配置文件 | `%USERPROFILE%\.config\agent-notify\config.json` |
| ClawBot 凭据 | `%USERPROFILE%\.config\agent-notify\clawbot.json` |
| OpenCode 开关 | `%USERPROFILE%\.config\agent-notify\opencode.off` |
| Codex 开关 | `%USERPROFILE%\.config\agent-notify\codex.off` |
| Antigravity 开关 | `%USERPROFILE%\.config\agent-notify\antigravity.off` |
| Devin 开关 | `%USERPROFILE%\.config\agent-notify\devin.off` |
| 推送历史 | `%TEMP%\agent-notify\push.log` |
| Antigravity Hook | `%USERPROFILE%\.gemini\config\hooks.json` |
| Devin Hook | `%APPDATA%\devin\config.json` |
| 引用路由 | `%USERPROFILE%\.config\agent-notify\reply-routes.jsonl` |
| 引用去重状态 | `%USERPROFILE%\.config\agent-notify\reply-state.jsonl` |
| OpenCode 回复收件箱 | `%USERPROFILE%\.config\agent-notify\opencode-reply-inbox` |
| 运行日志 | `%TEMP%\agent-notify\*.log` |
| Codex 标题诊断 | `%TEMP%\agent-notify\codex-title.log` |

`clawbot.json` 除登录 token 外，还保存账号绑定的 `context_token`、消息游标和失效标记；这些状态按 ClawBot 账号隔离，不会在切换账号时复用。文件不会写入日志或界面。

`config.json`：

```json
{
  "quietHours": "23-8",
  "cooldownMin": 10,
  "replyEnabled": false
}
```

- `quietHours` 为空表示关闭勿扰；格式为 `23-8`，结束时间不包含在静默时段内。
- `cooldownMin` 是 OpenCode 同一会话的去重窗口，默认 10 分钟，范围为 1 到 1440。
- `replyEnabled` 控制微信引用回复，默认是 `false`。开启后仍只处理当前绑定用户的私聊引用回复。
- 路由和去重 Claim 只保存在本机，默认保留 30 天；两类记录都按 ClawBot bot ID 和绑定用户 ID 隔离。
- 路由和去重文件达到大小阈值且积累足够过期或损坏记录时会原子压缩，过期记录不会被长期物理保留。

可用的 `AGENT_NOTIFY_*` 覆盖项见 [.env.example](.env.example)。所有路径和开关都统一使用 Agent-notify 命名；v1.0.0 不读取旧名称的配置、环境变量、命令别名或迁移文件。

## 微信引用回复

四个 Agent 共用同一套引用路由。该功能默认关闭；开启前必须完成一次真实的 `sendmessage` 与 `getupdates` 消息 ID 对照，确认平台返回的是稳定且一致的 ID。
P0 还要求匹配到的 ID 能解析到本机持久化路由；引用结构存在但没有消息 ID、ID 冲突以及路由缺失或过期都不会通过验收。
发送证据同时接受 scoped 调试日志中的 `sendmessage-result` 和当前账号未过期的本地路由；后者用于兼容调试开关未覆盖发送进程的情况，但仍要求引用 ID 与发送时平台消息 ID 或客户端 ID 精确相等。

启用步骤：

1. 设置 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 并重启 Agent-notify 悬浮窗。
2. 发送一条带稳定会话 ID 的通知（Codex 或 OpenCode 最便于手工触发），然后在微信中引用这条通知并回复普通文本。
3. 运行 `agent-notify reply-check`。该命令只读核对调试响应或本地路由提供的发送证据、引用 ID，以及与分发器一致的本地路由，不会改动开关或发送消息。
4. 只有输出“P0 对照通过”时才在设置中开启“引用回复”；“证据不足”时按提示补齐样本，“P0 未通过”时保持关闭。

P0 只承认当前登录账号的 scoped 证据：诊断里的 `account_scope` 必须与当前凭据一致，且引用样本必须是该用户的私聊。发送侧可来自 scoped 调试响应或 bot ID、用户 ID 均匹配的未过期本地路由；升级前的旧引用日志、其他账号、群聊和陌生发送者会被忽略并计数。引用仍必须由最新悬浮窗重新采集，不能引用升级前的旧通知充作引用样本。

需要人工复核时，同一份 `%TEMP%\agent-notify\clawbot-debug.log` 仍保留 `client_id`、脱敏后的 `sendmessage` 响应和解析出的引用 ID；`ref_msg.message_item.msg_id`、`ref_msg.msg_id`、`ref_msg.referenced_msg_id` 与顶层 `referenced_msg_id` 都会归一化成字符串后参与精确比对。发送进程未继承调试开关时，`reply-routes.jsonl` 中当前账号的未过期路由仍可作为发送证据，但不会绕过引用 ID 的精确匹配。

运行时约束：

- 引用回复由悬浮窗内的 ClawBot 会话轮询处理，使用期间需要保持 Agent-notify 运行。
- 只有当前绑定微信用户的私聊引用回复会触发 Agent。
- 只有能在本机 30 天路由中找到唯一精确消息 ID 时才会执行对应 Agent；分发器不会按标题、正文、工作目录或“最近会话”查找目标。
- Codex 执行 `codex queue --thread=<thread-id> --message=<text>`，按 `thread-id` 精确投递。
- OpenCode 通过插件本地收件箱调用已有会话的 `session.prompt`，兼容旧版 `promptAsync`。
- Antigravity 通过桌面端官方 `language_server.exe agentapi` 向原会话发送消息；端点、HTTP 端口和 CSRF token 从当前运行进程精确发现，不使用 CLI 或最近会话回退。
- Devin 先用完整 `session_id` 从 Devin 桌面端状态库解析内部 Cascade 标识，再由随 Agent-notify 安装的扩展直接向桌面端常驻的 `devin.exe acp` 子进程写 `session/prompt`（旧 Cascade 会话保留聊天面板回退）；不使用最近会话、标题或项目目录回退，也不启动第二个 Agent 进程，因此不依赖 Devin CLI 登录状态。
- `codex queue` 支持尚未在前台打开的持久化线程：消息由 Codex 写入线程队列，下次恢复同一线程时执行。已归档线程会提示先运行 `codex unarchive`；不存在或已删除的线程会明确失败。
- 临时会话（ephemeral）不支持引用续聊，Codex 未启用持久化队列或本地 app-server 状态冲突时也会返回可见错误，不会静默落到其他会话。
- `codex queue` 在 30 秒内没有确认退出时，结果按“投递未确认”处理；系统不会自动重试，并会提示先检查对应的 Codex 会话。
- Codex 命令按“`AGENT_NOTIFY_CODEX_BIN` / 测试注入 > PATH > `%LOCALAPPDATA%\OpenAI\Codex\bin` 本地安装目录”的顺序发现，避免开机自启进程没有 Codex 临时 PATH 时失联。
- 多个 ID 冲突、路由缺失或过期都会停止转发并在微信中显示错误；提交成功后不额外回复确认，Agent 下一轮完成时仍通过原有通知链路反馈。
- Codex 通知必须携带 `thread-id`（兼容 `thread_id`），Antigravity 必须携带 `conversationId`，Devin 必须携带 `session_id`，OpenCode 必须携带 `sessionID`；缺失时仍可正常推送，但不建立可回复路由。
- 重启 OpenCode 后插件才会刷新心跳；插件不支持 `session.prompt` / `promptAsync` 时会安全拒绝并返回错误。
- OpenCode 任务写入本地收件箱后，Go 侧最多等待 10 秒获取同步结果；超时后按受保护队列已接收处理，后台会在任务有效期内继续观察结果。后续失败或最终未确认会回写微信，但不会自动重试 Agent 任务。

调试日志会分别记录发送时生成的 `client_id`、脱敏后的 `sendmessage` 响应、解析后的发送结果，以及 `getupdates` 中每条消息解析出的引用 ID；`token`、`context_token`、`secret` 等字段和消息正文都会替换为 `[REDACTED]`。完成 P0 对照后应关闭 `AGENT_NOTIFY_CLAWBOT_DEBUG`。

## Codex 接入

安装器会把 Codex `config.toml` 的 notify 行写成：

```toml
notify = [ "C:/Users/<name>/bin/agent-notify.exe", "codex", "turn-ended" ]
```

调用流程：

1. 动态查找最新的 `codex-computer-use.exe` 并原样透传参数与 stdin。
2. 检查 `codex.off` marker；关闭时只跳过推送，不影响透传。
3. 从 `last-assistant-message` 提取摘要；标题按 Codex 状态库的 `threads.name → threads.title → threads.first_user_message`、`session_index.jsonl`、payload 首条消息依次降级。
4. 从 `thread-id`（兼容 `thread_id`）提取权威线程 ID；只有该 ID 存在时，发送成功后才建立 30 天引用路由。
5. 发送 `【codex】会话名`、正文和本地时间页脚并记录历史；引用回复命中已记录线程后按消息 ID 执行 `codex queue`。

悬浮窗每两分钟检查一次 Codex 配置；如果 Codex 更新后把 notify 行改回直调 `codex-computer-use.exe`，会自动恢复为 Agent-notify。

标题读取失败不会阻断通知或清除 `thread-id`：原通知末尾会显示简短降级提示，引用回复仍精确路由到原线程。详细诊断写入 `%TEMP%\agent-notify\codex-title.log`。

## Antigravity 接入

安装器在已有 `%USERPROFILE%\.gemini\config\hooks.json` 中维护独立的顶层 Hook，不修改其他 Hook 组：

```json
{
  "agent-notify": {
    "Stop": [
      {
        "type": "command",
        "command": ".\\agent-notify-hook.cmd antigravity stop",
        "timeout": 60
      }
    ]
  }
}
```

- Hook 只在 `fullyIdle=true` 且 `conversationId` 非空时发送；后台任务尚未结束时跳过，避免过早通知。
- 同目录的 `agent-notify-hook.cmd` 由安装器维护，内部再调用当前安装目录中的 `agent-notify.exe`；Hook 命令本身不包含引号，避免 Antigravity 的 Windows `cmd /c` 参数转义破坏带空格的安装路径。
- 摘要从 `transcriptPath` 指向的 JSONL 尾部按 assistant 角色提取；格式无法识别时仍发送空摘要，不阻塞 agent。
- 标题优先从 `%USERPROFILE%\.gemini\antigravity\annotations\<conversationId>.pbtxt` 精确读取；文件尚未生成或不可读时，按 `transcriptPath` 的首条用户请求降级，并在消息末尾说明降级来源。
- 命令始终输出 `{}`，解析、策略或发送失败都不会阻止 Antigravity 停止。
- 微信引用回复通过当前运行语言服务的官方 `agentapi send-message <conversationId> <text>` 发送；发送前先用 `get-conversation-metadata` 确认会话存在，不提供最近会话回退。
- 默认发现 `%LOCALAPPDATA%\Programs\antigravity\resources\bin\language_server.exe`，也可用 `AGENT_NOTIFY_ANTIGRAVITY_BIN` 指向其他安装位置。

## Devin 接入

安装器只向已有 `%APPDATA%\devin\config.json` 的 `hooks.Stop` 数组追加 Agent-notify 组；同组其他 handler、其他事件、`permissions`、`version` 等键全部保留。

- `Stop` 事件若 `stop_hook_active=true` 会直接跳过，防止 Hook 递归。
- 摘要取 `last_assistant_message`，会话目标取稳定字段 `session_id`。
- 安装器把回复扩展部署到 `%USERPROFILE%\.devin\extensions\agent-notify`，重启 Devin 后加载。
- Go 侧先用 `session_id` 从 `%APPDATA%\devin\User\globalStorage\state.vscdb` 读出桌面端 Cascade 标识 `acp/devin-cli/<session_id>`，作业随正文一起下发给扩展；查不到登记记录时直接报错，不用 CLI 会话号或最近会话顶替。
- 扩展轮询本地收件箱：ACP 会话先尽力把目标会话切到聊天面板，再直接向本窗口的 `devin.exe acp` 子进程写 `session/prompt` NDJSON，把消息续写到原 Cascade；旧 Cascade 会话仍用 `openCascadeIdInChatPanel` 与 `sendCascadeInput`。不按项目目录或最近会话选择目标。
- 消息由 Devin 桌面端自身处理，不会另外启动 Agent 进程；本轮完成时仍由 Devin `Stop` Hook 推送结果。
- 微信回复不读取 CLI 登录状态，也不需要 `devin auth status`，不写入或等待会话锁，也不要求工作区信任；扩展在本窗口存在 ACP 通道（或桌面端提供聊天动作）时报告就绪，通道缺失、存在多个候选或写入失败都会给出微信可读错误，不会把回复改投到新会话。
- Hook 始终输出 `{}`，通知失败不会改变 Devin 的停止决策。

## 卸载

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\uninstall.ps1
```

卸载器按 `agent-notify-install.json` 清理程序与插件，尝试恢复 Codex 配置，并只移除 Agent-notify 自己的 Antigravity / Devin Hook。其他 Hook 和用户配置保持不变；登录凭据和 Agent-notify 配置默认保留。如需彻底删除，请手动删除 `%USERPROFILE%\.config\agent-notify`。
Devin 回复扩展仅在 `package.json` 的 `name` 和 `publisher` 均属于 Agent-notify 时删除；目录中若还有其他文件会保留。

## 开发

要求：

- Windows 10 / 11
- Go 1.25+
- Node.js 20+，仅用于 OpenCode 插件类型检查
- Windows PowerShell 5.1+，用于安装器和 smoke 测试

```powershell
# 依赖缓存请留在当前项目盘，不要指向 C 盘
$env:npm_config_cache = 'D:\Temp\npm-cache'
npm ci

# Go 单测
$env:GOPATH = 'D:\Temp\agent-notify-go'
$env:GOMODCACHE = 'D:\Temp\agent-notify-go\pkg\mod'
$env:GOCACHE = 'D:\Temp\agent-notify-go\build'
go test ./...

# 插件类型检查
node_modules\.bin\tsc.cmd --noEmit

# PowerShell 静态检查与完整 smoke
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

`tools\test.ps1` 会运行 Go 单测、TypeScript 类型检查和隔离沙箱 smoke；smoke 不联网，不访问真实用户配置。

如需验证本机 Codex CLI 的真实 `queue` 入队路径，请只在隔离的 `CODEX_HOME` 和测试线程上运行：

```powershell
$env:CODEX_HOME = 'D:\Temp\codex-probe'
$env:AGENT_NOTIFY_CODEX_INTEGRATION_THREAD = '<isolated-thread-id>'
$env:AGENT_NOTIFY_CODEX_INTEGRATION_BIN = 'codex'
go test -count=1 -run TestCodexQueueRunnerRealCLIIntegration -v ./internal/reply
```

该测试会向指定测试线程写入一条探针消息，默认未设置环境变量时自动跳过。

## 文档

- [架构说明](docs/ARCHITECTURE.md)
- [故障排查](docs/TROUBLESHOOTING.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [更新日志](CHANGELOG.md)

## License

[MIT License](LICENSE) © 2026 Agent-notify contributors
