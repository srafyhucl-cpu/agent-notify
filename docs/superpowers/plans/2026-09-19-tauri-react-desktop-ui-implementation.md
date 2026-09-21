# Tauri 与 React 桌面 UI Implementation Plan

> 进度校正（2026-09-21）：Task 1 与 Task 2 已由 `30f919a`、`6fcf1a4` 实现并提交，复选框此前漏记；其余已完成任务保持原记录。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 Tauri 2 宿主和 React/TypeScript 主窗口替换 Win32 自绘 UI，提供动态 Agents、Channels、History、Diagnostics、Settings，并确保 UI 只能通过类型化 HostBridge 调用 Rust 核心。

**Architecture:** `hosts/desktop-tauri` 负责窗口、托盘、平台端口和业务命令；`apps/desktop-ui` 是唯一的共享 UI。命令与 DTO 由 Rust 导出为 TypeScript，React 使用 TanStack Query 读取快照，通过事件失效缓存；浏览器测试使用同一 HostBridge 的 mock 实现。

**Tech Stack:** Tauri 2 stable、React、TypeScript、Vite、TanStack Query、TanStack Virtual、React Router、`lucide-react`、Vitest、React Testing Library、Playwright、`axe-core`、Windows WebView2。

## Global Constraints

- 当前只实现 Windows 宿主；共享 UI 不得出现平台路径、平台条件分支或 macOS/HarmonyOS 预留页面。
- 使用一个主窗口，不使用每页独立原生窗口。关闭主窗口只隐藏，不停止 runtime。
- UI 不得访问 SQLite、Credential Manager、文件系统、shell、进程列表或渠道密钥。
- UI 不得根据 `agentId === "codex"`、`channelId === "clawbot"` 等固定值添加业务分支。
- Agent 和渠道设置由 descriptor、capabilities 与受支持 JSON Schema 子集驱动；不支持的结构必须显示明确错误。
- 密码、token 和 secret 只在提交时经过 HostBridge；表单状态、Query cache、日志和错误对象不得保存明文。
- Tauri capability 只开放业务命令、窗口控制、托盘和事件；禁止 `shell:allow-*`、通用 `fs:*` 和任意 URL 打开。
- 页面采用工作台布局：高密度列表、表格、筛选项和详情区，不使用营销 hero、装饰性渐变、嵌套卡片或大面积单色。
- 图标使用 `lucide-react`；纯图标按钮必须有 tooltip 和可访问名称。
- 卡片只用于重复条目、对话框和确实需要边框的工具；圆角不超过 8 像素。
- 字号不随 viewport 缩放；文字必须在 1280x720、1440x900 和 200% Windows 缩放下完整显示。
- 浏览器测试使用 mock HostBridge；Tauri 真实宿主另做权限、启动、托盘、单实例和安装 smoke。
- 每个任务结束运行 `tools/ui/gate.ps1` 并提交；涉及 Rust 宿主时同时运行 `tools/rust/gate.ps1`。
- 不修改现有 Go 文件；本计划只新增 Tauri 与 React 实现。

---

### Task 1: 建立 Tauri host 与 React/Vite 构建骨架

**Files:**
- Create: `hosts/desktop-tauri/Cargo.toml`
- Create: `hosts/desktop-tauri/build.rs`
- Create: `hosts/desktop-tauri/tauri.conf.json`
- Create: `hosts/desktop-tauri/capabilities/main.json`
- Create: `hosts/desktop-tauri/icons/icon.ico`
- Create: `hosts/desktop-tauri/src/main.rs`
- Create: `hosts/desktop-tauri/src/lib.rs`
- Create: `apps/desktop-ui/package.json`
- Create: `apps/desktop-ui/package-lock.json`
- Create: `apps/desktop-ui/index.html`
- Create: `apps/desktop-ui/tsconfig.json`
- Create: `apps/desktop-ui/vite.config.ts`
- Create: `apps/desktop-ui/src/main.tsx`
- Create: `apps/desktop-ui/src/app/App.tsx`
- Create: `apps/desktop-ui/src/styles/global.css`
- Create: `tools/ui/gate.ps1`
- Modify: `Cargo.toml`
- Test: `hosts/desktop-tauri/tests/bootstrap.rs`

**Interfaces:**
- Consumes: Rust 核心 workspace。
- Produces: `agentnotify-desktop` Tauri 应用、`apps/desktop-ui` Vite 应用、统一 UI 门禁脚本。

- [x] **Step 1: 建包并写启动测试**

`hosts/desktop-tauri/tests/bootstrap.rs`：

```rust
#[test]
fn package_exposes_tauri_builder_without_starting_a_window() {
    let app = agentnotify_desktop::build_test_app();
    assert_eq!(app.config().product_name.as_deref(), Some("AgentNotify"));
}
```

构建函数必须允许测试只构造 builder，不连接 WebView、不打开窗口、不读取用户配置。

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-desktop --test bootstrap
```

Expected: FAIL，host 包不存在。

- [x] **Step 3: 创建 Tauri 配置**

`Cargo.toml` 增加 `hosts/desktop-tauri` 成员，并添加依赖：

```toml
tauri = { version = "2", features = [] }
tauri-plugin-single-instance = "2"
tauri-plugin-autostart = "2"
serde = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

