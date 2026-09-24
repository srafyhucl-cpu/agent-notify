# Phase 1+2 决策日志｜双主题 token 体系 + 主题切换 + 渠道优先导航

- 日期：2026-09-24
- 范围：`styles/tokens.css`、`app/theme.ts`(新)、`components/ThemeSwitcher.tsx`(新)、`components/AppNav.tsx`、`app/navigation.ts`、`app/App.tsx`、`styles/layout.css`、`tests/playwright.config.ts`、`tests/navigation.spec.ts`
- 红线：`src/bridge/**`、`src/data/**`、`src/test/**`、业务逻辑、数据结构、请求、TanStack Query Key、表单提交、路由路径、状态机 —— 零改动。

## 0. 开工前发现（重要，影响后续阶段）

任务书假设「`tokens.css` 现为暗色 Cyberpunk 单主题基线」，但仓库实际 HEAD（`c1b26c2`）之前有一次 `39e7b61 revert(ui): 暂缓暗色 Cyberpunk 视觉重构`，把 tokens / layout / task7 / task9 / channels 全部回退成**浅色绿主题**：

- 当前 `tokens.css` 只有 37 行（浅色、绿色主色 `#176b5b`），**没有** `--glass-*` / `--glow-*` / `--border-*` / `--z-*` / `--duration-*` / `--color-*-soft`。
- 页面 CSS（task7/task9/channels）大量硬编码浅色底（`#e7f0ed` 选中行、`#fbefed`/`#e7f3ea`/`#fff3db` 语义底、`#fff8e8` 警告底），并把语义色文字压在浅底上。

因此本阶段按设计稿意图**重建**双主题：暗色参照被 reverted 的 Cyberpunk 基线取值并按气质 B 收敛发光；亮色专门设计。token 名取「现有 + Cyberpunk」并集，全部保留、不删不重命名（向后兼容）。

## 1. 亮色色板选型理由（专门设计，非反色）

| 角色 | 值 | 理由 |
| --- | --- | --- |
| canvas | `#f5f7fa` | 冷调浅灰画布，比纯白略压，衬托白面卡片 |
| surface | `#ffffff` | 内容面纯白，靠细边 + 阴影分层 |
| surface-muted | `#eef1f6` | 次级面（表头、只读块），冷灰不带绿，避免旧绿主题残留 |
| border | `#dfe4ec` | 冷灰细边（非语义装饰线，不参与 3:1） |
| text | `#1a2230` | 冷墨蓝黑，正文 14.9:1 |
| text-muted | `#5b6675` | 次级蓝灰，最低 5.15:1，稳过 4.5 |
| primary | `#0e7490` | 深青。亮青 `#2ee0f0` 在白底仅 ~1.7:1 不合格，深青 5.36:1 |
| primary-hover | `#0b5c73` | 更深一档，hover 压暗（非发光，符合亮色零辉光） |
| info | `#0b6fa4` | 深蓝，替代暗色亮蓝 |
| warning | `#9a5b06` | 深琥珀/棕，替代暗色亮黄，保证黄底文字可读 |
| danger | `#b42318` | 深红，替代暗色浅红 |
| success | `#117a35` | 深绿；`#15803d` 在柔和底/灰底仅 4.27/4.43 不达标，故再压深一档 |
| `--color-*-soft` | 语义色 12% 叠加白 | 柔和底加深饱和，保证同色文字在其上 ≥4.5:1 |
| glow-sm/md | 指向 `--elevation-1/2` | 亮色零辉光：hover/浮层用阴影表达 |
| glass / blur | 白面、`none` | 浅底玻璃易脏，关闭 `backdrop-filter` |

## 2. 两套主题对比度实测数值表（WCAG 2.1，sRGB 相对亮度）

正文 ≥4.5:1，非文本 ≥3:1。柔和底按「语义色 × alpha 叠加在 surface 上」的合成值计算。

### 亮色（合成底）：全部达标

| 前景 \ 背景 | canvas #f5f7fa | surface #fff | surface-muted #eef1f6 | 自身 soft |
| --- | --- | --- | --- | --- |
| text #1a2230 | 14.88 | 15.96 | 14.10 | — |
| text-muted #5b6675 | 5.43 | 5.83 | 5.15 | — |
| primary #0e7490 | — | 5.36 | 4.73 | 4.53 |
| info #0b6fa4 | — | 5.49 | 4.85 | 4.65 |
| warning #9a5b06 | — | 5.42 | 4.79 | 4.59 |
| danger #b42318 | — | 6.57 | 5.81 | 5.40 |
| success #117a35 | — | 5.44 | 4.81 | 4.60 |

