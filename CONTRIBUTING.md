# 贡献指南

感谢参与 AgentNotify。本项目中文优先，Issue、PR、提交说明与代码注释使用中文，Conventional Commits 的 type 和 scope 保持英文。

## 环境要求

- Windows 10 / 11 x64
- Rust stable（版本见 `rust-toolchain.toml`）：2.0 核心、桌面端与 Hook 的主语言
- Visual Studio 2022 Build Tools 的“使用 C++ 的桌面开发”和 Windows SDK：编译 Tauri 宿主
- Microsoft Edge WebView2 Runtime：运行桌面界面
- Node.js 22 与 npm 10：桌面 UI、OpenCode 插件与 Devin 扩展
- Tauri CLI 2：本地启动桌面开发窗口；发布构建不依赖全局 CLI
- Go 1.26+（最低版本以 `go.mod` 的 `go` 行为准）：只跑冻结的 Go 1.x 回滚门禁时需要
- Windows PowerShell 5.1+，用于门禁与构建脚本
- Inno Setup 6 与签名工具，仅本地构建正式安装器时需要

构建缓存和临时目录不要放到 C 盘。推荐：

```powershell
$env:npm_config_cache = 'D:\Temp\npm-cache'
$env:GOPATH = 'D:\Temp\agent-notify-go'
$env:GOMODCACHE = 'D:\Temp\agent-notify-go\pkg\mod'
$env:GOCACHE = 'D:\Temp\agent-notify-go\build'
$env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target'
```

## 本地开发

### 从克隆到启动桌面端

以下命令在仓库根目录执行。`CARGO_HOME`、`CARGO_TARGET_DIR` 和 npm 缓存应继续使用前文配置，避免落到 C 盘。

```powershell
git clone https://github.com/srafyhucl-cpu/agent-notify.git
cd agent-notify
npm ci
npm --prefix .\apps\desktop-ui ci

# 仅首次需要；Tauri CLI 2 用于本地 dev 窗口
cargo install tauri-cli --version "^2.0.0" --locked

# 修改 Rust bridge 命令后才需要重新生成；生成结果必须提交
cargo run --locked -p agentnotify-desktop --bin export-bindings --target x86_64-pc-windows-msvc

# Tauri 会按 tauri.conf.json 自动启动 Vite，再编译并打开桌面端
cargo tauri dev --config .\hosts\desktop-tauri\tauri.conf.json
```

如果只改桌面 UI，也可以先运行 `npm --prefix .\apps\desktop-ui run dev`，再在浏览器打开
`http://localhost:1420`；这条路径不会启动 Rust 运行时，因此不能验证命名管道、凭据、SQLite 或更新器。

### 开发门禁

```powershell
# 根 TypeScript / 插件、Go 遗留、脚本和隔离冒烟
node_modules\.bin\tsc.cmd --noEmit
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1

# Rust fmt / clippy / test
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1

# UI bridge、类型、Vitest、构建、Playwright 与 Rust 联合门禁
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1

# 脚本、workflow ASCII 与版本一致性
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
```

- `tools\test.ps1` **不是仓库全部测试**：它覆盖 Go 遗留、根 TypeScript、脚本、签名门禁和隔离冒烟，不替代 Rust 或 UI 门禁。
- `tools\test.ps1` 需要根 `node_modules`；未修改插件时可以运行 `-SkipTypeScript`，但发布前不能跳过。
- `tools\rust\gate.ps1` 会按可用内存限制 cargo 并发；需要提速时用 `-Jobs N` 或 `CARGO_BUILD_JOBS` 覆盖。
- 修改 `cmd\agent-notify\agent-notify.manifest`（Go 版遗留）后，运行 `go generate ./cmd/agent-notify`，并提交重新生成的三个 `rsrc_windows_*.syso`。

## Go 1.x 遗留代码（回滚窗口保留）

2.0.0 起正式交付切换为 Rust + Tauri 桌面版，但 Go 版源码**刻意保留**，边界如下：

