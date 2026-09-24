# 故障排查

2.0.0 的正式入口是 Tauri 桌面端 `agentnotify-desktop.exe`（工作台主窗口）和内部事件入口
`agentnotify-ingress.exe`；旧的 Win32 悬浮窗、管理 CLI（`status` / `doctor` / `notify` / `sync` /
`history` 等）和 Go 版 `agent-notify.exe` 都不再发布。故障排查通过桌面端界面与日志完成；界面打不开或
在无头环境时，可以用 `agentnotify-ingress.exe --doctor` / `--ping` 做只读探活（见下文）。

排查从三处开始：主窗口的 **Diagnostics / Agents / Channels** 页、应用运行日志、各 Agent 接入自己的
失败日志。日志不会写入 ClawBot token、凭据、回复正文或通知正文，但仍可能包含用户名、本机路径、账号提示、
客户端标识和平台消息标识；只附必要行，并在提交公开 Issue 前移除个人信息。

## 日志与数据位置

| 文件 / 目录 | 写入方 | 用途 |
|---|---|---|
| `%LOCALAPPDATA%\AgentNotify\data\state.db` | 桌面端运行时 | SQLite WAL 状态库：设置、渠道账号、通知历史、投递、引用路由与入站去重 |
| `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` | 桌面端运行时 | 统一运行日志（投递结果、引用回复结果、迁移检查、错误码）；超过 8 MiB 轮换为 `runtime.log.1` |
| `%LOCALAPPDATA%\AgentNotify\data\legacy-import-report.json` | 首次启动迁移 | 旧数据只读导入报告（导入条数、跳过与损坏警告） |
| `%LOCALAPPDATA%\AgentNotify\spool` | `agentnotify-ingress.exe` | 核心离线时的持久事件队列；无效事件移入 `spool\quarantine` 并附 `.error` 原因 |
| `%LOCALAPPDATA%\AgentNotify\temp\updates` | 一键升级 | 更新包下载与解压目录；安装器日志 `last-update.log`；被替换的旧文件备份在 `backup\` |
| `%TEMP%\agent-notify\codex-notify-debug.log` | Codex Hook | 仅失败时写：上游透传失败或事件提交失败的原因（含退出码）；超过 512 KiB 清空重写 |
| `%TEMP%\agent-notify\antigravity-notify-debug.log` | Antigravity Hook | 仅失败时写：ingress 提交失败的原因 |
| `%TEMP%\agent-notify\devin-notify-debug.log` | Devin Hook | 仅失败时写：ingress 提交失败的原因 |
| `%TEMP%\agent-notify\opencode-debug.log` | OpenCode V2 插件 | 需要 `AGENT_NOTIFY_DEBUG=1`（重启 OpenCode 生效）：打开、跳过与投递结果 |
| `%TEMP%\agent-notify\commandcode-debug.log` | Command Code V2 mod | 需要 `AGENT_NOTIFY_DEBUG=1`：事件提交、心跳与回复窗口判定 |

旧版的 `%USERPROFILE%\.config\agent-notify`（`config.json`、`clawbot.json`、`.off` 开关、`push.log`、
`reply-routes.jsonl`、`reply-state.jsonl`）在 2.0 只作为**只读迁移来源**：首次启动导入 SQLite 后不再被读写，
程序也不会改写或删除这些文件。ClawBot 登录凭据改由 Windows 凭据管理器保存。

## 无头环境探活：ingress 只读自检

界面打不开、远程排障或写脚本时，先跑 `agentnotify-ingress.exe` 的只读自检（不提交事件、不写盘）：

```powershell
$app = "$env:LOCALAPPDATA\Programs\Agent-notify"
& "$app\agentnotify-ingress.exe" --ping    # 一行结论，退出码 0 正常 / 1 异常
$report = & "$app\agentnotify-ingress.exe" --doctor | ConvertFrom-Json
$report.ok                # 总判定
$report.pipe.listening    # 桌面端是否在监听事件管道
$report.spool.pending     # 待补投事件数（> 0 = 核心离线积压）
$report.spool.quarantined # 隔离事件数（附 .error 原因）
```

`ok=true` 的口径是「命名管道在监听 + spool 无积压无错误」。`pipe.listening=false` 多半是桌面端没在运行；
`pending` 持续增长说明事件在积压、核心没消费，配合 `runtime.log` 看启动错误。

## 双击安装器后 Agent 仍未接入

1. 安装完成页会启动 AgentNotify；打开 **Agents 页**，每个 Agent 卡片显示接入健康状态和可执行的原因，
   例如「Codex 未接入 AgentNotify Hook，请运行安装器」「未检测到 Command Code mod，请运行安装器并重启 Command Code」。
2. 安装器执行接入是**尽力而为**：单个 Agent 接入失败不会让整个安装失败，结束时统一提示失败清单。
   需要重试时手动运行安装目录下的接入脚本（用绝对路径指向本次安装目录）：

   ```powershell
   $app = "$env:LOCALAPPDATA\Programs\Agent-notify"
   powershell -NoProfile -ExecutionPolicy Bypass -File "$app\tools\hooks\install-opencode-v2.ps1" `
     -Source "$app\plugin\agent-notify.ts" `
     -Destination "$env:USERPROFILE\.config\opencode\plugins\agent-notify.ts" `
     -Ingress "$app\agentnotify-ingress.exe"
   powershell -NoProfile -ExecutionPolicy Bypass -File "$app\tools\hooks\install-codex-v2.ps1" `
     -HookPath "$app\agentnotify-codex-hook.exe" -Ingress "$app\agentnotify-ingress.exe"
   powershell -NoProfile -ExecutionPolicy Bypass -File "$app\tools\hooks\install-antigravity-v2.ps1" `
     -HookPath "$app\agentnotify-antigravity-hook.exe" -Ingress "$app\agentnotify-ingress.exe"
   powershell -NoProfile -ExecutionPolicy Bypass -File "$app\tools\hooks\install-devin-v2.ps1" `
     -HookPath "$app\agentnotify-devin-hook.exe" -ExtensionSource "$app\plugin\devin-extension-v2" `
     -Ingress "$app\agentnotify-ingress.exe"
   powershell -NoProfile -ExecutionPolicy Bypass -File "$app\tools\hooks\install-commandcode-v2.ps1" `
     -Source "$app\plugin\commandcode-v2\agent-notify.ts" -Ingress "$app\agentnotify-ingress.exe"
   ```

3. 接入脚本只改写 AgentNotify 自己写入的配置项（Codex 的 `notify` 行、Antigravity 的顶层
   `agent-notify` 键与启动器、Devin 的 `hooks.Stop` handler、Command Code 的 mod）；已有第三方配置时
   脚本会明确报错而不是覆盖，按报错里的文件路径先处理冲突。
4. 自定义安装目录不需要额外设置：安装器和接入脚本都会写入绝对路径。不要手工只移动 exe。
5. 桌面端界面需要 Microsoft Edge **WebView2 运行时**；安装器会检查，缺失时提示先安装。静默安装（一键升级）
   遇到 WebView2 缺失会直接中止，并把原因写进升级日志。

## 升级失败与回滚

「检查更新」查询二进制仓库 `srafyhucl-cpu/agent-notify-releases` 的 `/releases/latest`；「下载并安装」会：

1. 把 `SHA256SUMS.txt` 和 `Agent-notify-Setup-vX.Y.Z.exe`（Release 没有安装器时退回
   `Agent-notify-vX.Y.Z.zip`）下载到 `%LOCALAPPDATA%\AgentNotify\temp\updates\<版本>`；
2. 依次校验 SHA256、PE 文件与文件版本号、Authenticode 签名者指纹；正式通道必须命中内置指纹
   `EDF9E283DF2407B318E65D59BB430FD546509ACD`；
3. 校验通过才以 `/SILENT /NORESTART /LOG=<应用临时目录>\updates\last-update.log /DIR=<安装目录>`
   拉起安装器；安装器启动成功后应用自动退出，安装结束由安装器自动重新启动。

排查顺序：

- **下载失败**：检查网络与代理是否可访问 GitHub API/下载域名。GitHub API 被限流时更新器会退回 HTML
  重定向路径，该路径只支持 ZIP 产物。失败原因显示在 Settings → 更新 的界面提示里。
- **旧版（Go 1.17）升级报 `下载校验文件失败：HTTP 403 … API rate limit exceeded`**：旧版更新器用
  GitHub API 的资产地址下载文件（每次升级约 3 次调用），匿名配额只有 60 次/小时，同 IP 的其它匿名请求
  也会占用，被限流时就失败。等配额重置（响应文本或 `https://api.github.com/rate_limit` 会给出时间，
  最长 1 小时）后重试即可。2.0.x 客户端不受影响：它只花 1 次 API 调用查版本，资产走
  `github.com/<repo>/releases/download/<tag>/` 直链。
