# Agent-notify Windows 标准安装与升级设计

## 背景

当前普通用户需要先解压 ZIP，再手动运行 `install.ps1`。实际分发中，用户容易只启动 `agent-notify.exe`，导致 OpenCode、Codex、Antigravity、Devin 均未接入。该流程不符合 Windows 桌面软件的使用习惯。

本设计将普通用户的安装入口改为标准 Windows 安装器，同时保留现有便携包和内部安装脚本。

## 目标

1. 普通用户下载一个 `Agent-notify-Setup-vX.Y.Z.exe`，双击后即可完成安装。
2. 安装过程不要求用户打开 PowerShell，不显示命令行窗口。
3. 安装结束页默认勾选“启动 Agent-notify”，启动后自动完成各 Agent 接入。
4. 保留悬浮窗“升级”按钮。点击后静默下载并校验新版安装器，然后直接启动安装程序。
5. 支持从任意旧版本的 ZIP 自动更新流程平滑升级到新的安装器流程。
6. 支持从 Windows“已安装的应用”中标准卸载，并清理 Agent-notify 自己写入的 Hook。
7. 不破坏旧配置、旧凭据、推送历史、引用路由和用户自定义 Agent 配置。

## 非目标

1. 不把 ClawBot 扫码登录做成无人值守流程。没有本机凭据时仍必须显示扫码界面。
2. 不自动安装或重启 OpenCode、Codex、Antigravity、Devin。
3. 不移除 ZIP 发布物。ZIP 继续用于便携使用、开发和过渡升级。
4. 不在本阶段引入后台常驻更新服务。

## 用户流程

### 首次安装

1. 用户从 Release 下载 `Agent-notify-Setup-vX.Y.Z.exe`。
2. 双击安装器，安装向导默认按当前用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，不要求管理员权限。
3. 安装器复制运行程序、OpenCode 插件、Devin 扩展和配置辅助文件，创建开始菜单入口及可选桌面快捷方式。
4. 安装完成页默认勾选“启动 Agent-notify”。
5. 用户点击完成后，已安装的 `agent-notify.exe` 启动。
6. 首次启动检测到尚未完成初始化时，在后台静默执行 Agent 接入配置。
7. 接入完成后显示悬浮窗。没有 ClawBot 凭据时，显示扫码登录窗口。
8. 接入失败时不在后台假装成功，悬浮窗显示可读错误，“检查修复”提供重试入口。

### 应用内升级

1. 用户点击悬浮窗底部“升级”。
2. 应用检查最新稳定 Release，并向用户确认升级版本。
3. 确认后，应用后台下载 `Agent-notify-Setup-vX.Y.Z.exe` 和 `SHA256SUMS.txt`，不要求用户选择解压目录。
4. 下载完成后严格校验 SHA256，校验失败立即终止并显示错误。
5. 校验通过后启动新版安装器。安装器关闭正在运行的旧版悬浮窗，原地更新文件并保留用户数据。
6. 安装器完成后重新启动悬浮窗。
7. 如果新版安装器缺失，兼容流程回退到现有 ZIP 更新包。

### 卸载

1. 用户在 Windows“已安装的应用”中选择 Agent-notify 并卸载。
2. 安装器在删除程序文件前调用隐藏的清理逻辑，仅移除 Agent-notify 自己写入的 Codex、Antigravity 和 Devin 配置。
3. 安装器删除程序目录、快捷方式和注册表卸载项。
4. ClawBot 凭据、配置、历史及路由默认保留，与现有卸载约定一致。

## 技术方案

### 安装器

使用 Inno Setup 构建 Windows 安装器。

- 安装模式：当前用户安装，`PrivilegesRequired=lowest`。
- 默认目录：`%LOCALAPPDATA%\Programs\Agent-notify`。
- 主要产物：`Agent-notify-Setup-vX.Y.Z.exe`。
- 兼容产物：继续生成 `Agent-notify-vX.Y.Z.zip`。
- 升级识别：AppId 固定，后续版本覆盖安装同一目录。
- 快捷方式：开始菜单必选，桌面和开机启动作为可选项。
- 启动项：完成页默认勾选启动。
- 代码签名：流水线支持 SignTool；未配置证书时允许构建未签名产物，但 README 必须明确 SmartScreen 风险。