`tauri.conf.json` 关键值：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "AgentNotify",
  "version": "2.0.0-dev.0",
  "identifier": "com.agentnotify.desktop",
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "AgentNotify",
        "width": 1120,
        "height": 720,
        "minWidth": 980,
        "minHeight": 640,
        "resizable": true,
        "visible": false
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; style-src 'self'; connect-src ipc: http://ipc.localhost"
    }
  },
  "build": {
    "beforeDevCommand": "npm --prefix ../apps/desktop-ui run dev",
    "beforeBuildCommand": "npm --prefix ../apps/desktop-ui run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../apps/desktop-ui/dist"
  }
}
```

`capabilities/main.json` 只授予 `core:default`、`core:event:default`、`core:window:allow-show`、`core:window:allow-hide`、`core:window:allow-close`、`core:window:allow-set-focus` 和 autostart 所需权限。不得配置 shell、fs 或 opener 插件。

- [x] **Step 4: 创建 React 入口与门禁**

`package.json` 固定 `packageManager: "npm@10.9.8"`，依赖 Rx、Vite、Router、Query、Virtual、lucide-react，开发依赖 TypeScript、Vitest、Testing Library、Playwright 和 axe-core。提交 `package-lock.json` 锁定实际解析版本。

`package.json` 的脚本固定为：

```json
{
  "scripts": {
    "dev": "vite --port 1420 --strictPort",
    "build": "tsc -b && vite build",
    "typecheck": "tsc --noEmit",
    "test": "vitest",
    "test:e2e": "playwright test --config tests/playwright.config.ts",
    "test:a11y": "playwright test --config tests/playwright.config.ts --grep @a11y",
    "test:visual": "playwright test --config tests/playwright.config.ts --grep @visual",
    "bridge:generate": "cargo run -p agentnotify-desktop --bin export-bindings"
  }
}
```

`tools/ui/gate.ps1`：

```powershell
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$env:npm_config_cache = if ($env:npm_config_cache -like 'D:\*') { $env:npm_config_cache } else { 'D:\Temp\npm-cache' }
$env:TEMP = if ($env:TEMP -like 'D:\*') { $env:TEMP } else { 'D:\Temp\agentnotify-temp' }
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:npm_config_cache,$env:TEMP | Out-Null

Push-Location (Join-Path $root 'apps\desktop-ui')
try {
    npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci 失败' }
    npm run typecheck
    if ($LASTEXITCODE -ne 0) { throw 'TypeScript 检查失败' }
    npm run test -- --run
    if ($LASTEXITCODE -ne 0) { throw 'Vitest 失败' }
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'Vite 构建失败' }
}
finally {
    Pop-Location
}

powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'tools\rust\gate.ps1')
```

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
npm --prefix .\apps\desktop-ui install
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: Rust 与前端门禁全部通过，Tauri 测试未打开窗口。

```powershell
git add Cargo.toml Cargo.lock hosts/desktop-tauri apps/desktop-ui tools/ui/gate.ps1 .gitignore
git commit -m "build(desktop): 初始化 Tauri 与 React 工程"
```

---

### Task 2: 建立类型化 HostBridge 与命令契约

**Files:**
- Create: `hosts/desktop-tauri/src/bridge/mod.rs`
- Create: `hosts/desktop-tauri/src/bridge/commands.rs`
- Create: `hosts/desktop-tauri/src/bridge/dto.rs`
- Create: `hosts/desktop-tauri/src/bridge/events.rs`
- Create: `hosts/desktop-tauri/src/bridge/error.rs`
- Create: `apps/desktop-ui/src/bridge/types.ts`（由导出工具生成，禁止手改）
- Create: `apps/desktop-ui/src/bridge/hostBridge.ts`
- Create: `apps/desktop-ui/src/bridge/tauriHostBridge.ts`
- Create: `apps/desktop-ui/src/bridge/mockHostBridge.ts`
- Create: `apps/desktop-ui/src/bridge/index.ts`
- Modify: `hosts/desktop-tauri/src/lib.rs`
- Modify: `apps/desktop-ui/package.json`
- Test: `hosts/desktop-tauri/tests/bridge_contract.rs`
- Test: `apps/desktop-ui/src/bridge/hostBridge.test.ts`

**Interfaces:**
- Consumes: `RuntimeSnapshot`、配置 DTO、历史 DTO 和登录事件。
- Produces: `HostBridge`、`BusinessCommand`、`HostEvent`、`CommandError` 以及 Rust 到 TypeScript 的生成绑定。

- [x] **Step 1: 写命令与错误契约测试**

```rust
#[test]
fn command_error_never_serializes_sensitive_fields() {
    let error = CommandError::new("channel_login_failed", "登录失败，请重新扫码")
        .with_retryable(true);
    let json = serde_json::to_string(&error).unwrap();
    assert!(!json.contains("token"));
    assert!(!json.contains("authorization"));
    assert!(!json.contains("cookie"));
}
```

前端：

```ts
import { describe, expect, it, vi } from "vitest";
import { createMockHostBridge } from "./mockHostBridge";

