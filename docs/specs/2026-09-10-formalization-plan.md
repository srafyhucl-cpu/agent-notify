# linkWeixin 规范化实施计划

- 对应设计：`docs/specs/2026-09-10-formalization-design.md`（评审通过）
- 执行环境：Windows 11 + PowerShell 5.1；本机已登录 gh（`srafyhucl-cpu`）；Python 3.13 可选
- 执行原则：
  1. 每个阶段以 commit 为边界，先本地验证再进入下一阶段
  2. 阶段 1-5 只动本地仓库与 `~/bin` 安装物；阶段 6 才动 GitHub
  3. spec 附录 A（不变量清单）是行为底线，任何阶段触碰即视为失败
- 写作说明：原计划的 writing-plans 技能未安装，本计划由主 agent 按同一标准手写

## 总览

| 阶段 | 主题 | 产出 | 验证门 | 提交 |
|---|---|---|---|---|
| 0 | 前置准备 | main 分支、noreply 邮箱、.gitignore | 配置就位 | 1 个 chore |
| 1 | 结构迁移 | src/ + plugin/、整树部署、安装记录、smoke 沙箱用例 | smoke 全绿 + 真机重装 + 真推一条 | 2 个 |
| 2 | 模块抽取 | LinkWeixin 模块、入口瘦身、Pester 单测 | 单测 + smoke 全绿 + 双链路真推 | 2 个 |
| 3 | widget 拆分 | widget/ 三文件、pythonw/vbs 双启动链 | 手动全交互清单 | 2 个 |
| 4 | 工程化门禁 | editorconfig/analyzer/lint/tsc | lint + tsc 全绿 | 1 个 |
| 5 | 版本与治理 | 0.1.0、CHANGELOG、治理文件、文档重组 | 版本一致 + 文档无死链 | 3 个 |
| 6 | GitHub 与发布 | CI/Release 工作流、私有仓库、v0.1.0 Release | 规范第 11 节 DoD 全部勾选 | 2 个 + push + tag |

依赖关系：1 → 2 → 3 → 4 → 5 → 6，严格串行。

---

## 阶段 0：前置准备

- [ ] `git branch -m master main`
- [ ] 提交邮箱关联 GitHub（现为占位符 `srafy@example.com`）：
  - `gh api user --jq .id` 取数字 id
  - `git config user.email "<id>+srafyhucl-cpu@users.noreply.github.com"`（仓库级，不动全局）
  - `git config user.name` 保持 `srafy`
- [ ] `.gitignore` 追加 `.freebuff/`
- 提交：`chore: 忽略 .freebuff 本地工具目录`

验证门：`git config user.email` 输出 noreply 地址；`git branch --show-current` = main。

回滚：`git branch -m main master`；`git config --unset user.email`。

---

## 阶段 1：结构迁移（行为不变）

### 1.1 迁移与引用更新

