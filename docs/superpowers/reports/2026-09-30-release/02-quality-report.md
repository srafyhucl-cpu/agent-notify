# 整库质量报告（M3，只读审计草稿）

- 审计对象（HEAD）：`8847fa7407d136d42ad792b499965c6f4191c4a9`（分支 `feat/cluster-ui-flow`，detached worktree `D:\Temp\wt-m3-audit`）
- 对比基线：`main`，merge-base `e0f7fedc989136daec8ce97833b8cc7209541aad`
- 本分支 diff：`main...HEAD` = **81 个文件，+10919 / -1368**，28 个提交
- 审计时间：**2026-09-30**
- 审计方式：**只读**（未修改任何源码、未提交、未 push；唯一写入为本报告）
- 结论一句话：**存量问题按计划登记、不阻塞发版；但「本分支新增零违规」不成立**（新增 5 个 >800 行文件、4 个 >80 行生产函数、1 个新越界的 >20 公有方法 `impl`、Rust 生产代码新增裸魔法数字 `80`）。详见 §5、§8。

---

## ① 范围与方法

覆盖 §0 声明的整库范围，本次可用工具与口径：

| 维度 | 命令 / 方法 | 备注 |
| --- | --- | --- |
| 文件行数 | `git ls-files` + `[IO.File]::ReadAllLines().Count` | 含空行（最初用 `Measure-Object -Line` 会漏空行，已纠正） |
| 函数行数 | PowerShell 花括号配对近似（Rust `fn` / Go `func` / TS `function`+箭头） | 近似口径；排除 `tests/` 目录、`*_test.go`、`*.test.*` 及 src 内 `#[cfg(test)]` 之后的行 |
| impl 公有方法 | 扫描 `impl` 块内 `pub(..) fn` 计数 | 只统计 Rust |
| 魔法数字 | `git diff --unified=0 main...HEAD` 逐行取新增行，正则匹配裸字面量 | 精确定位到新增行号 |
| 依赖/安全 | `cargo audit` / `govulncheck ./...` / `npm audit --omit=dev` / 仓库密钥扫描 | 见 §④ |
| 抽查 | `let _ =` / `.ok()` / 空 `catch{}` / Go `_ =` / 依赖方向 / 日志脱敏 / 旧数据兼容 | 见 §⑤（抽查） |

**工具可用性（2026-09-30）**：
- Rust：`cargo 1.98.1`；`cargo-audit` 未预装，尝试安装（见 §④）。
- Go：`go1.26.8`；`govulncheck` 未预装 → `go install golang.org/x/vuln/cmd/govulncheck@latest` 成功（`%USERPROFILE%\go\bin\govulncheck.exe`）。
- 前端：`node v22.23.1` / `npm 10.9.8`；`apps/desktop-ui\node_modules` **不存在**（未 `npm ci`），`tsc/vitest/playwright` 本次未运行。

---

## ② 四张排行表

### ②-1 文件行数 Top30（按语言，含空行）

**Rust（.rs）**

| # | 行数 | 文件 |
| --- | --- | --- |
| 1 | 1906 | hosts/desktop-tauri/src/production/orc_handler.rs |
| 2 | 1608 | hosts/desktop-tauri/tests/production_contract.rs |
| 3 | 1579 | hosts/desktop-tauri/tests/orchestration_commands.rs |
| 4 | 1060 | hosts/desktop-tauri/tests/orchestration_dispatch.rs |
| 5 | 1000 | hosts/desktop-tauri/tests/update_install.rs |
| 6 | 988 | hosts/desktop-tauri/src/bridge/commands.rs |
| 7 | 836 | hosts/desktop-tauri/src/production/orc_node_config.rs |
| 8 | 814 | crates/agentnotify-channel-clawbot/src/session.rs |
| 9 | 811 | hosts/desktop-tauri/src/bridge/dto.rs |
| 10 | 735 | crates/agentnotify-storage-sqlite/src/legacy/mod.rs |
| 11 | 733 | crates/agentnotify-channel-clawbot/src/send.rs |
| 12 | 729 | crates/agentnotify-application/tests/reply.rs |
| 13 | 724 | hosts/desktop-tauri/src/update/install.rs |
| 14 | 715 | hosts/desktop-tauri/tests/orc_report_observer.rs |
| 15 | 713 | crates/agentnotify-channel-clawbot/tests/send.rs |
| 16 | 713 | hosts/desktop-tauri/src/production/orc_watchdog.rs |
| 17 | 706 | hosts/desktop-tauri/src/production/agents.rs |
| 18 | 705 | crates/agentnotify-application/tests/delivery.rs |
| 19 | 696 | hosts/desktop-tauri/src/production/runtime.rs |
| 20 | 669 | hosts/desktop-tauri/src/update/verify.rs |
| 21 | 669 | crates/agentnotify-agent-commandcode/tests/event.rs |
| 22 | 644 | hosts/desktop-tauri/tests/orc_notify.rs |
| 23 | 626 | crates/agentnotify-channel-clawbot/tests/session.rs |
| 24 | 618 | crates/agentnotify-runtime/src/runtime.rs |
| 25 | 616 | crates/agentnotify-orchestration/src/task.rs |
| 26 | 614 | crates/agentnotify-channel-clawbot/src/login.rs |
| 27 | 609 | crates/agentnotify-application/src/reply.rs |
| 28 | 604 | crates/agentnotify-runtime/tests/runtime.rs |
| 29 | 589 | crates/agentnotify-testkit/tests/production_clawbot_loop.rs |
| 30 | 588 | crates/agentnotify-storage-sqlite/src/host_queries.rs |

**Go（.go，旧版，本分支未改动）**

