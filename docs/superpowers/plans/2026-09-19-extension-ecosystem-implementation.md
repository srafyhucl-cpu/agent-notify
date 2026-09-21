# 扩展生态 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Windows 正式闭环稳定后，以独立适配器增加 Codex、Antigravity、Devin、Command Code、飞书和外部 Agent 进程协议，同时证明新增适配器不需要修改 UI 页面、通知服务或回复核心。

**Architecture:** 每个内置 Agent 使用独立 crate 和内部 Hook 入口，所有 Hook 最终只提交版本化 `AgentEventEnvelope`。飞书作为渠道 crate 实现登录、token、出站、长连接和事件去重。外部 Agent 使用带 manifest 的独立进程协议，不采用动态库 ABI，也不能访问应用数据库和密钥库。

**Tech Stack:** Rust、Tokio、`reqwest` + rustls、`tokio-tungstenite`、JSON Schema、JSON-RPC 2.0、OpenCode/Codex/Antigravity/Devin/Command Code 客户端通信、Playwright。

## Global Constraints

- 本计划只在 Windows 替代版完成正式切换后执行；macOS 与 HarmonyOS PC 不进入本计划。
- 每个 Agent 或渠道只新增自身 crate、内部 Hook/插件、注册项和测试；不得修改 Overview、Agents、Channels、History、Diagnostics 或 Settings 的业务分支。
- 所有内部 Hook 只能提交 `protocolVersion=1` 的 `agent.event`；不提供状态查询、配置修改、渠道登录或退出命令。
- 外部适配器只能通过 manifest 声明能力和入口；不得直接访问 SQLite、Credential Manager、应用日志或任意文件。
- 外部进程协议破坏性变更必须提升协议版本；旧协议在明确弃用版本前继续可用。
- 飞书事件必须验签、去重并保留 event ID；无稳定消息 ID 时不建立 ReplyRoute。
- 多账号策略不得使用“最近账号”或“默认账号”兜底；通知选择或回复路由都必须有显式规则。
- 每个适配器必须通过共享 contract、错误映射、脱敏、隔离和真实链路验收。
- 所有用户可见错误必须说明下一步动作；日志不得记录消息正文、token、secret、cookie 或完整外部账号 ID。
- 每个任务结束运行 `tools/rust/gate.ps1`、相关 `tools/ui/gate.ps1` 和适配器测试。

---

### Task 1: 建立内置 Hook 打包与适配器认证规范

**Files:**
- Create: `crates/agentnotify-testkit/src/adapter_certification.rs`
- Create: `tools/hooks/hook-common.ps1`
- Create: `tools/hooks/test-hook-entry.ps1`
- Create: `docs/superpowers/specs/2026-09-19-adapter-authoring.md`
- Modify: `crates/agentnotify-testkit/src/lib.rs`
- Test: `crates/agentnotify-testkit/tests/adapter_certification.rs`

**Interfaces:**
- Consumes: `AgentAdapter`、`ChannelAdapter`、`agentnotify-ingress.exe`。
- Produces: `AdapterCertification::{Agent, Channel}`、统一 Hook 打包规则和新增适配器检查表。

- [ ] **Step 1: 写认证测试**

```rust
#[tokio::test]
async fn certified_agent_must_pass_contract_and_ingress_protocol() {
    let result = certify_agent(Arc::new(FakeAgent::new("future-agent"))).await;
    assert!(result.contract_passed);
    assert!(result.ingress_protocol_passed);
    assert!(result.no_fixed_ui_id_passed);
}

#[test]
fn manifest_rejects_unversioned_external_protocol() {
    let error = AdapterManifest::parse(fixture_manifest_without_version()).unwrap_err();
    assert_eq!(error.code(), "adapter_protocol_version_missing");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-testkit --test adapter_certification
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\hooks\test-hook-entry.ps1
```

Expected: FAIL，认证工具和 Hook 门禁不存在。

- [ ] **Step 3: 定义认证范围**

Agent 认证必须检查：

- descriptor ID、能力和 config schema 可解析。
- 事件通过 ingress 协议进入核心后能生成 Notification。
- `resume` 能力与实际实现一致。
- 空会话、空正文、未知事件和不可用客户端返回明确错误。
- Hook 调用有界返回，客户端关闭时不会挂住 Agent。
- 错误无法影响上游 Agent 的继续执行。

Channel 认证必须检查：