- `git mv scripts src`
- `git mv opencode-plugin plugin`
- 全仓更新路径引用：`scripts\` → `src\`、`opencode-plugin\` → `plugin\`
  - 涉及：`install.ps1`、`uninstall.ps1`、`tests\smoke.ps1`、`README.md`（本阶段只做最小改动，完整重写在第 5 阶段）
  - 不动：`codex-notify-watch.ps1` 的 `$PSScriptRoot` 派生、插件内部相对逻辑

### 1.2 install.ps1 升级

- 新增参数：`-SkipShortcuts`、`-SkipWidgetLaunch`（已有 `-SkipScheduledTask`、`-SkipCodexConfig` 保留）
- 拷贝改为整树：
  - `Copy-Item -Path (Join-Path $RepoRoot 'src\*') -Destination $InstallDir -Recurse -Force`
  - 注意 PowerShell 坑：`Copy-Item 目录 $目标` 会多套一层，必须用 `src\*`
- 写安装记录 `$InstallDir\linkweixin-install.json`：
  ```json
  {
    "name": "linkWeixin",
    "version": "<读 psd1 ModuleVersion，不存在则 dev>",
    "installedAt": "<ISO 8601>",
    "launcher": "vbs",
    "files": ["notify-ai.ps1", "codex-notify.ps1", "...", "lib/LinkWeixin/LinkWeixin.psd1"]
  }
  ```
  - `files` 为相对 InstallDir 的正斜杠路径；由源树枚举生成
- 自检（`$wants`）更新为 `src\` 下：5 个入口 + `run-hidden.vbs` + `widget-detached.py` + `plugin\notify-pushplus.ts`（第 2、3 阶段再把 lib/、widget/ 文件加入）
- `version` 读取逻辑一次到位：psd1 存在则解析 `ModuleVersion`，否则 `'dev'`（阶段 2 落 psd1 后无需再改）
- `launcher` 字段本阶段固定 `'vbs'`，阶段 3 改为实际检测结果

### 1.3 uninstall.ps1 升级

- 读取安装记录逐文件删除：
  - 校验相对路径不逃逸 `$InstallDir`（`[IO.Path]::GetFullPath` 前缀比对）
  - 删除后自底向上清理空目录（只动 `$InstallDir` 内、且安装前不存在的目录）
  - 删除记录文件本身
- 无记录（老装机）：按当前 `src\` 树枚举兜底
- 进程清理改为限定安装目录：命令行匹配 `[regex]::Escape($InstallDir)` 且含 `linkweixin-widget.ps1` 或 `widget-detached.py`（powershell / pythonw 两种宿主），避免误杀沙箱测试时的真实窗体
- 新增 `-SkipShortcuts`、`-SkipCodexConfig`、`-SkipScheduledTask`；快捷方式/任务/codex 还原逻辑保持
  - `-SkipScheduledTask` 是必要的沙箱保护：否则管理员权限下跑冒烟测试会误删真实计划任务

### 1.4 smoke.ps1 更新

- 全部 `scripts\` 引用改 `src\`；`linkweixin-widget.ps1` 大小写扫描路径同步
- 新增沙箱安装/卸载用例：
  1. 临时 `InstallDir`/`PluginDir` 跑 `install.ps1`（带全部 Skip 开关）
  2. 断言：入口/启动器文件齐全、plugin 已拷、记录存在且 `files/version/launcher/installedAt` 齐全
  3. 跑 `uninstall.ps1`（带 `-SkipShortcuts -SkipCodexConfig -SkipScheduledTask`）
  4. 断言：文件清零、记录已删、`lib/` 等空目录被剪枝、无残留
- 保留现有全部断言

### 1.5 验证与提交

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tests\smoke.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -SkipScheduledTask   # 真机重装（无需管理员）
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-ai.ps1" -DryRun -Title t -Summary "b1 b2"
```

- [ ] 手动：真推一条微信；重启悬浮窗确认行为不变；重启 opencode 跑一个任务确认推送（用户确认）
- 提交 1：`refactor: 目录迁移 src/plugin，安装/卸载改整树拷贝+安装记录`
- 提交 2：`test: 冒烟新增沙箱安装/卸载用例`

回滚：`git revert <阶段1提交>` + 重跑 `install.ps1`。

---

## 阶段 2：模块抽取（行为等价）

### 2.1 模块骨架

- `src/lib/LinkWeixin/LinkWeixin.psd1`：
  - `ModuleVersion = '0.1.0'`、`RootModule = 'LinkWeixin.psm1'`、`PowerShellVersion = '5.1'`
  - 新 GUID；Author/Copyright = `linkWeixin contributors`；Description 中文
  - `FunctionsToExport` 显式 7 项（见 2.2）
- `src/lib/LinkWeixin/LinkWeixin.psm1`：排序 dot-source `Private\*.ps1`，`Export-ModuleMember`
- 全部文件 UTF-8 BOM + CRLF

### 2.2 函数搬迁表

| 新函数（Private/） | 来源（现文件:行） | 注意事项 |
|---|---|---|
| `Get-LinkWeixinPaths.ps1` | toggle 25-26、wrapper 21、widget 82-87 的路径拼接 | 返回 hashtable；环境变量在函数内实时读；默认值与原实现逐字一致 |
| `Format-NotifySummary.ps1` | notify-ai.ps1 29-65 | 渲染逻辑逐字搬迁，纯函数无副作用 |
| `Send-PushPlusNotification.ps1` | notify-ai.ps1 67-114 主体 | token 读取、UTF-8 字节数组、10 秒超时、stderr 文案、`exit 0` 由入口做，函数只管发送 |
| `ConvertFrom-CodexNotifyEventArgs.ps1` | codex-notify.ps1 60-81 | 标题 30 字截断、摘要首个 `last-assistant-message`、坏 JSON 跳过 |
| `Get-CodexComputerUseExe.ps1` | codex-notify.ps1 26-32 | 动态最新目录排序逻辑不动 |
| `Set-NotifyMarker.ps1` / `Test-NotifyMarker.ps1` | toggle 31-43、widget 114-141 | On/Off/Flip 语义；目录自动创建；返回 `ON`/`OFF` 字符串 |

