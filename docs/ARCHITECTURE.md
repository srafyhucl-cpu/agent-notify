# 架构与设计

面向维护者。用户使用说明见 [README](../README.md)，故障排查见 [TROUBLESHOOTING](TROUBLESHOOTING.md)。

## 组件总览

```text
OpenCode
  └─ plugin/agent-notify.ts
        └─ agent-notify.exe notify

Codex
  └─ config.toml notify
        └─ agent-notify.exe codex turn-ended
              ├─ 透传 codex-computer-use.exe
              └─ 发送 ClawBot 消息

命令行 / 脚本
  └─ agent-notify.exe notify

所有发送链路
  └─ internal/notify → internal/clawbot → ClawBot API → 微信
```

常驻与运维命令：

- `agent-notify.exe widget`：原生 Windows 悬浮窗与托盘。
- `agent-notify.exe login`：ClawBot 扫码登录。
- `agent-notify.exe doctor`：配置、凭据、网络和接入自检。
- `agent-notify.exe watch`：仅在 Codex notify 指向 `codex-computer-use.exe` 时恢复 Agent-notify。
- `install.ps1` / `uninstall.ps1`：部署和清理，不再携带运行时业务逻辑。

## 代码职责

| 路径 | 职责 | 关键约束 |
|---|---|---|
| `cmd/agent-notify/main.go` | CLI 命令、控制台处理、交互输出 | 发布版使用 GUI 子系统；仅在实际无可用 stdout 时绑定控制台 |
| `internal/agent` | OpenCode、Codex、toggle、Codex 配置恢复 | hook 路径避免阻塞；Codex 先透传再推送 |
| `internal/clawbot` | 二维码登录、凭据读写、ClawBot API | 凭据字段校验；默认端点统一；发送有限重试 |
| `internal/notify` | 消息渲染、发送、JSONL 历史 | 摘要在最多 800 字符内截断；成功和失败都落历史 |
| `internal/config` | 配置默认值、校验、原子保存、路径解析 | 所有路径可由 `AGENT_NOTIFY_*` 隔离 |
| `internal/marker` | `opencode.off` / `codex.off` 开关 | 文件存在即暂停；不读取旧 marker |
| `internal/ui` | 原生 Win32 悬浮窗、设置、历史、托盘 | 单实例、两分钟 Codex 看护、`windowsgui` 发布模式 |
| `plugin/agent-notify.ts` | OpenCode V2 插件 | 只调用 `agent-notify.exe notify`；失败全部吞掉 |
| `install.ps1` / `uninstall.ps1` | 文件分发、安装记录、快捷方式、Codex 接管 | 不安装业务运行时；卸载按安装记录清理 |

## OpenCode 数据流

1. 插件订阅 `session.execution.succeeded`，同时兼容 `session.idle`。
2. 取 `session.get` 标题和 `session.context` 中最后一条 assistant 文本。
3. 检查 `AGENT_NOTIFY_OFF`、`opencode.off`、内存冷却与跨进程状态冷却。
4. 调用 `agent-notify.exe notify --title ... --summary ... --session ... --no-stdin`。
5. CLI 再检查开关、标题勿扰标记和时段勿扰，然后渲染并发送。
6. 成功发送或失败都写入 `%TEMP%\agent-notify\push.log`。

插件侧与 CLI 侧都做开关和冷却检查，避免不同 OpenCode location 重复拉起进程。

## Codex 数据流

1. Codex 完成一轮后调用 `agent-notify.exe codex turn-ended <json>`。
2. `HandleCodex` 读取 stdin，并动态查找最新 `codex-computer-use.exe`。
3. 将原始参数与 stdin 透传给上游程序。
4. 检查 `codex.off`；关闭时返回跳过，不透传失败也不影响。
5. 标题取 `input-messages[0]`，摘要取 `last-assistant-message`。
6. 渲染后发送，并写入结构化历史。

`agent-notify watch` 与悬浮窗只会在 notify 行仍指向 `codex-computer-use.exe` 时恢复配置；自定义 notify 程序始终保留。

## ClawBot 契约

默认端点：`https://ilinkai.weixin.qq.com`

| 用途 | 请求 |
|---|---|
| 获取二维码 | `GET /ilink/bot/get_bot_qrcode?bot_type=3` |
| 查询扫码状态 | `GET /ilink/bot/get_qrcode_status?qrcode=...` |
| 发送文本 | `POST /ilink/bot/sendmessage` |

登录成功后保存：

```json
{
  "bot_token": "...",
  "ilink_bot_id": "...",
  "baseurl": "https://ilinkai.weixin.qq.com",
  "ilink_user_id": "..."
}
```

`status` 与悬浮窗只显示脱敏后的用户标识，不输出 token。

## 路径与隔离

| 类型 | 默认位置 |
|---|---|
| 配置 | `%USERPROFILE%\.config\agent-notify\config.json` |
| 凭据 | `%USERPROFILE%\.config\agent-notify\clawbot.json` |
| marker | `%USERPROFILE%\.config\agent-notify\{opencode,codex}.off` |
| 历史 | `%TEMP%\agent-notify\push.log` |
| 运行日志 | `%TEMP%\agent-notify\*.log` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |

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

1. 停止安装目录内的旧悬浮窗进程。
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

## 配置与去重

- `quietHours`：`23-8` 表示 23:00 到次日 08:00，结束时间不包含。
- `cooldownMin`：OpenCode 同一会话默认 10 分钟去重，范围 1 到 1440。
- 标题包含 `🔕` 或 `[勿扰]` 时跳过 OpenCode 推送。
- marker 在冷却记账前检查，被暂停的会话不会消耗冷却窗口。

## 日志

`push.log` 每行是一条 JSON 对象：

```json
{"timestamp":"2026-09-12T09:00:00+08:00","agent":"opencode","session":"...","title":"【opencode】任务","summary":"摘要","status":"成功"}
```

失败会额外记录 `error`。历史列表按时间倒序读取，损坏的单行会被跳过，不影响其余记录。

## 版本与发布

- 应用版本唯一来源：`internal/app/version.go` 的 `Version`。
- 本地或 CI 使用 `tools/build-release.ps1` 生成 `Agent-notify-v<版本>.zip` 和 `SHA256SUMS.txt`。
- Release workflow 校验 tag `v<版本>` 与 Go 源码版本一致。
- 发布包解压后包含 `bin/agent-notify.exe`，最终用户不需要安装 Go。

## 不可破坏的契约

1. 安装入口名与位置：`agent-notify.exe`、`agent-notify.ts`、`agent-notify-install.json`。
2. 所有用户可覆盖项统一使用 `AGENT_NOTIFY_*`。
3. marker 文件名为 `opencode.off` 与 `codex.off`，存在即暂停。
4. ClawBot 凭据字段、默认端点和发送消息结构。
5. Codex notify 透传顺序：先上游，后推送；推送失败不得影响透传。
6. `push.log` 使用 JSON Lines，保持向后可读。
7. 发布 exe 保持 Windows GUI 子系统，悬浮窗与 hook 不闪控制台。
8. v1.0.0 不读取旧品牌名称、旧模块、旧脚本或旧环境变量。
