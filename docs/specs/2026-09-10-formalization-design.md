# linkWeixin 规范化设计方案

- 状态：已评审通过（待实施）
- 日期：2026-09-10
- 决策记录：方案 B（模块化重构 + 工程化 + 治理 + 发布链路）；仓库暂不公开，先建私有仓库验证全流程
- 实施红线：行为对外契约不变（见附录 A 不变量清单）

## 1. 背景

linkWeixin 是一个 Windows 工具：监听 opencode / codex 任务完成事件，经 PushPlus 推送微信通知，附带悬浮窗开关、免打扰、看守进程等能力。项目目前是本机私有仓库：

- `master` 分支，17 个提交，无 remote、无 tag、无 CI、无 CHANGELOG
- 运行时文件散在 `scripts/`，安装脚本逐文件白名单拷贝
- 悬浮窗单文件 484 行，其余脚本 42~103 行
- 测试仅 `tests/smoke.ps1`（端到端冒烟）
- 发现的问题：最后一个提交加入的 `widget-detached.py` 是"半成品"——未被 `install.ps1` 安装、未被任何代码引用、卸载也不清理

目标：把项目提升到"可发布、可维护、可协作"的工程标准，同时在私有仓库上跑通 CI / Release 全流程；治理文件按私有阶段范围裁剪，公开时待办单列（见第 9 节）。

## 2. 范围

**做：**

- 仓库重组：`src/`（运行时）、`plugin/`、`tests/`、`tools/`、`docs/`、`.github/`
- PowerShell 模块化：共享逻辑抽成 `LinkWeixin` 模块，入口脚本变薄
- 悬浮窗拆分：入口 + `widget/` 三个职责文件
- 部署模型升级：整树拷贝 + 安装记录（`linkweixin-install.json`），修复 `widget-detached.py` 断线
- 版本体系：psd1 `ModuleVersion` 单一来源 + SemVer + CHANGELOG + tag + 自动 Release
- 测试体系：保留冒烟 + 新增 Pester 单测与仓库守卫
- CI：lint + 单测 + 冒烟（windows-latest 跑真实 PS 5.1）
- TS 轻量工程化：`tsc --noEmit` 纳入 CI
- 治理：CONTRIBUTING / SECURITY / Issue / PR 模板
- 文档：README 瘦身 + ARCHITECTURE + TROUBLESHOOTING + 本 spec
- GitHub 私有仓库：`gh` 创建、推送、CI 验证、tag `v0.1.0` 发布 Release

**不做（YAGNI / 公开后再说）：**

- CODE_OF_CONDUCT（公开时以中文版补齐）、winget / scoop、在线一键安装（`irm | iex`，私有仓库 raw 不可用）
- eslint / prettier、测试覆盖率门槛、签名发布
- 其余与本目标无关的重构

**语言与受众原则（本次固化）：**

- 目标：中文优先，服务中文区 Windows 用户；界面、通知、错误提示等一切用户可见文案均为中文
- 项目文档（README / CONTRIBUTING / SECURITY / CHANGELOG / Issue 与 PR 模板 / docs）全部中文
- 代码注释用中文，标识符保持英文；commit 信息遵循 `type(scope): 中文描述`
- 不提供英文文档，也不预留双语结构；如未来有国际化需求，另立专项

## 3. 目标仓库结构

