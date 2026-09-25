# AgentNotify

<p align="center">
  <img src="https://img.shields.io/badge/version-2.0.7-blue.svg?style=flat-square" alt="Version" />
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6.svg?style=flat-square" alt="Platform" />
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
- 应用内一键升级：Settings → 更新 里检查最新版本，点「下载并安装」后优先静默安装并自动重启；安装器不可用时回退到 ZIP，文件就绪后需手动重启。正式渠道要求安装器通过签名校验，ZIP 回退还要求签名清单通过内置指纹校验，清单覆盖全部普通文件。
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
- 应用内一键升级：检查更新、下载、校验、静默安装；安装器成功时自动重启，ZIP 回退时手动重启。

后续扩展（尚未实现）：

- 飞书等其它渠道、外部适配器协议。
- 跨平台（macOS、HarmonyOS PC）。

## 安装

### 前置条件

- Windows 10 / 11 **x64**；
- Microsoft Edge WebView2 Runtime。安装器会检查，缺失时停止并给出下载提示；
- 微信端能够使用 ClawBot 扫码登录。

普通用户从[公开 Release](https://github.com/srafyhucl-cpu/agent-notify-releases/releases/latest)下载 `Agent-notify-Setup-vX.Y.Z.exe`。安装器按当前用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，不要求管理员权限。

当前发布使用私有代码签名证书。Windows SmartScreen 可能显示“未知发布者”；这不代表文件损坏，但必须先完成下面的摘要和发布者指纹核对，再决定是否运行。不要关闭安全软件或绕过系统安全提示来消除警告。

```powershell
# 1. 核对 Release 附件 SHA256SUMS.txt 中对应安装器的值
Get-FileHash .\Agent-notify-Setup-vX.Y.Z.exe -Algorithm SHA256

# 2. 核对 Authenticode 签名者指纹
(Get-AuthenticodeSignature .\Agent-notify-Setup-vX.Y.Z.exe).SignerCertificate.Thumbprint
# 当前发布指纹：EDF9E283DF2407B318E65D59BB430FD546509ACD
```

两项都一致后再双击安装。若使用 ZIP 备用包，还要确认包内存在 `RELEASE-MANIFEST.json` 与
`RELEASE-MANIFEST.p7s`，并让 AgentNotify 的 Stable 更新器完成清单验签；不要把外层 ZIP SHA256
一致误解为内部每个文件都已被认证。指纹轮换时，以仓库当前 `SECURITY.md` 和 `docs/code-signing.md` 公布的值为准。

### 安装向导

向导默认勾选：

- 创建桌面快捷方式、开机自动启动；
- 接入 OpenCode 通知插件：把 `plugin\rust\agent-notify.ts` 写到 `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`，并绑定本次安装目录中的 `agentnotify-ingress.exe`；接入后需要重启 OpenCode；
- 接入 Codex / Antigravity / Devin / Command Code：安装器只改写 AgentNotify 自己管理的 Codex `notify`、Antigravity Hook/启动器、Devin Stop Hook/V2 扩展和 Command Code mod。第三方配置与归属校验不通过的自定义内容会保留并报告冲突。

安装完成页会启动 AgentNotify。首次启动只读迁移旧配置、登录状态、开关、推送历史与引用路由，不改写旧文件。从旧版升级时继承旧 Agent 开关；全新安装时 Codex、Antigravity、Devin、Command Code 默认关闭，需要在 Agents 页手动开启，OpenCode 保持无配置即启用。

升级安装器保留原 AppId 并覆盖原目录；会停止旧进程、清理上一版本接入并安装本次勾选的接入。SQLite 状态库、凭据、历史与引用路由不随升级删除。

`Agent-notify-vX.Y.Z.zip` 是开发与备用包，不是普通用户的主安装入口。解压后可直接运行 `Agent-notify\bin\agentnotify-desktop.exe`，但不会自动创建快捷方式、自启动或部署 Agent 接入；需要接入时按[故障排查](docs/TROUBLESHOOTING.md#双击安装器后-agent-仍未接入)运行包内脚本。ZIP 仍使用同一 `%LOCALAPPDATA%\AgentNotify` 数据目录，不是完全隔离的免安装环境。

## 首次使用

1. 安装完成后启动 `agentnotify-desktop.exe`。
2. 打开 Channels 页，点击「添加渠道账号」扫码登录 ClawBot；如果微信要求数字配对码，在窗口里输入。
3. 看到「等待首条入站消息」后，在微信中给 ClawBot 发送任意一条消息；状态变成「配对已完成」才算建立主动推送会话。
4. 在 Channels 页的「测试发送」里选择这个账号，发送一条测试通知，确认微信能收到。
5. 需要引用续聊时，到 Settings → 回复 开启「引用回复」，说明见[微信引用回复](#微信引用回复)。

主窗口的关闭按钮只把窗口隐藏到托盘，AgentNotify 继续在后台运行；完全退出请使用托盘菜单的「退出」，或在 Settings 中退出。

## 更新与回滚

在 Settings → 更新 里点「检查更新」查询[发布仓库](https://github.com/srafyhucl-cpu/agent-notify-releases/releases)的最新版本。安装器通道会下载安装包，校验 SHA256、PE 版本与 Authenticode 签名者指纹，再静默安装并自动重启。安装器缺失或启动失败时才回退 ZIP；ZIP 会先验证 `RELEASE-MANIFEST.json` / `.p7s`、完整文件集合和逐文件哈希，再执行替换。ZIP 文件替换成功后**不会自动重启 AgentNotify**，需手动完全退出并重新打开。

- Stable 正式渠道不接受缺清单、清单签名无效、指纹不符、版本不匹配、文件集合/哈希不一致或校验失败的包；失败不会触碰已安装文件。
- Beta 仅在 ZIP 中两个清单控制文件同时缺失时兼容旧开发包；半缺失、清单损坏或签名无效仍会拒绝。
- 2.0.0–2.0.4 的更新器会误拒 32 位 Inno Setup 安装器存根，无法直接升级到 2.0.5 及以后版本。请先手动下载并覆盖安装一次 `Agent-notify-Setup-v2.0.5.exe` 或更高版本。
- 手动安装时，从发布仓库下载 `Agent-notify-Setup-vX.Y.Z.exe`，先按[安装校验](#安装)核对摘要和签名者，再覆盖安装。
- 更新失败、ZIP 回退和回滚到 v1.17.0 的详细步骤见[故障排查](docs/TROUBLESHOOTING.md#升级失败与回滚)。

回滚安装器保留原 AppId，会覆盖回原安装目录；配置、凭据、历史与 SQLite 都不会因覆盖安装自动删除。

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

设置项（勿扰、通知冷却、引用回复、路由有效期、更新通道、开机启动等）存在 SQLite 中，通过 Settings 页修改；ClawBot 凭据由 Windows 凭据管理器保存，目标名以 `AgentNotify/` 开头，后接不可逆账号摘要。迁移只读旧目录，不改写也不删除 `%USERPROFILE%\.config\agent-notify\` 里的文件。

`agentnotify-ingress.exe` 只接收版本化 Agent 事件；面向用户的能力只有只读自检 `--doctor`（输出 JSON 报告：命名管道是否在监听、spool 积压与隔离数）与 `--ping`（一行结论），两者都不提交事件、不写盘，退出码 0 正常、1 异常，供无头环境和远程排查探活。

## 卸载与彻底清理

### 普通卸载

先从托盘完全退出 AgentNotify，再在 Windows「设置 → 应用 → 已安装的应用」中卸载。普通卸载会：

- 删除安装目录、开始菜单/桌面快捷方式和自启动快捷方式；
- 从 Codex `notify` 链中移除 AgentNotify 入口，并尽可能还原先前 notify；
- 删除 AgentNotify 写入的 Antigravity Hook/启动器、Devin Stop Hook/回复扩展和 Command Code mod；
- 保留第三方 notify、自定义 matcher 以及其它用户文件。

### 普通卸载不会删除

- `%LOCALAPPDATA%\AgentNotify` 中的 SQLite 状态库、日志、spool 和更新备份；
- Windows 凭据管理器中的 ClawBot 凭据；
- `%USERPROFILE%\.config\agent-notify` 旧版只读迁移来源；
- `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts` OpenCode 插件。

### 手动彻底清理

彻底清理会永久删除登录状态、设置、历史和引用路由，操作前必须备份：

1. 打开 Channels 页，对每个 ClawBot 账号点「退出账号」。该操作会删除应用管理的 token、bot/recipient 标识和会话上下文，但账号摘要与历史仍留在 SQLite；
2. 从托盘完全退出并运行普通卸载；
3. 删除遗留 OpenCode 插件：`%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`；
4. 删除 `%LOCALAPPDATA%\AgentNotify`；
5. 如不再需要回滚，删除 `%USERPROFILE%\.config\agent-notify`；
6. 打开「控制面板 → 凭据管理器 → Windows 凭据 → 通用凭据」，删除所有以 `AgentNotify/` 开头的条目。

正常卸载优先使用；只有明确不再需要账号、历史和旧数据时才执行彻底清理。

## 开发

要求：Windows 10/11 x64、Rust stable、Visual Studio 2022 Build Tools 的 C++ 桌面开发工具与 Windows SDK、WebView2 Runtime、Node.js 22/npm 10、Tauri CLI 2、Windows PowerShell 5.1+。Go 1.26+ 只在执行冻结的 Go 1.x 回滚门禁时需要；Inno Setup 6 与签名工具只在本地构建正式安装器时需要。

```powershell
git clone https://github.com/srafyhucl-cpu/agent-notify.git
cd agent-notify

# 缓存放在非系统盘
$env:npm_config_cache = 'D:\Temp\npm-cache'
$env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target'

npm ci
npm --prefix .\apps\desktop-ui ci
cargo install tauri-cli --version "^2.0.0" --locked

# 修改 Rust bridge 命令后重新生成并提交 types.ts
cargo run --locked -p agentnotify-desktop --bin export-bindings --target x86_64-pc-windows-msvc

# 启动 Vite、编译 Rust 并打开 Tauri 桌面端
cargo tauri dev --config .\hosts\desktop-tauri\tauri.conf.json
```

完整门禁：

```powershell
node_modules\.bin\tsc.cmd --noEmit
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

`tools\test.ps1` 覆盖 Go 遗留、根 TypeScript、脚本、签名门禁与隔离冒烟，**不是仓库全部测试**；Rust 与 UI 必须分别运行对应门禁。`tools\lint.ps1` 校验脚本语法、workflow 纯 ASCII、Actions SHA 固定和版本一致性，版本唯一来源是仓库根 `VERSION`。正式发布还需要签名材料和 Inno Setup；Release 先 validate/build，再由受保护 `main` 上的可复用 workflow 创建 Draft、复核并发布源码仓与客户端更新仓。详细流程见[贡献指南](CONTRIBUTING.md)。

## 文档

- [架构说明](docs/ARCHITECTURE.md)
- [故障排查](docs/TROUBLESHOOTING.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [代码签名与发布信任](docs/code-signing.md)
- [更新日志](CHANGELOG.md)

## License

[MIT License](LICENSE) © 2026 AgentNotify contributors
