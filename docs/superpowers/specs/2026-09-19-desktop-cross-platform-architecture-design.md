# AgentNotify 桌面端跨平台与可扩展架构设计

## 状态

待评审的目标架构设计。本文定义从零构建时的目标形态，不直接修改当前 Go 实现。

## 背景

AgentNotify 的核心功能是接收不同 Agent 的任务完成消息，通过不同消息渠道推送，并允许用户在原消息上回复，把内容精确送回到对应 Agent 会话。

当前产品首先服务 Windows，后续需要扩展到 macOS 和 HarmonyOS PC。消息渠道需要扩展到飞书等平台，Agent 接入也需要持续增加。现有 Win32 自绘 UI、固定 Agent 列表和与 ClawBot 直接耦合的发送链路不适合作为长期扩展基础。

## 目标

1. 以 Windows 为第一阶段交付平台，同时保证核心代码不依赖 Windows。
2. 后续用同一套 Rust 核心和 React UI 扩展到 macOS 与 HarmonyOS PC。
3. 新 Agent 通过适配器接入，不让 UI、历史、路由和状态代码增加 Agent 专属分支。
4. 新渠道通过适配器接入，支持不同认证、发送能力、入站模式和回复规则。
5. 推送与引用回复使用同一套可靠投递、路由和去重模型。
6. 桌面应用保持小型、常驻、低资源占用，不引入 Electron、Chromium 或 Node 运行时。
7. 保留 CLI 与 Agent Hook 的快速、无界面调用方式。
8. 用户配置、登录状态、历史、路由和密钥在平台迁移后保持明确、安全、可测试。

## 非目标

1. 本方案不把产品拆成微服务，也不提供多租户云端后台。
2. 本方案不要求首版同时发布 Windows、macOS、HarmonyOS PC。
3. 本方案不依赖 Tauri 的实验性 HarmonyOS 支持作为 Windows 首版的前置条件。
4. 本方案不采用 Rust 动态库作为第三方插件 ABI。
5. 本方案不允许 UI 直接读写配置、密钥、数据库或执行系统命令。
6. 本方案不允许引用回复退化为“最近会话”、标题或正文匹配。

## 核心决策

| 领域 | 决策 |
|---|---|
| 应用形态 | 模块化单体，一个常驻桌面进程，窗口关闭后隐藏到托盘或菜单栏 |
| 业务核心 | Rust 工作区，核心不依赖 Tauri、WebView、数据库或具体操作系统 |
| 桌面 UI | React + TypeScript，构建为一个共享 Web 资源包 |
| Windows/macOS 宿主 | Tauri 稳定版，分别承载 WebView2 与 WKWebView |
| HarmonyOS PC 宿主 | 独立 ArkWeb/ArkTS 宿主，通过 NAPI 连接 Rust 核心 |
| HarmonyOS 收敛路径 | Tauri OHOS 达到稳定发布条件后，可替换宿主，不改核心和共享 UI |
| 本地状态 | SQLite WAL + 版本化迁移 |
| 密钥 | Windows Credential Manager/DPAPI 与 macOS Keychain，HarmonyOS 使用系统安全存储 |
| Agent 扩展 | Rust 内置适配器注册表 + 受控外部适配器进程协议 |
| 渠道扩展 | 渠道工厂与账号实例注册表，能力由描述符声明 |
| UI 通信 | 类型化命令与事件，Web 端只依赖 HostBridge |
| 可靠投递 | 事务型 outbox、幂等键、未知结果不自动重发 |
| 外部集成 | Windows/macOS 使用 CLI、命名管道和 Unix Socket；HarmonyOS 使用 Ability/IPC 适配 |

## 目标仓库结构

