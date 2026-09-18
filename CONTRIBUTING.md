# 贡献指南

感谢参与 AgentNotify。本项目中文优先，Issue、PR、提交说明与代码注释使用中文，Conventional Commits 的 type 和 scope 保持英文。

## 环境要求

- Windows 10 / 11
- Go 1.26+
- Node.js 20+，仅用于 OpenCode 插件类型检查
- Windows PowerShell 5.1+，用于安装器和 smoke 测试

构建缓存和临时目录不要放到 C 盘。推荐：

```powershell
$env:npm_config_cache = 'D:\Temp\npm-cache'
$env:GOPATH = 'D:\Temp\agent-notify-go'
$env:GOMODCACHE = 'D:\Temp\agent-notify-go\pkg\mod'
$env:GOCACHE = 'D:\Temp\agent-notify-go\build'
```

## 本地开发

```powershell
git clone https://github.com/srafyhucl-cpu/agent-notify.git
cd agent-notify
npm ci

# 静态检查
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
node_modules\.bin\tsc.cmd --noEmit

# 单测 + 类型检查 + 隔离 smoke
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

`tools\test.ps1` 需要 `node_modules`；未修改插件时可以运行 `-SkipTypeScript`，但发布前应跑完整套件。

修改 `cmd\agent-notify\agent-notify.manifest` 后，在仓库根目录运行 `go generate ./cmd/agent-notify`，并提交重新生成的三个 `rsrc_windows_*.syso`。

## 提交前检查

- [ ] `go test ./...` 全绿
- [ ] `go vet ./...` 全绿
- [ ] `node_modules\.bin\tsc.cmd --noEmit` 全绿
- [ ] `tools\lint.ps1` 全绿
- [ ] `tools\test.ps1` 全绿
- [ ] 新增或修改 `.ps1` / `.psm1` / `.psd1` 时保留 UTF-8 BOM + CRLF
- [ ] 行为变化已写入 `CHANGELOG.md`
- [ ] 没有重新引入旧品牌、旧模块或旧路径兼容层

## 架构约束

1. 发布运行时只有一个 `agent-notify.exe`。
2. OpenCode 插件只通过 `agent-notify.exe notify` 通信。
3. Codex 接入必须保留 `codex-computer-use.exe` 的原始透传。
4. 所有用户可配置项统一使用 `AGENT_NOTIFY_*`。
5. 配置和凭据默认只写入 `%USERPROFILE%\.config\agent-notify`。
6. ClawBot endpoint、凭据字段和发送消息结构属于外部契约。
7. `push.log` 保持 JSON Lines 格式，解析器应跳过损坏行而不丢整份历史。
8. 发布 exe 保持 `windowsgui`，不得恢复可见控制台闪烁。
9. 不保留旧名称的迁移读取、别名命令或兼容文件。
10. OpenCode 插件靠安装器写入的 `BAKED_BIN` 定位 exe，修改插件路径解析时必须同步更新 `install.ps1` 与 smoke 用例。

完整契约见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 提交规范

```text
feat: 增加 ClawBot 扫码登录
fix: 修复 PowerShell 重定向时 stdout 丢失
refactor: 将运行时统一为 Go 单文件
test: 增加 Codex 配置恢复测试
docs: 重写安装与排障文档
ci: 改为 Go 与 smoke 测试流程
```

常用 type：`feat` `fix` `docs` `refactor` `test` `chore` `ci`。

## 目录结构

```text
cmd/agent-notify/     CLI 入口
internal/agent/       OpenCode / Codex 接入
internal/clawbot/     ClawBot 登录与发送
internal/config/      配置和路径
internal/marker/      开关 marker
internal/notify/      渲染、发送、历史
internal/ui/          Windows 悬浮窗
plugin/               OpenCode 插件
tests/smoke.ps1       隔离安装与 CLI 冒烟
tools/                lint、test、build-release
docs/                 架构与排障
```

## 发布流程

1. 修改 `internal/app/version.go` 的 `Version`。
2. 在 `CHANGELOG.md` 顶部增加对应版本段落。
3. 提交并推送 `main`，确认 CI 全绿。
4. 打 tag：`git tag vX.Y.Z`。
5. 推送 tag：`git push origin vX.Y.Z`。
6. Release workflow 校验 tag 与 Go 版本一致，构建 `Agent-notify-vX.Y.Z.zip` 和 `SHA256SUMS.txt`，再创建 GitHub Release。
