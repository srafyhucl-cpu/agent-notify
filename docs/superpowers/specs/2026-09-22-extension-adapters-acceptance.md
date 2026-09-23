# 四个 Agent 适配器的真机验收清单

日期：2026-09-22（结果待填）
范围：阶段 D 的 Codex / Antigravity / Devin / Command Code 四个适配器。
**不含**：飞书、多账号策略、外部适配器协议（计划的 Task 6–10，属于新增能力而非覆盖 Go 版）。

## 0. 前置条件（必须按顺序）

1. **退出正在运行的桌面端**：托盘右键 → 退出（单实例互斥，不退出无法替换二进制）。
   - 说明：退出期间 ingress 会把事件写进 spool，重启后自动补投，不会丢通知。
2. **部署新构建**（release 产物）到预览安装目录 `D:\app\AgentNotify-Rust-Preview\`：

   > ⚠️ **必须用正式构建路径**：`tools\build-release.ps1`（或先 `npm run build` 再 `cargo build -p agentnotify-desktop --release --locked --target x86_64-pc-windows-msvc --features tauri/custom-protocol`）。
   > 直接裸跑 `cargo build --release` 得到的是**开发模式**二进制：窗口会显示 `localhost 拒绝连接`（它在找 devUrl `http://localhost:1420`），因为前端资源没有被嵌进去。

   | 源（`D:\Temp\agentnotify-rust-target\x86_64-pc-windows-msvc\release\`） | 目标 |
   | --- | --- |
   | `agentnotify-desktop.exe` | 同目录同名 |
   | `agentnotify-ingress.exe` | 同目录同名 |
   | `agentnotify-codex-hook.exe` | 同目录同名 |
   | `agentnotify-antigravity-hook.exe` | 同目录同名 |
   | `agentnotify-devin-hook.exe` | 同目录同名 |

   注意：Hook 必须与 `agentnotify-ingress.exe` 同目录（Hook 按 env → 同目录 → 正式安装目录的顺序找 ingress）。
3. **启动桌面端**（脱离 MSIX 包上下文：`explorer.exe 'D:\app\AgentNotify-Rust-Preview\start-preview.cmd'`）。
4. **确认 Agents 页出现 5 个 Agent**：`opencode`（启用）、`codex`、`antigravity`、`devin`、`commandcode`。
   - 四个新适配器**默认关闭**（升级时补的 `enabled=false` 配置行；`devin` 沿用你在 Go 版的关闭状态）。
   - **每个 Agent 必须先启用，否则通知会被跳过**（这是本次最容易踩的一步）。

## 1. Codex（优先：它现在是断的）

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\Project\Agent-notify\tools\hooks\install-codex-v2.ps1 `
  -HookPath 'D:\app\AgentNotify-Rust-Preview\agentnotify-codex-hook.exe' `
  -Ingress  'D:\app\AgentNotify-Rust-Preview\agentnotify-ingress.exe'
```

验收步骤：

1. 检查 `%USERPROFILE%\.codex\config.toml` 的 `notify` 行：链上旧 `agent-notify.exe` 已换成 `agentnotify-codex-hook.exe`，`codex-computer-use.exe` 包装仍在；备份 `config.toml.bak-notify-wrapper` 已生成。
2. Agents 页启用 `codex`。
3. 跑一个真实 Codex 任务并让它结束。
4. **预期**：微信收到 `【codex】<任务名>` 通知（标题 6 级降级链：状态库 `threads.name` → `title` → `first_user_message` → `session_index.jsonl` → payload 首条消息 → `跑完了`）。
5. **预期**：在微信里**引用**这条通知回复 → Codex 对应线程收到消息（走 `codex queue --thread=<id> --message=<text>`，参数数组不经 shell）。
6. 排查：`%TEMP%\agent-notify\codex-notify-debug.log`（透传/ingress 失败都会写原因；退出码 2 = 事件被协议拒绝）。

