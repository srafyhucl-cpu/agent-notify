# Phase 5b 决策日志｜总览页迁移

- 日期：2026-09-24
- 范围：`src/features/overview/OverviewPage.tsx`、`HealthSummary.tsx`、`RecentDeliveries.tsx`、`styles/task7.css`（仅总览段）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§0 渠道为主 / §1 五原则 / §5.3 总览：KPI 锚点带 → 例外面板 → 投递流）、阶段 4 渠道页与阶段 5a Agent 页作为同款结构语言与密度样例
- 红线：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`（含 RuntimeStatusBar 联动）、`components/patterns/**`、`patterns.css`、`tokens.css`、`layout.css`、`task9.css`、`tests/**` —— 零改动；`task7.css` 只动总览段。

## 1. 结构映射（旧区块 → 新范式）

| 旧结构 | 新结构（范式组件） | 说明 |
| --- | --- | --- |
| `workbench-page-header`（h1 + `page-summary` + 主按钮） | `PageHeader`（h1「总览」+ 摘要 + `actions` = 原「重新检查」按钮） | 文案与按钮可访问名原样保留 |
| 无 | **KPI 锚点带** `overview-kpi-band`：4 张等宽 `KpiCard`（渠道账号健康居首 → Agent 接入 → 运行状态 → 待处理） | 新增；`grid` + `auto-fit minmax(200px,1fr)`，窄窗自动折行 |
| `div.health-summary` 三列（`.overview-section` × 3） | `div.overview-health` 三列（`.overview-health-section` × 3，h2 + `metric-list`） | 明细层；h2 文案与暂停操作位置不变 |
| `section.overview-section`（h2「需要处理」+ `section-count` + `action-list`） | `SectionCard`「需要处理」（`count` = 项数，body 内同一 `action-list`） | 标题/计数/列表内容不变 |
| `section.overview-section`（h2「最近投递」+ `data-table` 5 列表格） | `SectionCard`「最近投递」（`count` = 条数，body 内 `.delivery-stream` 时间分组行） | 表格 → 分组行；见 §5 |
| `span.delivery-state--*` | `StatusBadge`（tone success/warning/danger） | 颜色 + 文字双载；文案「等待发送 / 已发送 / 发送失败 / 结果未知 / 已跳过」不变 |
| `workbench-page-title` + `page-summary` | `pattern-page-header-title` + `pattern-page-header-summary` | 由 `PageHeader` 承接 |
| 外层 `section aria-labelledby="page-title-overview"` | 外层 `section aria-label="总览"` | `PageHeader` 不暴露 h1 id，改用 `aria-label`；可访问名「总览」不变 |

## 2. KPI 指标来源（逐条对应现有数据字段，不发明新度量）

| KPI 卡 | 数值锚点 | 数据来源 | 语义色（tone） |
| --- | --- | --- | --- |
| 渠道账号健康（首位） | 在线数；旁缀 `/ 总数`；标签内含「等待登录 N · 失效 M」 | `snapshot.overview.channels`，经 `channelOnline` / `channelWaitingLogin` / `channelInvalid`（`HealthSummary.tsx` 既有纯函数，本阶段仅导出复用） | 无渠道=default；失效>0=danger；等待>0=warning；否则 success |
| Agent 接入 | 已接入数；旁缀「· N 异常」（异常>0 时） | `snapshot.overview.agents.length`、`!health.available` 计数 | 异常>0=danger；有 Agent=success；否则 default |
| 运行状态 | `RUNTIME_STATE_LABELS[snapshot.runtime.state]` | `snapshot.runtime.state` | Running=success；Failed/Stopped=danger；其余=warning |
| 待处理 | `items.length`（`actionItems(snapshot)` 项数） | `snapshot.overview.*` + `snapshot.diagnostics`（既有 `actionItems` 逻辑，零改动） | >0=warning；=0=success |

- `KpiCard` 只消费 `--surface-1*` 与 `--color-*`；28px 数字（`--font-display`）是全页唯一视觉锚点；tone 只作用于状态点与旁缀，不整卡变色。
- 健康明细三段的 `dd` 用 `--font-body`（14px）而非 28px，确保 28px 仍是唯一锚点层级。

## 3. 契约保持清单（逐条 role / 可访问名 + 测试出处）

| 契约 | 出处 |
| --- | --- |
| h1「总览」（`PageHeader`，每页唯一） | `tests/helpers.ts:gotoSection` / `openHarness`；`AppShell.test.tsx:92`（每个稳定路由 h1 名 = 导航标签）；`visual.spec.ts:181` 等 |
| h2「运行状态」，且其 `closest("section")` 内含暂停/恢复按钮 | `AppShell.test.tsx:207-268`（`overviewRuntimeRegion()` = `heading("运行状态").closest("section")`） |
| h2 顺序：运行状态 → Agent 接入 → 渠道账号 → 最近投递 → 需要处理 | `AgentsPage.test.tsx:249-288`（`findAllByRole("heading")` 位置单调递增） |
| 按钮「暂停通知」/「恢复通知」（同一可访问名，Overview 与状态栏各一处，测试用 `within` 区分） | `AppShell.test.tsx:133/141/148/228-236/256-264` |
| FieldRow 文案「运行状态 / 通知 / 版本 / 平台 / 已接入 / 异常 / 在线 / 等待登录 / 失效」 | `AgentsPage.test.tsx` 顺序断言间接依赖 h2；明细文案无独立断言，原样保留 |
| 「最近投递」20 条上限：`notification-1` 可见、`notification-21` 不存在 | `AgentsPage.test.tsx:286-287`（`MAX_RECENT_DELIVERIES = 20` 不变） |
| 空态文案「最近没有投递记录。」「当前没有需要处理的故障。」 | 无独立断言，原样保留 |
| `role="alert"` 错误提示（`InlineError`，标题「无法读取总览」+ 动作「重新检查」） | `AppShell.test.tsx:151-169`（状态栏路径）；本页同款保留 |
| `role="status"` 加载态（`LoadingRows aria-label="正在加载总览"`） | `AppShell.test.tsx:191-193` 通用形态 |
| axe：总览无 serious/critical | `accessibility.spec.ts:11`（`@a11y 总览` 通过） |
| 长文本不裁切 / 无水平溢出（200% 缩放） | `visual.spec.ts:42-101`（`@visual 长文本…`、`@zoom 200%…` 通过） |
| 外层 region 可访问名「总览」 | 无独立断言，用 `aria-label="总览"` 保持 |

## 4. 暂停/恢复联动保持说明（零改动）

- 暂停逻辑仍在 `OverviewPage.togglePause`（调用 `useSetRuntimePausedMutation`）→ 传给 `HealthSummary` 的 `onTogglePause`，按钮仍在 h2「运行状态」所在 `<section>` 内。
- 与顶部 `RuntimeStatusBar` 的联动由 `data/` 的 snapshot 事件驱动，**本阶段未触碰**；`AppShell.test.tsx` 两条双向联动断言（状态栏→总览、总览→状态栏）均通过。
- 仅重排视觉：按钮从旧 `overview-section-header` 的右槽移到 `overview-health-header` 的右槽，可访问名、`disabled` 条件（`pausePending`）、文案分支不变。

## 5. 投递流重排（表格 → 时间分组行）

- 按 `DeliveryDto.updatedAt` 推导分组：**今天 / 昨天 / 更早**（本地日界；无效时间或早于昨天归「更早」）。组标签为 h3，非空组才渲染。
- 行内结构：标题（`notificationId`，等宽 + 单行省略）/ 摘要（有错误显示错误信息，否则「账号 {accountId}」，单行省略）/ 相对时间（刚刚 / N 分钟前 / N 小时前 / N 天前，超 30 天退回绝对时间）/ `StatusBadge`。
- 仍只展示 `MAX_RECENT_DELIVERIES = 20` 条，满足 `notification-21` 不存在断言。
- 状态映射：Sent=success；Failed/Unknown=danger；Pending/Skipped=warning；文案不变。

## 6. 清掉的硬编码 / 游离间距清单（`task7.css` 总览段）

| 旧值 | 位置 | 新值 |
| --- | --- | --- |
| `min-width: 72px` | `.metric-list > div` | `min-width: 0`（去掉魔法数字） |
| `grid-template-columns: minmax(160px, 0.45fr) …` | `.action-list li` | 单列 `grid`（`gap: var(--space-1)`），去掉 160px |
| `var(--font-size-1/2/3/4)` | 总览段字号 | 别名 `--font-caption` / `--font-body` / `--font-heading`（`--font-display` 由 `KpiCard` 提供） |
| `var(--color-border)` 通栏上/下/右细线 | `.health-summary` / `.overview-section` | 健康明细去卡片边线；列表分割线改 `var(--hairline)` |
| 无（本段无 hex） | — | 总览段零 hex：grep 确认 `#e3efeb` / `#f7f8fa` 已不存在于 `task7.css`；`#aeb5af`（`.switch-track`）、`#fff8e8`（`.schema-field--unsupported`）属 Agent 段，未动 |

## 7. 共享段的处置说明（`task7.css`）

**仅从共享选择器列表中移除「仅总览使用」的选择器（不改共享规则本身）：**

| 共享规则（原） | 处置 | 理由 |
| --- | --- | --- |
| `.overview-page-content, .agents-page-content { … }` | 移除 `.overview-page-content,`，保留 `.agents-page-content` | `.overview-page-content` 仅总览使用；总览改在总览段自建 `grid gap: --space-5` |
| `.overview-section-header, .section-heading { … }` | 移除 `.overview-section-header,`，保留 `.section-heading` | `.overview-section-header` 仅总览使用，已被 `.overview-health-header` 取代 |
| `.overview-section h2, .section-heading h2 { … }` | 移除 `.overview-section h2,`，保留 `.section-heading h2` | `.overview-section h2` 仅总览使用；`.section-heading h2` 仍被诊断/历史使用 |

**保留不动的共享规则（未改一行）：** `.section-heading`、`.section-heading > div`、`.section-heading > .button`、`.section-heading h2`、`.section-count`、`.section-description`、`.section-empty`、`.table-scroll`、`.data-table*`、`.monospace-cell`、`.error-cell`、`.page-count` / `.status-label` / `.delivery-state*`。
- `.monospace-cell` 仍被 `ChannelAccountDetail` / `AgentDetail` 复用；投递流行标题复用它。
- `.delivery-state*` / `.data-table*` / `.table-scroll` / `.error-cell` 在总览改造后已不再被任何页面引用，但它们在共享选择器列表中与 `page-count` / `status-label` 同组；为不改动共享规则、并避免与并行阶段冲突，**保留为死代码并在本日志标注**，留待 Phase 6 统一收口。
- `.section-empty` 仍被总览（两处空态）与诊断/渠道/历史/设置复用。

## 8. 自主决策清单

1. **KPI 锚点带采用「无标题卡」+ 保留健康明细三段**：`AgentsPage.test.tsx:249` 的 OverviewPage 顺序断言把 h2 顺序钉死为「运行状态 → Agent 接入 → 渠道账号 → 最近投递 → 需要处理」。若让 KPI 卡自带 h2，则「渠道健康居首」与「运行状态居首」不可兼得。因此 KPI 带用无标题的 `KpiCard`（设计顺序：渠道健康居首），h2 由下方健康明细三段承接，两者互不干扰。
2. **`channelOnline/WaitingLogin/Invalid` 从 `HealthSummary.tsx` 导出复用**：KPI 带与明细段需要同一套健康判定，导出既有纯函数避免重复逻辑；未新建文件（遵守文件所有权清单）。
3. **例外面板未前置（与设计 §5.3 的已知偏差）**：`AgentsPage.test.tsx` 断言「最近投递」必须在「需要处理」之前。该测试位于 `src/features/agents/`（红线：其它 features 零改动）且属 `tests` 契约，本阶段不能改。为保门禁全绿，保留 DOM 顺序「投递流 → 需要处理」；作为补偿，把「待处理」提到 KPI 锚点带（tone 在 >0 时为 warning），在首屏就给出「有没有要处理的」信号。**此偏差需在 Phase 6 与测试归属方确认后再决定是否调序。**
4. **KPI 带用 `auto-fit minmax(200px,1fr)`**：无需媒体查询即自动折行（1280 视口 4 列、640 视口 2 列），避免魔法断点。
5. **健康明细三段不再用卡片表面**：仅 KPI 带与两张 `SectionCard` 使用表面层，符合「少边框、多表面差 / 一屏一锚点」；明细段 h2 用 `--font-heading` 但数值降为 `--font-body`，不抢 28px 锚点。
6. **投递流分组用本地日界**：以 `new Date().getTime()` 为「今天」基准，`startOfDay` 用本地时区；无效时间归「更早」，避免渲染失败。
7. **相对时间阈值命名常量**：`MINUTE_MS / HOUR_MS / DAY_MS / MONTH_DAYS`，无裸魔法数字。
8. **外层 section 用 `aria-label="总览"`**：`PageHeader` 不暴露 h1 id，沿用阶段 5a 的 `aria-label` 做法，保持「命名 region」语义与可访问名不变。
9. **健康明细三段仍渲染 h2 图标**：`Gauge` / `Bot` / `RadioTower` 图标原样保留（`aria-hidden`）。

## 9. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`（含 `OverviewPage` 顺序断言）。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built in 1.80s`。
- `PLAYWRIGHT_BROWSERS_PATH=… npm --prefix apps/desktop-ui run test:a11y`：`7 passed`（六页 + 登录对话框 Escape）。
- 额外实跑（`npx playwright test --config tests/playwright.config.ts --grep-invert "视觉基线"`）：`19 passed`（navigation / states / dynamic-adapters / a11y / `@visual 长文本…` / `@zoom 200%…`）。
- 视觉基线红为预期（`overview.png` diff ratio 0.02），未更新基线。

## 10. 未碰清单（零改动声明）

- 未改：`src/bridge/**`、`src/data/**`、`app/**`（含 `RuntimeStatusBar` 联动）、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task9.css`、`tests/**`、其它 `features/**`（含 `AgentsPage.test.tsx`）、`.opencode/`。
- 业务逻辑、数据结构、数据请求、Query Key、表单提交、路由路径、状态机：零改动（`actionItems`、`togglePause`、`useSnapshot`、`useSetRuntimePausedMutation` 均原样）。
- 可访问名与 role：零改动（§3 全部保持；h1/h2/按钮/FieldRow 文案/`role="alert"`/`role="status"` 不变）。
- `task7.css` 仅动总览段；Agent 段与其余共享规则零改动。
