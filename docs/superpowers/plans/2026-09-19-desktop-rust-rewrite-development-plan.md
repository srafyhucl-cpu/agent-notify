# AgentNotify 桌面端 Rust 重写开发计划书

**计划版本：** 1.0

**架构基线：** [AgentNotify 桌面端跨平台与可扩展架构设计](../specs/2026-09-19-desktop-cross-platform-architecture-design.md)

**计划状态：** 生效中。任务进度以四份执行计划中的复选框、独立提交和门禁证据为准，不在本总计划中重复维护。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从零构建一套以 Rust 为业务核心、Tauri 为 Windows 宿主、React/TypeScript 为共享 UI 的常驻桌面应用，先完成 Windows 的 Agent 消息推送与精确引用回复闭环，再以适配器方式扩展 Agent、渠道和未来平台。

**Architecture:** 采用模块化单体。`domain` 只包含纯业务模型，`application` 定义用例和端口，`runtime` 负责装配与监督，SQLite/Agent/Channel 适配器实现端口，Tauri 宿主只提供平台能力和类型化 UI 桥。现有 Go 程序在切换前保持原样，新实现完成后一次性替换发布入口，不长期维护双 UI。

**Tech Stack:** Rust stable（edition 2024）、Tokio、SQLite WAL、`rusqlite`、`serde`、`tracing`、Tauri 2 stable、React、TypeScript、Vite、TanStack Query、React Router、`lucide-react`、Playwright、Inno Setup。

**文档定位：** 本文件是开发总控计划，负责界定范围、阶段、依赖和验收门禁。具体到文件、接口、测试和提交的任务步骤由下方四份执行计划承载。

## Global Constraints

- 当前只交付 Windows 10/11；macOS 与 HarmonyOS PC 只保留 `PlatformHost` 边界，不安排实现、打包、真机验收或发布时间。
- 不引入 Electron、Chromium、Node 运行时或 Rust 动态库插件 ABI。
- 不提供面向用户或 AI 的管理 CLI，也不提供 MCP。桌面 UI 是配置、登录、历史、诊断和退出的唯一管理入口。
- 唯一保留的命令行入口是 `agentnotify-ingress.exe`：它只接收版本化 Agent 事件；不能查询状态、修改配置、登录渠道、删除历史或退出应用。
- Agent 与渠道均采用 Registry + Adapter；新增内置或外部适配器不得修改 UI 页面、通知服务、回复服务或路由核心。
- 引用回复只按 `ChannelId + ChannelAccountId + ExternalMessageId` 精确路由；禁止标题、正文、最近会话、工作目录和跨账号回退。
- 推送使用事务型 Outbox；引用回复保持至多一次语义；超时、崩溃或无法确认时记录为 `Unknown`，禁止自动重放。
- SQLite 迁移只前进；密钥不进入 SQLite、日志或 UI 事件；数据库、日志和本地 IPC 不允许保存未脱敏 token、secret、authorization、cookie 或消息正文。
- `domain`、`application` 和适配器代码不得出现 `cfg(windows)` 业务分支；平台差异只存在于 `hosts` 和宿主提供的端口实现。
- 所有注释、用户文案和新增文档使用中文；代码标识、协议字段和公开接口使用英文。
- 依赖缓存、Rust 工具链、Node 缓存和构建产物优先放在 `D:\Temp`、`D:\Tools` 或项目内目录，不使用 C 盘缓存。
- 每个任务必须独立提交；提交信息使用 Conventional Commits + 中文描述。
- 首个生产闭环固定为 `OpenCode + ClawBot/微信`。Codex、Antigravity、Devin、Command Code、飞书和外部 Agent 协议只在闭环通过后扩展。
- 在正式切换前，`internal/app/version.go` 仍是当前发布版本唯一来源。版本源切换只能作为切换任务的明确交付，不能在其他任务中顺带修改。
- 目标版本从 `2.0.0` 开始；只有 Windows 替换链路全部验收通过后才能生成正式版本。
- 本计划按依赖和门禁推进，不以日历压缩验收；前序阶段证据不完整时，不得把后续任务标记为完成。

## 实施拆分与里程碑

方案包含多个可独立验收的子系统，因此开发计划拆为一份总计划和四份执行计划。总计划只维护跨阶段目标和门禁关系，详细任务不在这里复制：