## 2. Antigravity

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\Project\Agent-notify\tools\hooks\install-antigravity-v2.ps1 `
  -HookPath 'D:\app\AgentNotify-Rust-Preview\agentnotify-antigravity-hook.exe' `
  -Ingress  'D:\app\AgentNotify-Rust-Preview\agentnotify-ingress.exe'
```

1. 检查 `%USERPROFILE%\.gemini\config\hooks.json`：顶层 `agent-notify` 键指向新 Hook，其他顶层键原样保留；备份 `hooks.json.bak-agent-notify`。
2. Agents 页启用 `antigravity`。
3. 跑一个真实 Antigravity 任务直到 stop。
4. **预期**：微信收到通知；`fullyIdle` 非真时不推送（避免中途误报）。
5. **预期**：引用回复 → 回到**原会话**（精确 `conversationId`，不回退最近会话）。
6. 排查：`%TEMP%\agent-notify\antigravity-notify-debug.log`。

## 3. Devin

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\Project\Agent-notify\tools\hooks\install-devin-v2.ps1 `
  -HookPath 'D:\app\AgentNotify-Rust-Preview\agentnotify-devin-hook.exe' `
  -Ingress  'D:\app\AgentNotify-Rust-Preview\agentnotify-ingress.exe'
```

1. 检查 `%APPDATA%\devin\config.json`：`hooks.Stop` 的 AgentNotify handler 已更新，其他 matcher 保留；备份 `config.json.bak-agent-notify`；扩展装到 `%USERPROFILE%\.devin\extensions\agent-notify-reply-v2`。
2. Agents 页启用 `devin`。
3. 跑一个真实 Devin 任务。
4. **预期**：微信收到通知（`stop_hook_active` 为真时跳过，避免递归推送）。
5. **预期**：引用回复 → 经 ACP 精确投递给对应会话（需要扩展在线；扩展离线时给可读错误，不猜目标）。
6. 排查：`%TEMP%\agent-notify\devin-notify-debug.log`。

## 4. Command Code

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\Project\Agent-notify\tools\hooks\install-commandcode-v2.ps1 `
  -Ingress 'D:\app\AgentNotify-Rust-Preview\agentnotify-ingress.exe'
```

1. 检查 `%USERPROFILE%\.commandcode\mods\agent-notify.ts` 已安装（该 mod 是本次新装的，之前不存在）。
2. Agents 页启用 `commandcode`。
3. 跑一个真实 Command Code 任务。
4. **预期**：微信收到通知（`run_end` 事件，同进程其他会话不误推）。
5. **预期**：在**回复窗口**内引用回复 → 注入到原会话（窗口秒数来源：环境变量 `AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC` → 应用写入的 `commandcode-reply-inbox/window.json`（界面保存值，适配器同源）→ 旧配置 `commandCodeReplyWindowSec` → 0 表示关闭）。
6. 排查：`%TEMP%\agent-notify\commandcode-notify-debug.log`；窗口关闭/过期有独立错误码。

## 5. 回滚（任一 Agent 出问题）

| Agent | 回滚方式 |
| --- | --- |
| Codex | 用 `config.toml.bak-notify-wrapper` 覆盖回 `config.toml` |
| Antigravity | 用 `hooks.json.bak-agent-notify` 覆盖回 `hooks.json` |
| Devin | 用 `config.json.bak-agent-notify` 覆盖回 `config.json`；删除 `%USERPROFILE%\.devin\extensions\agent-notify-reply-v2` |
| Command Code | 删除 `%USERPROFILE%\.commandcode\mods\agent-notify.ts` |

## 6. 通用排查

- **通知没到**：先看 Agents 页该 Agent 是否启用（默认关闭），再看 Diagnostics 页与日志。
- **`agent_not_registered`**：跑的是旧构建（未部署新 release）或事件来自未注册的 Agent。
- **事件被拒**：ingress 对 `body` 有 64 KiB 硬上限；mod/Hook 侧已做截断与诊断。
- spool 位置：`%LOCALAPPDATA%\AgentNotify\spool`（管道不可用时的事件会暂存并在重启后补投）。

