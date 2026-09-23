# 贡献指南

感谢参与 AgentNotify。本项目中文优先，Issue、PR、提交说明与代码注释使用中文，Conventional Commits 的 type 和 scope 保持英文。

## 环境要求

- Windows 10 / 11
- Rust stable（版本见 `rust-toolchain.toml`）：2.0 核心、桌面端与 Hook 的主语言
- Go 1.26+（最低版本以 `go.mod` 的 `go` 行为准）：只用于仓库保留的 Go 版运行时及其门禁（回滚窗口）
- Node.js 22：桌面 UI、OpenCode 插件与 Devin 扩展的类型检查和测试
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

```powershell
git clone https://github.com/srafyhucl-cpu/agent-notify.git
cd agent-notify
npm ci

# 静态检查
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
node_modules\.bin\tsc.cmd --noEmit

# Rust 门禁：cargo fmt / clippy / test
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1

# Go 门禁 + 插件状态机测试 + 隔离冒烟
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1

# 桌面 UI 类型检查与单测
cd apps\desktop-ui
npm ci
npm run typecheck
npm test
```

- `tools\test.ps1` 需要 `node_modules`；未修改插件时可以运行 `-SkipTypeScript`，但发布前应跑完整套件。
- `tools\rust\gate.ps1` 在内存较小的机器上会自动限制 cargo 并发（避免链接期打爆分页文件），需要提速时用 `-Jobs N` 或 `CARGO_BUILD_JOBS` 覆盖。
- 修改 `cmd\agent-notify\agent-notify.manifest`（Go 版遗留）后，在仓库根目录运行 `go generate ./cmd/agent-notify`，并提交重新生成的三个 `rsrc_windows_*.syso`。

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
- [ ] `go test ./...` 与 `go vet ./...` 全绿（Go 遗留代码门禁）
- [ ] `node_modules\.bin\tsc.cmd --noEmit` 全绿
- [ ] `tools\lint.ps1` 全绿
- [ ] `tools\test.ps1` 全绿
- [ ] 新增或修改 `.ps1` / `.psm1` / `.psd1` 时保留 UTF-8 BOM + CRLF
- [ ] `.github/workflows/*.yml` 保持纯 ASCII
- [ ] 行为变化已写入 `CHANGELOG.md`
- [ ] 没有重新引入旧品牌、旧模块或旧路径兼容层

## 架构约束

1. 2.0 发布物是 5 个二进制：`agentnotify-desktop.exe`（Tauri 桌面端）、`agentnotify-ingress.exe`（事件入口）与三个 Hook exe（`agentnotify-codex-hook` / `-antigravity-hook` / `-devin-hook`）。
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
4. 提交并推送 `main`，确认 CI 全绿。
5. 打 tag：`git tag vX.Y.Z`。
6. 推送 tag：`git push origin vX.Y.Z`。Release workflow 校验 tag 与 `VERSION` 一致并强制签名，构建安装器与 ZIP 后发布到源码仓库，再镜像到 `srafyhucl-cpu/agent-notify-releases`（客户端更新源）。

版本一致性由 `tools\check-version.ps1` 校验，`tools\lint.ps1` 与 Release workflow 都会调用它。