| # | 行数 | 文件 |
| --- | --- | --- |
| 1 | 1123 | internal/ui/widget.go |
| 2 | 940 | cmd/agent-notify/main.go |
| 3 | 732 | internal/ui/widget_views.go |
| 4 | 653 | internal/ui/widget_draw.go |
| 5 | 613 | internal/reply/dispatcher_test.go |
| 6 | 589 | internal/update/update_test.go |
| 7 | 582 | internal/integration/status.go |
| 8 | 531 | internal/clawbot/auth.go |
| 9 | 489 | internal/notify/sender_test.go |
| 10 | 478 | internal/winsqlite/winsqlite_test.go |
| 11 | 463 | internal/reply/lock_test.go |
| 12 | 449 | internal/reply/opencode_test.go |
| 13 | 417 | internal/clawbot/types.go |
| 14 | 401 | internal/reply/gate_test.go |
| 15 | 396 | internal/agent/codex_title_test.go |
| 16 | 378 | internal/update/update.go |
| 17 | 371 | internal/ui/win32.go |
| 18 | 368 | internal/reply/spool_queue_test.go |
| 19 | 366 | internal/reply/spool_queue.go |
| 20 | 363 | internal/clawbot/client.go |
| 21 | 346 | internal/clawbot/session_test.go |
| 22 | 343 | internal/reply/dispatcher.go |
| 23 | 343 | internal/ui/widget_mouse_down.go |
| 24 | 341 | internal/notify/protocol.go |
| 25 | 334 | internal/winsqlite/winsqlite.go |
| 26 | 325 | internal/ui/ui_layout_test.go |
| 27 | 325 | internal/config/config_test.go |
| 28 | 321 | internal/clawbot/auth_test.go |
| 29 | 320 | internal/ui/update.go |
| 30 | 319 | internal/reply/route_test.go |

**TS（.ts）**

| # | 行数 | 文件 |
| --- | --- | --- |
| 1 | 1542 | plugin/rust/agent-notify.ts |
| 2 | 1146 | plugin/agent-notify.ts |
| 3 | 1084 | apps/desktop-ui/src/bridge/mockHostBridge.ts |
| 4 | 936 | plugin/commandcode-v2/agent-notify.ts |
| 5 | 910 | plugin/commandcode-mod/agent-notify.ts |
| 6 | 648 | apps/desktop-ui/src/bridge/types.ts |
| 7 | 431 | apps/desktop-ui/src/test/fixtures.ts |
| 8 | 381 | apps/desktop-ui/src/bridge/hostBridge.test.ts |
| 9 | 326 | apps/desktop-ui/src/features/cluster/labels.ts |
| 10 | 297 | apps/desktop-ui/tests/helpers.ts |
| 11 | 251 | apps/desktop-ui/src/data/mutations.ts |
| 12 | 199 | apps/desktop-ui/src/bridge/tauriHostBridge.ts |
| 13 | 170 | apps/desktop-ui/src/data/accountNames.ts |
| 14 | 158 | apps/desktop-ui/src/features/channels/useChannelLogin.ts |
| 15 | 156 | apps/desktop-ui/src/bridge/hostBridge.ts |
| 16 | 131 | apps/desktop-ui/src/data/errors.ts |
| 17 | 128 | apps/desktop-ui/src/app/theme.ts |
| 18 | 122 | apps/desktop-ui/tests/states.spec.ts |
| 19 | 108 | apps/desktop-ui/tests/visual.spec.ts |
| 20 | 100 | apps/desktop-ui/tests/dynamic-adapters.spec.ts |
| 21 | 86 | apps/desktop-ui/tests/interactions.spec.ts |
| 22 | 85 | apps/desktop-ui/tests/playwright.config.ts |
| 23 | 85 | apps/desktop-ui/tests/smoke.spec.ts |
| 24 | 70 | apps/desktop-ui/tests/accessibility.spec.ts |
| 25 | 62 | apps/desktop-ui/src/data/queryClient.ts |
| 26 | 54 | apps/desktop-ui/tests/navigation.spec.ts |
| 27 | 48 | apps/desktop-ui/src/data/useHistory.ts |
| 28 | 47 | apps/desktop-ui/src/data/useHostEvent.ts |
| 29 | 35 | apps/desktop-ui/src/app/navigation.ts |
| 30 | 31 | apps/desktop-ui/src/data/queryKeys.ts |

**TSX（.tsx）**

| # | 行数 | 文件 |
| --- | --- | --- |
| 1 | 1574 | apps/desktop-ui/src/features/cluster/ClusterPage.test.tsx |
| 2 | 677 | apps/desktop-ui/src/features/settings/SettingsPage.test.tsx |
| 3 | 579 | apps/desktop-ui/src/features/settings/SettingsPage.tsx |
| 4 | 518 | apps/desktop-ui/src/features/cluster/CreateTaskDialog.tsx |
| 5 | 470 | apps/desktop-ui/src/features/channels/ChannelsPage.test.tsx |
| 6 | 467 | apps/desktop-ui/src/features/channels/ChannelsPage.tsx |
| 7 | 465 | apps/desktop-ui/src/features/diagnostics/DiagnosticsPage.tsx |
| 8 | 450 | apps/desktop-ui/src/features/cluster/ClusterPage.tsx |
| 9 | 373 | apps/desktop-ui/src/features/settings/OrcTemplateSettings.tsx |
| 10 | 351 | apps/desktop-ui/src/features/agents/AgentsPage.test.tsx |
| 11 | 347 | apps/desktop-ui/src/features/history/HistoryPage.test.tsx |
| 12 | 341 | apps/desktop-ui/src/features/history/HistoryPage.tsx |
| 13 | 337 | apps/desktop-ui/src/features/channels/ChannelLoginDialog.tsx |
| 14 | 323 | apps/desktop-ui/src/features/cluster/OrcNodeChain.tsx |
| 15 | 285 | apps/desktop-ui/src/components/SchemaForm.tsx |
| 16 | 270 | apps/desktop-ui/src/features/channels/ChannelAccountList.tsx |
| 17 | 270 | apps/desktop-ui/src/features/cluster/TaskDetail.tsx |
| 18 | 268 | apps/desktop-ui/src/app/AppShell.test.tsx |
| 19 | 258 | apps/desktop-ui/src/features/settings/UpdateSettings.tsx |
| 20 | 237 | apps/desktop-ui/src/features/history/HistoryDetail.tsx |
| 21 | 234 | apps/desktop-ui/src/features/history/HistoryTable.tsx |
| 22 | 216 | apps/desktop-ui/src/features/overview/OverviewPage.tsx |
| 23 | 208 | apps/desktop-ui/src/features/cluster/TaskList.tsx |
| 24 | 207 | apps/desktop-ui/src/data/events.test.tsx |
| 25 | 192 | apps/desktop-ui/src/components/RuntimeStatusBar.tsx |
| 26 | 190 | apps/desktop-ui/src/features/overview/RecentDeliveries.tsx |
| 27 | 183 | apps/desktop-ui/src/features/cluster/EditTaskDialog.tsx |
| 28 | 171 | apps/desktop-ui/src/features/cluster/RoundTimeline.tsx |
| 29 | 163 | apps/desktop-ui/src/features/diagnostics/DiagnosticList.tsx |
| 30 | 160 | apps/desktop-ui/src/features/channels/ChannelAccountDetail.tsx |