| 计划 | 独立交付物 | 前置 | 完成门禁 |
|---|---|---|---|
| [Rust 核心与 Windows 内部入口](2026-09-19-rust-core-and-windows-ingress-implementation.md) | 可在无 UI 环境运行的 Rust 核心、SQLite 状态、Outbox、精确回复和内部事件入口 | 无 | 假 Agent + 假渠道闭环、崩溃恢复、重复事件和未知结果测试通过 |
| [Tauri 与 React 桌面 UI](2026-09-19-tauri-react-desktop-ui-implementation.md) | 可运行的 Windows 主窗口、托盘、动态 Agent/渠道页面、历史和诊断页 | Rust 核心计划 | 真实 Tauri 进程可操作假适配器，UI 契约、视觉与安装链路通过 |
| [首个生产闭环、迁移与切换](2026-09-19-production-loop-migration-cutover-implementation.md) | OpenCode + ClawBot 的真实推送和引用回复、旧数据导入、安装器切换与回滚方案 | 前两份计划 | 微信真实收到消息并精确回复原 OpenCode 会话，旧数据不丢失，旧 UI 不再作为发布入口 |
| [扩展生态](2026-09-19-extension-ecosystem-implementation.md) | Codex、Antigravity、Devin、Command Code、飞书、多账号策略和外部适配器协议 | Windows 切换完成 | 每个适配器独立通过契约测试与真实链路验收，不修改核心页面和服务 |

执行顺序固定为 `Rust 核心 -> Tauri/UI -> 生产闭环与切换 -> 扩展生态`。可以使用假适配器并行进行核心、UI 和宿主开发，但真实链路、迁移和正式发布必须按依赖顺序通过门禁，不能用替身测试替代真实验收。

### 进度记录规则

- 执行计划中的复选框只有在实现、测试、门禁和独立提交全部完成后才能勾选。
- 一个任务出现阻塞时保留未完成状态，并在对应任务下记录阻塞事实；不得用“基本完成”跳过门禁。
- 跨阶段状态统一使用 `未开始 / 进行中 / 待验收 / 已完成 / 阻塞`；第一次真实验收通过前不得标记为 `已完成`。
- 架构说明、总计划和执行计划出现冲突时，先更新架构基线并完成影响评审，再修改执行任务；代码只作为当前实现事实，不自动覆盖已确认的设计。

## 目标目录

```text
Cargo.toml
rust-toolchain.toml
crates/
  agentnotify-domain/
  agentnotify-application/
  agentnotify-runtime/
  agentnotify-storage-sqlite/
  agentnotify-agent-sdk/
  agentnotify-channel-sdk/
  agentnotify-agent-opencode/
  agentnotify-channel-clawbot/
  agentnotify-agent-*/
  agentnotify-channel-*/
  agentnotify-testkit/
hosts/
  desktop-tauri/
apps/
  ingress/
  hooks/
  agent-adapter-host/
  desktop-ui/
docs/
  superpowers/
    plans/
    specs/
tools/
  rust/
  ui/
```

上述目录是稳定边界，不是“每类文件一个 crate”。只有出现独立依赖、独立发布或独立测试需求时才新增 crate；不允许为了目录对称拆分空壳模块。

## 架构冻结与变更控制

Windows 交付阶段冻结以下边界：

1. Rust 核心、Tauri 宿主和 React UI 的职责边界不重新拆分。
2. Agent、渠道和平台扩展只能沿现有 trait、注册表和事件协议增加实现，不能让专属逻辑反向进入 UI 或核心服务。
3. 推送、回复、迁移和密钥语义不变；如果需要改变可靠性语义，必须先修改架构设计并补充故障模型。
4. macOS 与 HarmonyOS PC 不在当前阶段计划中；具备真实测试环境后单独立项，不能在 Windows 任务中顺带实现。
5. 管理 CLI、MCP、Electron、Node 常驻运行时和 Rust 动态库插件 ABI 不进入当前范围。

可增量演进的内容包括：新增适配器、追加 SQLite 迁移、增加描述符能力、补充诊断项和 UI 页面内组件。任何破坏协议、数据兼容性或用户数据安全的变更都必须同时提供迁移、回滚和验收方案。

