# Phase 4 决策日志｜渠道页迁移（第一优先业务页）

- 日期：2026-09-24
- 范围：`src/features/channels/ChannelsPage.tsx`、`ChannelAccountList.tsx`、`ChannelAccountDetail.tsx`、`styles/channels.css`（`ChannelConfigForm.tsx` / `ChannelLoginDialog.tsx` 仅视觉由 CSS 承接，TSX 未改）
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（§0 渠道为主 / §1 五原则 / §4-5.1 渠道页范式 / §6 交互状态）、`docs/superpowers/plans/2026-09-24-ui-ux-redesign-plan.md`（Phase 4）
- 红线：`src/bridge/**`、`src/data/**`、`useChannelLogin.ts`、表单提交逻辑、TanStack Query 缓存 Key、路由路径、业务状态机、`tests/**`、`task7.css`、`task9.css`、`layout.css`、`tokens.css`、`patterns.css` —— 零改动。

## 1. 结构映射（旧区块 → 新范式组件）

| 旧结构 | 新结构（范式组件） | 说明 |
| --- | --- | --- |
| `workbench-page-header`（h1 + page-summary + page-count 徽标） | `PageHeader`（h1「渠道」+ 因果式摘要） | 计数徽标移除；`page-count` 无测试契约 |
| 页头下方平铺的「渠道 band」（h2 + 描述 + 添加按钮 + 表格） | 每渠道一张 `SectionCard`（title=渠道名、count=账号数、action=「添加渠道账号」、description=能力摘要） | 卡带语义 |
| 账号表格 7 列（账号/状态/绑定标识/最近入站/最近投递/启用/详情） | 4 列行卡（账号/状态/启用/详情） | 见 §4；时间与账号 ID 下沉详情 |
| `span.status-label--*` | `StatusBadge`（tone 映射健康度） | 文案不变 |
| 全宽 `section.channel-account-detail`（header + dl + 操作 + 能力 + 配置） | 右列 `SectionCard`「{账号名} 详情」（action=停用/启用 + 退出账号），体内四段：身份 / 健康与错误 / 账号配置 / 渠道能力 | 段间 24px |
| `dl.descriptor-list` 键值 | `FieldRow` | 一决策一行 |
| 平铺在页面底部的 `form.test-notification-form` | 右列 `SectionCard`「测试发送」包裹同一 `form`（保留 `aria-label="测试发送"`） | 未选中账号时右列只剩它 |
| 无账号时的 `EmptyState`「暂无渠道」 | 有渠道但无账号 → `EmptyFunnel`；宿主零 descriptor → 仍 `EmptyState`「暂无渠道」 | 见 §3 |
| 页面纵向单列 | `channels-layout`（左卡带 / 右详情），≤1080px 降为单列 | Master–Detail |

## 2. 契约保持清单（逐条核对的 role / 可访问名 + 测试出处）

| 契约 | 出处（测试） |
| --- | --- |
| h1「渠道」 | `tests/helpers.ts:gotoSection`（每页 h1）|
| 渠道名 h2（如「未来渠道」） | `dynamic-adapters.spec.ts:25` |
| 账号名 button（如「主账号」`exact`） | `dynamic-adapters.spec.ts:28`；`ChannelsPage.test.tsx:197` |
| 账号名 button `aria-pressed` | `ChannelsPage.test.tsx:204` |
| **表格行 `tr` 语义**（`getByText(name).closest("tr")`） | `ChannelsPage.test.tsx:156` |
| 状态文案「服务正常 / 状态已过期 / 登录异常 / 已停用」 | `ChannelsPage.test.tsx:121/165/171/177` |
| 健康错误摘要原文 | `ChannelsPage.test.tsx:167/173` |
| `role="switch"` `aria-label="{账号} 启用"` | `ChannelsPage.test.tsx:123` |
| button「添加渠道账号」（每渠道恰一个；空态/多渠道计数） | `ChannelsPage.test.tsx:228/258/277/361`；`accessibility.spec.ts:45`；`dynamic-adapters.spec.ts:45` |
| h2「{账号} 详情」 | `ChannelsPage.test.tsx:211` |
| form `aria-label="测试发送"` + 标签「测试发送账号 / 标题 / 正文」+ button「发送测试通知」 | `ChannelsPage.test.tsx:292-296`；`dynamic-adapters.spec.ts:81` |
| button `aria-label="退出账号 {账号}"` | `ChannelsPage.test.tsx:324` |
| `role="alertdialog"` 文案「路由和游标会失效，历史保留」+ button「确认退出」 | `ChannelsPage.test.tsx:327/330` |
| SchemaForm 文案「当前版本无法编辑此字段」；无「保存配置」 | `ChannelsPage.test.tsx:347/348` |
| 对话框可访问名「登录 {渠道}」+ button「关闭登录窗口」+ Escape 关闭与焦点恢复 | `accessibility.spec.ts:48-53`；`dynamic-adapters.spec.ts:47-49` |
| 登录流程文案「配对码 / 提交配对码 / 登录成功 / 等待首条入站消息 / 验证码已提交…」 | `ChannelsPage.test.tsx:262-280`；`dynamic-adapters.spec.ts:59-73` |
| 失败态「无法读取渠道账号」+「渠道列表暂时不可用」+ button「重新检查」 | `states.spec.ts:102-106` |
| 长文本：账号名 button `expectNotClipped` + 无水平溢出 | `visual.spec.ts:73-76` |