- 多账号游标、密钥、限流和状态隔离。
- 登录、发送、入站、logout 和健康状态的能力一致。
- Retryable、Permanent、Unknown 和 Skipped 映射准确。
- 声明 reply routing 时，成功回执必须有稳定外部消息 ID。
- 事件重复投递不会重复分发。

- [ ] **Step 4: 规定 Hook 交付**

内置 Hook 可以是：

- 单文件插件，例如 OpenCode/Command Code。
- 专用无管理命令的 exe，例如 Codex/Antigravity/Devin。
- 平台配置文件中的命令，但必须调用专用 Hook exe 或 ingress。

Hook 不得：

- 接受 `status`、`login`、`logout`、`history`、`config`、`quit` 等子命令。
- 回退到最近会话、工作目录或标题匹配。
- 写 SQLite 或 Credential Manager。
- 执行 hook 输入中的路径、URL 或命令。

- [ ] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-testkit --test adapter_certification
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add crates/agentnotify-testkit tools/hooks docs/superpowers/specs/2026-09-19-adapter-authoring.md
git commit -m "test(adapters): 增加内置适配器认证规范"
```

---

### Task 2: 增加 Codex Agent 适配器与兼容 Hook

**Files:**
- Create: `crates/agentnotify-agent-codex/Cargo.toml`
- Create: `crates/agentnotify-agent-codex/src/lib.rs`
- Create: `crates/agentnotify-agent-codex/src/event.rs`
- Create: `crates/agentnotify-agent-codex/src/title.rs`
- Create: `apps/hooks/codex/Cargo.toml`
- Create: `apps/hooks/codex/src/main.rs`
- Create: `tools/hooks/install-codex-v2.ps1`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-agent-codex/tests/event.rs`
- Test: `apps/hooks/codex/tests/passthrough.rs`

**Interfaces:**
- Consumes: Codex `notify` JSON stdin、`thread-id` / `thread_id`、现有 `codex-computer-use.exe`。
- Produces: `CodexAgent` 与 `agentnotify-codex-hook.exe`。

- [x] **Step 1: 写事件和透传顺序测试**

```rust
#[test]
fn codex_thread_id_is_required_for_resume_capability() {
    let adapter = CodexAgent::new_test();
    let event = adapter
        .parse_event(codex_envelope("thread-1", "任务完成"))
        .unwrap();
    assert_eq!(event.session_id.unwrap().as_str(), "thread-1");
}

#[tokio::test]
async fn upstream_runs_before_ingress_and_failure_does_not_block_upstream() {
    let fixture = codex_hook_fixture();
    fixture.handle().await;
    assert_eq!(fixture.calls(), vec!["upstream", "ingress"]);
    assert!(fixture.exit_code().is_success());
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-codex
cargo test -p agentnotify-codex-hook
```

Expected: FAIL，Codex 包不存在。

- [x] **Step 3: 实现 Codex 适配器**

Capabilities：

```rust
AgentCapabilities {
    notify: true,
    resume: true,
    session_title: true,
    hook_installer: true,
    reply_window: false,
}
```

事件要求：

- `thread-id` 优先，兼容 `thread_id`。
- 标题按现有顺序迁移：Codex 状态库 `threads.name` → `threads.title` → `threads.first_user_message` → `session_index.jsonl` → payload 首条消息 → `"跑完了"`。
- 正文取 `last-assistant-message`。
- 缺失 thread ID 时仍可推送，但不生成 ReplyRoute。
- 标题失败时使用默认标题并在正文标记降级来源，不伪造会话 ID。

- [x] **Step 4: 实现 Codex Hook 顺序**

`agentnotify-codex-hook.exe`：

1. 读取有界 stdin。
2. 查找当前 Codex 使用的 `codex-computer-use.exe`。
3. 原样透传参数与 stdin 给上游。
4. 等待上游完成，但总时长受上限控制。
5. 生成标准 AgentEventEnvelope。
6. 调用 `agentnotify-ingress.exe`。
7. ingress 失败吞掉；Hook 始终不改变上游退出语义。

安装器只修改 Codex `notify` 行，继续保留 `config.toml.bak-notify-wrapper` 和自定义 notify 保护规则。

- [x] **Step 5: Resume 精确入队**

Codex `resume` 直接执行：

```text
codex queue --thread=<thread-id> --message=<text>
```