```text
Cargo.toml
crates/
  agentnotify-domain/          领域模型、值对象、纯规则
  agentnotify-application/     用例、事务边界、端口定义
  agentnotify-runtime/         任务监督、事件总线、管道编排
  agentnotify-storage-sqlite/  SQLite 仓库实现
  agentnotify-agent-sdk/        Agent 适配器协议与注册表
  agentnotify-channel-sdk/      渠道适配器协议与注册表
  agentnotify-agent-*/         内置 Agent 适配器
  agentnotify-channel-*/       内置渠道适配器
  agentnotify-testkit/         契约测试、假时钟、假渠道、夹具

hosts/
  desktop-tauri/               Windows、macOS 桌面宿主
  harmony-pc/                  HarmonyOS PC ArkWeb/ArkTS 宿主

apps/
  cli/                         Hook、CLI、本地 IPC 客户端
  desktop-ui/                  React + TypeScript 共享 UI

docs/
  superpowers/
  architecture/
```

目录按稳定边界拆分，不按页面拆 crate。只有出现独立依赖、独立测试或独立发布需求时，才新增 crate。

## 分层与依赖方向

```text
domain <- application <- runtime <- hosts
                    \             \
                     storage       adapters
```

依赖规则：

1. `domain` 不依赖任何 I/O、Tauri、SQLite、HTTP 或操作系统 API。
2. `application` 只依赖 `domain` 与端口 trait，不依赖具体适配器。
3. `storage`、Agent 适配器、渠道适配器实现 `application` 定义的端口。
4. `runtime` 负责依赖装配、任务生命周期、重试、取消和事件分发。
5. `hosts` 只负责平台能力、窗口、权限、UI 桥接和进程入口。
6. React UI 不得导入 Rust 实现细节，只能通过版本化 HostBridge 通信。

## 领域模型

### 标识

所有标识使用强类型，避免跨账号、跨渠道误用：

```rust
pub struct AgentId(String);
pub struct AgentSessionId(String);
pub struct ChannelId(String);
pub struct ChannelAccountId(String);
pub struct ExternalMessageId(String);
pub struct NotificationId(String);
pub struct DeliveryId(String);
pub struct InboundMessageId(String);
```

### 通知

```rust
pub struct Notification {
    pub id: NotificationId,
    pub agent_id: AgentId,
    pub session_id: Option<AgentSessionId>,
    pub session_title: Option<String>,
    pub title: String,
    pub body: String,
    pub occurred_at: Timestamp,
    pub metadata: NotificationMetadata,
}
```

### 投递

一次通知可以投递到多个渠道。每次渠道发送都有独立记录：

```rust
pub enum DeliveryState {
    Pending,
    Sent,
    Failed,
    Unknown,
    Skipped,
}

pub struct Delivery {
    pub id: DeliveryId,
    pub notification_id: NotificationId,
    pub channel_id: ChannelId,
    pub account_id: ChannelAccountId,
    pub state: DeliveryState,
    pub external_message_id: Option<ExternalMessageId>,
    pub error: Option<SafeError>,
}
```

`Unknown` 表示调用超时、进程中断或渠道返回无法确认。该状态不能自动重发。

### 回复路由

路由必须以渠道、账号和渠道路由键共同定位：

```rust
pub struct RouteKey {
    pub channel_id: ChannelId,
    pub account_id: ChannelAccountId,
    pub external_message_id: ExternalMessageId,
}

pub struct ReplyRoute {
    pub key: RouteKey,
    pub agent_id: AgentId,
    pub session_id: AgentSessionId,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}
```

同一个平台消息 ID 出现在不同账号时不能互相匹配。

### 入站与去重

```rust
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
}
```

入站消息在分发前先持久化 Claim。消息 ID 缺失时使用渠道、账号、游标、引用 ID 和正文哈希生成确定性回退键。Claim 默认只允许一次成功分发。

## Agent 扩展模型

### 描述符

```rust
pub struct AgentDescriptor {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub config_schema: JsonSchema,
}

pub struct AgentCapabilities {
    pub notify: bool,
    pub resume: bool,
    pub session_title: bool,
    pub hook_installer: bool,
    pub reply_window: bool,
}
```

UI、状态页与 CLI 只读取描述符和能力，不维护 Agent 名称枚举。

### 适配器

```rust
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    fn descriptor(&self) -> AgentDescriptor;
    fn capabilities(&self) -> AgentCapabilities;

    fn parse_event(&self, envelope: AgentEventEnvelope)
        -> Result<NormalizedAgentEvent, AgentError>;

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError>;

    async fn inspect(&self) -> AgentHealth;
}
```