**CSS（.css）**

| # | 行数 | 文件 |
| --- | --- | --- |
| 1 | 1292 | apps/desktop-ui/src/styles/cluster.css |
| 2 | 1156 | apps/desktop-ui/src/styles/task9.css |
| 3 | 1049 | apps/desktop-ui/src/styles/task7.css |
| 4 | 894 | apps/desktop-ui/src/styles/channels.css |
| 5 | 734 | apps/desktop-ui/src/styles/layout.css |
| 6 | 529 | apps/desktop-ui/src/styles/patterns.css |
| 7 | 227 | apps/desktop-ui/src/styles/tokens.css |
| 8 | 75 | apps/desktop-ui/src/styles/reset.css |

**PowerShell（.ps1）**

| # | 行数 | 文件 |
| --- | --- | --- |
| 1 | 709 | tests/real-opencode-clawbot.ps1 |
| 2 | 668 | tests/smoke.ps1 |
| 3 | 644 | install.ps1 |
| 4 | 436 | uninstall.ps1 |
| 5 | 413 | tests/uninstall-v2-cleanup.tests.ps1 |
| 6 | 398 | tools/release-manifest.ps1 |
| 7 | 387 | tools/hook-config.ps1 |
| 8 | 367 | tools/build-release.ps1 |
| 9 | 355 | tests/signature-gate.tests.ps1 |
| 10 | 336 | tests/restart-acceptance.ps1 |
| 11 | 330 | tests/rollback-smoke.ps1 |
| 12 | 328 | tools/hooks/install-devin-v2.ps1 |
| 13 | 304 | tests/installer-smoke.ps1 |
| 14 | 297 | tests/desktop-installer-smoke.ps1 |
| 15 | 278 | tools/hooks/install-codex-v2.ps1 |
| 16 | 266 | tools/hooks/install-antigravity-v2.ps1 |
| 17 | 235 | tools/hooks/install-commandcode-v2.ps1 |
| 18 | 201 | tools/ui/build-desktop.ps1 |
| 19 | 178 | tests/stale-ui-acceptance.ps1 |
| 20 | 165 | tools/build-installer.ps1 |
| 21 | 160 | tools/publish-release.ps1 |
| 22 | 157 | tools/test.ps1 |
| 23 | 137 | tools/release-gate.ps1 |
| 24 | 120 | tools/ui/quick-build.ps1 |
| 25 | 119 | tools/rust/gate.ps1 |
| 26 | 111 | tools/check-version.ps1 |
| 27 | 106 | tools/rust/bootstrap-xwin.ps1 |
| 28 | 106 | tools/sync-version.ps1 |
| 29 | 94 | tools/lint.ps1 |
| 30 | 77 | tools/cache-report.ps1 |

### ②-2 函数行数 Top30（**生产代码**，排除 tests/、*_test.go、*.test.*、src 内 `#[cfg(test)]`）

命令：花括号配对近似；输出 `行数  文件:起行  函数名`

```
  586  apps/desktop-ui/src/bridge/mockHostBridge.ts:474  invoke
  240  crates/agentnotify-runtime/src/runtime.rs:218  start
  227  internal/ui/widget_wndproc.go:13  handleMessage
  205  hosts/desktop-tauri/src/production/mod.rs:121  bootstrap_internal
  190  hosts/desktop-tauri/src/production/targets.rs:45  resolve
  173  hosts/desktop-tauri/src/update/manifest_cms.rs:17  verify_detached_cms
  163  crates/agentnotify-storage-sqlite/src/legacy/credentials.rs:33  prepare_credentials
  158  internal/ui/widget.go:388  RunWidget
  152  apps/desktop-ui/src/components/SchemaForm.tsx:120  renderField
  146  internal/ui/widget_views.go:253  drawHistoryView
  145  crates/agentnotify-storage-sqlite/src/legacy/mod.rs:165  run
  143  hosts/desktop-tauri/src/production/orc_handler.rs:1263  dispatch_step
  141  internal/ui/widget_draw.go:500  drawViewContent
  139  internal/ui/widget_views.go:419  drawSettingsView
  136  apps/desktop-ui/src/features/channels/useChannelLogin.ts:23  useChannelLogin
  134  crates/agentnotify-application/src/reply.rs:321  handle_inner
  131  apps/desktop-ui/src/bridge/tauriHostBridge.ts:37  dispatchCommand
  131  cmd/agent-notify/main.go:616  runDoctor
  124  hosts/desktop-tauri/src/production/orc_watchdog.rs:430  run_once        <== 本分支新增
  123  crates/agentnotify-storage-sqlite/src/host_queries.rs:197  notification_page
  115  hosts/desktop-tauri/src/production/service.rs:100  get_snapshot
  115  internal/reply/gate.go:113  evaluateReplyGate
  113  crates/agentnotify-application/src/ingest.rs:129  ingest_inner
  112  hosts/desktop-tauri/src/bridge/commands.rs:111  get_update_status
  110  hosts/desktop-tauri/src/production/orc_handler.rs:1411  dispatch_summary
  110  hosts/desktop-tauri/src/production/events.rs:35  start
  109  crates/agentnotify-channel-clawbot/src/login.rs:290  run_login_task
  107  plugin/commandcode-mod/agent-notify.ts:796  start
  107  internal/clawbot/auth.go:171  PollQRStatus
  103  hosts/desktop-tauri/src/lib.rs:16  build_app_with_lifecycle
```