## 7. 已知限制（验收时留意，不算失败）

1. **Antigravity 端点发现未经真机验证**：进程枚举 + PEB 读 `--csrf_token` + 监听端口按 Go 同款实现复刻，真实端点选择需要本机 Antigravity 运行时才能确认。
2. **Command Code 结果文件残留**：回复超时（10 秒）后返回 Unknown 且不重试，扩展稍后写入的结果文件会留在磁盘（与 OpenCode 收件箱现状一致）。
3. **Devin `state.vscdb` 只读容错**：Devin 长时间独占锁时会提示稍后重试，不做多库回退（避免猜测式兜底）。

## 8. 验收结果记录（待填）

| Agent | 安装 | 推送 | 引用回复 | 备注 |
| --- | --- | --- | --- | --- |
| Codex | ✅ | ✅ | ✅ | 2026-09-22 10:31 安装（备份 `config.toml.bak-notify-wrapper`）；通知 `ec811566…` 于 10:45:00 投递成功（平台消息 `<平台消息ID已脱敏>`，路由有效期 24 小时）；引用回复已进入对应 Codex 线程（用户实测确认） |
| Antigravity | ✅ | ✅ | ✅ | 2026-09-22 12:27 安装（启动器 `~/.gemini/config/agent-notify-hook.cmd` 指向预览目录 Hook；第三方顶层键 `linkweixin-notify` 未被触碰）；通知 `29182ff7` 于 12:30:36 投递成功（平台消息 `<平台消息ID已脱敏>`，路由有效期 24 小时）；引用回复经用户实测通过（`fullyIdle` 过滤生效，未跑完不推送） |
| Devin | ✅ | ✅ | ✅ | 2026-09-22 12:27 装 Hook + V2 扩展（备份 `config.json.bak-agent-notify`）；启用后通知 `9287c35a…` 于 13:06:54 投递成功（此前 `628ea2bf` 因 Agent 未启用被跳过，符合预期）；引用回复经用户实测通过（Hook 透传链路正常） |
| Command Code | ✅ | ✅ | ✅ | 2026-09-22 13:56 装 mod（窗口通道修复后 `window.json` 送达 300 秒，mod 界面出现"正在等待微信引用回复"提示）；通知 `97c9abe5` 于 14:00:47 投递成功（平台消息 `<平台消息ID已脱敏>`）；引用回复注入原会话（`77d2f40b` 正文"收到引用回复"，14:01:17 投递成功） |

### 验收中发现的缺陷（均已定位）

1. **通知格式没有渲染**（影响全部 Agent）：`render_notification` 有实现有测试但**生产无调用**，投递层直接拼 `title\n\nbody`，微信里没有徽章/标题栏/页脚。→ 修复中（投递层把结构化信息交给渠道渲染，缺信息时保持原样）。
2. **Agent 配置不生效**（影响四个新 Agent）：`config_schema`（`codexHome` 等）在界面可编辑、存库，但适配器用 `from_default_location()` 构造，从不读取。→ 修复中。
3. **发布链未包含新接入**：安装器/`build-release.ps1` 只带 OpenCode 插件，未包含 3 个新 Hook、Devin V2 扩展、Command Code V2 mod。→ 修复中。
4. **积压通知不补投**（设计如此，非缺陷）：`Skipped`（如 `session_missing`）是终态，不会重投；只能在 History 里查看。
5. **ClawBot 主动推送会话失效需入站消息恢复**：平台返回 `PrepareFailed` 时运行时会清空上下文，此后所有 Agent 的推送都会 `session_missing`，直到用户给 bot 发一条消息。已观察到该状态与"应用重启 + `notifystart`"在时间上高度相关（10:26:56 重启 → 10:26:59 起全部失败），但**日志没有记录平台返回的具体原因**，属诊断缺口，待补日志后再定位是否为缺陷。

### 边缘验收结果（2026-09-22 下午）

