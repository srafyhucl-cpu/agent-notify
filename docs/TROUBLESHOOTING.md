# 故障排查

先运行自检，再按症状查日志。日志不包含 ClawBot token，可以安全粘贴相关片段。

token、context token 等敏感字段不会进入日志。`AGENT_NOTIFY_CLAWBOT_DEBUG=1` 生成的协议诊断日志会保留消息 ID 和引用结构，但消息正文会替换为 `[REDACTED]`；排查完成后应删除或停用该开关。引用回复诊断也不会记录回复正文、token 或 Agent CLI 的原始 stdout/stderr。

```powershell
$exe = "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe"
Start-Process $exe -ArgumentList "status" -Wait
Start-Process $exe -ArgumentList "doctor" -Wait
```

标准安装版使用上面的默认路径；ZIP 便携或源码安装请改成实际安装目录。

## 日志位置

默认目录：`%TEMP%\agent-notify`

| 文件 | 写入方 | 用途 |
|---|---|---|
| `push.log` | CLI / 发送器 | JSON Lines 推送历史，悬浮窗也读取此文件 |
| `opencode-debug.log` | OpenCode 插件 | `AGENT_NOTIFY_DEBUG=1` 时的打开、跳过和退出信息 |
| `codex-notify-debug.log` | Codex 命令 | `AGENT_NOTIFY_CODEX_DEBUG=1` 时的参数、透传与发送信息 |
| `codex-title.log` | Codex 标题解析 | 线程 ID、标题来源、失败阶段、SQLite 错误码、重试次数和降级来源，不含通知正文 |
| `clawbot-debug.log` | ClawBot 客户端 | `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 时的发送 `client_id`、脱敏响应和解析后的关联 ID |
| `reply-debug.log` | 引用分发器 | 成功分发（引用 ID、Agent、目标会话）、路由、状态和可见错误信息，不记录回复正文 |
| `push.log` 中的 Antigravity / Devin 记录 | Hook / 发送器 | 与其他 Agent 共用推送历史和会话 ID；Hook 失败只跳过本次通知，不阻塞 Agent |
| `codex-watch.log` | 悬浮窗看护 | Codex notify 行被恢复的时间 |
| `widget-error.log` | 悬浮窗 | UI 或消息循环错误 |
| `widget-trace.log` | 悬浮窗 | 启动、窗口创建和退出追踪 |
| `widget-alive.txt` | 悬浮窗 | 心跳时间 |
| `widget-exit.txt` | 悬浮窗 | 用户主动退出标记 |
| `setup.log` | 首次接入 | PowerShell 执行 `install.ps1 -ConfigureOnly` 失败时的完整输出，用于定位插件或 Hook 配置错误 |
| `updates\last-update.log` | ZIP 兼容更新 | 旧版 ZIP 更新启动器输出；新版安装器直接启动，不写此日志 |

用户配置、登录凭据和会话上下文在 `%USERPROFILE%\.config\agent-notify`。

首次接入状态在 `%USERPROFILE%\.config\agent-notify\setup-state.json`。正常情况下它记录已成功接入的 `version` 和 `completedAt`；文件缺失、损坏或版本与当前程序不一致时，下次启动会重新接入。

## 双击安装器后 Agent 仍未接入

1. 确认安装完成页中的“启动 Agent-notify”已执行；如果窗口没有出现，从开始菜单或桌面快捷方式手动打开。
2. 首次启动会短暂执行后台接入。等待窗口出现后查看 Agent 卡片；如果顶部显示“首次接入失败”，点击悬浮窗右下角“检查修复”。
3. 查看 `%TEMP%\agent-notify\setup.log`。该文件保存最近一次失败时 `install.ps1 -ConfigureOnly` 的 PowerShell 输出；成功接入时不会删除旧日志，因此同时检查文件修改时间和 `%USERPROFILE%\.config\agent-notify\setup-state.json`。
4. 如果 `setup-state.json` 中的 `version` 不是当前版本，完全退出 Agent-notify 后重新打开，或点击“检查修复”强制重跑接入。
5. 如果安装目录缺少 `install.ps1`、插件目录不完整，或开始菜单中没有 Agent-notify，重新运行最新版 `Agent-notify-Setup-vX.Y.Z.exe` 覆盖安装；不要只手动移动 exe。
6. 如果 `setup.log` 明确提示 Hook 被占用或用户配置冲突，先按日志中的文件路径处理冲突项，再重新点击“检查修复”。程序不会覆盖无法安全接管的 Hook。

## 升级按钮下载或安装失败

1. 更新器先把 `Agent-notify-Setup-vX.Y.Z.exe` 和 `SHA256SUMS.txt` 下载到 `%TEMP%\agent-notify\updates`。下载失败时检查网络、代理以及 GitHub 与 Release 资产域名是否可访问。
2. SHA256 不匹配、校验文件缺少对应文件名或安装器不是有效 Windows 程序时，更新会终止且不会运行下载内容。删除 `%TEMP%\agent-notify\updates` 后重试，确认磁盘或代理没有篡改下载文件。
3. 校验通过后安装器以静默模式原地升级。若安装没有继续，确认 `%LOCALAPPDATA%\Programs\Agent-notify` 可写，并完全退出旧悬浮窗后重试。
4. 旧 Release 没有安装器时，更新器会回退到 ZIP，并通过隐藏 PowerShell 执行包内 `install.ps1`。这时失败详情见 `%TEMP%\agent-notify\updates\last-update.log`。
5. 升级不会改动 ClawBot 凭据、配置、历史或引用路由。不要为了更新删除 `%USERPROFILE%\.config\agent-notify`。

## 微信完全收不到

1. 确认登录和主动推送会话都已就绪：

   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe" -ArgumentList "status" -Wait
   ```