- **2.0.0–2.0.4 报 `更新包不是 64 位 Windows 可执行文件`**：这些版本的更新器要求更新包必须是 64 位 PE，
  而 Inno Setup 的安装器存根是 32 位 PE，属于误拒；2.0.5 起已修复（安装器通道接受 32 位 PE，应用程序
  本体仍要求 64 位）。这些版本无法应用内升级到 2.0.5：请从 Release 页面手动下载
  `Agent-notify-Setup-v2.0.5.exe` 安装一次（会原地覆盖，数据库、凭据与配置都保留）。
  旧版（Go 1.17）之所以没有这个问题：它的安装器分支只校验 SHA256、文件头 `MZ` 与签名指纹，**从不检查
  位宽**；Rust 重写新增了 PE 结构与文件版本校验，但把 64 位要求误加到了安装器上。
- **校验失败**：提示会写明原因。`更新包校验失败，下载内容与发布清单不一致` 表示 SHA256 不符；
  `更新包不是有效的 Windows 可执行文件` 表示 PE 头不完整或不是 64 位程序；`更新包版本不匹配` 表示文件
  版本号与 Release 标签不一致。
- **签名被拒**：`更新包未签名…拒绝安装` / `更新包签名者不匹配：实际 …，不在信任列表中` /
  `更新包签名状态异常（HashMismatch），拒绝安装` / `更新包签名无效（…），拒绝安装`。
  这些都说明下载到的不是官方发布包（或发布流程异常）。**不要手动运行这个安装包**：删除
  `%LOCALAPPDATA%\AgentNotify\temp\updates` 后重试；仍失败就从发布仓库手动下载，用 Release 附件的
  `SHA256SUMS.txt` 核对后再安装。