- 使用参数数组，不经过 shell。
- 30 秒未确认返回 `Unknown`，不重试。
- 已归档线程返回可读错误并提示先恢复/解档。
- 永久线程、临时线程和不存在的线程有不同错误码，不落到最近会话。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-codex
cargo test -p agentnotify-codex-hook
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-agent-codex apps/hooks/codex tools/hooks/install-codex-v2.ps1
git commit -m "feat(codex): 增加 Codex 适配器与兼容 Hook"
```

---

### Task 3: 增加 Antigravity Agent 适配器

**Files:**
- Create: `crates/agentnotify-agent-antigravity/Cargo.toml`
- Create: `crates/agentnotify-agent-antigravity/src/lib.rs`
- Create: `crates/agentnotify-agent-antigravity/src/transcript.rs`
- Create: `crates/agentnotify-agent-antigravity/src/title.rs`
- Create: `apps/hooks/antigravity/Cargo.toml`
- Create: `apps/hooks/antigravity/src/main.rs`
- Create: `tools/hooks/install-antigravity-v2.ps1`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-agent-antigravity/tests/event.rs`
- Test: `crates/agentnotify-agent-antigravity/tests/reply.rs`

**Interfaces:**
- Consumes: Antigravity Stop JSON、transcript JSONL、annotations、运行中的 `language_server.exe agentapi`。
- Produces: `AntigravityAgent` 与 Antigravity Hook。

- [x] **Step 1: 写 fullyIdle 与精确会话测试**

```rust
#[test]
fn non_idle_stop_is_ignored_without_notification() {
    let adapter = AntigravityAgent::new_test();
    assert!(matches!(
        adapter.parse_event(antigravity_envelope(false, "conversation-1")),
        Err(AgentError::Ignored { .. })
    ));
}

#[test]
fn conversation_id_is_the_only_resume_target() {
    assert_eq!(antigravity_resume_target("conversation-1").as_str(), "conversation-1");
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-antigravity
```

Expected: FAIL，Antigravity 包不存在。

- [x] **Step 3: 实现事件和标题解析**

- 只处理 `fullyIdle=true` 且 conversationId 非空的事件。
- transcript 只读取尾部有界字节；无法解析时摘要为空，不阻塞 Stop。
- 标题优先 annotations `<conversationId>.pbtxt`，失败后使用 transcript 首条用户请求，再退回默认标题。
- Hook 始终输出 Antigravity 要求的 `{}`。

- [x] **Step 4: 实现精确回复**

- 发现当前运行的 language server：安装目录、PID 和命令行中的 CSRF token。
- 只连接本机 HTTP 端点。
- 先调用 `get-conversation-metadata` 验证目标会话，再调用 `send-message <conversationId> <text>`。
- token 和端口不落盘、不进入错误消息。
- 找不到唯一进程或会话返回 `AgentError::Unavailable`，不启动新语言服务，不用最近会话。

- [x] **Step 5: 安装 Hook**

安装器维护现有 Antigravity `agent-notify` 顶层键，同目录 launcher 调用 `agentnotify-antigravity-hook.exe`。不修改其他 Hook、权限或事件。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-antigravity
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-agent-antigravity apps/hooks/antigravity tools/hooks/install-antigravity-v2.ps1
git commit -m "feat(antigravity): 增加 Agent 适配器与原会话回复"
```

---

### Task 4: 增加 Devin Agent 适配器与桌面扩展

**Files:**
- Create: `crates/agentnotify-agent-devin/Cargo.toml`
- Create: `crates/agentnotify-agent-devin/src/lib.rs`
- Create: `crates/agentnotify-agent-devin/src/session.rs`
- Create: `crates/agentnotify-agent-devin/src/reply_inbox.rs`
- Create: `apps/hooks/devin/Cargo.toml`
- Create: `apps/hooks/devin/src/main.rs`
- Create: `plugin/devin-extension-v2/package.json`
- Create: `plugin/devin-extension-v2/extension.js`
- Create: `plugin/devin-extension-v2/acp-bridge.js`
- Create: `tools/hooks/install-devin-v2.ps1`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-agent-devin/tests/event.rs`
- Test: `crates/agentnotify-agent-devin/tests/reply.rs`

**Interfaces:**
- Consumes: Devin Stop JSON、桌面状态库 `state.vscdb`、Devin 扩展 ACP。
- Produces: `DevinAgent`、Devin Hook 和 V2 桌面扩展。

- [x] **Step 1: 写 stop_hook_active 和 ACP 目标测试**