```text
linkWeixin/
├── .github/
│   ├── workflows/{ci.yml, release.yml}
│   ├── ISSUE_TEMPLATE/{bug_report.yml, feature_request.yml}
│   ├── dependabot.yml
│   └── pull_request_template.md
├── src/                                  # 安装到 ~/bin 的完整运行时（整树拷贝）
│   ├── notify-ai.ps1                     # 薄入口（文件名 = 外部契约）
│   ├── codex-notify.ps1
│   ├── codex-notify-watch.ps1
│   ├── notify-toggle.ps1
│   ├── linkweixin-widget.ps1             # 薄入口：单实例锁 + 装配 + 消息循环
│   ├── widget/
│   │   ├── widget-form.ps1               # 窗口/控件/图标构建
│   │   ├── widget-state.ps1              # 轮询：运行灯/上次推送/插件与版本检测
│   │   └── widget-actions.ps1            # 开关/退出/拖动等动作
│   ├── lib/LinkWeixin/
│   │   ├── LinkWeixin.psd1               # 模块清单 = 版本号单一来源
│   │   ├── LinkWeixin.psm1
│   │   └── Private/*.ps1                 # 共享函数实现
│   ├── run-hidden.vbs
│   └── widget-detached.py
├── plugin/notify-pushplus.ts             # opencode 插件（原 opencode-plugin/）
├── tests/{smoke.ps1, unit/*.Tests.ps1}
├── tools/{lint.ps1, build-release.ps1}
├── docs/
│   ├── ARCHITECTURE.md
│   ├── TROUBLESHOOTING.md
│   └── specs/2026-09-10-formalization-design.md
├── install.ps1 / uninstall.ps1
├── README.md / CONTRIBUTING.md / SECURITY.md / CHANGELOG.md / LICENSE
├── PSScriptAnalyzerSettings.psd1 / .editorconfig / .gitattributes / .gitignore / .env.example
└── package.json / tsconfig.json          # 仅用于插件类型检查（devDeps）
```

## 4. 模块边界（行为等价搬迁）

模块 `LinkWeixin` 的导出函数：

| 函数 | 职责 | 来源 |
|---|---|---|
| `Get-LinkWeixinPaths` | 集中解析 marker / temp / 日志 / state 默认路径（支持环境变量覆盖） | 各脚本重复的路径拼接 |
| `Format-NotifySummary` | 摘要渲染纯函数（去代码块、标题加粗、列表、按句截断、HTML 转义） | `notify-ai.ps1` |
| `Send-PushPlusNotification` | 组装 payload + 调 PushPlus + 静默失败 + DryRun | `notify-ai.ps1` |
| `ConvertFrom-CodexNotifyEventArgs` | 解析 codex notify JSON 参数 → 标题/摘要 | `codex-notify.ps1` |
| `Get-CodexComputerUseExe` | 动态定位最新 `codex-computer-use.exe` | `codex-notify.ps1` |
| `Set-NotifyMarker` / `Test-NotifyMarker` | marker 读/写（On/Off/Flip） | `notify-toggle.ps1` 等 |

规则：

- 入口脚本只做：参数绑定 → `Import-Module $PSScriptRoot\lib\LinkWeixin\LinkWeixin.psd1` → 组装调用 → 静默 `exit 0`，每个入口目标 < 40 行
- 入口参数签名、环境变量、输出行为保持不变
- 模块加载失败：面向 agent 的入口（notify-ai / codex-notify / watch / toggle）捕获后写 stderr 并 `exit 0`；悬浮窗弹 MessageBox + 落盘日志
- 所有 `.ps1/.psm1/.psd1` 必须 UTF-8 BOM + CRLF（CI 硬门禁）

## 5. 部署模型

安装（`install.ps1`）：

1. 整棵 `src/` 拷贝到 `-InstallDir`（默认 `%USERPROFILE%\bin`），结构原样保留
2. `plugin/notify-pushplus.ts` → `-PluginDir`（默认 `%USERPROFILE%\.config\opencode\plugin`）
3. 写安装记录 `-InstallDir\linkweixin-install.json`：`{ version, installedAt, files[], launcher }`
4. codex `config.toml` 接管逻辑不变；计划任务注册不变
5. 悬浮窗启动方式自动检测（见第 6 节），写入快捷方式
6. 新增跳过开关供测试与特殊场景：`-SkipScheduledTask`（已有）、`-SkipCodexConfig`（已有）、`-SkipShortcuts`、`-SkipWidgetLaunch`

卸载（`uninstall.ps1`）：

1. 优先按安装记录精准清理文件与空目录（含旧版本残留、`lib/`、`widget/`）；无记录时按当前 `src/` 树扫描兜底
2. 删除安装记录本身
3. 杀悬浮窗进程：按**安装目录路径**匹配命令行（powershell 与 pythonw 宿主都覆盖）——同时让冒烟测试可在沙箱目录安全运行
4. 任务 / 快捷方式 / codex 配置还原逻辑不变；新增 `-SkipCodexConfig`、`-SkipShortcuts`