describe("HostBridge", () => {
  it("deduplicates snapshot queries through the bridge contract", async () => {
    const bridge = createMockHostBridge();
    const first = bridge.invoke("get_snapshot", {});
    const second = bridge.invoke("get_snapshot", {});
    await expect(first).resolves.toMatchObject({ runtime: { state: "Running" } });
    await expect(second).resolves.toMatchObject({ runtime: { state: "Running" } });
    expect(bridge.calls("get_snapshot")).toHaveLength(2);
  });
});
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-desktop --test bridge_contract
npm --prefix .\apps\desktop-ui run test -- --run hostBridge
```

Expected: FAIL，桥接类型不存在。

- [x] **Step 3: 定义稳定命令集合**

只允许以下命令：

```text
get_snapshot
list_agents
update_agent_config
list_channel_accounts
begin_channel_login
submit_channel_login_code
logout_channel_account
enable_channel_account
disable_channel_account
send_test_notification
list_notifications
get_notification_detail
retry_delivery
get_diagnostics
get_settings
update_settings
set_runtime_paused
quit_app
```

命令名和 DTO 一经首个 UI 版本发布只能追加，不得复用旧名称表达新语义。

- [x] **Step 4: 生成前端类型**

使用 `specta`/`tauri-specta` 或等价的 Rust-first 导出方案：

- Rust DTO 实现类型导出。
- 生成文件头写明“此文件由 Rust 生成，禁止手改”。
- `npm run bridge:generate` 生成 `types.ts`。
- CI 在生成后运行 `git diff --exit-code -- apps/desktop-ui/src/bridge/types.ts`，阻止 Rust/TS 漂移。
- 生成失败时门禁直接失败，不提交旧文件。

`HostBridge` 前端接口保持：

```ts
export interface HostBridge {
  invoke<TCommand extends BusinessCommand>(
    command: TCommand,
    payload: CommandPayload<TCommand>
  ): Promise<CommandResult<TCommand>>

  subscribe<TEvent extends HostEvent>(
    event: TEvent,
    handler: (payload: EventPayload<TEvent>) => void
  ): () => void
}
```

- [x] **Step 5: 实现 Tauri 与 mock 适配器**

`tauriHostBridge` 只调用生成的 typed command；`mockHostBridge` 保存调用记录并允许测试注入延迟、错误和事件。生产代码不得直接导入 `@tauri-apps/api/core` 的 `invoke`。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
npm --prefix .\apps\desktop-ui run bridge:generate
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
git diff --exit-code -- apps/desktop-ui/src/bridge/types.ts
```

Expected: 全部通过，生成文件无漂移。

```powershell
git add hosts/desktop-tauri apps/desktop-ui
git commit -m "feat(bridge): 增加类型化 HostBridge 契约"
```

---

### Task 3: 实现 Windows PlatformHost 端口

**Files:**
- Create: `hosts/desktop-tauri/src/platform/mod.rs`
- Create: `hosts/desktop-tauri/src/platform/windows/mod.rs`
- Create: `hosts/desktop-tauri/src/platform/windows/paths.rs`
- Create: `hosts/desktop-tauri/src/platform/windows/secrets.rs`
- Create: `hosts/desktop-tauri/src/platform/windows/process.rs`
- Create: `hosts/desktop-tauri/src/platform/windows/tasks.rs`
- Create: `hosts/desktop-tauri/src/platform/windows/system_ui.rs`
- Modify: `hosts/desktop-tauri/src/lib.rs`
- Test: `hosts/desktop-tauri/tests/platform_windows.rs`

**Interfaces:**
- Consumes: Rust 核心的 `PlatformHost`、`AppPaths`、`SecretStore`、`ProcessRunner`、`BackgroundTasks`、`LocalIpc`、`SystemUi`。
- Produces: `WindowsPlatformHost`。

- [x] **Step 1: 写路径、密钥与进程测试**

```rust
#[test]
fn test_overrides_isolate_all_platform_paths() {
    let paths = AppPaths::for_tests(Path::new(r"D:\Temp\agentnotify-tests"));
    assert!(paths.config_dir.starts_with(r"D:\Temp\agentnotify-tests"));
    assert!(paths.spool_dir.starts_with(r"D:\Temp\agentnotify-tests"));
    assert!(paths.log_dir.starts_with(r"D:\Temp\agentnotify-tests"));
}

#[tokio::test]
async fn process_runner_rejects_empty_program() {
    let runner = WindowsProcessRunner::default();
    let error = runner.run(ProcessRequest::new("")).await.unwrap_err();
    assert_eq!(error.code(), "process_program_empty");
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-desktop --test platform_windows
```

Expected: FAIL，WindowsPlatformHost 不存在。

- [x] **Step 3: 实现路径与系统 UI**

默认路径：

```text
config: %USERPROFILE%\.config\agent-notify
data:   %LOCALAPPDATA%\AgentNotify\data
logs:   %LOCALAPPDATA%\AgentNotify\logs
spool:  %LOCALAPPDATA%\AgentNotify\spool
```

`AGENT_NOTIFY_CONFIG_DIR`、`AGENT_NOTIFY_DATA_DIR`、`AGENT_NOTIFY_LOG_DIR`、`AGENT_NOTIFY_SPOOL_DIR` 继续作为测试和便携模式覆盖项。目录创建失败必须返回中文错误，不用当前工作目录兜底。

`SystemUi` 只提供显示/隐藏主窗口、设置托盘状态、显示系统通知和打开 AgentNotify 自己的日志目录。任何路径参数先 canonicalize，并确认位于 `AppPaths` 内。

- [x] **Step 4: 实现 Credential Manager 与无 shell 进程执行**

- `SecretStore` 使用 Windows Credential Manager，服务名固定 `AgentNotify`，账号键使用不可逆渠道账号引用。
- `get` 找不到返回 `NotFound`，不返回空字符串。
- 凭据写入失败不能降级到 SQLite。
- `ProcessRunner` 使用参数数组直接创建隐藏子进程，不通过 `cmd.exe` 或 PowerShell。
- stdout/stderr 各自最多保留 256 KiB，超出时截断并标记 truncated。
- 超时或取消返回 `UnknownResult`，调用方不得自动重放。
- 环境变量只允许显式 allowlist，不继承包含 token 的全量进程环境。