### 2.3 入口瘦身

- `notify-ai.ps1`：参数签名（`Title/Summary/MaxChars/DryRun/NoStdin`）与 stderr 文案不变；模块加载失败 → stderr + `exit 0`
- `codex-notify.ps1`：保留透传顺序、调试日志格式、`-Summary` 空则省略参数、动态找 exe；调用 notify-ai 仍走 `powershell -File` 子进程（**不**改为同进程调用，保隔离）
- `notify-toggle.ps1`：参数（含 `-MarkerPath` 别名、`-Agent`）与回显格式不变
- `codex-notify-watch.ps1`：**不引入模块**（无共享逻辑，减少依赖面；计划内明确例外）
- `linkweixin-widget.ps1` 本阶段只替换 marker 读写为模块函数，拆分在第 3 阶段

### 2.4 Pester 单测

- `tests/unit/LinkWeixin.Tests.ps1`：
  - `Format-NotifySummary`：标题加粗、列表转 `•`、代码块整段剔除、行内代码去反引号、HTML 转义、按句截断（含 100 字阈值）、连续空行折叠、首尾 `<br>` 清理
  - `ConvertFrom-CodexNotifyEventArgs`：标题截断 30+`…`、摘要原文透传、非 JSON 参数跳过、多种参数顺序
  - `Test/Set-NotifyMarker`：On/Off/Flip 三态、目录自动创建、临时路径隔离
  - `Get-LinkWeixinPaths`：环境变量覆盖 6 个路径、默认值断言
  - 模块清单：`ModuleVersion` 合法、`PowerShellVersion`='5.1'、导出函数与实现一一对应
- `tests/unit/Repo.Tests.ps1`：仓库全部 `.ps1/.psm1/.psd1` 带 UTF-8 BOM（遍历排除 .git/node_modules/dist）

### 2.5 验证与提交

```powershell
Install-Module Pester -MinimumVersion 5.5.0 -Force -SkipPublisherCheck -Scope CurrentUser
Invoke-Pester tests\unit -Output Detailed
powershell -NoProfile -ExecutionPolicy Bypass -File tests\smoke.ps1
```

- [ ] 真推一条；opencode + codex 双链路各真跑一次（用户确认）
- 提交 1：`refactor: 抽取 LinkWeixin 模块，入口脚本瘦身`
- 提交 2：`test: Pester 单测与仓库 BOM 守卫`

回滚：`git revert`；模块不存在时旧入口脚本仍可直接跑（回滚后重装）。

---

## 阶段 3：widget 拆分与启动链

### 3.1 拆分映射

| 新文件 | 内容 | 现文件行 |
|---|---|---|
| `linkweixin-widget.ps1`（入口） | 头注释/param、Add-Type、全局异常兜底、单实例锁、模块导入、上下文组装、计时器与消息循环、boot 日志 | 1-80、492-520 |
| `widget/widget-form.ps1` | 颜色/字体、`New-DotIcon`、`Add-Row`、`Build-WidgetForm`（窗体+全部控件+托盘+菜单+图标兜底） | 177-378 |
| `widget/widget-state.ps1` | `Test-AppRunning`、`Test-PluginGate`、`Test-WatchTask`、`Get-LastPushText`、`Update-WidgetState`（原 `Refresh-UI`） | 100-112、143-175、447-490 |
| `widget/widget-actions.ps1` | `Show/Hide/Toggle-Window`、`Real-Exit`、按钮/托盘/拖动/FormClosing 事件接线 | 380-445 |
| 删除（模块替代） | `Get/Set-NotifyOn`、`Get/Set-CodexNotifyOn` | 114-141 |

