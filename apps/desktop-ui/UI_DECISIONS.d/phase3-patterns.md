# Phase 3 决策日志｜范式骨架与公共组件

- 日期：2026-09-24
- 范围：`components/patterns/**`(新)、`components/LoadingRows.tsx`(升级)、`styles/patterns.css`(新)、`styles/tokens.css`(仅加法)、`styles/layout.css`(仅删除已迁走的骨架段)
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§1 五原则 / §2 气质 B + 呼吸档位 1 / §3 双主题 / §4-5 页面范式）、`docs/superpowers/plans/2026-09-24-ui-ux-redesign-plan.md`（Phase 3）
- 红线：`src/bridge/**`、`src/data/**`、`src/features/**`、业务逻辑、数据结构、数据请求、TanStack Query 缓存 Key、表单提交、路由路径、状态机 —— 零改动。组件为新建、暂未被页面消费（Phase 4 起消费），符合预期。

## 0. 交付物总览

| 文件 | 说明 |
| --- | --- |
| `components/patterns/PageHeader.tsx` | 页头范式 |
| `components/patterns/KpiCard.tsx` | KPI 锚点卡 |
| `components/patterns/SectionCard.tsx` | 分组卡（标题行唯一语法） |
| `components/patterns/FieldRow.tsx` | 一决策一行 |
| `components/patterns/StatusBadge.tsx` | 状态徽标（呼吸微光档位 1） |
| `components/patterns/EmptyFunnel.tsx` | 渠道漏斗空态 |
| `components/patterns/index.ts` | barrel：统一导出 + 引入 `patterns.css` |
| `styles/patterns.css` | 组件类样式 + 关键帧唯一归属 + 窄窗/reduced-motion 降级 |
| `styles/tokens.css` | 新增语义字阶别名（纯加法） |
| `components/LoadingRows.tsx` | 加一行 `patterns.css` 引入（骨架样式来源迁移） |
| `styles/layout.css` | 删除旧 `.loading-*` 段（迁至 patterns.css） |

## 1. 字阶别名最终取值（`tokens.css`，纯加法，旧 `--font-size-*` 原样保留）

```css
--font-display: 28px;   /* 展示值：页面标题、KPI 数字、空态锚点 */
--font-heading: 18px;   /* 区标题：分组卡标题 */
--font-body: 14px;      /* 正文：摘要、行标签、步骤标题 */
--font-caption: 12px;   /* 标签/说明：KPI 标签、计数、行说明、徽标 */
```

- 取设计稿 §1.4 原值 **28 / 18 / 14 / 12，未做 0.5~1px 微调**：四档差值已足够悬殊（28↔18↔14↔12），贴合"字阶少而差大"；`--font-display` 28px 与旧 `--font-size-5` 20px 拉开明显层级，作为一屏一锚点。
- 不删/不改旧 token，向后兼容；Phase 4/5 页面迁移时逐步改用别名。

## 2. 各组件 props / 类名 / 结构决策

### PageHeader（`.pattern-page-header`）
- props：`title`(string, 必填)、`summary?`(string)、`actions?`(ReactNode)。
- 结构：`<header>` → `.pattern-page-header-copy`(h1 + p) + `.pattern-page-header-actions`。
- 决策：标题固定 **h1**（每页仅一个），字阶 `--font-display`；摘要 `--font-body` + `--color-text-muted`；**不画底部通栏细线**（遵循 §1.5「去通栏细线」），仅用 `margin-bottom: --space-5` 留白分区；主操作靠右。

### KpiCard（`.pattern-kpi-card`）
- props：`value`(ReactNode, 必填)、`label`(string, 必填)、`tone?`(default/success/warning/danger, 默认 default)、`suffix?`(ReactNode)。
- 结构：`.pattern-kpi-card-value`(number + suffix) + `.pattern-kpi-card-label`(dot + label)。
- 决策：`tone` **只影响状态点与数字旁缀，不整卡变色**（同源色经 `--pattern-kpi-tone` 派生）；`tone="default"` 不渲染状态点；状态点 `aria-hidden`（语义由 label 文字承载）；数字 `tabular-nums` 防抖动；卡面统一 `--surface-1` 层。

### SectionCard（`.pattern-section-card`）
- props：`title`(string, 必填)、`count?`(number)、`action?`(ReactNode)、`description?`(string)、`children`(ReactNode)、`className?`(string，仅外层布局)。
- 结构：`<section aria-labelledby>` → header(标题 h2 + count + action) + description + body。
- 决策：**卡片标题行唯一语法**——标题在左、计数紧随标题、操作固定右上，各页不得自创；`count` 用 `typeof === "number"` 判定，0 也显示；`<section>` 经 `useId` + `aria-labelledby` 关联 h2，成为有名字的 region（a11y 加分，不新增可见文案）；表面 `--surface-1` / `-border` / `-shadow`，内边距 `--space-4`。