- [x] **Step 5: 实现后台任务包装**

`BackgroundTasks` 使用 Tokio `JoinSet`，任务名称必须稳定；shutdown 时先取消，再在 5 秒内等待，超时任务记录 `Unknown` 并交给 runtime 标记。不得用 detached thread。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
cargo test -p agentnotify-desktop --test platform_windows
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS，测试不读取真实用户凭据。

```powershell
git add hosts/desktop-tauri
git commit -m "feat(host): 实现 Windows 平台端口"
```

---

### Task 4: 实现主窗口、托盘、单实例、自启动与暂停

**Files:**
- Create: `hosts/desktop-tauri/src/lifecycle/mod.rs`
- Create: `hosts/desktop-tauri/src/lifecycle/window.rs`
- Create: `hosts/desktop-tauri/src/lifecycle/tray.rs`
- Create: `hosts/desktop-tauri/src/lifecycle/single_instance.rs`
- Create: `hosts/desktop-tauri/src/lifecycle/autostart.rs`
- Modify: `hosts/desktop-tauri/src/lib.rs`
- Test: `hosts/desktop-tauri/tests/lifecycle.rs`

**Interfaces:**
- Consumes: Tauri app builder、runtime handle、`get_snapshot` 和 `set_runtime_paused`。
- Produces: 一个主窗口生命周期、托盘菜单、第二次启动唤起、登录自启动和暂停状态。

- [x] **Step 1: 写生命周期纯逻辑测试**

```rust
#[test]
fn close_request_hides_window_while_runtime_is_running() {
    assert_eq!(
        window_action_for_close(RuntimeState::Running, true),
        WindowAction::Hide
    );
}

#[test]
fn quit_request_stops_runtime_before_exit() {
    assert_eq!(
        replacement_for_quit(RuntimeState::Running),
        LifecycleAction::ShutdownRuntime
    );
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-desktop --test lifecycle
```

Expected: FAIL，生命周期类型不存在。

- [x] **Step 3: 实现单实例与主窗口行为**

- 第二次启动只显示已有 `main` 窗口并获得焦点，不创建第二个 runtime。
- 启动时先创建 hidden 窗口，runtime 完成后显示，避免白屏。
- `WM_CLOSE` 只隐藏窗口。
- 托盘菜单固定为“显示 AgentNotify”“暂停通知/恢复通知”“退出”。
- “退出”先调用 runtime shutdown，再退出进程；关闭窗口或系统托盘不可直接杀进程。
- 不进入 Alt+Tab 不作为硬性要求；主窗口是正常应用窗口，托盘提供常驻入口。

- [x] **Step 4: 实现自启动与暂停**

- 自启动使用 Tauri autostart 插件注册当前用户登录启动，不写系统级 HKLM。
- 用户在 Settings 中修改自启动后马上反馈结果。
- 暂停状态持久化到 settings，不停止 ingress spool；暂停期间事件可以入队，但 Outbox 停止领取。
- 恢复后按原顺序继续。
- 退出时不删除 spool、SQLite、Route 或 Claim。

- [x] **Step 5: 运行真实宿主 smoke**

Run:

```powershell
cargo test -p agentnotify-desktop --test lifecycle
cargo tauri dev --no-watch
```

手工检查：窗口正常出现；关闭后托盘仍在；再次启动只唤起原窗口；暂停后状态栏变化；恢复后状态恢复；退出不留下进程。

- [x] **Step 6: 提交**

```powershell
git add hosts/desktop-tauri
git commit -m "feat(host): 增加主窗口托盘与单实例生命周期"
```

---

### Task 5: 建立 UI 设计令牌、工作台骨架与导航

**Files:**
- Create: `apps/desktop-ui/src/styles/tokens.css`
- Create: `apps/desktop-ui/src/styles/reset.css`
- Create: `apps/desktop-ui/src/styles/layout.css`
- Create: `apps/desktop-ui/src/app/AppShell.tsx`
- Create: `apps/desktop-ui/src/app/navigation.ts`
- Create: `apps/desktop-ui/src/app/router.tsx`
- Create: `apps/desktop-ui/src/components/AppNav.tsx`
- Create: `apps/desktop-ui/src/components/RuntimeStatusBar.tsx`
- Create: `apps/desktop-ui/src/components/EmptyState.tsx`
- Create: `apps/desktop-ui/src/components/InlineError.tsx`
- Create: `apps/desktop-ui/src/components/LoadingRows.tsx`
- Modify: `apps/desktop-ui/src/main.tsx`
- Modify: `apps/desktop-ui/src/app/App.tsx`
- Test: `apps/desktop-ui/src/app/AppShell.test.tsx`

**Interfaces:**
- Consumes: HostBridge 和 runtime snapshot。
- Produces: `/overview`、`/agents`、`/channels`、`/history`、`/diagnostics`、`/settings` 六个路由和统一页面骨架。

- [x] **Step 1: 写导航与可访问性测试**

```tsx
it("exposes all primary destinations with keyboard-readable names", async () => {
  render(<AppShell bridge={createMockHostBridge()} />);
  for (const name of ["总览", "Agents", "Channels", "History", "Diagnostics", "Settings"]) {
    expect(screen.getByRole("link", { name })).toBeVisible();
  }
  expect(screen.getByRole("main")).toHaveAttribute("id", "main-content");
});
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
npm --prefix .\apps\desktop-ui run test -- --run AppShell
```