Hook 安装、配置检查、标题解析等差异由 Agent 适配器或其独立 installer 组件负责。应用层只处理标准 `NormalizedAgentEvent`。

### 注册

```rust
pub struct AgentRegistry {
    adapters: HashMap<AgentId, Arc<dyn AgentAdapter>>,
}

impl AgentRegistry {
    pub fn register(&mut self, adapter: Arc<dyn AgentAdapter>) -> Result<(), RegistryError>;
    pub fn get(&self, id: &AgentId) -> Option<Arc<dyn AgentAdapter>>;
    pub fn all(&self) -> Vec<Arc<dyn AgentAdapter>>;
}
```

新增内置 Agent 的固定流程：

1. 新增适配器 crate。
2. 实现 `AgentAdapter`。
3. 注册到 runtime 的静态注册表。
4. 增加契约测试和真实链路验收。
5. UI 自动出现该 Agent，无需修改页面代码。

### 外部 Agent 适配器

需要兼容第三方 Agent 时，使用独立进程协议：

- 传输使用标准输入输出 JSON-RPC，或平台本地 IPC。
- 每个适配器提供版本化 manifest。
- manifest 声明 ID、协议版本、能力和入口。
- 外部适配器不得直接访问应用数据库和密钥库。
- 外部适配器只能返回标准事件和接收 resume 请求。
- 不支持 Rust 动态库 ABI。
- HarmonyOS PC 若不能启动普通子进程，则外部适配器只能通过 Ability 或平台扩展提供。

## 渠道扩展模型

### 描述符与能力

```rust
pub struct ChannelDescriptor {
    pub id: ChannelId,
    pub display_name: String,
    pub config_schema: JsonSchema,
}

pub struct ChannelCapabilities {
    pub send_text: bool,
    pub receive: bool,
    pub reply_routing: bool,
    pub edit_message: bool,
    pub attachments: bool,
    pub markdown: bool,
    pub max_text_bytes: Option<usize>,
    pub inbound_modes: Vec<InboundMode>,
}
```

`InboundMode` 覆盖长轮询、WebSocket、Webhook、本地事件等模式。UI 根据描述符动态生成渠道设置入口，不增加 ClawBot 或飞书的硬编码页面。

### 适配器

```rust
#[async_trait]
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

    async fn inspect(
        &self,
        account: ChannelAccount,
    ) -> ChannelHealth;

    async fn logout(
        &self,
        account: ChannelAccount,
    ) -> Result<(), ChannelError>;
}
```

渠道账号必须是独立实体。一个渠道可以同时配置多个账号，每个账号有独立密钥、游标、状态和限流器。

`DeliveryReceipt` 返回渠道自己定义的不可变路由信息：

```rust
pub struct DeliveryReceipt {
    pub external_message_id: Option<ExternalMessageId>,
    pub external_thread_id: Option<String>,
    pub state: DeliveryState,
    pub raw_safe_metadata: Map<String, String>,
}
```

应用层不解析渠道原始响应结构，只保存安全的标准结果。

### 飞书首批渠道约束

飞书适配器需要同时支持：

- 应用凭证与 tenant access token 生命周期。
- 出站消息发送和消息 ID 回执。
- 入站事件订阅或长连接。
- 事件去重、签名或回调验证。
- 富文本和普通文本能力差异。
- 私聊、群聊和消息引用结构归一化。
- 多账号与不同租户隔离。

飞书实现只新增 `agentnotify-channel-feishu`，不修改通知服务、回复服务或历史 UI。

## 应用服务

### AgentIngestService

职责：

1. 接收 Agent Hook、CLI 或外部适配器事件。
2. 解析并标准化事件。
3. 校验 Agent 开关、勿扰时段、去重窗口和协议块。
4. 创建 Notification。
5. 在事务中写入 Notification 与 Outbox。
6. 返回可机器读取的 `IngestResult`。

该服务不发送网络请求。

### NotificationService

职责：

