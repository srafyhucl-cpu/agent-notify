# 架构与设计

面向维护者。用户向文档见 [README](../README.md)，故障排查见 [TROUBLESHOOTING](TROUBLESHOOTING.md)。

## 组件总览

```text
opencode 桌面端
  └─ 全局插件 plugin/notify-pushplus.ts
        └─ spawn notify-ai.ps1（powershell -File，窗口隐藏）

codex 桌面端
  └─ config.toml 的 notify 行 → codex-notify.ps1（wrapper）
        ├─ 1) 原样透传 codex-computer-use.exe turn-ended（保住原电脑操控）
        └─ 2) spawn notify-ai.ps1（窗口隐藏）

        两者汇入：
notify-ai.ps1 → LinkWeixin 模块（渲染 + 发送） → PushPlus API → 微信

常驻/辅助：
  linkweixin-widget.ps1（悬浮窗，widget/ 三部件）
  notify-toggle.ps1（命令行开关）
  codex-notify-watch.ps1（看守：计划任务每 5 分钟恢复 codex 配置）
  install.ps1 / uninstall.ps1（部署与清理）
```

## 组件职责

| 组件 | 职责 | 关键约束 |
|---|---|---|
| `plugin/notify-pushplus.ts` | 订阅 `session.execution.succeeded`，取会话标题与末条 assistant 文本，spawn 推送脚本 | 零 import、失败全吞、同会话冷却（内存 + 状态文件） |
| `src/notify-ai.ps1` | 推送入口：参数/stdin 兜底，调模块 | 唯一发 PushPlus 的地方；除 DryRun 外永远 `exit 0` |
| `src/codex-notify.ps1` | codex notify 中转：先透传上游 exe，再推送 | 任何一步失败都不得影响透传；exe 路径动态找最新 |
| `src/codex-notify-watch.ps1` | 计划任务看守：恢复 codex 配置 + 悬浮窗看护（缺失且非主动退出时拉起） | 只动指向上游 exe 的行；改前备份；看护需安装记录守卫 |
| `src/notify-toggle.ps1` | 翻转/设置两边 marker | 永远回显一行并 `exit 0` |
| `src/linkweixin-widget.ps1` + `src/widget/` | 悬浮窗：双开关、运行灯、推送时间、版本号、托盘 | 单实例接管；无控制台；异常落盘不静默死 |
| `src/lib/LinkWeixin/` | 共享模块：路径/渲染/发送/codex 解析/exe 定位/marker | 版本号唯一来源；调用时才读环境变量（可测） |
| `install.ps1` / `uninstall.ps1` | 整树部署 + 安装记录；按记录精准清理 | 幂等；提供全部 `-Skip*` 沙箱开关 |

## 数据流

### opencode 链路

1. 桌面端事件 `session.execution.succeeded`（实测事件名；`session.idle` 保留兼容）
2. 插件取 `session.get` 标题、`session.context` 末条 assistant 文本（跳过 reasoning / tool）
3. 三道闸（见下）任一命中 → 跳过（不烧冷却）；否则记录冷却并 spawn `notify-ai.ps1`
4. 模块渲染摘要（去代码块 → 按句截断 → HTML 转义 → 行内排版）并调 PushPlus
5. 成功/失败都写 `%TEMP%\opencode\notify-push.log` 或 debug 日志

### codex 链路

1. codex 完成一轮后调用 `notify` 配置的程序（wrapper）并传入 JSON 参数
2. wrapper 动态定位最新 `codex-computer-use.exe`，把参数与 stdin 原样透传
3. 检查 codex marker；存在则只跳过推送（透传已完成）
4. 解析 JSON：标题取 `input-messages[0]`（30 字截断），摘要取 `last-assistant-message`
5. spawn `notify-ai.ps1`（空 `-Summary` 时整个省略参数，规避 PS 5.1 空串绑定 bug）

## 部署模型