- **安装没有继续**：查看 `%LOCALAPPDATA%\AgentNotify\temp\updates\last-update.log`。常见原因有
  WebView2 缺失（静默安装中止）、安装目录不可写、安装器在 2 秒观察窗口内非零退出（界面会给出退出码和
  日志路径）。先完全退出 AgentNotify 再重试。
- **安装器启动失败的回退**：更新器会改下载 ZIP，校验后原地替换安装目录文件（旧文件备份到
  `updates\backup\<版本>-<随机>`），提示「更新文件已就绪，重启 AgentNotify 后生效」；替换过程任何一步
  失败都会回滚到替换前状态。
- **回滚**：手动安装上一稳定版安装器（例如 `Agent-notify-Setup-v1.17.0.exe`）即可回到旧版；安装器
  AppId 不变，会覆盖回原安装目录，SQLite 状态库、凭据与旧配置都不会被删除。
- **数据安全**：下载或校验失败不会触碰已安装文件；升级也不会改动账号、设置、历史或引用路由，
  不要为了升级删除 `%LOCALAPPDATA%\AgentNotify`。
- **手动回滚**：确认下载包摘要和签名者后，运行上一稳定版安装器覆盖安装。AppId 不变，原程序目录会被覆盖，
  但 SQLite、凭据、旧配置和 OpenCode 接入不会被删除；回到 2.0 后可在 Agents/Channels 页检查开关和账号状态。

## 卸载后仍无法登录或接入

普通卸载只删除程序和 AgentNotify 自己管理的 Codex / Antigravity / Devin / Command Code 接入，以下内容会保留：

- `%LOCALAPPDATA%\AgentNotify`：SQLite、日志、spool 和更新备份；
- Windows 凭据管理器中以 `AgentNotify/` 开头的通用凭据；
- `%USERPROFILE%\.config\agent-notify`：旧版只读迁移来源；
- `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`：OpenCode 插件。

需要彻底清理时，先在 Channels 页对每个账号点「退出账号」，再卸载；随后按 README 的步骤删除上述残留，
并在「控制面板 → 凭据管理器 → Windows 凭据 → 通用凭据」中删除 `AgentNotify/` 条目。退出账号不会删除
SQLite 中的历史记录，只有删除 `%LOCALAPPDATA%\AgentNotify` 才会清掉本机历史和设置。

