# AgentNotify Rust 核心阶段 A 验收记录

## 验收范围

本记录只验证无 UI 的 Rust 核心闭环，包括领域规则、应用服务、SQLite、Outbox、精确回复路由、Claim、运行时监督、Windows 命名管道和持久化 spool。

本记录不代表真实微信、真实 Agent、Tauri 宿主、React UI、OpenCode、ClawBot、旧数据迁移或生产安装器已经验收。

## 环境

- 日期：2026-09-19
- 系统：Windows
- Rust：`rustc 1.98.1 (48a229cea 2026-09-01)`
- Cargo：`cargo 1.98.1 (797e8a9bc 2026-08-05)`
- 目标：`x86_64-pc-windows-msvc`
- 本地门禁使用项目内 `cargo-xwin + LLVM` 回退，因为当前账户没有 MSVC `link.exe`。
- 正式发布仍必须使用标准 MSVC Build Tools，并通过 `tools\rust\gate.ps1 -RequireMsvc`。

## 数据库基线

- 迁移版本：`1`
- 迁移文件：`crates/agentnotify-storage-sqlite/migrations/0001_init.sql`
- SHA-256：`1314d652de440e0f280fed18292f0e62b3627d45d9e4f5d3e1c87f8936867b68`
- journal mode：`WAL`
- checksum 不一致时启动明确失败，错误码为 `migration_checksum_mismatch`。

## 执行结果

### 全量门禁

命令：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

结果：退出码 `0`。

覆盖：

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features --target x86_64-pc-windows-msvc -- -D warnings`
- `cargo test --workspace --all-features --target x86_64-pc-windows-msvc`
- 单元与集成测试合计 `78` 个，`0` 失败；doc tests `0` 个。

### 阶段 A 验收测试

命令：

```powershell
cargo test -p agentnotify-testkit `
  --test full_flow `
  --test restart_recovery `
  --test duplicate_delivery `
  --target x86_64-pc-windows-msvc
```

结果：退出码 `0`。

| 测试文件 | 通过 |
|---|---:|
| `full_flow.rs` | 1 |
| `restart_recovery.rs` | 5 |
| `duplicate_delivery.rs` | 2 |

已确认行为：

1. 假 Agent 事件写入 Notification 与 Outbox，假渠道成功发送一次，并建立以渠道、账号和外部消息 ID 为键的精确 ReplyRoute。
2. 引用外发消息后，回复被路由到原 Agent 会话，Claim 最终为 `Completed`，重复引用不会再次调用 Agent。
3. 通知和 Outbox 在渠道调用前落库；重启后只投递一次。
4. 渠道返回 `Unknown` 后不会在重启时重新领取或自动重放。
5. 启动时遗留的 `Leased` Outbox 会收敛为 `Unknown`，不会重新发送。
6. 启动时遗留的 `InProgress` Claim 会收敛为 `Unknown`，不会再次调用 Agent。
7. 相同 Agent 事件按幂等键只产生一个 Notification 和一个 Delivery。
8. 并发处理同一引用回复时只有一个调用能够获得 Claim，Agent 只收到一次请求。
9. 迁移 checksum 被篡改后 runtime 拒绝启动，并返回可读中文诊断。
10. Windows 命名管道使用当前用户 ACL；核心离线时 ingress 写入持久化 spool。

## 尚未验收

- Tauri 主窗口、托盘、单实例、自启动和安装升级。
- React UI、动态 Agents/Channels 页面、History、Diagnostics 和 Settings。
- OpenCode 真实 Hook 与 V2 插件。
- ClawBot 扫码登录、微信真实推送、真实引用回复。
- Go 版本旧配置、登录状态、历史和路由的数据迁移。
- 生产签名、发布源镜像、正式安装器和回滚演练。
- macOS 与 HarmonyOS PC 宿主，当前没有测试环境和阶段计划。

以上项目不能根据假适配器测试通过而标记为真实链路验收通过。