```rust
#[test]
fn recursive_stop_is_ignored() {
    let adapter = DevinAgent::new_test();
    assert!(matches!(
        adapter.parse_event(devin_envelope(true, "session-1")),
        Err(AgentError::Ignored { .. })
    ));
}

#[test]
fn missing_cascade_mapping_fails_instead_of_using_latest_session() {
    let error = resolve_cascade("missing-session").unwrap_err();
    assert_eq!(error.code(), "devin_session_not_found");
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-devin
node --test .\plugin\devin-extension-v2\*.test.cjs
```

Expected: FAIL，Devin 适配器不存在。

- [x] **Step 3: 实现事件和会话解析**

- `stop_hook_active=true` 跳过。
- session ID 只取稳定 `session_id`。
- 正文取 `last_assistant_message`。
- 回复时通过桌面状态库解析 `acp/devin-cli/<session_id>`；找不到返回明确错误。
- 不读取 Devin CLI 登录状态，不使用最近 Cascade。

- [x] **Step 4: 实现 V2 扩展收件箱**

扩展保留：

- 原子 pending/processing/results 队列。
- heartbeat 和 instance owner。
- `processing` 中断只返回 Unknown，不自动重放。
- ACP `session/prompt` NDJSON 精确投递。
- 旧 Cascade 模式保留独立路径，但同样只接受显式 Cascade ID。

扩展不得访问 SQLite 或 Credential Manager；所有任务来自适配器提供的安全队列。

- [x] **Step 5: 安装 Hook 与扩展**

Devin Hook 只向已有 `hooks.Stop` 追加 AgentNotify 组。扩展安装目录使用 V2 包名，升级时确认 publisher/name 后替换，不递归删除其他扩展文件。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-devin
node --test .\plugin\devin-extension-v2\*.test.cjs
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-agent-devin apps/hooks/devin plugin/devin-extension-v2 tools/hooks/install-devin-v2.ps1
git commit -m "feat(devin): 增加桌面端适配器与 ACP 回复"
```

---

### Task 5: 增加 Command Code Agent 适配器

**Files:**
- Create: `crates/agentnotify-agent-commandcode/Cargo.toml`
- Create: `crates/agentnotify-agent-commandcode/src/lib.rs`
- Create: `crates/agentnotify-agent-commandcode/src/reply_inbox.rs`
- Create: `plugin/commandcode-v2/agent-notify.ts`
- Create: `tools/hooks/install-commandcode-v2.ps1`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-agent-commandcode/tests/event.rs`
- Test: `plugin/commandcode-v2/agent-notify.test.cjs`

**Interfaces:**
- Consumes: Command Code `run_end` 事件、reply window 设置。
- Produces: `CommandCodeAgent` 与 V2 mod。

- [ ] **Step 1: 写回复窗口边界测试**

```rust
#[test]
fn zero_reply_window_rejects_resume_without_blocking_run() {
    let adapter = CommandCodeAgent::new_test(0);
    let error = adapter.resume(&session("session-1"), "继续");
    assert_eq!(error.unwrap_err().code(), "commandcode_reply_window_closed");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-commandcode
node --test .\plugin\commandcode-v2\agent-notify.test.cjs
```

Expected: FAIL，Command Code 适配器不存在。

- [ ] **Step 3: 实现事件和回复窗口**

- `run_end` 生成标准事件，标题优先会话标题，再 transcript 首条用户消息，再默认标题。
- `reply_window_sec` 默认 0。
- 0 时回复明确失败“回复窗口未开启”，不能挂住 run。
- 1–600 时 mod 只等待本地 reply job，不消耗 token；等待期间用户输入行为必须在 UI/文档明确提示。
- 窗口结束后引用回复返回明确错误，不落到下一次 run。

- [ ] **Step 4: 实现 V2 mod**

- 零顶层 import，单文件。
- 调用 `agentnotify-ingress.exe` 提交事件。
- 写 heartbeat。
- reply job 只由持有该 run 的实例认领。
- pending → processing 使用原子 rename。
- 任何故障不得影响 Command Code。

