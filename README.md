# Agent-notify

<p align="center">
  <img src="https://img.shields.io/badge/version-1.0.0-blue.svg?style=flat-square" alt="Version" />
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6.svg?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/Go-1.25%2B-00ADD8.svg?style=flat-square" alt="Go" />
  <img src="https://img.shields.io/badge/License-MIT-green.svg?style=flat-square" alt="License" />
</p>

Agent-notify 是面向 OpenCode、Codex 与命令行长任务的 Windows 通知工具。任务完成后，它通过 ClawBot 把标题和摘要发送到微信，并在本机保留结构化推送历史。

v1.0.0 是一次彻底重构：运行时只有一个 `agent-notify.exe`，不再依赖旧脚本、旧模块、旧配置或旧环境变量。

## 功能

- ClawBot 微信扫码登录，凭据只保存在本机。
- OpenCode 全局插件，监听任务完成事件并自动提取会话摘要。
- Codex `notify` 接入，保留上游 `codex-computer-use.exe` 事件透传。
- 通用 CLI，可在编译、测试、训练或爬虫结束后主动推送。
- 原生 Windows 悬浮窗：OpenCode / Codex 开关、运行状态、勿扰设置、推送历史、测试推送和托盘。
- 设置窗内置 ClawBot 扫码登录、重新登录和退出登录，不再切换到独立控制台。
- 悬浮窗、设置、登录和历史窗口均按 DPI 缩放并使用双缓冲绘制，支持多显示器 DPI 变化。
- JSON Lines 推送历史，记录成功、失败、未登录与跳过状态。
- 退出码与输出面向脚本友好；hook 调用失败不会阻塞 agent。

## 安装

从 Release 下载 `Agent-notify-v1.0.0.zip`，解压后运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

默认安装结果：

| 内容 | 默认路径 |
|---|---|
| 运行程序 | `%USERPROFILE%\bin\agent-notify.exe` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |
| 安装记录 | `%USERPROFILE%\bin\agent-notify-install.json` |

安装器只在 Codex `config.toml` 的 `notify` 行缺失或指向 `codex-computer-use.exe` 时接管，并在修改前创建 `config.toml.bak-notify-wrapper`。自定义 notify 程序不会被覆盖。

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

# 微信扫码登录
Start-Process $exe -ArgumentList "login" -Wait

# 发送测试通知
Start-Process $exe -ArgumentList "test" -Wait

# 查看完整状态
Start-Process $exe -ArgumentList "status" -Wait
```

安装完成后也可以双击桌面上的 `Agent-notify 悬浮窗`，在“设置”里扫码登录并发送测试推送。

悬浮窗的推荐首次流程：

1. 点击顶部连接卡或“设置”，打开设置窗。
2. 点击“扫码登录”，使用微信扫描窗口内二维码。
3. 登录成功后返回悬浮窗，点击“发送测试”确认微信接收正常。
4. 最小化和关闭按钮只会隐藏悬浮窗；完全退出请右键托盘图标并选择“退出”。

## 命令行

```text
agent-notify login       微信扫码登录 ClawBot
agent-notify logout      删除本机 ClawBot 凭据
agent-notify status      查看登录、开关、路径与最近推送
agent-notify notify      发送一条通知
agent-notify test        发送测试通知
agent-notify doctor      检查配置、凭据、网络与接入
agent-notify toggle      开启或暂停 OpenCode / Codex 推送
agent-notify watch       恢复被改回的 Codex notify 配置
agent-notify history     查看最近推送记录
agent-notify widget      启动桌面悬浮窗
agent-notify version     查看版本与构建信息
```

常用参数：

```powershell
agent-notify.exe notify --title "构建完成" --summary "Release 已生成"
agent-notify.exe notify --title "数据任务完成" --no-stdin
agent-notify.exe notify --dry-run --title "预演" --summary "不会发送"
agent-notify.exe toggle --agent all --off
agent-notify.exe toggle --agent opencode --on
agent-notify.exe status --json
```

`notify` 未提供 `--summary` 且未指定 `--no-stdin` 时，会从标准输入读取摘要。

## 文件与配置

| 项目 | 默认路径 |
|---|---|
| 配置目录 | `%USERPROFILE%\.config\agent-notify` |
| 配置文件 | `%USERPROFILE%\.config\agent-notify\config.json` |
| ClawBot 凭据 | `%USERPROFILE%\.config\agent-notify\clawbot.json` |
| OpenCode 开关 | `%USERPROFILE%\.config\agent-notify\opencode.off` |
| Codex 开关 | `%USERPROFILE%\.config\agent-notify\codex.off` |
| 推送历史 | `%TEMP%\agent-notify\push.log` |
| 运行日志 | `%TEMP%\agent-notify\*.log` |

`config.json`：

```json
{
  "quietHours": "23-8",
  "cooldownMin": 10
}
```

- `quietHours` 为空表示关闭勿扰；格式为 `23-8`，结束时间不包含在静默时段内。
- `cooldownMin` 是 OpenCode 同一会话的去重窗口，默认 10 分钟，范围为 1 到 1440。

可用的 `AGENT_NOTIFY_*` 覆盖项见 [.env.example](.env.example)。所有路径和开关都统一使用 Agent-notify 命名；v1.0.0 不读取旧名称的配置或环境变量。

## Codex 接入

安装器会把 Codex `config.toml` 的 notify 行写成：

```toml
notify = [ "C:/Users/<name>/bin/agent-notify.exe", "codex", "turn-ended" ]
```

调用流程：

1. 动态查找最新的 `codex-computer-use.exe` 并原样透传参数与 stdin。
2. 检查 `codex.off` marker；关闭时只跳过推送，不影响透传。
3. 从 `input-messages` 提取标题，从 `last-assistant-message` 提取摘要。
4. 发送 ClawBot 消息并记录历史。

悬浮窗每两分钟检查一次 Codex 配置；如果 Codex 更新后把 notify 行改回直调 `codex-computer-use.exe`，会自动恢复为 Agent-notify。

## 卸载

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\uninstall.ps1
```

卸载器按 `agent-notify-install.json` 清理程序与插件，并尝试恢复 Codex 配置。登录凭据和用户配置默认保留；如需彻底删除，请手动删除 `%USERPROFILE%\.config\agent-notify`。

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

## 文档

- [架构说明](docs/ARCHITECTURE.md)
- [故障排查](docs/TROUBLESHOOTING.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [更新日志](CHANGELOG.md)

## License

[MIT License](LICENSE) © 2026 Agent-notify contributors