## 微信完全收不到

1. 打开 **Channels 页**，确认账号状态：
   - 未登录或登录失效（显示「ClawBot 登录状态已失效，请重新扫码」）时重新扫码；
   - 显示「ClawBot 会话已失效，请重新扫码或发送消息恢复」时，先给 ClawBot 发一条微信消息。
2. 重新登录步骤：Channels 页重新登录 → 扫码，遇到「需要配对码」时输入微信提示的数字配对码 →
   看到「等待首条入站消息」后，在微信中给 ClawBot 发送任意一条消息 → 状态变成「配对已完成」才算建立
   主动推送会话。扫码成功本身不代表可以主动发送。
3. 会话就绪后用 Channels 页的**测试发送**发一条通知，确认微信能收到；测试发送用的是随机生成的测试
   会话号（`test-session-<随机>`），引用它不会连到任何真实 Agent 会话，不能用它验证引用回复。
4. 仍收不到时看 `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` 的投递记录，错误都是可读中文，例如
   「ClawBot 登录凭据缺失，请重新扫码」「ClawBot 登录状态已失效，请重新扫码」
   「ClawBot 主动推送会话已失效，请先给 ClawBot 发送一条消息」「平台未能准备会话，请给 ClawBot
   发送一条消息后重试」。
5. 如果日志出现 `reason="prepare_failed"`（平台拒绝准备会话 PrepareFailed），说明登录仍有效，但旧的
   主动推送上下文已被拒绝；程序会清除旧上下文并标记为未建立，给 ClawBot 发一条新消息即可恢复。
   日志出现 `reason="invalid_account"` 表示登录失效，需要重新扫码。
6. 不要在多个账号之间手工搬运凭据；账号状态由桌面端按 bot ID 和绑定用户隔离，过期的上下文会被拒绝并清除。

## 主动推送会话未建立

- 刚登录、从未建立过会话属于正常等待：Channels 页显示「等待首条入站消息」，此时不需要重装。
- 曾经正常推送过、之后失效属于故障：Channels 页给出「ClawBot 会话已失效，请重新扫码或发送消息恢复」，
  按提示在微信中给 ClawBot 发送任意文字即可恢复，不需要重启应用。
- 失效状态会持续显示到恢复为止；恢复后再次断开才会重新记录一次错误。
- 不要手工把其他账号或其他用户的 `context_token` 放进任何文件：不同账号的上下文会被平台拒绝并清除。

## 二维码登录失败

1. 确认登录窗口里显示的是二维码，而不是「获取二维码失败」或「登录失败」。
2. 微信要求数字配对码时，在窗口里输入后提交；提交后可继续等待首条入站消息。
3. 二维码过期时在登录窗口点「刷新二维码」，刷新会取消上一轮轮询，不会叠加登录请求。
4. 提示节点跳转时等待客户端自动切换后继续扫码。
5. 「该微信已绑定过本机」只有在本机仍有有效旧凭据时才会复用，其余情况会重新获取二维码。
6. 持续失败时检查网络与代理，以及 `https://ilinkai.weixin.qq.com` 是否可访问。
7. 登录会话只保存在内存中：关闭登录窗口不会取消登录任务，但进程退出后需要重新开始。

## 微信引用回复不生效

先确认 **Settings → 回复** 里的「引用回复」已保存为开启；该开关默认关闭，「送达确认」默认也关闭。
引用回复由桌面端运行时处理：完全退出 AgentNotify 后不会触发任何 Agent。

1. 只能引用 AgentNotify 自己推送的通知，并且必须是当前绑定微信用户的私聊消息；群聊、其他发送者或
   普通文本会被忽略并记入诊断，不会报错打扰对方。
2. 目标只按引用消息携带的原始平台消息 ID 在本机路由里精确匹配；没有对应路由、ID 冲突或路由过期都会
   回微信可读原因（例如「无法续聊：无可用会话记录，这条通知可能已超过有效期或未建立引用关联。」），
   不会按标题、正文或「最近会话」猜测目标。
3. 只有通知携带稳定会话 ID 时才会建立路由：Codex 用 `thread-id`（兼容 `thread_id`），OpenCode 用
   `sessionID`，Antigravity 用 `conversationId`，Devin 用 `session_id`，Command Code 用 `sessionId`。
   缺失会话 ID 时通知仍会发送，但不能引用回复。
