# Phase 5d 决策日志｜诊断页迁移（例外分诊 · 按严重度分组 + 动作行右对齐 + 迁移卡独立成组）

- 日期：2026-09-24
- 范围：`src/features/diagnostics/DiagnosticsPage.tsx`、`DiagnosticList.tsx`、`styles/task9.css`（仅诊断/迁移段：`.diagnostic-*` / `.migration-*` / `.diagnostics-page-content` 专属覆盖）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§1 五原则 / §4 范式「例外分诊」/ §5.4 诊断：按严重度分组、动作行右对齐、迁移卡独立成组）
- 样例：阶段 4 渠道页、阶段 5c 历史页（`phase5c-history.md`）作为同款结构语言与共享段处置参照
- 红线：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task7.css`、`tests/**`、`.opencode/**` —— 零改动；`task9.css` 只动诊断/迁移段。

## 1. 结构映射（旧区块 → 新范式）

| 旧结构 | 新结构（范式组件） | 说明 |
| --- | --- | --- |
| `workbench-page-header`（h1 + `page-summary` + `page-actions`） | `PageHeader`（h1「诊断」+ 摘要 + `actions` = 原两个按钮） | 刷新 / 复制安全诊断信息文案、disabled、onClick 原样保留 |
| 页面 section `aria-labelledby="page-title-diagnostics"` | `<section className="workbench-page diagnostics-page" aria-label="诊断">` | 命名 region 语义保留（PageHeader 不暴露 h1 id） |
| `section.diagnostics-section[aria-labelledby="diagnostics-migration-title"]` + `section-heading` + h2「旧数据迁移」 | `SectionCard title="旧数据迁移"` | 卡片 h2 = 原 h2；卡片 `aria-labelledby` 使 region 名仍为「旧数据迁移」 |
| `section.diagnostics-summary` + h2「状态摘要」+ `dl.diagnostics-summary-list` | `SectionCard className="diagnostics-summary-card" title="状态摘要"` + 原 `dl` | 摘要列表 `dl` 结构、dt/dd 文案逐字保留 |
| `section.diagnostics-section[aria-labelledby="diagnostics-items-title"]` + h2「诊断项」+ `DiagnosticList` | `SectionCard title="诊断项"` + `DiagnosticList`（内部按严重度分组） | 见 §2 |
| `section.diagnostics-section[aria-labelledby="diagnostics-components-title"]` + h2「组件状态」 | `SectionCard title="组件状态"` | 组件卡 `diagnostic-component` 结构、状态文案保留 |
| `span.diagnostic-level.diagnostic-level--*` 色块 | `StatusBadge tone={LEVEL_TONES[level]}` | 语义色 + 文字双载；warning/danger 走默认呼吸微光 |
| `div.migration-status.migration-status--*`（左侧语义色描边） | `div.migration-status`（中性表面）+ `StatusBadge`（迁移状态） | tone 由徽标承担，删除 `--success/--warning/--danger` 描边类 |
| `strong`（迁移状态文案）+ `p.migration-source-state` | `migration-status-heading`（`StatusBadge` + `migration-source-state`）+ `p`（描述） | 文案逐字保留，仅重排 |

## 2. 严重度分组方案

- 分组常量 `SEVERITY_GROUPS`（`DiagnosticList.tsx`），按严重度降序，**仅渲染非空分组**：
  - `错误` ← `Error`
  - `等待` ← `Waiting`、`Paused`
  - `正常` ← `Normal`
- 落在单个 `SectionCard title="诊断项"` 内，各组以 `h3.diagnostic-group-title`（组内分隔标题，带计数徽标 `diagnostic-group-count`）区隔；组间 24px（`--space-5`）。
- **`Paused`（暂停）归入「等待」档**：设计稿只给三档（错误 / 等待 / 正常），而现有 `DiagnosticLevelDto` 有 4 个值；暂停既非错误也非正常，归入需关注但未失败的「等待」档，项目内每一项仍显示自己的「暂停」徽标（语义不丢）。见 §6 决策 1。
- `LEVEL_TONES`：`Normal→success`、`Waiting→warning`、`Error→danger`、`Paused→info`；`LEVEL_LABELS`（正常/等待/异常/暂停）逐字未改。
- 组标题文案用设计稿的「错误 / 等待 / 正常」，与条目徽标文案（异常/等待/正常/暂停）并存：组标题承载档位、徽标承载单条语义。

## 3. 契约保持清单（role / 可访问名 → 测试出处）

| 契约 | 出处 |
| --- | --- |
| h1「诊断」 | `tests/helpers.ts:gotoSection`（每页 h1 名 = 导航标签）；`routes.test.tsx:14` |
| `role="region"` name「旧数据迁移」 | `MigrationDiagnostics.test.tsx:33/76/110`（`SectionCard` 的 `aria-labelledby` 提供） |
| 迁移状态文案「未发现旧数据」「部分完成，跳过 2 条损坏记录」「失败，当前处于只读诊断模式」 | `MigrationDiagnostics.test.tsx:34/78/112` |
| 按钮「查看迁移报告」`disabled={report===null}`、「重新检测」 | `MigrationDiagnostics.test.tsx:36-40/81/124` |
| `role="list"` name「迁移损坏警告」+ `invalid_json` / `push.log` | `MigrationDiagnostics.test.tsx:84-88` |
| 失败详情文案「clawbot.json 中的账号凭据格式无效」、备份建议整句 | `MigrationDiagnostics.test.tsx:115-120` |
| `retry_legacy_migration` 调用 | `MigrationDiagnostics.test.tsx:128` |
| 诊断项 message「数据库完整性检查发现问题」/「渠道登录状态异常」、代码、时间 | `SettingsPage.test.tsx:462`；`states.spec.ts:78` |
| 等级徽标文案「异常」「正常」 | `SettingsPage.test.tsx:464/466` |
| 动作按钮文案（`action.label`，如「重新读取运行状态」「重新检查」）与 descriptor 命令调用 | `states.spec.ts:81-85`；`SettingsPage.test.tsx:469-472` |
| 「当前没有可执行的修复动作」 | `SettingsPage.test.tsx:467` |
| 组件状态空态「StatusService 未返回组件状态。」、组件名/状态文案 | 无测试，原样保留 |
| axe：诊断无 serious/critical | `accessibility.spec.ts:11` |
| 长文本无水平溢出 / @zoom 200% | `visual.spec.ts:90-101` |

> 说明：等级徽标文案「异常」「正常」由 `StatusBadge` 承载，仍是唯一文本节点，`getByText("异常")` / `getByText("正常")` 各命中一处；组标题「正常 · N」的直连文本为「正常 ·」，不等于「正常」，不与徽标冲突（见 §6 决策 2）。

## 4. 共享段处置说明（`task9.css`）

- `.page-actions`（第 1-7 行）：诊断/设置/历史共用，**零改动**（诊断页头改由 `pattern-page-header-actions` 承载，不再输出 `.page-actions`，但规则本身不动）。
- `.history-page-content, .diagnostics-page-content, .settings-page-content`（第 17-22 行）：共享规则**零改动**；诊断页另加专属覆盖 `.diagnostics-page .diagnostics-page-content { padding-top:0; gap:24px }`（更高特异性，只作用于诊断页）。
- `.history-existence-list, .diagnostics-summary-list`（第 293 行起）：与历史页共用，**共享规则零改动**；诊断页仅追加 `.diagnostics-summary-card .diagnostics-summary-list { margin-bottom:0 }` 覆盖块（`SectionCard` 已提供间距）。
- `.history-filters label, .settings-field` 等：与诊断无关，**零改动**。
- 历史段（`.history-*` / `.existence-*`，5c）、设置段（`.settings-*`，5e）：**零改动**。
- 640px 媒体查询中的共享选择器 `.history-existence-list, .diagnostics-summary-list`：**零改动**；诊断专属行（`.diagnostic-*` / `.migration-*`）就地保留。

## 5. 清理的硬编码清单（`task9.css` 诊断/迁移段）

| 旧值 | 位置 | 新值 |
| --- | --- | --- |
| `#e7f3ea` / `#fff3db` / `#fbefed` / `#e7f1f7` | `.diagnostic-level--normal/waiting/error/paused` | 随 `.diagnostic-level*` 死规则整体删除，改由 `StatusBadge`（`--color-*-soft` 语义底）承载 |
| `padding: 2px` | `.diagnostic-level` | 随规则删除 |
| `gap: 2px` | `.migration-issue-list > div` / `.migration-report-list > div` | `var(--space-1)` |
| `min-width: 150px` | `.diagnostic-action` | `var(--diagnostic-action-column)`（新增命名 token，值 168px） |
| `max-width: 180px` | `.diagnostic-action-unavailable` | `var(--diagnostic-action-column)` |
| `--font-size-1/2/3` | 诊断/迁移段多处（message / time / unavailable / component p / migration p / dt / dd / report-empty / warning span / issue h3） | `--font-caption` / `--font-body` / `--font-heading`（四档别名） |
| `border-left: 3px solid` 语义描边 + `--success/--warning/--danger` | `.migration-status` 及其 tone 类 | 删除（tone 改由 `StatusBadge` 承担），状态块改中性表面 |
| `.diagnostic-level` / `.diagnostic-level--*` | 随结构移除的死规则 | 删除，避免双定义 |

未纳入本阶段（结构性尺寸，沿用 history/channels 先例）：`.migration-warning-list li` 的 `minmax(100px, 0.35fr)` 轨道最小宽、`.diagnostic-components` 两列轨道；`--diagnostic-action-column: 168px` 为新增命名 token。

## 6. 自主决策清单

1. **`Paused` 归入「等待」档**：设计稿只给三档，`DiagnosticLevelDto` 有四值；暂停语义介于等待与正常之间，归入「等待」，条目徽标仍显示「暂停」。若后续要独立「暂停」档，只需调整 `SEVERITY_GROUPS` 常量。
2. **组标题用「错误 / 等待 / 正常 · N」而非纯档位名**：`getByText("正常")`（`SettingsPage.test.tsx:466`）要求「正常」在页面上唯一命中；组标题直连文本含「 ·」后不再等于「正常」，与徽标文本不冲突，契约零改动。
3. **迁移状态块改中性表面 + `StatusBadge`**：删除按 tone 着色的左侧描边（那是旧硬编码语义色的替代），改由徽标承担状态色，符合「颜色 + 文字双载」。
4. **`SectionCard` 统一四组**：迁移 / 状态摘要 / 诊断项 / 组件状态全部卡片化，沿用渠道/历史页表面语言；严重度分组作为「诊断项」卡内组内分隔标题（设计稿允许「SectionCard 或组内分隔标题」二选一）。
5. **动作行右对齐成列**：`.diagnostic-action { justify-content: flex-end; min-width: var(--diagnostic-action-column) }`——末列右缘恒贴卡片内容右缘，按钮天然右对齐成列；≤640px 单列时改为左对齐、取消最小宽。
6. **刷新动作保留在页头**：沿用旧「刷新 / 正在刷新」按钮（非「重新检查」，避免与诊断项动作文案冲突），复制按钮一并保留。
7. **`copyState` 反馈沿用 `.settings-feedback`**：为最小化改动，未新造诊断反馈类（该类定义在设置段，未改）；仅 JSX 复用，不触碰设置段 CSS。

## 7. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`（含 `MigrationDiagnostics` 3 项、`SettingsPage > DiagnosticsPage` 1 项）。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built in 1.43s`。
- `PLAYWRIGHT_BROWSERS_PATH=... npm --prefix apps/desktop-ui run test:a11y`：`7 passed`（六页 + 登录对话框 Escape；诊断一条 `ok`）。
- 额外实跑：`states.spec.ts`「诊断修复动作调用 descriptor 声明的命令」= `1 passed`；`visual.spec.ts`「长文本不被裁切且关键区域不重叠」+「@zoom 200% 缩放等价视口下无水平溢出」= `2 passed`。
- 视觉基线红为预期（`diagnostics.png` diff ratio `0.04`），**未更新基线**；已目检 `diagnostics-actual.png`：页头 + 四张卡片、迁移徽标/来源行、状态摘要三列、诊断项「错误 · 1」组与右对齐动作按钮均正确。

## 8. 未碰清单（零改动声明）

- 未改：`src/bridge/**`、`src/data/**`、其它 `features/**`、`app/**`、`components/patterns/**`、`styles/patterns.css` / `tokens.css` / `layout.css` / `task7.css`、`tests/**`、`.opencode/**`。
- 业务逻辑、数据结构、数据请求、TanStack Query Key、表单提交、路由路径、状态机：零改动。
- 可访问名与 role：零改动（§3 全部保持）。
- `task9.css` 仅改诊断/迁移段与诊断专属覆盖；历史段 / 设置段 / 共享规则本身零改动（§4）。