2. 未登录或提示登录失效时重新扫码：

   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe" -ArgumentList "login" -Wait
   ```

   扫码后按提示在微信中给 ClawBot 发送任意一条消息。`login` 默认会等待这条消息；也可以先使用 `login --wait=false`，再运行：

   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe" -ArgumentList "sync" -Wait
   ```

3. 若状态是“已登录 · 等待微信消息建立会话”，说明登录成功但还没有 `context_token`。保持悬浮窗运行，或运行一次 `agent-notify sync`，然后在微信中给 ClawBot 发消息。

4. 会话就绪后发送测试：

   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe" -ArgumentList "test" -Wait
   ```

5. 查看 `%TEMP%\agent-notify\push.log`。`未登录` 表示凭据缺失、损坏或登录失效；`会话未建立` 表示已登录但还没有收到微信消息；`失败` 表示网络、HTTP 或 ClawBot 业务返回错误。

## 主动推送会话未建立

- 先运行 `agent-notify status`。`loginStatus` 为“已登录”且 `sessionReady` 为 `false` 时，属于尚未收到首条微信消息。
- 在微信中给 ClawBot 发送任意文字，然后在终端运行 `agent-notify sync --timeout 10m`。
- 也可以在悬浮窗设置页保持窗口开启；后台会话循环会自动读取消息并保存上下文。
- 如果日志出现 `ret=-2 prepare failed`，说明登录仍有效但之前的主动推送上下文已被服务端拒绝；程序会清除旧上下文，给 ClawBot 发一条新消息即可恢复。
- 如果 `doctor` 显示登录失效，不要继续等待消息，先重新运行 `agent-notify login`。
- 不要手工把其他账号或其他用户的 `context_token` 放进凭据文件；不同账号的上下文会被拒绝并清除。

## 微信引用回复不生效

先确认设置里的“引用回复”已经保存为开启；该开关默认关闭。

确认 Agent-notify 悬浮窗正在运行；引用消息由悬浮窗的 ClawBot 长轮询处理，完全退出托盘程序后不会触发 Agent。

1. 只能引用 Agent-notify 自己推送的通知，并且必须是当前绑定微信用户的私聊消息。群聊、其他发送者和普通文本不会触发 Agent。
2. 打开 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 并重启悬浮窗。发送一条通知、在微信中引用它回复一句话，然后运行 `agent-notify reply-check`。
   该命令只读核对 scoped `sendmessage-result`、当前账号未过期的本地路由与 `getupdates-result`：退出码 `0` 表示引用 ID 全部精确匹配且路由可解析，`1` 表示存在无法对应的引用（不要开启），`2` 表示证据不足。
   PowerShell 脚本中建议用 `agent-notify reply-check --json | Out-String` 调用，确保等待进程结束并读取 `$LASTEXITCODE`。
   需要人工核对时仍可查看 `%TEMP%\agent-notify\clawbot-debug.log` 中的 `client_id`、`sendmessage-result` 和引用时解析出的消息 ID。
   P0 只承认当前登录账号产生的 scoped 证据：诊断记录里的 `account_scope` 必须与当前凭据一致，入站记录还必须是 `private` 且 `bound_sender`。发送证据也可来自 bot ID、用户 ID 均匹配且未过期的本地路由；升级前旧引用日志、其他账号、群聊和陌生发送者都会被忽略并计入 `ignoredSends` / `ignoredQuotes`。
   引用样本仍必须由最新悬浮窗采集；引用升级前的旧通知不会产生可用诊断，`reply-check` 会停留在“尚无发送记录/证据不足”。
3. 两者没有稳定一致的 ID 时不要继续开启；系统不会按标题、正文或最近通知猜测目标。引用结构存在但平台未返回 ID，或匹配到的 ID 无法解析到未过期的本机路由，同样属于 P0 未通过。
4. 只有通知携带稳定会话 ID 时才会建立路由：Codex 使用 `thread-id`（兼容 `thread_id`），OpenCode 使用 `sessionID`，Antigravity 使用 `conversationId`，Devin 使用 `session_id`。查看 `push.log` 的 `messageID` / `clientID`，以及 `reply-routes.jsonl` 是否存在对应记录。缺失会话 ID 时通知仍可发送，但不能引用回复。
5. 路由和入站去重 Claim 默认保留 30 天；账号重新登录后会按 bot ID 和绑定用户 ID 隔离，不会复用旧账号记录。
6. Codex：运行 `agent-notify doctor` 检查 `codex queue`。如果 Codex CLI 未安装或版本过旧，普通通知仍可使用，但 Codex 引用回复会失败并给出微信错误提示。
   命令优先读取 `AGENT_NOTIFY_CODEX_BIN`，其次搜索 PATH，最后自动扫描 `%LOCALAPPDATA%\OpenAI\Codex\bin`；开机自启时不应依赖 Codex 桌面端临时注入的 PATH。
7. Codex `queue` 可写入尚未在前台打开的持久化线程，恢复同一线程后会执行。若提示线程已归档，先运行 `codex unarchive <thread-id>`；若提示线程不存在或已删除，请确认通知中的 `thread-id`。系统不会自动回退到其他会话。
   临时会话（ephemeral）、Codex 未启用持久化队列，或检测到本地 app-server 状态冲突时同样会返回可见错误；按错误提示重启 Codex 或改用持久化会话后重试。
   如果 `codex queue` 在 30 秒内没有确认退出，错误会按“投递未确认”显示；系统不会自动重试，请先检查对应 Codex 会话是否已收到回复。
8. Antigravity：回复通过当前运行桌面端的官方 `language_server.exe agentapi` 发送，只使用通知携带的 `conversationId`。发送前会按 CSRF token 和 HTTP 监听端口定位服务，并用 `get-conversation-metadata` 确认会话存在。可用 `AGENT_NOTIFY_ANTIGRAVITY_BIN` 覆盖语言服务路径。
9. Devin：回复通过随安装器部署到 `%USERPROFILE%\.devin\extensions\agent-notify` 的扩展发送，只使用通知携带的 `session_id`。扩展必须在 Devin 重启后加载，随后直接向本窗口的 `devin.exe acp` 子进程写 `session/prompt`（旧 Cascade 会话才走 `devin.sendChatActionMessage` 聊天面板）；不依赖 Devin CLI 登录状态，也不启动第二个 Agent 进程。
10. OpenCode 必须先重新启动桌面端，使新版插件写入 5 秒心跳。心跳缺失、过旧或插件不支持 `session.prompt` / `promptAsync` 时，Agent-notify 会拒绝任务并返回错误。
11. OpenCode 超出 10 秒同步等待窗口后会按队列已接收处理；后台会在任务有效期内继续观察结果，插件之后报告的会话投递失败或最终未确认会回写微信，同时可在 `opencode-debug.log` 查看细节。
12. 插件提交会话 prompt 默认 30 秒未返回时按“状态未知”上报失败且不重试，避免重复执行；单个请求悬空不会阻塞后续回复任务，可用 `AGENT_NOTIFY_OPENCODE_REPLY_TIMEOUT_MS` 调整该超时（重启桌面端生效）。
13. 插件读取会话标题和摘要各自默认 10 秒超时：超时只退回默认标题或空摘要，推送照发，且不会把该会话永久标记为处理中；可用 `AGENT_NOTIFY_OPENCODE_FETCH_TIMEOUT_MS` 调整（重启桌面端生效）。

成功提交引用回复后不会额外发送“已收到”消息；请在对应 Agent 完成后等待下一条通知。无法关联、去重状态不可用、路由过期、命令明确失败或投递未确认时，错误会直接发回微信。

## 二维码登录失败

1. 确认窗口中已显示二维码，而不是“获取二维码失败”或“登录失败”。
2. 如果微信要求数字配对码，按窗口或终端的提示输入。
3. 二维码过期时点击“重新获取”；重新获取会取消上一轮轮询，不会叠加登录请求。
4. 如果提示节点跳转，等待客户端自动切换后继续扫码。
5. 如果显示“该微信已绑定过本机”，只有本机仍有有效旧凭据时才会复用；其余情况会自动重新获取二维码。
6. 获取二维码或轮询持续失败时检查网络、代理和 `https://ilinkai.weixin.qq.com` 是否可访问。
7. 确认凭据文件可写：`%USERPROFILE%\.config\agent-notify\clawbot.json`。该文件只保存本机登录凭据、游标和会话上下文，不写入日志或界面。