- **安装 = 整树拷贝**：`src/` 原样拷贝到 `~/bin`（保留 `lib/`、`widget/` 子结构），
  插件拷到 `~/.config/opencode/plugins/`（OpenCode V2 约定；旧版单数 `plugin\` 里如有残留会被清理）
- **安装记录** `~/bin/linkweixin-install.json`：`version / installedAt / launcher / files[]`
  - 重装时按记录清理上一版已不存在的文件（防止旧 `lib/`、`widget/` 残留）
  - 卸载按记录逐文件删除（路径逃逸校验）、剪空目录；无记录时按 `src/` 树兜底
- **悬浮窗启动方式**：安装时检测 `pythonw` —— 有则快捷方式指向
  `pythonw widget-detached.py`（GUI 子系统，无控制台、无 WT 页签），否则回退
  `wscript run-hidden.vbs linkweixin-widget.ps1`；`-WidgetLauncher` 可显式覆盖
- 卸载进程清理限定安装目录路径匹配（powershell / pythonw 两种宿主）

## 关键设计决策

### 1. marker 三闸（或关系）

| 闸 | 作用域 | 判定 |
|---|---|---|
| marker 开关 | opencode / codex 各自独立 | 文件存在 = 关 |
| 标题免打扰 | 仅 opencode | 标题含 🔕 或 `[勿扰]` |
| 时段免打扰 | 仅 opencode | `OPENCODE_NOTIFY_QUIET`，左闭右开；解析失败 fail-open |

marker 在冷却记账之前检查：被关掉的会话不消耗冷却。

### 2. 冷却与去重

- opencode 事件可能重复投递（桌面端多 location 各自订阅），插件用
  内存 Map（同实例）+ 共享状态文件 `notify-push-sent.json`（跨实例）双保险
- 默认同会话 10 分钟冷却；codex 无冷却（turn 事件粒度已足够粗）

### 3. 看守机制（对抗 codex 改写配置）

codex 桌面启动/更新会把 `config.toml` 的 notify 行改回直调 exe。
计划任务每 5 分钟 + 登录时运行 watcher：只在 notify 行指向
`codex-computer-use.exe` 时才改写为 wrapper，改写前备份 `.bak-notify-wrapper`。
计划任务动作经 `run-hidden.vbs` 中转（见第 5 条）。

### 4. 静默失败契约

推送链路任何失败（无 token、网络失败、code=$999、脚本异常）都写 stderr 后
`exit 0`，绝不阻塞 agent 或 codex 的 turn 流程。失败可查：
`notify-push.log`（成功记录）、`notify-debug.log` / `codex-notify-debug.log`（调试）。
例外：`install.ps1` 失败会以非零退出（它是交互工具，需要用户知道）。

### 5. 无窗口启动链（Win11 + Windows Terminal）

WT 会在 powershell 应用 `-WindowStyle Hidden` **之前**先创建窗口/页签。因此：

- `.lnk` / 计划任务 → `wscript.exe run-hidden.vbs <ps1>`（wscript 无控制台）
- agent 子进程 → `-WindowStyle Hidden` + 父进程无窗口，实测无闪
- 悬浮窗 → 优先 `pythonw` + `CREATE_NO_WINDOW`（GUI 宿主，连控制台都不分配，
  避免 WT 空页签被误关导致窗体被杀）

### 6. 编码与运行时约束

- 全仓库 `.ps1/.psm1/.psd1` 必须 **UTF-8 BOM + CRLF**（PS 5.1 按 GBK 读无 BOM
  文件会坏结构）；由 `.editorconfig` 约定 + `Repo.Tests.ps1` 硬检
- 只使用 PS 5.1 兼容语法（PSScriptAnalyzer `PSUseCompatibleSyntax` 检查）
- 渲染顺序固定：去代码块 → 截断 → HTML 转义 → 行内排版（插入的 `<b>/<br>`
  不能被转义）

### 7. 版本与发布

- 版本号唯一来源：`src/lib/LinkWeixin/LinkWeixin.psd1` `ModuleVersion`
- tag `v<版本>` 触发 Release：校验 tag == psd1 → 打包 zip + `SHA256SUMS.txt` → 发布
- CHANGELOG 按 Keep a Changelog；悬浮窗底栏与安装记录都读同一版本来源

### 8. 悬浮窗找回与看护

- `—` 最小化到任务栏（任务栏按钮带状态圆点，最可靠的找回路径）；`×` 藏托盘并弹气泡
- 看守任务每 5 分钟检查悬浮窗进程：不在且无主动退出标记时，按安装记录的 `launcher`
  拉起（pythonw 或 run-hidden.vbs）
- 主动退出（红字 / 托盘菜单）写 `%TEMP%\opencode\widget-exit.txt`，启动时清除——
  区分"崩了要自救"与"用户不想再看到它"
- 30 秒心跳 + ProcessExit 退出日志用于定位静默死亡时间窗

## 外部契约清单（改动红线）

1. `~/bin` 入口文件名与位置：`notify-ai.ps1`、`codex-notify.ps1`、
   `codex-notify-watch.ps1`、`notify-toggle.ps1`、`linkweixin-widget.ps1`、
   `run-hidden.vbs`、`widget-detached.py`
2. 全部环境变量与默认值（README 配置表）
3. marker 路径与语义：`~/.config/opencode/{notify-pushplus,codex-notify}.off`，存在 = 关
4. 日志/状态路径：`%TEMP%\opencode\` 下 `notify-push.log`、`notify-debug.log`、
   `codex-notify-debug.log`、`codex-watch.log`、`notify-push-sent.json`
5. codex `config.toml` notify 行格式与 watcher 策略（只动指向 exe 的行、改前备份）
6. 推送行为：标题前缀 `【opencode】`/`【codex】`、html 模板、失败静默 `exit 0`
7. 无窗口承诺：agent 侧子进程不闪控制台 / WT 页签
8. 悬浮窗：单实例、托盘化、双开关、快捷方式名 `linkWeixin 悬浮窗.lnk`