## 3. 空态 CTA 命名方案与理由

- 空态漏斗（有渠道、零账号）标题「先连接渠道」+ 三步（选择渠道 → 扫码登录 → 验证投递）+ 主 CTA **「连接第一个渠道」**（`onClick` 打开首个渠道登录）。
- **为什么不复用「添加渠道账号」**：Playwright strict 模式下，`accessibility.spec.ts:45`、`dynamic-adapters.spec.ts:45` 以及 Vitest 的 `findByRole("button", { name: "添加渠道账号" })`（`ChannelsPage.test.tsx:228/258`）都要求**全页恰有一个**该名按钮可点。渠道卡带每张卡各有一个「添加渠道账号」（`ChannelsPage.test.tsx:361` 断言 2 渠道 = 2 个且索引 `[0]→channel-a`、`[1]→channel-b`），漏斗若同名必成第 3 个 → strict 红。故漏斗另起文案，语义更聚焦「第一步」。
- 宿主零 descriptor（无法连接）保持 `EmptyState`「暂无渠道」原行为，不误导用户去点无法完成的 CTA。

## 4. 页头主操作缺席（自主裁决）与表格→行卡

- **页头不放「添加渠道账号」**：设计稿 §5.1 想要页头主操作，但同上 strict 约束——页头同名按钮 + 每渠道一个必冲突（空态场景会出现 2 个）。测试为准，主操作下沉到**每张渠道卡右上**（语义上也更准确：添加账号必属于某个渠道）。漏斗 CTA 承接首次引导。
- **保留 `<table>` 语义、只改样式为行卡**：`ChannelsPage.test.tsx:156` 用 `getByText(name).closest("tr")` 定位账号行，**依赖 `tr`**；`role`/表格语义一旦去掉该断言必红。故不改为 div 行卡，而是 `table-layout: fixed` + 单元格拼出带圆角的 `--surface-2` 行卡（首/末单元格负责圆角与左右描边），列数由 7 收到 4（账号 / 状态 / 启用 / 详情），时间与账号 ID 下沉详情「身份」段。
- 账号名列宽 46%、状态 30%、启用 12%、详情 12%（`nth-child`，纯百分比）；状态列内错误摘要单行省略号（不在 `expectNotClipped` 断言范围内），账号名用内层 `span.channel-account-name-text` 做省略号——外层 `button` 因内层 `overflow:hidden` 承接溢出，`button.scrollWidth == clientWidth`，满足 `expectNotClipped`（已实跑 `长文本不被裁切` 通过）。

## 5. 清掉的硬编码值对照表（`channels.css`）

| 旧值 | 位置 | 新值 |
| --- | --- | --- |
| `#e7f0ed` | `.channel-account-row--selected` 背景 | `var(--color-primary-soft)` |
| `#fbefed` | `.login-state-panel--error` 背景 | `var(--color-danger-soft)` |
| `#edf7f0` | `.login-state-panel--success` 背景 | `var(--color-success-soft)` |
| `#ffffff` | `.button-danger` 文字 | `var(--color-surface)` |
| `#922d27` | `.button-danger:hover` 背景/边框 | `color-mix(in srgb, var(--color-danger) 88%, var(--color-canvas))` |
| `white`（混色） | `.button-danger-text` 边框 | `color-mix(in srgb, var(--color-danger) 45%, transparent)` |
| `rgb(22 29 25 / 42%)` | `.dialog-overlay` 遮罩 | `var(--channels-scrim)`（命名 token） |
| `rgb(25 32 28 / 22%)` | 模态阴影 | `var(--surface-3-shadow)` |
| `var(--color-surface)+var(--color-border)` | 模态面 | `var(--surface-3)` + `var(--surface-3-border)` + `var(--surface-3-blur)` |
| `z-index:100` | `.dialog-overlay` | `var(--z-modal)` |
| `calc(100vh - 32px)` | 模态 max-height | `calc(100vh - var(--space-6))` |
| `820px` | 账号表 min-width | 删除（改 `table-layout:fixed` + 百分比列宽） |
| `180px` | 二维码尺寸（2 处） | `var(--channels-qr-size)` |
| `#ffffff` | 二维码底 | `var(--channels-qr-surface)`（命名 token，扫码必须白底） |
| `92px` | `.channel-descriptor-list` 列宽 | 规则删除（改用 `FieldRow`） |
| `minmax(220px,…)` | 测试发送字段列 | `minmax(0, …)` |
| `76px` | textarea min-height | `calc(var(--space-6) * 2)` |
| `900ms` | `.spin-icon` 动画 | `var(--channels-spin-duration)` |
| `560px` | 模态宽度 | `var(--channels-dialog-width)` |
| `--font-size-1/2/3/4` | 多处字号 | `--font-caption` / `--font-body` / `--font-heading` |
| `.channel-band*` / `.channel-descriptor-list` / `.channel-detail-actions` / `.channel-config-section` / `.test-notification-heading` 整段 | 随结构移除的死代码 | 已删除，避免双定义 |