### FieldRow（`.pattern-field-row`）
- props：`label`(string, 必填)、`description?`(string)、`control`(ReactNode, 必填)、`controlId?`(string)。
- 结构：左 `.pattern-field-row-text`(label/span + description)，右 `.pattern-field-row-control`。
- 决策：**行间用 `--hairline` 分隔**（`+` 相邻选择器，避免首行多余线），行内 `padding: --space-2 0`（组内 8px 节奏）；控件列右对齐成列；提供 `controlId` 时左侧渲染 `<label htmlFor>` 保证可访问名关联，否则用 `<span>` 并由调用方给控件 `aria-label`；**不发明交互**，控件与状态由调用方提供。

### StatusBadge（`.pattern-status-badge`）
- props：`tone`(success/warning/danger/info/neutral, 必填)、`children`(ReactNode, 必填)、`breath?`(boolean)、`className?`。
- 结构：单个 `<span data-tone>`；呼吸由 `::after` 伪元素承载。
- 决策：柔和底 `--color-*-soft` + 语义色文字 + `--radius-pill`；`neutral` 无对应 soft token，用 `--color-surface-muted` + `--color-text-muted`（语义仍由文字承载）；`breath` 默认 `warning/danger` 开、其余关，`breath={false}` 显式关、`breath={true}` 可强制开；**不设 role**（避免与既有 `role="status"` 测试契约冲突，语义靠文字）。

### EmptyFunnel（`.pattern-empty-funnel`）
- props：`title`(string)、`description`(string)、`steps?`(readonly EmptyFunnelStep[])、`action?`(ReactNode)。
- 结构：`<section aria-labelledby>` → copy(h2 + p) + `<ol>`(步骤) + action。
- 决策：锚点大标题用 `--font-display`；步骤用 `<ol>` + CSS counter 编号（语义列表，编号不写入文案位，符合 §2「禁止把动效/编号词写进文案位」精神）；`steps` 横向等分（`flex:1 1 0`，无 px 魔法数），≤960px 转纵向；**文案与主 CTA 全部由调用方传入**，组件只负责容器与层级。

## 3. 呼吸微光档位 1 最终参数（StatusBadge）

| 项 | 值 | 实现 |
| --- | --- | --- |
| 周期 | `--pattern-breath-duration: 2500ms` | 命名 token，`animation-duration` 引用 |
| 缓动 | `var(--ease-out)` | 复用既有动效 token |
| 幅度 | 暗色峰值 `0.35`、亮色峰值 `0.18`（仅动画伪元素透明度） | 关键帧 `pattern-breath-glow` / `pattern-breath-shadow` |
| 暗色表达 | **辉光脉冲**：`--pattern-breath-glow: 0 0 8px 0 currentColor` | `:root:not([data-theme="light"])` 分支 |
| 亮色表达 | **极淡阴影脉冲**（亮色零辉光）：`--pattern-breath-shadow: 0 1px 3px 0 currentColor` | `:root[data-theme="light"]` 分支 |
| 同源色 | `currentColor`（伪元素继承徽标语义色） | 不新增任何硬编码色 |
| 默认范围 | 仅 `warning` / `danger` | TSX 内 `DEFAULT_BREATH_TONES` |
| reduced-motion | `animation: none`（伪元素 opacity 保持 0 → 不显示） | 文件末降级段 |

- 实现方式：动画**只动伪元素 `opacity`**，辉光/阴影样式按主题静态定义。好处：幅度可控、暗亮差异清晰、`currentColor` 同源、关键帧只需两条且集中在 `patterns.css`（全局唯一）。
- 关键帧命名：`pattern-breath-glow` / `pattern-breath-shadow`，其它文件禁止重复定义。

## 4. 扫描微光骨架最终参数（升级 LoadingRows）

