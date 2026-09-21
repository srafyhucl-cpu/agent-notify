# OpenCode + ClawBot 真实闭环验收记录

- 状态：**未通过（进行中）**
- 最近更新：2026-09-21 16:40（Asia/Shanghai）
- 对应计划：`docs/superpowers/plans/2026-09-19-production-loop-migration-cutover-implementation.md` Task 9

任何一项真实链路未通过前，阶段状态保持“未通过”，不得进入正式切换。

## 环境与构建

| 项目 | 值 |
| --- | --- |
| 操作系统 | Microsoft Windows 11 家庭版 Insider Preview，10.0.26340 |
| WebView2 Runtime | 153.0.4234.32 |
| AgentNotify Rust Preview | 2.0.0-dev.0（`D:\app\AgentNotify-Rust-Preview`） |
| 预览二进制源码提交 | `ac4548b fix(storage): 统一数据库时间精度并修复 Outbox 租约漂移` |
| 验收探针提交 | `f24ade1 test(e2e): 区分平台受理与用户可见的验收证据` |
| 预览二进制历史修复 | `fedb049 fix(desktop): 退出时完成运行时关闭与 SQLite 检查点`（已包含在当前重建产物中） |
| OpenCode 桌面端 | 2.0.11（`opencode-cli.exe serve --service`） |
| ClawBot 协议版本 | 预览通道 `channel_version=2.4.6`，`iLink-App-ClientVersion=132102`；对照试验使用官方 `2.4.9` |
| SQLite | 普通进程启动时为 `%LOCALAPPDATA%\AgentNotify\data\state.db`；**本文历史证据来自 Codex 包虚拟化路径** `...\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\AgentNotify\data\state.db`（见「LOCALAPPDATA 分裂」）；2026-09-21 16:04 起已在真实路径重新取证 |
| 运行时日志 | 普通进程启动时为 `%LOCALAPPDATA%\AgentNotify\logs\runtime.log`；Codex 会话内启动时该目录位于包虚拟化路径（见「LOCALAPPDATA 分裂」）。`3c90849` 接线后才开始写入，此前为空 |

## LOCALAPPDATA 分裂（2026-09-21 核查，影响本文所有路径描述）

桌面宿主与 ingress 在**不同的 `LOCALAPPDATA` 下工作**，导致两者看到的 `data`、`logs`、`spool` 并不是同一套目录：

| 进程 | 启动者 | `LOCALAPPDATA` 解析到 | 实际落点 |
| --- | --- | --- | --- |
| `agentnotify-ingress` | OpenCode 桌面端（普通进程） | 真实 | `C:\Users\srafy\AppData\Local\AgentNotify\spool` |
| `agentnotify-desktop` | Codex 会话（MSIX 包上下文） | 虚拟化 | `C:\Users\srafy\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\AgentNotify\{data,logs,spool}` |

原因：MSIX 包会把包上下文内进程的 `LOCALAPPDATA` 重定向到 `LocalCache\Local`。

实测（核查 2026-09-21 15:40，spool 移走 16:04）：