| 项目 | 结果 | 证据 |
| --- | --- | --- |
| **重复事件** | ✅ 通过 | 用真实 Hook + 真实 ingress 提交同一 `thread-id`+`turn-id` 两次（cmd 管道喂 stdin，因为 Hook 是 GUI 子系统 exe，PowerShell 管道不可靠）：数据库仅 1 条通知，`ingest_key = codex:01a0c6f4-dedup-acceptance:turn-dedup-0003` |
| **脱敏日志** | ✅ 通过 | `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` 中 `token`/`secret`/`authorization`/`cookie`/`context_token`/`bot_token` 全部零出现，且无 32 位以上疑似密钥串；新诊断日志只含错误码、固定 reason 与文案 |
| **推送会话失效原因可区分** | ✅ 通过 | 新回执文案实测落在路径 1（"请先给 ClawBot 发送一条消息"）而非路径 2（"平台未能准备会话"），与"重启清空上下文"的判断一致 |
| **客户端退出** | ✅ 通过（Command Code 实测） | 关闭 Command Code 后引用其推送回复，微信收到：`无法续聊：发送到 Agent 失败（Command Code 目标会话未在运行，请先打开该会话；回复不会改投到其他会话）。` —— 可读、指明原因、且明确不误投。**其余三个适配器的同类场景仍待各自实测**（Codex/Antigravity/Devin 的离线路径目前仅契约测试覆盖） |
| **超时 / Unknown** | ⚠️ 仅契约测试覆盖 | 真机未注入故障（需要断网或让客户端无响应，代价过高）。语义由 `crates/agentnotify-application` 与各适配器的契约测试锁定：超时记 `Unknown` 且不自动重试、中断的投递重启后不重发 |

### 发布链预检（2026-09-22 傍晚）

| 检查 | 结果 |
| --- | --- |
| `tools\build-release.ps1 -Version 2.0.0` | exit 0 ✅ |
| 产物 | `Agent-notify-Setup-v2.0.0.exe`（7.67 MB）、`Agent-notify-v2.0.0.zip`（8.68 MB）、`SHA256SUMS.txt` ✅ |
| 安装器静默契约（内嵌 + 独立冒烟） | 通过 ✅（无裸 `MsgBox`；`[Run]` 不带 `skipifsilent`；Rust 正式包契约通过） |
| `tools\check-version.ps1` | 2.0.0 在所有发布位置一致 ✅ |
| 发布前置条件 | workflow 触发条件 `tags: v*` ✅；三个 secrets 均已配置 ✅ |

### 一键升级（静默安装 + 自动重启）的验证状态

- **已覆盖**：`installer_arguments` 与 Go 版一致（`/SILENT /NORESTART /LOG=`）、拉起成功后请求优雅退出、拉起失败不退出且返回原因、安装器静默分支（`WizardSilent`）与结构断言、UI 安装中/失败文案 —— 单测与结构断言全绿。
- **未覆盖（需真机端到端）**：点「下载并安装」→ 无向导无弹窗 → 应用自动退出 → 安装器静默替换 → 自动重启回到新版。**该验证必须等一个比当前更高的版本发布后才能做**（本次 2.0.0 发布后，可用下一次发布验证，或用 1.15/1.17 的旧客户端升级到 2.0.0 来验证同一套静默安装链路）。
- **有意不加 `/SUPPRESSMSGBOXES`**（理由见 `hosts/desktop-tauri/src/update/install.rs` 的注释）：加上会让 Restart Manager 的「无法关闭应用」提示变成静默中止安装，应用已退出且不会自动重启，更难恢复。

### 计划 Task 11 Step 4 的完成度

计划要求每个适配器单独记录：真实客户端版本与测试时间、正常推送、精确回复、账号或客户端退出、超时/`Unknown`/重复事件、脱敏日志检查、失败复现步骤。