## 界面模糊、过小或点击位置偏移

- Agent-notify 使用 Per-Monitor V2 DPI 感知，窗口大小、字体和命中区域会按显示器 DPI 一起缩放。
- 本版本支持 72 到 384 DPI；如果修改 Windows 缩放后界面仍不对，请从托盘菜单退出并重新启动悬浮窗。
- 多显示器在不同缩放比例之间移动窗口时，窗口会自动重新布局，不会裁切固定区域。
- 如果截图或远程桌面里文字模糊，先确认客户端没有把远程会话再次缩放；本机原分辨率下不应出现半像素缩放。

## OpenCode 任务结束不推送

1. 设置 `AGENT_NOTIFY_DEBUG=1`，完全重启 OpenCode 桌面端。
2. 运行一个任务后查看 `%TEMP%\agent-notify\opencode-debug.log`。
3. 常见跳过原因：
   - `skip: OFF=1`：插件总开关打开。
   - `skip: marker-off`：`opencode.off` 存在，可在悬浮窗或 `toggle` 中开启。
   - `skip: cooldown`：同一会话仍在冷却窗口。
   - `skip: file-cooldown`：其他 OpenCode 实例已推送同一会话。
   - 没有 `session.execution.succeeded`：当前 OpenCode 版本可能改了事件名或插件未加载。