- 真实 `%LOCALAPPDATA%\AgentNotify\` 下**只有 `spool`**，没有 `data`、没有 `logs`；该 spool 在移走时有 **119 个文件、去重后 42 个事件**，从未被消费。
- 虚拟化路径下 `data\state.db` 为 1,835,008 字节，内容与本文 2026-09-21 14:53 之前的库证据一致（notifications 366 / deliveries 365 / reply_routes 265 / inbound_claims 59 / outbox 18 / channel_accounts 2）；其 `spool\` 为 0 个文件，`logs\` 为空。

因此：

1. 本文第 11–22 行与各节记录的路径**只对「在普通进程里启动」成立**；在正常 PowerShell 里按文档找不到虚拟化路径下的库与日志。
2. 真实 `LOCALAPPDATA` 里那份 spool 永远不会被 Codex 会话内启动的桌面端消费，这是路径分裂，不是平台风控、也不是程序缺陷。**该缺陷已于 2026-09-21 16:04 修复并重新取证，见本文「修复验证」。**
3. 生产链路上「spool → 桌面端」这一段此前未在真实环境验证过；已通过的输入侧验证全部来自显式指定 `AGENT_NOTIFY_SPOOL_DIR` 的隔离环境。修复后已在真实路径验证（启动消费 spool 与运行中命名管道注入两条路径都通过）。
4. 规则：验收与生产一律在**非 MSIX 包上下文**（普通 PowerShell 或双击快捷方式）启动桌面端；确需在 Codex 内启动时，必须显式设置 `AGENT_NOTIFY_CONFIG_DIR` / `AGENT_NOTIFY_DATA_DIR` / `AGENT_NOTIFY_LOG_DIR` / `AGENT_NOTIFY_SPOOL_DIR` 指向真实目录。

### 修复验证（2026-09-21 16:04–16:09，真实 `%LOCALAPPDATA%`）

修复方式：桌面端改为**非 MSIX 包上下文**启动（由 Explorer 拉起一个临时 `.cmd`，进程由 Explorer 创建）；同时按计划先把积压 spool 整体重命名移走。

| 断言 | 结果 |
| --- | --- |
| 真实 `%LOCALAPPDATA%\AgentNotify\data\state.db` 已创建 | 通过（1,904,640 B） |
| 真实 `logs\runtime.log` 已写入 | 通过（1393 B，含启动 / 初始化完成 / 停止三行） |
| 虚拟化 `state.db` mtime 冻结 | 通过（仍为 2026-09-21 11:45:06，未再被写入） |
| `schema_migrations`（真实库） | `1,2` —— 迁移 `0002` 首次真正应用 |
| legacy 导入（首次启动） | notifications 374 / deliveries 374 / reply_routes 250 / inbound_claims 52 / outbox 0 / channel_accounts 1 / settings 7；`skipped=52`，全部为 `claim_conflict`；1 条 `legacy_quiet_hours_invalid` |
| 投递隔离（注入探针前设置） | `channel_accounts.enabled=0`、`notificationsPaused=true`、`reply.enabled=false`，与 `tests\restart-acceptance.ps1` 使用同一套 SQL |
| **启动消费 spool**（Run 2，探针先落 spool） | spool 1→**0**；notifications 374→**375**；outbox 0→**1**；deliveries 374→**374 不变**；日志出现 `delivery_no_target` |
| **运行中命名管道注入**（Run 3） | spool 保持 **0**，未回退落盘；notifications 375→**376**；outbox 1→**2**；deliveries 不变 |
| 启动收敛中断工作 | 日志 `启动时收敛了中断的投递与回复，记录不会自动重放 interrupted_outbox=1 interrupted_claims=0` |
| 两次退出 | ExitCode `0` / `0`，`state.db-wal` 均为 **0 字节** |
| 积压备份 | `spool-backup-20260921-160401`，119 个文件，未投递、未删除 |
| 托盘四项人工验收（关闭隐藏 / 托盘恢复 / 暂停切换 / 托盘退出） | **待人工确认**（执行方无法点击托盘菜单） |

结论：**「spool → 桌面端」在生产链路上首次验证通过，且两条输入路径（启动消费 spool、运行中命名管道）都成立。** 投递按隔离设置被门控，全程零平台请求。

**账号差异（需注意）**：真实路径首次启动时，legacy 导入按 Go 侧 `clawbot.json`（2026-09-21 10:55 写入）只导入了 **1 个**账号 `clawbot-旧账号`；本文 P0 一节判定为受限的 `clawbot-受限账号` 只存在于虚拟化库（当时该库有 2 个账号），**没有进入真实库**。因此：

- 当前真实库的投递目标是 `clawbot-旧账号`，它是否同样受平台风控**尚未验证**。
- 「受限账号」的既有结论只对 `clawbot-受限账号` 成立，不能直接套用到新库。
- 换第二条账号验收时，应同时确认这两个账号的实际可达性，不要默认其一可用或不可用。

已修复缺陷（本轮发现并修复）：启动/停止日志的 `ipc_status` 曾硬编码为 `"disabled"`，不反映 `config.ingress_pipe_enabled`。已改为按真实开关输出，诊断时该字段现在可信。

---

## 验收结果总览

| 计划步骤 | 结论 | 说明 |
| --- | --- | --- |
| Step 1 隔离契约测试 | 通过 | `tools\rust\gate.ps1`、`tools\test.ps1`、`tools\lint.ps1` 全绿 |
| Step 2 准备隔离验收环境 | **未通过** | 隔离目录与 `-Prepare` 校验已实现并实测通过；仍缺第二条独立 ClawBot 账号，现有证据来自生产账号 `clawbot-受限账号` |
| Step 3 正常推送 | **未通过** | 投递与路由已 `Sent` 且 ID 一致，但微信端不可见；已定位为平台侧风控，见下节。输入侧（真实 spool → ingest → outbox）已在隔离环境验证，并于 2026-09-21 16:04 在**生产路径**首次验证通过（见「修复验证」）；探针现已自动断言 ReplyRoute 精确挂在平台 message ID 上（`cab8872`） |
| Step 4 精确引用回复 | **未通过** | 尚无 `Completed` Claim；`VerifyReply -OtherSessionId` 对照断言已实现并用本地桩验证（目标含文本、对照不含、同一会话报错）；重复投递不重复执行已有用例证据，真实微信回复待第二条账号 |
| Step 5 错误边界 | 部分通过 | 六项边界的代码路径均已用真实 ClawBot 适配器 + 本机假平台离线跑通（`6d37452`：无 Route 引用回发可读提示、引用 ID 冲突拒绝、`ret=-14` 清 context 并标记 stale、`ret=-2` 收敛为 `Skipped/session_missing`、真实 HTTP 超时 → `Unknown` 且不重发），Agent 结果超时与重启不重发由 `restart_recovery` 文件库用例覆盖；`ret=-14`/`ret=-2` 提示链已贯通适配器→宿主 DTO→UI；微信端真实展示仍待第二条账号 |
| Step 6 退出与重启 | 部分通过 | 退出路径已修复并取得真机 checkpoint 证据；新增生产库快照自动化重启验收，两次退出均为 code 0/WAL 0，迁移仅应用一次，历史、Route、Claim、Outbox、账号和设置保持一致；托盘消失与窗口关闭交互仍未逐步记录 |
| Step 7 记录验收结论 | 本文件 | 持续更新 |
| Step 8 提交 | 待完成 | 验收通过后再提交本文件 |

## P0 根因：ClawBot 下行投递被平台侧风控

**现象**：`notifystart` 与 `sendmessage` 均返回 `ret=0`，`sendmessage` 还返回稳定 `message_id`，SQLite 中投递终态为 `Sent`，但微信端完全看不到任何正文，账号轮询 `getupdates` 持续正常。

**排除过程**：

1. 直接使用官方 `channel_version=2.4.9`、`bot_agent=OpenClaw` 与官方 `client_id` 格式直连发送，返回 `ret=0` 与 `message_id`，微信端仍不可见，因此不是 Rust 侧请求体、协议版本或字段格式问题。
2. 换 Bot、干净重绑、重装客户端在同一微信账号下均无效；同一账号下重新分配新 Bot 无效。
3. 腾讯官方仓库存在完全相同症状的多个 Issue：[#264](https://github.com/Tencent/openclaw-weixin/issues/264)（`sendmessage ret=0` 但不可见，换微信账号恢复）、[#266](https://github.com/Tencent/openclaw-weixin/issues/266)（typing 可见、正文静默丢失）、[#268](https://github.com/Tencent/openclaw-weixin/issues/268)（维护者回复“由于风控影响”）、[#280](https://github.com/Tencent/openclaw-weixin/issues/280)、[#290](https://github.com/Tencent/openclaw-weixin/issues/290)、[#303](https://github.com/Tencent/openclaw-weixin/issues/303)、[#304](https://github.com/Tencent/openclaw-weixin/issues/304)、[#305](https://github.com/Tencent/openclaw-weixin/issues/305)、[#310](https://github.com/Tencent/openclaw-weixin/issues/310)。

**结论**：故障跟随微信用户账号，不跟随 Bot、代码、网络或主机。部分用户 48 小时左右自行恢复，部分用户更换微信账号后立即恢复。

**处置规则**：

- 不再向该账号重复发送验收探针，避免加重限制。
- Step 3/Step 4 必须改用另一个微信用户账号重新验收；仅更换 Bot 或重绑无效。
- `Sent` 只表示平台受理并返回消息 ID，不能作为“用户已收到”的证据；探针输出已改为 `push=PLATFORM_ACCEPTED` 与 `pushVisibility=manual_confirmation_required`，防止把平台受理记成通过。

## 为什么必须换第二个微信账号

1. **计划门禁**：Task 9 Step 2 写的是“必须使用独立 ClawBot 测试账号；如果无法提供第二条独立账号，不能把向当前账号发送测试消息记作隔离验收通过”。用生产账号跑，结论按规则就不成立。
2. **当前账号拿不到证据**：风控跟随微信用户账号（不跟随 Bot、代码或机器），`sendmessage` 返回 `ret=0` 与稳定 `message_id`、SQLite 记 `Sent`，但微信端什么都不显示。看不到通知就无法引用它，Step 3（用户可见）和 Step 4（引用→路由→Agent）都拿不到真实证据。
3. **验收会做破坏性动作**：Step 5 要真实触发 `ret=-14`（清 context token、账号标 stale、要求重新扫码）与 `ret=-2`，还要压力场景下的超时。这些不应在用户唯一的生产账号上制造，否则会影响正常推送。

第二个账号要做的事只有四步：扫码绑定一个新 Bot → 给它发一条消息建立 context → 收到通知并确认微信端可见 → 引用它回复唯一 token 并把时间告诉执行方；随后同一个账号还要复测 `ret=-14`/`ret=-2` 提示。

当前账号仍可用的部分：入方向（用户→Bot）没有被风控，引用一条**仍可见且未过期**的旧通知可以单独验证“引用→路由→Agent→Claim”这条回复腿；但这只能作为补充证据，不能替代 Step 3。


## P0 修复记录（会话就绪时机）
**问题**：Rust 版在 runtime 启动时立即调用 `notifystart`，此时尚无 context token；失败被 `debug` 日志吞掉且不再重试。Go 版是在会话就绪后调用 `announceSessionStart`。

**修复**：`crates/agentnotify-channel-clawbot/src/session.rs`

- 移除启动时的提前调用；
- 每个成功轮询后检查 context 是否就绪，就绪后发送一次 `notifystart`；
- 发送失败不置位，后续成功轮询自动重试；
- 回归测试：`notify_start_waits_until_session_context_is_established`、`notify_start_failure_is_retried_on_next_successful_poll`。

**验证**：`cargo test -p agentnotify-channel-clawbot --test session` 5 项通过；`tools\rust\gate.ps1` 全量通过。

## 运行时日志接线

**问题**：桌面宿主此前以 `telemetry: None` 启动运行时，`%LOCALAPPDATA%\AgentNotify\logs` 始终为空，P0 只能靠 SQLite 反推证据。

**修复**：

- `hosts/desktop-tauri/src/production/runtime.rs`：运行时配置写入 `TelemetryConfig { log_path: <log_dir>/runtime.log }`。
- `crates/agentnotify-runtime/src/telemetry.rs`：`init_telemetry` 改为进程级幂等——全局 subscriber 只安装一次，重复初始化切换写入文件而不是报 `telemetry_init_failed`；日志超过 8 MiB 时在启动阶段轮换为 `runtime.log.1`。
- 回归测试：`crates/agentnotify-runtime/tests/telemetry_init.rs` 覆盖“重复初始化 + 超大日志轮换 + 日志落盘”，`hosts/desktop-tauri/tests/production_contract.rs` 的 `runtime_restarts_repeatedly_and_maintains_running_state` 覆盖连续重启不失败。

## 引用回复拒绝提示（Step 5）

**问题**：Rust 版在路由或 Agent 阶段拒绝引用回复时只写日志，微信端没有任何反馈；Go 版会通过绑定私聊回发可读原因。

**修复**（`e0121c0 feat(reply): 引用回复被拒时向绑定会话回发可见原因`）：

- `ReplyRejection::notice()` 为无可用路由、引用冲突、Agent 缺失/不支持/失败/结果未确认生成微信可读文案；
- `ReplyService` 在 Claim 进入 `Failed` 或 `Unknown` 后通过同一渠道发送 `MessagePurpose::ReplyRejection`；发送失败只写 tracing 与 StatusStore，不改变 Claim 终态、不重试、不写 ReplyRoute；
- 账号、发送者或会话未通过绑定校验的拒绝不回发提示，避免向非绑定用户暴露内部原因。

**证据**：`cargo test -p agentnotify-application --test reply` 16 项通过（其中 6 项覆盖拒绝提示），`tools\rust\gate.ps1` 全绿。

**限制**：提示是否真实出现在微信端仍需独立账号验收；当前账号受平台风控，不能作为展示证据。


## 重启与持久化证据（Step 6）

以预览重启时间 `2026-09-20T12:15:45Z` 为界：

- `deliveries` 中 `updated_at > 12:15:45Z` 的记录：0 条（重启未重发）。
- `inbound_claims` 中 `updated_at > 12:15:45Z` 的记录：0 条（重启未重放回复）。
- `outbox` 全部为 `Done`（无 `Pending`/`Leased` 残留）。
- `deliveries` 无重复 `(notification_id, channel_id, account_id)` 分组。
- 当时 `schema_migrations` 仍只有首次一行，二次启动未重复迁移。
- 历史通知 361 条、投递 360 条、路由 260 条、Claim 53 条均在重启后保留。
- 2026-09-20T13:30:27Z 预览进程（环境回收后重新拉起）再次验证：deliveries、inbound_claims 在重启时间之后均为 0 条，outbox 无未完成记录，投递总数仍为 361 条，重启不重发。

**生产库快照自动化重启验收（2026-09-21 14:51）**：新增 `tests\restart-acceptance.ps1`，使用 SQLite `.backup` 复制生产库，在副本中禁用渠道账号并把 runtime 置为暂停，随后连续启动两次预览桌面程序。副本位于 `D:\Temp\agentnotify-restart-acceptance-20260921-145155`，脚本退出时已自动清理；全程不访问 ClawBot、不读取生产 spool。

| 断言 | 结果 |
| --- | --- |
| 源数据库 SHA256 / 桌面二进制 SHA256 | `0536105E6BF1C417434C0EA268BF26C629C0AA9A532B2DAD25024787FCE0F8D8` / `DDF4B69FF745FEED2F1A02D294146FB2C5CAC9E25262A4E2B0E5E0BEAF33E448` |
| `schema_migrations` | 副本为 `1,2`（迁移 `0002` 仅应用一次，第二次启动迁移表完全一致）；**当时的生产库本身仍为 `1`**——`0002` 只在副本上应用过，直到 2026-09-21 16:04 在真实路径首次启动才真正应用（见「修复验证」） |
| 重启后记录数 | notifications=366、deliveries=365、reply_routes=265、inbound_claims=59、outbox=18、channel_accounts=2、settings=14 |
| 数据身份校验 | Notification、Delivery、Route、Claim、Outbox、账号和设置摘要均保持不变，无重复历史、无重复投递 |
| 两次退出 | ExitCode `0` / `0`，WAL 均为 `0` 字节，无残留桌面进程 |
| 网络隔离 | `channel_accounts.enabled=0`、`notificationsPaused=true`、`reply.enabled=false`，副本不会触达真实平台 |

结论：Step 6 第 2～4 项已由生产库快照自动化通过；第 1 项的无残留进程与 WAL checkpoint 也通过，但“托盘图标消失”和“托盘菜单点击退出”仍需人工视觉确认，因此 Step 6 暂不整项记为通过。
## 隔离验收环境准备（Step 2）

**问题**：计划 Step 2 指定的 `tests\real-opencode-clawbot.ps1 -Prepare` 原本不存在（脚本只支持 Status/Send/VerifyReply），且计划里的 `AGENT_NOTIFY_OPENCODE_PLUGIN_DIR` 在代码中并无对应实现。

**修复**：脚本新增 `-Prepare`，建立 `config`/`data`/`logs`/`spool`，校验 ingress 与预览桌面二进制、输出启动环境变量、比对插件烘焙的 ingress 路径，并对生产数据目录直接报错（`-UseProductionDataDir` 才放行）；`-InstallPlugin` 可选刷新固定路径的 OpenCode 插件。计划 Step 2 已同步删除不存在环境变量并补充隔离约束。

**最新实测**（2026-09-21 14:41，隔离根 `D:\Temp\agentnotify-real-e2e-20260921-144139`，验收后已清理）：

| 断言 | 结果 |
| --- | --- |
| `prepare=READY` / `isolated=True` / `stateDbExists=False` | 通过 |
| ingress SHA256 | `8FECD20BBF4828E995A64B78D731FDAE5B694E484B68942FD07883AD4B34DB8D` |
| 预览桌面 SHA256 | `DDF4B69FF745FEED2F1A02D294146FB2C5CAC9E25262A4E2B0E5E0BEAF33E448` |
| 插件烘焙路径与当前 ingress 一致 | `pluginMatchesIngress=True` |
| 传入生产数据目录 | 明确报错，未创建或修改任何目录 |
| `-InstallPlugin` 幂等 | 重复执行后插件 SHA256 不变（`38E015B0...FCAE2`） |
| 原有 `-Mode Status` 未受影响 | 对生产库只读查询正常输出 |

## 失效账号提示链路（Step 5）

计划要求 `ret=-14` 后“账号标记失效、context token 被清除、UI 提示重新扫码”，`ret=-2` 后“提示向 ClawBot 发一条消息恢复”。本轮把这条链路逐层补齐并加上了缺失的测试：

| 层 | 行为 | 证据 |
| --- | --- | --- |
| 适配器 | `ret=-14` → 清 context token、置 `stale_at`、返回 `clawbot_invalid_account` | `agentnotify-channel-clawbot`：`send_invalid_account_clears_context_cursor_and_marks_account_stale`、`ret_minus_14_marks_account_stale_and_clears_session_state`、`login_http` 剧本服务返回 `ret=-14` |
| 适配器 | `ret=-2` → `Skipped/session_missing` 并标记 stale | `agentnotify-channel-clawbot/tests/send.rs`（`session_missing` 回执 + `stale_at` 落库） |
| 适配器 | `inspect` 把 stale 映射为「ClawBot 会话已失效，请重新扫码或发送消息恢复」 | `src/adapter.rs`；新增宿主测试断言该文案 |
| 宿主 DTO | `list_channel_accounts` 把 `health.stale/detail` 透传到 `ChannelHealthDto` | `hosts/desktop-tauri/tests/production_contract.rs::list_channel_accounts_reports_stale_and_missing_credentials`（`c3d7469`） |
| UI | 账号列表按 stale/不可用/停用显示状态标签与明细文案 | `apps/desktop-ui/src/features/channels/ChannelsPage.test.tsx`（`d963d4f`） |

宿主测试使用 `http://127.0.0.1:9` 作为账号 base_url 并只断言 DTO，不会向真实平台发送请求；日志文案「未找到该账号的渠道密钥，请重新登录」与 stale 文案并存，两者都是可操作提示。