Expected: FAIL，AppShell 不存在。

- [x] **Step 3: 定义设计令牌**

`tokens.css` 使用亮色中性基调：

```css
:root {
  --color-canvas: #f4f5f3;
  --color-surface: #ffffff;
  --color-surface-muted: #eef0ed;
  --color-border: #d7dbd6;
  --color-text: #1f2521;
  --color-text-muted: #687069;
  --color-primary: #176b5b;
  --color-primary-hover: #125447;
  --color-info: #2878a8;
  --color-warning: #b56b16;
  --color-danger: #b33a32;
  --color-success: #2f7d4a;
  --radius-control: 6px;
  --radius-panel: 8px;
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 24px;
  --space-6: 32px;
  --font-size-1: 12px;
  --font-size-2: 13px;
  --font-size-3: 14px;
  --font-size-4: 16px;
  --font-size-5: 20px;
}
```

状态色只表达 `正常 / 等待 / 异常 / 暂停`。正文使用系统 UI 字体，标题和正文均不得用 viewport 单位缩放。

- [x] **Step 4: 实现工作台布局**

- 左侧导航固定宽度 208px，六个目的地。
- 顶部状态带显示 runtime 状态、暂停控制和当前版本。
- 内容区最大宽度不锁死在窄列；高密度列表使用剩余空间。
- 页面截面是 full-width band 或不带浮层边框的直接布局，不使用 hero 和嵌套卡片。
- 空态只显示当前事实和一个主要动作。
- 错误条显示影响范围与下一步动作。
- 加载态保持列宽和行高稳定，不使用跳动的居中 spinner。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS，六个路由测试通过。

```powershell
git add apps/desktop-ui
git commit -m "feat(ui): 增加工作台骨架与设计令牌"
```

---

### Task 6: 建立查询、事件和统一错误层

**Files:**
- Create: `apps/desktop-ui/src/data/queryClient.ts`
- Create: `apps/desktop-ui/src/data/queryKeys.ts`
- Create: `apps/desktop-ui/src/data/useSnapshot.ts`
- Create: `apps/desktop-ui/src/data/useHostEvent.ts`
- Create: `apps/desktop-ui/src/data/errors.ts`
- Create: `apps/desktop-ui/src/data/mutations.ts`
- Modify: `apps/desktop-ui/src/main.tsx`
- Test: `apps/desktop-ui/src/data/events.test.tsx`

**Interfaces:**
- Consumes: `HostBridge`。
- Produces: `queryKeys`、`useSnapshot`、`useHostEvent`、`toUserError`，以及配置、登录、投递、历史和诊断 mutation hooks。

- [x] **Step 1: 写事件失效测试**

```tsx
it("invalidates snapshot and deliveries when delivery.changed arrives", async () => {
  const bridge = createMockHostBridge();
  const queryClient = createQueryClient();
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  render(<HostEventHarness bridge={bridge} queryClient={queryClient} />);

  bridge.emit("delivery.changed", { deliveryId: "delivery-1" });
  await waitFor(() =>
    expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.snapshot() })
  );
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ["deliveries"] });
});
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
npm --prefix .\apps\desktop-ui run test -- --run events
```

Expected: FAIL，事件层不存在。

- [x] **Step 3: 配置 TanStack Query**

- `staleTime` 按资源设置：snapshot 5 秒，历史 30 秒，诊断按需刷新。
- 窗口隐藏时降低轮询；显示时立即 refetch。
- 事件到达时 invalidate 对应 query，不把服务端状态复制到 Zustand 等全局 store。
- 网络以外的业务错误不默认重试。
- Query 统一保留上一成功快照，避免页面闪烁。
- 历史列表和诊断日志不进入全局内存缓存超过 200 条摘要。

- [x] **Step 4: 实现错误文案规则**

`toUserError(error)` 输出：

```ts
type UserError = {
  title: string;
  message: string;
  action?: { label: string; command: BusinessCommand; payload: unknown };
  diagnosticId?: string;
};
```

所有错误必须包含中文 `message`；`401` 类错误不得显示原始响应；凭据错误提示重新登录；数据库错误提示备份并查看 Diagnostics；Unknown 投递提示先检查原渠道，不提供自动重发按钮。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS。

```powershell
git add apps/desktop-ui
git commit -m "feat(ui): 增加查询事件与统一错误层"
```

---

### Task 7: 实现 Overview 与动态 Agents 页面

**Files:**
- Create: `apps/desktop-ui/src/features/overview/OverviewPage.tsx`
- Create: `apps/desktop-ui/src/features/overview/HealthSummary.tsx`
- Create: `apps/desktop-ui/src/features/overview/RecentDeliveries.tsx`
- Create: `apps/desktop-ui/src/features/agents/AgentsPage.tsx`
- Create: `apps/desktop-ui/src/features/agents/AgentList.tsx`
- Create: `apps/desktop-ui/src/features/agents/AgentDetail.tsx`
- Create: `apps/desktop-ui/src/features/agents/AgentConfigForm.tsx`
- Create: `apps/desktop-ui/src/components/SchemaForm.tsx`
- Modify: `apps/desktop-ui/src/app/router.tsx`
- Test: `apps/desktop-ui/src/features/agents/AgentsPage.test.tsx`

**Interfaces:**
- Consumes: `get_snapshot`、`list_agents`、`update_agent_config`、descriptor capabilities、config schema。
- Produces: Overview 页面和完全 descriptor 驱动的 Agents 列表、详情、开关与配置表单。

