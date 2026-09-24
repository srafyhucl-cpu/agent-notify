# Phase 5a 决策日志｜Agent 管理页迁移

- 日期：2026-09-24
- 范围：`src/features/agents/AgentsPage.tsx`、`AgentList.tsx`、`AgentDetail.tsx`、`styles/task7.css`（仅 Agent 段）；`AgentConfigForm.tsx` 未改（表单逻辑与视觉控件原样保留）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§1 五原则 / §4 范式分配 / §5.4 Agent：行卡列表 + 三段详情）、阶段 4 渠道页（`ChannelsPage.tsx` / `ChannelAccountDetail.tsx` / `channels.css`）作为同款结构语言与视觉密度样例
- 红线：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css`、`tokens.css`、`layout.css`、`task9.css`、`tests/**` —— 零改动；`task7.css` 只动 Agent 段。

## 1. 结构映射（旧区块 → 新范式）

| 旧结构 | 新结构（范式组件） | 说明 |
| --- | --- | --- |
| `workbench-page-header`（h1 + page-summary + `page-count` 徽标） | `PageHeader`（h1「Agent 管理」+ 摘要 + `actions` = 原 `page-count`） | 计数徽标移入 actions，`aria-label="共 N 个 Agent"` 原样保留 |
| `workbench-page-content agents-page-content`（gap 16px） | 同结构 + `.agents-page .agents-page-content`（`padding-top:0`、`gap:24px`） | 抵消 `layout.css` 与 `PageHeader` 的双重顶距 |
| `agents-workspace`（`1.45fr / minmax(320px,0.85fr)`，gap 16） | `1.6fr / 0.9fr`，gap 24，对齐渠道页 | Master–Detail |
| `agent-list` 6 列表格（名称/通知/回复/接入状态/最近事件/配置） | 4 列**行卡**（名称 + 回复元信息 / 通知 / 接入状态 + 最近事件 / 配置），保留 `<table>` 语义 | 见 §3、§4 |
| `span.status-label--success/danger` | `StatusBadge`（tone success/danger），文案仍「接入正常 / 接入异常」 | 颜色 + 文字双载 |
| `.agent-row--selected` 背景 `#e7f0ed` | `var(--color-primary-soft)` + `var(--surface-3-border)` | 见 §5 |
| 详情 `section-heading`（h2 + `status-label` 服务可用） | `agent-detail-header`（h2 + description）+ 身份卡 `action` 放 `StatusBadge`「服务可用」 | 见 §6 决策 3 |
| `dl.descriptor-list`（Agent ID / 启用状态 / 最近事件） | `FieldRow` × 3（一决策一行） | 身份段 |
| 平铺 `capability-list`（5 枚胶囊） | `SectionCard`「能力」包裹同一 `capability-list`（`aria-label="Agent 能力"` 保留） | 文案「通知可用 / 续聊可用 / …」零改动 |
| `agent-config-section`（h3「配置」+ form） | `SectionCard`「配置」包裹同一 `AgentConfigForm` | 表单提交逻辑零改动 |
| 空态 `EmptyState`「暂无 Agent」 | **保持原 `EmptyState`**（文案 / role 不变） | 契约见 §2 |
| 页面纵向单列 | `agents-workspace` 左列表 / 右详情，≤1180px 降单列 | 同渠道页 |

## 2. 契约保持清单（逐条 role / 可访问名 + 测试出处）

| 契约 | 出处 |
| --- | --- |
| h1「Agent 管理」 | `tests/helpers.ts:gotoSection`（每页 h1 名 = 导航标签） |
| Agent 名按钮（文本 = displayName，`aria-pressed`） | `AgentsPage.test.tsx:123/149/195/302/336`；`dynamic-adapters.spec.ts:17/20`（`exact`） |
| 长文本 Agent 名按钮 `getByRole("button",{name:longChineseText})` 不被裁切 | `visual.spec.ts:52-56` |
| `role="switch"` `aria-label="{名称} 通知"`（能力 notify 时存在、非 notify 时不存在） | `AgentsPage.test.tsx:125/151/237-239`；`visual.spec.ts:57-60`（与名称按钮不重叠） |
| 「回复」（精确文本，唯一） | `AgentsPage.test.tsx:127` |
| 「接入正常」 | `AgentsPage.test.tsx:128` |
| 「不支持」（notify=false 时，唯一） | `AgentsPage.test.tsx:153` |
| h2「{名称} 详情」（`agent-detail-{id}`）不被裁切 | `visual.spec.ts:62-67` |
| 「服务可用」（精确文本）不被裁切 | `visual.spec.ts:68-70` |
| 空态 h2「暂无 Agent」+ `/安装或启用 Agent 适配器后/` | `states.spec.ts:113-121` |
| form `aria-label="{名称} 配置"` + button「保存配置」+ SchemaForm 文案「已配置 / 当前版本无法编辑此字段」+ `getByLabelText("API Key")` | `AgentsPage.test.tsx:196-203/337-338` |
| `role="alert"` 更新失败提示 | `AgentsPage.test.tsx:242` |
| axe：Agent 管理无 serious/critical | `accessibility.spec.ts:11` |
| `page-count` `aria-label="共 N 个 Agent"` | 无测试，原样保留 |
| 配置按钮 `aria-label="{名称} 配置"` | 无测试，原样保留（避免删除可访问名） |

## 3. 表格 → 行卡决策依据

- **保留 `<table>` 语义**：测试虽未依赖 `tr`（不同于渠道页 `closest("tr")`），但 `AgentsPage.test.tsx:127` 的 `getByText("回复")` 依赖列表里存在精确「回复」文本。若整表改为 div 行卡，需重新安放该文案，风险更高；沿用渠道页先例（保留 `<tr>`，`table-layout: fixed` + 单元格拼圆角表面条）最稳。
- **列数 6 → 4**：名称 / 通知 / 接入状态 / 配置；「回复」能力下沉到名称下的**元信息行**（`回复` 独立文本节点，满足精确匹配且不重复）；「最近事件」摘要下沉到接入状态列内（单行省略号），对齐渠道页「状态 + 错误摘要」的列内组织。
- **元信息行值沿用 `支持 / —`**（不用「不支持」）：避免与通知列 notify=false 的「不支持」形成重复文本，破坏 `getByText("不支持")` 唯一性。
- 列宽 42% / 16% / 28% / 14%（百分比 + `table-layout: fixed`），删除旧的 `min-width: 680px` 与 `overflow-x: auto`，消除横向滚动。

## 4. 详情三段落地

- 结构：`agent-detail-header`（h2「{名称} 详情」+ description）→ `agent-detail-segments`（gap 24）内三张 `SectionCard`：**身份 / 能力 / 配置**。
- 身份段：3 条 `FieldRow`（Agent ID 等宽字体 / 启用状态 / 最近事件）。
- 能力段：`capability-list`（`aria-label="Agent 能力"`）+ 5 枚 `capability-item`，文案「通知可用 / 续聊可用 / 会话标题可用 / Hook 安装关闭 / 回复窗口关闭」零改动。
- 配置段：`AgentConfigForm` 原样（`SchemaForm` 控件与「保存配置」按钮不变）。
- 与渠道页差异：渠道页用「单卡 + 四段（h3）」，本页按交付规格「每段一张 `SectionCard`」用三张卡；h2「{名称} 详情」由卡片上方的 header 承接，两个契约同时满足（见 §6 决策 2）。

## 5. 清掉的硬编码清单（`task7.css` Agent 段）

| 旧值 | 位置 | 新值 |
| --- | --- | --- |
| `#e7f0ed` | `.agent-row--selected` 背景 | `var(--color-primary-soft)`（选中描边 `var(--surface-3-border)`） |
| `min-width: 680px` | `.agent-list table` | 删除（改 `table-layout: fixed` + 百分比列宽） |
| `overflow-x: auto` | `.agent-list` | 删除 |
| `88px` | `.descriptor-list > div` 列宽 | 规则删除（改用 `FieldRow`） |
| `var(--color-border)` 通栏细线 | `.agent-list` / `.agent-detail` 上下列边框 | 行卡用 `--surface-2-border` 圆角描边；详情段用 `--hairline`（SectionCard 内部） |
| `--font-size-1/2/3` | Agent 段多处字号 | `--font-caption` / `--font-body` |
| `margin-top: var(--space-3)` | `.agent-config-form` | 删除（`SectionCard` body 已提供 16px 顶距） |
| `.descriptor-list` / `.descriptor-list > div` / `dt` / `dd`、`.agent-config-section`、`.agent-list,.agent-detail` 边框块、`.agent-config-section h3` 选择器 | 随结构移除的死代码 | 已删除，避免双定义 |

未纳入本阶段的共享硬编码（不在所有权内）：`.switch-track{background:#aeb5af}`、`.schema-field--unsupported{background:#fff8e8}`、`layout.css .inline-error{background:#fbefed}`、`task9.css` 若干浅底 —— Phase 6 统一收口；本阶段 `.icon-text-button` / `.switch-control` / `.switch-track` / `.schema-*` / `.capability-list` 规则**零改动**。

## 6. 自主决策清单

1. **保留 `<table>` 语义、样式改行卡**，列数 6→4（§3）。
2. **详情用 header + 三张 `SectionCard`**：交付规格明确「每段一张 `SectionCard`」，而 h2「{名称} 详情」是视觉/无障碍契约；两者冲突时以 header 承接 h2、三张卡承接三段（能力/配置/身份），同时满足。渠道页的单卡多段语言在卡面/elevation/间距上沿用（SectionCard 自带）。
3. **健康徽标从 header 移入「身份」卡 `action`**：初版放在 header（直接落在画布）时 axe 报 `color-contrast .agent-detail-health`——浅色下 `success-soft` 合成于 canvas `#f5f7fa` 时对比度约 4.3:1，低于 4.5。移入卡面（`--surface-1` = `#ffffff`）后恢复 ≥4.5:1，a11y 全绿（实测见 §8）。这也更符合「徽标落在表面层」的语义。
4. **「回复」元信息值用 `支持 / —`**，避免「不支持」重复（§3）。
5. **section 命名保留**：旧 `<section aria-labelledby="page-title-agents">` → 新 `<section aria-label="Agent 管理">`（`PageHeader` 不暴露 h1 id），保持「命名 region」无障碍语义不变。
6. 列表「接入状态」徽标 tone：`available ? success : danger`，文案保持「接入正常 / 接入异常」。

## 7. 间距 / 字阶落地说明

- 间距只用 `--space-1..6`：组内 8px（`--space-2`，行卡 `border-spacing` 行距、能力胶囊间距）、组间 24px（`--space-5`，workspace 左右分栏、详情三段、内容区纵向），页头留白 24px（`PageHeader` 自带）；`.agents-page .agents-page-content{padding-top:0}` 抵消 `layout.css` 的重复顶距。
- 字阶只用别名：页头 `--font-display`（28，PageHeader 内置）、详情 h2 与 SectionCard 区标题 `--font-heading`（18）、正文 / Agent 名 `--font-body`（14）、表头 / 元信息 / 最近事件 / 能力胶囊 / 描述 `--font-caption`（12）。
- elevation：行卡 `--surface-2`（hover `--color-surface-muted`，选中 `--color-primary-soft` + `--surface-3-border`），详情三段卡与能力/配置卡 `--surface-1`（SectionCard 内置），分割线 `--hairline`。
- 长文本：Agent 名与最近事件走「外层 `overflow:hidden` + 内层 `text-overflow:ellipsis`」，详情 description / h2 走 `overflow-wrap:anywhere`，满足「不裁切 / 不重叠 / 无水平溢出」。
- 窄窗：≤1180px 左右分栏降单列；≤640px 详情 header 转纵向。

## 8. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`（Agent 7 项全绿）。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built in 1.68s`。
- `PLAYWRIGHT_BROWSERS_PATH=... npm --prefix apps/desktop-ui run test:a11y`：`7 passed`（六页 + 登录对话框 Escape）。
- 额外实跑：`states / dynamic-adapters / navigation` = `10 passed`；`@visual 长文本不被裁切且关键区域不重叠` + `@zoom 200% 缩放等价视口下无水平溢出` = `2 passed`。
- visual 基线红为预期（`agents.png` diff ratio 0.04），未改基线。

## 9. 未碰清单（零改动声明）

- 未改：`src/bridge/**`、`src/data/**`、`AgentConfigForm.tsx`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task9.css`、`tests/**`、`.opencode/`。
- 业务逻辑、数据结构、数据请求、Query Key、表单提交、路由路径、状态机：零改动。
- 可访问名与 role：零改动（§2 全部保持；`page-count` 与配置按钮可访问名亦保留）。
- `task7.css` 仅动 Agent 段；`.icon-text-button` / `.switch-control` / `.switch-track` / `.schema-*` / `.capability-list` / `.overview-*` / `.health-summary` / `.metric-list` / `.action-list` / `.data-table` / `.section-heading` / `.page-count` 等共享段零改动。
