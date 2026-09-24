# Phase 5e 决策日志｜设置页迁移（配置中枢 · 左目录 + 一决策一行）

- 日期：2026-09-24
- 范围：`src/features/settings/SettingsPage.tsx`、`ReplySettingsForm.tsx`、`QuietHoursForm.tsx`、`UpdateSettings.tsx`、`styles/task9.css`（仅设置段）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§1 五原则 / §5.2 设置页 / §6 交互状态）
- 红线：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task7.css`、`tests/**`、`.opencode/**` —— 零改动。

## 1. 结构映射（旧区块 → 新范式组件）

| 旧结构 | 新结构 | 说明 |
| --- | --- | --- |
| `workbench-page-header`（h1 + `page-summary` + 顶部保存按钮） | `PageHeader`（h1「设置」+ 摘要 + 右侧「保存设置（顶部）」） | 摘要文案不变；按钮 `aria-label` 原样保留 |
| `form.settings-form` 内 5 个 `section.settings-group`（左标题列 + 右字段列，`border-bottom` 分隔） | `.settings-layout` 网格：左 `.settings-index` 目录 + 右 `form.settings-form` | 参差的多列分组改为卡片表面差 |
| 每个 `section.settings-group > header(h2 + p)` | 每分组一张 `SectionCard`（h2 = 组名 + `description` = 一句话说明） | h2 文案与层级不变 |
| `label.settings-switch-row`（strong/small + 复选框，`space-between` 拉满整行） | `FieldRow`（label + description 在左，控件列右对齐） | 一决策一行 |
| `label.settings-field`（span + input + small） | `FieldRow`（description 承接原 small） | 数值/下拉统一控件列宽 |
| `QuietHoursForm` 的 `div.settings-subsection.settings-quiet-hours`（开关 + `settings-inline-fields` 两列时间） | `QuietHoursForm` 返回两个 `FieldRow` 片段：勿扰时段开关 / 勿扰时间（等宽两列时间控件） | 逻辑零改动，仅拆行 |
| `div.settings-subsection`（Agent 默认开关标题 + Agent 列表） | `div.settings-subsection`（子区块标题）+ `FieldRow` 列表 | 保留子区块语义 |
| 渠道 unavailable 行 + 只读配置子区块 | `FieldRow`（默认通知账号）+ `.settings-unavailable-row` + `.settings-readonly-config` | 文案不变 |
| UpdateSettings 的开关/下拉/unavailable 行/反馈 | `FieldRow` 三行 + `.settings-unavailable-row`（检查与安装更新）+ 反馈条 | 全部状态表达保留 |
| `div.settings-save-row`（普通底部行） | `.settings-save-row`（`position: sticky; bottom: 0` 表面条） | 保存状态常驻可见 |
| 页面纵向单列 | `.settings-layout`（左目录 sticky / 右面板单列），≤1080px 降单列 | Settings 范式 |

## 2. 契约保持清单（role / 可访问名 → 测试出处）

| 契约 | 出处 |
| --- | --- |
| h1「设置」 | `tests/helpers.ts:190`（`gotoSection` 每页 h1） |
| h2「通知 / 回复 / 渠道 / 应用 / 数据」 | `SettingsPage.test.tsx:193-199` |
| `role="switch"`「全局暂停」 | `SettingsPage.test.tsx:200/231/259/288/321/354/378/418/439` |
| `getByLabelText`「通知冷却（秒）」 | `SettingsPage.test.tsx:381/440`；`visual.spec.ts:85` |
| `getByLabelText`「默认通知账号」 | `SettingsPage.test.tsx:384`；`visual.spec.ts:85` |
| option `/主账号.*account-alpha/` | `SettingsPage.test.tsx:202` |
| button「打开数据目录 / 导出脱敏诊断包 / 备份数据库」disabled | `SettingsPage.test.tsx:204-206` |
| 文案「当前版本不可用」≥3 | `SettingsPage.test.tsx:207` |
| button「检查更新」→「正在检查」 | `SettingsPage.test.tsx:209/232/265/289/322/355` |
| button「下载并安装」→「正在下载并安装」 | `SettingsPage.test.tsx:237/245/294/302/303/334/363` |
| 文案「测试包未签名，仅供内部验证；正式版不会安装未签名更新。」 | `SettingsPage.test.tsx:212` |
| 文案「先检查更新，确认存在可安装版本后才能下载。」 | `SettingsPage.test.tsx:262` |
| 文案「上次检查更新失败，请先重新检查。」 | `SettingsPage.test.tsx:270` |
| 文案「正在安装新版本，完成后应用会自动重启，请保持应用运行。」 | `SettingsPage.test.tsx:296/329` |
| 文案「上次安装未成功，请重新检查更新后再试。」 | `SettingsPage.test.tsx:333/365` |
| `role="alert"`「下载更新包失败…」 | `SettingsPage.test.tsx:359` |
| button「保存设置」（表单底部提交；唯一精确名） | `SettingsPage.test.tsx:385/422/443` |
| button「保存设置（顶部）」（`aria-label`，非精确「保存设置」，不冲突） | 原实现保留 |
| `role="status"`「设置已保存」 | `SettingsPage.test.tsx:399` |
| `role="alert"`「设置保存失败，请检查数据库状态」 | `SettingsPage.test.tsx:424` |
| `role="alert"`「通知冷却必须是 0 到 3600 之间的整数」 | `SettingsPage.test.tsx:445` |
| form `aria-label="AgentNotify 设置"` | 原实现保留 |
| 安装禁用原因 `id="settings-update-install-reason"`（`aria-describedby`） | 原实现保留 |
| `role="switch"`「启用勿扰时段 / 引用回复 / 送达确认 / 随系统启动 / 启动时隐藏 / {Agent} 默认通知」 | 原实现保留（`aria-label` 未改） |
| 标签「路由有效期（秒） / 勿扰开始 / 勿扰结束 / 更新通道」 | 原实现保留（`aria-label` 未改） |
| 设置页 axe 无严重问题 | `accessibility.spec.ts:11`（设置一条） |
| 长文本三标签不裁切 | `visual.spec.ts:84-86` |

> 说明：`SettingsPage.test.tsx` 的 `name: "保存设置"` 为精确匹配；顶部按钮 `aria-label="保存设置（顶部）"` 与之不冲突，故全页「保存设置」精确名唯一。

## 3. 左目录跳转实现方案与理由

- **方案**：原生 `<a href="#settings-<组>">` 锚点，指向每个分组卡外的 `<div id="settings-<组>" class="settings-anchor">`（`scroll-margin-top: var(--space-5)`）。
- **理由**：原生链接天然可聚焦、可键盘触发、可被读屏宣告为链接、无需 JS/状态机，`prefers-reduced-motion` 下浏览器自行处理；较 `button + scrollIntoView` 更少代码、无行为分叉。红线要求「二选一」，故选锚点。
- **所有分组同时渲染且可见**：目录只做跳转，**不做「点组切换、隐藏其它组」**——`SettingsPage.test.tsx:193-199` 的 h2 断言与 axe 都要求全部组始终在 DOM 且可见。
- **当前位置高亮**：仅视图层新增 `IntersectionObserver`（`rootMargin: -96px 0 -55% 0`）滚动侦测，高亮落在 `aria-current="true"`；`typeof IntersectionObserver === "undefined"`（jsdom）时直接跳过，不新增业务状态机。点击锚点时同步置高亮。

## 4. 一决策一行的控件列宽策略

- 所有行统一用 `FieldRow`：标签+说明在左（`max-width: 60ch`），控件在右列 `align-items: flex-end`——这正是范式组件内置的「控件列右对齐，勾选框不再飘到最右边缘」。
- 数值/下拉：`.settings-control { width: var(--settings-control-width) }`（220px）统一宽度，右缘对齐成一列。
- 开关：`.settings-fields input[type="checkbox"]` 18px + `accent-color`，随控件列右对齐，不再各自拉到行尾。
- 勿扰开始/结束：`.settings-inline-fields` 等宽两列（`repeat(2, minmax(0,1fr))`），整体宽度同为 `--settings-control-width`，消除参差。
- ≤640px：控件与两列时间降为满宽，行纵向堆叠。

## 5. 清理的硬编码清单（task9.css 设置段）

| 旧值 | 位置 | 新值 |
| --- | --- | --- |
| `#e7f3ea` | `.settings-feedback--success` 背景 | `var(--color-success-soft)` |
| `#fbefed` | `.settings-feedback--error` 背景 | `var(--color-danger-soft)` |
| `--font-size-1/2/4` | 设置段多处字号 | `--font-caption` / `--font-body` / `--font-heading`（SectionCard 内置） |
| `gap: var(--space-3)` 叠在 `.settings-fields` | 行列表双倍行距 | 去掉 gap，交由 `FieldRow` 的 8px 行距 + `--hairline` 分隔 |
| `padding-left: var(--space-4)` | `.settings-inline-fields` | 去掉（改由控件列定位） |
| `minmax(180px, 0.36fr)` 左标题列 / `border-bottom` 通栏线 | `.settings-group` | 整段删除（改用 `SectionCard` 表面差） |
| `.settings-switch-row` / `.settings-field` / `.settings-quiet-hours` / `.settings-group` | 随结构移除的死类（设置段） | 删除；`task9.css` 顶部与 `.history-filters` 同组的 `.settings-field` 规则**未动**（避免碰 history 段） |

新增命名 token（布局尺寸，集中不游离）：`--settings-control-width: 220px`、`--settings-index-width: 148px`。

> 附带影响（有意）：`.settings-feedback` 为设置与 `DiagnosticsPage` 共用类（`DiagnosticsPage.tsx:353-354`），语义柔和底替换后诊断页复制反馈同样去硬编码，两主题自动成立。

## 6. 间距 / 字阶 / 主题落地

- 间距只用 `--space-1..6`：卡间距 24px（`--space-5`）、卡内子区块上距/上距 8/12px、行距由 FieldRow 的 8px 节奏承担。
- 字阶只用别名：h1 `--font-display`（PageHeader 内置）、h2 `--font-heading`（SectionCard 内置）、正文/控件 `--font-body`、说明/标签 `--font-caption`。
- 表面：分组卡 `--surface-1`（SectionCard 内置）、保存条 `--surface-2`、只读配置 `--color-surface-muted`、分割线 `--hairline`；设置段内零裸 hex/rgba。

## 7. 自主决策清单

1. **保留 5 个实际分组**（通知 / 回复 / 渠道 / 应用 / 数据），未按设计稿 §5.2 的六组拆分：h2 断言精确钉住这五个名字，且实际分组如此；目录与之一一对应。
2. **目录用锚点链接**（见 §3），不做组切换隐藏。
3. **保存区**：顶部保存按钮与底部提交按钮均保留原 `aria-label`/文案与禁用逻辑；底部行做成吸底「常驻保存区」，只消费既有 `canSave`/`savedMessage`/pending 状态。
4. **高亮态文字用 `--color-text`**（而非 `--color-primary`）：亮色下「深青文字 + primary-soft 底」对比度 4.25 < 4.5 会被 axe 判红；改为正文色 + 左侧主色描边 + 加粗 + 柔和底，颜色不单载，两主题均过。
5. 新增 `--settings-control-width` / `--settings-index-width` 命名 token（无现成语义 token，遵循 channels.css 先例）。
6. `settings-feedback` 共用类的重排（见 §5 附带影响）。

## 8. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`（设置 9 项全绿）。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built in 2.07s`。
- `PLAYWRIGHT_BROWSERS_PATH=... npm --prefix apps/desktop-ui run test:a11y`：设置页 `ok`；**Agent 管理一条失败**（`color-contrast: .agent-detail-health`），系并行 agent 正在修改的 `task7.css` + `agents/*`（`git diff --stat` 显示其 287 处改动）所致，**不在本阶段文件所有权内，未触碰**。其余 6 条通过。
- 额外实跑：`@visual 长文本不被裁切且关键区域不重叠` = `1 passed`；`@zoom 200%` = `1 passed`。
- 设置视觉基线 diff ratio `0.04`（预期，Phase 6 统一刷新），未改基线。已出 `settings-actual.png` 目检单列滚动 + sticky 目录 + 吸底保存条正常。

## 9. 未碰清单

- 未改：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/tokens.css` / `patterns.css` / `layout.css` / `task7.css`、`tests/**`、`.opencode/**`。
- `task9.css` 仅改设置段（diff hunk 全部 ≥ 第 668 行，未触及 `.history-*` / `.diagnostic-*` / `.migration-*` / `.page-actions` / `.existence-*` / `.history-filters`）。
- 业务逻辑、数据结构、数据请求、TanStack Query Key、表单提交、路由路径、状态机、`SettingsDto` 形状：零改动。
- 可访问名与 role：零改动（见 §2 全部保持）。
