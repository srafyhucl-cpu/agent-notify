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
| **客户端退出** | ⏳ 待验（需真机引用回复） | 步骤：先给 bot 发一条消息重建推送会话 → 关闭目标客户端 → 引用其历史推送回复 → 预期得到可读错误而非静默丢弃 |
| **超时 / Unknown** | ⚠️ 仅契约测试覆盖 | 真机未注入故障（需要断网或让客户端无响应，代价过高）。语义由 `crates/agentnotify-application` 与各适配器的契约测试锁定：超时记 `Unknown` 且不自动重试、中断的投递重启后不重发 |

### 计划 Task 11 Step 4 的完成度

计划要求每个适配器单独记录：真实客户端版本与测试时间、正常推送、精确回复、账号或客户端退出、超时/`Unknown`/重复事件、脱敏日志检查、失败复现步骤。

- 已完成：四个适配器的**正常推送 + 精确回复**、**重复事件**、**脱敏日志检查**、失败复现步骤（5 条缺陷及定位）
- 未完成：真实**客户端版本**记录、**客户端退出**的真机行为、**超时 / Unknown** 的真机故障注入
- 结论：Task 11 的 Step 4 **不勾选**；其余步骤还依赖 Task 6–10（飞书、多账号、外部适配器协议）的实现。

## 9. 待办（本轮修复落地后排队）
1. **一键升级**（已确认排期）——Go 版有、Rust 版缺失的最后一块能力：
   - **已有**：`agentnotify_desktop::update::{sha256_file, verify_download, SignatureRequirement}`（SHA256 / 签名指纹 / PE 版本三类校验，测试完备）、Settings 页"检查更新"入口、`get_update_status` 主机命令、发布链的 `SHA256SUMS.txt` 与 `agent-notify-releases` 镜像仓库。
   - **缺**：查询最新 Release 并比对版本、下载安装包到临时目录、校验、拉起安装器、失败时给用户可读提示。
   - **约束**：正式渠道必须强制签名校验（未签名包一律拒绝安装，保持 `internal/update/signature.go` 的内置指纹约定）；预览/本地构建可放宽；沿用现有 `UpdateStateDto`（`UpToDate`/`Available`/`ReadyToInstall`/`Unsupported`/`Failed`）。
   - **相邻问题（已定位，本轮不修）**：Devin 的 `replyInbox` 有同类通道问题——扩展只认环境变量 `AGENT_NOTIFY_DEVIN_REPLY_DIR` 或默认路径，界面里改 `replyInbox` 不会同步到扩展，引用回复会投到旧目录；修法可参考 Command Code 的 `commandcode-reply-inbox/window.json`（应用把界面值写进自己的收件箱，扩展优先读它）。
2. **补齐 ClawBot 诊断缺口**：记录 `notifystart` 的结果与"清空推送上下文"的原因。当前 `PrepareFailed` 与"上下文不存在"在用户侧是同一句话，无法定位"重启后推送静默失效"的根因；补日志后再判定是否为缺陷。
3. **安装器冒烟纳入新接入**：本轮修复会在 `tests/installer-smoke.ps1` 增加断言，验收时需在沙箱跑一次确认（不触碰真实环境）。
4. **卸载路径补齐 V2 清理**：`uninstall.ps1 -HooksOnly` 的识别模式只认旧程序名（`agent-notify.exe` / `agent-notify-hook.cmd`），因此**卸载后** Devin 的 `hooks.Stop` handler 与 Codex 的 `notify` 行仍指向已删除的 exe。需为 V2 Hook 增加显式识别（注意不能改变升级清理的语义：升级时先清旧、再装新）。
5. **发布门禁补三个 Hook 的签名校验**：`tools/publish-release.ps1` 目前只校验安装器与 ZIP 内主程序的签名指纹，三个 Hook 虽在构建时已签名并校验，但发布补发路径未覆盖；建议加入 `SHA256SUMS.txt` 与指纹校验循环。
6. **升级时的自定义 Codex notify 不会被接管**（已知限制，非缺陷）：若用户的 `config.toml` 里 `notify` 指向**第三方程序**（非 `codex-computer-use.exe` 也不是 AgentNotify），接入脚本会保持原样并打印"如需接入请手动改为…"。原因是把任意程序包进 `--previous-notify` 会改变它的调用参数、可能破坏用户自己的集成；Go 版也只对 CUA 做包装。如需覆盖此场景，应作为独立评审的接入脚本行为变更。