- 已完成：四个适配器的**正常推送 + 精确回复**、**重复事件**、**脱敏日志检查**、失败复现步骤（5 条缺陷及定位）、**客户端退出**（Command Code 实测通过）
- 未完成：其余三个适配器的**客户端退出**实测、**超时 / Unknown** 的真机故障注入
- 真实客户端版本：Antigravity **2.15.1**（可执行文件版本读出）；Codex / Devin / Command Code 未能在常见安装位置自动读出，可在客户端内查看后补记
- 结论：Task 11 的 Step 4 **不勾选**；其余步骤还依赖 Task 6–10（飞书、多账号、外部适配器协议）的实现。

## 9. 2.0.0 发布记录（2026-09-22）

| 项 | 值 |
| --- | --- |
| tag | `v2.0.0`（annotated，指向 `54f579a`；首次指向 `0f10827` 时 Release 因构建脚本写死本地 cargo 路径而失败，修复后重指） |
| 修复提交 | `54f579a fix(release): 构建脚本改为 CI 可移植的 cargo 解析` |
| Release workflow | 运行 `35724402360`，**成功**，21 分 40 秒 |
| 发布产物 | `Agent-notify-Setup-v2.0.0.exe`（8,020,168 B / SHA256 `84bf6c94…45110`）、`Agent-notify-v2.0.0.zip`（9,101,429 B / `1f93274b…e7b63`）、`SHA256SUMS.txt` |
| 镜像 | 二进制仓库 `srafyhucl-cpu/agent-notify-releases` 已同步，`/releases/latest` = `v2.0.0` ✅ |
| 独立验证 | 下载安装器核对 SHA256 一致 ✅；Authenticode 指纹 = `EDF9E283DF2407B318E65D59BB430FD546509ACD`（与客户端内置指纹一致）✅；签名者 `CN=Agent-notify Code Signing`（自签名，故 Windows 报 `UnknownError`，客户端按指纹校验） |

**发布后的待办**：真机升级验收（旧客户端检查更新 → 原地升级到 2.0.0 → 验证迁移、五个 Agent、微信推送与引用回复），以及升级后卸掉预览版避免两个实例抢同一个 ClawBot 会话。

## 10. 待办（更新于 2026-09-22 晚）

> 本节先前列的 5 项（一键升级、ClawBot 诊断、安装器冒烟、卸载 V2 清理、发布签名门禁）**均已完成并提交**，见 `9d34ece`、`e1dbb7e`、`2e5b1cb` 与本文档的预检章节。

1. ~~**一键升级的真机端到端验证**~~：✅ 已完成（2026-09-23，Go 1.17.0 → 2.0.2 手动升级成功），结果与后续缺陷见 **第 11 节**。
2. **计划 Task 6–10**：飞书渠道、多账号通知策略、外部适配器进程协议（属新增能力，按既定安排排在 2.0.0 之后）。
3. **验收完整性**：其余三个适配器的「客户端退出」实测、超时/`Unknown` 的真机故障注入、Codex / Devin / Command Code 的客户端版本补记。
4. **迁移遗留的死设置**：`defaultAgent`、`widgetAgentMode`、`theme` 已被迁移导入但无消费者（原本只服务已移除的悬浮窗）；清理需要动迁移契约，应独立评审。
5. **`hook_installer` 能力没有应用内动作**：Agents 页显示「Hook 安装 可用」，但目前重装接入要手动跑 `tools\hooks\install-*-v2.ps1`（Go 版靠 `sync`）。
6. **Devin 的 `replyInbox` 通道问题**：扩展只认环境变量 `AGENT_NOTIFY_DEVIN_REPLY_DIR` 或默认路径，界面里改 `replyInbox` 不会同步到扩展；修法可参考 Command Code 的 `commandcode-reply-inbox/window.json`。
7. **升级时的自定义 Codex notify 不会被接管**（已知限制，非缺陷）：若 `notify` 指向第三方程序，接入脚本保持原样并打印手动接入说明——把任意程序包进 `--previous-notify` 会改变它的调用参数，Go 版也只对 CUA 做包装。