**仍未验证**：真实平台返回 `ret=-14` / `ret=-2` 时，上述提示在微信端与桌面窗口上的实际展示（需第二条账号与可用会话）。

## 下次启动前的投递目标与积压处理（Step 2/3 前置）

**事实（2026-09-21 12:30 快照，未执行任何发送）**：

| 项目 | 观测值 |
| --- | --- |
| **真实** `%LOCALAPPDATA%\AgentNotify\spool`（ingress 侧）积压 | 44 个 `agent.event` 文件，全部 `session.completed`，跨度 09:07～11:25，来自 3 个会话 |
| 去重后事件数 | 17 个 `idempotencyKey`（同一事件被 3 个 OpenCode 实例窗口各写一份，属预期，靠幂等键折叠） |
| `notification.defaultChannelAccountId` | `null` |
| 隐含第一顺位账号 | `clawbot-受限账号`（受限账号，`session_established_at` 2026-09-21 01:08 +08，最新） |

**跟踪（2026-09-21 13:12）**：spool 已增至 **50** 个文件。新增的 6 个来自同一个会话在 13:10:54 与 13:12:14 的两次 `session.completed`，每次被 3 个 OpenCode 实例各写一份（靠幂等键折叠）。这证明应用未运行期间事件确实在排队而不丢失；仍**未投递、未清理**。

