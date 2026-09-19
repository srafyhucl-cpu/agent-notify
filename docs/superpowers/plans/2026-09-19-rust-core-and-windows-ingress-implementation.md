# Rust 核心与 Windows 内部入口 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建一个不依赖 UI 的 Rust 核心，使用 SQLite、事务型 Outbox 和精确回复路由跑通假 Agent + 假渠道闭环，并提供只接收 Agent 事件的 Windows 命名管道与离线 spool。

**Architecture:** `agentnotify-domain` 保存纯规则，`agentnotify-application` 编排用例，`agentnotify-runtime` 负责注册、监督和事件循环，SQLite 与适配器实现端口。`agentnotify-ingress.exe` 只把版本化事件送入命名管道；管道不可用时写 spool 后退出。

**Tech Stack:** Rust stable、edition 2024、Tokio、`rusqlite`、`serde`、`thiserror`、`time`、`uuid`、`sha2`、`tracing`、`async-trait`、`windows`、`tempfile`、`rstest`。

## Global Constraints

- 只交付 Windows 10/11；核心和适配器不得依赖 Windows API。
- 不提供管理 CLI、状态查询命令、配置命令和 MCP。
- `agentnotify-ingress.exe` 只能提交 `agent.event`，不能接受任意 shell、路径、渠道登录或管理参数。
- 推送可对确定性可重试错误重试；引用回复不做自动重放；`Unknown` 永不由后台自动重试。
- 路由键必须是 `ChannelId + ChannelAccountId + ExternalMessageId`；缺失或冲突时拒绝处理。
- SQLite 迁移只前进；迁移 SQL 一旦提交不得改内容，只能追加新版本。
- 密钥只通过 `SecretStore` 访问，不进入 SQLite、日志、panic 消息或测试快照。
- 所有仓储调用必须显式接收 `ChannelAccountId`；不得提供“默认账号”或“最近账号”隐式回退。
- 注释和错误文案使用中文；Rust 公共类型与字段使用英文。
- Cargo 缓存、target 和测试临时目录固定到 D 盘；门禁脚本负责设置目录。
- 每个任务结束运行 `tools/rust/gate.ps1` 并提交；不得把多个任务合并成一次提交。

---

### Task 1: 建立 Rust workspace 与可重复门禁

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`（追加 Rust 与前端忽略项）
- Create: `tools/rust/gate.ps1`
- Create: `crates/agentnotify-domain/Cargo.toml`
- Create: `crates/agentnotify-domain/src/lib.rs`
- Create: `crates/agentnotify-application/Cargo.toml`
- Create: `crates/agentnotify-application/src/lib.rs`
- Create: `crates/agentnotify-runtime/Cargo.toml`
- Create: `crates/agentnotify-runtime/src/lib.rs`
- Create: `crates/agentnotify-storage-sqlite/Cargo.toml`
- Create: `crates/agentnotify-storage-sqlite/src/lib.rs`
- Create: `crates/agentnotify-agent-sdk/Cargo.toml`
- Create: `crates/agentnotify-agent-sdk/src/lib.rs`
- Create: `crates/agentnotify-channel-sdk/Cargo.toml`
- Create: `crates/agentnotify-channel-sdk/src/lib.rs`
- Create: `crates/agentnotify-testkit/Cargo.toml`
- Create: `crates/agentnotify-testkit/src/lib.rs`
- Test: `Cargo.lock`（由 Cargo 生成并提交）

**Interfaces:**
- Consumes: 无。
- Produces: 根 workspace 与七个当前 Rust crate；`agentnotify-ingress` 由后续 Task 14 加入后才成为第八个包；每份执行计划的 Cargo 命令都以这些包名工作。

- [ ] **Step 1: 验证 Windows Rust 工具链，缺少时安装到 D 盘**

先检查：

```powershell
rustc --version
cargo --version
```

如果命令不存在，使用独立目录安装，不把 `RUSTUP_HOME` 或 `CARGO_HOME` 留在 C 盘：

```powershell
$env:RUSTUP_HOME = 'D:\Tools\rustup'
$env:CARGO_HOME = 'D:\Tools\cargo'
$env:TEMP = 'D:\Temp\agentnotify-temp'
$env:TMP = 'D:\Temp\agentnotify-temp'
New-Item -ItemType Directory -Force -Path 'D:\Temp\agentnotify-rustup','D:\Temp\agentnotify-temp' | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile 'D:\Temp\agentnotify-rustup\rustup-init.exe'
& 'D:\Temp\agentnotify-rustup\rustup-init.exe' -y --profile minimal --default-toolchain stable-x86_64-pc-windows-msvc
& 'D:\Tools\cargo\bin\rustup.exe' component add rustfmt clippy
```

MSVC Build Tools 缺失时安装流程必须失败并提示安装“Desktop development with C++”。不要改用 GNU 工具链，因为 Tauri Windows 宿主固定使用 MSVC。

- [ ] **Step 2: 创建 workspace 清单**

创建 `Cargo.toml`：

```toml
[workspace]
resolver = "3"
members = [
  "crates/agentnotify-domain",
  "crates/agentnotify-application",
  "crates/agentnotify-runtime",
  "crates/agentnotify-storage-sqlite",
  "crates/agentnotify-agent-sdk",
  "crates/agentnotify-channel-sdk",
  "crates/agentnotify-testkit",
]

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "MIT"
rust-version = "1.85"