4. 路由有效期在 Settings → 回复 里配置，默认 24 小时（60–604800 秒）；账号重新登录后会按 bot ID 和
   绑定用户隔离，不会复用旧账号记录。
5. 同一条引用消息至多投递一次：命中过的消息不会重复执行 Agent；投递失败或拒绝会回一条可读原因。
   在 History 页可以查看对应通知和投递详情，`runtime.log` 里有路由与 Claim 的结果码。

各 Agent 的常见错误见下一节。

## 引用回复发送失败的常见原因（按 Agent）

### Codex

- 运行 `codex queue` 投递，超时 30 秒；超时按「投递未确认」显示，不会自动重试，请先检查目标 Codex
  会话是否已收到回复。
- 「目标 Codex 线程已归档」：先恢复（解档），或运行 `codex unarchive <thread-id>` 后重新引用回复。
- 「目标 Codex 线程不存在或已删除」：确认通知里的 `thread-id`；系统不会回退到其他会话。
- 「临时会话不支持引用续聊」、「当前 Codex 未启用可持久化的消息队列」、「Codex 本地服务状态冲突」：
  按提示重启 Codex 或改用持久化会话后重试。
- 找不到 Codex CLI 时普通通知仍可用，但引用回复会明确失败；可用 `AGENT_NOTIFY_CODEX_BIN` 指定
  `codex.exe`。

### OpenCode

- 回复投递到本地 `%USERPROFILE%\.config\agent-notify\opencode-reply-inbox`，由 V2 插件认领。
- 「OpenCode 插件未连接，请启动 OpenCode 后重试」：插件进程不在或心跳超过 30 秒未更新；
  重启 OpenCode 桌面端让新版插件加载。
- 插件不支持会话投递时拒绝任务并返回错误；插件提交默认 30 秒未确认时按「状态未知」上报且不重试，
  避免重复执行。
- 单条任务有效期 10 分钟，过期或处理中断只报告失败，不会自动重放。

### Antigravity

- 回复通过当前运行的 Antigravity 官方 `language_server.exe agentapi` 发送，只使用通知携带的
  `conversationId`，发送前先用 `get-conversation-metadata` 确认会话存在。
- 「未找到正在运行的 Antigravity 语言服务」：确认桌面端已打开；可用 `AGENT_NOTIFY_ANTIGRAVITY_BIN`
  指定实际安装路径。
- 「语言服务令牌已失效」：语言服务端口与 CSRF token 每次启动都会变化，重启 Antigravity 后重试；
  程序不会缓存旧 token。
- 「会话当前不可用」：`conversationId` 不在当前语言服务里；不会按标题、项目或最近会话回退。

### Devin

- 回复通过 `%USERPROFILE%\.devin\extensions\agent-notify-reply-v2` 扩展发送，只使用通知携带的
  `session_id`，再由适配器换成桌面端 Cascade 标识；不依赖 Devin CLI 登录状态。
- 「Devin 引用回复扩展未运行 / 已离线」：完全退出并重启 Devin 桌面端；Devin 只在启动时扫描用户扩展目录。
- 「当前 Devin 桌面端未提供精确回复能力」：桌面端版本过旧，更新后重启。
- 「目标 Devin 会话不存在或已删除」、「Devin 桌面端尚未登记该会话」：先在 Devin 中打开一次该会话，
  再引用通知回复；系统不会拿 CLI 会话号或最近会话代替。
- 「Devin 桌面端登录状态已失效」「目标工作区在 Devin 中未受信任」「目标会话正被 Devin 占用」：
  按提示在 Devin 里处理后再重试。
- 接入是否正常可看 Agents 页的 Devin 健康状态；扩展目录缺失或配置未接入时会给出「请运行安装器」。

### Command Code

见下一节的回复窗口说明；核心错误是「回复窗口已过」「目标会话未在运行」「未检测到 Command Code 的
AgentNotify mod」。

## Command Code 引用回复窗口（实验性，默认关闭）