- [x] **Step 1: 写动态 Agent 测试**

```tsx
it("renders a new agent without editing page business branches", async () => {
  const bridge = createMockHostBridge({
    agents: [
      agentFixture({ id: "future-agent", displayName: "Future Agent", notify: true, resume: true }),
    ],
  });
  render(<AgentsPage bridge={bridge} />);

  expect(await screen.findByText("Future Agent")).toBeVisible();
  expect(screen.getByRole("switch", { name: "Future Agent 通知" })).toBeChecked();
});
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
npm --prefix .\apps\desktop-ui run test -- --run AgentsPage
```

Expected: FAIL，页面不存在。

- [x] **Step 3: 实现 Overview**

Overview 固定信息顺序：

1. Runtime 健康与暂停状态。
2. Agent 接入数量和异常数量。
3. Channel 账号在线、等待登录、失效数量。
4. 最近 20 条投递，显示 Notification、账号、状态、时间和可见错误。
5. 需要用户动作的故障列表。

主操作只有“重新检查”和“暂停/恢复通知”。不使用统计大卡片矩阵，不放营销式欢迎语。

- [x] **Step 4: 实现 descriptor 驱动的 Agents 页面**

- 列表列：名称、通知、回复、接入状态、最近事件、配置。
- 能力由 `capabilities` 控制按钮是否显示；UI 不读取固定 Agent ID。
- 详情只显示 descriptor 和标准化状态。
- 配置表单接收 JSON Schema，支持 `string`、`secret-string`、`boolean`、`integer`、`number`、`enum`、`textarea`。
- 不支持的类型显示“当前版本无法编辑此字段”，不静默丢失值。
- 开关修改调用 `update_agent_config`，成功后 invalidate snapshot；失败保持原值。
- secret 字段只显示“已配置/未配置”；提交后清空本地输入。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS，新增假 Agent 不要求修改页面代码。

```powershell
git add apps/desktop-ui
git commit -m "feat(ui): 实现总览与动态 Agents 页面"
```

---

### Task 8: 实现动态 Channels、登录与多账号管理

**Files:**
- Create: `apps/desktop-ui/src/features/channels/ChannelsPage.tsx`
- Create: `apps/desktop-ui/src/features/channels/ChannelAccountList.tsx`
- Create: `apps/desktop-ui/src/features/channels/ChannelAccountDetail.tsx`
- Create: `apps/desktop-ui/src/features/channels/ChannelLoginDialog.tsx`
- Create: `apps/desktop-ui/src/features/channels/ChannelConfigForm.tsx`
- Create: `apps/desktop-ui/src/features/channels/useChannelLogin.ts`
- Modify: `apps/desktop-ui/src/app/router.tsx`
- Test: `apps/desktop-ui/src/features/channels/ChannelsPage.test.tsx`

**Interfaces:**
- Consumes: `list_channel_accounts`、`begin_channel_login`、`submit_channel_login_code`、`logout_channel_account`、`enable/disable_channel_account`、`send_test_notification`、`channel.login.changed`。
- Produces: 渠道账号列表、登录二维码/配对码对话框、账号隔离设置和测试发送。

- [x] **Step 1: 写登录状态机测试**

```tsx
it("moves from qr to paired without reloading the page", async () => {
  const bridge = createMockHostBridge();
  render(<ChannelsPage bridge={bridge} />);
  await userEvent.click(await screen.findByRole("button", { name: "添加渠道账号" }));
  expect(await screen.findByAltText("渠道登录二维码")).toBeVisible();

  bridge.emit("channel.login.changed", {
    accountId: "account-1",
    state: "Paired",
    message: "登录成功，等待首条入站消息",
  });
  expect(await screen.findByText("等待首条入站消息")).toBeVisible();
});
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
npm --prefix .\apps\desktop-ui run test -- --run ChannelsPage
```

Expected: FAIL，Channels 页面不存在。

- [x] **Step 3: 实现多账号列表**

- 每个账号显示渠道 descriptor、账号名、状态、绑定标识、最近入站、最近投递、启用开关。
- 同一渠道允许多个账号；所有操作明确携带 `accountId`。
- 切换账号后详情和 Query 数据必须重新加载，不沿用上一个账号的状态。
- 删除或退出登录前显示影响：路由和游标会失效，历史保留。
- 测试发送必须选具体账号，不提供隐式默认账号。

- [x] **Step 4: 实现登录对话框**

状态包括：

```text
Idle
Preparing
QrReady
WaitingScan
NeedVerifyCode
WaitingFirstInbound
Paired
Expired
Blocked
Failed
```

- 二维码使用宿主提供的内存图片或 data URL，不写磁盘。
- 配对码使用密码输入框，提交后立即清空输入。
- 关闭对话框不取消已完成的宿主登录任务；重新打开恢复当前状态。
- 过期时提供“刷新二维码”；被阻止时显示中文原因和重试动作。
- 登录成功但尚未建立主动会话时显示“等待首条入站消息”，不能误报可发送。

- [x] **Step 5: 运行门禁并提交**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS。

```powershell
git add apps/desktop-ui
git commit -m "feat(ui): 实现动态渠道账号与登录流程"
```

---

### Task 9: 实现 History、Diagnostics 与 Settings