4. 检查插件路径是否为 `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`。
5. 点击悬浮窗“检查修复”，或重新运行最新版 `Agent-notify-Setup-vX.Y.Z.exe` 覆盖安装；然后重启 OpenCode，确保插件是当前版本。ZIP 或源码环境改用对应的 `install.ps1`。
6. 如果程序装在自定义目录，检查插件副本里的 `BAKED_BIN` 是否指向实际 exe；重新接入会刷新它，也可以用 `AGENT_NOTIFY_BIN` 临时覆盖。

CLI 还会跳过标题含 `🔕` 或 `[勿扰]` 的推送，以及 `config.json` 中 `quietHours` 覆盖的时段。

## Codex 任务结束不推送

1. 设置 `AGENT_NOTIFY_CODEX_DEBUG=1`，重启 Codex。
2. 查看 `codex-notify-debug.log`。
3. 检查 `%USERPROFILE%\.codex\config.toml` 的 notify 行是否指向：

   ```toml
   notify = [ "C:/Users/<name>/AppData/Local/Programs/Agent-notify/agent-notify.exe", "codex", "turn-ended" ]
   ```

4. 检查 `%USERPROFILE%\.config\agent-notify\codex.off` 是否存在。
5. 运行 `agent-notify watch`，或重启悬浮窗。只有 notify 行仍直指 `codex-computer-use.exe` 时，看护才会恢复 Agent-notify。
6. 如果 Codex 原本使用自定义 notify 程序，安装器不会覆盖；需要手动把自定义程序与 Agent-notify 串接。

