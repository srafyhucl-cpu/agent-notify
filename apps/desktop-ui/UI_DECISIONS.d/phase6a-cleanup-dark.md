# Phase 6a 决策日志｜收尾清理 + 暗色视觉/无障碍矩阵接入

- 日期：2026-09-24
- 范围：`styles/task7.css`、`styles/task9.css`、`styles/layout.css`（死代码清扫 + 暗色对比度修复）、`features/history/HistoryPage.tsx`、`features/overview/RecentDeliveries.tsx`、`features/diagnostics/DiagnosticsPage.tsx`、`features/diagnostics/DiagnosticList.tsx`（空态文案/CTA）、`tests/playwright.config.ts`（新增 dark 项目）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§0 渠道为主 / §0.2 空态漏斗 / §3 明暗双主题一等公民 / §6 状态）、`docs/superpowers/plans/2026-09-24-ui-ux-redesign-plan.md`（Phase 6 双主题回归）
- 红线：`src/bridge/**`、`src/data/**`、业务逻辑/请求/Query Key/表单提交/路由路径/状态机 —— 零改动；可访问名与 role —— 零改动；`tests/**` 仅新增 `playwright.config.ts` 的 dark 项目，其它测试文件与断言零改动；`.opencode/**` 不碰。

## 1. 死代码清扫（类名 → 引用数 → 动作）

扫描口径：`src/**`（`.tsx/.ts`）+ `tests/**`（`.ts`）+ `src/styles/*.css` 全量精确匹配（正则加词边界，避免 `runtime-status-label` / `history-error-cell` 之类子串误判）。

| 类名 | 定义 | 引用数 | 动作 |
| --- | --- | --- | --- |
| `.page-summary` | task7.css | 0（已被 `pattern-page-header-summary` 取代） | **删除** |
| `.status-label`、`.status-label--success`、`.status-label--danger` | task7.css | 0 | **删除**（含从 `.page-count, .status-label, .delivery-state` 共享组中拆出） |
| `.delivery-state`、`--sent/--failed/--unknown/--pending/--skipped` | task7.css | 0 | **删除** |
| `.page-count` | task7.css | 2（`AgentsPage.tsx:55`、`HistoryPage.tsx:145`） | 保留（原共享组收敛为单条 `.page-count` 规则，声明合并、值不变） |
| `.section-count` | task7.css | 0 | **删除**（从 `.section-count, .section-description` 组中移除） |
| `.section-description` | task7.css | 1（`HistoryDetail.tsx:113`） | 保留 |
| `.section-heading`（含 `> div` / `> .button` / `h2`） | task7.css | 1（`HistoryDetail.tsx:110`） | 保留 |
| `.table-scroll` | task7.css | 0 | **删除** |
| `.data-table`（含 `th/td`、`th`） | task7.css | 0 | **删除** |
| `.monospace-cell` | task7.css | 3（AgentDetail / ChannelAccountDetail / RecentDeliveries） | 保留 |
| `.error-cell` | task7.css | 0（`history-error-cell` 是 task9 的独立类，未受影响） | **删除** |
| `.metric-list`（含 `> div` / `dt` / `dd`） | task7.css | 3（`HealthSummary.tsx`） | 保留 |
| `.action-list`（含 `li` / `strong` / `span`） | task7.css | 1（`OverviewPage.tsx:256`） | 保留 |
| `.section-empty` | task7.css | 7（六页 + 历史详情） | 保留 |
| `.page-actions` | task9.css | 0 | **删除** |
| `.existence-value--yes` / `--no` | task9.css | 1（`HistoryDetail.tsx:45`） | 保留 |
| `.history-existence-list` / `.diagnostics-summary-list`（共享块） | task9.css | 各 1（HistoryDetail / DiagnosticsPage） | 保留：两侧专属覆盖只改 `margin-bottom`，未整体取代共享基样式 |
| `.empty-state*` | layout.css | 5（`EmptyState.tsx`） | 保留 |
| `.inline-error*` | layout.css | 6（`InlineError.tsx`） | 保留（背景硬编码另修，见 §4） |
| `.loading-*` | layout.css | 0（骨架定义已在 Phase 3 迁至 `patterns.css`，layout.css 仅留注释） | 无定义可删 |

- 删除只针对明确选择器块；`.page-count` 因与已死选择器共享规则，按最小改动合并为单条规则（声明与取值不变），未做无关格式化。
- **额外发现（未处理，仅记录）**：`styles/global.css` 全仓库无任何 `import`，是一份整体死文件（且含 `#20242a/#f3f4f6/#24313a` 等硬编码色）。它不在本阶段候选清单内，为避免擅自扩大范围，未删除、未改动。

## 2. 空态引导一致性（设计稿 §0.2）