**Files:**
- Create: `apps/desktop-ui/src/features/history/HistoryPage.tsx`
- Create: `apps/desktop-ui/src/features/history/HistoryTable.tsx`
- Create: `apps/desktop-ui/src/features/history/HistoryDetail.tsx`
- Create: `apps/desktop-ui/src/features/diagnostics/DiagnosticsPage.tsx`
- Create: `apps/desktop-ui/src/features/diagnostics/DiagnosticList.tsx`
- Create: `apps/desktop-ui/src/features/settings/SettingsPage.tsx`
- Create: `apps/desktop-ui/src/features/settings/QuietHoursForm.tsx`
- Create: `apps/desktop-ui/src/features/settings/ReplySettingsForm.tsx`
- Create: `apps/desktop-ui/src/features/settings/UpdateSettings.tsx`
- Modify: `apps/desktop-ui/src/app/router.tsx`
- Test: `apps/desktop-ui/src/features/history/HistoryPage.test.tsx`
- Test: `apps/desktop-ui/src/features/settings/SettingsPage.test.tsx`

**Interfaces:**
- Consumes: History、Diagnostics、Settings commands 和 snapshot 事件。
- Produces: 可筛选历史、可执行诊断和非敏感设置页面。

- [x] **Step 1: 写筛选与 Unknown 测试**

```tsx
it("shows Unknown with an instruction instead of automatic retry", async () => {
  const bridge = createMockHostBridge({
    notifications: [notificationFixture({ deliveryState: "Unknown" })],
  });
  render(<HistoryPage bridge={bridge} />);
  await userEvent.selectOptions(await screen.findByLabelText("状态"), "Unknown");

  expect(await screen.findByText("投递结果未确认")).toBeVisible();
  expect(screen.queryByRole("button", { name: "自动重试" })).not.toBeInTheDocument();
  expect(screen.getByText("请先检查原渠道是否已收到消息")).toBeVisible();
});
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
npm --prefix .\apps\desktop-ui run test -- --run HistoryPage SettingsPage
```

Expected: FAIL，页面不存在。

- [x] **Step 3: 实现 History**

- 使用虚拟列表，首屏支持 10,000 条摘要不冻结。
- 筛选：Agent、渠道、账号、状态、时间范围、正文/标题关键词。
- 表格列：时间、Agent、会话、渠道账号、状态、错误摘要。
- 详情显示标准 Notification、Delivery、Route 是否存在和安全错误。
- `Failed + retryable` 可显式“重试”；`Unknown` 只显示检查指引，不允许自动重试。
- 正文默认折叠；不在日志和前端错误埋点中发送正文。

- [x] **Step 4: 实现 Diagnostics**

诊断项固定来自 StatusService，不在 UI 粗拼：

- 数据库迁移与完整性。
- Credential Manager 可用性。
- ingress 管道与 spool 状态。
- Agent 客户端发现和接入。
- 渠道登录、游标和长连接状态。
- 最近投递错误。
- 应用版本、WebView2 版本和更新状态。

每一项显示 `正常 / 等待 / 异常 / 暂停`、中文说明、最近检查时间和一个可执行修复动作。修复动作只能调用已定义业务命令。

- [x] **Step 5: 实现 Settings**

设置分组：

- 通知：全局暂停、勿扰时段、冷却、Agent 默认开关。
- 回复：引用回复总开关、送达确认、路由有效期。
- 渠道：默认通知账号选择；仍必须显示具体账号 ID。
- 应用：随系统启动、启动时隐藏、更新通道。
- 数据：打开数据目录、导出脱敏诊断包、备份数据库。

修改有校验、保存状态和失败回滚。危险操作必须二次确认；不提供直接 SQL、任意路径或 secret 查看。