Command Code 的 mod 只能把回复投递到**正在运行的 run**里，所以每次回答后需要一段「回复窗口」。
窗口秒数在 **Agents 页 → Command Code 配置** 里设置（`commandCodeReplyWindowSec`，0 = 关闭，
范围 1–600），保存后应用会把界面值原子写入 mod 读取的
`%USERPROFILE%\.config\agent-notify\commandcode-reply-inbox\window.json`，**下次任务生效，无需重启**。

**开启后出现「界面卡住 / 手打的消息只进队列不被处理」**：这是窗口的固有副作用——它会在 `onStop` 里
挂住还没结束的那一轮。把窗口秒数设为 **0** 即可恢复，或在当前会话执行 `/reload`。

1. 引用回复本身还需要 **Settings → 回复 → 引用回复** 为开启，且该 Agent 未被停用。
2. 通知页脚会写明时限（`*引用此消息可继续对话（60 秒内）*`）。**在这个时间内**引用回复才有效；
   窗口期只是等待，不消耗 token。
3. 窗口开着时**每次回答都会多停最多 N 秒**，这是设计取舍；不需要引用回复时设为 0。
4. 各报错的含义：
   - 「Command Code 回复窗口已过」「回复窗口未开启」：窗口没开或已结束，等下一次通知再引用；
     回复不会留到下一次运行。
   - 「Command Code 目标会话未在运行，请先打开该会话」：目标 sessionId 没有新鲜心跳（5 秒一次，
     30 秒过期），通常是会话已关闭或 mod 未加载。
   - 「未检测到 Command Code 的 AgentNotify mod」：`%USERPROFILE%\.commandcode\mods\agent-notify.ts`
     缺失或未加载，运行安装器并重启 Command Code。
   - 「Command Code mod 心跳无效」：回复收件箱目录权限异常，检查后重启 Command Code。
5. 环境变量 `AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC`（大于 0 才生效）优先级高于界面值；旧
   `config.json` 里的 `commandCodeReplyWindowSec` 只在界面未设置时兜底。

## OpenCode 任务结束不推送

1. 确认 Agents 页里 OpenCode 的通知开关是「已启用」（旧的 `opencode.off` 文件只在首次迁移时导入为
   停用状态，之后不再实时读取）。
2. 设置 `AGENT_NOTIFY_DEBUG=1`，完全重启 OpenCode 桌面端。
3. 运行一个任务后查看 `%TEMP%\agent-notify\opencode-debug.log`，常见跳过原因：
   - `skip idle after failure`：同一轮失败后 2 秒内的 `session.idle` 被抑制，避免重复推送；
   - `skip terminal without body`：没有可发送的正文；
   - `skip duplicate terminal`：同一会话的同一终端事件被重复投递；
   - 完全没有 `terminal submitted` 记录：当前 OpenCode 版本可能改了事件名，或插件未加载。
4. Agent 开关、勿扰时段、标题 `🔕` / `[勿扰]` 与冷却都在桌面端运行时判定（事件仍会提交，结果记录在
   `runtime.log`，History 里该通知显示为「已跳过」），所以插件日志正常时也要检查这两处。
5. 检查插件路径是否为 `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`，且文件里绑定了当前
   安装目录的 `agentnotify-ingress.exe` 绝对路径。重新登录/重装后需要重启 OpenCode 让插件重新加载。

## Codex 任务结束不推送

1. 确认 Agents 页里 Codex 的状态没有「未接入 AgentNotify Hook」告警。
2. 检查 `%USERPROFILE%\.codex\config.toml` 的 `notify` 行是否指向 Hook：

   ```toml
   notify = [ "C:/Users/<name>/AppData/Local/Programs/Agent-notify/agentnotify-codex-hook.exe", "codex", "turn-ended" ]
   ```

   原本的 `codex-computer-use.exe` 或第三方 notify 会被保留在链上（`--previous-notify`），
   安装/接入脚本不会直接删掉它们。
3. 查看 `%TEMP%\agent-notify\codex-notify-debug.log`：Hook 失败只写这个文件（含上游透传失败原因与
   ingress 退出码），Codex 本身照常收到 `{}` 继续语义。
4. `notify` 是多行 TOML 数组时接入脚本会拒绝改写并报错，先把 notify 合并为单行后重试。
5. 如果 Codex 原本使用自定义 notify 程序，安装器不会覆盖；需要手动把自定义程序与 AgentNotify 串接。

## Antigravity 任务结束不推送

