# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与
[语义化版本](https://semver.org/lang/zh-CN/)。版本号唯一来源是仓库根目录的 `VERSION`
（2.0.0 起；此前为 `internal/app/version.go`）。

## [2.0.7] - 2026-09-26

### Added

- 桌面 UI 明暗双主题视觉基线矩阵（6 页 × 2 主题）与 axe 无障碍扫描纳入门禁；新增交互回归测试（列表整块可点、卡片折叠、对话框 Escape）与「控件冒烟」（逐页真实点击所有可用控件，断言无 console 错误与未捕获异常）。
- 正式 ZIP 更新包新增 `RELEASE-MANIFEST.json` 与 detached CMS 签名 `RELEASE-MANIFEST.p7s`；清单覆盖 ZIP 内全部普通文件，Stable 会在替换安装目录前拒绝缺清单、签名无效、文件集合不一致或哈希不符的包，Beta 保留两个控制文件同时缺失时的旧开发包兼容边界。
- Release 构建与补发门禁现在使用 Rust `DEFAULT_SIGNATURE_THUMBPRINT` 作为 2.0 唯一信任锚，并校验安装器、五个 ZIP 内程序、清单和资产摘要；发布流程拆为 validate、build、受保护 `main` 上的 publish，源码与客户端更新仓均先 Draft 后复核发布。

### Changed

- 桌面 UI 全面重构（第二、三轮）：六页统一到各自范式（渠道 Master–Detail 优先、总览 Dashboard、历史检索报表、诊断例外分诊、设置一决策一行）；导航顺序改为渠道优先；设置页合并分组卡；无边框窗口 + 自绘标题栏 + 托盘图标；亮色主题按 WCAG 逐对核算单独设计，不再机械反色。
- 交互一致性：列表页统一为「整块可点 + 选中态」（柔和主色底 + 左侧 3px 主色药丸）；渠道账号支持行内重命名；登录与退出账号两处对话框支持 Escape 关闭并恢复焦点。
- 无障碍语义：Agent 卡片、历史行、渠道账号行的展开/选中按钮从 `aria-pressed` 改为 `aria-expanded`（披露语义），并在被控内容存在时补 `aria-controls`。
- 一致性整理：字重收敛到 400/600/650/700 四档；间距统一到 4/8 网格的 `--space-*` token；删除设置页遗留的死 CSS。
- 依赖与安全：`time`、`serde_with` 升级修复安全告警（3 → 0），恢复并验证 Dependabot 的 cargo 更新通道。
- 内部重构，行为不变：命令服务 `service.rs`（1254 行）拆出 `mapping.rs`（7 个纯映射与脱敏函数）和 `app_exit.rs`（更新器与退出命令共用的退出端口）；运行时 `runtime.rs`（966 行）拆出 `error.rs`（错误归一化）和 `workers.rs`（入站消费、投递、状态刷新与监督 worker）。全量测试验证零行为变化，方便后续 UI 迭代在清晰的模块边界上进行。
- 更新器 ZIP 回退只复制已验证清单声明的文件；安装器启动失败时复用同一清单、PE、版本和 Authenticode 校验链，失败会回滚，不触碰已安装文件。

### Fixed

- 亮色主题对比度门禁失败（语义色与次级文字整体深一档，主色白底 4.09→5.93）、渠道下拉在暗色下渲染成一排 ✓、Agent 卡片「回复/支持」被超长 ID 挤成竖排。
- Agent 卡片与历史行「看起来可点、实际只有标题文字可点」的交互缺陷（点击改挂整块，标题按钮保留为键盘路径）。
- CI 偶发失败的 `production_contract` 组件断言：组件表锁中毒被静默吞掉会让诊断显示成「没有任何后台组件」，现改为显式从中毒锁恢复，并补「中毒后快照仍报告组件」的回归测试。

## [2.0.6] - 2026-09-23

### Added

- `agentnotify-ingress.exe` 增加只读自检命令行：`--doctor` 输出 JSON 报告（命名管道是否在监听、spool 待补投与隔离数），`--ping` 给一行结论；两者都不提交事件、不写盘，退出码 0 正常、1 异常，供无头环境与远程排查探活。

### Changed

- 维护者文档对齐 2.0 现状：`SECURITY.md`（凭据进 Windows 凭据管理器、Tauri 工作台、更新包签名与解包校验）、`CONTRIBUTING.md`（Rust 门禁、5 个发布二进制、发布流程以 `VERSION` 为准、Go 1.x 遗留代码的保留与冻结边界）、`.env.example`（按 2.0 与 Go 1.x 遗留分段，变量名以代码为准）。
- 桌面 UI 包版本纳入版本一致性：`apps/desktop-ui/package.json` 与锁文件随 `VERSION` 同步（`tools\sync-version.ps1` / `tools\check-version.ps1`）。
- Release workflow 移除无用的 Go 工具链安装：正式包只构建 Rust 产物，减少发版故障面。
- `hosts/desktop-tauri` 里散落的时间与大小裸数字改为命名常量（Delivery 终态等待、事件订阅重试、进程默认超时、哈希分块等，行为不变）。

### Fixed

- OpenCode 适配器不再因事件类型改名而静默拒收：`eventType` 缺失、为空或为已知终态类型（`session.idle` / `session.error` / `session.execution.succeeded` / `session.execution.failed`）时照常推送，未知类型（如 `session.started`）仍然拒绝，与其余四个适配器的容错口径对称。
- Rust 门禁在内存较小的机器上不再随机失败：`tools\rust\gate.ps1` 按物理内存限制 cargo 并发（可用 `-Jobs` 或 `CARGO_BUILD_JOBS` 覆盖），避免测试二进制并行链接打爆分页文件（Windows 错误 1455）。
- `tests\desktop-installer-smoke.ps1` 改为校验正式安装器 `installer\agent-notify.iss` 的 2.0 分发清单（原先校验已废弃的 Rust 预览脚本），并纳入 `tools\test.ps1` 门禁；不传 `-Installer` 时只做定义检查，CI 无需构建产物。
- 根目录 `install.ps1` 明确标注为 Go 1.x 遗留运行时安装入口，运行时提示普通用户改用 2.0 正式安装器。

## [2.0.5] - 2026-09-23

### Fixed

- 应用内升级不再误拒安装器：更新器要求更新包必须是 64 位 PE，而 Inno Setup 的安装器存根是 32 位 PE，导致点「下载并安装」报「更新包不是 64 位 Windows 可执行文件」而无法升级。现在安装器通道接受 32 位 PE（位宽不影响静默安装能力），替换进安装目录的应用程序本体仍强制 64 位。

### 说明

- 2.0.0–2.0.4 的更新器会以同一原因拒绝 2.0.5 的安装器，这些版本**无法应用内升级**：请从 Release 页面手动下载 `Agent-notify-Setup-v2.0.5.exe` 安装一次，之后的应用内升级恢复正常。

## [2.0.4] - 2026-09-23

### Fixed

- 微信推送的页脚时间改为本地时间：通知时间以 UTC 存储，此前页脚直接按 UTC 渲染，显示的日期时间与本地相差整个时区（真机案例：本地 `14:34` 显示成 `06:34`）。现在页脚按系统本地时区渲染，与 Go 版行为一致。

## [2.0.3] - 2026-09-23

### Fixed

- 升级后首启不再整段丢失推送：安装器先结束旧版、几秒后拉起新版，旧版心跳（`%TEMP%\agent-notify\widget-alive.txt`）仍落在运行时的 30 秒存活判定窗口内，首启会进入「迁移诊断模式」——该模式不启动渠道、ingress 与发件箱，表现为所有 Agent 的推送与引用回复全部停止，且不会自动恢复。现在因「旧版仍在运行」进入诊断模式时，会在后台每 35 秒重试一次常规启动（最多 6 次），心跳过期后自动恢复并刷新界面；其它迁移失败原因不重试，仍原样暴露。诊断期间的事件由 ingress 落盘保留，恢复后自动补推。
- 发布镜像不再出现「半成品窗口」：镜像改为先以草稿上传全部资产、最后一步才发布为 Latest，避免客户端在上传中途查询到只有压缩包的 Release 而选错升级路径（旧版客户端表现为自动检查更新报「更新包缺少文件」）。

## [2.0.2] - 2026-09-23

### Fixed

- 静默升级不再弹出「无法自动关闭所有应用程序」：旧版悬浮窗收到关闭请求时隐藏到托盘，安装器替换文件前关不掉它。现在安装器在静默模式下先结束旧进程（`agent-notify.exe`、`agentnotify-desktop.exe`），交互式安装行为不变。
- 升级继承旧版 Agent 开关：旧版语义为「没有 marker 即开启」，此前只导入关闭状态、且新适配器默认关闭，导致从旧版升级后 Codex / Antigravity / Devin / Command Code 的通知静默停止。现在存在旧版遗留时按旧版状态继承，全新安装仍保持保守默认（需在 Agents 页手动开启）。
## [2.0.1] - 2026-09-22

### Fixed

- 修复 2.0.0 升级后无法启动：数据库迁移校验和按原始字节计算，导致 CI（CRLF 检出）构建的程序与本地（LF 检出）构建的程序对同一份迁移算出不同校验和，已有数据库被安全门拒绝（表现为「打开数据库失败：数据库迁移校验失败」）。现在校验和按行尾归一化计算；已存在但「仅行尾不同」的历史校验和会自动自愈为归一化值，内容不一致时仍然报错。
- .gitattributes 增加 `*.sql text eol=lf`，统一迁移文件的检出行为。
## [2.0.0] - 2026-09-22

桌面端改用 Tauri + React 重写，替换原 Win32 自绘悬浮窗；正式安装入口切换为
`agentnotify-desktop.exe` + `agentnotify-ingress.exe`。本次同时把 Go 版支持的 Agent 接入
全部补齐（Codex、Antigravity、Devin、Command Code），并补上应用内一键升级。

### Added

- 工作台式主窗口：总览、Agents、Channels、History、Diagnostics、Settings。界面由 descriptor 与 JSON Schema 驱动，新增 Agent 或渠道不需要改页面分支。
- Rust 核心运行时：SQLite WAL 状态库、事务型 Outbox、按「渠道 + 账号 + 渠道消息 ID」精确引用路由、入站至多一次 Claim。
- 内部事件入口 `agentnotify-ingress.exe`：版本化事件协议、当前用户命名管道；核心离线时写入持久化 spool，并在下次启动消费。
- 首次启动只读迁移：导入旧配置、登录状态、开关、推送历史、引用路由与 Claim；重复启动不重复迁移。
- Codex、Antigravity、Devin、Command Code 四个 Agent 适配器：完成通知与精确引用续聊（Codex 走 `codex queue`、Antigravity 走本机 agentapi 原会话、Devin 走 ACP 桌面扩展、Command Code 走回复窗口 mod），各自带独立 Hook / 扩展 / mod 与失败诊断日志。
- 应用内一键升级：查询 `agent-notify-releases` 最新版本 → 下载到应用临时目录 → SHA256 + 签名指纹 + PE 版本校验 → 优先拉起安装器，失败退回 ZIP 解包替换。
- Settings 页「下载并安装」入口：仅在存在可安装版本时可用，安装中与失败都有明确中文反馈。
- 每个 Agent 的配置项（`codexHome`、`annotationsDir`、`sessionsDatabase`、`commandCodeReplyWindowSec` 等）真正作用到适配器，改动后自动重建注册表并重启运行时。

### Changed

- 安装器保留原 AppId 与标准安装目录，升级落回原位置；自启动与快捷方式指向新的桌面程序。
- OpenCode 接入改为 V2 插件，任务完成事件经 `agentnotify-ingress.exe` 提交。
- 版本号唯一来源从 `internal/app/version.go` 切换为仓库根 `VERSION`：`tools/sync-version.ps1` 同步各发布位置，`tools/check-version.ps1` 校验一致性。
- 界面文案统一为中文（导航、页面标题、表头、投递状态枚举、错误提示），保留 Agent、descriptor、Hook 等既有技术术语。
- 发布链打包三个新 Hook、Devin V2 扩展与 Command Code V2 mod；安装器新增可取消的接入任务，升级时先清旧 Go 版 Hook 再装新接入。
- 卸载只清理 AgentNotify 自己的条目（Codex notify、Antigravity Hook 与启动器、Devin handler 与 V2 扩展、Command Code mod），第三方 notify 与自定义 matcher 原样保留；没有备份时按 `--previous-notify` 载荷还原，或给出可照做的说明。
- 发布补发门禁校验 ZIP 内五个程序（桌面端、ingress 与三个 Hook）的存在性、`SHA256SUMS.txt` 覆盖与哈希、以及签名指纹。

### Fixed

- 启动竞态：窗口先于宿主初始化就绪时，命令不再直接报「桌面宿主尚未完成初始化」，改为等待初始化完成（上限 20 秒）；初始化失败时返回具体原因，而不是笼统的「请稍后重试」。
- Command Code 回复窗口配置通道：界面里的窗口秒数此前只写入 SQLite，而 mod 读的是旧 JSON 配置，导致窗口永不打开、引用回复恒报「窗口已过」；改为应用把界面值原子写入收件箱 `window.json`，mod 按「环境变量 > 界面值 > 旧配置」读取（界面值 0 视为显式关闭）。
- ClawBot 主动推送诊断：区分「上下文不存在」与「平台拒绝准备会话（PrepareFailed）」两种失效原因，`notifystart` 结果与清空上下文原因改为默认可见日志，且不打印令牌与响应体。
- 通知格式恢复 Go 版样式：投递层把结构化信息交给渠道渲染，微信里重新出现「🟢 显示名｜会话名」标题栏、正文与「*引用此消息可继续对话*」页脚（此前只发原始「标题 + 正文」）。
- 安装器 `{userprofile}` 非法常量：原写法会让 OpenCode 接入任务在安装末尾抛异常，已改为 `{%USERPROFILE}` 并补回归断言。

### Removed

- 旧 Win32 悬浮窗不再作为发布入口；Go 版 `agent-notify.exe` 不再随正式包发布（上一稳定 Release 仍保留，供回滚使用）。

## [1.17.0] - 2026-09-19

### Added

- CommandCode 支持微信引用续聊：`commandCodeReplyWindowSec`（1–600 秒，0 = 不等待，**默认 0**）为每次回答留出
  「回复窗口」，窗口内引用通知即可把回复作为新的用户指示续跑同一会话。
  **该功能为实验性**：它通过在 `onStop` 里挂住尚未结束的那一轮来等待，窗口期内该会话界面会呈现"卡住"、
  手打输入只进队列，直到窗口结束或收到引用回复；不需要时设为 0 即可恢复。
- 通知页脚在开启窗口时写明时限：`*引用此消息可继续对话（60 秒内）*`。
- 新增 `AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC` 环境覆盖与 `notify --reply-window <秒>` 参数。

### Changed

- CommandCode 的引用回复改为经 mod 的 `onStop` 把用户正文送进会话（`queueMessage` 在窗口内不会被消费，
  实测模型只收到声明、收不到正文）；窗口内每 0.5 秒轮询本地收件箱，回复一到立刻续跑，不再干等剩余窗口。
- 窗口只在「`commandCodeReplyWindowSec` > 0 + `replyEnabled` + 该 Agent 未被暂停」时开启；
  关闭滑块或关闭引用回复时不会多等。
- mod 状态改为**每会话一份**：Command Code 会为每个会话各调用一次 mod 工厂，模块级共享状态会让心跳指向错误会话。
- 悬浮窗整体重新布局：窗口由 400×450 加高到 400×570，代理网格改为 2×3 且 CommandCode 独占整行，
  修掉三列布局下「CommandCode / 已接入」被截断的问题；子视图（检查修复 / 历史 / 设置 / 登录）的主卡片与
  底部按钮改为按窗口高度自适应，修复加高后第五张卡片与按钮被裁切的问题。
- 悬浮窗与推送历史中的标题按纯文本展示：去掉 Markdown 加粗符 `**` 与状态徽标，不再出现 `**🟢 …**` 字样。
- 设置页新增「CommandCode 回复窗口」输入项（0 = 关闭），提示行写明"等待时该会话暂停响应，不消耗 token"；
  选项卡片改为铺满到按钮上方，不再留空带。保存时与其它设置项一起写入 `config.json`。
- 推送历史每页由 5 条增至 6 条，详情卡片铺满可用高度；Agent 胶囊加宽到能容纳最长的显示名（CommandCode），
  不再裁字。
- 新增渲染快照脚手架 `internal/ui/ui_snapshot_test.go`（默认跳过，设 `AGENT_NOTIFY_SNAPSHOT_DIR` 后可为五个视图
  输出 PNG），用于 UI 改动的目视复核与回归。

### Fixed

- 修复引用回复误报「目标会话未在运行」：进程重启留下的同会话僵死心跳排在前面时，检查会提前返回；
  现在扫完整个心跳目录再判定，并定期清理僵死心跳。
- 窗口已过的引用回复给出明确提示，不再静默丢弃；mod 不再对无法投递的回复回写成功。
- 修复回复窗口在多会话下会互相挂起：`onStop` 等钩子是进程级注册、每个会话实例各注册一份，此前任何会话结束都会让
  所有实例各挂 60 秒（表现为界面卡住、输入只排队）。现在只有拥有该 run 的会话实例才会动作，且窗口默认关闭。

## [1.16.0] - 2026-09-18

### Added

- 新增第五个 Agent「CommandCode」的接入：安装器会把 Command Code mod 部署到用户级
  `%USERPROFILE%\.commandcode\mods\agent-notify.ts`（并把安装目录绝对路径写进 `BAKED_BIN`），
  mod 在每轮任务结束时调用 `agent-notify notify --agent commandcode` 推送微信通知。
- `status`、`doctor`、`integration-status`、`toggle` 与悬浮窗支持第五个 Agent；新增
  `commandcode.off` marker 与 `AGENT_NOTIFY_COMMANDCODE_*` 路径覆盖。
- 新增 Command Code 接入状态检测：mod 文件归属、`BAKED_BIN` 指向的程序，以及 mod 心跳新鲜度。
- 卸载器按归属标识（`agent-notify-commandcode-mod`）只删除 AgentNotify 自己部署的 mod。

### Changed

- OpenCode 的开关判定收敛为按 agent 的 marker 策略：显式 `--agent opencode` / `--agent commandcode`
  都会在 CLI 侧强制执行各自的 `.off` 开关（此前 `notify --agent opencode` 不查 `opencode.off`）。
- 悬浮窗代理网格由 2×2 改为 3×2，容纳第五张卡片；副标题与「展开全部」文案同步为 5 个 Agent。

### Fixed

- `tools/lint.ps1` 的 workflow 非 ASCII 校验此前因 `Get-ChildItem -Include` 未配合 `-Recurse`/通配
  而枚举不到文件、静默空转；现改为按 `-Path <dir>\*` 枚举，并在枚举到 0 个文件时直接判失败。
- `docs/ARCHITECTURE.md` 补充引用回复 at-most-once 的崩溃窗口语义说明。

## [1.15.0] - 2026-09-18

### Changed

- 产品显示名统一为 AgentNotify：悬浮窗标题、托盘提示与右键菜单、CLI 横幅与自检标题、接入状态文案、更新失败提示、安装与卸载提示、安装器名称与卸载列表、桌面与开始菜单快捷方式名。
- 快捷方式改名为 `AgentNotify.lnk`。安装、卸载与安装器升级时会自动清理改名前的 `Agent-notify.lnk` 与 `Agent-notify 悬浮窗.lnk`，老用户升级不会留下两个图标。
- 以下技术标识保持不变，避免影响老用户升级与更新链路：`agent-notify.exe`、安装目录 `%LOCALAPPDATA%\Programs\Agent-notify`、配置目录 `.config\agent-notify`、`AGENT_NOTIFY_*` 环境变量、Go 模块路径、插件与 Hook 文件名，以及发布产物名 `Agent-notify-Setup-vX.Y.Z.exe` / `Agent-notify-vX.Y.Z.zip`。

## [1.14.0] - 2026-09-18

### Added

- 微信「主动推送会话已断开」提醒：曾经正常推送过、之后主动推送上下文被服务端回收时（日志里的 `ret=-2 prepare failed`），悬浮窗会弹一次托盘气泡「微信推送已断开」，底部「微信」入口显示「推送已断」，设置页与微信配置页同步显示真实状态。按提示在微信里给 ClawBot 发一条消息即可恢复，恢复后再次断开才会再提醒一次。
- 凭据文件新增 `session_established_at` / `session_alert_at` 记录会话世代与提醒标记；旧凭据文件缺少这两个字段时按“从未就绪”处理，只做界面引导、不弹气泡，不需要重新登录。

### Changed

- 通知页脚：分隔符由 `———` 改为单个 `—`，且只有引用提示保持斜体，时间改为正体。
- 悬浮窗状态判定统一收敛为单一微信链路状态，`health()`、托盘、四处界面与提醒共用同一判定。

### Fixed

- 登录已失效（`stale`）时，设置页 ClawBot 卡片与微信配置页此前会显示绿色「会话正常 / 链路正常」，现在显示真实的红色失效状态。
- 自检推送（`agent-notify test`）不再声称可用来验证引用续聊：它不携带会话 ID、不会写入引用路由，引用它只会收到「无法续聊」。

## [1.13.2] - 2026-09-18

### Fixed

- 更新下载偶发失败（`context deadline exceeded`）：下载失败会自动重试；HTTP 下载超时由 3 分钟放宽到 10 分钟（单次安装准备整体 15 分钟）；超时时给出可操作提示（稍后重试或到 Releases 页面手动下载）。

## [1.13.1] - 2026-09-18

### Fixed

- OpenCode 插件：改动冷却（`cooldownMin` / `AGENT_NOTIFY_COOLDOWN_MIN`）后即时生效，不再需要重启 OpenCode。
- OpenCode 引用回复后的回答不再被会话冷却压掉：引用回复提交成功后，该会话的下一次完成事件豁免一次冷却，保证“引用必得回答”。

## [1.13.0] - 2026-09-18

### Added

- 引用回复成功后回一条送达确认（`✅ 已送达 **Agent**，会话：…`），让你明确回复已抵达目标会话；默认开启，可在配置里用 `replyConfirmation: false` 关闭，也会叠加在 `AGENT_NOTIFY_REPLY_CONFIRMATION` 环境覆盖上。

### Changed

- 通知改为 Markdown 模板：加粗标题行 `**🟢 Agent｜会话名**`、保留 Agent 的 Markdown（标题、列表、代码块等）、`———` 分隔与 `引用此消息可继续对话 · MM/DD HH:mm` 页脚；纯文本客户端也保持可读，不再把 Markdown 破坏成半成品。

## [1.12.0] - 2026-09-18

### Added

- 悬浮窗自动检查更新：启动约 15 秒后静默检查一次，之后每 2 小时轮询；发现新版本时“升级”按钮标黄并显示版本号，不弹窗、也不自动安装（点“升级”后仍走原有的确认安装流程）。检查失败只写调试日志。

### Changed

- 更新提示与操作报错改用应用内自绘对话框，与设置、修复、登录等页面的风格统一（不再使用原生消息框）。
- 手动补发入口 `tools\publish-release.ps1` 上传前也会校验签名者指纹，堵住“secret 丢失时手动镜像未签名产物”的路径。

### Fixed

- 本地结果文件删除失败时做有界重试，避免坏结果文件在 Windows 上残留导致反复报错。

## [1.11.2] - 2026-09-17

### Fixed

- 更新包签名探测失败时，错误信息附带 PowerShell 子进程 stderr，便于定位客户端"更新失败"的原因。

### Changed

- 发布流程新增签名门禁：Release workflow 强制要求签名 secret，构建脚本（`tools/signature-common.ps1`）校验产物已签名且签名者指纹等于 `internal/update/signature.go` 的内置信任指纹；未签名、状态异常或指纹不符都会在发布前失败，避免发出客户端拒绝安装的包。
- `Prepare` 通用用例改为注入签名探测、并发压力测试显式放宽锁等待，保证 `-race` 下稳定。

## [1.11.1] - 2026-09-17

### Fixed

- 修复目标进程命令行长度为奇数或为空时的越界写与 panic，以及 Codex notify 重写时安装路径含 `$` 被正则展开导致路径被吞的问题。
- 修复 ZIP 解压大小上限可被 uint64 溢出绕过、本地状态文件锁永久阻塞、原子写固定临时名并发覆盖、`winsqlite` 文本上限边界误判等健壮性问题。
- `recordFailure`/`RecordSkipped` 的历史写入失败改由 `Warning` 上报；悬浮窗清空/读取推送历史、定位自身可执行文件失败不再静默；旁路 helper 与离线通知失败写入诊断日志。

### Changed

- 悬浮窗窗口句柄改为原子访问，消除跨 goroutine 数据竞争。
- ClawBot HTTP 响应体读取限制为 8 MiB；`codex-computer-use` 旁路 helper 增加 30 秒超时。
- 本地标记文件与原子写临时文件权限统一为 0700/0600。

### Added

- CI 新增 `-race` 并发检测作业。
- 补齐 winsqlite 封装、CLI 纯逻辑、文件锁超时、心跳门禁、`$` 路径重写与 stdin 读取等回归测试。

## [1.11.0] - 2026-09-16

### Added

- 启用代码签名（自签名证书）并把证书指纹内置到客户端：更新时只接受由该证书签名的安装包，其它签名者、未签名或哈希不符一律拒绝；自签名不受信链的状态（`UnknownError`/`NotTrusted`）在指纹匹配时放行。
- CI 从仓库 secrets 读取 PFX 自动签名主程序与安装器；缺少 PFX 时跳过签名（仍可构建），**配置了签名就必须签上**，否则构建失败。
- 新增 `tools/sign-selfsigned.cmd` 垫片（供 Inno Setup 调用）与 `docs/code-signing.md` 的证书生成、备份、轮换说明。

## [1.10.0] - 2026-09-16

### Added

- 支持用自签名/私有证书签名发布产物：客户端在配置了信任指纹时接受 `UnknownError`/`NotTrusted`（指纹即信任锚），被篡改（`HashMismatch`）仍一律拒绝；新增 `tools/sign-selfsigned.ps1`（CI 中导入 PFX 签名后自动清理）与 `docs/code-signing.md` 的零成本接入方案。
- 配置 `AGENT_NOTIFY_SIGNTOOL` 后，构建会对 ZIP 内主程序与安装器强制校验签名状态，未签成直接失败。

### Fixed

- 安装时改写 Codex notify 链不再局限于盘符路径：UNC 与其它路径形式都能更新到当前安装目录；对历史/手写的不规范转义有兜底，无法改写时会明确警告。

## [1.9.0] - 2026-09-16

### Added

- 发布流程新增版本一致性门禁：`tools/check-version.ps1` 校验 `VERSION`、manifest、README 徽章、`BotAgent`、Devin 扩展 `package.json` 与 CHANGELOG 段落，`tools/lint.ps1` 与 Release workflow 共用。
- CI 静态检查新增 `govulncheck` 漏洞扫描。

### Changed

- 最低 Go 版本提升到 1.26.8：1.25.6 的标准库存在 15 个可被利用的漏洞。
- Release workflow 的发布步骤改为幂等：重复运行或用新提交重指 tag 时会更新已有 Release，而不是直接失败。

### Fixed

- 安装/卸载的残留进程清理要求目录边界，避免误伤路径前缀相同的其它安装（例如 `D:\bin` 与 `D:\bin2`）；悬浮窗清理残留实例时只结束同一安装目录下的进程。
- 安装路径统一归一化为绝对路径，避免相对路径被写进插件与 Codex 配置；链式 notify 无法自动更新时给出明确警告。
- 清理只被测试引用的死代码（`connectionText`、`agentMenuLabel` 及不再绘制的文本矩形）。

## [1.8.0] - 2026-09-16

### Changed

- 推送历史改为从文件末尾按块倒序读取，只解析最近的若干条，不再全量解析整个日志；日志超过 4 MiB 自动裁剪旧记录。悬浮窗对历史增加文件级缓存，鼠标移动、重绘与定时刷新不再反复扫描日志。
- 界面字体按 DPI 缓存，绘制不再每帧创建/销毁 5 个 GDI 字体对象。

### Fixed

- 用户操作失败不再静默：设置保存、主题与代理切换失败会明确提示；读取配置失败时不再用默认值覆盖 `config.json`；暂停/开启 Agent 标记写入失败也会提示。
- 通知已发出但本地记录失败时返回警告（CLI 打印到 stderr）；引用回复路由写入失败会提示“引用本条消息将无法续聊”。
- 静默安装新增 `/LOG`，安装器启动即失败时直接报错并给出日志路径，不再无提示地退出悬浮窗。

## [1.7.0] - 2026-09-16

### Added

- 更新安装包在 SHA256 校验之外新增 Authenticode 签名校验：签名校验失败或不可信一律拒绝安装；未签名默认放行，可用 `AGENT_NOTIFY_REQUIRE_SIGNATURE=1` 强制要求签名，或用 `AGENT_NOTIFY_SIGNATURE_THUMBPRINT` 限定信任的签名者指纹。

### Fixed

- 修复卸载会损坏 Codex 配置：不再用整份备份覆盖 `config.toml`，改为只定点还原 notify 行，保留安装后用户对配置的其它修改；无备份时只从 `--previous-notify` 链里摘掉 AgentNotify 片段，不再连带删除用户原有的 codex-computer-use 包装。
- 修复重新登录与常驻轮询之间的凭据竞态：旧 token 的失效响应只作用于发起该次轮询的凭据，轮询结果也不再整结构回写，避免把刚重新登录得到的新 token、游标与会话上下文覆盖回旧值。

## [1.6.2] - 2026-09-16

### Fixed

- 修复“点升级提示当前已是最新版本”：发版流程此前只把产物发布到源码仓库，客户端更新源 `srafyhucl-cpu/agent-notify-releases` 需要手动镜像，漏跑后更新器查不到新版本。现在 Release workflow 会自动镜像安装器、ZIP 与 `SHA256SUMS.txt` 并置为 Latest，镜像脚本也会显式标记 Latest，避免发布顺序影响。

## [1.6.1] - 2026-09-16

### Removed

- 清理悬浮窗重构后遗留的死代码：不再使用的设置、历史、扫码登录对话框及其布局/命中辅助函数、`repairIntegrations`、对话框绘制与消息循环辅助、`summary.go` 中不再使用的内联标记正则，以及 `layout.connection` 等失效字段；同步删除只覆盖旧对话框的测试。无功能变化。

## [1.6.0] - 2026-09-16

### Added

- 桌面悬浮窗支持深色/浅色主题切换（配置项 `theme`，默认深色），顶栏与系统设置页都能切换。
- 悬浮窗内置系统设置、推送历史、微信扫码配置、接入检查修复四个视图，不再弹出独立对话框。
- 主面板支持“全部代理网格”与“收起为单个”两种布局（配置项 `widgetAgentMode`），单代理模式可下拉切换聚焦的代理（配置项 `defaultAgent`）。
- 微信推送加入状态徽标（🟢 正常、⚠️ 需关注），可回复的推送附“引用本消息可继续对话”提示；`agent-notify test` 改为发送带链路状态的 Markdown 卡片。

### Changed

- 托盘图标状态色统一由悬浮窗健康度驱动：等待微信消息为黄色、一切就绪为绿色，只有未登录、登录失效或全部代理暂停才显示红色。
- 推送摘要保留 Markdown 强调与行内代码，连续书写的项目符号自动换行，并清理尾部的分隔线与引用提示残留。

### Fixed

- 应用内扫码登录在微信索要数字配对码时补齐输入框与回车提交；修正配对码消息与刷新消息同号（0x0401）导致请求被忽略的问题。
- 离开“微信配置”视图时会真正取消扫码流程，不再后台持续轮询或写入凭据。
- “检查修复”改为后台执行并把结果交回界面，修复期间不再卡住界面，也消除了跨线程读写界面状态。
- 推送历史列表的点击范围与可见行对齐，卡片底部空白不再选中未显示的第 6 条记录。
- 统一快捷方式名：install.ps1 与标准安装器都创建 `Agent-notify.lnk`，并清理旧版 `Agent-notify 悬浮窗.lnk`，避免升级后桌面和启动项各出现一份。

## [1.5.2] - 2026-09-15

### Fixed

- 一键升级走标准安装器时未指定安装目录，会把程序装到默认的 `%LOCALAPPDATA%\Programs\Agent-notify`；现在把当前 exe 所在目录通过 `/DIR` 交给安装器，升级始终原地进行，自定义目录（例如 `D:\app\Agent-notify`）不会失联。
- README 的自动更新说明补充“升级保留当前安装目录”。

## [1.5.1] - 2026-09-15

### Fixed

- `install.ps1` 会把自身、`uninstall.ps1`、`VERSION`、`tools/hook-config.ps1` 与 `plugin/` 一并部署到安装目录；ZIP 与源码安装也能在悬浮窗首次启动时静默完成接入，不再报“首次配置程序不存在”。
- Codex notify 被 codex-computer-use 用 `--previous-notify` 链式包装且链内指向有效 `agent-notify.exe` 时按已接入处理，不再误报接入异常；安装器只更新链内路径，不拆掉包装。
- 首次接入失败时的提示改为可直接照做的中文说明（缺少 `install.ps1` 时提示重新运行安装器或安装脚本）。
- 安装器替换 exe、插件时遇到客户端正在读取文件会短暂重试，避免首次接入偶发“无法删除要被替换的文件”。

## [1.5.0] - 2026-09-15

### Added

- 新增标准 Windows 安装器 `Agent-notify-Setup-vX.Y.Z.exe`，普通用户无需解压 ZIP 或运行 PowerShell。
- 标准安装版首次启动自动静默完成 OpenCode、Codex、Antigravity、Devin 接入，失败后在悬浮窗显示错误并可通过“检查修复”重试。

### Changed

- 普通用户主安装入口切换为 `Agent-notify-Setup-vX.Y.Z.exe`，默认按当前用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，并提供标准开始菜单、快捷方式和卸载入口。
- 应用内升级优先静默下载并校验新版安装器，校验通过后直接进入安装流程；旧 Release 缺少安装器时继续兼容 ZIP。
- ZIP 包保留给便携运行、开发和旧版客户端过渡，不再作为普通用户的默认安装方式。

## [1.4.1] - 2026-09-15

### Fixed

- 更新检查在 GitHub API 限流或临时不可用时，自动回退到公开 `releases/latest` 跳转解析，不依赖单一 API 配额。

## [1.4.0] - 2026-09-15

### Added

- 悬浮窗新增“升级”按钮：自动检查官方 GitHub Release、下载 `Agent-notify-v<版本>.zip`、校验 `SHA256SUMS.txt`，随后调用新版本安装器更新程序并重启悬浮窗。
- 新增只分发编译产物的公开 Release 仓库，源码仓库保持私有；桌面端默认从公开仓库检查更新。

## [1.3.0] - 2026-09-15

### Added

- 新增 `integration-status`，并让安装器、`status`、`doctor` 与悬浮窗共用同一套 Agent 接入检测；悬浮窗提供“检查接入”入口，可安全恢复被改回的 Codex notify，但不会自动重启任何 Agent。
- 新增首次安装无凭据时自动打开微信扫码，并在安装结束时逐项显示 OpenCode、Codex、Antigravity、Devin 的真实接入状态。

### Fixed

- 修复悬浮窗沿用越界坐标导致右侧被裁切的问题；启动和 DPI 变化时均按目标显示器工作区重新校正窗口边界。
- 修复悬浮窗把“开关开启”误显示为“监听中”的问题；现在区分未接入、待重启、接入异常和已接入，OpenCode / Devin 使用真实加载心跳，Codex 与 Antigravity 校验有效配置和目标程序。
- 修复安装器被 Codex 嵌套 `--previous-notify` 误导而跳过接入的问题，并确保安装结束在 Windows PowerShell 5.1 下正确显示四个 Agent 的真实状态。

## [1.2.0] - 2026-09-15

### Added

- 新增 Antigravity 全局 `Stop` Hook：仅在 `fullyIdle=true` 且存在 `conversationId` 时发送通知，并从 transcript 尾部提取摘要；Hook 始终返回 `{}`，通知失败不会阻塞 Antigravity。
- 新增 Devin 用户级 `Stop` Hook：使用 `session_id` 和 `last_assistant_message`，跳过 `stop_hook_active=true` 的重入事件；Hook 失败不会改变 Devin 的停止决策。
- 新增 Antigravity / Devin 微信引用回复：Antigravity 使用桌面端官方 `language_server.exe agentapi`；Devin 使用随 AgentNotify 安装的桌面扩展，直接向桌面端 `devin.exe acp` 子进程写入 `session/prompt` 续写原会话；两者都不依赖对应 CLI 登录。
- `status`、`doctor`、`toggle` 和悬浮窗统一支持四个 Agent；新增 `antigravity.off` / `devin.off` marker 与对应环境变量覆盖。
- 安装和卸载脚本新增共享 `tools/hook-config.ps1`，以原子替换方式只维护 AgentNotify 自己的 Hook，保留其他 JSON 配置和 handler。

### Changed

- Agent 标识、标题、页脚、推送开关和可回复判定统一收敛到 `internal/agentmeta`，避免四套重复定义。
- OpenCode 与 Devin 复用通用本地 spool 队列，共享原子写入、心跳校验、同步结果等待、异步失败观察和禁止自动重放语义。
- Devin 回复不再启动第二个 Agent 进程：ACP 会话复用桌面端自己拉起的 ACP 子进程，因此不受会话锁、工作区信任和 CLI 登录状态影响。
- Devin 回复扩展安装与卸载纳入正式安装器、发布包和冒烟测试；卸载只删除归属校验通过的扩展文件。
- Antigravity / Devin 安装器只在已有配置或父目录存在时写入 Hook，不会为未安装的客户端创建配置目录。

### Fixed

- Antigravity Hook 改为调用同目录无空格启动器，避免 Windows `cmd /c` 对 exe 外层引号的转义导致 Stop Hook 实际未执行。
- Antigravity 推送优先读取真实会话标题；首次通知时标题文件尚未生成，则从 transcript 首条用户请求生成可识别标题，不再退回“跑完了”。

- Devin 引用回复改为直接写桌面端常驻的 `devin.exe acp` 子进程 stdin（NDJSON `session/prompt`），不再经聊天面板提交，修掉引用回复新开对话、落入 Ask 模式以及误报 CLI 未登录的问题；目标通道缺失、存在多个候选或写入失败都会返回微信可读错误，不向 CLI 或新会话回退。
- Devin 引用回复改用桌面端内部 Cascade 标识：Go 侧按 `session_id` 从 `%APPDATA%\devin\User\globalStorage\state.vscdb` 精确解析后随作业下发，修掉直接把 CLI 会话号当作桌面端标识导致的“目标 Devin 会话不存在”。
- Antigravity 回复只使用当前语言服务实际监听的 HTTP 端口和本次启动的 CSRF token，并在发送前验证目标会话存在。
- release 校验新增 `antigravity.off` / `devin.off`，并对 `tools/hook-config.ps1` 进行打包检查。

## [1.1.0] - 2026-09-13

### Added

- 新增微信引用通知回复：按平台消息 ID 精确关联已记录通知，Codex 通过 `codex queue` 向对应线程续聊，首版默认关闭。
- Codex 冷持久化线程由本地持久队列承接，恢复同一线程后执行；归档、已删除或不存在的线程返回可见错误，不静默切换会话。
- Codex CLI 在 PATH 不可用的开机自启场景下会继续从 `%LOCALAPPDATA%\OpenAI\Codex\bin` 自动发现最新安装，避免引用回复因环境变量差异失效。
- 新增引用路由、入站去重、账号隔离、30 天过期和可见失败提示。
- 路由与去重 JSONL 达到阈值后原子压缩，回收已过期或损坏记录，避免文件无限增长。
- 新增 OpenCode 本地回复收件箱、插件心跳和 `session.prompt` 投递路径，兼容旧版 `promptAsync`，等待第二阶段真实验收。
- 新增 `AGENT_NOTIFY_CLAWBOT_DEBUG=1` 脱敏协议诊断，以及 `doctor` 的 `codex queue` 检查。
- 新增只读命令 `agent-notify reply-check`：自动核对引用 ID 与发送记录、本地路由的对应关系，作为开启“引用回复”前的 P0 闸门（退出码 `0` 通过、`1` 失败、`2` 证据不足）。
- P0 发送证据合并 scoped `sendmessage-result` 与当前账号未过期的本地路由，兼容调试开关未覆盖发送进程的情况，同时继续按账号和过期时间隔离。
- P0 诊断区分普通消息与引用了消息但未返回 ID 的异常样本；引用结构存在、路由缺失、过期或无法读取时一律不通过。
- P0 诊断写入 `account_scope`、`private` 与 `bound_sender`；`reply-check` 只采信当前登录账号绑定用户的私聊样本，跨账号、群聊和升级前旧日志被忽略并计数。
- 核对并记录 OpenCode 桌面端插件投递契约：`session.prompt({ sessionID, text, delivery })` 在入力被持久接纳后返回，不等待整轮任务完成。
- Codex 通知使用只读 SQLite 解析真实会话名，按状态库字段、会话索引和 payload 逐级降级；标题失败时在原通知中显示提示，且不影响引用路由。
- 新增 `codex-title.log` 诊断，记录标题来源、失败阶段、SQLite 错误码、重试次数和降级来源。
- 通知新增协议块解析、本地时间页脚和默认不限长策略；正常心跳静默，异常或未知心跳不会被漏报。

### Changed

- `codex queue` 超时或调用取消按“投递未确认”处理，不自动重试，避免重复发送。
- ClawBot 发送接口返回平台消息 ID 和客户端 ID；历史记录增加可选的 `messageID`、`clientID`。
- Codex notify 提取并保存 `thread-id`（兼容 `thread_id`）；缺失线程 ID 时只发送普通通知。
- 设置界面增加“引用回复”开关，配置文件增加 `replyEnabled`。
- Codex 与 OpenCode 标题统一为 `【codex】会话名` / `【opencode】会话标题`；显式 `--max-chars` 现在按渲染后的完整消息计算。

## [1.0.3] - 2026-09-12

### Changed

- 移除源码安装与构建脚本中的开发机专用 Go 路径；`go.exe` 现在仅从 `AGENT_NOTIFY_GO` 或系统 `PATH` 查找。

### Added

- 增加任意用户目录默认路径回归测试，并确认发布包中的插件模板不携带任何人的绝对安装路径。

## [1.0.2] - 2026-09-12

### Fixed

- 推送时不再拉起可见的控制台窗口：Codex 通知钩子转发上游程序时显式使用 `CREATE_NO_WINDOW` 并隐藏子进程窗口。
- ClawBot 返回 `ret=-2 prepare failed` 时识别为主动推送会话失效，清除本地过期上下文并在状态中重新提示发送微信消息，避免继续误报“会话就绪”。


## [1.0.1] - 2026-09-12

### Changed

- 悬浮窗、设置、历史、登录窗口字号整体放大，并集中到 `internal/ui/ui_fonts.go` 统一按 DPI 缩放。
- 设置窗口的勿扰时段输入框新增格式占位提示。

### Fixed

- “最近推送”卡片的时间与标题不再被截断，标题区域扩展到整卡宽度。
- 悬浮窗改为托盘型工具窗，不再额外占用任务栏按钮，桌面端启动后不会再出现“两个图标”。
- 新增文本宽度回归测试，覆盖 96/144/192 DPI 下关键文案不溢出。

## [1.0.0] - 2026-09-12

### Added

- 新增 Go 单文件运行时 `agent-notify.exe`。
- 新增 ClawBot 2.4.6 二维码登录、配对码、节点跳转、凭据保存、状态查询和有限重试发送。
- 新增首条微信消息建立主动推送会话的 `sync` 命令与会话上下文持久化。
- 新增 OpenCode 全局插件，监听任务完成事件并提取最新 assistant 摘要。
- 新增 Codex notify 接入，保留 `codex-computer-use.exe` 原始参数和 stdin 透传。
- 新增原生 Win32 悬浮窗、托盘、OpenCode / Codex 开关、勿扰设置、推送历史和测试推送。
- 新增设置窗内 ClawBot 二维码登录、重新登录、退出登录和四类连接状态展示。
- 新增 DPI 感知、双缓冲绘制和可滚动的历史详情面板，统一悬浮窗与弹窗视觉语言。
- 新增 JSON Lines 推送历史，区分成功、失败、未登录、会话未建立和跳过状态。
- 新增 `doctor`、`watch`、`status --json`、`toggle` 等运维命令。
- 新增 Go 单测、OpenCode 插件类型检查、PowerShell 静态检查和隔离安装 smoke。
- 新增 GitHub Actions CI 与版本包发布流程。
- 新增 `history --json` 机器可读输出。
- 新增 `go vet ./...` 门禁，插件类型检查提升到 TypeScript `strict`。
- 源码安装注入 `Version`、`Commit`、`BuildTime`，`status` 与 `doctor` 显示真实构建信息。

### Changed

- 安装模型改为发布包中的 `bin/agent-notify.exe` 加 `plugin/agent-notify.ts`。
- 配置、凭据和会话上下文统一保存到 `%USERPROFILE%\.config\agent-notify`。
- 日志与去重状态统一保存到 `%TEMP%\agent-notify`。
- 环境变量统一使用 `AGENT_NOTIFY_*` 前缀。
- marker 统一为 `opencode.off` 和 `codex.off`。
- Codex 配置看护改由悬浮窗定时执行，只在 notify 行仍直指上游程序时恢复。
- 发布包名改为 `Agent-notify-v<版本>.zip`。
- 悬浮窗关闭与最小化只隐藏到托盘，完全退出改由托盘菜单执行。
- ClawBot 成为唯一微信推送通道；设置、登录、历史窗口不再调用旧运行时或外部脚本。
- 扫码登录与主动推送会话明确拆成两个阶段；只有登录不再被视为可发送。
- `ret/errcode=-14` 会将登录标记为失效并停止轮询，避免继续高压重试。
- 安装时把 `$InstallDir` 中的绝对路径写进插件 `BAKED_BIN`，插件按 `BAKED_BIN`、`AGENT_NOTIFY_BIN`、默认目录、`PATH` 顺序解析运行程序。

### Fixed

- 统一放大悬浮窗与弹窗的正文、辅助文字和图标字号，高缩放显示器上的文字不再细小难读。
- 修正 Codex 配置看护和安装器错误地把项目路径中的 `agent-notify` 当作已完成接管的问题；现在只检查实际 `notify` 行，旧 `codex-computer-use.exe` 包装链会被替换。
- 安装器会校验并重建 Windows GUI 子系统二进制，避免把 Console 构建安装后同时出现 Windows Terminal 空白窗口和悬浮窗。
- GUI 子系统程序在 PowerShell 或管道重定向时不再把 stdout 覆盖为控制台设备。
- Codex DryRun 只输出一份 JSON，不再重复打印。
- 安装和卸载 smoke 使用明确文件路径清理，避免误删沙箱外内容。
- 重新登录不会再把失效 token 作为可复用 `local_token_list`。
- 切换 ClawBot 账号时会清空旧账号的游标和会话上下文。
- 会话循环退出时会尽力发送 `notifystop`。
- 修正自定义安装目录下 OpenCode 插件仍去找 `%USERPROFILE%\bin\agent-notify.exe` 导致任务完成不推送的问题。
- 配置与凭据保存改为直接原子替换，写入失败时不再先删掉上一份可用文件。

### Removed

- 删除所有旧运行时、包装脚本、模块加载和兼容入口。
- 删除旧品牌命名、旧配置文件、旧 marker、旧日志和旧环境变量。
- 删除旧安装记录、旧快捷方式及旧发布包命名。
- 不提供旧版本配置迁移或别名；v1.0.0 只使用本文档中的新契约。
