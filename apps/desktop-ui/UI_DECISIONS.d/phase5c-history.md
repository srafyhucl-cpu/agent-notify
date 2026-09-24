# Phase 5c 决策日志｜历史页迁移（检索报表 · 筛选带收拢 + 固定轨道表 + 详情三段）

- 日期：2026-09-24
- 范围：`src/features/history/HistoryPage.tsx`、`HistoryTable.tsx`、`HistoryDetail.tsx`、`styles/task9.css`（仅历史段：`.history-*` / `.existence-*`）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§1 五原则 / §4 范式分配 / §5.4 历史：筛选带收拢、固定列轨道、虚拟列表防白闪、双栏底对齐）
- 样例：阶段 4 渠道页（`ChannelsPage.tsx` / `channels.css`）与阶段 5a Agent 页（`AgentsPage.tsx` / `phase5a-agents.md`）作为同款结构语言与视觉密度参照
- 红线：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task7.css`、`tests/**`、`.opencode/**` —— 零改动；`task9.css` 只动历史段。

## 1. 结构映射（旧区块 → 新范式）

| 旧结构 | 新结构（范式组件） | 说明 |
| --- | --- | --- |
| `workbench-page-header`（h1 + `page-summary` + `page-count` 徽标） | `PageHeader`（h1「历史」+ 摘要 + `actions` = 原 `page-count`） | 计数徽标移入 actions，`aria-label="共 N 条通知"` 原样保留 |
| 页面 section `aria-labelledby="page-title-history"` | `<section className="workbench-page history-page" aria-label="历史">` | 命名 region 语义保留（`PageHeader` 不暴露 h1 id） |
| 单一 `.history-filters` 8 列网格（主/次筛选混排、重置独占一格） | `.history-filter-primary`（Agent/渠道/账号/状态）+ `.history-filter-secondary`（开始/结束/关键词 + 重置行尾） | 见 §2 |
| `div.history-workspace`（`1.65fr / minmax(320px,.85fr)`，gap 16） | `minmax(0, 2fr) / minmax(300px, .85fr)`，gap 24，`align-items: start` | 见 §5 |
| 表格行内 `span.history-state history-state--*` | `span.history-state-cell[role=cell]` > `StatusBadge`（success/warning/danger/neutral） | 颜色 + 文字双载；`role="cell"` 保留 |
| 详情 `span.history-state history-state--neutral` / `--*` | `StatusBadge tone="neutral"` / `deliveryStateTone()` 返回 tone | 语义色统一 |
| 详情平铺（h2 头 + `dl` + 正文 div + 投递 div） | 详情头（`.section-heading`）+ 三张 `SectionCard`：存在性 / 正文 / 投递记录 | 见 §4 |
| `div.history-deliveries > h3「投递记录」` | `SectionCard title="投递记录" count={deliveries.length}` | h3 → 卡片 h2；文案「投递记录」保留 |
| 空态 `EmptyState`「暂无历史通知」 | **原样保留**（文案 / role 不变） | 见 §6 决策 4 |
| 页面纵向单列 | `history-workspace` 左表 / 右详情，≤1120px 降单列 | 同渠道页 |

## 2. 筛选带重排方案

- 主筛选（一行）：Agent / 渠道 / 账号 / 状态，放入 `.history-filter-primary`（`flex-wrap`，`> label { flex: 1 1 170px }`）——宽屏一行四列，窄屏自然折行。
- 次筛选（第二行）：开始时间 / 结束时间 / 关键词，放入 `.history-filter-secondary`；**「重置」并入行尾**（`margin-left: auto`），不再独占一行。
- 所有 `label` 仍走既有共享规则 `.history-filters label { display:grid; gap:4px }` 与 `.history-filters input/select`，控件视觉零变化。
- 表单 `aria-label="历史筛选"`、`onSubmit preventDefault`、各字段 `aria-label`（Agent/渠道/账号/状态/开始时间/结束时间/关键词）、重置按钮文案「重置」与 `disabled={!hasActiveFilters(filters)}` 全部原样保留。
- 渠道切换清空账号的联动、`filters` 的 `useMemo`、`dateTimeToIso`/`valueOrNull` 等逻辑零改动。

## 3. 虚拟列表 CSS 防白闪 / 防抖动清单（**JS 零改动**）

> `HistoryTable.tsx` 中 `useVirtualizer` 的 `count` / `getScrollElement` / `estimateSize`（54）/ `overscan`（8）/ `initialRect` / `getItemKey` 与 `HISTORY_ROW_HEIGHT` / `HISTORY_OVERSCAN` / `TEST_VIEWPORT_RECT` / `INITIAL_FALLBACK_ROW_COUNT` **一行未改**；仅替换了状态单元格的展示组件与列轨道 CSS。

| 目标 | 旧值 | 新值 |
| --- | --- | --- |
| 容器明确高度 | `height: min(58vh, 560px)`（内联魔法） | `height: var(--history-viewport-height)`（`min(58vh, 560px)`，命名 token） |
| 行底透明防白闪 | `.history-virtual-space .history-row { background: var(--color-surface) }`（与视口同色但显式覆盖，选中行需 `!important`） | 删除行背景 → 行底透明，统一由视口 `background: var(--color-surface)` 承载 |
| 选中态 | `background: #e7f0ed !important`（硬编码 + `!important`） | `background: var(--color-primary-soft)`（去 `!important`） |
| 防抖动 | 无 | `scrollbar-gutter: stable` |
| 滚动条透明轨道 | 无 | `scrollbar-width: thin`；`scrollbar-color: var(--color-border) transparent`；`::-webkit-scrollbar-track { background: transparent }`，宽度走 `--history-scrollbar-size` |

## 4. 详情三段落地

- 结构：`section.history-detail[aria-label="通知详情"]`（`display:grid; gap:24px; align-content:start; align-self:stretch; overflow:auto`）→ 头部 `.section-heading`（h2 = 通知标题 + 时间 + `StatusBadge`）→ `SectionCard`「存在性」/「正文」/「投递记录」。
- 存在性段：保留 `dl.history-existence-list` 与 `dt`「通知 / 投递 / 路由」、`dd`「存在（N 条）/ 不存在」、`.existence-value--yes/no`（语义 token），仅外层套 `SectionCard`，并去掉列表自带底距（卡片已提供间距）。
- 正文段：保留「展开正文 / 收起正文」按钮与 `pre.history-body`，套 `SectionCard title="正文"`；去掉旧的分隔线/外边距。
- 投递段：`SectionCard title="投递记录"`，投递卡片内状态改 `StatusBadge`；错误条 `.history-delivery-error` 与 `InlineError` **同源**（`--color-danger-soft` 底 + `--color-danger` 左侧描边 + 语义色文字）。
- **底部对齐**：`history-workspace` 保持 `align-items: start`（顶对齐），详情面板 `align-self: stretch` 撑满同一行高（左列 = 表头 38px + 虚拟视口 + 加载更多），面板内部 `overflow:auto`；≤1120px 单列时 `align-self:auto; overflow:visible` 恢复自然高度。这样避免「右侧详情远短于左侧列表」的大片参差。

## 5. 表格固定列轨道与重叠修复（自主决策）

- 旧列轨道：`132px / 110px / minmax(190px,1.15fr) / minmax(170px,1fr) / 132px / minmax(180px,1.15fr)`，最小内容宽约 970px，**大于左列实际宽度**（约 660px），在宽屏下会越入右列、被旧详情面板的不透明底遮住（旧基线即如此）。
- 新列轨道：`132px / minmax(0,1.1fr) / minmax(0,1.6fr) / minmax(0,1.2fr) / 112px / minmax(0,1.4fr)`，时间/状态定宽、其余按比例分配，行 `min-width: 0`，**始终落在左列内**；超长文本按设计走「`overflow:hidden` + `text-overflow:ellipsis`」省略号截断。
- 同步删除 ≤1120px 的 `.history-row { min-width: 920px }` 与 `.history-table { overflow-x:auto }`（单列下不再需要，也不产生内层横向滚动）。
- 保留 `.history-row > * { word-break: keep-all }`（中文不断字）与 ≤1120px 表头吸顶 `position: sticky`。

## 6. 契约保持清单（role / 可访问名 → 测试出处）

| 契约 | 出处 |
| --- | --- |
| h1「历史」 | `tests/helpers.ts:gotoSection`（每页 h1 名 = 导航标签） |
| 筛选 `aria-label`：Agent / 渠道 / 账号 / 状态 / 开始时间 / 结束时间 / 关键词 | `HistoryPage.test.tsx:146/167-177` |
| 「全部 Agent / 全部渠道 / 全部账号 / 全部状态」option 与提交 payload 逐字段 | `HistoryPage.test.tsx:180-190` |
| 行内标题按钮可访问名 = `notification.title`、`aria-pressed` | `HistoryPage.test.tsx:236/293/336`；`states.spec.ts:48/65` |
| 长文本标题按钮 `getByRole("button",{name:longChineseText})` 不被裁切 | `visual.spec.ts:78-82` |
| `role="table"` `aria-label="历史通知列表"`、虚拟化（<200 行 / 不渲染第 10000 行） | `HistoryPage.test.tsx:194-206` |
| `role="region"` `aria-label="通知详情"` | `HistoryPage.test.tsx:244/295/338`；`states.spec.ts:50` |
| 存在性文案「通知 / 投递 / 路由」「存在（1 条）」「不存在」 | `HistoryPage.test.tsx:238-242` |
| 状态中文「投递结果未确认」「请先检查原渠道是否已收到消息」 | `HistoryPage.test.tsx:148-153/299-306`；`states.spec.ts:52-58` |
| 「展开正文」/「收起正文」按钮与正文 `pre` 显隐 | `HistoryPage.test.tsx:248-251` |
| 重试按钮「重试」仅在 Failed 且 retryable 出现；Unknown/不可重试不出现；`retry_delivery` payload | `HistoryPage.test.tsx:253-261/305/345`；`states.spec.ts:57/68-71` |
| 失败错误文案「渠道拒绝了这条消息，请检查账号权限」「账号权限不足，需要人工处理」 | `HistoryPage.test.tsx:246/342`；`states.spec.ts:66` |
| 长文本场景无水平溢出 | `visual.spec.ts:82`；`@zoom` 200% 六页 |
| axe：历史无 serious/critical | `accessibility.spec.ts:11` |
| `page-count` `aria-label="共 N 条通知"` | 无测试，原样保留 |

## 7. 清理的硬编码清单（`task9.css` 历史段）

| 旧值 | 位置 | 新值 |
| --- | --- | --- |
| `#e7f0ed !important` | `.history-row--selected` | `var(--color-primary-soft)`（去 `!important`） |
| `#fbefed` | `.history-delivery-error` | `var(--color-danger-soft)` |
| `min(58vh,560px)` 内联魔法 | `.history-virtual-viewport` | `var(--history-viewport-height)` |
| `38px` | `.history-row--header` min-height | `var(--history-header-height)` |
| `gap: 2px` | `.history-delivery-header > div` | `var(--space-1)` |
| `--font-size-1/2/3/4` | 历史段多处 | `--font-caption` / `--font-body` / `--font-heading`（`history-detail--empty`、`session-text`、`body`、`delivery`、`retry` 等） |
| `border-bottom` 通栏线 | `.history-body-section` / `.history-deliveries h3` / 详情面板上下列边框 | 删除（改 `SectionCard` 表面差；投递分隔线改 `--hairline`） |
| `.history-state` / `.history-state--*`、`.history-deliveries h3` | 随结构移除的死规则 | 删除，避免双定义 |

未纳入本阶段（不在所有权内 / 非硬编码色值）：`.history-title-button { gap:1px }`、`.history-search-control input { min-height:30px }`、`.history-body { max-height:240px }` 为历史段既有微观尺寸常量，本阶段未动。

## 8. 共享段处置说明（`task9.css`）

- `.page-actions`（第 1-7 行）：诊断/设置/历史共用，**零改动**。
- `.history-page-content, .diagnostics-page-content, .settings-page-content`（第 17-22 行）：共享规则**零改动**；历史页另加专属覆盖 `.history-page .history-page-content { padding-top:0; gap:24px }`（更高特异性，只作用于历史页）。
- `.history-filters label, .settings-field` 与 `.history-filters input/select, .settings-field input/select`（第 75-101 行）：设置段与本段共用，**零改动**；历史标签/输入继续复用。
- `.history-existence-list, .diagnostics-summary-list`（第 293 行起）：诊断段共用，**零改动**；历史页仅在 `.history-existence-card` 内追加 `margin-bottom:0` 覆盖。
- 设置段（`.settings-*`）、诊断段（`.diagnostic-*` / `.migration-*`）：**零改动**（diff 均落在历史段与历史专属覆盖块内）。

## 9. 自主决策清单

1. **筛选带用 flex 折行**而非再引入多档媒体查询：主/次两容器各自 `flex-wrap`，重置 `margin-left:auto` 贴行尾；删除旧的 1320px 筛选网格媒体块（仅含历史规则），640px 筛选单列规则同样删除（flex 已覆盖）。
2. **详情面板 `align-self: stretch` 实现底对齐**：设计稿要求「与左侧列表底部对齐」，`align-items:start` 保证顶对齐，stretch 保证底对齐；≤1120px 单列时恢复自然高度。
3. **修复表格越入详情面板的重叠**（§5）：旧基线靠详情面板不透明底遮挡，本阶段详情面板改为卡片化透明列后暴露；通过收缩列轨道最小宽度彻底消除，满足「不重叠 / 无水平溢出」。
4. **空态文案保持不变**：设计稿 §0.5 期望历史空态指向「连接渠道」，但 §5.4 交付规格未列空态，且该 CTA 需要新增渠道导航（超出本阶段范围）；`EmptyState`「暂无历史通知」文案与结构原样保留，留待后续统一处理。
5. **错误摘要列颜色保持现状**：`.history-error-cell` 恒为 `--color-danger`（含「无可见错误」）属既有语义表达，非本阶段交付项，未改（避免扩大范围）。
6. **状态 tone 映射**：表行 `Unknown/Failed→danger`、`Pending/Skipped→warning`、空投递→`neutral`、其余→`success`；详情投递卡同源。
7. **命名 token**：新增 `--history-viewport-height` / `--history-header-height` / `--history-scrollbar-size`（无现成语义 token，遵循 channels.css 先例）。

## 10. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`（历史 6 项全绿，含虚拟化 10,000 行与筛选 payload）。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built in 1.94s`。
- `PLAYWRIGHT_BROWSERS_PATH=... npm --prefix apps/desktop-ui run test:a11y`：`7 passed`（六页 + 登录对话框 Escape；历史一条 `ok`）。
- 额外实跑：`states.spec.ts` = `5 passed`（Unknown 不重试 / Failed 显式重试 / 诊断动作 / 渠道失败重试 / 无 Agent 空态）；`@visual 长文本不被裁切且关键区域不重叠` + `@zoom 200% 缩放等价视口下无水平溢出` = `2 passed`。
- 视觉基线红为预期（`history.png` diff ratio `0.02`），**未更新基线**；已目检 `history-actual.png`：筛选带主/次两行 + 重置行尾、表头六列完整落在左列、详情面板与列表底部对齐、无重叠。

## 11. 未碰清单（零改动声明）

- 未改：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task7.css`、`tests/**`、`.opencode/**`。
- 业务逻辑、数据结构、数据请求、TanStack Query Key、表单提交、路由路径、状态机：零改动。
- `@tanstack/react-virtual` 的 `useVirtualizer` 参数：零改动。
- 可访问名与 role：零改动（§6 全部保持）。
- `task9.css` 仅改历史段与历史专属覆盖；设置段 / 诊断段 / 共享规则本身零改动（§8）。