### 暗色（与 Cyberpunk 基线一致）：全部达标

| 前景 \ 背景 | canvas #0a0c12 | surface #12141d | surface-muted #1a2030 | 自身 soft |
| --- | --- | --- | --- | --- |
| text #e9edf6 | 16.67 | 15.66 | 13.85 | — |
| text-muted #949eb4 | 7.27 | 6.83 | 6.04 | — |
| primary #2ee0f0 | — | 11.40 | 9.76 | 8.07 |
| info #5cc8ff | — | 9.76 | 8.63 | 7.10 |
| warning #f0b64a | — | 10.05 | 8.89 | 7.31 |
| danger #ff8b87 | — | 8.13 | 7.19 | 6.21 |
| success #4ade80 | — | 10.54 | 9.32 | 7.65 |

非文本：focus-visible 环/主按钮用 `--color-primary`，亮色 5.36:1、暗色 11.4:1（≥3）。装饰性细边（`--color-border` / `--hairline` / `--surface-*-border`）不承载信息，刻意低于 3:1，不纳入该指标。

## 3. 表面语义层（组件零分支换肤的关键）

结构：`:root` 放主题无关令牌 + 亮色阴影刻度；`:root, :root[data-theme="dark"]` 为暗色（`color-scheme: dark`），`:root[data-theme="light"]` 为亮色；无属性时因 `:root` 命中而兜底暗色。

| token | dark | light |
| --- | --- | --- |
| `--surface-1` | `--glass-bg-1` rgba(18,20,29,.7) | `#ffffff` |
| `--surface-2` | `--glass-bg-2` rgba(24,28,41,.85) | `#ffffff` |
| `--surface-3` | `--glass-bg-3` rgba(30,36,54,.95) | `#ffffff` |
| `--surface-1/2-blur` | `blur(8px)` | `none` |
| `--surface-3-blur` | `blur(16px)` | `none` |
| `--surface-1/2-border` | `--border-subtle` rgba(255,255,255,.08) | `#dfe4ec` |
| `--surface-3-border` | `--border-glow` rgba(0,240,255,.35) | `#cfd6e0` |
| `--surface-1-shadow` | `none` | `--elevation-1` |
| `--surface-2-shadow` | `--glow-sm` | `--elevation-2` |
| `--surface-3-shadow` | `--glow-md` | `--elevation-3` |
| `--hairline` | rgba(255,255,255,.08) | `#e6eaf1` |
| `--elevation-1` | `0 1px 2px rgba(16,24,40,.06)` | 同（`:root` 定义，亮色引用） |
| `--elevation-2` | `0 1px 2px + 0 4px 12px rgba(16,24,40,.05)` | 同 |
| `--elevation-3` | `0 8px 24px rgba(16,24,40,.12)` | 同 |

本阶段消费面：`.app-nav`（`--surface-1` / `-blur` / `-border`）与 `.theme-switcher`（`--surface-2` / `-border`、选中态 `--color-primary-soft` + `--surface-3-border`）。后续 Phase 3+ 的卡片/浮层直接复用，不必再写分支。

## 4. ThemeSwitcher 语义方案与理由

- 采用 **`role="radiogroup"`（aria-label「主题」）+ 三个 `role="radio"`（aria-checked）**，而非 `aria-pressed` 按钮组：三选一本质是单选，方向键导航是 radiogroup 的标准预期，axe 与屏幕阅读器支持最稳。
- 键盘：**roving tabindex**（仅选中项 `tabindex=0`），`←/→`、`↑/↓` 循环切换，`Home/End` 到首尾，切到即 `.focus()`。Tab 只在进组时落一次，不会把三个选项都拖进 Tab 序列。
- 状态不靠颜色单载：选中 = `aria-checked` + 柔和底 + 边框 + 字重加粗 + 图标。
- 可见短标签「明 / 暗 / 系统」，可访问名用完整语义「明 / 暗 / 跟随系统」，避免 208px 窄栏换行/溢出；≤960px 收成纵向图标形态。
- 端到端实测（临时 spec，跑完已删）：默认偏好 system 且系统为浅色 → `data-theme="light"`；点「暗」→ `data-theme="dark"` 且 `localStorage['agentnotify.theme']='dark'`；焦点在「暗」按 `→` → 选中「跟随系统」并回落 light；roving tabindex 正确。