| 位置 | 前 | 后 | CTA |
| --- | --- | --- | --- |
| 历史页空态（`HistoryPage.tsx`） | 标题「暂无历史通知」+「当前筛选条件下没有通知记录。可以调整筛选条件后重新查看。」 | 标题不变；描述改为「当前筛选条件下没有通知记录。连接渠道并触发通知后，这里才会出现投递记录。」 | `EmptyState` 的 `action` 槽：`<Link className="button" to="/channels">去连接渠道</Link>` |
| 总览-最近投递空态（`RecentDeliveries.tsx`） | `<p className="section-empty">最近没有投递记录。</p>` | 改用 `EmptyState`：标题「最近没有投递记录」+ 描述「连接渠道并触发通知后，这里才会出现投递记录。」 | 同上 `<Link to="/channels">去连接渠道</Link>` |
| 诊断-迁移未检测（`DiagnosticsPage.tsx`） | 「已检查常见旧版安装位置，未发现可迁移的数据。」 | 「已检查常见旧版安装位置，未发现可迁移的数据，无需处理。」 | 无（诊断空态不加链接） |
| 诊断-诊断项空（`DiagnosticList.tsx`） | 「StatusService 当前没有返回诊断项。」 | 「StatusService 当前没有返回诊断项；运行与存储检查未发现问题，因此没有需要处理的项。」 | 无 |
| 诊断-组件状态空（`DiagnosticsPage.tsx`） | 「StatusService 未返回组件状态。」 | 「StatusService 未返回组件状态；当前没有可上报的组件，无需处理。」 | 无 |
| 诊断-动作不可用（`DiagnosticList.tsx`） | 「当前没有可执行的修复动作」 | **保持原样**（被 `SettingsPage.test.tsx:467` 精确断言，测试零改动红线） | 无 |

- 未新增按钮/表单；未改 `EmptyState` / `EmptyFunnel` 组件本身，只传 props。
- CTA 用 react-router `<Link>`（非 `<button>`），复用既有 `.button` 类，落到 `EmptyState` 已支持的 `action` 槽；`/channels` 路由路径未变。

## 3. 暗色矩阵接入（`tests/playwright.config.ts`）

新增 `dark` 项目（`desktop` / `zoom-200` 保持 `colorScheme: "light"` 不动）：

```ts
{
  name: "dark",
  grep: /@(a11y|visual)/,
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    colorScheme: "dark",
  },
}
```

- 与 `desktop` 项目同视口（1280×800）/ 同 `deviceScaleFactor`（1）/ 同 devices；仅切换 `colorScheme`，专跑 `@a11y` 与 `@visual`。
- 矩阵：light 由 `desktop` 项目承担，dark 由本项目承担 → a11y = 7（light）+ 7（dark）= 14。

## 4. 暗色 a11y 结果与修复（对比度核算）

### 4.1 首次 dark 结果（修复前）

`14 tests` 中 **13 passed / 1 failed**：`[dark] @a11y 渠道` 报 `color-contrast`（serious，2 处）：

```
- .schema-field--unsupported > .schema-field-label
- .schema-field-unsupported
```

根因：`task7.css` 的 `.schema-field--unsupported { background: #fff8e8 }` 是硬编码浅底，暗色下 `--color-warning: #f0b64a` 落在近白底上对比度仅 **1.73:1**。

### 4.2 修复

| 文件 | 旧值 | 新值 |
| --- | --- | --- |
| `task7.css` `.schema-field--unsupported` | `background: #fff8e8`（硬编码） | `background: var(--color-warning-soft)`（语义柔和底，两主题自动换肤） |
| `layout.css` `.inline-error` | `background: #fbefed`（硬编码，潜在暗色缺陷） | `background: var(--color-danger-soft)`（语义柔和底） |

`.inline-error` 的暗色缺陷未被本次 axe 命中（默认场景不渲染错误态），属**主动修复**：旧值在暗色下文字 `--color-text #e9edf6` 落在近白 `#fbefed` 上对比度仅 **1.04:1**，与设计稿「组件只消费 `var(--*)`，零硬编码」及「两套主题都要过 axe 对比度」冲突，故一并语义化。

### 4.3 对比度核算（WCAG 相对亮度；背景按表面语义层合成）

| 场景 | 前景 | 背景（合成后） | 修复前 | 修复后 |
| --- | --- | --- | --- | --- |
| 不支持字段 暗色 | `--color-warning #f0b64a` | `warning-soft` over surface-1 over canvas ≈ `rgb(52,44,34)` | **1.73:1（红）** | **7.52:1** |
| 不支持字段 亮色 | `--color-warning #9a5b06` | `warning-soft` over `#fff` ≈ `rgb(243,235,225)` | 5.12:1 | **4.59:1**（仍 ≥4.5，非退化到不达标） |
| InlineError 正文 暗色 | `--color-text #e9edf6` | `danger-soft` over surface-1 over canvas ≈ `rgb(54,37,43)` | **1.04:1（红）** | **12.30:1** |
| InlineError 说明 暗色 | `--color-text-muted #949eb4` | 同上 | — | 5.36:1 |
| InlineError 正文 亮色 | `--color-text #1a2230` | `danger-soft` over `#fff` ≈ `rgb(246,229,227)` | 13.1:1 | 13.10:1 |
| InlineError 说明 亮色 | `--color-text-muted #5b6675` | 同上 | — | 4.78:1 |