1. Agents 页确认 Antigravity 接入健康；提示「未接入 AgentNotify Hook」时重跑安装器或接入脚本。
2. 检查 `%USERPROFILE%\.gemini\config\hooks.json` 的顶层 `agent-notify` 键，命令应为
   `.\agent-notify-hook.cmd antigravity stop`；同目录启动器
   `%USERPROFILE%\.gemini\config\agent-notify-hook.cmd` 应存在并指向当前安装目录的
   `agentnotify-antigravity-hook.exe`。
3. 直接运行启动器并输入测试 JSON；正常会静默输出 `{}`。
4. Hook payload 只有在 `fullyIdle=true` 且 `conversationId` 非空时才提交；普通中途停止或缺少会话 ID
   会静默跳过。
5. 摘要从 `transcriptPath` 尾部读取，读取失败会降级为空摘要并照常发送；会话标题优先读
   `%USERPROFILE%\.gemini\antigravity\annotations\<conversationId>.pbtxt`，文件未生成时降级到 transcript
   首条用户请求，再不行用默认标题（正文里会有降级提示）。
6. 接入或卸载只维护顶层 `agent-notify` 键，不会覆盖 `hooks.json` 中其他配置。

## Devin 任务结束不推送

1. Agents 页确认 Devin 接入健康（hook 未接入会明确提示）。
2. 检查 `%APPDATA%\devin\config.json` 的 `hooks.Stop`，应有指向当前安装目录
   `agentnotify-devin-hook.exe devin stop` 的 handler 组。
3. `stop_hook_active=true` 的事件会被跳过，防止 hook 重入；如果 payload 包含 `hook_event_name`，
   它必须是 `Stop`。
4. 必须包含非空 `session_id`；缺少时不会建立引用路由。`last_assistant_message` 为空时使用默认正文。
5. 失败原因（ingress 提交失败等）写入 `%TEMP%\agent-notify\devin-notify-debug.log`。
6. 安装和卸载只增删 AgentNotify 自己的 handler，保留其他 `hooks.Stop` handler、其他事件和权限配置。

## Codex 标题不正确或出现「标题读取失败」

- 标题优先使用 `threads.name`，缺失时依次降级到 `threads.title`、`threads.first_user_message`、
  `session_index.jsonl`、notify payload 和默认标题；正常状态只读解析
  `%USERPROFILE%\.codex\state_*.sqlite`。
- 通知正文出现「标题读取失败：…」说明状态库不可读、锁超时或字段不兼容；正文仍会发送，原
  `thread-id` 不会被清除，引用回复仍按该线程精确路由。
- 短暂锁定由程序内部处理，不需要手工重启；持续失败时先确认 Codex 未损坏，再检查 `CODEX_HOME` 与
  Agents 页里配置的 `codexHome`。

## Codex 电脑操控失效

AgentNotify 应在发送前透传原始参数和 stdin 给上游。检查：

1. `config.toml` 的 notify 仍是 Hook 调用（`agentnotify-codex-hook.exe "codex" "turn-ended"`），
   自定义 notify 通过 `--previous-notify` 原样保留在链上。
2. `%TEMP%\agent-notify\codex-notify-debug.log` 能看到上游透传失败的原因（未找到
   `codex-computer-use.exe`、启动失败、超时等）。
3. 最近的 Codex 版本是否改变了 `codex-computer-use.exe` 的位置：Hook 会扫描
   `%LOCALAPPDATA%\OpenAI\Codex\runtimes\cua_node` 下最新的运行时；找不到时不会猜测路径，
   只写诊断并继续提交通知。
4. 上游超时上限 30 秒；超时只记录诊断，不会改变 Codex 的退出语义。

## 推送重复

- 通知冷却在 Settings 里配置（0–3600 秒，0 = 不去重，默认 0）；同一 Agent、同一会话的重复完成
  事件会在冷却窗口内跳过。改动保存后立即生效。
- 入站引用回复按渠道消息 ID 持久化去重（缺失时使用序号 + 引用 ID + 文本哈希），同一条微信消息重复
  投递不会重复执行 Agent。
- 重复推送通常来自上游重复触发（同一会话连续触发完成事件）。可以先用冷却止血，再检查对应 Agent 的
  客户端是否重复上报。
- 去重状态存 SQLite（`state.db`），不要手工删除文件来「重置」。

## 开关关了仍在推送

