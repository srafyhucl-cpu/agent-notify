# 四个 Agent 适配器的真机验收清单

日期：2026-09-22（结果待填）
范围：阶段 D 的 Codex / Antigravity / Devin / Command Code 四个适配器。
**不含**：飞书、多账号策略、外部适配器协议（计划的 Task 6–10，属于新增能力而非覆盖 Go 版）。

## 0. 前置条件（必须按顺序）

1. **退出正在运行的桌面端**：托盘右键 → 退出（单实例互斥，不退出无法替换二进制）。
   - 说明：退出期间 ingress 会把事件写进 spool，重启后自动补投，不会丢通知。
2. **部署新构建**（release 产物）到预览安装目录 `D:\app\AgentNotify-Rust-Preview\`：

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
5. **预期**：在**回复窗口**内引用回复 → 注入到原会话（窗口秒数来源：环境变量 `AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC` → 旧配置 `commandCodeReplyWindowSec` → 0 表示关闭）。
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
| Codex | ☐ | ☐ | ☐ | |
| Antigravity | ☐ | ☐ | ☐ | |
| Devin | ☐ | ☐ | ☐ | |
| Command Code | ☐ | ☐ | ☐ | |