未纳入本阶段的共享硬编码（不在文件所有权内，Phase 6 处理）：`layout.css .inline-error{background:#fbefed}`、`task9.css` 若干浅底、`task7.css .switch-track{background:#aeb5af}` 与 `.schema-field--unsupported{background:#fff8e8}`。渠道页在亮色下由这些类承担的部分（InlineError、开关轨道、SchemaForm 不支持字段）沿用既有语义，暗色下由 Phase 6 统一收口。

## 6. 间距 / 字阶落地说明

- 间距只用 `--space-1..6`：组内 8px（`--space-2`，如 FieldRow 行距、段内字段间距）、组间 24px（`--space-5`，如卡带卡间距、左右分栏间距、详情四段间距）、页头留白 24px（`PageHeader` 自带 margin-bottom）。`.channels-page .channels-page-content{padding-top:0}` 抵消 `layout.css` 的重复顶距，避免 48px 双留白。
- 字阶只用别名：页头 `--font-display`（28，PageHeader 内置）、区标题 `--font-heading`（18，SectionCard 内置）、正文/账号名/健康信息 `--font-body`（14）、标签/说明/徽标/段标题 `--font-caption`（12）。
- elevation：内容卡 `--surface-1`（SectionCard/EmptyFunnel 内置），行卡 `--surface-2`，模态 `--surface-3`；分割线用 `--hairline`。
- 交互态沿用全局 `reset.css` 的 `:focus-visible` 描边；行卡 hover 用 `--color-surface-muted`，选中行用 `--color-primary-soft` + `--surface-3-border`。
- 窄窗：≤1080px 左右分栏降单列；≤900px 测试发送字段降单列；≤640px 表单底部与对话框头改纵向。

## 7. 自主决策清单

1. 页头不放「添加渠道账号」（§4，测试 strict 约束）。
2. 漏斗主 CTA 命名「连接第一个渠道」（§3）。
3. 保留 `<table>` 语义、样式改行卡，列数 7→4（§4）。
4. 账号名省略号用内层 `span` 承接溢出，保证外层 button 不被判裁切（§4）。
5. 详情健康徽标用「正常 / 需重新登录 / 登录异常 / 已停用」，与列表行文案区分，避免 `getByText("服务正常")` 命中文案重复（`ChannelsPage.test.tsx:121`）。
6. 详情「身份」不重复展示账号名（避免 `getByText("账号 account-a")` 双命中），渠道名以 `渠道名 · 渠道ID` 串联展示。
7. 测试发送保留独立 `form`（`aria-label` 契约），外层套 `SectionCard` 标题「测试发送」；region 与 form 角色不同，a11y 不冲突（已实跑 axe）。
8. 二维码白底、模态遮罩、二维码尺寸、旋转周期、模态宽度、旋转时长定义为本文件命名 token（无现成语义 token，集中不游离）。
9. 停用/启用账号按钮与「退出账号」一并放入详情卡右上操作区。

## 8. 门禁结果

- `npm --prefix apps/desktop-ui run typecheck`：通过（无输出）。
- `npm --prefix apps/desktop-ui run test -- --run`：`Test Files 10 passed`、`Tests 65 passed`（渠道 9 项全绿）。
- `npm --prefix apps/desktop-ui run build`：`✓ 1999 modules transformed`、`✓ built`。
- `PLAYWRIGHT_BROWSERS_PATH=... npm --prefix apps/desktop-ui run test:a11y`：`7 passed`（六页 + 登录对话框 Escape）。
- 额外实跑：`states / dynamic-adapters / navigation` = `10 passed`；`@visual 长文本不被裁切且关键区域不重叠` = `1 passed`。
- visual 基线红为预期（`channels.png` diff ratio 0.06，Phase 6 统一刷新），未改基线。

## 9. 未碰清单

- 未改：`src/bridge/**`、`src/data/**`、`useChannelLogin.ts`、`ChannelConfigForm.tsx`、`ChannelLoginDialog.tsx`、`app/**`、其它 `features/**`、`tests/**`、`styles/tokens.css` / `patterns.css` / `layout.css` / `task7.css` / `task9.css`、`.opencode/`。
- 业务逻辑、数据结构、数据请求、Query Key、表单提交、路由路径、状态机：零改动。
- 可访问名与 role：零改动（见 §2 全部保持）。
