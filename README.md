# AgentNotify

<p align="center">
  <img src="https://img.shields.io/badge/version-2.0.1-blue.svg?style=flat-square" alt="Version" />
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6.svg?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/License-MIT-green.svg?style=flat-square" alt="License" />
</p>

AgentNotify 是 Windows 通知工具。任务完成后，它通过 ClawBot 把标题和摘要发到微信，并在本机保留推送历史；在微信里引用这条通知回复，可以继续对应的 Agent 会话。

2.0.0 起正式入口是 Tauri 桌面版：主程序 `agentnotify-desktop.exe`，工作台式主窗口，状态存 SQLite。当前支持 **OpenCode、Codex、Antigravity、Devin、Command Code 五个 Agent + ClawBot/微信**，其它渠道尚未实现，见[接入范围](#接入范围)。

## 功能

- 工作台式主窗口，六个区域：总览、Agents、Channels、History、Diagnostics、Settings。
- ClawBot 扫码登录，并等待微信首条消息建立主动推送会话；登录、测试发送都在 Channels 页完成。
- 凭据由 Windows 凭据管理器保存；运行日志和界面不显示 token 等敏感字段。
- OpenCode 使用 V2 插件监听会话完成/失败事件，经 `agentnotify-ingress.exe` 提交；插件出错不会阻塞 OpenCode。
- Codex、Antigravity、Devin、Command Code 四个 Agent 各自通过独立接入上报完成事件：Codex 走 notify Hook、Antigravity 走 Stop Hook、Devin 走 Stop Hook 与桌面扩展、Command Code 走 V2 mod；接入失败不会阻塞各自客户端，失败原因写入各自的调试日志。
- 应用内一键升级：Settings → 更新 里检查最新版本，点「下载并安装」后静默完成安装并自动重启；正式渠道只接受通过内置指纹校验的签名安装包。
- 通知内容为会话标题加摘要正文，正文保留 Markdown；超过渠道长度上限时明确失败，不静默截断。
- 推送历史存 SQLite：History 页可按 Agent、渠道、账号、状态和时间定位通知，查看投递详情；失败且标记为可重试的投递可以手动重试。
- Diagnostics 页展示存储、后台组件与旧数据迁移诊断，失败都会给出可读原因。
- 微信引用回复：只按引用消息的原始平台消息 ID 与持久化路由精确匹配，不回退到最近会话。
- 通知冷却、勿扰时段、全局暂停在 Settings 中调整。
- 托盘常驻：关闭主窗口只隐藏到托盘，可从托盘显示窗口、暂停/恢复通知或退出。
- 单实例：重复启动只会聚焦已有窗口，不会启动第二个运行时。
- 首次启动自动做一次只读旧数据迁移，旧文件不会被改写。

## 接入范围

当前支持：

- Agent：OpenCode（V2 插件）、Codex（notify Hook）、Antigravity（Stop Hook）、Devin（Stop Hook + 桌面扩展）、Command Code（V2 mod）。
- 渠道：ClawBot / 微信。
- 应用内一键升级：检查更新、下载、校验、静默安装并自动重启。

后续扩展（尚未实现）：

- 飞书等其它渠道、外部适配器协议。
- 跨平台（macOS、HarmonyOS PC）。

## 安装

普通用户从[公开 Release](https://github.com/srafyhucl-cpu/agent-notify-releases/releases/latest)下载 `Agent-notify-Setup-vX.Y.Z.exe`，双击后按向导完成安装。安装器默认按当前用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，不要求管理员权限；安装前会检查 Microsoft Edge WebView2 运行时，缺失时提示先安装。

向导默认勾选：

- 创建桌面快捷方式、开机自动启动。
- 接入 OpenCode 通知插件：把 `plugin\rust\agent-notify.ts` 写到 `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`，并把 `agentnotify-ingress.exe` 的绝对路径绑定进插件。接入后需要重启 OpenCode。
- 接入 Codex / Antigravity / Devin / Command Code：由安装器调用对应接入脚本，只改写各自的配置项（Codex 的 `notify` 行、Antigravity 的 Hook 与启动器、Devin 的 Stop Hook 与 V2 扩展、Command Code 的 mod），第三方接入与自定义配置原样保留。不需要时可以取消勾选。

安装完成页会启动 AgentNotify。首次启动做一次**只读**旧数据迁移：读取旧配置、登录状态、开关、推送历史与引用路由导入新库，旧文件保持不变，重复启动不会重复迁移。从旧版升级时，**Agent 开关按旧版状态继承**：旧版开着的 Agent 升级后继续开着，旧版关掉的保持关闭，不会因为升级静默停掉通知。全新安装则相反：Codex、Antigravity、Devin、Command Code 默认关闭，需要在 Agents 页手动开启；只有 OpenCode 保持“无配置即启用”的历史默认。

升级安装器保留原 AppId，会覆盖回原安装目录；安装时先停止旧进程，清理上一版本写入的旧接入（旧 Go 版 Hook 与 V1 扩展），再按勾选安装新的接入。用户数据一律保留。

发布的安装器与程序都带 Authenticode 签名，Release 同时发布 `SHA256SUMS.txt` 供校验。ZIP 包 `Agent-notify-vX.Y.Z.zip` 用于便携与开发，不是普通用户的主安装入口。

## 首次使用

1. 安装完成后启动 `agentnotify-desktop.exe`。
2. 打开 Channels 页，点击「添加渠道账号」扫码登录 ClawBot；如果微信要求数字配对码，在窗口里输入。
3. 看到「等待首条入站消息」后，在微信中给 ClawBot 发送任意一条消息；状态变成「配对已完成」才算建立主动推送会话。
4. 在 Channels 页的「测试发送」里选择这个账号，发送一条测试通知，确认微信能收到。
5. 需要引用续聊时，到 Settings → 回复 开启「引用回复」，说明见[微信引用回复](#微信引用回复)。

主窗口的关闭按钮只把窗口隐藏到托盘，AgentNotify 继续在后台运行；完全退出请使用托盘菜单的「退出」，或在 Settings 中退出。

## 更新与回滚

在 Settings → 更新 里点「检查更新」查询[发布仓库](https://github.com/srafyhucl-cpu/agent-notify-releases/releases)的最新版本；存在更高版本时点「下载并安装」，应用会下载安装包、校验 SHA256 与签名指纹，然后**静默安装并自动重启**（安装期间主窗口会关闭，安装器显示进度，装完自动启动）。

- 正式渠道只接受由内置指纹签名、且 PE 版本号匹配的安装包；未签名或指纹不符一律拒绝安装，不会静默放行。
- 下载或校验失败不会触碰已安装的文件，当前版本继续可用，失败原因会在界面里给出可读说明。
- 也可以手动安装：从发布仓库下载 `Agent-notify-Setup-vX.Y.Z.exe` 覆盖安装，校验值以 Release 附件 `SHA256SUMS.txt` 为准。

回滚窗口内可以安装上一稳定版 `Agent-notify-Setup-v1.17.0.exe` 回到旧版；校验和、回滚步骤与窗口结束条件见 [Windows 正式入口切换到 Rust 桌面版](docs/superpowers/specs/2026-09-19-windows-rust-cutover.md)。回滚安装器同样保留 AppId，会覆盖回原安装目录；配置、凭据、历史与 SQLite 都不会删除。

## 微信引用回复

默认关闭，在 Settings → 回复 中开启；同一处可以调整送达确认与路由有效期（默认 24 小时）。

- 只有当前绑定微信用户的私聊引用回复会触发 Agent，群聊或其它账号的回复会被忽略。
- 目标只按引用消息携带的原始平台消息 ID 在本机路由中精确匹配；没有对应路由、ID 冲突或路由过期都会明确失败，不会回退到「最近会话」，也不按标题或工作目录猜测。
- 同一条引用消息至多投递一次；投递失败或拒绝会在微信里给出可读原因。
- 引用回复要求 AgentNotify 保持运行：下行轮询由桌面端运行时处理，投递由对应 Agent 的接入完成（OpenCode V2 插件、Codex `codex queue`、Antigravity 本机 agentapi、Devin 桌面扩展、Command Code 回复窗口）。

## 数据与配置

| 内容 | 默认位置 |
|---|---|
| 主程序 | `%LOCALAPPDATA%\Programs\Agent-notify\agentnotify-desktop.exe` |
| 内部事件入口 | `%LOCALAPPDATA%\Programs\Agent-notify\agentnotify-ingress.exe` |
| 状态库（SQLite WAL） | `%LOCALAPPDATA%\AgentNotify\data\state.db` |
| 运行日志 | `%LOCALAPPDATA%\AgentNotify\logs\runtime.log` |
| 离线事件 spool | `%LOCALAPPDATA%\AgentNotify\spool` |
| 旧数据迁移报告 | `%LOCALAPPDATA%\AgentNotify\data\legacy-import-report.json` |
| OpenCode 插件 | `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` |
| 旧配置与凭据（只读） | `%USERPROFILE%\.config\agent-notify\` |

设置项（勿扰、通知冷却、引用回复、路由有效期、更新通道、开机启动等）存在 SQLite 中，通过 Settings 页修改；ClawBot 凭据由 Windows 凭据管理器保存。迁移只读旧目录，不改写也不删除 `%USERPROFILE%\.config\agent-notify\` 里的文件。

`agentnotify-ingress.exe` 只接收版本化 Agent 事件，不面向用户，也不提供管理命令行。

## 卸载

在 Windows「设置 → 应用 → 已安装的应用」中选择 AgentNotify 卸载。卸载只删除程序文件、快捷方式与自启动项；SQLite 状态库、迁移报告、旧配置与凭据都会保留，OpenCode 配置目录里的插件文件也不会被删除。需要彻底清理时请先备份，再手动删除 `%LOCALAPPDATA%\AgentNotify`、`%USERPROFILE%\.config\agent-notify` 与 `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`。

## 开发

要求：

- Windows 10 / 11
- Rust stable（版本见 `rust-toolchain.toml`）
- Node.js 22，用于桌面 UI 与插件类型检查
- Windows PowerShell 5.1+，用于门禁与构建脚本
- Inno Setup 6，仅构建正式安装器时需要

```powershell
# 依赖缓存请留在当前项目盘，不要指向 C 盘
$env:npm_config_cache = 'D:\Temp\npm-cache'

# Rust 门禁：cargo fmt / clippy / test（本地工具链约定见 tools\rust\gate.ps1）
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1

# 插件类型检查
npm ci
node_modules\.bin\tsc.cmd --noEmit

# 桌面 UI 类型检查与单测
cd apps\desktop-ui
npm ci
npm run typecheck
npm test

# 静态检查与完整门禁
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

`tools\lint.ps1` 校验脚本语法、workflow 纯 ASCII 与版本号一致性（唯一来源是仓库根 `VERSION`）；`tools\test.ps1` 运行仓库保留的全部测试、签名门禁回归与隔离冒烟。正式包由 Rust 桌面端构建：构建机需要 `D:\Tools\cargo` + `D:\Tools\rustup`（约定见 `tools\rust\gate.ps1`），本地发布门禁还需要 Inno Setup 6 与签名工具；Release workflow 签名后发布产物，并镜像到二进制仓库 `srafyhucl-cpu/agent-notify-releases`。

## 文档

- [架构说明](docs/ARCHITECTURE.md)
- [故障排查](docs/TROUBLESHOOTING.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [更新日志](CHANGELOG.md)

## License

[MIT License](LICENSE) © 2026 AgentNotify contributors