- **未改动 `tokens.css`**：无需调整暗色令牌值。
- 亮色「不支持字段」由 5.12 降到 4.59（仍 ≥4.5:1 正文阈值），换来的收益是暗色从 1.73（不达标）升到 7.52；取舍为“两主题都达标”的硬约束。
- 已知可接受项（记录、不修）：`channels.css` 的 `--channels-qr-surface: #ffffff` 是二维码白底，业务上必须恒白，属设计稿允许的例外。

### 4.4 修复后 dark 结果

`14 passed`（light 7 + dark 7，含登录对话框 Escape 一条），无 serious/critical。

## 5. visual 两组计数与 diff（未刷新任何基线）

`npm run test:visual`（`--grep @visual`）：

- **light（desktop 项目）6 张红**（页面重设计，预期）——diff 像素 / 比例：
  - `overview` 21739 px（ratio 0.03）
  - `agents` 32018 px（ratio 0.04）
  - `channels` 52349 px（ratio 0.06）
  - `history` 14088 px（ratio 0.02）
  - `diagnostics` 32485 px（ratio 0.04）
  - `settings` 32534 px（ratio 0.04）
- **dark 6 张缺基线**（预期）：`overview/agents/channels/history/diagnostics/settings` 各缺 `*-dark-win32.png`。
- 其余 2 条 `@visual` 非截图断言（长文本不裁切/不重叠，light + dark）= `2 passed`。
- 计数说明：任务书预期 dark「12 张缺基线」，实测为 **6 张**。原因是 `visual.spec.ts` 每主题仅 6 条 `toHaveScreenshot`（六页视觉基线），另 2 条 `@visual` 不产基线；`zoom-200` 项目不匹配 dark 的 `grep`。已如实报告实测值。
- **未刷新基线**：Playwright 默认 `updateSnapshots: 'missing'` 会在缺基线时自动写出 PNG；本次跑完后已删除自动生成的 6 个未跟踪 `*-dark-win32.png`，`tests/visual.spec.ts-snapshots/` 仅保留原有 6 个 light 基线，工作区无基线变更。

## 6. 自主决策清单

1. **`.page-count` 共享规则合并**：原 `.page-count, .status-label, .delivery-state` 中两个选择器已死，按最小改动把声明合并进单条 `.page-count` 规则（值不变），避免留下重复选择器。
2. **`global.css` 不删**：确认为全仓库无 import 的死文件，但不在本阶段候选清单，记录不动（不擅自扩大范围）。
3. **总览-最近投递空态改用 `EmptyState`**：为把 CTA 放进组件既有的 `action` 槽（任务指定落点），将原 `<p>` 替换为 `EmptyState`；不新增按钮、不改组件本身。
4. **诊断空态不加链接**：按任务要求只补因果句；「当前没有可执行的修复动作」被 Vitest 精确断言，保持原样。
5. **主动修 `.inline-error` 硬编码底**：暗色对比度 1.04:1 是明确缺陷，且属本阶段拥有的 `styles/*.css`；语义化后两主题均达标（§4.3）。
6. **亮色「不支持字段」对比度 4.59:1**：接受该值（≥4.5:1），优先保证暗色达标；如后续要更大余量，可单独给 `.schema-field-label` 用 `--color-text`。

## 7. 门禁结果（最终态，全绿）

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built`。
- `PLAYWRIGHT_BROWSERS_PATH=D:\Tools\playwright-browsers npm --prefix apps/desktop-ui run test:a11y`：**`14 passed`**（light 7 + dark 7）。
- `test:visual`：12 failed（light 6 红 + dark 6 缺基线，均为预期）/ 2 passed；**未刷新基线**（§5）。

## 8. 未碰清单（零改动声明）

- 未改：`src/bridge/**`、`src/data/**`、`app/**`、`components/**`（含 `EmptyState`/`EmptyFunnel`/`patterns/**`）、`styles/patterns.css`、`styles/tokens.css`、`styles/channels.css`、`styles/reset.css`、`styles/global.css`。
- 业务逻辑、数据结构、数据请求、TanStack Query Key、表单提交、路由路径、业务状态机：零改动。
- 可访问名与 role：零改动（空态仅新增链接文案「去连接渠道」，原有名称/role 全部保持）。
- `tests/**`：仅 `tests/playwright.config.ts` 新增 dark 项目；其它测试文件与断言零改动。
- 视觉基线：零刷新（自动生成的 dark 基线已删除）。