- **保留清单**：`cmd/`、`internal/`、`go.mod` / `go.sum`、`plugin/agent-notify.ts`（Go 版 OpenCode 插件）、`plugin/devin-extension/`（V1 扩展）、`plugin/commandcode-mod/`（V1 mod）、根目录 `install.ps1`（Go 版源码安装）、`tests/smoke.ps1` 与 `tests/rollback-smoke.ps1` 的 Go 链路。它们**不进正式发布包**：安装器只装 5 个 Rust 二进制与 v2 插件 / 扩展 / mod。
- **冻结状态**：Go 侧代码与测试**不再更新、不再新增**（回滚链路自身损坏需修复时除外），本目录的演进只发生在 Rust / TypeScript 侧。
- **为什么保留**：支撑回滚窗口——用户随时可退回上一稳定版 `Agent-notify-Setup-v1.17.0.exe`；Go 侧保持可构建、可测试，回退链路才完好（`tests\rollback-smoke.ps1` 持续验证回滚后旧版可用、旧文件零改动）。
- **门禁不豁免**：存量 `go test ./...`、`go vet ./...`、`gofmt` 与 CI `govulncheck` 仍必须全绿（保证随时可回滚）；最低 Go 版本以 `go.mod` 的 `go` 行为唯一来源。
- **何时可删**：`docs/superpowers/specs/2026-09-19-windows-rust-cutover.md` 第 8 节的 5 个条件**同时满足**、且用户明确同意结束回滚窗口后，作为独立变更重新评审删除（需同时给出迁移兼容层的移除方案）。在此之前不删、不改其行为、不顺手重构。
- **注意双职责文件**：`uninstall.ps1` 同时服务 2.0 安装器（`-HooksOnly` 清理接入）与 Go 版完整卸载，`tools/hook-config.ps1` 也随 2.0 安装包分发，**都不在可删清单里**。

## 提交前检查

- [ ] `tools\rust\gate.ps1` 全绿（fmt / clippy / test）
- [ ] `tools\ui\gate.ps1` 全绿（bridge / typecheck / Vitest / build / Playwright / Rust）
- [ ] `go test ./...`、`go vet ./...` 与 `gofmt -l cmd internal` 无输出（Go 遗留代码门禁）
- [ ] `node_modules\.bin\tsc.cmd --noEmit` 全绿
- [ ] `tools\lint.ps1` 全绿
- [ ] `tools\test.ps1` 全绿
- [ ] 新增或修改 `.ps1` / `.psm1` / `.psd1` 时保留 UTF-8 BOM + CRLF
- [ ] `.github/workflows/*.yml` 保持纯 ASCII，第三方 Actions 固定到完整 commit SHA
- [ ] 正式 ZIP 含 `RELEASE-MANIFEST.json` / `.p7s`，清单文件集合与哈希通过发布门禁
- [ ] 没有提交 token、证书私钥、真实账号标识、平台消息 ID 或未经脱敏的个人日志
- [ ] 行为变化已写入 `CHANGELOG.md`
- [ ] 没有重新引入旧品牌、旧模块或旧路径兼容层

## 架构约束

1. 2.0 发布物是 5 个二进制：`agentnotify-desktop.exe`（Tauri 桌面端）、`agentnotify-ingress.exe`（事件入口）与三个 Hook exe（`agentnotify-codex-hook` / `-antigravity-hook` / `-devin-hook`）；正式 ZIP 还必须带签名发布清单，清单覆盖全部普通文件。
2. OpenCode 插件、Command Code mod 与三个 Hook 只通过 `agentnotify-ingress.exe` 提交版本化事件（当前用户命名管道，离线落 spool）；Hook 用 `AGENT_NOTIFY_INGRESS`、插件与 mod 用 `AGENT_NOTIFY_INGRESS_BIN` 定位入口。
3. Codex 接入必须保留 `codex-computer-use.exe` 的原始透传。
4. 所有用户可配置项统一使用 `AGENT_NOTIFY_*`。
5. 状态数据存 `%LOCALAPPDATA%\AgentNotify`（SQLite WAL、spool、logs），凭据进 Windows 凭据管理器；`%USERPROFILE%\.config\agent-notify` 只作为旧数据只读迁移来源。
6. ClawBot endpoint、凭据字段和发送消息结构属于外部契约。
7. 运行日志脱敏后写入 `logs\runtime.log`，日志写入失败不得影响推送与退出码。
8. 发布 exe 保持 `windowsgui`，不得恢复可见控制台闪烁；命令行诊断输出需显式接回父控制台。
9. 不保留旧品牌的迁移读取、别名命令或兼容文件。
10. OpenCode 插件与 Command Code mod 靠安装器写入的 `BAKED_INGRESS` 定位入口，修改路径解析时必须同步 `tools\hooks\install-opencode-v2.ps1`、`tools\hooks\install-commandcode-v2.ps1` 与冒烟用例。