## 核心接口边界

这些接口在后续计划中保持一致，后续任务不得自行改名或改变语义。

```rust
#[async_trait]
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

```rust
#[async_trait]
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

## 阶段与门禁

### 阶段 A：Rust 核心与内部事件入口

**交付：** 一个不依赖 Tauri、WebView、Windows UI 和具体渠道的核心运行时，包含领域模型、应用服务、SQLite、Outbox、回复 Claim、适配器注册表、内部事件入口和测试工具。

**必须通过：**

- 假 Agent 产生事件后，假渠道收到一次推送，并建立精确回复路由。
- 重复事件只产生一个 Notification 和一个 Delivery。
- 同一入站消息并发处理时只成功 Claim 一次。
- 渠道超时或进程中断后记录 `Unknown`，重启不自动重放。
- 两次重启后 Notification、Delivery、Route 和 Claim 状态保持一致。
- 命名管道仅允许当前 Windows 用户连接；核心离线时事件写 spool 后快速退出，下次启动消费。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 全部通过。

### 阶段 B：Tauri 与 React UI

**交付：** 可运行的 Windows 主窗口、托盘、单实例、关闭隐藏、动态 Agents/Channels、History、Diagnostics、Settings，以及仅能调用业务命令的类型化 HostBridge。

**必须通过：**

- UI 不读取数据库、不持有密钥、不执行 shell、不根据固定 Agent 或渠道名写业务分支。
- Agents 与 Channels 页面完全由 descriptor、capabilities 和 JSON Schema 驱动。
- 页面在 1280x720、1440x900 和 200% 缩放下无文字溢出、遮挡或控件跳动。
- 关键流程可用键盘完成；图标按钮有 tooltip；错误文案说明可执行的下一步。
- Tauri capability 不开放通用文件系统和 shell；窗口关闭隐藏，托盘可显示、暂停和退出。
- Playwright 对假 HostBridge 的核心流程、空态、错误态和视觉快照通过。
- 安装版能自启动、单实例、加载共享 UI，并在升级后保留用户状态。

### 阶段 C：首个真实闭环、迁移与切换

**交付：** OpenCode 和 ClawBot 的生产适配器、旧数据导入、真实微信推送与引用回复、安装器切换和可回滚发布。

**必须通过：**

- OpenCode Hook/插件产生事件后，桌面核心写入 SQLite 和 Outbox。
- ClawBot 收到消息并返回稳定消息 ID，ReplyRoute 使用该 ID 精确落库。
- 在微信中引用该通知并回复后，原 OpenCode 会话收到消息；错误路由不会落到其他会话。
- 应用退出、重启、客户端断线重连后，历史、Claim、Route 和 Outbox 状态保持一致。
- 旧配置、登录状态、开关、推送历史、路由和 Claim 完成幂等导入；重复导入不产生重复记录。
- 迁移失败时新应用拒绝修改旧文件，旧程序仍可启动，并向用户显示明确诊断。
- 安装、自启动、签名、升级和卸载通过自动化 smoke 与真实 Windows 验收。
- 旧 Win32 UI 不再是发布入口；新版本失效时可回退到上一版，但回退不重复执行已 Claim 的回复。

### 阶段 D：扩展生态

**交付：** 其余内置 Agent、飞书渠道、多账号通知策略和外部适配器进程协议。

本阶段不改变核心接口。每个新增能力按固定流程执行：

1. 新增一个适配器 crate，并实现既有 SDK trait。
2. 从 descriptor 和 capabilities 暴露能力，不增加 UI 枚举。
3. 增加契约测试、错误映射测试和脱敏测试。
4. 增加 isolatable 集成测试，不访问真实用户配置。
5. 运行真实链路验收。
6. 注册到 runtime；若适配器要求修改通知服务、回复服务或页面业务分支，设计不得合并。

## 开发执行流程

每个执行任务固定遵循以下流程：

1. 确认前置任务的提交、门禁和验收证据齐全。
2. 先写能够证明目标行为的失败测试或契约测试。
3. 实现最小闭环，保持变更只覆盖当前任务。
4. 运行任务要求的专项测试和所属阶段的完整门禁。
5. 在隔离目录完成必要的 smoke；涉及真实 Agent、渠道或安装器时必须保留脱敏证据。
6. 独立提交，使用 Conventional Commits + 中文描述，并同步勾选执行计划。