## Antigravity 任务结束不推送

1. 运行 `agent-notify doctor`，确认 `Antigravity 接入` 没有告警。
2. 检查 `%USERPROFILE%\.gemini\config\hooks.json` 的顶层 `agent-notify.Stop`，命令应为 `.\agent-notify-hook.cmd antigravity stop`；同目录启动器应存在并指向当前安装目录中的 `agent-notify.exe`。
3. 直接运行启动器并输入测试 JSON；正常会静默输出 `{}`。如果 Antigravity 日志出现带引号 exe 无法执行的错误，说明旧版 Hook 尚未重装，重新运行当前安装器即可。
4. 检查 `%USERPROFILE%\.config\agent-notify\antigravity.off` 是否存在；marker 存在时只在 `push.log` 记录跳过。
5. Hook payload 只有在 `fullyIdle=true` 且 `conversationId` 非空时才推送；普通中途停止或缺少会话 ID 会静默跳过。
6. 摘要从 `transcriptPath` 尾部读取。transcript 不存在、仍在写入或 schema 不兼容时摘要降级为空，但通知仍会发送。
7. 安装或卸载只修改顶层 `agent-notify` Hook，不应覆盖 `hooks.json` 中其他顶层配置。

## Antigravity 引用回复不可用

1. 确认 Antigravity 桌面端仍处于运行状态，并保留着引用通知对应的会话；回复通过当前 `language_server.exe` 的 HTTP 端点发送，不依赖 `agy` CLI。
2. 如果提示找不到语言服务，检查 `%LOCALAPPDATA%\Programs\antigravity\resources\bin\language_server.exe`，或通过 `AGENT_NOTIFY_ANTIGRAVITY_BIN` 指定同时运行中的实际安装路径。
3. 语言服务的端口与 CSRF token 每次启动都会变化。令牌失效时重启 Antigravity 后重试；Agent-notify 会重新发现端点，不会缓存旧 token。
4. 如果提示会话不可用，说明 `get-conversation-metadata` 没在当前语言服务中找到该 `conversationId`；系统不会按标题、项目或最近会话回退。

## Devin 任务结束不推送

1. 运行 `agent-notify doctor`，确认 `Devin 接入` 没有告警。
2. 检查 `%APPDATA%\devin\config.json` 的 `hooks.Stop`，应有一个 matcher 为空、命令指向当前 `agent-notify.exe devin stop` 的独立 handler 组。
3. 检查 `%USERPROFILE%\.config\agent-notify\devin.off` 是否存在；marker 存在时只在 `push.log` 记录跳过。
4. `stop_hook_active=true` 的事件会被跳过，防止 hook 重入；如果 payload 包含 `hook_event_name`，它必须是 `Stop`。
5. 必须包含非空 `session_id`；缺少时不会建立引用路由。`last_assistant_message` 为空时使用默认正文。
6. 安装和卸载只增删 Agent-notify 自己的 handler，保留其他 `hooks.Stop` handler、其他事件和权限配置。

## Devin 引用回复不可用