**跟踪（2026-09-21 13:40）**：spool 为 **53** 个文件，仍未投递、未清理；当前无 `agentnotify-desktop`/`agentnotify-ingress` 进程在运行。

**跟踪（2026-09-21 16:04）**：spool 增至 **119** 个文件、去重后 **42** 个事件（3 个会话，跨度 09:07～15:56），仍未投递、未清理。经核查该目录属于**真实** `%LOCALAPPDATA%`，而 Codex 会话内启动的桌面端读取的是虚拟化路径下的空 spool，因此这些事件不会自行被消费（见「LOCALAPPDATA 分裂」）。已整体重命名为 `spool-backup-20260921-160401`（只移不删）。

`agentnotify-application` 的投递在通知未带 `target_account_id` 时选择**第一个 enabled 目标**（`delivery.rs`），顺序由 `production/targets.rs` 决定：显式默认账号 > 最近建立主动推送会话 > 账号 ID。因此当前直接启动预览会把 17 条积压事件全部投给受限账号，既拿不到可见证据，也会继续向该账号发送。

**建议顺序（待用户确认后执行，本轮未执行）**：

1. 先换第二个微信账号扫码绑定，并向 ClawBot 发一条消息建立会话，使新账号成为第一顺位；
2. 或在启动前把受限账号 `channel_accounts.enabled` 置 0，避免它抢到未指定目标的投递；
3. 若只想验收单条探针，先把整个 `spool` 目录移动到 `D:\Temp\agentnotify-spool-backup-<时间戳>` 备份，再启动，避免 17 条积压一次性灌入新账号（首次绑定的账号可能因此触发平台风控）。