[workspace.dependencies]
async-trait = "0.1"
futures = "0.3"
rusqlite = { version = "0.37", features = ["bundled", "serde_json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
sha2 = "0.10"
thiserror = "2"
time = { version = "0.3", features = ["serde", "formatting", "parsing"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time", "fs", "process"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
```

创建 `rust-toolchain.toml`：

```toml
[toolchain]
channel = "stable-x86_64-pc-windows-msvc"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

每个 crate 的 `Cargo.toml` 使用 `version.workspace = true`、`edition.workspace = true`、`license.workspace = true` 和按依赖方向声明的 workspace dependency。每个 `src/lib.rs` 至少导出一个真实类型或 trait；不得创建空占位 crate。

- [ ] **Step 3: 创建门禁脚本**

创建 `tools/rust/gate.ps1`：

```powershell
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$env:CARGO_HOME = 'D:\Tools\cargo'
$env:RUSTUP_HOME = 'D:\Tools\rustup'
$env:CARGO_TARGET_DIR = 'D:\Temp\agentnotify-rust-target'
$env:TEMP = 'D:\Temp\agentnotify-temp'
$env:TMP = $env:TEMP
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
New-Item -ItemType Directory -Force -Path $env:CARGO_TARGET_DIR,$env:TEMP | Out-Null
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    throw "找不到 cargo.exe：$cargo。请先把 Rust 工具链安装到 D 盘。"
}

Push-Location $root
try {
    & $cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt 失败' }
    & $cargo clippy --workspace --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy 失败' }
    & $cargo test --workspace --all-features
    if ($LASTEXITCODE -ne 0) { throw 'cargo test 失败' }
}
finally {
    Pop-Location
}
```

`.gitignore` 追加：

```gitignore
/target/
/apps/desktop-ui/node_modules/
/apps/desktop-ui/dist/
/apps/desktop-ui/playwright-report/
/apps/desktop-ui/test-results/
```

- [ ] **Step 4: 生成锁文件并运行门禁**

Run:

```powershell
cargo generate-lockfile
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
cargo metadata --no-deps --format-version 1
```

Expected: 三条命令退出码均为 `0`，`Cargo.lock` 已生成，metadata 包含七个当前成员包；`agentnotify-ingress` 由后续 Task 14 加入后才成为第八个包。

- [ ] **Step 5: 提交**

```powershell
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore tools/rust/gate.ps1 crates
git commit -m "build: 初始化 Rust workspace 与质量门禁"
```

---

### Task 2: 定义强类型标识、时间和领域错误

**Files:**
- Create: `crates/agentnotify-domain/src/identifier.rs`
- Create: `crates/agentnotify-domain/src/timestamp.rs`
- Create: `crates/agentnotify-domain/src/error.rs`
- Modify: `crates/agentnotify-domain/src/lib.rs`
- Test: `crates/agentnotify-domain/tests/identifier.rs`

**Interfaces:**
- Consumes: Task 1 的 `agentnotify-domain` crate。
- Produces: `AgentId`、`AgentSessionId`、`ChannelId`、`ChannelAccountId`、`ExternalMessageId`、`NotificationId`、`DeliveryId`、`InboundMessageId`、`RequestId`、`Timestamp`、`DomainError`。

- [ ] **Step 1: 写失败测试**

创建 `crates/agentnotify-domain/tests/identifier.rs`：

```rust
use agentnotify_domain::{AgentId, ChannelAccountId, DomainError, Timestamp};

#[test]
fn string_ids_reject_empty_and_whitespace() {
    assert!(matches!(AgentId::new(""), Err(DomainError::InvalidIdentifier { .. })));
    assert!(matches!(
        ChannelAccountId::new("  "),
        Err(DomainError::InvalidIdentifier { .. })
    ));
}

#[test]
fn ids_are_not_interchangeable() {
    let agent = AgentId::new("opencode").unwrap();
    assert_eq!(agent.as_str(), "opencode");
    assert_eq!(agent.to_string(), "opencode");
}

#[test]
fn timestamp_round_trips_rfc3339_millis() {
    let value = Timestamp::parse_rfc3339("2026-09-19T10:20:30.123+08:00").unwrap();
    assert_eq!(value.to_rfc3339(), "2026-09-19T10:20:30.123+08:00");
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-domain --test identifier
```

Expected: FAIL，`AgentId` 或 `Timestamp` 不存在。

- [ ] **Step 3: 实现标识与时间**

在 `identifier.rs` 使用宏生成独立新类型：

```rust
macro_rules! define_string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, crate::DomainError> {
                let value = value.into();
                let trimmed = value.trim();
                if trimmed.is_empty() || trimmed != value {
                    return Err(crate::DomainError::InvalidIdentifier {
                        kind: stringify!($name),
                    });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl TryFrom<String> for $name {
            type Error = crate::DomainError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

define_string_id!(AgentId);
define_string_id!(AgentSessionId);
define_string_id!(ChannelId);
define_string_id!(ChannelAccountId);
define_string_id!(ExternalMessageId);
define_string_id!(NotificationId);
define_string_id!(DeliveryId);
define_string_id!(InboundMessageId);
define_string_id!(RequestId);
```

`timestamp.rs`：

```rust
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    pub fn now_utc() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }

    pub fn parse_rfc3339(value: &str) -> Result<Self, crate::DomainError> {
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Self)
            .map_err(|_| crate::DomainError::InvalidTimestamp)
    }

    pub fn to_rfc3339(self) -> String {
        self.0
            .format(&time::format_description::well_known::Rfc3339)
            .expect("OffsetDateTime 使用固定 RFC3339 格式不会失败")
    }

    pub fn checked_add(self, duration: time::Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }
}
```

`error.rs` 定义 `DomainError::{InvalidIdentifier, InvalidTimestamp, InvalidStateTransition, RouteExpired, RouteMissing, RouteConflict}`，每个错误都提供稳定 `code()` 和中文 `message()`；不得把底层 token 或渠道原文放进错误。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-domain --test identifier
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，门禁无 clippy 或 fmt 问题。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-domain
git commit -m "feat(domain): 增加强类型标识与时间值对象"
```

---

### Task 3: 实现 Notification 与 Delivery 状态机

**Files:**
- Create: `crates/agentnotify-domain/src/notification.rs`
- Create: `crates/agentnotify-domain/src/delivery.rs`
- Modify: `crates/agentnotify-domain/src/lib.rs`
- Test: `crates/agentnotify-domain/tests/delivery_state.rs`

**Interfaces:**
- Consumes: Task 2 的标识、`Timestamp` 和 `DomainError`。
- Produces: `Notification`、`NotificationMetadata`、`SafeError`、`Delivery`、`DeliveryState`、`DeliveryReceiptState`。

- [ ] **Step 1: 写状态迁移测试**

```rust
use agentnotify_domain::{
    ChannelAccountId, ChannelId, Delivery, DeliveryErrorKind, DeliveryId, DeliveryState,
    ExternalMessageId, NotificationId, SafeError,
};

#[test]
fn pending_delivery_can_retry_only_after_retryable_failure() {
    let mut delivery = Delivery::pending(
        DeliveryId::new("delivery-1").unwrap(),
        NotificationId::new("notification-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    );
    delivery.mark_retryable(
        SafeError::new("channel_timeout", "渠道请求超时，稍后会重试").unwrap(),
    );
    assert_eq!(delivery.state(), DeliveryState::Failed);
    assert!(delivery.can_retry());
}

#[test]
fn unknown_and_sent_deliveries_never_retry() {
    let mut sent = Delivery::pending(
        DeliveryId::new("delivery-1").unwrap(),
        NotificationId::new("notification-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    );
    sent.mark_sent(ExternalMessageId::new("message-1").unwrap());
    assert!(!sent.can_retry());

    let mut unknown = Delivery::pending(
        DeliveryId::new("delivery-2").unwrap(),
        NotificationId::new("notification-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    );
    unknown.mark_unknown(SafeError::new("channel_unknown", "无法确认渠道是否已接收").unwrap());
    assert_eq!(unknown.state(), DeliveryState::Unknown);
    assert!(!unknown.can_retry());
}

#[test]
fn permanent_failure_cannot_retry() {
    let mut delivery = Delivery::pending(
        DeliveryId::new("delivery-3").unwrap(),
        NotificationId::new("notification-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    );
    delivery.mark_permanent_failure(
        SafeError::new("invalid_recipient", "渠道账号已失效，请重新登录").unwrap(),
    );
    assert!(!delivery.can_retry());
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-domain --test delivery_state
```

Expected: FAIL，类型未定义。

- [ ] **Step 3: 实现领域状态**

`notification.rs` 定义：

```rust
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Notification {
    pub id: NotificationId,
    pub ingest_key: String,
    pub agent_id: AgentId,
    pub session_id: Option<AgentSessionId>,
    pub session_title: Option<String>,
    pub title: String,
    pub body: String,
    pub occurred_at: Timestamp,
    pub metadata: NotificationMetadata,
}
```

`Notification::new` 拒绝空 `ingest_key`、空标题和空白正文。`NotificationMetadata` 内部使用 `BTreeMap<String, String>`，只允许安全元数据。

`delivery.rs` 定义：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryState {
    Pending,
    Sent,
    Failed,
    Unknown,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeError {
    code: &'static str,
    message: String,
}

impl Delivery {
    pub fn pending(
        id: DeliveryId,
        notification_id: NotificationId,
        channel_id: ChannelId,
        account_id: ChannelAccountId,
    ) -> Self;

    pub fn state(&self) -> DeliveryState;
    pub fn can_retry(&self) -> bool;
    pub fn mark_sent(&mut self, external_message_id: ExternalMessageId);
    pub fn mark_retryable(&mut self, error: SafeError);
    pub fn mark_permanent_failure(&mut self, error: SafeError);
    pub fn mark_unknown(&mut self, error: SafeError);
    pub fn mark_skipped(&mut self, error: SafeError);
}
```

所有 `mark_*` 先调用私有 `ensure_transition_allowed`。`Sent`、`Unknown`、`Skipped` 是终态；`Failed` 仅允许重试性失败再次迁移。`can_retry()` 只在 `Pending` 或带 retryable 标记的 `Failed` 上返回 `true`。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-domain --test delivery_state
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-domain
git commit -m "feat(domain): 增加通知与投递状态机"
```

---

### Task 4: 实现路由键、入站消息与 Claim 规则

**Files:**
- Create: `crates/agentnotify-domain/src/routing.rs`
- Create: `crates/agentnotify-domain/src/inbound.rs`
- Modify: `crates/agentnotify-domain/src/lib.rs`
- Test: `crates/agentnotify-domain/tests/routing.rs`

**Interfaces:**
- Consumes: Task 2 与 Task 3 的标识、时间和错误。
- Produces: `RouteKey`、`ReplyRoute`、`InboundMessage`、`ClaimKey`、`InboundClaim`、`ClaimState`、`ClaimOutcome`。

- [ ] **Step 1: 写精确路由测试**

```rust
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, ExternalMessageId,
    InboundMessage, RouteKey, Timestamp,
};

#[test]
fn same_external_message_id_in_different_accounts_is_not_equal() {
    let first = RouteKey::new(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-a").unwrap(),
        ExternalMessageId::new("message-1").unwrap(),
    );
    let second = RouteKey::new(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-b").unwrap(),
        ExternalMessageId::new("message-1").unwrap(),
    );
    assert_ne!(first, second);
}

#[test]
fn fallback_claim_key_is_deterministic_without_message_id() {
    let message = InboundMessage::without_external_id(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-a").unwrap(),
        "cursor-7",
        vec![ExternalMessageId::new("quoted-1").unwrap()],
        "继续检查",
        Timestamp::parse_rfc3339("2026-09-19T10:20:30.123+08:00").unwrap(),
    )
    .unwrap();
    let first = ClaimKey::from_inbound(&message).unwrap();
    let second = ClaimKey::from_inbound(&message).unwrap();
    assert_eq!(first, second);
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-domain --test routing
```

Expected: FAIL，`InboundMessage::without_external_id` 或 `ClaimKey` 不存在。

- [ ] **Step 3: 实现路由与 Claim 领域规则**

`RouteKey` 必须是 `Hash + Eq`，字段为三个强类型 ID。`ReplyRoute::is_active(now)` 要求 `now < expires_at`，过期时返回 `DomainError::RouteExpired`，不返回“最近可用路由”。

`InboundMessage`：

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundMessage {
    pub id: InboundMessageId,
    pub channel_id: ChannelId,
    pub account_id: ChannelAccountId,
    pub external_message_id: Option<ExternalMessageId>,
    pub sender_id: String,
    pub conversation_id: String,
    pub referenced_message_ids: Vec<ExternalMessageId>,
    pub text: String,
    pub received_at: Timestamp,
    claim_material: ClaimMaterial,
}
```

`ClaimMaterial` 保存渠道、账号、游标、引用 ID 和正文的 SHA-256；不保存额外正文副本。`ClaimKey::from_inbound`：

- 有消息 ID：`sha256("message\0channel\0account\0message_id")`
- 无消息 ID：`sha256("fallback\0channel\0account\0cursor\0joined_reference_ids\0text_hash")`
- 缺失引用 ID 时仍可生成回退键，但回复服务后续必须因路由缺失而拒绝。
- 适配器提供兼容 `legacy_key` 时优先使用它；ClawBot 用它匹配旧版 `reply-state.jsonl` 的确定性 key。

`InboundClaim` 状态为 `InProgress`、`Completed`、`Failed`、`Unknown`。`ClaimOutcome` 为 `Acquired(InboundClaim)` 或 `AlreadyClaimed { state, updated_at }`。一旦记录已存在，任何状态都不允许重新执行 Agent。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-domain --test routing
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-domain
git commit -m "feat(domain): 增加精确回复路由与入站 Claim"
```

---

### Task 5: 建立 Agent SDK、注册表和契约测试

**Files:**
- Create: `crates/agentnotify-agent-sdk/src/descriptor.rs`
- Create: `crates/agentnotify-agent-sdk/src/adapter.rs`
- Create: `crates/agentnotify-agent-sdk/src/registry.rs`
- Create: `crates/agentnotify-agent-sdk/src/contract.rs`
- Modify: `crates/agentnotify-agent-sdk/src/lib.rs`
- Test: `crates/agentnotify-agent-sdk/tests/registry.rs`
- Test: `crates/agentnotify-agent-sdk/tests/contract.rs`

**Interfaces:**
- Consumes: `agentnotify-domain`。
- Produces: `AgentDescriptor`、`AgentCapabilities`、`AgentHealth`、`AgentEventEnvelope`、`NormalizedAgentEvent`、`ResumeReceipt`、`AgentError`、`AgentAdapter`、`AgentRegistry`、`assert_agent_contract`。

- [ ] **Step 1: 写注册表与契约失败测试**

```rust
#[tokio::test]
async fn registry_rejects_duplicate_agent_id() {
    let mut registry = AgentRegistry::default();
    registry.register(Arc::new(FakeAgent::new("opencode"))).unwrap();
    let error = registry.register(Arc::new(FakeAgent::new("opencode"))).unwrap_err();
    assert_eq!(error.code(), "agent_already_registered");
}

#[tokio::test]
async fn adapter_contract_rejects_empty_resume_text() {
    let adapter = Arc::new(FakeAgent::new("opencode"));
    assert_agent_contract(adapter).await;
}
```

`FakeAgent` 放在测试文件内，返回稳定 descriptor，并在 `text.trim().is_empty()` 时返回 `AgentError::InvalidInput`。

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-agent-sdk --test registry --test contract
```

Expected: FAIL，SDK 类型未定义。

- [ ] **Step 3: 实现 SDK**

公开接口保持：

```rust
#[async_trait::async_trait]
pub trait AgentAdapter: Send + Sync {
    fn descriptor(&self) -> AgentDescriptor;
    fn capabilities(&self) -> AgentCapabilities;

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError>;

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError>;

    async fn inspect(&self) -> AgentHealth;
}
```

`AgentRegistry` 使用 `RwLock<HashMap<AgentId, Arc<dyn AgentAdapter>>>`；`register` 拒绝重复 ID，`get` 返回克隆后的 `Arc`，`all` 按 `AgentId` 排序，保证 UI 和测试结果稳定。

`assert_agent_contract` 必须验证：

- descriptor ID 与注册 ID 一致。
- `capabilities().resume == false` 时，`resume` 返回 `AgentError::UnsupportedCapability`。
- 空 session ID 或空正文返回 `AgentError::InvalidInput`。
- 未知事件返回 `AgentError::InvalidEvent`，不 panic。
- contract 不访问真实用户目录、网络或 Agent 客户端。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-agent-sdk
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-agent-sdk
git commit -m "feat(agent-sdk): 增加 Agent 协议与契约测试"
```

---

### Task 6: 建立 Channel SDK、账号隔离和契约测试

**Files:**
- Create: `crates/agentnotify-channel-sdk/src/descriptor.rs`
- Create: `crates/agentnotify-channel-sdk/src/account.rs`
- Create: `crates/agentnotify-channel-sdk/src/adapter.rs`
- Create: `crates/agentnotify-channel-sdk/src/login.rs`
- Create: `crates/agentnotify-channel-sdk/src/registry.rs`
- Create: `crates/agentnotify-channel-sdk/src/contract.rs`
- Modify: `crates/agentnotify-channel-sdk/src/lib.rs`
- Test: `crates/agentnotify-channel-sdk/tests/login.rs`
- Test: `crates/agentnotify-channel-sdk/tests/contract.rs`

**Interfaces:**
- Consumes: `agentnotify-domain`。
- Produces: `ChannelDescriptor`、`ChannelCapabilities`、`InboundMode`、`ChannelAccount`、`SecretRef`、`ChannelHealth`、`OutboundMessage`、`DeliveryReceipt`、`ChannelError`、`ChannelTask`、`InboundEmitter`、`ChannelAdapter`、`ChannelRegistry`、`assert_channel_contract`。
- Produces: `LoginSessionId`、`LoginSessionState`、`LoginSession`、`BeginLoginRequest`、`ChannelLoginAdapter`。

- [ ] **Step 1: 写多账号与 Unknown 契约测试**

```rust
#[tokio::test]
async fn contract_proves_account_isolation() {
    let adapter: Arc<dyn ChannelAdapter> = Arc::new(FakeChannel::new());
    assert_channel_contract(adapter).await;
}

#[tokio::test]
async fn timeout_maps_to_unknown_not_retryable() {
    let adapter = FakeChannel::unknown_on_send();
    let error = adapter
        .send(account("account-a"), text_message("hello"))
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::Unknown(_)));
}
```

`tests/login.rs` 追加：

```rust
#[tokio::test]
async fn login_adapter_rejects_empty_verification_code() {
    let adapter = FakeLoginChannel::default();
    let session = adapter
        .begin_login(BeginLoginRequest::new("account-a"))
        .await
        .unwrap();
    let error = adapter
        .submit_login_code(session.id(), " ")
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::Permanent(_)));
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-channel-sdk --test contract
cargo test -p agentnotify-channel-sdk --test login
```

Expected: FAIL，Channel SDK 类型未定义。

- [ ] **Step 3: 实现 Channel SDK**

```rust
pub type InboundEmitter = tokio::sync::mpsc::Sender<InboundMessage>;

#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn descriptor(&self) -> ChannelDescriptor;
    fn capabilities(&self) -> ChannelCapabilities;

    async fn start(
        &self,
        account: ChannelAccount,
        emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError>;

    async fn send(
        &self,
        account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError>;

    async fn inspect(&self, account: ChannelAccount) -> ChannelHealth;

    async fn logout(&self, account: ChannelAccount) -> Result<(), ChannelError>;
}
```

登录能力使用独立 trait，避免把二维码和配对码流程塞进普通发送接口：

```rust
#[async_trait::async_trait]
pub trait ChannelLoginAdapter: Send + Sync {
    async fn begin_login(
        &self,
        request: BeginLoginRequest,
    ) -> Result<LoginSession, ChannelError>;

    async fn submit_login_code(
        &self,
        session_id: &LoginSessionId,
        code: &str,
    ) -> Result<LoginSession, ChannelError>;

    async fn cancel_login(
        &self,
        session_id: &LoginSessionId,
    ) -> Result<(), ChannelError>;
}
```

`LoginSessionState` 固定为 `Preparing`、`QrReady`、`WaitingScan`、`NeedVerifyCode`、`WaitingFirstInbound`、`Paired`、`Expired`、`Blocked`、`Failed`。登录会话只存在内存，二维码不得写磁盘；确认登录后才把 secret 写入 `SecretStore`。

`ChannelError` 分为：

```rust
pub enum ChannelError {
    Permanent(SafeError),
    Retryable { error: SafeError, retry_after: Option<time::Duration> },
    Unknown(SafeError),
    InvalidAccount(SafeError),
    UnsupportedCapability(SafeError),
}
```

`ChannelTask` 包装 `tokio::task::JoinHandle<Result<(), ChannelError>>` 和取消句柄；`Drop` 不阻塞，runtime 负责显式取消并等待。每个账号实例必须独立保存游标、限流器和工作状态。

`assert_channel_contract` 必须验证：

- descriptor ID 唯一且与注册 ID 一致。
- 多账号发送不会共享游标、密钥或状态。
- `send_text == false` 时拒绝对外发送。
- `max_text_bytes` 超限时在调用网络前返回 `Permanent`。
- 超时或取消返回 `Unknown`，不是 `Retryable`。
- 成功回执的 `state` 可以是 `Sent`、显式 `Unknown` 或带安全原因的 `Skipped`；`Sent` 且声明 `reply_routing` 时必须有稳定 `external_message_id`。
- 声明支持登录的适配器必须拒绝空配对码，并且取消后不允许继续使用该 session ID。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-channel-sdk
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-channel-sdk
git commit -m "feat(channel-sdk): 增加渠道协议与多账号契约"
```

---

### Task 7: 定义应用端口、事务边界和内存测试工具

**Files:**
- Create: `crates/agentnotify-application/src/clock.rs`
- Create: `crates/agentnotify-application/src/ports/ingest_store.rs`
- Create: `crates/agentnotify-application/src/ports/delivery_store.rs`
- Create: `crates/agentnotify-application/src/ports/route_store.rs`
- Create: `crates/agentnotify-application/src/ports/channel_account_store.rs`
- Create: `crates/agentnotify-application/src/ports/secret_store.rs`
- Create: `crates/agentnotify-application/src/ports/claim_store.rs`
- Create: `crates/agentnotify-application/src/ports/status_store.rs`
- Create: `crates/agentnotify-application/src/ports/event_sink.rs`
- Create: `crates/agentnotify-application/src/error.rs`
- Modify: `crates/agentnotify-application/src/lib.rs`
- Create: `crates/agentnotify-testkit/src/clock.rs`
- Create: `crates/agentnotify-testkit/src/memory_store.rs`
- Create: `crates/agentnotify-testkit/src/fake_agent.rs`
- Create: `crates/agentnotify-testkit/src/fake_channel.rs`
- Modify: `crates/agentnotify-testkit/src/lib.rs`
- Test: `crates/agentnotify-testkit/tests/store_contract.rs`

**Interfaces:**
- Consumes: domain 与两个 SDK。
- Produces: `Clock`、`IngestStore`、`DeliveryStore`、`RouteStore`、`ClaimStore`、`StatusStore`、`EventSink`、`ApplicationError`、`FakeClock`、`MemoryStore`、`FakeAgent`、`FakeChannel`。
- Produces: `ChannelAccountStore`、`SecretStore`、`SecretKind`、`SecretValue` 和对应的内存测试实现。

- [ ] **Step 1: 写仓储契约测试**

```rust
#[tokio::test]
async fn ingest_store_commits_notification_and_outbox_together() {
    let store = MemoryStore::default();
    let notification = fixture_notification();
    let outbox = fixture_outbox(&notification);
    store
        .commit_ingest(notification.clone(), vec![outbox.clone()])
        .await
        .unwrap();

    assert_eq!(store.notification_count().await, 1);
    assert_eq!(store.outbox_count().await, 1);
    assert_eq!(store.notification(notification.id.clone()).await.unwrap(), notification);
}

#[tokio::test]
async fn claim_store_never_returns_acquired_twice() {
    let store = MemoryStore::default();
    let claim = fixture_claim();
    assert!(matches!(
        store.claim(claim.clone()).await.unwrap(),
        ClaimOutcome::Acquired(_)
    ));
    assert!(matches!(
        store.claim(claim).await.unwrap(),
        ClaimOutcome::AlreadyClaimed { .. }
    ));
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-testkit --test store_contract
```

Expected: FAIL，端口与内存实现未定义。

- [ ] **Step 3: 实现端口**

端口使用 `async_trait`，错误使用 `StoreError`，不得直接暴露 `rusqlite::Error`。核心签名：

```rust
#[async_trait::async_trait]
pub trait IngestStore: Send + Sync {
    async fn commit_ingest(
        &self,
        notification: Notification,
        outbox: Vec<OutboxItem>,
    ) -> Result<(), StoreError>;

    async fn notification_by_ingest_key(
        &self,
        agent_id: &AgentId,
        ingest_key: &str,
    ) -> Result<Option<Notification>, StoreError>;
}

#[async_trait::async_trait]
pub trait DeliveryStore: Send + Sync {
    async fn lease_next_outbox(&self, now: Timestamp, lease_until: Timestamp)
        -> Result<Option<OutboxLease>, StoreError>;

    async fn commit_delivery(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        route: Option<ReplyRoute>,
    ) -> Result<(), StoreError>;

    async fn reschedule_outbox(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        next_attempt_at: Timestamp,
    ) -> Result<(), StoreError>;
}

#[async_trait::async_trait]
pub trait RouteStore: Send + Sync {
    async fn find_route(
        &self,
        key: &RouteKey,
        now: Timestamp,
    ) -> Result<Option<ReplyRoute>, StoreError>;

    async fn insert_route(&self, route: ReplyRoute) -> Result<(), StoreError>;
}

#[async_trait::async_trait]
pub trait ChannelAccountStore: Send + Sync {
    async fn get(&self, account_id: &ChannelAccountId) -> Result<Option<ChannelAccount>, StoreError>;
    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError>;
    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError>;
    async fn set_enabled(&self, account_id: &ChannelAccountId, enabled: bool) -> Result<(), StoreError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretKind {
    BotToken,
    ContextToken,
    AppSecret,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretValue(String);

#[async_trait::async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, account_id: &ChannelAccountId, kind: SecretKind) -> Result<SecretValue, SecretError>;
    async fn set(&self, account_id: &ChannelAccountId, kind: SecretKind, value: SecretValue) -> Result<(), SecretError>;
    async fn delete(&self, account_id: &ChannelAccountId, kind: SecretKind) -> Result<(), SecretError>;
}
```

`ClaimStore::claim` 在一个原子操作中插入或返回已有状态；`StatusStore` 返回可序列化 DTO；`EventSink` 只发布脱敏后的状态快照。

- [ ] **Step 4: 实现内存测试工具**

`FakeClock` 支持 `advance(Duration)`；`MemoryStore` 使用 `tokio::sync::RwLock`，所有集合按 ID 排序；`FakeAgent` 可配置成功、失败、Unknown 和延迟；`FakeChannel` 可配置每账号回执、失败分类和入站消息序列。测试工具不得依赖 Windows、网络或真实时间。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-testkit --test store_contract
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 6: 提交**

```powershell
git add crates/agentnotify-application crates/agentnotify-testkit
git commit -m "feat(application): 定义端口与内存测试工具"
```

---

### Task 8: 建立 SQLite 初始迁移

**Files:**
- Create: `crates/agentnotify-storage-sqlite/migrations/0001_init.sql`
- Create: `crates/agentnotify-storage-sqlite/src/migrations.rs`
- Modify: `crates/agentnotify-storage-sqlite/src/lib.rs`
- Test: `crates/agentnotify-storage-sqlite/tests/migrations.rs`

**Interfaces:**
- Consumes: Task 7 的端口。
- Produces: `SqliteStore::open(path)`、`run_migrations(&mut Connection)`、`schema_version(&Connection)`。

- [ ] **Step 1: 写迁移与约束测试**

```rust
#[test]
fn migration_creates_required_tables_and_enables_wal() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = SqliteStore::open(temp.path().join("state.db")).unwrap();
    store.migrate().unwrap();

    assert_eq!(store.schema_version().unwrap(), 1);
    assert_eq!(store.journal_mode().unwrap(), "wal");
    for table in [
        "settings", "agent_configs", "channel_accounts", "notifications",
        "deliveries", "reply_routes", "inbound_claims", "outbox", "adapter_manifests",
    ] {
        assert!(store.table_exists(table).unwrap(), "缺少表 {table}");
    }
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-storage-sqlite --test migrations
```

Expected: FAIL，`SqliteStore::open` 不存在。

- [ ] **Step 3: 编写首个迁移**

`0001_init.sql` 必须完整创建以下核心内容：

```sql
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE agent_configs (
    agent_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE channel_accounts (
    account_id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL,
    secret_ref TEXT,
    cursor_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE notifications (
    notification_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    ingest_key TEXT NOT NULL,
    session_id TEXT,
    session_title TEXT,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (agent_id, ingest_key)
);

CREATE TABLE deliveries (
    delivery_id TEXT PRIMARY KEY,
    notification_id TEXT NOT NULL REFERENCES notifications(notification_id),
    channel_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('Pending','Sent','Failed','Unknown','Skipped')),
    external_message_id TEXT,
    error_code TEXT,
    error_message TEXT,
    retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (notification_id, channel_id, account_id)
);

CREATE TABLE reply_routes (
    channel_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    external_message_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (channel_id, account_id, external_message_id)
);

CREATE TABLE inbound_claims (
    claim_key TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    external_message_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('InProgress','Completed','Failed','Unknown')),
    error_code TEXT,
    error_message TEXT,
    received_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE outbox (
    outbox_id TEXT PRIMARY KEY,
    notification_id TEXT NOT NULL REFERENCES notifications(notification_id),
    state TEXT NOT NULL CHECK (state IN ('Pending','Leased','Done','Unknown','Dead')),
    available_at TEXT NOT NULL,
    lease_owner TEXT,
    lease_until TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE adapter_manifests (
    adapter_id TEXT PRIMARY KEY,
    adapter_kind TEXT NOT NULL CHECK (adapter_kind IN ('agent','channel')),
    version TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_notifications_occurred_at ON notifications(occurred_at DESC);
CREATE INDEX idx_deliveries_notification ON deliveries(notification_id);
CREATE INDEX idx_reply_routes_expires_at ON reply_routes(expires_at);
CREATE INDEX idx_inbound_claims_expires_at ON inbound_claims(expires_at);
CREATE INDEX idx_outbox_ready ON outbox(state, available_at, created_at);
```

`migrations.rs` 在单个事务中执行迁移，并保存 SQL 文件的 SHA-256 到 `schema_migrations`。若已应用版本的 checksum 与文件不一致，`migrate()` 返回 `StoreError::MigrationChecksumMismatch`，不得自动修复。

- [ ] **Step 4: 配置连接与测试通过**

`SqliteStore::open` 必须设置：

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
```

Run:

```powershell
cargo test -p agentnotify-storage-sqlite --test migrations
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，journal mode 为 `wal`。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-storage-sqlite
git commit -m "feat(storage): 增加 SQLite 初始迁移"
```

---

### Task 9: 实现 SQLite 仓储与原子事务

**Files:**
- Create: `crates/agentnotify-storage-sqlite/src/ingest_store.rs`
- Create: `crates/agentnotify-storage-sqlite/src/delivery_store.rs`
- Create: `crates/agentnotify-storage-sqlite/src/route_store.rs`
- Create: `crates/agentnotify-storage-sqlite/src/claim_store.rs`
- Create: `crates/agentnotify-storage-sqlite/src/status_store.rs`
- Modify: `crates/agentnotify-storage-sqlite/src/lib.rs`
- Test: `crates/agentnotify-storage-sqlite/tests/repositories.rs`

**Interfaces:**
- Consumes: Task 8 的 schema。
- Produces: 所有 application port 的 SQLite 实现，以及 `SqliteStore` 对 `IngestStore`、`DeliveryStore`、`RouteStore`、`ClaimStore`、`StatusStore` 的实现。

- [ ] **Step 1: 写事务与并发 Claim 测试**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_claim_allows_exactly_one_owner() {
    let store = Arc::new(fixture_store());
    let first = store.clone();
    let second = store.clone();
    let claim = fixture_claim();

    let (a, b) = tokio::join!(first.claim(claim.clone()), second.claim(claim));
    let acquired = [a.unwrap(), b.unwrap()]
        .into_iter()
        .filter(|outcome| matches!(outcome, ClaimOutcome::Acquired(_)))
        .count();
    assert_eq!(acquired, 1);
}

#[test]
fn sent_delivery_and_route_commit_in_one_transaction() {
    let store = fixture_store();
    commit_fixture_notification(&store);
    let lease = store.lease_next_outbox(now(), now_plus_seconds(30)).unwrap().unwrap();
    let delivery = sent_delivery(&lease);
    let route = fixture_route();
    store.commit_delivery(lease, delivery, Some(route.clone())).unwrap();

    assert_eq!(store.delivery(fixture_delivery_id()).unwrap().state(), DeliveryState::Sent);
    assert_eq!(store.find_route(&route.key, now()).unwrap().unwrap(), route);
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-storage-sqlite --test repositories
```

Expected: FAIL，仓储方法未实现。

- [ ] **Step 3: 实现同步 SQLite 与异步端口适配**

`rusqlite::Connection` 不跨线程共享。`SqliteStore` 持有 `Database` 调度器：后台专用线程拥有连接，命令通过有界 `tokio::sync::mpsc` 发送，响应通过 `oneshot` 返回。`open` 在返回前完成迁移。

事务规则：

- `commit_ingest`：`BEGIN IMMEDIATE`，写 `notifications` 与 `outbox`，任一步失败全部回滚。
- `lease_next_outbox`：选择 `Pending` 或可重试且 `available_at <= now` 的记录，以 `UPDATE ... WHERE` 抢租约；没有记录返回 `None`。
- `commit_delivery`：同一事务写 `deliveries`、更新 `outbox.state`，若投递为 `Sent` 且有 route，再写 `reply_routes`。
- `reschedule_outbox`：写失败 Delivery 和退避时间；`Unknown` 写终态并禁止重试。
- `claim`：`INSERT ... ON CONFLICT DO NOTHING` 后读取；插入成功才是 `Acquired`。
- `find_route`：必须同时带 `channel_id`、`account_id`、`external_message_id` 和 `expires_at > now`。
- 所有时间以 UTC RFC3339 毫秒写入。

不得在端口 trait 中暴露 `Connection`、`Transaction` 或 SQL 字符串。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-storage-sqlite --test repositories
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，并发 Claim 恰好一个成功。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-storage-sqlite
git commit -m "feat(storage): 实现事务型 SQLite 仓储"
```

---

### Task 10: 实现通知策略、IngestService 与事务型 Outbox

**Files:**
- Create: `crates/agentnotify-application/src/policy.rs`
- Create: `crates/agentnotify-application/src/ingest.rs`
- Modify: `crates/agentnotify-application/src/lib.rs`
- Test: `crates/agentnotify-application/tests/ingest.rs`

**Interfaces:**
- Consumes: Agent SDK、`IngestStore`、`Clock`、`IdGenerator`、通知策略设置。
- Produces: `PolicyDecision`、`SkipReason`、`IngestService::ingest(AgentEventEnvelope) -> Result<IngestResult, IngestError>`、`IngestResult::{Queued, Duplicate, Skipped}`。

- [ ] **Step 1: 写去重与策略测试**

```rust
#[tokio::test]
async fn duplicate_event_returns_original_notification_and_single_outbox() {
    let fixture = ingest_fixture();
    let first = fixture.service.ingest(fixture.envelope.clone()).await.unwrap();
    let second = fixture.service.ingest(fixture.envelope).await.unwrap();

    assert_eq!(first.notification_id(), second.notification_id());
    assert!(matches!(second, IngestResult::Duplicate { .. }));
    assert_eq!(fixture.store.outbox_count().await, 1);
}

#[tokio::test]
async fn quiet_hours_skip_produces_notification_without_outbox() {
    let fixture = ingest_fixture_at("2026-09-19T23:30:00+08:00");
    let result = fixture.service.ingest(fixture.envelope).await.unwrap();
    assert_eq!(result, IngestResult::Skipped { reason: SkipReason::QuietHours });
    assert_eq!(fixture.store.outbox_count().await, 0);
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-application --test ingest
```

Expected: FAIL，`IngestService` 不存在。

- [ ] **Step 3: 实现策略**

`NotificationPolicy::evaluate(&PolicyInput) -> PolicyDecision` 按顺序判断：

1. Agent 配置不存在时返回 `SkipReason::AgentNotConfigured`。
2. Agent 显式禁用时返回 `SkipReason::AgentDisabled`。
3. 标题含 `🔕` 或 `[勿扰]` 时返回 `SkipReason::TitleSuppressed`。
4. 当前时间落在跨午夜或同日的 quiet hours 时返回 `SkipReason::QuietHours`。
5. 同一 `AgentId + AgentSessionId` 在冷却窗口内返回 `SkipReason::Cooldown`。
6. 其他情况返回 `PolicyDecision::Deliver`。

策略是纯函数，接收 `now`、设置和最近通知时间，不读取系统时钟或数据库。

- [ ] **Step 4: 实现 IngestService**

固定流程：

1. 从 envelope 中取 `agent_id`。
2. `AgentRegistry::get`，缺失返回 `IngestError::AgentNotRegistered`。
3. 调用 `adapter.parse_event`。
4. 计算 ingest key：Agent 提供的 idempotency key 优先；缺失时使用 `request_id`。
5. 查询已有 `(agent_id, ingest_key)`；存在时直接返回 `Duplicate`。
6. 评估策略；跳过时仍写 Notification，状态元数据记录 skip reason，但不写 Outbox。
7. 交付时在一个事务中写 Notification 与 Outbox。
8. 发布脱敏 `EventSink::notification_changed(notification_id)`，再返回 `Queued`。

IngestService 不调用任何渠道，不执行网络请求。所有错误都必须带稳定 code 和中文 message。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-application --test ingest
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 6: 提交**

```powershell
git add crates/agentnotify-application
git commit -m "feat(application): 实现通知策略与事务型入站"
```

---

### Task 11: 实现投递 Worker、重试与 Unknown 语义

**Files:**
- Create: `crates/agentnotify-application/src/delivery.rs`
- Create: `crates/agentnotify-application/src/retry.rs`
- Modify: `crates/agentnotify-application/src/lib.rs`
- Test: `crates/agentnotify-application/tests/delivery.rs`

**Interfaces:**
- Consumes: `DeliveryStore`、`RouteStore`、`ChannelRegistry`、`Clock`、`IdGenerator`。
- Produces: `DeliveryService::process_next() -> Result<ProcessOutcome, DeliveryError>`、`RetryPolicy::next_attempt(attempt, kind, now) -> Option<Timestamp>`。

- [ ] **Step 1: 写重试边界测试**

```rust
#[test]
fn unknown_never_gets_next_attempt() {
    let policy = RetryPolicy::default();
    assert_eq!(
        policy.next_attempt(1, DeliveryErrorKind::Unknown, now()),
        None
    );
}

#[test]
fn retryable_uses_bounded_exponential_backoff() {
    let policy = RetryPolicy::default();
    let second = policy.next_attempt(1, DeliveryErrorKind::Retryable, now()).unwrap();
    let third = policy.next_attempt(2, DeliveryErrorKind::Retryable, now()).unwrap();
    assert!(third > second);
    assert!(third <= now().checked_add(time::Duration::minutes(5)).unwrap());
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-application --test delivery
```

Expected: FAIL，`RetryPolicy` 不存在。

- [ ] **Step 3: 实现重试策略**

默认最多尝试 8 次，基础退避 5 秒，指数上限 5 分钟，加入 0–20% 抖动。`Permanent`、`Unknown`、`Sent`、`Skipped` 均返回 `None`。`Retry-After` 存在时取“渠道建议值”和“指数退避值”的较大者，并受 15 分钟上限约束。

- [ ] **Step 4: 实现投递流程**

`process_next` 使用租约处理一条 Outbox：

1. 选择目标渠道账号。首个生产实现只选择一个显式启用的账号；多账号策略由后续独立任务加入。
2. 创建 Pending Delivery。
3. 调用 `ChannelAdapter.send`。
4. `Ok(receipt)`：
   - `Sent` 时写 Delivery；声明 `reply_routing` 时必须有 `external_message_id`，否则改判 `Unknown`。
   - 有稳定 ID 且 Agent 有 session ID 时，以 receipt 构造 `ReplyRoute`。
   - `Unknown` 时只写 Delivery 和 Outbox Unknown，不写 Route。
   - `Skipped` 时写 Delivery 和 Outbox Done，不写 Route；错误码保留为 `session_missing`、`account_disabled` 或渠道提供的安全原因。
5. `Err(Retryable)`：写 Failed Delivery 和新 `available_at`，释放租约。
6. `Err(Permanent)`：写 Failed Delivery，Outbox 置 Dead。
7. `Err(Unknown)`：写 Unknown Delivery，Outbox 置 Unknown。
8. 发布脱敏 `delivery_changed(delivery_id)` 事件。

渠道适配器 panic 不得带崩 worker：runtime 在任务边界捕获 JoinError，将该 Outbox 置 `Unknown`，不重试并写 host error 日志。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-application --test delivery
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 6: 提交**

```powershell
git add crates/agentnotify-application
git commit -m "feat(application): 实现可靠投递与未知结果语义"
```

---

### Task 12: 实现 ReplyService 与至多一次 Claim

**Files:**
- Create: `crates/agentnotify-application/src/reply.rs`
- Modify: `crates/agentnotify-application/src/lib.rs`
- Test: `crates/agentnotify-application/tests/reply.rs`

**Interfaces:**
- Consumes: `ClaimStore`、`RouteStore`、Agent Registry、Channel Registry、`Clock`。
- Produces: `ReplyService::handle(InboundMessage) -> Result<ReplyOutcome, ReplyError>`、`ReplyOutcome::{Accepted, AlreadyClaimed, Rejected}`。

- [ ] **Step 1: 写拒绝与 Claim 测试**

```rust
#[tokio::test]
async fn missing_message_id_does_not_fall_back_to_recent_route() {
    let fixture = reply_fixture_with_recent_route();
    let message = inbound_without_references();
    let result = fixture.service.handle(message).await.unwrap();
    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::NoExactRoute)
    ));
    assert_eq!(fixture.agent.resume_count().await, 0);
}

#[tokio::test]
async fn duplicate_inbound_is_not_submitted_twice() {
    let fixture = reply_fixture_with_route();
    let message = quoted_message();
    let first = fixture.service.handle(message.clone()).await.unwrap();
    let second = fixture.service.handle(message).await.unwrap();
    assert!(matches!(first, ReplyOutcome::Accepted { .. }));
    assert!(matches!(second, ReplyOutcome::AlreadyClaimed { .. }));
    assert_eq!(fixture.agent.resume_count().await, 1);
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-application --test reply
```

Expected: FAIL，`ReplyService` 不存在。

- [ ] **Step 3: 实现严格处理顺序**

流程固定为：

1. 校验账户存在并启用。
2. 校验发送者为该账号绑定发送者，且会话为允许的私聊模式。
3. 校验正文非空、长度符合渠道能力。
4. 生成 `ClaimKey` 并原子 Claim；已存在时返回 `AlreadyClaimed`，不调用 Agent。
5. 要求 `referenced_message_ids` 恰好包含一个可路由 ID；0 个或多个返回 `NoExactRoute` / `AmbiguousRoute`。
6. 用 `RouteKey` 精确查未过期路由。
7. 查 Agent Registry 和 `resume` 能力。
8. 调用 `AgentAdapter.resume`。
9. 将 Claim 更新为 `Completed`、`Failed` 或 `Unknown`。
10. 发布脱敏 `reply_changed` 事件。

`AgentError::Unknown` 写 Claim `Unknown`；绝不重试。成功回执只能表示“已接纳到 Agent 队列”，不能表示 Agent 已完成回答。

- [ ] **Step 4: 可选送达确认**

当设置为开启时，ReplyService 通过同一渠道适配器发送 `MessagePurpose::ReplyConfirmation`。确认发送失败不改变已成功的 Claim，只写 tracing 和 StatusStore 的最近错误。确认消息不得写入 ReplyRoute。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-application --test reply
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，重复消息只调用 Agent 一次。

- [ ] **Step 6: 提交**

```powershell
git add crates/agentnotify-application
git commit -m "feat(application): 实现精确引用回复与至多一次 Claim"
```

---

### Task 13: 实现 StatusService、runtime 监督与优雅关闭

**Files:**
- Create: `crates/agentnotify-application/src/status.rs`
- Create: `crates/agentnotify-runtime/src/event_bus.rs`
- Create: `crates/agentnotify-runtime/src/runtime.rs`
- Create: `crates/agentnotify-runtime/src/supervisor.rs`
- Create: `crates/agentnotify-runtime/src/telemetry.rs`
- Modify: `crates/agentnotify-runtime/src/lib.rs`
- Test: `crates/agentnotify-runtime/tests/runtime.rs`
- Test: `crates/agentnotify-runtime/tests/telemetry.rs`

**Interfaces:**
- Consumes: 全部 application 服务、SQLite Store、Agent Registry、Channel Registry。
- Produces: `AppRuntime::start(config) -> Result<RuntimeHandle, RuntimeError>`、`RuntimeHandle::shutdown()`、`RuntimeSnapshot`、`StatusService::snapshot()`。
- Produces: `init_telemetry`、脱敏 tracing layer，以及 `ingest`、`delivery`、`reply`、`host` span 约定。

- [ ] **Step 1: 写启动、故障隔离与关闭测试**

```rust
#[tokio::test]
async fn one_channel_failure_does_not_stop_other_tasks() {
    let fixture = runtime_fixture();
    let handle = fixture.runtime.start().await.unwrap();
    fixture.failing_channel.fail_next_start().await;
    fixture.healthy_channel.wait_started().await;

    assert!(handle.is_running());
    assert!(fixture.status.snapshot().await.channels.failed_or_unknown() >= 1);
}

#[tokio::test]
async fn shutdown_waits_for_outbox_checkpoint() {
    let fixture = runtime_fixture();
    let handle = fixture.runtime.start().await.unwrap();
    handle.shutdown().await.unwrap();
    assert!(fixture.store.integrity_check().await.unwrap());
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-runtime --test runtime
```

Expected: FAIL，runtime 不存在。

- [ ] **Step 3: 实现 runtime 装配**

`AppRuntime::start` 顺序：

1. 打开并迁移 SQLite。
2. 创建 `SecretStore`、Clock、IdGenerator、EventBus。
3. 注入 Agent 与 Channel Registry。
4. 启动每个启用渠道账号的 `ChannelAdapter::start`。
5. 启动 Outbox worker、Inbound consumer 和 Status refresher。
6. 返回 `RuntimeHandle`。

每个后台任务由 supervisor 监督。单任务 panic 或明确失败只标记该组件状态，不取消其他渠道；数据库损坏或 SecretStore 不可用属于致命错误，停止 runtime 并返回中文错误。

- [ ] **Step 4: 实现状态快照与关闭**

`RuntimeSnapshot` 只包含稳定 DTO：应用版本、平台、runtime 状态、Agent 状态列表、渠道账号状态列表、最近投递摘要和诊断项。不得包含密钥、token、完整 prompt、Agent 正文或渠道原始响应。

关闭顺序：

1. 停止接收新的 ingress。
2. 停止领取 Outbox。
3. 取消渠道长连接并等待任务退出。
4. 等待正在执行的 Reply Claim 结束或标记 `Unknown`。
5. SQLite `PRAGMA wal_checkpoint(TRUNCATE)`。
6. 发布 `runtime.stopped` 后释放资源。

`telemetry.rs` 初始化 `tracing`，日志写入应用日志目录并经过统一脱敏层。必须提供稳定 span：

- `ingest`：agent、event hash、session hash、结果。
- `delivery`：notification、channel、account、delivery、状态和尝试次数。
- `reply`：intent hash、路由结果、agent、最终 Claim 状态。
- `host`：平台、宿主版本、IPC 或后台任务状态。

脱敏层递归替换 `token`、`secret`、`authorization`、`cookie`、`context_token` 和消息正文字段。单元测试用敏感夹具写入日志，断言磁盘文件中不存在原值。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-runtime --test runtime
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，关闭后数据库完整性检查通过。

- [ ] **Step 6: 提交**

```powershell
git add crates/agentnotify-application crates/agentnotify-runtime
git commit -m "feat(runtime): 增加状态快照与后台任务监督"
```

---

### Task 14: 定义内部事件协议与持久化 spool

**Files:**
- Create: `apps/ingress/Cargo.toml`
- Create: `apps/ingress/src/main.rs`
- Create: `apps/ingress/src/protocol.rs`
- Create: `apps/ingress/src/spool.rs`
- Create: `crates/agentnotify-runtime/src/ingress.rs`
- Modify: `crates/agentnotify-runtime/src/lib.rs`
- Modify: `Cargo.toml`
- Test: `apps/ingress/tests/protocol.rs`
- Test: `apps/ingress/tests/spool.rs`

**Interfaces:**
- Consumes: `AgentEventEnvelope` 与 `RuntimeHandle`。
- Produces: `agentnotify-ingress.exe`、`IngressEvent::parse(bytes) -> Result<AgentEventEnvelope, IngressError>`、`Spool::write_event(&AgentEventEnvelope)`、`Spool::drain_batch(limit)`。

- [ ] **Step 1: 写协议和 spool 测试**

```rust
#[test]
fn protocol_rejects_unknown_version_and_kind() {
    let unknown_version = br#"{"protocolVersion":2,"kind":"agent.event","requestId":"r1","agentId":"opencode","payload":{}}"#;
    assert!(matches!(
        IngressEvent::parse(unknown_version),
        Err(IngressError::UnsupportedVersion)
    ));

    let unknown_kind = br#"{"protocolVersion":1,"kind":"status.query","requestId":"r1","agentId":"opencode","payload":{}}"#;
    assert!(matches!(
        IngressEvent::parse(unknown_kind),
        Err(IngressError::UnsupportedKind)
    ));
}

#[test]
fn spool_write_is_atomic_and_bounded() {
    let spool = Spool::open(tempdir().path(), SpoolLimits::default()).unwrap();
    spool.write_event(&fixture_event("request-1")).unwrap();
    assert_eq!(spool.queued_count().unwrap(), 1);
    assert!(spool.path_for("request-1").unwrap().exists());
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-ingress --test protocol --test spool
```

Expected: FAIL，ingress 包不存在。

- [ ] **Step 3: 实现协议**

输入 JSON：

```json
{
  "protocolVersion": 1,
  "kind": "agent.event",
  "requestId": "75fe53aa-2314-4c21-b12e-773efce521d9",
  "agentId": "opencode",
  "payload": {
    "eventType": "session.completed",
    "idempotencyKey": "opencode:session-1:event-9",
    "occurredAt": "2026-09-19T10:20:30.123+08:00",
    "sessionId": "session-1",
    "title": "构建完成",
    "body": "Release 已生成",
    "metadata": {}
  }
}
```

解析限制：

- body 最大 64 KiB。
- payload 最大 256 KiB。
- 只接受 `protocolVersion == 1` 和 `kind == "agent.event"`。
- requestId 必须是 UUID。
- `agentId` 必须是非空强类型 ID。
- payload 必须是对象；未知字段保留给适配器，不在入口层解释。
- 协议错误退出码为 `2`，spool 容量错误为 `3`，IPC 不可达并成功入 spool 为 `0`。

- [ ] **Step 4: 实现 spool**

默认路径由宿主注入的 `AppPaths.spool_dir` 决定。写入流程：

1. 创建 `spool/<epoch_millis>-<requestId>.json.tmp`。
2. 写入 JSON、flush、`sync_all`。
3. 原子重命名为 `.json`。
4. 单事件超过 256 KiB 或队列超过 10,000 条 / 64 MiB 时删除临时文件并返回容量错误。
5. 超过 7 天的文件由 core 启动清理；不能由 ingress 递归删除整个目录。
6. 相同 requestId 已存在时视为成功，不重复写。

core 启动顺序必须在渠道启动前先 drain spool。成功 ingest 后删除文件；永久无效事件移动到 `spool/quarantine/` 并保留错误原因；临时数据库不可用时不删除。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-ingress --test protocol --test spool
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS。

- [ ] **Step 6: 提交**

```powershell
git add Cargo.toml apps/ingress crates/agentnotify-runtime
git commit -m "feat(ingress): 增加内部事件协议与持久化 spool"
```

---

### Task 15: 实现 Windows 命名管道与最小入口进程

**Files:**
- Create: `crates/agentnotify-runtime/src/platform/local_ipc.rs`
- Create: `crates/agentnotify-runtime/src/platform/windows_pipe.rs`
- Modify: `crates/agentnotify-runtime/src/lib.rs`
- Modify: `apps/ingress/src/main.rs`
- Test: `crates/agentnotify-runtime/tests/windows_pipe.rs`
- Test: `apps/ingress/tests/ipc_fallback.rs`

**Interfaces:**
- Consumes: Task 14 的协议与 spool。
- Produces: `LocalIpc::{serve, connect}` 的 Windows 实现、管道名生成器和当前用户 ACL。

- [ ] **Step 1: 写管道名、ACL 与降级测试**

```rust
#[cfg(windows)]
#[test]
fn pipe_name_is_stable_for_current_user_and_has_no_sid_text() {
    let first = windows_pipe_name().unwrap();
    let second = windows_pipe_name().unwrap();
    assert_eq!(first, second);
    assert!(first.starts_with(r"\\.\pipe\agentnotify-v1-"));
    assert!(!first.contains("S-1-5-"));
}

#[cfg(windows)]
#[tokio::test]
async fn client_spools_when_pipe_is_unavailable() {
    let result = submit_with_fallback(
        fixture_event(),
        r"\\.\pipe\agentnotify-v1-does-not-exist",
        fixture_spool(),
        Duration::from_millis(150),
    )
    .await;
    assert_eq!(result, SubmitResult::Spooled);
    assert_eq!(fixture_spool().queued_count().unwrap(), 1);
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-runtime --test windows_pipe
cargo test -p agentnotify-ingress --test ipc_fallback
```

Expected: FAIL，Windows IPC 未实现。

- [ ] **Step 3: 实现当前用户命名管道**

- 管道名：`\\.\pipe\agentnotify-v1-<sha256(current_user_sid)[0..16]>`。
- 服务端：`PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS`，单消息最大 256 KiB。
- ACL 使用当前用户 SID 的 SDDL，只授予当前用户 `GA`；不得使用 `Everyone` 或 `Authenticated Users`。
- 服务端最多同时接受 8 个连接，每次连接只接收一个 UTF-8 JSON 消息。
- 验证消息 UTF-8、协议版本、kind 和大小后才提交 `IngestService`。
- 解析错误返回机器错误码；不能把原始恶意输入写进日志。
- 服务端不接收 shell、文件路径、管理动作或其他 RPC 方法。

- [ ] **Step 4: 实现 ingress 快速降级**

`agentnotify-ingress.exe` 只执行：

1. 从 stdin 读取一个有界 JSON 对象。
2. 解析并验证协议。
3. 在 150 毫秒内连接当前用户管道。
4. 连接成功时只写入一条消息，等待 core 写入确认后退出 `0`。
5. 连接失败时原子写 spool 并退出 `0`。
6. 输入协议错误退出 `2`；spool 超过限制退出 `3`。
7. 全程不分配控制台窗口，不实现帮助页和状态查询子命令。

- [ ] **Step 5: 运行测试并确认通过**

Run:

```powershell
cargo test -p agentnotify-runtime --test windows_pipe
cargo test -p agentnotify-ingress --test ipc_fallback
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
```

Expected: PASS，管道 ACL 测试确认没有远程或全局用户权限。

- [ ] **Step 6: 提交**

```powershell
git add apps/ingress crates/agentnotify-runtime
git commit -m "feat(ingress): 增加当前用户 Windows 命名管道"
```

---

### Task 16: 完成无 UI 端到端验收与恢复测试

**Files:**
- Create: `crates/agentnotify-testkit/tests/full_flow.rs`
- Create: `crates/agentnotify-testkit/tests/restart_recovery.rs`
- Create: `crates/agentnotify-testkit/tests/duplicate_delivery.rs`
- Create: `docs/superpowers/specs/2026-09-19-rust-core-acceptance.md`

**Interfaces:**
- Consumes: 前 15 个任务全部接口。
- Produces: 阶段 A 验收证据、测试命令和已知限制。

- [ ] **Step 1: 写完整假渠道闭环测试**

```rust
#[tokio::test]
async fn fake_agent_to_fake_channel_creates_exact_reply_route() {
    let fixture = full_flow_fixture().await;
    let ingest = fixture.ingest_event("opencode", "session-1", "任务完成").await.unwrap();
    fixture.runtime.deliver_all().await.unwrap();

    let receipt = fixture.channel.last_receipt().await.unwrap();
    assert_eq!(receipt.external_message_id.as_ref().unwrap().as_str(), "external-1");

    let outcome = fixture
        .reply_service
        .handle(fixture.quoted_message("external-1", "继续处理"))
        .await
        .unwrap();
    assert!(matches!(outcome, ReplyOutcome::Accepted { .. }));
    assert_eq!(fixture.agent.resumed_session().await.unwrap().as_str(), "session-1");
}
```

- [ ] **Step 2: 写崩溃、重启和未知结果测试**

测试必须覆盖：

- 写入 Notification 和 Outbox 后、网络调用前退出；重启后仍只投递一次。
- 渠道返回 `Unknown` 后重启；重启不重新领取该 Outbox。
- Claim 已写 `InProgress` 后退出；重启后标记 `Unknown`，不再次 resume。
- 重复 ingress 事件和重复引用消息不产生第二次副作用。
- SQLite 损坏或迁移 checksum 不匹配时 runtime 启动失败，并给出中文诊断。

- [ ] **Step 3: 运行全部核心门禁**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
cargo test -p agentnotify-testkit --test full_flow --test restart_recovery --test duplicate_delivery
```

Expected: 全部 PASS；测试输出中没有 token、正文快照或明文凭据。

- [ ] **Step 4: 记录验收结果**

在 `docs/superpowers/specs/2026-09-19-rust-core-acceptance.md` 记录：

- 使用的 Rust 与 Cargo 版本。
- 测试命令、退出码和测试数量。
- 数据库迁移版本与 checksum。
- 假 Agent、假渠道闭环结果。
- 重启、Unknown、重复事件和并发 Claim 结果。
- 尚未实现的内容：Tauri 宿主、React UI、OpenCode、ClawBot、旧数据迁移和生产安装器。

不得把“测试通过”描述为真实微信或真实 Agent 已验收。

- [ ] **Step 5: 提交**

```powershell
git add crates/agentnotify-testkit docs/superpowers/specs/2026-09-19-rust-core-acceptance.md
git commit -m "test(core): 完成无 UI 端到端与恢复验收"
```

---

## 阶段完成标准

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --all-features` 全部通过。
- `agentnotify-ingress.exe` 只接受 `protocolVersion=1` 的 `agent.event`，离线时能写 spool，恢复后只提交一次。
- 假 Agent + 假渠道可以完成推送、建立精确 Route、处理引用回复并记录 Claim。
- Unknown、崩溃和重复投递不会自动重放。
- SQLite 包含所有核心表，迁移 checksum 不匹配会显式失败。
- 核心、应用、SDK 和 Windows IPC 模块没有 Agent 或渠道专属分支。
- 尚未进入 UI、真实适配器、迁移和发布切换；这些内容由后续两份执行计划处理。
