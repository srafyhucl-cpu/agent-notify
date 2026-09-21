# Agent-notify 桌面 Rust 重写交接

- 交接日期：2026-09-20
- 当前分支：`codex/rust-desktop-rewrite`
- 当前 HEAD：`b77393e7cc964b83c3a15e5f060098fb5c6c27a8`
- HEAD 提交：`feat(runtime): 支持 Outbox 暂停与宿主查询`
- 工作区状态：有未提交改动，不能回退、覆盖或按旧版本重建
- 接收人：Gemini
- 复核人：Codex

## 1. 任务目标

按以下总计划和四份执行计划继续完成 Windows 桌面端 Rust 重写，最终以 OpenCode +
ClawBot/微信真实链路作为首个生产闭环，完成旧数据迁移、正式切换和后续扩展生态。

计划文件：

- [总开发计划](D:/Project/Agent-notify/docs/superpowers/plans/2026-09-19-desktop-rust-rewrite-development-plan.md)
- [Rust 核心与 Windows 内部入口](D:/Project/Agent-notify/docs/superpowers/plans/2026-09-19-rust-core-and-windows-ingress-implementation.md)
- [Tauri 与 React 桌面 UI](D:/Project/Agent-notify/docs/superpowers/plans/2026-09-19-tauri-react-desktop-ui-implementation.md)
- [首个生产闭环、迁移与切换](D:/Project/Agent-notify/docs/superpowers/plans/2026-09-19-production-loop-migration-cutover-implementation.md)
- [扩展生态](D:/Project/Agent-notify/docs/superpowers/plans/2026-09-19-extension-ecosystem-implementation.md)

当前范围约束：

- 当前只实现 Windows。
- 首个真实闭环只验收 OpenCode + ClawBot/微信。
- macOS、HarmonyOS PC 只保留 `PlatformHost` 边界，不进入当前阶段。
- 不重新引入管理 CLI、管理 MCP。
- Codex、Antigravity、Devin、Command Code、飞书、外部适配器属于后续扩展计划。
- 真实验收必须使用真实 OpenCode、真实 ClawBot 账号和真实微信。测试替身只能证明契约，不能
  代替 Task 9 的生产验收。

## 2. 当前工作区状态

本轮开始前工作区已经是脏工作区。以下改动必须保留：

```text
 M Cargo.lock
 M crates/agentnotify-channel-clawbot/src/login.rs
 M crates/agentnotify-channel-sdk/src/login.rs
 M crates/agentnotify-runtime/src/lib.rs
 M crates/agentnotify-runtime/src/runtime.rs
 M crates/agentnotify-runtime/tests/migration_startup.rs
 M crates/agentnotify-runtime/tests/runtime.rs
 M crates/agentnotify-testkit/tests/support/mod.rs
 M hosts/desktop-tauri/Cargo.toml
 M hosts/desktop-tauri/src/bridge/commands.rs
?? docs/agent-review-prompt.md
```

约束：

- `docs/agent-review-prompt.md` 是用户文件，不要编辑、删除或提交。
- 不要执行 `git reset --hard`、`git checkout --`、`git clean`。
- 不要顺手格式化无关文件。
- 开工前先阅读未提交 diff，确认并沿用现有方向。
- 建议先验证这些未提交改动可编译，再单独提交一个基础提交，不要把它们和新功能混成一个
  不可审查的大提交。

本轮没有修改源码。为了清理项目内可再生产物，只删除了被忽略的
`hosts/desktop-tauri/gen/`，该目录会由 Tauri 构建重新生成。

## 3. 已完成基础

### 3.1 Runtime

`agentnotify-runtime` 已具备：

- `AppRuntime::start(config) -> Result<RuntimeHandle, RuntimeError>`
- 迁移失败时 `start_migration_diagnostics(config, failure)`
- `RuntimeError::Migration(Box<MigrationFailure>)`
- `RuntimeTargetProvider` / `ResolvedRuntimeTargets`
- `RuntimeHandle::{snapshot, subscribe_events, store, ingest, set_outbox_paused,
  outbox_paused, shutdown}`
- Runtime 事件：
  - `NotificationChanged`
  - `DeliveryChanged`
  - `ReplyChanged`
  - `RuntimeStopped`

未提交的 Runtime 改动新增了 `target_provider`，使目标策略在旧数据迁移之后解析。不要恢复成
启动前固定读取目标。

### 3.2 Tauri Bridge

