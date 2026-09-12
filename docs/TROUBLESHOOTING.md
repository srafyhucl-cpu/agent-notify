# 故障排查

先运行自检，再按症状查日志。日志不包含 ClawBot token，可以安全粘贴相关片段。

```powershell
$exe = "$env:USERPROFILE\bin\agent-notify.exe"
Start-Process $exe -ArgumentList "status" -Wait
Start-Process $exe -ArgumentList "doctor" -Wait
```

## 日志位置

默认目录：`%TEMP%\agent-notify`

| 文件 | 写入方 | 用途 |
|---|---|---|
| `push.log` | CLI / 发送器 | JSON Lines 推送历史，悬浮窗也读取此文件 |
| `opencode-debug.log` | OpenCode 插件 | `AGENT_NOTIFY_DEBUG=1` 时的打开、跳过和退出信息 |
| `codex-notify-debug.log` | Codex 命令 | `AGENT_NOTIFY_CODEX_DEBUG=1` 时的参数、透传与发送信息 |
| `codex-watch.log` | 悬浮窗看护 | Codex notify 行被恢复的时间 |
| `widget-error.log` | 悬浮窗 | UI 或消息循环错误 |
| `widget-trace.log` | 悬浮窗 | 启动、窗口创建和退出追踪 |
| `widget-alive.txt` | 悬浮窗 | 心跳时间 |
| `widget-exit.txt` | 悬浮窗 | 用户主动退出标记 |

用户配置与凭据在 `%USERPROFILE%\.config\agent-notify`。

## 微信完全收不到

1. 确认已登录：

   ```powershell
   Start-Process "$env:USERPROFILE\bin\agent-notify.exe" -ArgumentList "status" -Wait
   ```

2. 未登录或凭据损坏时重新扫码：

   ```powershell
   Start-Process "$env:USERPROFILE\bin\agent-notify.exe" -ArgumentList "login" -Wait
   ```

3. 发送测试：

   ```powershell
   Start-Process "$env:USERPROFILE\bin\agent-notify.exe" -ArgumentList "test" -Wait
   ```

4. 查看 `%TEMP%\agent-notify\push.log`。若状态是 `未登录`，检查 `%USERPROFILE%\.config\agent-notify\clawbot.json` 是否存在且完整；若状态是 `失败`，根据 `error` 判断网络、TLS、超时或 ClawBot 返回。

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
5. 重新运行 `install.ps1` 并重启 OpenCode，确保插件是当前版本。

CLI 还会跳过标题含 `🔕` 或 `[勿扰]` 的推送，以及 `config.json` 中 `quietHours` 覆盖的时段。

## Codex 任务结束不推送

1. 设置 `AGENT_NOTIFY_CODEX_DEBUG=1`，重启 Codex。
2. 查看 `codex-notify-debug.log`。
3. 检查 `%USERPROFILE%\.codex\config.toml` 的 notify 行是否指向：

   ```toml
   notify = [ "C:/Users/<name>/bin/agent-notify.exe", "codex", "turn-ended" ]
   ```

4. 检查 `%USERPROFILE%\.config\agent-notify\codex.off` 是否存在。
5. 运行 `agent-notify watch`，或重启悬浮窗。只有 notify 行仍直指 `codex-computer-use.exe` 时，看护才会恢复 Agent-notify。
6. 如果 Codex 原本使用自定义 notify 程序，安装器不会覆盖；需要手动把自定义程序与 Agent-notify 串接。

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
5. 若安装目录内有旧进程占用文件，运行 `uninstall.ps1` 后重新安装。

## 开关关了仍在推送

- 确认当前用户是安装 Agent-notify 的同一 Windows 用户；配置与 marker 都按用户目录隔离。
- 运行 `agent-notify status --json`，检查 `openCodeEnabled` 与 `codexEnabled`。
- OpenCode 插件与 CLI 都会检查 marker；如果只有插件未更新，重跑安装并重启 OpenCode。
- Codex 关闭推送时仍会透传上游程序，这是预期行为。

## 推送重复

- OpenCode 默认对同一 `sessionID` 在 10 分钟内去重。
- 修改 `cooldownMin` 后重启 OpenCode，使插件重新读取配置。
- `opencode-sent.json` 是跨实例去重状态；删掉它只会让后续事件重新建立状态，不会补发历史消息。

## PowerShell 中的 CLI 输出

`agent-notify.exe` 是 GUI 子系统程序，避免在 hook 和开机启动时闪窗。不要用捕获表达式等待输出；对需要等待的命令使用：

```powershell
Start-Process "$env:USERPROFILE\bin\agent-notify.exe" -ArgumentList "doctor" -Wait
```

需要机器可读输出时，显式重定向：

```powershell
Start-Process "$env:USERPROFILE\bin\agent-notify.exe" `
  -ArgumentList "status --json" `
  -Wait -NoNewWindow `
  -RedirectStandardOutput "$env:TEMP\agent-notify-status.json"
```

## 安装或卸载失败

- 关闭正在运行的 Agent-notify 悬浮窗后重试。
- 安装目录必须可写；默认是 `%USERPROFILE%\bin`。
- 如果 `bin` 中没有 exe，安装器会尝试调用 Go 编译。安装 Go，或通过 `AGENT_NOTIFY_GO` 指定 `go.exe`。
- 卸载保留 `%USERPROFILE%\.config\agent-notify`，这是避免误删登录凭据和用户配置。
- 如需完全重置，先备份需要的配置，再手动删除上述目录并重新安装。

## 提 Issue 前收集

- `agent-notify-install.json` 的版本字段。
- `agent-notify doctor` 的文本输出。
- 对应日志最后 30 行。
- Windows 版本、Agent-notify 版本、OpenCode 或 Codex 版本。
- 已执行的排查步骤。
