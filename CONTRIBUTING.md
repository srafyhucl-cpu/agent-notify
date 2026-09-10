# 贡献指南

感谢参与 linkWeixin。本项目**中文优先**：Issue、PR、提交信息、代码注释请使用中文
（type/scope 等约定关键字保持英文）。

## 环境要求

- Windows 10 / 11
- Windows PowerShell 5.1（最低支持运行时；CI 会做 5.1 语法兼容检查）
- 可选：Python 3（仅悬浮窗 pythonw 启动链使用；不装自动回退 vbs）
- 可选：Node.js 20+（仅 opencode 插件类型检查）

## 本地开发

```powershell
git clone <repo-url>
cd linkWeixin
npm install   # 仅类型检查依赖；不跑插件可跳过

# 三件套
powershell -NoProfile -ExecutionPolicy Bypass -File tools\lint.ps1    # PSScriptAnalyzer
powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1    # 单测 + 冒烟
npx tsc --noEmit                                                      # 插件类型检查（改了 plugin/ 才必须）
```

`tools\test.ps1` 首次运行会自动把 Pester / PSScriptAnalyzer 装到 CurrentUser。

冒烟测试不联网、不碰真实配置（沙箱目录 + dry-run + 临时 marker）。

## 提交前检查清单

- [ ] `tools\lint.ps1` 全绿（排除规则见 `PSScriptAnalyzerSettings.psd1`，新增排除必须附理由）
- [ ] `tools\test.ps1` 全绿
- [ ] 改动 `plugin/` 时 `npx tsc --noEmit` 全绿
- [ ] 新增/修改 `.ps1/.psm1/.psd1` 保持 **UTF-8 BOM + CRLF**（`.editorconfig` 已配好，
      仓库守卫测试会硬检，忘了会被 `Repo.Tests.ps1` 拦下）
- [ ] 行为变化已写进 `CHANGELOG.md`
- [ ] 不破坏外部契约（见下）

## 外部契约（改动红线）

以下内容属于对外契约，改动需特别谨慎并在 CHANGELOG 说明：

1. 安装到 `%USERPROFILE%\bin` 的入口文件名与位置（`notify-ai.ps1` / `codex-notify.ps1` /
   `codex-notify-watch.ps1` / `notify-toggle.ps1` / `linkweixin-widget.ps1` /
   `run-hidden.vbs` / `widget-detached.py`）
2. 全部环境变量与默认值（见 README 配置表）
3. marker 路径与语义（存在 = 关）
4. `%TEMP%\opencode\` 下日志/状态文件路径
5. 推送行为：标题前缀、html 渲染、（除 DryRun 外）任何失败静默且 `exit 0`
6. 无窗口承诺：agent 侧子进程不得闪控制台 / Windows Terminal 页签

完整清单见 `docs/ARCHITECTURE.md`。

## 提交规范

Conventional Commits，type/scope 英文、描述中文：

```text
feat: 安装时检测 pythonw，悬浮窗双启动链接线
fix: PushPlus 请求超时 10s→20s，适配慢代理链路
docs: README 重组与架构/排障文档拆分
test: Pester 单测与仓库 BOM/CRLF 守卫
```

常用 type：`feat` `fix` `docs` `refactor` `test` `chore` `ci`。

## 分支模型

trunk-based：`main` 保持可发布状态；功能/修复走短分支
`feat/<主题>`、`fix/<主题>`，合并前保证三件套通过。
仓库私有阶段允许维护者直接提交 `main`。

## 目录结构

```text
src/                 安装到 ~/bin 的运行时（整树拷贝，结构原样保留）
├── *.ps1            入口脚本（薄封装）
├── lib/LinkWeixin/  共享逻辑模块（版本号唯一来源）
└── widget/          悬浮窗部件（入口 dot-source，共享 $ctx）
plugin/              opencode 全局插件（TypeScript）
tests/
├── smoke.ps1        端到端冒烟（含沙箱安装/卸载）
└── unit/            Pester 单测与仓库守卫
tools/               本地与 CI 共用入口（lint / test）
docs/                架构与排障文档（含 specs/ 设计记录）
```

## 发布流程（维护者）

1. 修改 `src/lib/LinkWeixin/LinkWeixin.psd1` 的 `ModuleVersion`（唯一版本来源）
2. 在 `CHANGELOG.md` 顶部新增对应版本段落（Keep a Changelog 格式）
3. 提交并推送 `main`，确认 CI 全绿
4. 打 tag 并推送：`git tag vX.Y.Z; git push origin vX.Y.Z`
5. Release 工作流会校验 tag == psd1 版本，然后构建 `linkWeixin-vX.Y.Z.zip` +
   `SHA256SUMS.txt` 并创建 GitHub Release（notes 取 CHANGELOG 段落）