- **保留 `role="status"` 与 `aria-label` 原样**（`AppShell.test.tsx` 断言 `getByRole("status", { name })` 且 `children` 数量 = rows）；DOM 结构（`.loading-rows` / `.loading-row` / `.loading-row-bar` / `-short`）**零改动**，只迁移+升级样式，**旧类名保留兼容**。
- 条高：`--pattern-skeleton-bar-height: var(--font-caption)`（12px，与真实标签/说明文字 1:1 量级）。
- 行高：`--pattern-skeleton-row-height: 48px`，与旧骨架一致、且与真实列表行高量级一致 → **加载完成不跳变，防 CLS**。
- 列轨道：`minmax(0,2fr) minmax(0,3fr) minmax(0,1fr)`（用分数单位替换旧的 `minmax(96px,.7fr) minmax(120px,1.3fr) 72px`，消除 px 魔法数）。
- 动效：`@keyframes pattern-scan`，周期 `--pattern-scan-duration: 1600ms`，`linear` 无限循环；`background-size: 220% 100%`，`background-position` 由 `150% → -50%`（高光自左入、向右出）。
- 微光色：渐变高光用 `var(--color-primary-soft)`（主题感知，暗色呈青辉、亮色呈极淡青），基底 `var(--color-surface-muted)`，**零硬编码色**。
- 分隔：行间 `--hairline`（替换旧 `--color-border` 通栏线）。
- reduced-motion：`animation: none`（高光停在屏外，退化为静态灰条）。

## 5. 与既有组件的关系

- `EmptyState.tsx`：**保持不动**。`EmptyFunnel` 是"渠道漏斗"专用空态（锚点大标题 + 三步 + 主 CTA），与通用 `EmptyState` 并存；Phase 4/5 迁移时按"是否需要漏斗引导"择一使用。
- `LoadingRows.tsx`：升级（非新建）。`plan` 中写的 `SkeletonRows` **未新建**，按本任务书更具体的指令升级既有 `LoadingRows`，避免出现两套骨架。

## 6. CSS 接线（关键工程决策）

- **barrel 引入样式**：`components/patterns/index.ts` 顶部 `import "../../styles/patterns.css";`。Phase 4/5 页面一旦 `import { ... } from "../components/patterns"`，样式即随模块图进入；真实入口与 Playwright harness 都覆盖，**未改 `src/main.tsx` 与 `tests/harness/main.tsx`**。
- **LoadingRows 额外直连样式**：`LoadingRows` 不在 patterns/ 下、且已被现有页面消费，故在其模块顶部直接 `import "../styles/patterns.css";`，保证既有页面（经 router）与 Vitest 都能取到骨架样式。Vite 对重复 CSS import 去重，无副作用。
- **样式源唯一**：从 `layout.css` 删除旧 `.loading-*` 段（仅删这一段，未格式化其它内容），避免同选择器双定义、级联顺序不确定。这是"骨架样式落在 patterns.css"的必要迁移，属本阶段相关改动。

## 7. 自主决策清单（依据设计稿裁决）

1. 字阶别名取设计稿原值 28/18/14/12，不微调（§1.4）。
2. PageHeader 不加底部通栏细线，改用留白分区（§1.5「去通栏细线」）。
3. SectionCard / EmptyFunnel 用 `<section aria-labelledby>` + `useId`，标题 h2 关联，成为有名字 region（a11y 提升，不新增可见文案）。
4. KpiCard 用 `--pattern-kpi-tone` 同源色变量，避免"整卡变色"（设计稿 §5.3 明确 tone 只影响点与旁缀）。
5. StatusBadge 不设 `role`，语义由文字 + 颜色冗余承载（避免与既有 `role="status"` 测试契约冲突）。
6. 呼吸动画实现为"静态辉光/阴影 + 只动画 opacity"，暗亮分支按主题选择 shadow 形态（§2 档位 1）。
7. 骨架列轨道改用分数单位、行高/条高 token 化，去掉 px 魔法数（工程底线）。
8. `plan` 的 `SkeletonRows` 由升级 `LoadingRows` 替代（任务书优先）。

## 8. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`。
- `npm --prefix apps/desktop-ui run build`：`✓ 1992 modules transformed`、`✓ built`。
- `PLAYWRIGHT_BROWSERS_PATH=... npm --prefix apps/desktop-ui run test:a11y`：`7 passed`（渠道 / Agent 管理 / 总览 / 历史 / 诊断 / 设置 + 登录对话框 Escape）。
- visual 基线仍红为预期（Phase 6 统一刷新），本阶段未跑/未改基线。

## 9. 未碰清单

- 未改：`src/bridge/**`、`src/data/**`、`src/features/**`、`src/main.tsx`、`tests/**`、路由路径、表单提交、业务状态机、`.opencode/`、`styles/task7.css` / `task9.css` / `channels.css`、`EmptyState.tsx`。
- `tokens.css` 仅新增 4 个语义字阶别名，既有 token 数值/命名全部原样。