`hosts/desktop-tauri/src/bridge/commands.rs` 已具备：

- `HostCommandService` 的 20 个命令 trait。
- `BridgeState::{unavailable, replace}`。
- 动态 `Arc<RwLock<Arc<dyn HostCommandService>>>` 服务槽。
- 20 个 Tauri command 包装。

当前所有命令仍由 `UnavailableHostCommandService` 返回 `host_command_unavailable`。这是下一步
最直接的实现入口。

### 3.3 ClawBot 登录

未提交改动已经让 `LoginSession` 能保存 `account_id`，并在登录确认、账号持久化后对外发布：

- 早期事件的账号 ID 仍为空。
- `account_id` 只能在账号已持久化后出现。
- 不要用空字符串伪造未绑定账号。

## 4. 当前里程碑

里程碑：把 Tauri 壳从测试壳接到真实 Runtime，并让 20 个宿主命令拥有真实语义。

完成标准：

1. `setup` 不再只安装 `WindowsPlatformHost` 和托盘。
2. 生产 runtime 能启动、迁移、订阅事件、接受命令、退出并完成 SQLite checkpoint。
3. React UI 通过 Tauri command 获得真实 snapshot、Agent、渠道、设置、历史和诊断数据。
4. OpenCode + ClawBot 的设计链路可以被真实宿主调用。
5. 迁移失败进入只读诊断模式，而不是普通启动失败。
6. 测试发送走 `ingest -> SQLite Outbox -> 渠道发送`，不能直接绕过 Outbox。

## 5. Gemini 的第一个动作

不要立即写新功能。先执行：

```powershell
git status --short --branch
git diff --stat
git diff -- hosts/desktop-tauri/src/bridge/commands.rs
git diff -- crates/agentnotify-runtime/src/runtime.rs
```

然后跑当前 Rust 桌面端基线：

```powershell
$env:CARGO_HOME='D:\Tools\cargo'
$env:RUSTUP_HOME='D:\Tools\rustup'
$env:CARGO_TARGET_DIR='D:\Temp\agentnotify-rust-target'
$env:TEMP='D:\Temp\agentnotify-temp'
$env:TMP=$env:TEMP
. .\tools\rust\xwin-env.ps1
cargo test -p agentnotify-desktop --all-targets --target x86_64-pc-windows-msvc
```

如果基线失败，先修复未提交改动或明确记录失败原因，再继续。构建产物和临时文件放在 D 盘
临时目录；任务结束前清理本轮创建的临时内容。

## 6. 推荐实现结构

不要在 `hosts/desktop-tauri/src/lib.rs` 中堆装配代码。新建：

```text
hosts/desktop-tauri/src/production/
  mod.rs
  runtime.rs       # runtime 生命周期、重启、暂停、shutdown
  settings.rs      # SQLite 设置与旧暂停设置兼容
  targets.rs       # RuntimeTargetProvider
  events.rs        # runtime/login 事件到 Tauri 事件的转发
  service.rs       # HostCommandService 实现
```

允许按实际依赖再拆分，但保持职责单一。`lib.rs` 只负责：

- 创建 `BridgeState::unavailable()` 并同步 `manage`。
- 创建 `WindowsPlatformHost`，其中 `SystemUi` 必须使用真实
  `WindowsSystemUi::new(app.handle().clone(), paths.clone())`，不能继续用 unavailable 版本。
- 启动异步初始化任务。
- 初始化成功后替换 `BridgeState`。
- 把 runtime 接到 `LifecycleController`。
- 安装托盘和窗口事件。
- 处理 smoke 退出环境变量。

## 7. Runtime 装配设计

### 7.1 Runtime 槽

使用：

```rust
tokio::sync::Mutex<Option<RuntimeHandle>>
```

原因：`RuntimeHandle::shutdown` 需要 `&mut self`。所有命令必须通过当前 runtime 代理访问，
不能把旧 `RuntimeControl` 永久绑定到迁移前 runtime。

重启流程：

1. 获取重启互斥锁，避免并发重建。
2. 构建新的 `RuntimeConfig`。
3. `AppRuntime::start(config.clone())`。
4. 如果返回 `RuntimeError::Migration(failure)`，调用
   `start_migration_diagnostics(config, *failure)`。
5. 替换 runtime 槽。
6. 停止旧 runtime。
7. 重启状态/事件订阅。
8. 同步生命周期与托盘状态。

### 7.2 启动顺序

必须遵守：