- 上下文 `$ctx` hashtable 承载：Form、控件引用、图标、颜色/字体、路径（模块提供）、错误日志函数、计数状态
- 事件脚本块闭包捕获 `$ctx`；行为与现实现逐项对齐（单实例接管、托盘自愈、10 分钟版本复查、30 秒心跳）
- 第 5 阶段在此加"版本号显示"（底栏）

### 3.2 install.ps1 启动链接线

- 新增参数 `-WidgetLauncher Auto|Python|Vbs`（默认 Auto）
- Auto 检测：`Get-Command pythonw.exe` 且 `src\widget-detached.py` 存在 → Python，否则 Vbs
- 快捷方式（shell:startup + 桌面）目标分支：
  - Python：`pythonw.exe "<InstallDir>\widget-detached.py"`
  - Vbs：`wscript.exe "<InstallDir>\run-hidden.vbs" "<InstallDir>\linkweixin-widget.ps1"`
- 安装记录 `launcher` 字段写实际值
- `-SkipWidgetLaunch` 跳过"本次启动窗体"

### 3.3 uninstall.ps1 兼容

- 进程匹配覆盖两种宿主：powershell（`-File ...linkweixin-widget.ps1`）与 pythonw（`widget-detached.py`），均限定 `$InstallDir`
- 文件清单天然覆盖 `widget/`、`widget-detached.py`（树枚举）

### 3.4 smoke.ps1 更新

- 沙箱用例新增：记录 `launcher` 字段存在且为枚举值；`install.ps1` 含 pythonw 检测分支；`widget/` 三个文件 + 入口齐全
- 大小写撞车扫描范围扩到 `src\linkweixin-widget.ps1` + `src\widget\*.ps1`
- 防闪回归断言扩展：`widget-detached.py` 的 `CREATE_NO_WINDOW` 校验保持；install 快捷方式分支包含 `run-hidden.vbs`

### 3.5 验证与提交

- [ ] 手动全交互（用户参与）：pythonw 启动 → 无控制台/WT 页签、托盘、双开关、拖动、隐藏/恢复、退出、重开（单实例接管）
- [ ] `-WidgetLauncher Vbs` 重装后跑同一清单
- [ ] 桌面/开机快捷方式指向正确；重启电脑后自启正常
- 提交 1：`refactor: 悬浮窗拆分 widget/ 三文件`
- 提交 2：`feat: 安装时检测 pythonw，双启动链接线`

回滚：`git revert` + 重跑 `install.ps1`（快捷方式会随安装重建）。

---

## 阶段 4：工程化门禁

- `.editorconfig`：ps1/psm1/psd1 → `charset = utf-8-bom` + CRLF + 2 空格；ts/json/md/yml → LF/UTF-8；vbs → CRLF；py → LF
- `.gitattributes` 补齐：`*.psm1`、`*.psd1`、`*.vbs`、`*.py`
- `.gitignore` 补：`node_modules/`、`dist/`
- `PSScriptAnalyzerSettings.psd1`：默认规则 + 排除项（逐项注释理由）+ `PSUseCompatibleSyntax`（TargetVersions 5.1）
- `tools/lint.ps1`：缺 PSScriptAnalyzer 自动安装（TLS1.2 前置）；有 Error/Warning 非零退出
- `tools/test.ps1`（spec 7.6 补充项）：本地/CI 共用测试入口，自动装 Pester 5 → 单测 + smoke
- `package.json`（private，devDeps：`typescript`、`@types/node`；script：`typecheck`）+ `tsconfig.json`（`noEmit`、`strict: false`、`skipLibCheck: true`、include `plugin/**/*.ts`）
- 修复全部 lint/tsc 告警；修不动的行内 suppress + 理由