积压事件属于用户真实工作内容，本轮**未删除、未清理、未发送**；清理或备份前需用户确认。

## 投递输入侧隔离验证（Step 3 前置，2026-09-21 12:18）

把真实 `spool` 的 44 个文件**复制**到隔离 spool（原目录未移动、未删除），用预览二进制 + 隔离 `config`/`data`/`log`/`spool` 启动 9 秒后正常退出。因为隔离环境没有任何绑定账号，本次不会向平台发送任何消息。

| 断言 | 结果 |
| --- | --- |
| spool 剩余文件 | 0（44 个全部 ack，无 quarantine 目录） |
| 解析告警 | 0（`runtime.log` 无“隔离无效/无法处理的离线入口事件”） |
| 新增 notifications | 17（365 → 382），17 个 `idempotencyKey` 每个恰好 1 条 |
| 新增 outbox | 17 行，对应 17 个不同 notification |
| 新增 deliveries | 0（仍为旧数据迁移的 365），未触达平台 |
| 无账号时行为 | 17 条 Outbox 保持 `Leased`，worker 报 `delivery_no_target` 并等租约到期重试，不丢弃 |

**结论**：真实插件产出的载荷能被 Rust ingress 完整解析，同一事件被多个 OpenCode 窗口重复写入时靠幂等键折叠为一条；未绑定账号不会丢消息，但一旦绑定的账号成为第一顺位，这 17 条会立即投递（印证上节建议）。

## 退出检查点（Step 6）

**问题**：桌面 smoke 退出直接调用 `app.exit(0)`，绕过 `LifecycleController`，SQLite WAL 未完成 checkpoint，无法保证退出后回滚窗口内的一致性。

**修复**（`fedb049`）：`schedule_smoke_exit` 改为先调用 `shutdown_for_quit()` 再退出，与托盘退出（`lifecycle/tray.rs`）和 `quit_app`（`production/service.rs`）共用同一条优雅关闭路径。
契约测试 `quit_app_checkpoints_wal_and_stops_runtime` 断言退出后 `state.db-wal` 长度为 0、`integrity_check` 通过、`current_snapshot()` 为 `None`。

**真机证据**（2026-09-21 12:11，隔离目录 `D:\Temp\agentnotify-preview-smoke-20260921-121122`，验收后已按临时文件规则清理；`AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS=6000`；桌面二进制 SHA256 `087CDD62D25C4CEA957CF45CB85BF49EAD95365C69A326DE9FFD883E80AFFAFC`）：

| 断言 | 结果 |
| --- | --- |
| 进程退出码 | `0` |
| `data\state.db-wal` | 0 字节（checkpoint 完成） |
| `logs\runtime.log` | 636 字节，含“启动桌面运行时”“桌面生产运行时及宿主服务初始化完成”“停止桌面运行时” |

**限制**：Step 6 第 1 项中的“无 UI/runtime 进程、托盘消失”未逐项截图或逐步记录；本次仅覆盖 smoke 退出与宿主关闭路径，托盘菜单点击退出仍待人工确认。

**最新重建证据**（2026-09-21 14:40，隔离目录 `D:\Temp\agentnotify-preview-smoke-20260921-144049`，验收后已清理；`AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS=6000`；桌面二进制 SHA256 `DDF4B69FF745FEED2F1A02D294146FB2C5CAC9E25262A4E2B0E5E0BEAF33E448`）：

| 断言 | 结果 |
| --- | --- |
| 进程退出码 | `0` |
| `data\state.db-wal` | 0 字节（checkpoint 完成） |
| `logs\runtime.log` | 636 字节，含“启动桌面运行时”“桌面生产运行时及宿主服务初始化完成”“停止桌面运行时” |