- [ ] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-commandcode
node --test .\plugin\commandcode-v2\agent-notify.test.cjs
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-agent-commandcode plugin/commandcode-v2 tools/hooks/install-commandcode-v2.ps1
git commit -m "feat(commandcode): 增加 Agent 适配器与回复窗口"
```

---

### Task 6: 增加多账号通知选择策略

**Files:**
- Create: `crates/agentnotify-application/src/routing_policy.rs`
- Create: `crates/agentnotify-storage-sqlite/migrations/0002_channel_routing_rules.sql`
- Create: `crates/agentnotify-storage-sqlite/src/routing_rule_store.rs`
- Modify: `crates/agentnotify-application/src/delivery.rs`
- Modify: `apps/desktop-ui/src/features/channels/ChannelAccountDetail.tsx`
- Test: `crates/agentnotify-application/tests/routing_policy.rs`
- Test: `crates/agentnotify-storage-sqlite/tests/routing_rules.rs`

**Interfaces:**
- Consumes: `AgentId`、`ChannelId`、`ChannelAccountId`、settings。
- Produces: `RoutingRule`、`RoutingPolicy::select(notification) -> Result<Vec<DeliveryTarget>, RoutingError>`。

- [ ] **Step 1: 写无显式规则拒绝测试**

```rust
#[test]
fn two_accounts_without_rule_do_not_fall_back_to_latest() {
    let policy = RoutingPolicy::new(vec![rule("clawbot", "account-a")]);
    let targets = policy.select(&fixture_notification("opencode")).unwrap();
    assert_eq!(targets, vec![target("clawbot", "account-a")]);

    let error = RoutingPolicy::new(Vec::new())
        .select(&fixture_notification("opencode"))
        .unwrap_err();
    assert_eq!(error.code(), "routing_rule_missing");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-application --test routing_policy
cargo test -p agentnotify-storage-sqlite --test routing_rules
```

Expected: FAIL，路由策略不存在。

- [ ] **Step 3: 定义显式规则**

首版支持：

- 全局默认目标列表。
- 按 `AgentId` 覆盖目标。
- 按 `AgentId + ChannelId` 覆盖账号。
- 每个目标显式保存 `ChannelAccountId`。
- 禁用账号可以被规则引用，但发送时记 `Skipped/account_disabled`。
- 多条规则冲突时使用更具体的 `AgentId + ChannelId`，仍冲突则拒绝。

不实现“根据正文关键词”“最近使用账号”或“账号在线优先”这类隐式回退。

- [ ] **Step 4: 迁移与 UI**

`0002_channel_routing_rules.sql` 创建规则表，不修改 0001。Channels 页面允许为账号设置“用于哪些 Agent”，Agents 页面只显示 descriptor，不新增 Agent 专属逻辑。

- [ ] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-application --test routing_policy
cargo test -p agentnotify-storage-sqlite --test routing_rules
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.lock crates/agentnotify-application crates/agentnotify-storage-sqlite apps/desktop-ui
git commit -m "feat(routing): 增加显式多账号通知策略"
```

---

### Task 7: 增加飞书账号、认证与出站发送

**Files:**
- Create: `crates/agentnotify-channel-feishu/Cargo.toml`
- Create: `crates/agentnotify-channel-feishu/src/lib.rs`
- Create: `crates/agentnotify-channel-feishu/src/account.rs`
- Create: `crates/agentnotify-channel-feishu/src/auth.rs`
- Create: `crates/agentnotify-channel-feishu/src/send.rs`
- Create: `crates/agentnotify-channel-feishu/src/render.rs`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-channel-feishu/tests/auth.rs`
- Test: `crates/agentnotify-channel-feishu/tests/send.rs`

**Interfaces:**
- Consumes: 应用凭证、tenant access token、飞书消息 API。
- Produces: `FeishuChannel` 账号模型、token 生命周期和文本发送。

- [ ] **Step 1: 写 tenant token 与发送回执测试**

```rust
#[tokio::test]
async fn tenant_token_is_cached_per_tenant_and_refreshed_before_expiry() {
    let fixture = auth_fixture();
    let first = fixture.auth.token().await.unwrap();
    let second = fixture.auth.token().await.unwrap();
    assert_eq!(first, second);
    assert_eq!(fixture.http.token_calls(), 1);
    fixture.clock.advance_minutes(110);
    fixture.auth.token().await.unwrap();
    assert_eq!(fixture.http.token_calls(), 2);
}

#[test]
fn successful_send_requires_stable_message_id() {
    let receipt = parse_send_receipt(fixture_response("om_1")).unwrap();
    assert_eq!(receipt.external_message_id.unwrap().as_str(), "om_1");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-feishu
```

Expected: FAIL，飞书 crate 不存在。

- [ ] **Step 3: 实现账号与认证**

- `app_id` 是非密钥配置；`app_secret` 只写 Credential Manager。
- account ID 由 `tenant_key + app_id` 的 SHA-256 前缀生成，不保存明文 tenant key 全文。
- tenant token 按账号和 tenant 缓存，只存内存。
- 过期前 5 分钟刷新；刷新失败时返回 Retryable 或清楚的可重试错误。
- 多租户账号之间不共享 token、游标或限流器。

- [ ] **Step 4: 实现出站能力**

Capabilities：

```rust
ChannelCapabilities {
    send_text: true,
    receive: true,
    reply_routing: true,
    edit_message: true,
    attachments: false,
    markdown: true,
    max_text_bytes: Some(30 * 1024),
    inbound_modes: vec![InboundMode::WebSocket],
}
```

- 私聊和群聊都显式保存 `chat_id`。
- 发送返回稳定 message ID 才允许建立 Route。
- 富文本或普通文本能力差异由 descriptor 和 render 决定，不进入通知核心。
- 限流按账号和 tenant 独立，遵守 Retry-After。
- 不可重试参数错误返回 Permanent；未知结果返回 Unknown。

- [ ] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-channel-feishu
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-channel-feishu
git commit -m "feat(feishu): 增加账号认证与消息发送"
```

---

### Task 8: 增加飞书长连接、事件去重与精确回复

**Files:**
- Create: `crates/agentnotify-channel-feishu/src/event.rs`
- Create: `crates/agentnotify-channel-feishu/src/websocket.rs`
- Create: `crates/agentnotify-channel-feishu/src/reply.rs`
- Modify: `crates/agentnotify-channel-feishu/src/lib.rs`
- Test: `crates/agentnotify-channel-feishu/tests/event.rs`
- Test: `crates/agentnotify-channel-feishu/tests/websocket.rs`
- Test: `crates/agentnotify-channel-feishu/tests/reply.rs`

**Interfaces:**
- Consumes: 飞书 WebSocket 事件、事件 event ID、回复消息结构和 ReplyService。
- Produces: 飞书入站、ack、去重、引用回复和账号健康状态。

- [ ] **Step 1: 写事件重复与引用归一化测试**

```rust
#[test]
fn duplicate_event_id_is_not_emitted_twice() {
    let mut normalizer = EventNormalizer::default();
    assert!(normalizer.normalize(fixture_event("event-1")).unwrap().is_some());
    assert!(normalizer.normalize(fixture_event("event-1")).unwrap().is_none());
}

#[test]
fn missing_reply_parent_message_id_does_not_fall_back_to_chat() {
    let message = normalize_reply_without_parent().unwrap();
    assert!(message.referenced_message_ids.is_empty());
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-feishu --test event --test websocket --test reply
```

Expected: FAIL，长连接和入站未实现。

- [ ] **Step 3: 实现长连接生命周期**

- 使用飞书支持的 WebSocket 长连接客户端，不在桌面端暴露公网回调。
- 启动前获取 tenant token 和连接端点。
- 连接断开使用有上限退避；认证失效时暂停并显示重新配置。
- 处理 heartbeat、ack、重连和连接世代。
- shutdown 发送 active=false 并等待有界时间。
- 连接状态按账号隔离。

- [ ] **Step 4: 实现事件归一化**

- 私聊、群聊、引用消息分别归一化。
- 事件 ID 必填；缺失时使用账号、chat、message、sender 和文本哈希生成确定性回退。
- 引用父消息 ID 只从稳定 message ID 读取；没有时拒绝回复路由。
- 发送者校验必须约束绑定用户或允许的群成员策略。
- ack 只在事件持久化 Claim 成功后发送。
- 重复 event ID 不重复 emit。

- [ ] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-channel-feishu --test event --test websocket --test reply
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add crates/agentnotify-channel-feishu
git commit -m "feat(feishu): 增加长连接事件与精确回复"
```

---

### Task 9: 建立外部 Agent 适配器进程协议

**Files:**
- Create: `crates/agentnotify-agent-sdk/src/external/manifest.rs`
- Create: `crates/agentnotify-agent-sdk/src/external/protocol.rs`
- Create: `crates/agentnotify-agent-sdk/src/external/process.rs`
- Create: `crates/agentnotify-agent-sdk/src/external/registry.rs`
- Create: `apps/agent-adapter-host/Cargo.toml`
- Create: `apps/agent-adapter-host/src/main.rs`
- Create: `docs/superpowers/specs/2026-09-19-external-agent-protocol.md`
- Test: `crates/agentnotify-agent-sdk/tests/external_protocol.rs`
- Test: `apps/agent-adapter-host/tests/stdio.rs`

**Interfaces:**
- Consumes: 版本化 adapter manifest、JSON-RPC 2.0 stdin/stdout。
- Produces: `ExternalAgentManifest`、`ExternalAgentProcess`、`ExternalAgentRegistry`、stdio adapter host。

- [ ] **Step 1: 写协议握手与越权拒绝测试**

```rust
#[tokio::test]
async fn handshake_rejects_mismatched_protocol_version() {
    let process = ExternalAgentProcess::fixture_with_version(2);
    let error = process.handshake().await.unwrap_err();
    assert_eq!(error.code(), "external_adapter_protocol_mismatch");
}

#[test]
fn adapter_manifest_cannot_request_database_access() {
    let error = parse_manifest(fixture_manifest_with_database_permission()).unwrap_err();
    assert_eq!(error.code(), "external_adapter_permission_forbidden");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-sdk --test external_protocol
cargo test -p agentnotify-agent-adapter-host --test stdio
```

Expected: FAIL，外部适配器协议不存在。

- [ ] **Step 3: 定义 manifest**

示例：

```json
{
  "manifestVersion": 1,
  "protocolVersion": 1,
  "id": "example-agent",
  "displayName": "Example Agent",
  "version": "1.0.0",
  "entrypoint": "adapter.exe",
  "capabilities": {
    "notify": true,
    "resume": true,
    "sessionTitle": true
  },
  "permissions": ["process:spawn", "network:https"]
}
```

禁止权限：

- `database:direct`
- `secret:direct`
- `filesystem:arbitrary`
- `shell:arbitrary`
- `ui:direct`

manifest 必须通过 JSON Schema、版本、ID、路径和签名/来源校验。

- [ ] **Step 4: 定义 JSON-RPC 方法**

核心 → 适配器：

```text
initialize
parseEvent
resume
inspect
shutdown
```

适配器 → 核心：

```text
emitEvent
logSafe
```

- 消息最大 1 MiB。
- `initialize` 协商版本和能力。
- 所有请求有超时和 correlation ID。
- 不允许核心方法透传数据库、secret、任意路径或 shell。
- 适配器超时或崩溃返回 Unknown，不自动重放 resume。
- stdout 只允许 JSON-RPC；stderr 进入有界脱敏诊断日志。

- [ ] **Step 5: 实现外部适配器宿主**

`agentnotify-agent-adapter-host.exe` 根据 manifest 启动一个适配器进程并桥接到 Agent SDK。它不提供管理命令，不读取用户参数；runtime 通过内部 handle 启动和停止。

- [ ] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-sdk --test external_protocol
cargo test -p agentnotify-agent-adapter-host --test stdio
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-agent-sdk apps/agent-adapter-host docs/superpowers/specs/2026-09-19-external-agent-protocol.md
git commit -m "feat(agent-sdk): 增加外部适配器进程协议"
```

---

### Task 10: 接入外部适配器安装、权限展示与动态注册

**Files:**
- Create: `crates/agentnotify-runtime/src/external_adapters.rs`
- Create: `crates/agentnotify-storage-sqlite/migrations/0003_external_adapter_state.sql`
- Modify: `hosts/desktop-tauri/src/bridge/commands.rs`
- Create: `apps/desktop-ui/src/features/diagnostics/ExternalAdapters.tsx`
- Test: `crates/agentnotify-runtime/tests/external_adapters.rs`
- Test: `apps/desktop-ui/src/features/diagnostics/ExternalAdapters.test.tsx`

**Interfaces:**
- Consumes: manifest、外部宿主、`adapter_manifests`。
- Produces: 外部适配器注册、启停、能力展示、权限展示和健康状态。

- [ ] **Step 1: 写禁用适配器不启动测试**

```rust
#[tokio::test]
async fn disabled_external_adapter_is_not_spawned() {
    let fixture = external_runtime_fixture(false);
    fixture.runtime.start().await.unwrap();
    assert_eq!(fixture.process.spawn_count(), 0);
    assert_eq!(fixture.snapshot.external_adapters()[0].state, "Disabled");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-runtime --test external_adapters
npm --prefix .\apps\desktop-ui run test -- --run ExternalAdapters
```

Expected: FAIL，外部注册未接线。

- [ ] **Step 3: 实现注册状态**

`0003_external_adapter_state.sql` 增加启用状态、最后一次协议版本、来源策略和健康时间。迁移不修改前两版 SQL。

runtime 只启动：

- manifest 校验通过。
- 用户显式启用。
- 入口路径 canonicalize 后仍是允许目录内的普通文件。
- 文件 hash 与批准时记录一致。
- protocol capability 与 core 兼容。

- [ ] **Step 4: Diagnostics 增加外部适配器**

列表显示：

- 名称、ID、版本、来源。
- 协议版本。
- Agent capabilities。
- 请求的权限。
- 推送、回复、最后健康时间。
- 启用/禁用和“重新校验”。

UI 不显示任意 manifest 原文、文件路径或未脱敏 stderr。启用未知来源时显示权限确认，不提供“允许任意文件系统/数据库”选项。

- [ ] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-runtime --test external_adapters
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.lock crates/agentnotify-runtime crates/agentnotify-storage-sqlite hosts/desktop-tauri apps/desktop-ui
git commit -m "feat(adapters): 接入外部适配器注册与权限展示"
```

---

### Task 11: 完成扩展生态真实链路与发布门禁

**Files:**
- Create: `crates/agentnotify-testkit/tests/all_adapters_contracts.rs`
- Create: `tests/extension-smoke.ps1`
- Create: `docs/superpowers/specs/2026-09-19-extension-ecosystem-acceptance.md`
- Modify: `tools/lint.ps1`
- Modify: `tools/test.ps1`
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: 所有内置 Agent、飞书、外部适配器和扩展 UI。
- Produces: 每个适配器独立发布门禁与真实链路验收记录。

- [ ] **Step 1: 写全适配器契约测试**

```rust
#[tokio::test]
async fn every_registered_builtin_adapter_passes_its_contract() {
    let agents = builtin_agents();
    for adapter in agents {
        certify_agent(adapter).await.unwrap();
    }

    let channels = builtin_channels();
    for adapter in channels {
        certify_channel(adapter).await.unwrap();
    }
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-testkit --test all_adapters_contracts
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\extension-smoke.ps1
```

Expected: FAIL，全量适配器注册和 smoke 未完成。

- [ ] **Step 3: 增加扩展 smoke**

`extension-smoke.ps1` 使用隔离目录验证：

- 每个 Hook/插件只安装自己的配置项。
- 卸载只删除自己的 Hook、扩展和 launcher。
- 每个 Agent 的 ingress 事件在客户端退出或 hook 失败时不会阻塞上游。
- 每个渠道使用独立账号、游标和限额。
- 飞书重复 event ID 不重复投递。
- 外部适配器禁用后不启动，协议不匹配不注册。
- UI 在新增适配器后无需代码修改即可显示 descriptor、能力和状态。

- [ ] **Step 4: 增加逐适配器真实验收**

每个适配器单独记录：

- 真实客户端版本与测试时间。
- 正常推送。
- 精确回复。
- 账号或客户端退出。
- 超时、Unknown 和重复事件。
- 脱敏日志检查。
- 失败时的复现步骤。

不允许用“同类型适配器已经通过”代替当前适配器验收。

- [ ] **Step 5: 更新 CI 与发布门禁**

`tools/lint.ps1` 和 `tools/test.ps1` 增加：

- Rust workspace fmt、clippy、test。
- 前端 typecheck、Vitest、Playwright、视觉和 axe。
- 每个 Hook/插件测试。
- manifest schema 与权限测试。
- 安装、卸载和签名 smoke。
- 真实外部网络测试不进入默认 CI；通过受控、显式启用的人工验收任务执行。

- [ ] **Step 6: 提交**

```powershell
git add crates/agentnotify-testkit tests/extension-smoke.ps1 docs/superpowers/specs/2026-09-19-extension-ecosystem-acceptance.md tools/lint.ps1 tools/test.ps1 .github/workflows/release.yml
git commit -m "test(adapters): 增加扩展生态发布门禁"
```

---

## 阶段完成标准

- Codex、Antigravity、Devin、Command Code 均通过各自真实客户端和精确回复验收。
- 飞书通过多租户认证、长连接、事件去重、出站和引用回复验收。
- 外部 Agent 只能以版本化进程协议接入，不能访问数据库、密钥库或任意文件。
- 多账号通知选择有显式规则，不存在最近账号或默认账号回退。
- UI 只通过 descriptor、capabilities 和 schema 展示所有新适配器。
- 每个新增适配器只增加 crate、Hook/插件、注册项和测试，没有修改通知服务、回复服务或页面业务分支。
- 扩展版本发布后，OpenCode + ClawBot 的 Windows 基线仍持续通过。
- macOS 与 HarmonyOS PC 不在本计划内；拥有真实测试环境后另行评审宿主计划。