## 11. 2.0.1 / 2.0.2 真机升级验收与两个发布链缺陷（2026-09-23）

### 验收结果（用户机器，Go 1.17.0 → 2.0.2）

| 项 | 结果 |
| --- | --- |
| 手动「检查更新 → 升级」 | ✅ 成功：下载安装器 → 静默替换 → 自动重启回到 2.0.2（注册表 `DisplayVersion=2.0.2`，进程运行中） |
| 静默升级不再弹「无法关闭应用程序」 | ✅ 2.0.2 的 `PrepareToInstall` 强杀旧进程后未再出现（2.0.1 曾出现，用户点 Try again 才装成） |
| 迁移继承 Agent 开关 | ✅ 四个 Agent（codex / antigravity / devin / commandcode）升级后均为启用 |
| 数据库迁移 | ✅ 校验和按行尾归一化后可正常打开（2.0.1 修复的缺陷） |
| 微信推送与引用回复 | ⏸ 升级后被测缺陷阻断（见下），待自愈/重启后回归 |

### 缺陷 1：首启落入"迁移诊断模式"，**所有推送整段断掉**（2.0.3 修复）

- **现象**：升级到 2.0.2 后收不到任何推送（用户先报 OpenCode，实测四个 Agent 全断）。
- **证据**：`runtime.log` 记录 `旧数据迁移失败，启动迁移诊断模式 code=legacy_app_running`；`%TEMP%\agent-notify\widget-alive.txt`（旧版心跳）mtime `10:29:13`，新版启动 `10:29:26`，相差 13 秒，落在 runtime 的 **30 秒**存活判定窗口内。
- **机理**：升级安装器先杀旧版、几秒后拉起新版 → 心跳必然新鲜 → `prepare_migration` 判定"旧版仍在运行" → `start_migration_diagnostics`（**不启动渠道、ingress 与 Outbox**）→ 推送全断，且**不会自动恢复**，只能手动重启应用或点界面"重新检测"。
- **影响面**：所有从旧版原地升级的用户都会命中，属于升级路径缺陷。
- **修复**（提交 `c133400`）：`ProductionRuntimeCoordinator::spawn_migration_autoretry` —— 诊断原因是 `legacy_app_running` 时后台每 35 秒重试一次常规启动（上限 6 次 ≈ 3.5 分钟），心跳过期即自愈，恢复后发 `snapshot.changed` 让界面刷新；其它迁移失败原因不重试，保持原样暴露。含 4 个单元测试（恢复即停、用尽次数、不可自愈错误即停、原因分类）。
- **事件不丢**：ingress 的 spool 会保留事件（实测积压 10:31 / 10:48 共 6 条），运行时恢复后自动补推。

### 缺陷 2：镜像存在"半成品窗口"，客户端可能选错升级路径（`b7f17e9` 修复）

- **现象**：Go 版**自动**检查更新报 `更新包缺少文件：…\updates\2.0.2\extracted\Agent-notify\install.ps1`，手动点「检查更新」则成功。
- **机理**：Go 升级器优先选安装器、回退压缩包（`ArtifactArchive`）；镜像用 `gh release create` **逐个**上传资产，Release 在 `Setup` 传完前就已可见并置 Latest，客户端在窗口期内查询只看到 ZIP → 选压缩包分支 → 该分支要求 Go 时代布局（`install.ps1` + `bin/agent-notify.exe`），Rust ZIP 必然不满足。
- **修复**：`tools/publish-release.ps1` 改为**草稿创建 → 上传全部资产 → 最后一步 `--draft=false --latest` 发布**，客户端在资产齐备前看不到该 Release；下一次发布生效。

### 待办（本轮新增）

1. **2.0.3 发布**：带上缺陷 1 的自愈修复，并复验"自动检查更新"路径（缺陷 2 的修复在 workflow/工具侧，发布即生效）。
2. **验收回归**：自愈后的微信推送与引用回复实测（含 spool 补推的观察）。