## 本轮离线补强（2026-09-21，待第二条账号）

| 提交 | 内容 |
| --- | --- |
| `ac4548b fix(storage): 统一数据库时间精度并修复 Outbox 租约漂移` | 固定 SQLite 时间为三位毫秒，新增 `0002_canonical_timestamps.sql` 统一旧库时间格式；迁移执行器补齐 checksum、未来版本与版本缺口校验，并增加旧库升级、未来版本、迁移缺口和 Outbox 租约边界回归测试。 |
| `0b6988f test(e2e): 为引用回复验收增加对照会话断言` | `VerifyReply` 新增 `-OtherSessionId`：目标会话必须含唯一回复文本、对照会话必须不含；目标与对照同 ID 直接报错；未提供对照时输出 `controlSessionContainsReply=UNKNOWN`，不构成 Step 4 通过证据。已用本地桩验证四条路径（目标含/对照不含 = PASS，对照泄漏 = 失败，同一会话 = 失败，目标缺失 = 失败）。 |
| `8768799 test(smoke): 全绿后清理自建临时沙箱避免残留` | `tests\smoke.ps1` 全绿后按「临时根前缀 + `agent-notify-smoke-<32位十六进制>`」校验兜底清理自建沙箱；失败时保留现场供排查。复跑 `tools\test.ps1` 后 `D:\Temp` 无残留目录、无清理告警。 |
| `d06e464 test(e2e): 隔离验收入口提示补齐环境继承步骤` | `-Mode Prepare` 的 `next` 提示补上「OpenCode 必须与预览程序在同一组 `AGENT_NOTIFY_*` 环境变量下启动」等 5 步，避免插件把事件写进生产 `spool`。 |
| `cab8872 test(e2e): 校验 ReplyRoute 精确绑定平台消息 ID` | `-Mode Send` 不再把 Step 3 第 4 条留给人眼：投递终态后等待 `(channel_id, account_id, external_message_id)` 精确匹配的 ReplyRoute，断言目标会话与 `agent=opencode`、未过期，并输出 `routeMatchesDelivery=true`。`-Mode Status` 同时输出 `routeMatchesDelivery=true/false/UNKNOWN`，用三组本地夹具验证（路由一致 / 路由挂在其它 message ID / 尚无平台 ID），VerifyReply 回归仍 PASS。 |
| `69fb3f0 test(clawbot): 增加真实适配器离线闭环测试` | 新增 `crates\agentnotify-testkit\tests\production_clawbot_loop.rs`：用真实 `ClawBotChannel` + `ClawBotAccount` + `ClawBotCredentials` + `normalize_inbound` 拼上真实 `IngestService`/`DeliveryService`/`ReplyService`，平台侧用本机假 HTTP 服务。断言出站确实打 `POST /ilink/bot/sendmessage` 且带授权头；ReplyRoute 挂在平台返回的稳定 `message_id`（而非标题/正文）；引用该消息回复时 Claim 转 `Completed`、Agent 恰好 resume 1 次且会话正确；引用无路由消息时返回 `Rejected(NoExactRoute)` 且不再唤起 Agent。 |

| `6d37452 test(clawbot): 补齐无路由提示与失效账号离线边界` | 同一测试文件扩到 6 个用例：无路由引用除拒绝外还必须向绑定会话回发可读原因（断言出站正文含 `ReplyRejection::NoExactRoute.notice()` 文案且 `client_id` 以 `reply-notice-` 开头）；引用 ID 冲突在归一化阶段就被拒（`clawbot_reference_conflict`，不唤起 Agent、不发出站）；`ret=-14` 经真实适配器后 Delivery `Failed/clawbot_invalid_account`、context token 被清除、账号落库 `stale_at` 且 `inspect` 返回 stale 提示；`ret=-2` 后 Delivery `Skipped/session_missing`、不建路由；真实 HTTP 传输超时（500ms）后 Delivery `Unknown/clawbot_send_result_unknown`，再次调度 `Idle` 且平台只收到 1 次请求。 |
| `aa57330 feat(e2e): 验收探针支持按回复文本反查命中会话` | `tests\real-opencode-clawbot.ps1` 新增 `-Mode LocateReply`：按时间窗（默认 30 小时）取候选路由、按会话去重后逐个导出 OpenCode 会话，用唯一回复文本反查命中的 `sessionId`，同时自动挑一个未命中的对照会话并打印可直接复制的 VerifyReply 命令；单会话导出失败不中断扫描但计入 `exportFailedCount`；命中多个会话时直接报错并列出哈希。该模式会输出原始 `sessionId` 供下一步命令直接使用，**写入验收记录时只保留 `sessionHash`**。 |

门禁：`tools\lint.ps1` 通过（24 个脚本语法 + PSScriptAnalyzer Error 级 + 版本一致性）；`tools\test.ps1` 通过（Go 单测/vet/格式 + 插件类型与状态机 + `SMOKE ALL GREEN`）；`tools\rust\gate.ps1` 在 `cab8872`、`69fb3f0` 与 `6d37452` 上全绿（`cargo fmt --check` + `clippy -D warnings` + workspace 全量测试）。

该测试使用内存 SQLite（`:memory:`）而不是临时目录：`SqliteStore` 的连接由专用线程持有，落盘的临时库会在测试结束时因删除竞态残留成 `D:\Temp` 垃圾。

预览二进制：已按 `ac4548b` 重建并覆盖 `D:\app\AgentNotify-Rust-Preview`（桌面 SHA256 `DDF4B69F...E448`，ingress SHA256 `8FECD20B...DB8D`）。新桌面二进制已在隔离目录通过 6 秒退出 smoke：退出码 `0`、WAL 为 0 字节、日志包含完整的启动与停止记录；新 ingress 也通过 `-Mode Prepare` 的路径与哈希校验。

未推进的原因：本轮无法操作微信（`computer-use` 再次返回 `apps: []`、浏览器返回 `Codex auth token is unavailable`），且受风控账号无法作为收信证据，Step 2~4 仍需另一个微信用户账号。

### Step 5 / Step 4 可离线构造项（2026-09-21 实测）

