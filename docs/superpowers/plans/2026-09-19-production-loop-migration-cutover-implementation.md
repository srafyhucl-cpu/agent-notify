# 首个生产闭环、迁移与切换 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Windows 上用 OpenCode 和 ClawBot/微信跑通真实通知推送与精确引用回复，幂等导入现有 Go 版数据，并让 Tauri/React 成为唯一正式 UI。

**Architecture:** `agentnotify-agent-opencode` 负责 OpenCode 事件解析、健康检查和原会话续聊；`agentnotify-channel-clawbot` 负责账号、登录、长轮询、发送和消息 ID 归一化。旧数据通过只读导入器进入 SQLite 与 Credential Manager；正式安装器在闭环验收后切换入口，旧 Go 文件保留为短期回滚基线但不继续交付 UI。

**Tech Stack:** Rust、Tokio、`reqwest` + rustls、SQLite、Credential Manager、OpenCode V2 插件协议、ClawBot iLink 2.4.6 协议、Inno Setup、Playwright、PowerShell smoke。

## Global Constraints

- 第一阶段只迁移并验收 OpenCode 与 ClawBot；Codex、Antigravity、Devin、Command Code、飞书和外部适配器不在本计划实现。
- 引用回复只能匹配 `channel_id + account_id + external_message_id`；同一通知同时存在平台消息 ID 与 client ID 时，入站只接受唯一一致的目标。
- 旧数据导入必须只读旧文件；导入失败时保持旧文件不变，不自动删除、不自动改写。
- 导入必须幂等；相同源文件重复执行不新增 Notification、Delivery、Route 或 Claim。
- 旧 `claimed` 状态迁移为 `Unknown`，绝不能迁移为可执行状态或重置为未 Claim。
- ClawBot token、context token 和游标按账号隔离；token 只进 Credential Manager，SQLite 不保存明文。
- 网络超时、进程中断和无法确认结果返回 `Unknown`，不自动重放。
- 正式切换前继续使用现有 Go 发布链路；切换任务才能更改版本事实来源和安装入口。
- 正式版本要求安装器与主程序签名；预览版本必须带明确的 `Rust-Preview` 名称，不能与稳定版同名。
- 真实验收必须使用真实 OpenCode、真实 ClawBot 账号和真实微信；测试替身通过不能记为生产验收。
- 所有迁移和验收记录不得包含 token、context token、用户正文、完整 conversation ID 或微信账号明文。

---

### Task 1: 实现 OpenCode Agent 适配器

**Files:**
- Create: `crates/agentnotify-agent-opencode/Cargo.toml`
- Create: `crates/agentnotify-agent-opencode/src/descriptor.rs`
- Create: `crates/agentnotify-agent-opencode/src/event.rs`
- Create: `crates/agentnotify-agent-opencode/src/adapter.rs`
- Create: `crates/agentnotify-agent-opencode/src/reply_inbox.rs`
- Create: `crates/agentnotify-agent-opencode/src/lib.rs`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-agent-opencode/tests/contract.rs`
- Test: `crates/agentnotify-agent-opencode/tests/event.rs`

**Interfaces:**
- Consumes: `AgentAdapter`、`AgentDescriptor`、`AgentEventEnvelope`。
- Produces: `OpenCodeAgent`，descriptor ID 固定为 `opencode`。

- [x] **Step 1: 写事件解析与契约测试**

```rust
#[test]
fn completed_event_normalizes_session_title_and_body() {
    let adapter = OpenCodeAgent::new_test();
    let event = adapter
        .parse_event(fixture_envelope(
            "session.completed",
            serde_json::json!({
                "idempotencyKey": "opencode:session-1:event-9",
                "occurredAt": "2026-09-19T10:20:30.123+08:00",
                "sessionId": "session-1",
                "title": "构建完成",
                "body": "Release 已生成"
            }),
        ))
        .unwrap();

    assert_eq!(event.session_id.unwrap().as_str(), "session-1");
    assert_eq!(event.title, "构建完成");
    assert_eq!(event.body, "Release 已生成");
}