> 口径说明：`mockHostBridge.ts invoke` 为 `src` 下的 mock 桥（供测试/预览用），是整库最大单函数。

### ②-3 `impl` 公有方法数 Top20（Rust）

命令：扫描 `impl` 块内 `pub(..) fn`；输出 `公有方法数  span行数  文件:起行  impl`

```
 39 pub  span= 377  crates/agentnotify-orchestration/src/task.rs:205  impl OrcTask {
 25 pub  span=1422  hosts/desktop-tauri/src/production/orc_handler.rs:143  impl OrcCommandHandler {
 23 pub  span= 412  hosts/desktop-tauri/src/production/runtime.rs:95  impl ProductionRuntimeCoordinator {
 16 pub  span= 196  crates/agentnotify-orchestration/src/store.rs:74  impl OrcStore {
 15 pub  span= 119  crates/agentnotify-domain/src/delivery.rs:117  impl Delivery {
 14 pub  span= 117  crates/agentnotify-channel-clawbot/src/account.rs:23  impl ClawBotAccount {
 13 pub  span= 246  crates/agentnotify-orchestration/src/template.rs:180  impl TemplateResolver {
 12 pub  span= 127  crates/agentnotify-runtime/src/supervisor.rs:80  impl Supervisor {
 12 pub  span=  82  crates/agentnotify-orchestration/src/error.rs:70  impl OrcError {
 11 pub  span= 273  crates/agentnotify-agent-opencode/src/reply_inbox.rs:59  impl OpenCodeReplyInbox {
 11 pub  span= 323  crates/agentnotify-agent-commandcode/src/reply_inbox.rs:72  impl CommandCodeReplyInbox {
 11 pub  span= 165  crates/agentnotify-orchestration/src/workflow.rs:102  impl Workflow {
 10 pub  span=  64  crates/agentnotify-channel-sdk/src/login.rs:74  impl LoginSession {
 10 pub  span=  68  crates/agentnotify-agent-devin/src/adapter.rs:38  impl DeviAgent {
 10 pub  span= 269  crates/agentnotify-agent-devin/src/reply_inbox.rs:60  impl DevinReplyInbox {
 10 pub  span=  98  hosts/desktop-tauri/src/lifecycle/mod.rs:64  impl LifecycleController {
 10 pub  span=  89  crates/agentnotify-agent-commandcode/src/adapter.rs:25  impl CommandCodeAgent {
 10 pub  span=  75  crates/agentnotify-storage-sqlite/src/legacy/mod.rs:653  impl LegacyImportError {
  9 pub  span=  60  crates/agentnotify-channel-sdk/src/adapter.rs:191  impl ChannelError {
  9 pub  span=  58  hosts/desktop-tauri/src/platform/windows/mod.rs:86  impl WindowsPlatformHost {
```

> 含 `pub` 方法的 `impl` 共 195 个；`>20` 的有 **3 个**（`OrcTask` 39、`OrcCommandHandler` 25、`ProductionRuntimeCoordinator` 23）。与基线对比见 §5。

### ②-4 TODO/FIXME 清单 + 生产代码 `unwrap()/expect()/unsafe` 统计

**TODO/FIXME/XXX/HACK**（命令：`git grep -i -E '\b(todo|fixme|hack)\b' -- '*.rs' '*.go' '*.ts' '*.tsx' '*.css' '*.ps1'`）
- **代码零命中**；`todo!()/unimplemented!()` 亦零命中。
- 仅文档里有 2 处字样（计划文档自述、2026-09-15 安装器文档的写作规范），非待办。

**生产代码 `unwrap()/expect()/unsafe`**（口径：排除 `tests/` 目录与 src 内 `#[cfg(test)]` 之后的行）
- 合计：**`unwrap()` 8 次 / `expect()` 88 次 / `unsafe` 63 次**，分布在 60 个文件。
- Top 命中文件：

```
 文件                                                       unwrap expect unsafe 合计
 crates/agentnotify-agent-antigravity/src/discovery.rs           0      1     15   16
 hosts/desktop-tauri/src/update/manifest_cms.rs                  0      0     16   16
 hosts/desktop-tauri/src/update/verify.rs                        0      1     11   12
 hosts/desktop-tauri/src/platform/windows/secrets.rs             0      1      9   10
 apps/ingress/src/windows_pipe.rs                                0      1      8    9
 crates/agentnotify-testkit/src/fake_channel.rs                  2      4      0    6
 crates/agentnotify-testkit/src/fake_agent.rs                    1      5      0    6
 crates/agentnotify-channel-sdk/src/contract.rs                  2      3      0    5
 crates/agentnotify-storage-sqlite/src/legacy/credentials.rs     0      4      0    4
 crates/agentnotify-channel-clawbot/src/inbound.rs               0      4      0    4
```

- `unsafe` 全部集中在 Windows FFI 模块（凭据 `CredReadW/CredWriteW`、CMS 签名校验、命名管道、Win32 窗口、`GetExtendedTcpTable`），属架构预期；**本分支新增行未引入任何 `unsafe`**（`git diff main...HEAD | Select-String '^\+.*\bunsafe\b'` 仅命中计划文档标题）。
- Go 侧：生产 `.go` 中 **`panic(` 零命中**（仅 `internal/diag/diag_test.go` 注释提到）；`log.Fatal/os.Exit` 集中在 `cmd/agent-notify/main.go` 入口。
- TS 侧：`src` 生产代码非空断言 `x!` **零命中**（仅测试文件 3 处）。

---

## ③ 裸字面量清单与新增核对结论

### ③-1 Rust（新增行，已剔除 src 内 `#[cfg(test)]` 之后的行）

**新增生产代码中的裸魔法数字（唯一一处）**：

```
hosts/desktop-tauri/src/production/orc_handler.rs:1173  let mut text: String = detail.chars().take(80).collect();
hosts/desktop-tauri/src/production/orc_handler.rs:1174  if detail.chars().count() > 80 {
```