验证门：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1
npm install; npx tsc --noEmit
```

提交：`chore: 工程化门禁（editorconfig/analyzer/lint/tsc）`

---

## 阶段 5：版本与治理

- 确认 psd1 `ModuleVersion = 0.1.0`；安装记录 version 改读 psd1（阶段 1 已预留）
- `CHANGELOG.md`：Keep a Changelog 1.1.0；`[0.1.0] - 2026-09-10` 概括现有能力 + 本次规范化（Added/Changed/Fixed）
- `CONTRIBUTING.md`：环境、三件套、Conventional Commits（`type(scope): 中文描述`）、分支模型、BOM/CRLF 规则、发布流程
- `SECURITY.md`：报告渠道、支持策略、安全设计说明（token 只走环境变量）
- `.github/ISSUE_TEMPLATE/{bug_report.yml, feature_request.yml}`（中文表单）+ `pull_request_template.md` + `dependabot.yml`（github-actions + npm，月度）
- README 重组（~150 行，中文优先）：简介、特性、快速开始、日常使用、环境变量表、文档索引、路线图（含公开时待办）、License
- `docs/ARCHITECTURE.md`：架构图、组件职责、数据流、关键设计决策
- `docs/TROUBLESHOOTING.md`：12 条坑 + 日志位置速查 + 症状排查
- `widget` 底栏显示版本号（读已加载模块 `Version`）

验证门：psd1/CHANGELOG/安装记录三处版本一致；README 链接全部可达；治理文件无占位符。

提交：`docs: README 重组与架构/排障文档拆分`、`docs: 治理文件（CONTRIBUTING/SECURITY/模板）`、`feat: 悬浮窗显示版本号`

---

## 阶段 6：GitHub 与发布

- `tools/build-release.ps1`：`-Version`（默认读 psd1）`-OutDir`（默认 `dist/`）；产出 `linkWeixin-v<ver>.zip`（install/uninstall + src/** + plugin/** + LICENSE + README + CHANGELOG + .env.example）与 `SHA256SUMS.txt`
- `.github/workflows/ci.yml`：
  - `lint`（ubuntu，pwsh）：PSScriptAnalyzer + `npm ci` + `tsc --noEmit`
  - `test`（windows-latest，`shell: powershell`）：`tools/test.ps1`
  - `concurrency` 取消旧运行；`permissions: contents: read`
- `.github/workflows/release.yml`：tag `v*` → 校验 tag==psd1 → `tools/build-release.ps1` → `gh release create`（notes 取 CHANGELOG 段落，缺则 `--generate-notes`）
- 本地先跑一遍 `tools/build-release.ps1` + 校验和解压验证
- 建仓与推送：
  ```powershell
  gh repo create srafyhucl-cpu/linkWeixin --private --source . --remote origin --push
  gh repo edit --description "多 agent（opencode / codex）任务完成推送微信（PushPlus），Windows / PowerShell 5.1，中文优先。"
  ```
- 分支保护：用 API 尝试，失败（私有 Free 计划限制）记录结果不阻塞
- `gh run watch` 盯 CI 到全绿；失败修复后重推
- 打 tag：`git tag v0.1.0; git push origin v0.1.0`；`gh release view` 验证 zip + SHA256SUMS；本地下载比对
- 最终逐项勾选 spec 第 11 节 DoD

提交：`ci: CI 与 Release 工作流`、`chore: 发布构建脚本`（随后 push）

---

## 附录 A：手动验证清单（需用户参与）

| # | 时机 | 内容 |
|---|---|---|
| 1 | 阶段 1 后 | 真推一条微信；opencode 任务跑完确认推送 |
| 2 | 阶段 2 后 | codex turn 跑完确认推送（含原电脑操控透传不受影响） |
| 3 | 阶段 3 后 | 悬浮窗全交互（pythonw 与 vbs 各一轮）；桌面/开机快捷方式；重启自启 |
| 4 | 阶段 5 后 | 底栏版本号 0.1.0 显示正确 |
| 5 | 阶段 6 后 | 卸载全清无残留（文件/任务/快捷方式/进程/codex 配置）；Release 下载校验 |

## 附录 B：回滚手册

- 每个阶段独立 commit：`git revert <hash>` 即回到上一稳定态；随后重跑 `install.ps1` 同步 `~/bin`
- 阶段 6 仅影响 git/GitHub；如 CI/Release 配置有问题，本地撤销对应 workflow 提交即可，不影响已安装运行
- 最坏情况：`git checkout <阶段开始前 hash>` 重装（marker/环境变量/安装物模型全程兼容）

## 附录 C：命令速查

```powershell
# 冒烟 / 单测 / lint / 打包
powershell -NoProfile -ExecutionPolicy Bypass -File tests\smoke.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-release.ps1

# 安装 / 卸载（日常）
powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -SkipScheduledTask
powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1

# GitHub
gh run list; gh run watch; gh release view v0.1.0
```