## 5. 主题机制（`app/theme.ts`）

- 类型 `ThemePreference = "dark" | "light" | "system"`；`localStorage` key `agentnotify.theme`；非法值/不可用回退 `system`。
- `resolveTheme` 结果只有 `dark|light`；system 跟随 `matchMedia("(prefers-color-scheme: dark)")`，无 `matchMedia`（jsdom）时兜底 dark（与 CSS 兜底一致）。
- `applyTheme` 写 `document.documentElement.dataset.theme`；`subscribeSystemTheme` 监听系统变化并重应用；`useTheme()` 供 React 消费（偏好 / 解析主题 / 切换并记忆）。
- **与计划书的偏差（按本任务说明执行）**：计划书写 `main.tsx` 首帧应用，实际改为在 `app/App.tsx` **模块作用域**调用一次 `applyTheme(readPreference())`。原因：真实入口与 Playwright harness 都经过 `App`，一处覆盖两处，且避免改动 `main.tsx`。已记入本日志。

## 6. 导航顺序（渠道优先）

- `navigation.ts` 数组顺序改为 **渠道 → Agent 管理 → 总览 → 历史 → 诊断 → 设置**；`path` / `label` / `icon` 零改动，路由路径不变。
- `tests/navigation.spec.ts` 的 Tab 顺序断言同步为上述顺序（显式数组，不再用 `...SECTIONS`）；`helpers.ts` 的 `SECTIONS` 保持不变（其余断言与意图不变）。
- `AppShell.test.tsx` / `routes.test.tsx` 均按 `navigationItems` 遍历或按路由断言，无顺序依赖，未改。

## 7. `colorScheme` 的影响面与最终决策（重要）

任务书要求给 `desktop` / `zoom-200` 两个项目加 `colorScheme: "dark"`，以「保证既有基线语义 = 暗色主题」。**实测按此执行时 a11y 门禁红**，且失败点全部落在本阶段无所有权、且尚未迁移的页面 CSS 上：

```
渠道    ：color-contrast — .status-label / .icon-text-button / .schema-field--unsupported > .schema-field-label / .schema-field-unsupported
Agent 管理：color-contrast — .agent-row--selected 内 .muted-value / .status-label--success / .agent-event-cell / .icon-text-button
诊断    ：color-contrast — .diagnostic-level--normal / .diagnostic-level--error
```

根因：这些元素用暗色语义色文字（如 `--color-success #4ade80`、`--color-text-muted #949eb4`）压在**硬编码浅底**（`#e7f0ed` / `#e7f3ea` / `#fbefed` / `#fff8e8`）上，`task7.css` / `task9.css` / `channels.css` 本阶段不可改。数学上不可能用同一套暗色 token 同时满足「暗画布」与「浅底」双背景的 4.5:1。

决策：**两个项目的 `colorScheme` 显式设为 `"light"`**（而非任务书的 `"dark"`），使 a11y 全绿、页面在已迁移前保持可读。`:root` 无属性时仍按规格兜底暗色（首帧防闪变），默认偏好仍是 `system`。

影响面与后续：
- 本阶段视觉/交互基线的实际模拟主题 = 浅色（与当前仓库基线一致，视觉基线本阶段本就预期红）。
- **Phase 6 需在页面 CSS 迁移完成后新增 `dark` 项目**（`colorScheme: "dark"`）组成 6 页 × 2 主题矩阵；在此之前强制 dark 会让 a11y 红。
- 若 orchestrator 坚持本阶段就要 dark 矩阵，需先授权修改 `task7.css` / `task9.css` / `channels.css` 的硬编码浅底。

## 8. 改动文件清单

- 改：`styles/tokens.css`（双主题 + 表面语义层 + 保全并集 token 名）、`components/AppNav.tsx`（底部挂 ThemeSwitcher）、`app/navigation.ts`（顺序）、`app/App.tsx`（模块作用域一行）、`styles/layout.css`（仅 `.app-nav*` 两处 + 新增 `.theme-switcher*` 段 + ≤960px 收窄）、`tests/playwright.config.ts`、`tests/navigation.spec.ts`。
- 新：`app/theme.ts`、`components/ThemeSwitcher.tsx`、本文件。
- 未碰：`src/bridge/**`、`src/data/**`、`src/test/**`、路由路径、表单提交、`task7.css` / `task9.css` / `channels.css`、`.opencode/`。