1. 点击悬浮窗“检查修复”，或重新运行最新版 `Agent-notify-Setup-vX.Y.Z.exe` 覆盖安装；确认 `%USERPROFILE%\.devin\extensions\agent-notify` 下存在 `package.json`、`extension.js` 和 `acp-bridge.js`。ZIP 或源码环境改用对应的 `install.ps1`。
2. 完全退出并重启 Devin 桌面端。Devin 只会在启动时扫描用户扩展目录；重启后可在扩展日志中查看 `Agent-notify Devin Reply`。
3. 如果微信提示“扩展未运行”或“扩展已离线”，检查 `%USERPROFILE%\.config\agent-notify\devin-reply-inbox\heartbeats` 是否有新心跳文件。
4. 如果微信提示“未找到 Devin 桌面端 ACP 通道”，说明本窗口还没拉起常驻的 `devin.exe acp` 子进程（常见于 Devin 刚启动）；等 Devin 就绪或重启 Devin 后重试，系统不会把回复改投到新会话。
5. 如果微信提示“目标 Devin 会话不存在或已删除”，说明桌面端找不到该 Cascade；请确认引用的通知来自当前仍存在的会话，系统不会回退到最近会话。
6. 如果微信提示“Devin 桌面端尚未登记该会话”，说明 `%APPDATA%\devin\User\globalStorage\state.vscdb` 里没有该 `session_id` 的会话元数据；先在 Devin 中打开一次该会话，再引用通知回复。系统不会拿 CLI 会话号直接充当桌面端 Cascade 标识。
7. 如果微信提示“检测到多个 Devin ACP 通道”，说明当前窗口出现了多个候选子进程，系统无法确定目标；重启 Devin 后重试，系统不会改投到新会话。
8. 引用回复不读取 Devin CLI 登录状态，不需要运行 `devin auth status`，也不受工作区信任影响。回复正文不会写入诊断日志，扩展失败只返回脱敏后的错误。

## Codex 标题不正确或出现标题读取失败

运行 `agent-notify doctor`，查看 `Codex 会话标题` 检查结果。正常状态会只读解析 `%USERPROFILE%\.codex\state_*.sqlite`。

- 标题优先使用 `threads.name`，缺失时依次降级到 `threads.title`、`threads.first_user_message`、`session_index.jsonl`、notify payload 和默认标题。
- 通知末尾出现“标题读取失败”说明数据库不可读、锁超时或字段不兼容；正文仍会发送，原 `thread-id` 不会被清除，引用回复仍按该线程精确路由。
- 详细错误阶段、SQLite 错误码和重试次数见 `%TEMP%\agent-notify\codex-title.log`；该日志不记录通知正文。
- 数据库短暂锁定时程序会有限重试，不需要手工重启；持续失败时先确认 Codex 未损坏，再检查 `CODEX_HOME`。

## Codex 电脑操控失效

Agent-notify 应在发送前透传原始参数和 stdin。检查：

1. 使用的配置是 `agent-notify.exe codex turn-ended`，不是只调用 `notify`。
2. `codex-notify-debug.log` 能看到参数。
3. 最近的 Codex 版本是否改变了 `codex-computer-use.exe` 路径；`agent-notify doctor` 会检查接入。
4. 如果自定义 notify 已存在，先恢复自定义链路，再在其后追加 Agent-notify。

## 悬浮窗不见了

- 最小化按钮会隐藏窗口，任务栏按钮仍可用于恢复。
- 关闭按钮会藏入托盘，双击托盘图标可以恢复。
- 桌面快捷方式名为 `Agent-notify 悬浮窗`。
- 托盘图标可能在 Windows 11 的溢出区中，可在“任务栏设置 → 其他系统托盘图标”里固定。
- 用户主动退出会写 `widget-exit.txt`，悬浮窗不会在当前登录会话中被独立看守进程重新拉起。
- 异常退出后重新打开桌面快捷方式即可；开机自启也会在下次登录时恢复悬浮窗。

## 悬浮窗无法启动或反复退出