1. 读取待处理 Outbox。
2. 根据策略选择目标渠道和账号。
3. 为每个目标创建 Delivery。
4. 调用 ChannelAdapter.send。
5. 保存成功、失败或未知结果。
6. 成功且有稳定会话 ID 时写入 ReplyRoute。

### ReplyService

职责：

1. 接收标准 InboundMessage。
2. 验证账号和发送者。
3. 持久化 Claim。
4. 校验引用消息 ID 和正文。
5. 精确查询 ReplyRoute。
6. 调用 AgentAdapter.resume。
7. 标记 Claim 最终状态并可选发送送达确认。

任何一步不能确认目标时都返回用户可读错误，不回退到最近会话。

### StatusService

统一生成以下模型：

- Agent 接入状态。
- 渠道账号状态。
- 凭据和会话状态。
- 最近投递状态。
- 可执行的修复动作。

CLI、桌面 UI 和诊断页读取同一个模型，不各自拼接状态。

## 本地运行时与进程模型

### 桌面进程

Windows/macOS 首版使用一个常驻进程：

- Tauri 窗口显示时提供完整 UI。
- 窗口关闭只隐藏，不结束 Agent 与渠道任务。
- 托盘或菜单栏提供显示窗口、暂停、退出。
- 开机或登录时自动启动。
- runtime 持有 Agent Registry、Channel Registry、数据库连接和任务监督器。
- HarmonyOS 若受 Ability 生命周期约束，可以拆为 UI Ability 与后台扩展，但它们共享同一套核心协议和唯一状态源。

### CLI 与 Hook

CLI 是轻量客户端，不复制业务逻辑。Windows/macOS 使用：

- Windows 命名管道。
- macOS Unix Domain Socket。

本地协议使用带版本号的 JSON 消息：

```json
{
  "protocolVersion": 1,
  "kind": "agent.event",
  "requestId": "uuid",
  "payload": {}
}
```

运行中的桌面核心在线时，CLI 直接提交事件。核心未运行时，CLI 把事件写入持久化 spool 后立即退出；下一次启动时核心消费 spool。这样 Agent Hook 不会因 UI 未启动或数据库锁而阻塞。

### HarmonyOS PC

HarmonyOS 宿主不能假设存在同名可执行文件。平台适配需要提供：

- Ability 或扩展进程作为事件入口。
- ArkTS HostBridge 与 NAPI 连接 Rust 核心。
- 后台任务或 ServiceExtension 维持渠道长连接。
- 系统安全存储保存凭据。
- 平台允许的文件目录和数据库路径。
- 不含 Windows/macOS 路径推断逻辑。

若 Tauri OHOS 在目标发布时间前达到稳定版本，则优先把 `hosts/harmony-pc` 实现为 Tauri 宿主；否则使用 ArkWeb/ArkTS 宿主。两种实现的共享 UI 和 Rust 核心不变。

## 跨平台宿主协议

各平台宿主实现同一个 `PlatformHost`：

```rust
#[async_trait]
pub trait PlatformHost: Send + Sync {
    fn paths(&self) -> AppPaths;
    fn secret_store(&self) -> Arc<dyn SecretStore>;
    fn process_runner(&self) -> Arc<dyn ProcessRunner>;
    fn background_tasks(&self) -> Arc<dyn BackgroundTasks>;
    fn local_ipc(&self) -> Arc<dyn LocalIpc>;
    fn system_ui(&self) -> Arc<dyn SystemUi>;
}
```

平台差异只允许出现在 `hosts` 和对应 `PlatformHost` 实现中。`domain`、`application` 和适配器代码不能使用 `cfg(windows)` 分支判断业务行为。

React 使用统一桥接接口：

```ts
export interface HostBridge {
  invoke<TCommand, TResult>(
    command: TCommand,
    payload: unknown
  ): Promise<TResult>

  subscribe<TEvent>(
    event: TEvent,
    handler: (payload: unknown) => void
  ): () => void
}
```

Windows/macOS 由 Tauri 实现，HarmonyOS 由 ArkWeb/ArkTS 实现。

## UI 架构

### 技术栈