`cargo test -p agentnotify-testkit --target x86_64-pc-windows-msvc`（整包通过）：

| 计划项 | 用例 | 结果 |
| --- | --- | --- |
| ClawBot 发送超时 → Delivery `Unknown`，重启后不重发（Step 5） | `restart_recovery::unknown_delivery_is_not_reclaimed_after_restart` | 通过：重启后 `process_next()` 返回 `Idle`，`pending_outbox_count=0`，delivery 仍只 1 条 |
| 投递中被中断 → `Unknown`，重启不重发（Step 5） | `restart_recovery::runtime_marks_interrupted_outbox_unknown_without_resending` | 通过：重启后无渠道调用，`recent_error=runtime_recovered_interrupted_work` |
| OpenCode result 超时 → Claim `Unknown`，重启后不重发（Step 5） | `restart_recovery::runtime_marks_interrupted_claim_unknown_and_never_resumes_again` | 通过：`AlreadyClaimed{state=Unknown}` 且 `resume_count=0` |
| 同一入站事件不重复执行（Step 4） | `duplicate_delivery::duplicate_quoted_reply_resumes_agent_once` | 通过：并发重复投递只 1 次 `Accepted` |
| 重复 ingress 事件只产生一条通知与投递（Step 3） | `duplicate_delivery::duplicate_ingress_event_creates_one_notification_and_delivery` | 通过 |
| 假 Agent + 假渠道全链路建路由并精确回复 | `full_flow::fake_agent_to_fake_channel_creates_exact_reply_route` | 通过：路由命中目标会话、Claim `Completed` |
| 生产适配器契约 | `production_contracts::production_adapters_pass_shared_contracts` | 通过 |
| 真实 ClawBot 适配器 + 本机假平台全链路（Step 3/4 代码路径） | `production_clawbot_loop::production_clawbot_adapter_creates_route_from_platform_message_id` | 通过：出站命中 `/ilink/bot/sendmessage` 且带授权头；路由键用的就是平台返回的 `msg-1`；路由指向目标会话与 `agent=opencode` |
| 引用路由消息命中 / 未命中（Step 4） | `production_clawbot_loop::production_clawbot_loop_replies_only_to_quoted_route` | 通过：命中路由时 Claim `Completed`、`resume_count=1`、会话正确；引用无路由消息时 `Rejected(NoExactRoute)` 且 `resume_count` 仍为 1 |
| 无路由引用的可见提示（Step 5） | `production_clawbot_loop::production_clawbot_loop_replies_only_to_quoted_route` | 通过：向假平台发出的第二个请求正文包含微信可读提示，`client_id` 为 `reply-notice-*`，证明拒绝不是只写日志 |
| 引用 ID 冲突（Step 5） | `production_clawbot_loop::production_clawbot_conflicting_reference_is_rejected_before_agent` | 通过：`clawbot_reference_conflict`，无 Agent 调用、无出站请求 |
| `ret=-14` 失效链路（Step 5） | `production_clawbot_loop::production_clawbot_invalid_account_clears_context_and_marks_stale` | 通过：Delivery `Failed/clawbot_invalid_account`、context token 被清除、账号 `stale_at` 落库、`inspect` 返回 stale |
| `ret=-2` 会话失效（Step 5） | `production_clawbot_loop::production_clawbot_session_missing_skips_delivery_without_route` | 通过：Delivery `Skipped/session_missing`、context token 被清除、不建立引用路由 |
| 真实传输超时（Step 5） | `production_clawbot_loop::production_clawbot_send_timeout_records_unknown_without_resend` | 通过：Delivery `Unknown/clawbot_send_result_unknown`、无消息 ID、再次调度 `Idle`、平台仅收到 1 次请求 |

说明：这几项验收对象是“超时/中断/重复投递时运行时自己的行为”，用受控替身构造即可成立，无需真实平台；但 `ret=-14`/`ret=-2` 与微信端展示仍必须真实账号。

## 可选：不等第二条账号先验证“回复腿”

受风控影响的只是**下行展示**；入方向（用户→Bot）一直是通的。生产库里已有 265 条未过期 ReplyRoute，其中 09-20 下午到晚上那批通知在风控开始前应当是可见的，可以用它们先验证引用回复链路：

**可用时间窗口（2026-09-21 13:40 核查）**：09-20 那批路由的 `created_at` 为 UTC 09:28～12:22（北京时间 17:28～20:22），TTL 24 小时，因此它们在**北京时间 2026-09-21 17:28～20:22 之前**仍然有效；今天 09:07～09:28（北京时间）那批创建于风控期间，微信端本来就看不到。窗口过后只能等第二条账号。

1. 先把生产 `spool` **移**到 `D:\Temp\agentnotify-spool-backup-<时间戳>`（不删；截至 2026-09-21 16:04 已有 119 个文件、42 个去重事件，已整体移至 `spool-backup-20260921-160401`），避免启动后把积压一次性发给受限账号。
2. 启动生产预览实例（不要用隔离环境，否则监听不到生产账号的入站）。
3. 在微信里找到 ClawBot 私聊中**仍看得到**的一条通知（优先最近的那条），引用它并回复一个唯一 token，例如 `AGENT_NOTIFY_REPLY_OK_20260921`。
4. 执行 `-Mode LocateReply -ReplyText <token>`：探针按时间窗把最近路由的会话去重后逐个导出比对，直接给出命中的 `sessionId`、路由时间与一个未命中的对照会话，并打印可直接复制的 VerifyReply 命令；token 命中多个会话或一个都没命中时会明确报错。
5. 用上一步打印的命令跑 `-Mode VerifyReply -SessionId <命中会话> -OtherSessionId <对照会话> -ReplyText <token>`，取得 Step 4 第 2~4 条的运行层证据。

这只能补上“引用→路由→Agent→Claim”，**不能**代替 Step 3 的微信可见推送；计划门禁仍以第二条账号的完整闭环为准。