- 上下文：`ClusterHint`/继续迭代提示的正文截断（`render_cluster_message` 调用前，第 1169–1178 行）。
- `git show main:...orc_handler.rs | Select-String 'take\(80\)|> 80'` → **无输出**，即该裸值 **本分支新增**。
- 同文件已有正确范式 `const FAILURE_DETAIL_LIMIT: usize = 120;`（第 1656 行），此处 `80` 应提为命名常量。
- 其余新增数字均为**命名常量**或 `1_000_000`（纳秒→毫秒单位换算，属换算因子）等可接受上下文；`orc_watchdog.rs` / `opencode_models.rs` 中出现的裸数字全部位于各自 `#[cfg(test)]` 测试模块内。

命名常量（新增，示例，说明分支常量化做得较规范）：`ORC_ROUND_HISTORY_LIMIT=20`、`ORC_ROUND_SUMMARY_LIMIT=1200`、`ORC_MODEL_MAX_CHARS=200`、`ORC_VARIANT_MAX_CHARS=64`、`FAILURE_DETAIL_LIMIT=120`、`WATCHDOG_INTERVAL=30s`、`WATCHDOG_GRACE_MS=60_000`、`REQUEST_TIMEOUT=10s`、`OPENCODE_DEFAULT_PORT=49374`、`LOG_READ_LIMIT_BYTES=4MiB`、`ALERT_INTERVAL=60s`。

### ③-2 TypeScript / TSX（新增行，排除 `*.test.*`）

新增裸字面量候选：

```
apps/desktop-ui/src/bridge/mockHostBridge.ts  ::  ].slice(-20);                        # 逻辑裸值 -20
apps/desktop-ui/src/features/cluster/ClusterPage.tsx   ::  size={15} / size={24}
apps/desktop-ui/src/features/cluster/ContinueTaskDialog.tsx ::  size={18}
apps/desktop-ui/src/features/cluster/CreateTaskDialog.tsx   ::  size={18}
apps/desktop-ui/src/features/cluster/EditTaskDialog.tsx     ::  size={18}
apps/desktop-ui/src/features/cluster/RoundTimeline.tsx      ::  size={13}（2 处）
apps/desktop-ui/src/features/cluster/TaskList.tsx           ::  size={13}（2 处）、size={16}
```

- 说明：图标 `size={N}` 为内联字面量（未抽 design token），属低风险 UI 裸值；`slice(-20)` 为逻辑裸值。
- 反例（做得好）：`OrcNodeChain.tsx` 新增 `const INLINE_MODEL_SAVE_DEBOUNCE_MS = 250;`；`plugin/rust/agent-notify.ts` 新增 `const TERMINAL_RETRY_DELAY_MS = 1200` 与 `20 * MILLISECONDS_PER_SECOND`（命名常量）——即 B4/B5 防抖与退避都做到了命名常量（B5 验收要求）。
- `labels.ts` 的 `[0-9a-fA-F-]{36}` 等为 UUID 正则量词，非魔法数字。

### ③-3 CSS（新增行，共 812 行新增）

- `z-index` 新增 3 处：`1 / 2 / 3`（局部层叠，隔离在 `cluster.css`）。
- 硬编码颜色新增 11 处：**10 处在 `tokens.css` 定义为 `--orc-step-1..5`（含暗色主题覆盖）——放在 token 文件，符合规范**；`cluster.css` 内联 1 处 `box-shadow: ... rgba(0, 0, 0, 0.02)`。
- 其他裸值：`font-weight: 650`（多处，可变字重）、`min(560px, 100%)` 等尺寸字面量——属 CSS 常态，未全部 token 化。

### ③-4 核对结论

> **「本分支新增零裸魔法数字」不成立。**
> - Rust 生产代码：新增裸值 `80`（`orc_handler.rs:1173-1174`）。
> - TS 生产代码：新增 `slice(-20)`、多组图标 `size={N}`。
> - CSS：新增若干尺寸/字重裸值（配色已进 `tokens.css`）。
>
> 若将口径收窄为「Rust 逻辑常量」，仍至少存在 `80` 一处不达标；其余新增数字以命名常量为主，总体常量化程度较高。

---

## ④ 依赖与安全审计结果

| 项目 | 命令 | 结果（2026-09-30） |
| --- | --- | --- |
| Rust 依赖漏洞 | `cargo audit` | **2 个 high 漏洞 + 9 个 allowed 警告**（均为存量依赖，见下） |
| Go 漏洞 | `govulncheck.exe ./...` | **No vulnerabilities found.**（真实执行） |
| 前端依赖漏洞 | `npm audit --omit=dev --registry=https://registry.npmjs.org/` | **found 0 vulnerabilities** |
| 仓库密钥扫描 | `git grep` 关键字 + 跟踪文件后缀 | **零真实命中** |

**cargo audit 说明（真实执行，未伪造）**：
- `cargo audit --version` → 初始 `error: no such command: audit`（未预装）。
- 第一次 `cargo install cargo-audit --locked`（默认环境）→ 编译期失败：`link.exe was not found`（本机无 MSVC），`cargo-audit v0.22.2` 未能产出。
- 第二次改用仓库回退链路 `tools\rust\xwin-env.ps1`（cargo-xwin + `lld-link`，`--target x86_64-pc-windows-msvc`）→ **安装成功**（`Installed package cargo-audit v0.22.2`，耗时 3m29s）。
- 随后 `cargo audit`（在 HEAD worktree）输出：
  - `error: 2 vulnerabilities found!` / `warning: 9 allowed warnings found`；扫描 `Cargo.lock` 596 个 crate。
  - **漏洞（high, CVSS 7.5）**：`quick-xml v0.38.4`
    - RUSTSEC-2026-0194（重复属性名检查二次方运行时间）
    - RUSTSEC-2026-0195（`NsReader` 命名空间声明无界分配 DoS）
    - 修复：升级到 `>=0.41.0`。
  - **警告（9，allowed）**：`paste 1.0.15`（unmaintained）、`proc-macro-error 1.0.4`（unmaintained）、`unic-char-property/unic-char-range/unic-common/unic-ucd-ident/unic-ucd-version 0.9.0`（unmaintained）、`glib 0.18.5`（unsound，RUSTSEC-2024-0429）、`yoke-derive 0.8.3`（yanked）。