- React。
- TypeScript。
- Vite。
- TanStack Query 管理异步数据。
- React Router 管理桌面导航。
- 轻量本地状态只处理窗口、抽屉和表单交互。
- 设计令牌统一颜色、间距、字号、圆角和状态色。

不使用全局万能 store，不把服务端状态复制到多个状态容器。

### 信息架构

1. 总览：连接状态、Agent 状态、渠道状态、最近投递、待处理故障。
2. Agents：Agent 列表、能力、开关、接入状态、配置与修复。
3. Channels：渠道账号列表、登录、会话、限流、启停和测试发送。
4. History：Notification 与 Delivery 的筛选、详情、错误和重试状态。
5. Diagnostics：网络、凭据、IPC、数据库、后台任务和版本检查。
6. Settings：勿扰、冷却、默认渠道、更新和界面偏好。

### UI 规则

- 页面只消费标准 DTO 和 descriptor。
- 不允许通过 `if agentId === "codex"` 控制业务展示。
- 不允许通过 `if channelId === "clawbot"` 控制业务展示。
- 渠道和 Agent 专属配置由 schema 驱动。
- 所有错误文案说明用户下一步能做什么。
- 状态颜色只表达 `正常 / 等待 / 异常 / 暂停`，不使用同一颜色表达两种语义。
- 列表、表格和详情用于高频工作流，不用营销式大卡片布局。

## 数据与事务

### SQLite 表

首版需要以下核心表：

| 表 | 用途 |
|---|---|
| `schema_migrations` | 数据库版本 |
| `settings` | 非敏感应用设置 |
| `agent_configs` | Agent 启停和适配器配置 |
| `channel_accounts` | 渠道账号元数据与密钥引用 |
| `notifications` | 标准通知记录 |
| `deliveries` | 每个渠道的投递结果 |
| `reply_routes` | 出站消息到 Agent 会话的精确路由 |
| `inbound_claims` | 入站消息认领与最终状态 |
| `outbox` | 待执行投递任务 |
| `adapter_manifests` | 外部适配器版本和启用状态 |

### 事务规则

1. 先写 Notification 与 Outbox，再执行网络调用。
2. 网络调用成功后，用一个事务写 Delivery 与 ReplyRoute。
3. 结果不确定时写 `Unknown`，不自动重试。
4. Claim 与 ReplyRoute 查询在同一个应用服务调用中完成。
5. 多账号操作必须显式携带 `ChannelAccountId`。
6. 数据库迁移只前进，不在启动时自动删除旧表。

### 密钥

密钥不进入 SQLite：

- Windows 使用 Credential Manager 或 DPAPI。
- macOS 使用 Keychain。
- HarmonyOS 使用系统安全存储。
- 数据库只保存不可逆账号标识和不含密钥的引用。

## 安全设计

1. 本地 IPC 必须验证当前用户或进程身份。
2. WebView 使用严格 CSP，只加载应用资源。
3. Tauri capability 只开放业务命令，不暴露通用文件和 shell 权限。
4. 外部适配器需要版本、来源和权限校验。
5. 日志递归脱敏 token、secret、authorization、cookie 和凭据字段。
6. 更新包必须校验签名、校验和和发布源。
7. macOS 发布需要 Developer ID 签名和 notarization。
8. HarmonyOS 发布需要 HarmonyOS 应用签名和后台权限审核。
9. Agent 原文与用户回复正文默认不写调试日志。

## 可靠性规则

1. 推送可以重试，但必须依据渠道返回的确定性失败类型。
2. 回复投递不做自动重放，保持至多一次语义。
3. 超时或进程崩溃后的投递状态一律标记为 `Unknown`。
4. 路由只使用精确的外部消息 ID 和渠道账号。
5. 路由过期、冲突或缺失时返回可见错误。
6. 渠道账号切换时清空属于旧账号的游标、会话和路由上下文。
7. 每个渠道实现独立限流、退避和熔断。
8. 单个渠道故障不能阻塞其他渠道或 Agent 入站。

## 可观测性

统一使用 `tracing`：