## 6. 悬浮窗拆分与启动链

拆分：

- `linkweixin-widget.ps1`：单实例互斥锁、模块加载、装配 context（hashtable 传递窗体与控件引用）、消息循环、全局异常兜底落盘
- `widget/widget-form.ps1`：窗体与控件构建、托盘图标与菜单、图标位图
- `widget/widget-state.ps1`：5 秒轮询（进程运行灯、上次推送时间、marker 开关状态、插件版本检测、看守任务版本检测）、托盘图标自愈
- `widget/widget-actions.ps1`：开关按钮、托盘菜单、拖动、隐藏/恢复、退出
- 保留行为：单实例接管、托盘显隐、两种开关独立、异常日志、大小写撞车守卫

启动链（修复 `widget-detached.py` 断线）：

- `install.ps1` 安装时检测 `pythonw`：
  - 有 → 快捷方式（开机 + 桌面）直指 `pythonw.exe "<InstallDir>\widget-detached.py"`（GUI 子系统，无控制台、无 WT 页签）
  - 无 → 回退现有 `wscript.exe run-hidden.vbs linkweixin-widget.ps1`
  - 提供 `-WidgetLauncher Auto|Python|Vbs`（默认 Auto）显式覆盖
- 两种路径都不产生控制台窗口；Python 为可选优化项，不装不影响功能
- README 注明：装/卸 Python 后重跑 `install.ps1` 可切换启动方式

## 7. 版本 / 测试 / CI / 发布

### 7.1 版本体系

- 唯一来源：`src/lib/LinkWeixin/LinkWeixin.psd1` → `ModuleVersion`，初始 `0.1.0`
- 悬浮窗底栏显示版本号（从已加载模块 `(Get-Module LinkWeixin).Version` 读取）
- `CHANGELOG.md`：Keep a Changelog 1.1.0 + SemVer 2.0.0；补 `[0.1.0]` 条目概括现有能力与本次规范化
- 发布：改 psd1 版本 + CHANGELOG → commit → tag `v<ver>` → CI 校验 tag == psd1 后自动发布

### 7.2 测试体系（三层）

| 层 | 位置 | 内容 |
|---|---|---|
| 单测 | `tests/unit/*.Tests.ps1`（Pester 5） | `Format-NotifySummary`（加粗/列表/代码块剔除/转义/按句截断/空行折叠）、`ConvertFrom-CodexNotifyEventArgs`（标题 30 字截断/摘要/坏参数跳过）、marker 语义、路径环境变量覆盖、模块清单合法性 |
| 仓库守卫 | `tests/unit/Repo.Tests.ps1` | 所有 ps1/psm1/psd1 带 BOM；`widget-detached.py` 被安装与启动链引用；冒烟测试引用的文件真实存在（安装/卸载对 `src/` 的整树覆盖由冒烟层的沙箱用例断言，不在此重复） |
| 冒烟 | `tests/smoke.ps1`（保留） | 现有全部端到端断言，路径更新；新增：沙箱目录完整跑 `install.ps1`（带全部 Skip 开关）→ 断言文件/记录 → 跑 `uninstall.ps1` → 断言清零 |

### 7.3 CI（`.github/workflows/ci.yml`）

- 触发：push `main` + PR；`concurrency` 取消旧运行；`permissions: contents: read`
- `lint` job（ubuntu-latest，pwsh）：PSScriptAnalyzer 按 `PSScriptAnalyzerSettings.psd1` 全量扫描；`npm ci` + `tsc --noEmit` 检查插件
- `test` job（windows-latest，`shell: powershell` = PS 5.1）：安装 Pester 5 → `Invoke-Pester tests/unit` → `tests/smoke.ps1`
- 备选：Pester 5 在 PS 5.1 若出现兼容问题，回退 Pester 4.10.1（记录于本 spec 实施备注）

### 7.4 发布（`.github/workflows/release.yml`）

- 触发：push tag `v*`
- 步骤：校验 tag == psd1 版本 → 调 `tools/build-release.ps1`（本地同款）
- 产物：
  - `linkWeixin-v<ver>.zip`：`install.ps1`、`uninstall.ps1`、`src/**`、`plugin/**`、`LICENSE`、`README.md`、`CHANGELOG.md`、`.env.example`（解压后根目录即可安装）
  - `SHA256SUMS.txt`