- **归属与是否新增**：`quick-xml` 的引入链为 `quick-xml → plist v1.8.0 → tauri-utils v2.9.3 → tauri v2.11.6`；`git show main:Cargo.lock` 中同样是 `quick-xml 0.38.4`，即 **`main` 已存在，非本分支引入**（存量）。9 个警告同为 Tauri/程序宏生态的存量依赖。
- 处置建议：升 `quick-xml` 需等上游 `plist`/`tauri-utils` 跟进（本项目不直接依赖），可登记为存量、随 Tauri 升级一并解决；本次发版不因此阻塞。

**npm audit 细节**：默认 registry 为 `registry.npmmirror.com`，其 `/-/npm/v1/security/*` 未实现（`[NOT_IMPLEMENTED]`）；改指官方 `registry.npmjs.org` 后返回 `found 0 vulnerabilities`（`--omit=dev`）。

**仓库密钥扫描命中口径**：
- `BEGIN ... PRIVATE KEY`、`gho_/ghp_/ghs_/github_pat_`、`password="..."` 赋值：**仅命中计划文档自述行**（`2026-09-30-cluster-v1-release-plan.md:77` 的检查项文字），**无真实密钥**。
- 跟踪文件中的 `.pfx/.pem/.p12/.key`：**0 个**。
- 日志/控制台是否会泄露 token：见 §⑤。

---

## ⑤ 抽查

### ⑤-1 静默吞异常

| 口径 | 数量 | 典型/需关注 |
| --- | --- | --- |
| Rust `let _ =`（生产） | 75 | 多为托盘/事件 emit、临时文件清理、取消信号发送（可接受）；**需关注**：`hosts/desktop-tauri/src/production/orc_watchdog.rs:480-481`（`let _ = task.mark_dispatched(value); let _ = repository.save_task(...)` 吞掉看门狗回注落库错误）、`production/settings.rs:68`、`production/service/settings.rs:85/126/127`（吞写库/关闭错误） |
| Rust `.ok()`（生产） | 59 | 绝大多数是 `parse().ok()?` / `File::open().ok()?` 等「可选即 None」语义，配合 `?` 正常传播；无「把 Result 静默丢弃」的滥用 |
| TS 空 `catch {}`（src） | 6 | `app/AppShell.tsx:26`、`app/theme.ts:70/79`、`components/RuntimeStatusBar.tsx:123/139/155`；均为可选能力探测/清理，属可接受，但建议注释说明 |
| Go `_ =`（生产） | 47 | 多为 `os.Remove/Close/Chmod` 清理；`internal/reply/dispatcher.go:281`（`_ = d.state.Mark(...)`）、`internal/ui/widget.go:434/438`（`_ = agent.HandleWatch`）吞错误，属旧版存量 |

### ⑤-2 分层越界（依赖方向）

命令：解析各 `Cargo.toml` 的 `agentnotify-*` 依赖。

```
domain        -> (无内部依赖)            ✓
agent-sdk     -> domain                  ✓
channel-sdk   -> domain                  ✓
application   -> domain, agent-sdk, channel-sdk   ✓
orchestration -> (无内部依赖)            ✓
channel-clawbot -> application, channel-sdk, domain  ✓（适配器依赖端口，符合）
ingress(app)  -> agent-sdk, domain       ✓
storage-sqlite -> application, channel-clawbot, channel-sdk, domain, orchestration
runtime       -> application, storage-sqlite, ingress(app), ...
```

- 无「domain/application 反向依赖 storage/runtime」的越界。
- **灰色地带（登记）**：`storage-sqlite` 依赖具体渠道实现 `agentnotify-channel-clawbot`（应为 legacy 凭据导入所需）；`runtime` 依赖 `apps/ingress`（app 被 crate 依赖）。建议后续以端口抽象收敛，本次不阻塞。
- 前端**无直接文件系统访问**：`apps/desktop-ui/src` 内 `plugin-fs` / `node:fs` / `readFile` 等 **零命中**，一律走 bridge 命令。

### ⑤-3 死代码 / 未用导出

- 未做独立全量分析（`node_modules` 缺失，`tsc`/`ts-prune` 未运行）；Rust 侧 `dead_code` 由基准门禁 `cargo clippy --workspace --all-targets -D warnings` 覆盖（M1 已跑，本报告未重复运行重门禁，避免与并行 agent 抢 target）。
- 生产代码存在 **7 处 `#[allow(clippy::too_many_arguments)]`**（`application/delivery.rs:117`、`application/reply.rs:233`、`channel-clawbot/session.rs:316/519`、`domain/notification.rs:73`、`production/runtime.rs:96/124`）——已知容忍项，登记。

### ⑤-4 日志脱敏

- `info!/warn!/error!/debug!/trace!/println!/eprintln!/console.*` 与 `token|password|secret|authorization|cookie|api_key|credential` 同现：**零命中**。
- 打印路径类的日志仅 1 处，且在测试文件（`crates/agentnotify-runtime/tests/runtime.rs:491` 的 DEBUG）。
- `apps/desktop-ui/src` 生产代码无 `console.log` 残留。
- 凭据经 `SecretStore` 抽象（`platform/windows/secrets.rs` → Windows Credential Manager）读写；legacy `clawbot.json` 的 `bot_token` 导入后落 `SecretStore`，未见明文落日志。

### ⑤-5 旧数据兼容

- Rust 旧版 Go 遗留文件读取：`storage-sqlite/src/legacy/`（`config.rs`/`credentials.rs`/`routes.rs`/`claims.rs`/`history.rs`/`report.rs`）显式读取 `reply-routes.jsonl` / `reply-state.jsonl`；对应测试 `crates/agentnotify-storage-sqlite/tests/legacy_import.rs`。
- 迁移幂等：`crates/agentnotify-runtime/tests/migration_startup.rs` 有 `valid_import_is_idempotent_and_starts_runtime_after_migration`、`active_legacy_widget_blocks_migration_before_channels_start`；`crates/agentnotify-storage-sqlite/tests/migrations.rs` 有 `reopening_is_idempotent`、`migration_checksum_mismatch_is_rejected`、`future_migration_version_is_rejected` 等。
- 新字段对旧数据宽容：`crates/agentnotify-orchestration/src/task.rs` 大量 `#[serde(default, skip_serializing_if=...)]`；前端 `bridge/types.ts` 中 `variant?: string | null`、`roundHistory` 等对旧任务可缺省。**未见明显缺口（序列化层横向）**。