任一门禁失败都阻止进入下一任务。修复必须保留失败证据、失败原因和最终通过命令，不能用删除测试、降低断言或扩大超时掩盖不稳定行为。

## 统一门禁

| 范围 | 最低门禁 | 证据 |
|---|---|---|
| Rust 核心 | `tools\rust\gate.ps1` 通过 fmt、clippy、workspace tests | 命令退出码与测试汇总 |
| React/Tauri | `tools\ui\gate.ps1` 通过 typecheck、Vitest、构建、Playwright、视觉与 Rust 门禁 | 截图基线、axe 结果与命令退出码 |
| 迁移 | 隔离数据目录完成幂等导入、失败回滚与旧文件 hash 不变检查 | 迁移报告与自动化测试 |
| 真实闭环 | OpenCode + ClawBot 在真实 Windows、真实微信完成推送和精确引用回复 | 脱敏验收记录与可复现步骤 |
| 发布 | 安装、签名、自启动、升级、卸载和回滚 smoke 全部通过 | 构建产物、校验和、签名指纹与 smoke 结果 |
| 扩展 | 每个适配器独立通过契约、错误映射、脱敏和真实链路验收 | 逐适配器验收记录，不以同类适配器代替 |

非确定性网络测试、真实账号验收和真实安装升级不得作为默认 CI 的前置条件；它们由受控的隔离验收任务执行，但必须进入正式切换门槛。

## 设计覆盖矩阵

| 设计章节 | 落地位置 |
|---|---|
| 目标仓库结构、分层依赖 | Rust 核心计划 Task 1、Task 2 |
| 标识、通知、投递、路由、入站去重 | Rust 核心计划 Task 2 至 Task 5 |
| Agent 扩展模型与注册表 | Rust 核心计划 Task 5；生产闭环计划 Task 1 至 Task 2 |
| 渠道扩展模型与注册表 | Rust 核心计划 Task 6；生产闭环计划 Task 3 至 Task 6 |
| AgentIngestService | Rust 核心计划 Task 10 |
| NotificationService 与事务型 Outbox | Rust 核心计划 Task 11 |
| ReplyService 与至多一次语义 | Rust 核心计划 Task 12 |
| StatusService 与进程模型 | Rust 核心计划 Task 13 |
| 可观测性与脱敏 tracing | Rust 核心计划 Task 13 |
| SQLite、密钥与事务 | Rust 核心计划 Task 8、Task 9；UI 计划 Windows host task |
| 命名管道与 spool | Rust 核心计划 Task 14 至 Task 15 |
| PlatformHost 与 HostBridge | UI 计划 Task 2 至 Task 4 |
| UI 技术栈、信息架构与规则 | UI 计划 Task 5 至 Task 11 |
| 测试策略 | 四份执行计划的每个任务；生产闭环计划最终验收 |
| 安装、自启动、更新、签名 | UI 计划 Task 11；生产闭环计划 Task 10 至 Task 11 |
| macOS 与 HarmonyOS PC | 仅保留 `PlatformHost` 边界；当前无实现任务 |
| Codex、Antigravity、Devin、Command Code | 扩展生态计划 Task 2 至 Task 5 |
| 飞书渠道 | 扩展生态计划 Task 7 至 Task 8 |
| 多账号通知策略 | 扩展生态计划 Task 6 |
| 外部 Agent 适配器协议 | 扩展生态计划 Task 9 至 Task 10 |

## 跨阶段依赖

```text
核心领域与端口
  -> SQLite 与假适配器闭环
    -> 内部事件入口
      -> Tauri host + React UI
        -> OpenCode 适配器
          -> ClawBot 出站
            -> ClawBot 入站与精确回复
              -> 旧数据迁移
                -> 安装与切换
                  -> 扩展生态
```

禁止的捷径：

- 不允许 UI 直接调用 SQLite、凭据库或 Agent 进程。
- 不允许先做页面，再把状态推导塞回 React 或 Tauri command。
- 不允许用渠道原始响应结构进入 domain。
- 不允许用“最近消息”补全缺失的 ClawBot 消息 ID。
- 不允许为了赶进度增加面向用户的管理 CLI。
- 不允许在没有真实测试环境时提前实现 macOS 或 HarmonyOS PC 宿主。