- 发布：`gh release create`，notes 取 CHANGELOG 对应版本段落（缺失则 `--generate-notes`）
- 只使用 GitHub 官方 action / CLI，不引入第三方 action

### 7.5 TS 工程化

- `package.json`：`private: true`，devDeps 仅 `typescript` + `@types/node`，`scripts.typecheck`
- `tsconfig.json`：`noEmit`、`strict: false`（初始，逐步收紧）、`skipLibCheck: true`、仅包含 `plugin/**/*.ts`
- 不加 eslint / prettier；`node_modules/` 进 `.gitignore`

### 7.6 本地工具

- `tools/lint.ps1`：CI 同款 lint 入口（缺 PSScriptAnalyzer 时自动安装），有 Error/Warning 时非零退出
- `tools/build-release.ps1`：参数 `-Version`（默认读 psd1）`-OutDir`（默认 `dist/`），产出与 CI 一致的 zip + SHA256SUMS

## 8. 工程化细节

- `.editorconfig`：ps1/psm1/psd1 → `utf-8-bom` + CRLF + 2 空格缩进；ts/json/md/yml → UTF-8 + LF；vbs → CRLF；py → LF
- `.gitattributes` 同步补齐（psm1/psd1/vbs/py）
- `PSScriptAnalyzerSettings.psd1`：默认规则 + 显式排除项（每项附理由注释）；开启 PS 5.1 兼容性规则 `PSUseCompatibleSyntax`（TargetVersions 5.1）
- 修复全部 lint 告警；确实不能修的用行内 suppress + 理由

## 9. 治理与文档（私有阶段）

- `CONTRIBUTING.md`：环境要求、提交前三件套（lint + 单测 + 冒烟）、Conventional Commits（`type(scope): 中文描述`）、分支模型（`main` + `feat/` `fix/` 短分支）、BOM/CRLF 硬规则、发布流程
- `SECURITY.md`：报告渠道（GitHub 私密漏洞报告；备选邮箱）、支持策略（仅最新版本）、安全设计说明（token 只走环境变量、不落盘、失败静默）
- `.github/ISSUE_TEMPLATE/*.yml`（bug / feature，中文表单）+ `pull_request_template.md`（检查清单）
- `.github/dependabot.yml`：github-actions + npm 两个生态，月度
- README 重组（约 150 行）：简介、特性、快速开始、日常使用、环境变量表、文档索引、路线图、License；细节外移
- `docs/ARCHITECTURE.md`：架构图、组件职责、两条数据流、关键设计决策
- `docs/TROUBLESHOOTING.md`：现有 12 条坑 + 日志位置速查表 + 症状 → 排查步骤

**公开时待办（防遗忘清单）：**

1. 仓库转 public；补中文版 CODE_OF_CONDUCT（Contributor Covenant 官方简体中文翻译）
2. winget / scoop 上架；在线一键安装（`irm | iex`）
3. GitHub topics / 主页信息完善；分支保护规则复核（私有 Free 计划可能不可用）
4. 全量重读 README/docs，去除内部口气描述；确认全部文案符合中文优先原则

## 10. 实施阶段

前置：`master` → `main`；仓库级 `user.email` 改为 GitHub noreply（现为占位符 `srafy@example.com`）。

| 阶段 | 内容 | 验收 |
|---|---|---|
| 1 结构迁移 | `git mv scripts→src`、`opencode-plugin→plugin`；install/uninstall 改整树+安装记录；smoke 路径更新+沙箱安装用例 | 本机重装 + smoke 全绿 |
| 2 模块抽取 | LinkWeixin 模块落地，入口变薄；Pester 单测补齐 | 单测 + smoke 全绿；真推一条 |
| 3 widget 拆分+接线 | `widget/` 三文件；安装时检测 pythonw；卸载兼容 | 手动全交互验证清单 |
| 4 工程化门禁 | .editorconfig、AnalyzerSettings、tools/lint.ps1、package.json/tsconfig，修全部告警 | lint + tsc 全绿 |
| 5 版本与治理 | psd1=0.1.0、CHANGELOG、CONTRIBUTING、SECURITY、模板、README/docs 重组 | 文档与实现一致 |
| 6 GitHub 与发布 | workflows、build-release.ps1、gh 建私有仓库、push、CI 绿、tag v0.1.0 | 见第 11 节 |