- [x] **Step 6: 运行门禁并提交**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
```

Expected: PASS。

```powershell
git add apps/desktop-ui
git commit -m "feat(ui): 实现历史诊断与设置页面"
```

---

### Task 10: 完成状态覆盖、键盘操作和视觉 QA

**Files:**
- Create: `apps/desktop-ui/tests/playwright.config.ts`
- Create: `apps/desktop-ui/tests/navigation.spec.ts`
- Create: `apps/desktop-ui/tests/dynamic-adapters.spec.ts`
- Create: `apps/desktop-ui/tests/states.spec.ts`
- Create: `apps/desktop-ui/tests/visual.spec.ts`
- Create: `apps/desktop-ui/tests/accessibility.spec.ts`
- Create: `apps/desktop-ui/src/test/fixtures.ts`
- Create: `apps/desktop-ui/src/test/renderWithBridge.tsx`
- Modify: `apps/desktop-ui/package.json`
- Modify: `tools/ui/gate.ps1`

**Interfaces:**
- Consumes: mock HostBridge、六个页面和所有稳定状态。
- Produces: 页面流程、空态、错误态、无数据、长文本、200% 缩放和可访问性自动化证据。

- [x] **Step 1: 建立固定测试夹具**

夹具至少提供：

```ts
export const noAgents = { agents: [] };
export const twoAgents = {
  agents: [
    { id: "alpha", displayName: "Alpha Agent", capabilities: { notify: true, resume: true } },
    { id: "future", displayName: "未来 Agent", capabilities: { notify: true, resume: false } },
  ],
};
export const channelStates = [
  { state: "NotLoggedIn" },
  { state: "WaitingFirstInbound" },
  { state: "Ready" },
  { state: "Blocked" },
];
export const longChineseText = "这是一个用于验证中文长文本、数字 1234567890 与错误信息换行的测试标题";
```

- [x] **Step 2: 编写 Playwright 流程测试**

必须覆盖：

- 从总览进入 Agents、Channels、History、Diagnostics、Settings。
- 新增 descriptor 自动出现，无固定 ID 分支。
- 渠道登录从二维码到等待首条入站再到 Ready。
- Unknown 投递不出现重试按钮。
- 错误恢复动作调用正确 command。
- 键盘 Tab 顺序、Enter/Space 操作、Escape 关闭对话框。
- 200% 缩放等价 viewport 下无水平溢出。

- [x] **Step 3: 增加长文本与布局断言**

对以下区域测量并断言不重叠、不裁切：

- 导航标签。
- Agent 名称、状态和配置字段。
- 渠道账号名和长错误。
- History 表格单元格。
- 设置项标签和帮助文本。

不能用 `white-space: nowrap` 配合裁切来通过测试。长单词使用 `overflow-wrap: anywhere`，固定格式控件设置稳定宽高。

- [x] **Step 4: 运行视觉与可访问性门禁**

Run:

```powershell
npm --prefix .\apps\desktop-ui run test:e2e
npm --prefix .\apps\desktop-ui run test:a11y
npm --prefix .\apps\desktop-ui run test:visual
```

Expected: 所有流程通过；视觉截图人工检查后作为基线提交；axe 无 serious/critical 问题。

- [x] **Step 5: 更新完整 UI 门禁**

在 `tools/ui/gate.ps1` 的 Vitest 后追加 Playwright 和视觉检查；渲染快照差异时失败，并要求人工确认后更新基线。

- [x] **Step 6: 提交**

```powershell
git add apps/desktop-ui tools/ui/gate.ps1
git commit -m "test(ui): 覆盖动态页面状态与视觉门禁"
```

---

### Task 11: 接入 Windows 安装、更新与宿主 smoke

**Files:**
- Create: `tools/ui/build-desktop.ps1`
- Create: `tools/ui/smoke-desktop.ps1`
- Create: `installer/agent-notify-rust.iss`
- Create: `hosts/desktop-tauri/src/update/mod.rs`
- Create: `hosts/desktop-tauri/src/update/verify.rs`
- Modify: `hosts/desktop-tauri/src/bridge/commands.rs`
- Modify: `apps/desktop-ui/src/features/settings/UpdateSettings.tsx`
- Test: `hosts/desktop-tauri/tests/update_verify.rs`
- Test: `tests/desktop-installer-smoke.ps1`

**Interfaces:**
- Consumes: Tauri build、WindowsPlatformHost、当前 Release 与签名约定。
- Produces: 独立命名的 Rust 测试安装包、启动 smoke、签名/校验 smoke；正式切换由后续生产闭环计划执行。

- [x] **Step 1: 写更新包拒绝测试**

```rust
#[test]
fn updater_rejects_checksum_mismatch_and_unsigned_package_when_required() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let error = verify_download(
        file.path(),
        "0000000000000000000000000000000000000000000000000000000000000000",
        SignatureRequirement::Required,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_checksum_mismatch");
}
```

- [x] **Step 2: 运行测试并确认失败**

Run:

```powershell
cargo test -p agentnotify-desktop --test update_verify
```

Expected: FAIL，更新校验模块不存在。

- [x] **Step 3: 实现独立构建与签名校验**

- Rust 测试包命名为 `Agent-notify-Rust-Preview-Setup-v<version>.exe`，不得覆盖当前正式安装器。
- `build-desktop.ps1` 先运行 `tools/ui/gate.ps1`，再执行 Tauri build 和 Inno 打包。
- 更新流程沿用渠道：先下载到 `D:\Temp\agentnotify-updates` 或 `%TEMP%` 下 app 专属目录，再校验 SHA-256、PE、版本和 Authenticode。
- 签名要求由宿主配置决定；正式版要求签名，预览测试允许显式关闭但 UI 必须显示“测试包未签名”。
- 更新器不接受任意 URL 或任意安装命令参数。

- [x] **Step 4: 实现安装 smoke**

`desktop-installer-smoke.ps1` 在隔离目录安装预览版并检查：

- 主程序、WebView2 依赖和内嵌资源存在。
- 首次启动创建隔离配置目录。
- 单实例生效。
- 关闭窗口后进程仍存活且托盘入口存在。
- 再次启动唤起窗口。
- 退出后无残留进程。
- 卸载只删除安装文件，保留数据目录。

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\build-desktop.ps1 -Version 2.0.0-dev.0
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\desktop-installer-smoke.ps1 -Installer .\dist\Agent-notify-Rust-Preview-Setup-v2.0.0-dev.0.exe
```

Expected: 安装 smoke 通过，当前正式安装入口未被替换。

- [x] **Step 5: 提交**

```powershell
git add tools/ui hosts/desktop-tauri apps/desktop-ui installer tests
git commit -m "build(desktop): 增加 Rust 预览安装与宿主 smoke"
```

---

## 阶段完成标准

- Tauri + React 可以独立启动，并通过 HostBridge 操作假 Agent 和假渠道。
- UI 页面完全由 DTO、descriptor、capabilities 和 schema 驱动，没有固定 Agent/渠道业务分支。
- 单一主窗口、托盘、单实例、关闭隐藏、自启动和暂停行为通过 smoke。
- Overview、Agents、Channels、History、Diagnostics、Settings 六页完成空态、错误态、长文本和可访问性覆盖。
- Tauri capability 没有 shell、fs、opener 或任意命令权限。
- `tools/ui/gate.ps1`、Playwright、视觉基线、axe 和 Rust workspace 门禁全部通过。
- 预览安装包不覆盖当前 Go 正式版；真实 OpenCode、ClawBot、数据迁移和正式切换由下一份计划完成。