**LocateReply 自测（2026-09-21）**：用临时 `.cmd` 桩替换 `opencode-cli`（桩已删除，未进仓库）验证三条路径：桩只对目标会话回显 token → `locate=FOUND` 且给出正确的 `sessionId` 与对照会话；桩对所有会话回显同一 token → 正确报“命中多个会话”并列出 5 个哈希；对生产库用不存在的 token 跑真实会话导出 → “6 个候选会话都没有出现验收回复文本”。同一提交后 `-Mode Status` 在真实生产库上仍正常，仍报 `routeMatchesDelivery=true`。

## 已知限制与待补项

1. **未使用独立测试账号**：当前使用用户生产 ClawBot 账号，按计划不能记为隔离验收通过；且该账号已受平台侧风控限制。
2. **微信端展示未确认**：`Sent` 只代表 ClawBot 接受请求，必须由用户在微信端确认收到。
3. **引用回复未闭环**：需要用户引用本次通知回复唯一文本，且 Claim 必须从 `InProgress` 变为 `Completed`；还必须提供对照会话，证明回复文本没有串入其它会话（`VerifyReply -OtherSessionId`）；无 Route 引用的可见提示也必须在微信端确认展示。
4. **Claim 无法按被引用消息 ID 关联**：`inbound_claims` 只保存回复消息自身 ID，探针改为“同渠道账号 + 路由创建时间窗 + 唯一回复文本落库”三段证据。
5. **`session export --sanitize` 会脱敏正文**：探针已改为内存中未脱敏比对，不落盘正文。
6. 未覆盖：Codex、Antigravity、Devin、Command Code 等其它 Agent；飞书等其它渠道；macOS 与 HarmonyOS PC。
7. **隔离验收依赖环境继承**：OpenCode 桌面端必须与预览程序在同一组 `AGENT_NOTIFY_*` 环境变量下启动，否则插件会把事件写进生产 `spool`；探针不代为设置这些变量。
8. **受限账号与真实库账号不一致**：真实库导入的 `clawbot-旧账号` 是否同样受平台风控尚未验证；「受限账号」既有结论只对 `clawbot-受限账号` 成立。详见「修复验证」下的「账号差异」。
9. **启动方式曾是验收缺陷来源**：桌面端必须在非 MSIX 包上下文启动，否则 `LOCALAPPDATA` 会被重定向（见「LOCALAPPDATA 分裂」）。后续新增的验收脚本不得在包上下文里启动桌面端。

## 复现步骤（未通过项）

1. 准备隔离验收环境（隔离根固定为 `D:\Temp\agentnotify-real-e2e`）：
   ```powershell
   $env:AGENT_NOTIFY_CONFIG_DIR = 'D:\Temp\agentnotify-real-e2e\config'
   $env:AGENT_NOTIFY_DATA_DIR = 'D:\Temp\agentnotify-real-e2e\data'
   $env:AGENT_NOTIFY_LOG_DIR = 'D:\Temp\agentnotify-real-e2e\logs'
   $env:AGENT_NOTIFY_SPOOL_DIR = 'D:\Temp\agentnotify-real-e2e\spool'
   powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\real-opencode-clawbot.ps1 -Mode Prepare
   ```
2. 先退出正在运行的 OpenCode 桌面端和生产预览实例：命名管道按当前用户 SID 派生，同一用户只能跑一个桌面实例；而且已经启动的 OpenCode 不会继承隔离环境变量。
3. 在**同一个**已设置上述环境变量的 PowerShell 窗口里启动两个程序，让插件写事件时沿用隔离 `spool`：
   ```powershell
   Start-Process "$env:LOCALAPPDATA\Programs\@opencode-aidesktop\OpenCode.exe"
   Start-Process 'D:\app\AgentNotify-Rust-Preview\agentnotify-desktop.exe'
   ```
   省略这一步会让 OpenCode 插件把事件写进生产 `spool`，隔离实例看不到任何事件。

   > 第 2、3 步必须在**非 MSIX 包上下文**（普通 PowerShell 或双击快捷方式）中执行；在 Codex / 商店应用会话内执行会把桌面端的 `LOCALAPPDATA` 重定向到包 `LocalCache`，本流程将读不到同一个 `spool`（见「LOCALAPPDATA 分裂」）。
4. 在 Channels 中用**另一个微信用户账号**扫码绑定：换 Bot 或重绑同一账号无效，风控跟随微信账号而不是 Bot；随后从该账号向 ClawBot 发一条普通消息建立 context。
5. 确认账号在轮询：隔离库 `channel_accounts.updated_at` 持续前进。
6. 在 OpenCode 中开两个会话（两个不同目录的任务），分别记下 target 与 control 的 session id，供第 8 步对照。
7. 执行：
   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\real-opencode-clawbot.ps1 `
     -Mode Send -SessionId <session-id> -TargetAccountId <account-id> -ReplyText <unique-token>
   ```
8. 在微信端确认收到通知（`push=PLATFORM_ACCEPTED` 不代表已展示），再引用它回复 `<unique-token>`，随后执行：
   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\real-opencode-clawbot.ps1 `
     -Mode VerifyReply -SessionId <session-id> -OtherSessionId <control-session-id> -ReplyText <unique-token>
   ```
   正式验收必须提供 `-OtherSessionId`：探针会导出对照会话并确认它不含同一回复文本，否则输出 `controlSessionContainsReply=UNKNOWN`，不构成 Step 4 通过证据。目标会话与对照会话为同一 ID 时直接报错。
9. 若失败，读取隔离目录自身的 `logs\runtime.log`、`inbound_claims` 最新状态与 `opencode session export <session-id>` 判断是路由失败还是 Agent 未接纳。
10. 验收通过后：隔离环境里的绑定和凭据不回流生产；生产实例需要用自己的数据目录重新扫码绑定同一测试账号，绑定后生产 `spool` 里那 17 条积压才会投递（投递前先把 spool 复制备份一份更保险）。

隔离验收不会投递生产 `spool` 里积压的事件（截至 2026-09-21 16:04 为 119 个文件、42 个去重事件，已整体移至 `spool-backup-20260921-160401`）；这些真实工作事件仍留给生产实例，不受本次验收影响。