---

## ⑥ 覆盖率基线（§3.3 步骤 9，可选）

- `cargo llvm-cov --version` → `error: no such command: llvm-cov`（未安装）→ **跳过**。
- `apps/desktop-ui` 的 `devDependencies` 仅有 `vitest`，**无 `@vitest/coverage-v8`**，且 `node_modules` 不存在 → `npx vitest run --coverage` **跳过**。
- 结论：**本机覆盖率工具不可用，按计划记录并跳过，不安装大依赖**。

---

## ⑦ 门槛核对（§4.6）——本分支新增是否超标

命令/口径：逐文件比较 `main` 与 HEAD 行数、函数行数与 `impl` 公有方法数；新增行魔法数字见 §③。

| 门槛 | 本分支新增是否超标 | 证据 |
| --- | --- | --- |
| 文件 > 800 行 | **超标** | 由 ≤800 变 >800（本分支导致）：`bridge/mockHostBridge.ts` 781→1084、`features/cluster/ClusterPage.test.tsx` 642→1574（测试）、`styles/cluster.css` 725→1292、`bridge/dto.rs` 728→811、`production/orc_node_config.rs` 647→836。另有 8 个文件在 main 上已 >800（`orc_handler.rs` 1245→1906 等），属存量 |
| 函数 > 80 行 | **超标（生产）** | 新增或跨过 80：`orc_handler.rs:517 update_task_step` 102、`orc_handler.rs:961 report_from_agent_locked` 92、`orc_watchdog.rs:430 run_once` 124（新文件）、`plugin/rust/agent-notify.ts:1323 dispatchTerminalEvent` 69→86。测试新增长函数（informational）：`orchestration_commands.rs` 3 个（107/157/98）、`orchestration_dispatch.rs` 2 个（127/93）、`orc_node_config.rs` 的 `task_steps_snapshot_requires_agent_every_step` 83（cfg(test)） |
| 单 `impl` 公有方法 > 20 | **超标（新增越界）** | `impl OrcCommandHandler` 19→**25**（`orc_handler.rs:143`，本分支越过 20）；`impl OrcTask` 24→**39**（`task.rs:205`，main 已 >20）；`impl ProductionRuntimeCoordinator` 23→23（存量） |
| 裸魔法数字 | **超标** | Rust 生产新增 `80`（`orc_handler.rs:1173-1174`）；TS 新增 `slice(-20)` 与图标 `size={N}`；CSS 若干（见 §③） |

**判定**：按 §4.6「本分支新增超标…→ 先修再进 §5」，**当前不满足「新增零违规」**，需主会话决定处理批次（见 §⑧ 待治理清单）。

---

## ⑧ 待治理清单（风险排序 + 建议批次）

> 原则：存量登记不阻塞发版；新增违规须在进 §5（合并/版本）前处置或明确豁免。

**P0 — 本分支新增，建议进 §5 前修（对应 §4.6 门禁）**

1. **拆分/常量化 `orc_handler.rs:1173-1174` 裸值 `80`** → 提为 `const`（如 `CONTINUE_HINT_MAX_CHARS`）。改动极小。（新增裸魔法数字，最易达标）
2. **长函数收敛**（>80）：
   - `production/orc_watchdog.rs:430 run_once`（124）— 新文件，可按「读取快照 / 判定 / 回注」拆 3 个小函数。
   - `production/orc_handler.rs:517 update_task_step`（102）— B2 校验逻辑可抽 `validate_step_edit(...)` 或 `validate_variant(...)`。
   - `production/orc_handler.rs:961 report_from_agent_locked`（92）。
   - `plugin/rust/agent-notify.ts:1323 dispatchTerminalEvent`（69→86）。
3. **`impl OrcCommandHandler` 公有方法 25（19→>20）**：或将只读查询分组到子结构体，或对 B2/B3 校验抽独立 `*Validator`，把公有面收敛到 ≤20。
   - （`OrcTask` 39 为 main 已 >20 的存量，可随构件拆分单独排期。）

**P1 — 新增越界但要权衡（可登记后发版，建议紧随 2.1.x patch）**

4. **>800 行文件**：`styles/cluster.css` 1292、`production/orc_node_config.rs` 836、`bridge/dto.rs` 811、`bridge/mockHostBridge.ts` 1084（mock）、`ClusterPage.test.tsx` 1574（测试）。CSS 按区块拆文件、Rust 按职责拆模块；测试文件可豁免（需主会话确认口径——测试/mock 是否计入 >800 门禁）。

**P2 — 静默吞异常/存量（登记，不阻塞）**

5. **依赖安全（存量）**：`quick-xml 0.38.4` 两个 high 漏洞（RUSTSEC-2026-0194/0195，经 `tauri→tauri-utils→plist` 引入）；9 个 unmaintained/unsound/yanked 警告。建议随 Tauri 升级收敛（`quick-xml>=0.41.0`），本次不阻塞。
6. `orc_watchdog.rs:480-481`、`production/settings.rs:68`、`production/service/settings.rs` 的 `let _ =` 改为显式错误处理或记录（新增代码优先）。
7. Go 旧版 `internal/reply/dispatcher.go:281`、`internal/ui/widget.go:434/438` 等吞错（存量）。
8. 依赖灰区：`storage-sqlite → channel-clawbot`、`runtime → apps/ingress`。
9. 移除/收敛 TS 图标 `size={N}` 裸值到 design token（可选，低风险）。

**P3 — 存量登记**

10. `expect()` 88 处、Windows FFI `unsafe` 63 处（架构预期）；`mockHostBridge.ts invoke` 586 行超大函数（mock 层）。
11. 7 处 `#[allow(clippy::too_many_arguments)]`。

---

## ⑨ 结论