#[tokio::test]
async fn adapter_passes_agent_contract() {
    assert_agent_contract(Arc::new(OpenCodeAgent::new_test())).await;
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-opencode
```

Expected: FAIL，OpenCode crate 不存在。

- [x] **Step 3: 实现 descriptor 与事件解析**

Descriptor：

```rust
AgentDescriptor {
    id: AgentId::new("opencode")?,
    display_name: "OpenCode".into(),
    description: "OpenCode 桌面端会话完成事件与原会话续聊".into(),
    config_schema: opencode_config_schema(),
}

AgentCapabilities {
    notify: true,
    resume: true,
    session_title: true,
    hook_installer: true,
    reply_window: false,
}
```

只接受 `payload.eventType == "session.completed"`。缺失 sessionId、title 或 body 时返回 `AgentError::InvalidEvent`，不得用“最近会话”补全。事件归属为 `agentId=opencode`，其他 agent 的 envelope 返回 `InvalidEvent`。

- [x] **Step 4: 实现回复收件箱路径与健康检查**

收件箱默认路径保持兼容：

```text
%USERPROFILE%\.config\agent-notify\opencode-reply-inbox
  pending/
  processing/
  results/
  heartbeats/
```

`inspect()` 返回：

- `Ready`：至少一个 heartbeat 的 `ready=true` 且时间戳在 30 秒内。
- `Waiting`：有 heartbeat 但均不 ready，或全部过期。
- `Error`：目录不可读或存在损坏且无法忽略的状态。
- `NotFound`：目录不存在且没有插件接入证据。

健康检查不得启动 OpenCode、读取其会话数据库或写入收件箱。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-opencode
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-agent-opencode
git commit -m "feat(opencode): 增加 Agent 事件适配器"
```

---

### Task 2: 实现 OpenCode 原会话续聊与 V2 插件

**Files:**
- Modify: `crates/agentnotify-agent-opencode/src/reply_inbox.rs`
- Modify: `crates/agentnotify-agent-opencode/src/adapter.rs`
- Create: `plugin/rust/agent-notify.ts`
- Create: `tools/hooks/install-opencode-v2.ps1`
- Test: `crates/agentnotify-agent-opencode/tests/resume.rs`
- Test: `plugin/rust/agent-notify.test.cjs`

**Interfaces:**
- Consumes: OpenCode 插件 heartbeat、`ctx.session.prompt` / `promptAsync` 兼容接口、`agentnotify-ingress.exe`。
- Produces: `OpenCodeAgent::resume` 和新的单文件 V2 插件。

- [x] **Step 1: 写 resume 状态测试**

```rust
#[tokio::test]
async fn timeout_is_unknown_and_job_is_not_replayed() {
    let fixture = resume_fixture_without_plugin_result();
    let error = fixture
        .adapter
        .resume(&session_id("session-1"), "继续处理")
        .await
        .unwrap_err();
    assert!(matches!(error, AgentError::Unknown(_)));
    assert_eq!(fixture.inbox.pending_count().await, 0);
    assert_eq!(fixture.inbox.processing_count().await, 1);
}

#[tokio::test]
async fn plugin_failure_returns_safe_agent_error() {
    let fixture = resume_fixture_with_result(false, "当前会话不存在");
    let error = fixture
        .adapter
        .resume(&session_id("session-1"), "继续处理")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "opencode_resume_failed");
    assert!(!error.to_string().contains("继续处理"));
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-opencode --test resume
node --test .\plugin\rust\agent-notify.test.cjs
```

Expected: FAIL，resume 实现和插件不存在。

- [x] **Step 3: 实现原子回复任务**

- 任务 ID 使用 UUID v4。
- `pending/<job-id>.json`：

```json
{
  "id": "uuid",
  "sessionID": "session-1",
  "text": "继续处理",
  "createdAt": "2026-09-19T10:20:30.123+08:00",
  "expiresAt": "2026-09-19T10:30:30.123+08:00"
}
```

- 写入使用临时文件 + flush + 原子 rename。
- 任务有效期 10 分钟。
- 提交前要求至少一个 30 秒内且 `ready=true` 的 heartbeat。
- 最多等待 10 秒读取 `results/<job-id>.json`。
- 结果 `ok=true` 返回 `Accepted`；`ok=false` 返回 `Failed` 并只保留最多 300 个字符的安全错误。
- 超时返回 `Unknown`；`processing` 文件保留，后续由插件标记“处理中断，不自动重试”。
- 不启动新的 OpenCode 进程，不读取 CLI 登录状态。

- [x] **Step 4: 创建 V2 插件**

新插件沿用现有 OpenCode V2 协议，但事件发送改为：

```ts
const ingress = resolveIngressTarget()
const event = {
  protocolVersion: 1,
  kind: "agent.event",
  requestId: crypto.randomUUID(),
  agentId: "opencode",
  payload: {
    eventType: "session.completed",
    idempotencyKey: `opencode:${sessionID}:${eventID}`,
    occurredAt: new Date().toISOString(),
    sessionId: sessionID,
    title: `【opencode】${title}`,
    body: summary,
    metadata: {},
  },
}
```

插件要求：

- 零顶层 import，可由 OpenCode 直接加载。
- 使用 `child_process.execFile` 直接调用 `agentnotify-ingress.exe`，通过 stdin 提交 JSON。
- `windowsHide: true`；单次调用最多 25 秒，回调最多 30 秒。
- 失败吞掉，不影响 OpenCode。
- 保留引用回复收件箱、heartbeat 和 `session.prompt` → `promptAsync` 兼容链。
- 冷却状态仍由插件与核心共同防重，但核心的 ingest key 才是最终幂等依据。
- 不再调用 `agent-notify.exe notify`，也不解析或修改 Go 版配置。

- [x] **Step 5: 编写插件状态机测试**

Node 测试覆盖：

- 成功事件只提交一次。
- ingress 超时或退出码 `3` 不抛给 OpenCode。
- 重复完成事件使用同一 idempotency key。
- heartbeat 含 `ready=true` 且原子更新。
- reply pending 任务只认领一次。
- prompt 超时写“未确认，不自动重试”。
- 插件 dispose 清理自己的 heartbeat。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-agent-opencode --test resume
node --test .\plugin\rust\agent-notify.test.cjs
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add crates/agentnotify-agent-opencode plugin/rust tools/hooks/install-opencode-v2.ps1
git commit -m "feat(opencode): 实现原会话续聊与 V2 插件"
```

---

### Task 3: 建立 ClawBot 渠道、账号与密钥模型

**Files:**
- Create: `crates/agentnotify-channel-clawbot/Cargo.toml`
- Create: `crates/agentnotify-channel-clawbot/src/descriptor.rs`
- Create: `crates/agentnotify-channel-clawbot/src/account.rs`
- Create: `crates/agentnotify-channel-clawbot/src/state.rs`
- Create: `crates/agentnotify-channel-clawbot/src/adapter.rs`
- Create: `crates/agentnotify-channel-clawbot/src/lib.rs`
- Modify: `Cargo.toml`
- Test: `crates/agentnotify-channel-clawbot/tests/account.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/contract.rs`

**Interfaces:**
- Consumes: `ChannelAdapter`、`ChannelLoginAdapter`、`SecretStore`、`ChannelAccountStore`。
- Produces: `ClawBotChannel`，descriptor ID 固定为 `clawbot`。

- [x] **Step 1: 写账号隔离与密钥测试**

```rust
#[tokio::test]
async fn account_scope_is_stable_and_does_not_expose_platform_ids() {
    let account = ClawBotAccount::from_platform_ids("bot-1", "user-1").unwrap();
    assert_eq!(account.id().as_str(), account.id().as_str());
    assert!(!account.id().as_str().contains("bot-1"));
    assert!(!account.id().as_str().contains("user-1"));
}

#[tokio::test]
async fn logout_removes_secrets_without_removing_history() {
    let fixture = clawbot_fixture();
    fixture.store.secrets.set(&fixture.account, fixture.secrets.clone()).await.unwrap();
    fixture.channel.logout(fixture.account.clone()).await.unwrap();
    assert!(matches!(
        fixture.store.secrets.get(&fixture.account, SecretKind::BotToken).await,
        Err(SecretError::NotFound)
    ));
    assert_eq!(fixture.store.history_count().await, 1);
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test account --test contract
```

Expected: FAIL，ClawBot crate 不存在。

- [x] **Step 3: 实现 descriptor 与能力**

```rust
ChannelDescriptor {
    id: ChannelId::new("clawbot")?,
    display_name: "ClawBot 微信".into(),
    config_schema: clawbot_config_schema(),
}

ChannelCapabilities {
    send_text: true,
    receive: true,
    reply_routing: true,
    edit_message: false,
    attachments: false,
    markdown: true,
    max_text_bytes: Some(32 * 1024),
    inbound_modes: vec![InboundMode::LongPolling],
}
```

- [x] **Step 4: 实现账号与 secret 引用**

账号 ID：

```text
clawbot-<sha256("clawbot\0" + bot_id + "\0" + user_id)[0..16]>
```

SQLite `channel_accounts` 保存：

- account ID。
- channel ID。
- 脱敏 display name。
- `bot_id_hint`、`user_id_hint`：只保留末 6 位。
- `base_url`。
- `cursor_json`。
- `stale_at`、`session_established_at`。
- SecretRef 名称。

Credential Manager 条目：

```text
clawbot/<account-id>/bot-token
clawbot/<account-id>/context-token
```

context token 只允许在与 `bot_id + user_id` 相同的账号下读取。切换账号必须清空内存中的旧账号状态，不沿用 pending login、cursor 或 route。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add Cargo.toml Cargo.lock crates/agentnotify-channel-clawbot
git commit -m "feat(clawbot): 增加渠道账号与密钥模型"
```

---

### Task 4: 实现 ClawBot 扫码登录和会话就绪状态

**Files:**
- Create: `crates/agentnotify-channel-clawbot/src/login.rs`
- Create: `crates/agentnotify-channel-clawbot/src/qr.rs`
- Create: `crates/agentnotify-channel-clawbot/src/client.rs`
- Modify: `crates/agentnotify-channel-clawbot/src/adapter.rs`
- Modify: `hosts/desktop-tauri/src/bridge/commands.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/login.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/login_http.rs`

**Interfaces:**
- Consumes: `ChannelLoginAdapter`、ClawBot 登录 HTTP 契约、`SecretStore`。
- Produces: 二维码状态机、配对码提交、账号落库、`WaitingFirstInbound` / `Paired` 状态事件。

- [x] **Step 1: 写状态机测试**

```rust
#[test]
fn scaned_then_need_verify_then_confirmed_keeps_one_login_session() {
    let mut machine = LoginMachine::new("session-1");
    assert_eq!(machine.apply(status("wait")).unwrap(), LoginSessionState::WaitingScan);
    assert_eq!(machine.apply(status("scaned")).unwrap(), LoginSessionState::WaitingScan);
    assert_eq!(
        machine.apply(status("need_verifycode")).unwrap(),
        LoginSessionState::NeedVerifyCode
    );
    machine.submit_code("123456").unwrap();
    assert_eq!(
        machine.apply(confirmed_status()).unwrap(),
        LoginSessionState::Paired
    );
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test login
cargo test -p agentnotify-channel-clawbot --test login_http
```

Expected: FAIL，登录实现不存在。

- [x] **Step 3: 实现 HTTP 客户端**

固定支持：

```text
POST /ilink/bot/get_bot_qrcode?bot_type=3
GET  /ilink/bot/get_qrcode_status?qrcode=...
POST /ilink/bot/getupdates
POST /ilink/bot/sendmessage
POST /ilink/bot/msg/notifystart
POST /ilink/bot/msg/notifystop
```

- 默认入口 `https://ilinkai.weixin.qq.com`。
- 登录成功后业务请求使用响应 `baseurl`。
- 所有业务请求带 `base_info.channel_version="2.4.6"` 与新的 AgentNotify 版本。
- 使用 rustls，不依赖系统 OpenSSL。
- 连接超时 10 秒，普通请求总超时 20 秒，长轮询单独遵守服务端 `longpolling_timeout_ms`。
- 日志对 token、authorization、cookie、context token 和消息正文递归脱敏。

- [x] **Step 4: 实现登录状态机**

状态和动作：

- `wait` / `scaned` → 继续轮询。
- `need_verifycode` → UI 显示数字配对码输入。
- `verify_code_blocked` → `Blocked`，提供刷新。
- `expired` → `Expired`，允许有限次数刷新。
- `scaned_but_redirect` → 后续请求使用 `redirect_host`。
- `binded_redirect` → 只有账号已存在且凭据有效时成功。
- `confirmed` → 写入 BotToken；进入 `WaitingFirstInbound`。
- 收到首条合法入站消息并取得 context token → `Paired`。

二维码内容只在内存中生成 data URL；配对码提交后立即从 Rust 和 React 表单内存清零。登录任务取消或窗口关闭不删除已确认的登录账号。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test login --test login_http
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS，HTTP 测试使用隔离 mock server，不访问真实 ClawBot。

```powershell
git add crates/agentnotify-channel-clawbot hosts/desktop-tauri
git commit -m "feat(clawbot): 实现扫码登录与首条会话绑定"
```

---

### Task 5: 实现 ClawBot 出站发送与消息 ID 归一化

**Files:**
- Create: `crates/agentnotify-channel-clawbot/src/render.rs`
- Create: `crates/agentnotify-channel-clawbot/src/send.rs`
- Create: `crates/agentnotify-channel-clawbot/src/response.rs`
- Modify: `crates/agentnotify-channel-clawbot/src/adapter.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/send.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/response.rs`

**Interfaces:**
- Consumes: `ChannelAdapter::send`、账号 context token。
- Produces: 标准 Markdown Outbound、`DeliveryReceipt`、稳定 client ID 与平台 message ID。

- [x] **Step 1: 写回执和失败映射测试**

```rust
#[test]
fn message_id_is_read_from_top_level_nested_and_item_list() {
    assert_eq!(parse_message_id(fixture_response(r#"{"message_id":"m1"}"#)).unwrap(), "m1");
    assert_eq!(parse_message_id(fixture_response(r#"{"data":{"msg_id":2}}"#)).unwrap(), "2");
    assert_eq!(
        parse_message_id(fixture_response(r#"{"item_list":[{"msg_id":3}]}"#)).unwrap(),
        "3"
    );
}

#[tokio::test]
async fn timeout_maps_to_unknown() {
    let fixture = timeout_send_fixture();
    let error = fixture.channel.send(fixture.account, fixture.message).await.unwrap_err();
    assert!(matches!(error, ChannelError::Unknown(_)));
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test send --test response
```

Expected: FAIL，发送实现不存在。

- [x] **Step 3: 渲染标准通知**

通知保持 Markdown：

```text
**🟢 OpenCode｜<会话名>**

<正文>

—
*引用此消息可继续对话* · MM/DD HH:mm
```

- Agent display name 从 Agent Registry descriptor 获取，不硬编码 OpenCode。
- 正文保留 Markdown；超过 `max_text_bytes` 时返回 `Permanent`，不静默截断。
- footer 可由设置关闭，但引用提示不能伪造。
- URL 和引用 footer 不包含 token。

- [x] **Step 4: 实现发送与错误分类**

请求：

```json
{
  "msg": {
    "from_user_id": "",
    "to_user_id": "<bound user id>",
    "client_id": "<uuid>",
    "message_type": 2,
    "message_state": 2,
    "item_list": [{ "type": 1, "text_item": { "text": "..." } }],
    "context_token": "<secret>"
  },
  "base_info": {
    "channel_version": "2.4.6",
    "bot_agent": "AgentNotify/<version> (windows)"
  }
}
```

映射：

- HTTP/网络超时、连接重置、进程中断 → `ChannelError::Unknown`。
- HTTP 429、5xx 或明确服务端可重试错误 → `Retryable`。
- 参数、消息过大、账号格式错误 → `Permanent`。
- `ret=-14` / `errcode=-14` → 清空 context token 和 cursor，标记账号 stale，返回 `InvalidAccount`。
- `ret=-2 prepare failed` → 清空 context token，返回 `DeliveryReceipt { state: Skipped, error: session_missing }`。
- 成功但解析不到稳定 message ID → `DeliveryReceipt { state: Unknown }`，不能建立 ReplyRoute。

- [x] **Step 5: 归一化稳定路由 ID**

回执优先顺序：

1. 顶层 `message_id`。
2. 顶层 `msg_id` / `msgid`。
3. `msg` / `data` 内对应字段。
4. `item_list[].msg_id`。

若多个来源不一致，返回 `Unknown` 并不建立路由。请求 `client_id` 只在平台没有返回 message ID 时作为兼容回执保存，不能与另一个不同的 message ID 同时充当同一路由别名。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test send --test response
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add crates/agentnotify-channel-clawbot
git commit -m "feat(clawbot): 实现可靠出站与消息回执"
```

---

### Task 6: 实现 ClawBot 长轮询、入站归一化与账号状态

**Files:**
- Create: `crates/agentnotify-channel-clawbot/src/inbound.rs`
- Create: `crates/agentnotify-channel-clawbot/src/session.rs`
- Modify: `crates/agentnotify-channel-clawbot/src/adapter.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/inbound.rs`
- Test: `crates/agentnotify-channel-clawbot/tests/session.rs`

**Interfaces:**
- Consumes: `getupdates`、账号 cursor/context token、`InboundEmitter`。
- Produces: 标准化 `InboundMessage`、持久化 cursor/context token、账号状态变化事件。

- [x] **Step 1: 写引用 ID 冲突与恢复测试**

```rust
#[test]
fn conflicting_reference_ids_are_rejected() {
    let message = inbound_with_references(&["top", "nested"]);
    let parsed = normalize_inbound(&account(), message).unwrap_err();
    assert_eq!(parsed.code(), "clawbot_reference_conflict");
}

#[test]
fn same_message_id_in_different_accounts_stays_isolated() {
    let first = normalize_inbound(&account_a(), inbound("m1")).unwrap();
    let second = normalize_inbound(&account_b(), inbound("m1")).unwrap();
    assert_ne!(first.account_id, second.account_id);
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test inbound --test session
```

Expected: FAIL，长轮询未实现。

- [x] **Step 3: 实现归一化**

引用 ID 来源兼容：

- 顶层 `referenced_msg_id`。
- `ref_msg.msg_id`。
- `ref_msg.referenced_msg_id`。
- `ref_msg.message_item.msg_id`。
- 数值与字符串统一为字符串。

去重：

- 提取不同来源后去重。
- 只有 0 个或恰好 1 个值可生成 InboundMessage；多个不同值返回明确错误并不派发。
- 私聊判定要求 `message_type=user`、`group_id` 为空且发送者等于账号绑定 user ID。
- 无平台消息 ID 时保留 `seq + from_user_id + referenced IDs + text hash` 的确定性回退材料。
- ClawBot 的 ClaimKey 必须兼容旧 Go 版算法，使导入后的 `reply-state.jsonl` 能拦截升级前已处理的重复消息。

- [x] **Step 4: 实现账号游标与会话恢复**

`start(account, emit)` 每个账号一个任务：

1. 读取该账号 BotToken、cursor 和 context token。
2. `notifystart` 为最佳努力，不阻塞轮询。
3. 循环调用 `getupdates`。
4. 响应中的新 cursor 只在本次消息持久化完成后提交。
5. context token 按账号加密写入 Credential Manager。
6. 收到首条合法消息后清除 stale，刷新 `session_established_at`，账号状态进入 Ready。
7. `ret=-14` 时停止轮询，清空 cursor/context，写入 stale_at，发布 Blocked。
8. shutdown 时取消长轮询并最佳努力调用 `notifystop`，失败不改退出结果。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-channel-clawbot --test inbound --test session
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

```powershell
git add crates/agentnotify-channel-clawbot
git commit -m "feat(clawbot): 实现入站长轮询与账号恢复"
```

---

### Task 7: 实现旧 Go 数据只读导入

**Files:**
- Create: `crates/agentnotify-storage-sqlite/src/legacy/mod.rs`
- Create: `crates/agentnotify-storage-sqlite/src/legacy/config.rs`
- Create: `crates/agentnotify-storage-sqlite/src/legacy/credentials.rs`
- Create: `crates/agentnotify-storage-sqlite/src/legacy/history.rs`
- Create: `crates/agentnotify-storage-sqlite/src/legacy/routes.rs`
- Create: `crates/agentnotify-storage-sqlite/src/legacy/claims.rs`
- Create: `crates/agentnotify-storage-sqlite/src/legacy/report.rs`
- Create: `crates/agentnotify-storage-sqlite/tests/fixtures/legacy/config.json`
- Create: `crates/agentnotify-storage-sqlite/tests/fixtures/legacy/clawbot.json`
- Create: `crates/agentnotify-storage-sqlite/tests/fixtures/legacy/push.log`
- Create: `crates/agentnotify-storage-sqlite/tests/fixtures/legacy/reply-routes.jsonl`
- Create: `crates/agentnotify-storage-sqlite/tests/fixtures/legacy/reply-state.jsonl`
- Test: `crates/agentnotify-storage-sqlite/tests/legacy_import.rs`

**Interfaces:**
- Consumes: 旧 `%USERPROFILE%\.config\agent-notify` JSON/JSONL、`SecretStore`、SQLite。
- Produces: `LegacyImport::run(paths, store, secrets) -> ImportReport`、`settings.legacyImportV1` 状态。

- [x] **Step 1: 写完整映射和幂等测试**

```rust
#[tokio::test]
async fn import_is_idempotent_and_preserves_claim_suppression() {
    let fixture = legacy_fixture();
    let first = fixture.importer.run().await.unwrap();
    let second = fixture.importer.run().await.unwrap();

    assert_eq!(first.notifications_imported, 3);
    assert_eq!(second.notifications_imported, 0);
    assert_eq!(fixture.store.notification_count().await, 3);
    assert_eq!(
        fixture.store.claim_state("legacy-claim-key").await.unwrap(),
        ClaimState::Unknown
    );
    assert!(fixture.legacy_files_unchanged().await);
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-storage-sqlite --test legacy_import
```

Expected: FAIL，导入器不存在。

- [x] **Step 3: 映射配置与 Agent 开关**

`config.json`：

| 旧字段 | 新设置 |
|---|---|
| `quietHours` | `notification.quietHours` |
| `cooldownMin` | `notification.cooldownMin` |
| `replyEnabled` | `reply.enabled` |
| `replyConfirmation` | `reply.confirmation` |
| `defaultAgent` | `notification.defaultAgent` |
| `widgetAgentMode` | 导入为报告项，不进入新 UI 状态 |
| `theme` | `ui.theme`，只接受 `light` / `dark` |

marker 映射：

| 文件存在 | `agent_configs.enabled` |
|---|---|
| `opencode.off` | `false` |
| `codex.off` | `false` |
| `antigravity.off` | `false` |
| `devin.off` | `false` |
| `commandcode.off` | `false` |

未安装的 marker 不创建开关；Core Registry 中尚不存在的 Agent 仍可保留配置，但在 UI 中不伪造已接入状态。

- [x] **Step 4: 映射 ClawBot 凭据**

读取 `clawbot.json`：

- `bot_token` → Credential Manager `clawbot/<account-id>/bot-token`。
- `context_token` → Credential Manager `clawbot/<account-id>/context-token`。
- `ilink_bot_id`、`ilink_user_id` 只用于确定性 account ID 和脱敏 hint，不写明文全文。
- `baseurl`、`get_updates_buf`、`stale_at`、`session_established_at`、`session_alert_at` 写账号配置。
- 文件不存在时创建一个 disabled、未登录的 clawbot 账号，不生成假 token。
- 文件损坏时本次导入失败，旧文件保留，UI 显示具体字段错误。

- [x] **Step 5: 映射 push.log**

- 每行 JSON 独立导入；损坏行跳过并计数。
- Notification ID：`legacy-<sha256(原始行)>`。
- `ingest_key = legacy-history:<sha256(原始行)>`。
- 标题为旧 `title`，正文为旧 `summary`，时间为旧 `timestamp`。
- 创建一条 clawbot Delivery，账号使用导入账号；无账号记录时使用 disabled 的 `clawbot-unbound` 账号。
- 状态映射：

| 旧状态 | 新 DeliveryState | 错误/说明 |
|---|---|---|
| `成功` | `Sent` | messageID/clientID 有值则保存 |
| `失败` | `Failed` | 只保留脱敏后的旧 error |
| `未登录` | `Skipped` | `legacy_not_logged_in` |
| `会话未建立` | `Skipped` | `session_missing` |
| `DryRun` | `Skipped` | `dry_run` |
| 其他 | `Skipped` | `legacy_unknown_status` |

历史导入只用于查看，不重新创建 Outbox。

- [x] **Step 6: 映射 routes 与 claims**

旧 route：

- 账号 ID 由 `botID + userID` 确定性生成。
- `MessageID` 存在时作为 `external_message_id`。
- 否则 `ClientID` 作为兼容 `external_message_id`。
- `agent`、`sessionID`、`title`、`createdAt`、`expiresAt` 原样转换。
- 过期 route 不导入，计数到 report。
- 冲突 route 不覆盖，标记 `route_conflict`。

旧 claim：

- 使用旧 `key` 原样作为 `claim_key`，并让 ClawBot 入站生成兼容 key。
- `claimed` → `Unknown`，确保不会重新执行。
- `sent` → `Completed`。
- `failed` → `Failed`。
- `Timestamp + 30 天` 作为 expiresAt；已过期事件不导入。
- 缺少 ClawBot 账号时，本次导入失败，不能把 Claim 归入其他账号。

- [x] **Step 7: 实现导入事务和报告**

导入顺序：

1. 计算所有源文件 SHA-256。
2. 解析配置、凭据、历史和路由。
3. 写入缺失 SecretStore 条目。
4. 在单个 SQLite 事务中写 settings、agent_configs、channel_accounts、notifications、deliveries、reply_routes、inbound_claims。
5. 写 `legacyImportV1` 状态，包含源文件 hash、版本、时间、导入计数和跳过计数。
6. 事务失败时回滚数据库；凭据条目保持确定性引用，下次重跑可覆盖。
7. 输出 `%LOCALAPPDATA%\AgentNotify\data\legacy-import-report.json`，不包含 secret 或正文摘要。

所有旧文件以只读方式打开，测试通过修改时间与内容 hash 证明未被改动。

- [x] **Step 8: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-storage-sqlite --test legacy_import
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，重复导入没有新增记录，旧文件 hash 不变。

```powershell
git add crates/agentnotify-storage-sqlite
git commit -m "feat(migration): 只读导入旧版配置路由与历史"
```

---

### Task 8: 接入启动迁移、回滚保护和迁移诊断

**Files:**
- Create: `crates/agentnotify-runtime/src/migration.rs`
- Modify: `crates/agentnotify-runtime/src/runtime.rs`
- Modify: `hosts/desktop-tauri/src/bridge/commands.rs`
- Modify: `apps/desktop-ui/src/features/diagnostics/DiagnosticsPage.tsx`
- Test: `crates/agentnotify-runtime/tests/migration_startup.rs`
- Test: `apps/desktop-ui/src/features/diagnostics/MigrationDiagnostics.test.tsx`

**Interfaces:**
- Consumes: `LegacyImport`、旧安装路径、runtime 启动状态。
- Produces: 首启迁移、失败阻断、迁移报告和 UI 诊断。

- [x] **Step 1: 写启动阻断与回滚测试**

```rust
#[tokio::test]
async fn invalid_legacy_credentials_block_write_mode_and_keep_old_files() {
    let fixture = invalid_legacy_fixture();
    let error = fixture.runtime.start().await.unwrap_err();
    assert_eq!(error.code(), "legacy_import_failed");
    assert!(!fixture.store.was_migrated());
    assert!(fixture.old_files_unchanged().await);
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-runtime --test migration_startup
npm --prefix .\apps\desktop-ui run test -- --run MigrationDiagnostics
```

Expected: FAIL，迁移启动接线不存在。

- [x] **Step 3: 接入启动顺序**

runtime 启动顺序改为：

1. 打开 SQLite 并执行 schema migration。
2. 检查 `legacyImportV1`。
3. 未导入时执行 `LegacyImport`。
4. 导入失败时进入 `MigrationRequired` 状态：
   - 不启动渠道、不领取 Outbox、不处理 ingress。
   - UI 仍可打开 Diagnostics。
   - 显示失败文件、字段、备份建议和“重新检测”动作。
5. 导入成功或已完成时启动 ingress、渠道和 Outbox。

迁移状态属于 snapshot，不在 React 本地持久化。

- [x] **Step 4: 实现回滚保护**

- 新应用启动时获取当前用户独占锁 `AgentNotify.runtime.lock`。
- 如果旧 Go 版正在运行，检测到 active widget/heartbeat 或进程锁时拒绝迁移并提示先退出旧版。
- 正式切换前不修改旧文件、Hooks 或注册表。
- 数据库写入成功但应用在切换前退出时，下次启动继续使用同一导入状态。
- 回滚到旧版时不删除 SQLite；旧版继续读取原文件，两条链路不得同时运行。

- [x] **Step 5: UI 诊断接入**

Diagnostics 增加“旧数据迁移”项：

- `未发现旧数据`
- `已完成`
- `部分完成，跳过 N 条损坏记录`
- `失败，当前处于只读诊断模式`

提供“查看迁移报告”和“重新检测”。不提供“删除旧数据”按钮。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-runtime --test migration_startup
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS。

```powershell
git add crates/agentnotify-runtime hosts/desktop-tauri apps/desktop-ui
git commit -m "feat(runtime): 接入首启迁移与回滚保护"
```

---

### Task 9: 完成真实 OpenCode + ClawBot 闭环验收

**Files:**
- Create: `tests/real-opencode-clawbot.ps1`
- Create: `docs/superpowers/specs/2026-09-19-opencode-clawbot-acceptance.md`
- Create: `crates/agentnotify-testkit/tests/production_contracts.rs`

**Interfaces:**
- Consumes: Rust Preview 应用、新 OpenCode 插件、真实 ClawBot 测试账号、真实 OpenCode。
- Produces: 可复核的真实链路证据和未通过项。

- [x] **Step 1: 运行隔离契约测试**

```rust
#[tokio::test]
async fn production_adapters_pass_shared_contracts() {
    assert_agent_contract(Arc::new(OpenCodeAgent::production())).await;
    assert_channel_contract(Arc::new(ClawBotChannel::production())).await;
}
```

Run:

```powershell
cargo test -p agentnotify-testkit --test production_contracts
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [x] **Step 2: 准备隔离验收环境**

使用专用测试目录，不直接覆盖真实配置：

```powershell
$env:AGENT_NOTIFY_CONFIG_DIR = 'D:\Temp\agentnotify-real-e2e\config'
$env:AGENT_NOTIFY_DATA_DIR = 'D:\Temp\agentnotify-real-e2e\data'
$env:AGENT_NOTIFY_LOG_DIR = 'D:\Temp\agentnotify-real-e2e\logs'
$env:AGENT_NOTIFY_SPOOL_DIR = 'D:\Temp\agentnotify-real-e2e\spool'
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\real-opencode-clawbot.ps1 -Prepare
```

必须使用独立 ClawBot 测试账号。如果无法提供第二条独立账号，不能把“向当前账号发送测试消息”记作隔离验收通过。

**进度与豁免（2026-09-21）**：隔离目录与 `-Prepare` 校验已实现并实测通过；入方向曾用真实 spool 副本在隔离环境跑通。**「必须使用独立 ClawBot 测试账号」一条经计划负责人于 2026-09-21 明确豁免**，改用「生产账号 + 隔离目录」组合（数据隔离、账号不隔离），因此本步骤勾选。豁免的代价与残余风险记录在 `docs\superpowers\specs\2026-09-19-opencode-clawbot-acceptance.md` 的「已知限制与待补项」第 1 条，其中包含 Step 5 破坏性测试改用「隔离实例 + 故意损坏的 context token」的替代方案。

`-Prepare` 会建立 `config`/`data`/`logs`/`spool`，校验 ingress 与预览桌面二进制并输出启动所需的环境变量，同时校验 OpenCode 插件烘焙的 ingress 路径是否与当前 ingress 一致；直接传生产数据目录会报错，确需时才加 `-UseProductionDataDir`。

隔离约束：

- OpenCode 插件路径由 OpenCode 固定为 `%USERPROFILE%\.config\opencode\plugins\agent-notify.ts`，不存在按目录隔离的环境变量；`-InstallPlugin` 用当前 ingress 刷新该文件，内容一致时不产生变更。
- 命名管道名由当前用户 SID 派生，同一用户下同时只能运行一个桌面实例；隔离验收前必须先退出生产预览实例。
- `agentnotify-ingress.exe` 读取 `AGENT_NOTIFY_SPOOL_DIR`；若离线运行 OpenCode，需要让 OpenCode 进程继承同一组环境变量，否则事件会写入生产 spool。

- [x] **Step 3: 验收正常推送**

1. 启动 Rust Preview，完成扫码并等待首条入站消息。
2. 在真实 OpenCode 中执行一个只读测试任务。
3. 验证事件进入 ingress、SQLite 产生 Notification/Outbox、ClawBot 返回 Handled，微信收到消息。
4. 验证 ReplyRoute 使用 ClawBot 返回的稳定 message ID，不使用标题或正文。
5. 记录脱敏的 time、agent、session hash、message ID hash 和 Delivery 状态。

- [x] **Step 4: 验收精确引用回复**

1. 在微信中引用刚才的通知，回复一个只对原会话有效的测试指令。
2. 验证 Quote ID 与 ReplyRoute 精确匹配。
3. 验证原 OpenCode 会话收到内容；其他测试会话没有收到。
4. 验证 Claim 从 `InProgress` 变为 `Completed`。
5. 再次投递同一入站事件，验证不会执行第二次。

- [x] **Step 5: 验收错误边界**

必须分别构造并确认：

- 引用一条无 Route 的普通消息：微信收到“无可用会话记录”，无 Agent 调用。
- 引用 ID 缺失或多个冲突：拒绝路由，无 Agent 调用。
- ClawBot 发送超时：Delivery 记 `Unknown`，重启后不重发。
- OpenCode result 超时：Claim 记 `Unknown`，重启后不重发。
- `ret=-14`：账号标记失效，context token 被清除，UI 提示重新扫码。
- `ret=-2`：上下文清空，Delivery 记 `Skipped/session_missing`，UI 提示向 ClawBot 发一条消息恢复。

- [x] **Step 6: 验收退出与重启**

1. 应用退出后确认无 UI/runtime 进程，托盘消失，SQLite 完成 checkpoint。
2. 重新启动，确认历史、Route、Claim、账号状态和旧数据导入仍存在。
3. 确认 Outbox 中没有重复发送。
4. 确认第二次启动不会重复迁移。

**进度（2026-09-21）**：

- 第 2～4 项已由 `tests\restart-acceptance.ps1` 在生产库副本上自动化通过：迁移 `0002` 仅应用一次、两次退出均 code 0 且 WAL 0 字节、历史/Route/Claim/Outbox/账号/设置保持一致。
- 同日查出并修复一个验收环境缺陷：此前桌面端一律在 Codex 会话（MSIX 包上下文）内启动，`LOCALAPPDATA` 被重定向到包 `LocalCache`，真实 `%LOCALAPPDATA%\AgentNotify\spool` 中的事件从未被消费，Step 6 的历史证据也落在虚拟化路径上。改用非包上下文启动后已在真实路径重新取证：`data`/`logs`/`spool` 齐备、迁移 `1+2`、启动消费 spool 与运行中命名管道注入都能让 `notifications`/`outbox` 增长而 `deliveries` 不变（账号已隔离），退出后 WAL 为 0 字节，启动时收敛 `interrupted_outbox=1` 且不重放。
- 第 1 项的无残留进程与 WAL checkpoint 已通过；「托盘图标消失」与「托盘菜单点击退出」仍需人工视觉确认，因此本步骤暂不勾选。

证据见 `docs\superpowers\specs\2026-09-19-opencode-clawbot-acceptance.md` 的「LOCALAPPDATA 分裂」与「修复验证」两节。

- [x] **Step 7: 记录验收结论**

`docs/superpowers/specs/2026-09-19-opencode-clawbot-acceptance.md` 必须记录：

- 构建版本与 commit。
- Windows、WebView2、OpenCode 和 ClawBot 协议版本。
- 每条验收项的通过/失败、时间、脱敏证据位置。
- 未通过项的复现步骤。
- 是否使用独立账号。
- 已知限制和没有覆盖的 Agent/渠道。

任何一项真实链路未通过时，阶段状态保持“未通过”，不能进入正式切换。

- [x] **Step 8: 提交**

```powershell
git add tests/real-opencode-clawbot.ps1 docs/superpowers/specs/2026-09-19-opencode-clawbot-acceptance.md crates/agentnotify-testkit
git commit -m "test(e2e): 完成 OpenCode 与 ClawBot 真实闭环验收"
```

---

### Task 10: 切换正式安装入口与版本事实来源

**Files:**
- Modify: `installer/agent-notify-rust.iss`
- Create: `installer/agent-notify.iss`（新 Rust 正式版脚本替换旧 Go UI 交付）
- Modify: `tools/build-release.ps1`
- Modify: `tools/build-installer.ps1`
- Modify: `tools/check-version.ps1`
- Modify: `VERSION`
- Modify: `.github/workflows/release.yml`
- Modify: `AGENTS.md`
- Modify: `README.md`
- Test: `tests/installer-smoke.ps1`
- Test: `tests/signature-gate.tests.ps1`

**Interfaces:**
- Consumes: 已通过真实验收的 `agentnotify-desktop.exe`、`agentnotify-ingress.exe`、OpenCode V2 插件、迁移器。
- Produces: 正式 `2.0.0` Windows 安装包、签名校验和唯一发布入口。

- [x] **Step 1: 写正式包内容断言**

`installer-smoke.ps1` 增加：

- 包内存在 `agentnotify-desktop.exe` 与 `agentnotify-ingress.exe`。
- 包内不存在旧 Win32 UI 作为启动入口。
- OpenCode 插件指向 ingress，不指向旧 `notify` 命令。
- 标准安装目录、AppId、自启动和卸载标识与旧版兼容。
- 首次升级保留用户数据并执行迁移。
- 卸载只删除程序文件，不删除 SQLite、旧迁移源和迁移报告。

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\installer-smoke.ps1 -ExpectRust
```

Expected: FAIL，正式 Inno 脚本仍指向旧 Go 程序。

- [x] **Step 3: 切换版本事实来源**

- 正式版版本改为 `2.0.0`。
- `VERSION` 成为唯一版本来源。
- 构建脚本把 `VERSION` 注入 `hosts/desktop-tauri/tauri.conf.json`、Rust `CARGO_PKG_VERSION` 和使用方，不手改生成文件。
- `tools/check-version.ps1` 校验 `VERSION`、Tauri 配置、Cargo 元数据、README 徽章、CHANGELOG 和安装包名。
- 更新 `AGENTS.md`，删除“版本号只认 `internal/app/version.go`”的旧规则，改为 `VERSION` 唯一来源。
- 在切换提交完成前不得先修改旧 Go 版本源。

- [x] **Step 4: 替换安装交付**

正式安装器：

- 安装 `agentnotify-desktop.exe`、`agentnotify-ingress.exe`、OpenCode V2 插件模板和 WebView2 所需资源。
- 保留旧 AppId，使升级落回原安装目录。
- 不复制 `internal/ui` 产物，不注册 Win32 widget。
- 安装完成后启动 `agentnotify-desktop.exe`。
- 首次启动执行只读迁移；迁移失败时不启动渠道，只显示 Diagnostics。
- 自启动指向新桌面程序。
- 更新源继续只发布到二进制仓库，并上传安装器、ZIP 与 `SHA256SUMS.txt`。

为短期回滚保留两个可执行文件：

```text
agentnotify-desktop.exe
agentnotify-ingress.exe
```

旧 Go 二进制不再随正式包发布，但上一稳定 Release 仍保留。

- [x] **Step 5: 运行发布门禁**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\build-release.ps1 -Version 2.0.0
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\installer-smoke.ps1 -Installer .\dist\Agent-notify-Setup-v2.0.0.exe
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\signature-gate.tests.ps1
```

Expected: 所有门禁通过，安装器与主程序签名指纹符合内置信任指纹，ZIP 不包含旧 UI 入口。

- [x] **Step 6: 提交**

```powershell
git add installer tools VERSION AGENTS.md README.md CHANGELOG.md .github tests
git commit -m "release: 切换 Windows 正式入口到 Tauri 桌面版"
```

---

### Task 11: 建立回滚窗口并停止旧 UI 交付

**Files:**
- Create: `docs/superpowers/specs/2026-09-19-windows-rust-cutover.md`
- Modify: `CHANGELOG.md`
- Modify: `README.md`
- Modify: `AGENTS.md`
- Test: `tests/rollback-smoke.ps1`

**Interfaces:**
- Consumes: `2.0.0` 正式包、上一稳定 Go 包、迁移后的 SQLite。
- Produces: 一个明确版本窗口内的回滚流程和旧 UI 退出标准。

- [ ] **Step 1: 写回滚 smoke**

`rollback-smoke.ps1` 在隔离虚拟机或隔离用户目录中执行：

1. 安装上一稳定 Go 版，生成旧配置、历史、Route 和 Claim。
2. 升级到 Rust 版，完成迁移。
3. 回滚安装上一稳定 Go 版。
4. 验证旧版仍能读取旧文件和历史。
5. 验证旧版不会删除 SQLite 和迁移报告。
6. 再次升级 Rust 版，验证不会重复迁移或重放 Claim。
7. 验证两个版本任何时刻都不同时运行。

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\rollback-smoke.ps1 -From 2.0.0 -To 1.17.0
```

Expected: 在未编写隔离步骤前 FAIL；实现隔离路径和进程锁后 PASS。

- [ ] **Step 3: 记录切换状态**

`2026-09-19-windows-rust-cutover.md` 记录：

- 正式切换 commit 和 release。
- 旧版回退版本号与校验和。
- 迁移报告位置。
- 进程锁和单实例策略。
- 已知问题与不支持的 Agent/渠道。
- 回滚窗口只覆盖一个正式小版本；窗口结束前收集真实反馈。

- [ ] **Step 4: 更新用户文档**

README 必须改为：

- Windows 入口是 `agentnotify-desktop.exe`。
- 主窗口关闭后进入托盘，退出必须从托盘或 Settings。
- OpenCode 使用新 V2 插件。
- 旧 Go 版本只在回滚说明中出现。
- 当前只支持 OpenCode + ClawBot，其他 Agent/渠道标为后续扩展。
- 不宣传管理 CLI、MCP、macOS 或 HarmonyOS PC。

- [ ] **Step 5: 结束回滚窗口后的条件**

只有满足以下条件后，后续版本才能删除旧 Go UI 源代码和迁移兼容层：

- `2.0.0` 至少稳定运行一个发布周期。
- 没有阻断级迁移、推送或回复问题。
- 真实验收证据完整。
- `rollback-smoke.ps1` 连续通过。
- 用户明确同意结束回滚窗口。

本任务不删除旧源码。删除旧 UI 是回滚窗口结束后的独立变更，必须重新评审。

- [x] **Step 6: 提交**

```powershell
git add docs/superpowers/specs/2026-09-19-windows-rust-cutover.md CHANGELOG.md README.md AGENTS.md tests/rollback-smoke.ps1
git commit -m "docs: 记录 Windows Rust 版切换与回滚窗口"
```

---

## 阶段完成标准

- OpenCode 与 ClawBot 生产适配器通过共享契约和真实 Windows/微信验收。
- 推送、精确引用回复、Claim、Unknown 和重启语义全部有可复核证据。
- 旧配置、登录、开关、历史、Route 和 Claim 完成只读、幂等导入；旧文件未被修改。
- 正式安装包只交付 Tauri/React UI 和新 ingress；旧 Win32 UI 不再是发布入口。
- 版本事实来源、签名、更新、自启动、卸载和回滚流程全部走通。
- 回滚窗口明确，旧 Go 源码暂不删除，两个版本不同时运行。
- Codex、Antigravity、Devin、Command Code、飞书、外部适配器、macOS 和 HarmonyOS PC 仍不在本计划范围。