- `ingest` span：Agent、事件 ID、会话 ID 的哈希。
- `delivery` span：Notification、渠道、账号、投递状态。
- `reply` span：入站消息、路由结果、Agent、最终状态。
- `host` span：平台、宿主版本、IPC 或 Ability 状态。

日志默认写当前用户临时目录或应用日志目录。破坏性操作、凭据错误和未知投递状态必须保留可追踪记录。

## 测试策略

### 单元测试

- domain 规则使用纯函数测试。
- application 服务使用内存仓库和假时钟。
- Agent 与 Channel 适配器分别测试解析、能力、错误映射。
- UI 只测试 DTO 映射、表单和交互状态。

### 契约测试

每个 Agent 适配器必须通过以下契约：

- descriptor 唯一。
- 能力声明与实际实现一致。
- 无法解析的事件返回明确错误。
- resume 不接受空会话 ID 或空正文。
- 不支持的平台返回明确错误。

每个 Channel 适配器必须通过以下契约：

- 多账号隔离。
- 发送结果包含稳定的消息 ID 或明确 Unknown。
- 入站事件能被标准化。
- 回调或长连接重复投递不会重复分发。
- 文本长度和能力限制被正确执行。

### 集成测试

Windows 首版：

- 真实 Agent Hook 产生事件。
- 核心写入 SQLite 和 Outbox。
- 测试渠道收到消息。
- 引用消息能精确路由回测试 Agent。
- 应用重启后路由、Claim 和历史保持。

macOS：

- Keychain、Unix Socket、菜单栏、自启动和签名链路。
- 与 Windows 相同的应用层契约测试。

HarmonyOS PC：

- HAP 安装、ArkWeb 渲染、NAPI 调用、后台任务和系统安全存储。
- 首次接入、真实渠道收发、引用回复和进程重启。
- x86_64 与目标 ARM64 真机分别验证。

## 分阶段落地

### 第一阶段：Windows 核心与兼容入口

1. 建立 Rust workspace 和领域/应用边界。
2. 实现 SQLite、outbox、路由、Claim。
3. 实现 Agent 与渠道注册表。
4. 迁移或重写一个真实 Agent 和一个真实渠道。
5. 保持 CLI 与 Hook 快速返回语义。
6. 用集成测试证明推送和引用回复闭环。

### 第二阶段：React 桌面 UI

1. 建立共享 HostBridge 与 Tauri 宿主。
2. 实现总览、Agents、Channels、History、Diagnostics。
3. 替换 Win32 UI，移除自绘窗口和固定布局。
4. Windows 安装、自启动、更新和签名全部走通。

### 第三阶段：扩展生态

1. 增加飞书渠道。
2. 增加更多渠道账号和入站模式。
3. 增加更多内置 Agent 适配器。
4. 落地外部 Agent 适配器协议。
5. 用契约测试约束所有新增适配器。

### 第四阶段：macOS

1. 完成 Tauri macOS 宿主。
2. 接入 Keychain、Unix Socket、菜单栏和登录项。
3. 完成签名、notarization 和更新。
4. 对现有适配器执行 macOS 真实链路验收。

### 第五阶段：HarmonyOS PC

1. 建立 ArkWeb/ArkTS 宿主或采用稳定 Tauri OHOS 宿主。
2. 通过 NAPI 嵌入 Rust 核心。
3. 实现后台任务、系统安全存储和 Ability IPC。
4. 分别验证 HarmonyOS PC 目标架构。
5. 确认外部 Agent 与后台任务的平台限制并提供明确支持边界。

## 完成标准

架构迁移完成必须同时满足：

1. Windows、macOS、HarmonyOS PC 的 UI 使用同一份 React 资源。
2. 所有平台共用同一套 Rust domain、application、Agent 与渠道协议。
3. 新增 Agent 或渠道不修改 UI 页面和通知/回复核心服务。
4. 推送、历史、路由和引用回复在三个平台具有一致的领域语义。
5. 所有平台都能通过适配器契约测试。
6. Windows 先完成真实 Agent、真实渠道和真实引用回复验收。
7. macOS 与 HarmonyOS PC 在官方支持能力可用后再进入发布门禁。