1. 创建应用目录。
2. 打开或准备 SQLite。
3. 执行 schema migration。
4. 检查 `legacyImportV1`。
5. 未导入时执行只读旧数据导入。
6. 导入失败进入 `MigrationRequired`，不启动渠道、Outbox、ingress。
7. 导入成功或已完成时启动 ingress、渠道和 Outbox。

`RuntimeError::Migration` 必须走 `start_migration_diagnostics`，不能当成普通启动失败。

### 7.3 路径

默认路径沿用现有 `AppPaths`：

```text
config_dir = %USERPROFILE%\.config\agent-notify
data_dir   = %LOCALAPPDATA%\AgentNotify\data
log_dir    = %LOCALAPPDATA%\AgentNotify\logs
spool_dir  = %LOCALAPPDATA%\AgentNotify\spool
```

关键派生路径：

```text
SQLite:              data_dir/state.db
OpenCode 回复收件箱: config_dir/opencode-reply-inbox
Legacy:              LegacyPaths::new(config_dir, temp_root, data_dir)
旧进程活动文件:       temp_root/agent-notify/widget-alive.txt
```

### 7.4 适配器

生产注册表至少注册：

```rust
OpenCodeAgent::new(OpenCodeReplyInbox::new(
    config_dir.join("opencode-reply-inbox"),
))
```

```rust
ClawBotChannel::new(secrets).with_account_store(store)
```

登录适配器：

```rust
ClawBotLoginAdapter::new(
    Arc::new(ClawBotHttpClient::new()?),
    secrets,
    store,
)
```

`ClawBotChannel::new(secrets)` 内部使用真实 HTTP transport。运行时应共享注册表，但每次重建
runtime 时不要错误地清掉登录适配器的内存会话。

## 8. 设置设计

SQLite `settings` 表是正式唯一来源。首次读取时兼容旧 `settings.json` 的
`notificationsPaused`，导入后不长期双写。

建议键：

```text
notificationsPaused
notification.quietHours
notification.cooldownMin
notification.defaultAgent
notification.defaultChannelAccountId
reply.enabled
reply.confirmation
reply.routeTtlSeconds
autoStart
startHidden
updateChannel
```

迁移器已导入：

```text
notification.quietHours
notification.cooldownMin
reply.enabled
reply.confirmation
notification.defaultAgent
```

映射规则：

- `delivery_receipt_enabled` -> `ReplyConfig.send_confirmation`
- `route_ttl_seconds` -> ReplyRoute TTL 和 Claim TTL
- `quiet_hours` 旧格式为 `start-end` 小时范围
- `cooldownMin` 是分钟，DTO 是秒
- `updateChannel` 只接受 stable/beta，并映射为 DTO 的 `Stable` / `Beta`

设置持久化后通常需要重建 runtime，因为 policy、delivery targets、reply config 在启动时被
捕获。暂停只改变 Outbox 门控，应优先即时调用 `set_outbox_paused`，无需为仅暂停变化重建。

## 9. RuntimeTargetProvider

`resolve()` 必须在迁移完成后读取：

- `agent_configs`
- `settings`
- `channel_accounts`
- Credential Manager 中的 ClawBot 凭据

要求：

- `NotificationPolicy` 必须为每个已注册 Agent 提供配置。
- 没有显式配置的 OpenCode 要保守启用，否则所有 OpenCode 通知都会被
  `AgentNotConfigured` 跳过。
- ClawBot 的 `conversation_id` 使用凭据中的 `user_id`，不能使用账号 ID。
- `ReplyTarget.bound_sender_id` 与 `private_conversation_id` 使用同一个绑定 `user_id`。
- 多账号阶段只按显式默认账号选择发送目标；没有默认账号时应有确定性的顺序，不能依赖不可控
  的数据库枚举顺序。

## 10. 20 个宿主命令语义

`ProductionHostCommandService` 负责将 `RuntimeSnapshot`、SQLite 查询结果和适配器状态映射为
脱敏 DTO。禁止把 token、context token、原始响应或完整 conversation ID 放入 DTO。

命令行为：