- 确认当前用户是运行 AgentNotify 的同一 Windows 用户；账号、设置与数据都按用户目录隔离。
- 在 **Agents 页**逐个检查 Agent 的通知开关（停用后事件仍会提交，但桌面端会跳过并记录原因）。
  旧的 `%USERPROFILE%\.config\agent-notify\{opencode,codex,antigravity,devin,commandcode}.off`
  只在首次迁移时导入为停用状态，之后不再实时读取。
- 全局暂停在 **Settings** 里；暂停期间新通知不会调度，恢复后按正常策略继续。
- 标题含 `🔕` 或 `[勿扰]`、以及勿扰时段内的通知会被跳过：这是策略，不是开关失效。
- Command Code mod 在客户端侧还会检查总开关 `AGENT_NOTIFY_OFF=1` 与旧 marker
  `commandcode.off`（设为 1 或 marker 存在时它连事件都不提交）；OpenCode 插件与 Codex / Antigravity /
  Devin 的 Hook 不检查开关，由桌面端统一判定。

## 桌面端无法启动、界面空白或反复退出

1. 查看 `%LOCALAPPDATA%\AgentNotify\logs\runtime.log`（含启动、迁移与运行时错误）以及
   `runtime.log.1`（轮换后的上一份）。
2. 单实例：重复启动只会聚焦已有窗口，不会启动第二个运行时。若提示「运行时锁已被其他实例占用」，
   切回已有窗口，或从托盘完全退出后重试。
3. 界面白屏或加载失败时确认已安装 Microsoft Edge WebView2 运行时；缺失时安装器会提示，已安装的应用
   可到微软官网安装 Evergreen 运行时后重启。
4. 首次启动会做旧数据迁移；迁移失败会进入**只读诊断模式**（见下一节），此时不能写入新数据。
5. 应用启动后窗口先于宿主就绪时，命令会等待宿主初始化（上限 20 秒）再返回，不需要手动重试；
   初始化失败会给出具体原因（例如数据库损坏、迁移失败），而不是笼统的「请稍后重试」。
6. 若二进制被安全软件隔离，从发布仓库重新下载安装器并核对 `SHA256SUMS.txt` 后覆盖安装。

## 旧数据迁移失败

1. 打开 **Diagnostics 页**查看迁移状态：`已完成` / `部分完成（有跳过记录）` /
   `失败，当前处于只读诊断模式`，并给出具体文件与字段。
2. 「失败，当前处于只读诊断模式」时运行时不会写入新数据；先按页面提示备份旧版数据目录，处理问题后
   点「重新检测」。
3. `检测到旧版 Agent-notify 仍在运行`：说明旧 Win32 悬浮窗还在运行（心跳文件仍新鲜）。先从旧版托盘
   完全退出，再重新检测；当前不会读取或改写旧数据。
4. 部分完成（有跳过记录）时旧文件保持原样，可以保留核对；导入报告在
   `%LOCALAPPDATA%\AgentNotify\data\legacy-import-report.json`。
5. 迁移只读旧文件，重复启动不会重复迁移；不会删除旧配置、旧凭据或旧历史。

## 提 Issue 前收集

先移除用户名、本机路径、账号提示、客户端标识和平台消息标识；日志不会包含 token，但仍需要人工检查。
只附解决问题所需的片段，不要直接上传整份 `state.db` 或完整日志。

- 应用版本（主窗口底部状态栏或 Diagnostics 页显示的版本，也可看安装目录 `VERSION`）、Windows 版本与架构。
- `agentnotify-ingress.exe --doctor` 的 JSON 报告（只读，无敏感字段）。
- Diagnostics 页的存储、后台组件与迁移状态文本；必要时附 `legacy-import-report.json`。
- `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` 最后 30 行（以及 `runtime.log.1` 的相关片段）。
- 对应 Agent 的调试日志：`codex/antigravity/devin-notify-debug.log`，或打开
  `AGENT_NOTIFY_DEBUG=1` 后复现的 `opencode-debug.log` / `commandcode-debug.log`。
- 一键升级失败的完整界面提示，以及 `%LOCALAPPDATA%\AgentNotify\temp\updates\last-update.log`。
- 相关 Agent 客户端版本（OpenCode、Codex、Antigravity、Devin、Command Code）与已执行的排查步骤。