## 交付窗口与观察期

正式交付按以下顺序推进，不使用日期代替质量门禁：

1. 先构建独立的 Rust Preview，不覆盖稳定的 Go 安装入口。
2. 在隔离配置目录完成旧数据迁移演练，确认幂等、回滚和故障诊断。
3. 使用真实测试账号完成 OpenCode + ClawBot 闭环验收，保留脱敏证据。
4. 通过全部发布门禁后生成 `2.0.0`，由安装器切换正式入口。
5. 进入一个正式小版本的观察期，监控迁移、推送、回复、崩溃和更新故障。
6. 观察期结束且用户明确同意后，才删除旧 Go UI 和迁移兼容层；删除旧源码必须是独立变更。

观察期内出现阻断级迁移、推送或回复问题时，停止扩展生态开发，优先修复或按既定回滚方案恢复；不得用开启新适配器掩盖核心可靠性问题。

## 发布与回滚策略

- 新版本先以独立测试标识构建，不覆盖当前稳定安装路径。
- 生产闭环通过后，先在隔离配置目录中完成迁移演练，再对真实用户数据执行备份和导入。
- 正式切换保留上一稳定安装器和校验文件；回滚由安装上一版完成，不修改已迁移的 SQLite。
- 回滚期间旧程序只能读取旧文件；新程序退出前必须完成数据库 checkpoint 和安全关闭，禁止两个版本同时运行。
- Outbox 中无法确认的推送不因回滚重发；Reply Claim 不因回滚重置。
- 每一次扩展适配器都只增加自身 crate、注册项和测试，不修改核心迁移历史。

## 风险与应对

| 风险 | 影响 | 应对 |
|---|---|---|
| 当前机器未安装 MSVC Build Tools，且当前账户无管理员权限 | 本地开发和正式打包的前置条件不同 | Rust 核心计划 Task 1 提供 D 盘 `cargo-xwin + LLVM` 本地回退以跑通测试；正式发布门禁使用 `gate.ps1 -RequireMsvc`，仍强制标准 Build Tools |
| Tauri WebView 自动化能力弱于普通浏览器 | UI 回归不稳定 | UI 逻辑通过 HostBridge mock 用 Playwright 覆盖；Tauri 宿主单独做启动、托盘、权限和安装 smoke |
| ClawBot 消息 ID 或在多账号下不稳定 | 回复误投或漏投 | 在真实链路完成 ID 对照；缺 ID、冲突、过期一律拒绝路由并保留 `Unknown` |
| 旧 JSONL 数据存在损坏行 | 迁移中断或漏数据 | 逐行导入、跳过损坏行并报告计数；迁移记录使用确定性哈希保证幂等 |
| Windows 凭据写入失败 | 渠道无法登录 | 不落回明文 SQLite；显示阻塞错误并允许重试 |
| React 页面逐步出现 Agent/渠道专属分支 | 扩展成本回升 | 强制 descriptor/schema 驱动；契约测试可检查固定标识未进入 UI 业务逻辑 |
| Rust 重写范围不断膨胀 | 长期无法切换 | 首个闭环严格限定 OpenCode + ClawBot；其他适配器只按扩展流程进入阶段 D |

## 完成定义

Windows 替代版只有在以下条件同时成立时才算完成：

1. Rust 核心、SQLite、Outbox、路由、Claim 和内部事件入口通过自动化门禁。
2. Tauri + React 是唯一 Windows 交付 UI，旧 Win32 自绘界面不再进入安装包。
3. 新增 Agent 或渠道只增加适配器、注册项和测试，不修改 UI、通知服务和回复服务。
4. OpenCode + ClawBot 在真实 Windows、真实微信上完成推送和引用回复验收。
5. 旧配置、登录、开关、历史和路由完成可审计、可重试、幂等的迁移。
6. 超时和崩溃不会产生自动重放；所有不可确认状态都可见。
7. 安装、自启动、升级、签名和卸载通过自动化 smoke 与人工验收。
8. 没有管理 CLI、管理 MCP、Electron、Node 运行时或未排期的平台发布门禁。