每阶段独立提交（允许阶段内多个小提交），失败可单独回退。

## 11. 验收标准（DoD）

- [ ] 本机全新安装 → 单测 + 冒烟 + lint 全绿
- [ ] 真推微信一条成功；opencode 与 codex 两条链路各验证一次
- [ ] 悬浮窗全交互正常（开关/托盘/拖动/重开/退出），版本号显示正确
- [ ] pythonw 与 vbs 回退两种启动方式都验证
- [ ] 卸载后无残留（文件 / 任务 / 快捷方式 / 进程 / codex 配置还原）
- [ ] GitHub 私有仓库 CI 全绿（lint + test）
- [ ] tag `v0.1.0` → Release 自动产出 zip + SHA256SUMS，下载后校验一致
- [ ] README / CONTRIBUTING / SECURITY / CHANGELOG 与实际行为一致

## 12. 风险与对策

| 风险 | 对策 |
|---|---|
| PS 5.1 模块加载/编码坑（项目有 BOM/编码踩坑史） | 全仓库 BOM 硬门禁；入口对模块失败保持契约行为（agent 侧 exit 0 / 悬浮窗可见报错） |
| Pester 5 在 PS 5.1 稳定性 | 备选回退 Pester 4.10.1；CI 失败可先只跑 smoke 保底 |
| widget 拆分回归 | 拆分后按手动全交互清单逐项验证；保留大小写撞车守卫测试 |
| 卸载遗漏/误删 | 安装记录 + 兜底扫描 + 沙箱安装/卸载用例；进程匹配限定安装目录 |
| CI 私有仓库分钟数 | Windows 2x 计费，预计 ~10 分钟/次；免费额度 2000 分钟/月，充裕 |
| 阶段 6 意外影响本机运行中的服务 | 阶段 1-5 全部本地完成并验证；阶段 6 只动 git/GitHub，不动本机已装文件 |

## 附录 A：不变量清单（外部契约）

1. 入口文件名与安装位置：`%USERPROFILE%\bin\{notify-ai,codex-notify,codex-notify-watch,notify-toggle,linkweixin-widget}.ps1`、`run-hidden.vbs`、`widget-detached.py`
2. 环境变量：`PUSHPLUS_TOKEN`、`NOTIFY_AI_SCRIPT`、`OPENCODE_NOTIFY_SCRIPT`、`OPENCODE_NOTIFY_COOLDOWN_MIN`、`OPENCODE_NOTIFY_STATE_FILE`、`OPENCODE_NOTIFY_LOG_FILE`、`OPENCODE_NOTIFY_OFF`、`OPENCODE_NOTIFY_MARKER_FILE`、`CODEX_NOTIFY_MARKER_FILE`、`OPENCODE_NOTIFY_QUIET`、`OPENCODE_NOTIFY_DEBUG`、`CODEX_NOTIFY_DEBUG`、`CODEX_CONFIG`、`CODEX_NOTIFY_WRAPPER`（含义与默认值均不变）
3. marker 路径与语义：`~\.config\opencode\notify-pushplus.off`（opencode）、`~\.config\opencode\codex-notify.off`（codex），存在 = 关
4. 日志/状态路径：`%TEMP%\opencode\` 下 `notify-push.log`、`notify-debug.log`、`codex-notify-debug.log`、`codex-watch.log`、`notify-push-sent.json`
5. codex `config.toml` 的 notify 行格式与 watcher 改写策略（只动指向 exe 的行、改写前备份）
6. 推送行为：标题前缀 `【opencode】` / `【codex】`；PushPlus html 模板；任何失败静默且 `exit 0`
7. 无窗口承诺：agent 侧子进程不得闪控制台 / WT 页签
8. 悬浮窗：单实例、托盘化、两个独立开关、启动项与桌面快捷方式名称 `linkWeixin 悬浮窗.lnk`