| 命令 | 必须行为 |
|---|---|
| `get_snapshot` | 当前 runtime snapshot + Agent 配置/健康 + 渠道/账号/最近投递 |
| `list_agents` | descriptor 驱动，读取 `agent_configs`，调用 `inspect()` |
| `update_agent_config` | 保存 Agent 启用状态和 config，随后重建 runtime |
| `list_channel_accounts` | descriptor + 所有账号 + 脱敏 health |
| `begin_channel_login` | 只允许 ClawBot，调用登录适配器，二维码只存内存 |
| `submit_channel_login_code` | 提交数字配对码，不落盘 |
| `logout_channel_account` | 调 `ChannelAdapter::logout`，停用账号，重建 runtime |
| `enable_channel_account` | `set_enabled(true)`，重建 runtime |
| `disable_channel_account` | `set_enabled(false)`，重建 runtime |
| `send_test_notification` | 走 `ingest -> Outbox -> channel`，必须选中指定账号 |
| `list_notifications` | 映射 `NotificationQuery` 和分页 DTO |
| `get_notification_detail` | 映射正文、metadata、deliveries、route_exists |
| `retry_delivery` | `requeue_delivery` 后重新查询最新 Delivery 再返回 |
| `get_diagnostics` | runtime components、storage、migration、稳定诊断项 |
| `retry_legacy_migration` | shutdown 旧 runtime，重新构建并替换 runtime |
| `get_settings` | SQLite 设置 + 当前暂停状态 |
| `update_settings` | 校验、保存、即时暂停、必要时重建 runtime |
| `set_runtime_paused` | 同时更新设置和 Outbox 门控；失败要回滚设置 |
| `quit_app` | shutdown runtime、WAL checkpoint，再退出 Tauri |
| `get_update_status` | 当前诚实返回 `Unsupported` |

### 测试发送的账号选择

当前 `DeliveryService` 默认选择第一个启用 target。测试发送指定账号时不能依赖这个隐式行为。

推荐做法：

1. 在通知 metadata 中写 `targetAccountId`。
2. `DeliveryService` 优先选择 metadata 指定的启用账号。
3. 找不到指定账号时明确失败，不回退到其他账号。
4. 测试命令仍通过 runtime `ingest`，并轮询 SQLite 直到拿到 Delivery 终态或超时。
5. 不要直接调用 `ChannelAdapter::send` 绕过 Outbox。

## 11. 事件映射

Runtime 事件转 Tauri 事件：

```text
NotificationChanged -> snapshot.changed
ReplyChanged        -> snapshot.changed
DeliveryChanged     -> delivery.changed
RuntimeStopped      -> 刷新 snapshot 后 snapshot.changed
ClawBot 登录广播     -> channel.login.changed
```

注意：`ChannelLoginChangedEvent.account_id` 当前是必填 `String`。早期登录事件没有账号 ID，
应改为 `Option<String>`，不能写空字符串。

登录进入 `WaitingFirstInbound` 且拿到 `account_id` 后，应重建 runtime，让新账号的长轮询任务启动。
重建要幂等，避免多个登录事件并发触发多次启动。

## 12. 生命周期与窗口

现有 `LifecycleController::attach_runtime` 固定返回 `ShowMain`，尚未依据 `start_hidden` 和
`--autostart` 决定窗口显示。

推荐：

- 增加 `attach_runtime_with_action(runtime, settings, action)`。
- 保留现有 `attach_runtime` 作为默认 ShowMain 的兼容入口。
- `start_hidden` 或 `--autostart` 为真时返回 `KeepHidden`。
- runtime 就绪前主窗口保持隐藏。
- 托盘在 runtime attach 前已安装；暂停状态初始化后同步托盘菜单标签。
- 托盘“退出”和 Bridge `quit_app` 必须走同一个 shutdown 路径。
- 退出后确认 runtime task 全部结束、WAL checkpoint 执行、托盘消失。

`WindowsPlatformHost::from_environment()` 的 `SystemUi` 是不可用实现。生产 setup 中应改用：

```rust
WindowsPlatformHost::with_system_ui(
    paths.clone(),
    Arc::new(WindowsSystemUi::new(app.handle().clone(), paths.clone())),
)
```

## 13. 已知风险与实现陷阱