### 首次启动接入

安装器负责复制文件，首次启动负责用户级配置。

- 新增初始化完成标记，记录版本和完成时间。
- 无标记时，程序复用 `install.ps1` 中已经验证过的配置逻辑，但在进程内创建隐藏窗口的 PowerShell 子进程执行，用户看不到命令行。
- `install.ps1` 增加仅配置模式，跳过正在运行的 exe 替换，只安装或更新用户级 OpenCode 插件、Devin 扩展和 Hook，并写入插件所需的实际 exe 路径。
- 初始化不下载依赖，不要求管理员权限。
- 成功后写入完成标记，后续启动直接进入悬浮窗。
- 失败时保留详细日志，不写入完成标记；悬浮窗展示错误，用户可点击“检查修复”重试。
- CLI 子命令不触发交互式初始化。

### 更新模块

更新模块增加安装器资产类型，同时保留 ZIP 兼容路径。

- Release 检查优先选择 `Agent-notify-Setup-vX.Y.Z.exe`。
- 下载目录继续使用非 C 盘优先的应用临时目录，并沿用现有超时和大小限制。
- 同时下载并校验 `SHA256SUMS.txt`。
- 安装器启动参数由更新模块统一生成，安装结束后自动重启应用。
- 不通过 shell 拼接命令，所有路径和参数按独立参数传递。
- 旧版客户端仍可从同一个 Release 的 ZIP 资产完成过渡升级。

## 组件边界

| 组件 | 职责 |
|---|---|
| `installer/agent-notify.iss` | Inno Setup 安装、升级、快捷方式和卸载定义 |
| `tools/build-release.ps1` | 继续构建 ZIP，并调用安装器构建流程 |
| `tools/build-installer.ps1` | 定位 ISCC、生成安装器、校验版本和产物 |
| `internal/update` | 检查 Release、下载安装器、校验 SHA256、启动安装器 |
| `cmd/agent-notify` | 首次启动初始化及可测试的命令入口 |
| `install.ps1` | 保留源码安装和便携模式，并复用配置逻辑 |
| `uninstall.ps1` | 保留便携卸载，并为安装器提供 Hook 清理逻辑 |

## 错误处理

1. 下载失败：保留当前版本，提示网络或代理错误。
2. 校验失败：删除下载文件，不启动安装器。
3. 安装器启动失败：当前应用保持运行，写出日志并给出可读提示。
4. 首次初始化失败：悬浮窗仍可打开，显示各 Agent 的真实状态和重试入口。
5. Hook 配置被用户占用：不覆盖无法安全接管的配置，明确指出冲突项。
6. 初始化中断：下次启动重新执行，已成功步骤必须保持幂等。

## 验证

### 自动化

1. 安装器构建脚本能在干净环境中生成 Setup EXE 和 ZIP。
2. 更新模块单测覆盖安装器资产选择、SHA256 校验、参数生成和 ZIP 回退。
3. 启动初始化单测覆盖首次执行、成功标记、失败重试和 CLI 不触发。
4. 运行现有门禁：`go test ./...`、`go vet ./...`、`gofmt -l cmd internal`、`npx tsc --noEmit`。
5. 运行 `tools/test.ps1` 和 `tools/lint.ps1`。

### 真实链路

1. 在干净 Windows 用户环境双击 Setup EXE，确认无命令行窗口、无管理员提示。
2. 点击完成后确认悬浮窗启动，四个 Agent 的接入状态正确。
3. 从旧版 ZIP 安装环境点击“升级”，确认静默下载、校验、安装和自动重启完整成功。
4. 验证卸载后 Hook 和快捷方式被清理，用户凭据、历史和路由保留。
5. 分别验证 OpenCode、Codex、Antigravity、Devin 的真实推送或接入状态。

## 发布约束

1. Release 必须同时保留 Setup EXE、ZIP 和包含两者哈希的 `SHA256SUMS.txt`。
2. 版本号仍以 `internal/app/version.go` 为唯一来源。
3. 安装器版本、应用版本、ZIP 名称和 Release tag 必须一致。
4. 没有代码签名证书时不得宣称已消除 SmartScreen 提示。