1. 查看 `%TEMP%\agent-notify\widget-error.log`。
2. 查看 `%TEMP%\agent-notify\widget-trace.log` 最后几行。
3. 确认 `agent-notify.exe widget` 可以手动启动。
4. 若二进制被安全软件隔离，重新解压 Release 并校验 `SHA256SUMS.txt`。
5. 若安装目录内仍有旧进程占用文件，从托盘完全退出 Agent-notify，再重新运行安装器；ZIP 或源码环境可运行对应的 `uninstall.ps1` 后重新安装。

## 开关关了仍在推送

- 确认当前用户是安装 Agent-notify 的同一 Windows 用户；配置与 marker 都按用户目录隔离。
- 运行 `agent-notify status --json`，检查 `openCodeEnabled` 与 `codexEnabled`。
- 同时检查 `antigravityEnabled` 与 `devinEnabled`。四个开关分别对应 `%USERPROFILE%\.config\agent-notify\{opencode,codex,antigravity,devin}.off`。
- OpenCode 插件与 CLI 都会检查 marker；如果只有插件未更新，重跑安装并重启 OpenCode。
- Antigravity 与 Devin 的 Hook 会继续执行但不发送通知，Markers 用于暂停推送而不是卸载 Hook。
- Codex 关闭推送时仍会透传上游程序，这是预期行为。

## 推送重复

- OpenCode 默认对同一 `sessionID` 在 10 分钟内去重。
- 修改 `cooldownMin` 后重启 OpenCode，使插件重新读取配置。
- `opencode-sent.json` 是跨实例去重状态；删掉它只会让后续事件重新建立状态，不会补发历史消息。
- 引用回复按入站 `msg_id` 去重；缺失时使用 `seq + 引用 ID + 文本哈希`。同一微信消息重复投递不会重复执行 Agent。
- 删除 `reply-state.jsonl` 只会移除本机去重历史；不要在正常运行时手工删除，否则旧消息重投可能再次执行。

## PowerShell 中的 CLI 输出

`agent-notify.exe` 是 GUI 子系统程序，避免在 hook 和开机启动时闪窗。不要用捕获表达式等待输出；对需要等待的命令使用：

```powershell
Start-Process "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe" -ArgumentList "doctor" -Wait
```

需要机器可读输出时，显式重定向：

```powershell
Start-Process "$env:LOCALAPPDATA\Programs\Agent-notify\agent-notify.exe" `
  -ArgumentList "status --json" `
  -Wait -NoNewWindow `
  -RedirectStandardOutput "$env:TEMP\agent-notify-status.json"
```

## 安装或卸载失败

- 从托盘完全退出 Agent-notify 后重试安装或升级。
- 标准安装目录必须可写；默认是 `%LOCALAPPDATA%\Programs\Agent-notify`，当前用户通常不需要管理员权限。
- 标准安装器包含已编译 exe，不需要 Go。ZIP 或源码安装缺少 `bin\agent-notify.exe` 时才会尝试调用 Go 编译，可通过 `AGENT_NOTIFY_GO` 指定 `go.exe`。
- 标准卸载请使用 Windows“设置 → 应用 → 已安装的应用”。它会删除程序文件和 Agent-notify 自己写入的 Hook、快捷方式，但保留 `%USERPROFILE%\.config\agent-notify` 中的登录凭据、配置、历史和引用路由。
- 如需完全重置，先备份需要的数据，再手动删除 `%USERPROFILE%\.config\agent-notify` 和 `%TEMP%\agent-notify`，然后重新安装。

## 提 Issue 前收集

- `%LOCALAPPDATA%\Programs\Agent-notify\agent-notify-install.json` 的版本字段，以及 `setup-state.json` 的 `version` / `completedAt`。
- `%TEMP%\agent-notify\setup.log` 和对应更新日志（如有）。
- `agent-notify doctor` 的文本输出。
- 对应日志最后 30 行。
- Windows 版本、Agent-notify 版本，以及相关 OpenCode、Codex、Antigravity 或 Devin 版本。
- Antigravity 语言服务路径或 Devin 回复扩展目录，以及对应桌面端版本。
- 已执行的排查步骤。