1. `RuntimeHandle` 不可 Clone，也不能用 `Arc<RuntimeHandle>` 直接调用 `shutdown(&mut self)`。
2. 迁移重试替换 runtime 后，`RuntimeControl` 必须代理新 runtime，不能继续持有旧句柄。
3. `RuntimeError::Migration` 包含 snapshot，必须保留给 UI 诊断。
4. `NotificationPolicy` 缺少 Agent 配置会跳过所有通知。
5. `DeliveryService` 当前只选择第一个启用 target，测试发送指定账号需要显式覆盖。
6. ClawBot 的 conversation ID 是凭据 `user_id`，不是 stable account ID。
7. 登录早期事件没有 account ID，DTO 必须允许空值。
8. `delivery_receipt_enabled` 映射到 `ReplyConfig.send_confirmation`。
9. `route_ttl_seconds` 同时控制 Route TTL 和 Claim TTL。
10. `requeue_delivery` 返回重排前记录，命令必须重新查询最新状态。
11. 登录、Agent 配置、设置、渠道启停都可能要求重建 runtime，否则后台任务不会立即反映变化。
12. 任何网络超时或进程中断都不能自动重放，必须保持 `Unknown`。
13. 正式更新状态当前没有实现，返回 `Unsupported` 比伪造状态正确。
14. 不要将 `docs/agent-review-prompt.md` 加入提交。
15. 删除 `hosts/desktop-tauri/gen` 后，后续 Tauri 构建会重新生成。

## 14. 测试与验收

### 聚焦测试

至少新增：

- 生产 settings 读取/写入与旧 `settings.json` 一次性导入测试。
- RuntimeTargetProvider 的 Agent policy、默认账号、ClawBot user ID 映射测试。
- 20 个命令的 service 合约测试，可用内存 Store、fake publisher、fake action。
- runtime 重建后命令仍操作新 runtime 的测试。
- 暂停失败回滚测试。
- 测试发送必须产生 Notification、Outbox、Delivery 的测试。
- `retry_delivery` 返回重排后最新状态的测试。
- 迁移重试确实 shutdown 旧 runtime 并替换的测试。
- 登录事件 account ID 为空和有值时的事件 DTO 测试。
- start hidden / autostart 的窗口动作测试。

### Rust 门禁

```powershell
$env:CARGO_HOME='D:\Tools\cargo'
$env:RUSTUP_HOME='D:\Tools\rustup'
$env:CARGO_TARGET_DIR='D:\Temp\agentnotify-rust-target'
$env:TEMP='D:\Temp\agentnotify-temp'
$env:TMP=$env:TEMP
. .\tools\rust\xwin-env.ps1

cargo test -p agentnotify-runtime --all-targets --target x86_64-pc-windows-msvc
cargo test -p agentnotify-desktop --all-targets --target x86_64-pc-windows-msvc
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

### 前端门禁

```powershell
npm --prefix .\apps\desktop-ui run test -- --run
npm --prefix .\apps\desktop-ui run build
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

### 全量门禁

```powershell
go test ./...
go vet ./...
gofmt -l cmd internal
node_modules\.bin\tsc.cmd --noEmit
tools\test.ps1
tools\lint.ps1
tools\rust\gate.ps1
tools\ui\gate.ps1
```

### 真实闭环

生产闭环 Task 9 不能以 mock 通过代替。必须准备：

- 隔离的 `AGENT_NOTIFY_*` 目录。
- 真实 OpenCode。
- 独立 ClawBot 测试账号。
- 真实微信。
- 可复核的脱敏证据。

没有第二条独立 ClawBot 账号时，Task 9 必须保持未通过。

## 15. 提交策略

建议按可独立审查的边界提交：

```text
feat(desktop): 接通生产 Runtime 与宿主查询
feat(desktop): 实现登录、设置与测试发送命令
feat(desktop): 接入窗口、托盘与事件生命周期
test(desktop): 覆盖生产宿主命令语义
```

后续真实闭环和扩展计划继续沿用各自执行计划中的提交信息。不要在一个提交里混入无关文件。

## 16. Definition of Done

Gemini 完成本轮后，必须能证明：

- Tauri 生产 setup 已连接真实 runtime 和真实 `SystemUi`。
- Runtime 迁移失败进入诊断模式，成功时启动渠道、ingress 和 Outbox。
- 20 个命令不再返回 `UnavailableHostCommandService`。
- 设置、Agent 配置、渠道启停和登录状态变化能正确重建或控制 runtime。
- 测试发送通过正式 Outbox 链路且能指定账号。
- 退出会 shutdown runtime、停止后台任务并 checkpoint。
- 事件 ID 映射到前端事件，登录早期空 account ID 可序列化。
- 所有聚焦测试、Rust gate、UI gate 通过。
- 真实 OpenCode + ClawBot + 微信验收未完成时，Task 9 和阶段状态不得标记完成。
- 清理本轮临时文件，且没有覆盖用户未提交改动或 `docs/agent-review-prompt.md`。