- **存量不阻塞发版**：整库 TODO/FIXME 为零；密钥扫描零真实命中；`govulncheck` 与 `npm audit` 无漏洞；旧数据兼容与迁移幂等有测试覆盖；日志未发现敏感信息泄露；`unwrap/expect/unsafe` 均集中在既有模块与 Windows FFI，未见新增 `unsafe`。
- **依赖安全**：`cargo audit` 检出 `quick-xml 0.38.4` 两个 high 漏洞（RUSTSEC-2026-0194/0195）与 9 个警告，**均经 `main` 已存在的 Tauri/程序宏依赖链引入，非本分支新增**；建议随 Tauri 升级收敛，登记不阻塞。
- **新增零违规不成立**（按 §4.6）：新增 5 个 >800 行文件（4 生产 + 1 测试）、4 个 >80 行生产函数、1 个新越界的 `impl`（`OrcCommandHandler` >20）、以及 Rust 生产裸魔法数字 `80`。**需主会话据此决定「先修再进 §5」或显式豁免（尤其测试/mock 文件是否计入 >800 口径）。**
- **未完成/未验证项**：覆盖率基线（`cargo llvm-cov` 与 `@vitest/coverage-v8` 均不可用，明确跳过）；`node_modules` 缺失导致 `tsc/vitest/playwright` 未在本次审计中复跑（由 §3.3 基准门禁负责）。

---

## cargo audit 回填

**已回填**（2026-09-30）：`cargo-audit v0.22.2` 经 `tools\rust\xwin-env.ps1` 回退链路安装成功，`cargo audit` 在 HEAD worktree 实际执行，结果：

```
error: 2 vulnerabilities found!        # quick-xml 0.38.4（RUSTSEC-2026-0194/0195, high 7.5）
warning: 9 allowed warnings found      # paste / proc-macro-error / unic-* / glib(unsound) / yoke-derive(yanked)
```

- `quick-xml` 由 `plist → tauri-utils → tauri` 传递引入，`main` 的 `Cargo.lock` 同为 `0.38.4` → **存量、非本分支新增**。
- 修复路径：升级 `quick-xml >= 0.41.0`（受上游 `tauri-utils`/`plist` 约束），建议登记，随 Tauri 升级处理，不阻塞 2.1.0。
- 原始命令与输出已在上文 §④ 记录；本节仅作汇总。

---

## ⑩ 主会话合成与处置（2026-09-30）

> 本报告由 M3 只读审计子代理产出原始数据与草稿（审计对象 `8847fa7`，基线 `main` `e0f7fed`），主会话以源码为准合成并给出 §4.6 处置决定。修复后分支 HEAD 为 `ad7b408`（见下）。

### 处置一：新增裸魔法数字 —— 已修（不扩大范围、行为不变）
| 项 | 处理 | 证据 |
| --- | --- | --- |
| Rust 生产 `orc_handler.rs` 裸值 `80` | 提为 `const ROUND_INPUT_HINT_MAX_CHARS: usize = 80`（继续迭代微信提示的本轮要求截断） | commit `ad7b408` |
| TS `mockHostBridge.ts` `slice(-20)` | 提为 `const MOCK_ROUND_HISTORY_LIMIT = 20`（对齐后台 `ORC_ROUND_HISTORY_LIMIT`） | commit `ad7b408` |
| CSS 尺寸/字重、图标 `size={N}` | 归入设计 token 待办（P2/P3，低风险 UI 呈现值，不属逻辑裸魔法数字） | 本报告 §③ |

修复后本机门禁：`cargo fmt --all --check` 0 diff、`cargo clippy --workspace --all-targets -- -D warnings` 0 警告、`tsc --noEmit` 0 错误、`vitest run` 140/140 全过（2026-09-30）。

### 处置二：本分支新增结构性超阈值 —— 显式豁免（**待用户确认**）
§4.6 要求「本分支新增超标（文件>800 / 函数>80 / 单 impl 公有方法>20）或裸魔法数字 → 先修再进 §5」。其中**裸魔法数字已在处置一清零**；结构性阈值按下列理由**显式豁免**并在发布后按批次治理，理由与证据如下：

- **判据**：这些是「体量构成」类发现（含测试/mock/CSS），并非功能缺陷。其中 4 处 >80 函数中 2 处（`update_task_step`/`report_from_agent_locked`）是 M1 必修 B1/B2 落地时自然增长；`run_once` 来自本分支看门狗特性。
- **为何现在不修**：在发布候选已通过整库门禁 + CI 之后，对 5 个文件 + 4 个函数 + 1 个 `impl` 做拆分重构，会**扩大范围并让已验证的发布候选失效重验**，与 AGENTS.md「只改和需求相关的代码、不顺手重构」及本次任务「不扩大范围」冲突；且计划 §8 对质量问题的既定策略是「登记清单」。
- **口径问题需用户确认**：`ClusterPage.test.tsx`、`mockHostBridge.ts` 属测试/mock，`cluster.css` 属样式——是否计入「文件>800」门禁口径需用户拍板（本报告按「计入并豁免」处理）。
- **拟治理批次（发布后）**：
  - P0：拆 `orc_watchdog.rs run_once`(124) / `update_task_step`(102) / `report_from_agent_locked`(92) / 插件 `dispatchTerminalEvent`(86)；`impl OrcCommandHandler` 25 个公有方法按域拆 `impl` 块。
  - P1：拆 `styles/cluster.css` 1292 与 `orc_node_config.rs` 836、`bridge/dto.rs` 811；测试/mock 是否豁免按用户口径。
  - P2/P3：存量 `quick-xml` 随 Tauri 升级；看门狗/设置写库 `let _ =` 吞错显式化；Go 旧版吞错。

### 结论（合成后）
- **存量不阻塞发版**：整库 TODO/FIXME 为零、密钥扫描零真实命中、`govulncheck`/`npm audit` 无漏洞、旧数据兼容有测试覆盖、无新增 `unsafe`；`cargo audit` 的 `quick-xml` 两 high 为存量（`main` 同版本），登记不阻塞。
- **新增逻辑裸魔法数字已清零**（处置一）。
- **新增结构性超阈值已按 §4.6 显式豁免、待用户确认**（处置二）；除此之外新增未见功能缺陷或安全违规。
- **未完成/跳过**：覆盖率基线（`cargo llvm-cov`、`@vitest/coverage-v8` 工具均不可用，按计划记录跳过）；`tsc/vitest/playwright` 由 §3.3 基准门禁（本机已全绿）负责。