完整契约见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## Issue、分支与 PR 流程

1. 先搜索现有 Issue；Bug 按 `docs/TROUBLESHOOTING.md` 最小化复现并移除日志中的个人信息；安全漏洞只走 GitHub 私密报告。
2. 大型功能先创建或更新设计文档，写清目标、边界、兼容策略和验收标准，再开始实现。
3. 从最新 `main` 创建短生命周期分支；一个 PR 只处理一个可独立验证的问题，不顺带重构无关模块。
4. 提交前运行“提交前检查”全部适用门禁；只暂存本 PR 相关文件。
5. PR 说明用户可见变化、风险、真实链路验收和未完成项，并关联 Issue；维护者按安全、兼容、可测试性和文档一致性评审。
6. 评审意见解决后压缩无意义中间提交，但保留能解释设计演进的提交；不使用 force push 掩盖未评审变化。

当前不要求 DCO 或 CLA；若未来引入，会先在本文件更新流程，不追溯要求历史贡献者补签。

## 提交规范

```text
feat: 增加 ClawBot 扫码登录
fix: 修复 PowerShell 重定向时 stdout 丢失
refactor: 将事件入口拆到 agentnotify-ingress
test: 增加 Codex 配置恢复测试
docs: 重写安装与排障文档
ci: 增加 Rust 工作区门禁
```

常用 type：`feat` `fix` `docs` `refactor` `test` `chore` `ci`。

## 目录结构

```text
apps/desktop-ui/      桌面 UI（React + Vite，Tauri WebView）
apps/ingress/         事件入口（版本化协议 + 命名管道 + spool）
apps/hooks/           Codex / Antigravity / Devin Hook exe
crates/               领域、应用、Agent 适配器、渠道、存储、运行时
hosts/desktop-tauri/  桌面宿主、更新器、凭据与平台层
installer/            Inno Setup 安装器脚本
plugin/               OpenCode 插件、Command Code mod、Devin 扩展（v2）
tools/                lint、test、build-release、sync-version、接入安装助手
tests/                冒烟、签名门禁、卸载清理与安装器结构回归
cmd/ internal/        Go 版运行时（回滚窗口保留）
docs/                 架构与排障
```

## 发布流程

1. 递增仓库根 `VERSION`（唯一版本来源，SemVer）。
2. 运行 `powershell -File tools\sync-version.ps1` 把版本同步到各发布位置并提交（`tauri.conf.json`、`Cargo.toml` 的 workspace 版本、Devin 扩展、桌面 UI 包）。
3. 在 `CHANGELOG.md` 顶部增加对应版本段落。
4. 提交并推送 `main`，确认 CI、CodeQL、`tools\lint.ps1`、`tools\test.ps1`、`tools\rust\gate.ps1` 与
   `tools\ui\gate.ps1` 全绿；`main` 与 `v*` tag 必须已启用保护规则。
5. 配置 `release` Environment 的签名 Secret（`AGENT_NOTIFY_SIGN_PFX_BASE64` /
   `AGENT_NOTIFY_SIGN_PFX_PASSWORD`），并确认仓库 Secret `RELEASE_REPO_TOKEN` 只进入发布阶段的权限边界；
   不要把 PFX 写入仓库或 artifact。
6. 打 tag：`git tag vX.Y.Z`，推送 tag：`git push origin vX.Y.Z`。Release workflow 先在精确 tag 上
   validate，再在带 PFX 的 build job 生成安装器、ZIP 清单和外层摘要；源码与客户端更新仓的 Draft 发布
   由受保护 `main` 上的 `.github/workflows/publish-release.yml@main` 接收 artifact 完成。
7. 发布后核对两个仓库的资产名称、SHA256、清单和签名；已发布 Release 不得用 workflow `--clobber` 覆盖，
   失败时保留 Draft 并按 `tools\publish-release.ps1` 的显式补发流程处理。

版本一致性由 `tools\check-version.ps1` 校验，`tools\lint.ps1` 与 Release workflow 都会调用它。
