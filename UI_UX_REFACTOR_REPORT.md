# Agent-notify 前端 UI/UX 视觉与交互深度重构报告

> **任务性质**：纯 UI/UX 重构（视觉风格 / Design Tokens / 布局流式防御 / CSS 样式 / 无障碍语义 / 动效与 6 态微交互）
> **执行方式**：夜间无人值守、全自主执行（主编排架构师 + 5 个子代理流水线，编码子代理统一 `opencode-go/deepseek-v4.1-flash`）
> **设计权威**：`.opencode/skills/agent-notify-ui-ux/SKILL.md`（设计分歧一律以它裁决，不中断、不等待）
> **完成时间**：2026-09-23
> **完整自主决策记录**：`apps/desktop-ui/UI_DECISIONS.log`（主编排 13 条 + 五个子代理共 60+ 条裁决）

---

## 〇、结论速览

1. **全量门禁 6 条全绿**：`typecheck` / `test`（Vitest 65 条）/ `build` / `test:a11y`（7 条）/ `test:visual`（7 条）/ `test:e2e`（25 条全量，含 200% 缩放等价视口）。
2. **业务红线零越界（git 客观取证）**：最终 `git diff` 中 `.ts` / `.tsx` 文件数为 **0**，全部改动只落在 6 个 CSS + 6 张视觉基线 PNG。Rust Bridge 接口、数据契约、数据请求、TanStack Query 缓存 Key、表单提交逻辑、路由路径、业务状态机、虚拟滚动 JS 参数——**一行未动**。
3. 整体从**浅色基线**演进为 **Cyberpunk 暗色 + 毛玻璃层次 + 克制发光**，边缘零瑕疵（无黑边/脏边/1px 缝隙漏光），交互组件 **6 态闭环**齐备，间距严格 4/8px 网格，z-index 全局位阶收口。
4. 顺手修复 **2 个既有缺陷**（均属视觉/无障碍范畴）：确认框「删除」危险按钮被 `.button` 层叠覆盖而显示为青色；`#ffffff` 在珊瑚红上对比度仅 2.27:1。
5. **需要人工做的唯一一件事**：在真实桌面窗口（Tauri 宿主）做最终视觉验收（详见第四节）。视觉基线已按技能书流程刷新，属"有意改版下的合法变化"。

---

## 一、重构覆盖面

### 1.1 样式与 Design Tokens（6 个 CSS 文件，+973 / -185 行）

| 文件 | 角色 | 主要改动 |
| --- | --- | --- |
| `src/styles/tokens.css` | **Design Tokens 唯一来源** | 浅色基线 → 暗色 Cyberpunk 阶梯（token 命名不变、值演进）；新增毛玻璃 `--glass-bg-1/2/3` + `--glass-blur-sm/md` + `--glass-saturate`、边框 `--border-subtle/-hover/-glow`、发光 `--glow-sm/-md`、动效 `--duration-fast/base` + `--ease-out`、层叠 `--z-content/sticky/popover/modal`、圆角 `--radius-pill`、语义柔和底 `--color-*-soft`×5；`color-scheme: dark` |
| `src/styles/reset.css` | 全局基础层 | `:focus-visible` 升级为高对比发光聚焦环（2px 主色描边 + offset 2px + `--glow-sm`），**无 `outline: none`** |
| `src/styles/layout.css` | 壳层 + 公共控件 | **壳层**：页壳定高 + 唯一滚动容器（防双滚动条）、导航/状态栏毛玻璃 + 单条 1px 分割线（0px 拼接偏差）、导航激活态胶囊 + 切换动效、矮窗口防遮挡、图标 4px 网格对齐、滚动条无黑带 + `scrollbar-gutter: stable`、`prefers-reduced-motion` 降级<br>**控件**：`.button`（主/次/危险）6 态、`EmptyState`、`InlineError`、`LoadingRows` 骨架屏扫描微光（`@keyframes control-skeleton-sweep`） |
| `src/styles/task7.css` | 总览 / Agent 管理 + 控件段 | 信息卡片毛玻璃分档 + 内边距统一 `--space-4`、投递表格去末行双线、Agent 选中行暗色胶囊态、配置表单节奏统一；状态徽标胶囊 + 呼吸微光（`@keyframes control-badge-breathe`，全局仅定义 1 次）、开关 6 态、SchemaForm 控件同源化 |
| `src/styles/task9.css` | 历史 / 诊断 / 设置 + 控件段 | 筛选工具栏卡片化、表格/详情卡片分档、**虚拟列表**透明行底防白闪 + `scrollbar-gutter: stable` 防抖 + 明确可视高度约束、选中态去 `!important` 暗色化、历史状态/存在性徽标胶囊化、诊断与迁移卡片统一、设置表单与 `.schema-field` **逐字同源** |
| `src/styles/channels.css` | 渠道 + 浮层与表单控件段 | 渠道三张卡片统一配方与 `--space-4`、账号选中态暗色化、账号错误条柔和红渐变、只读配置禁用态可辨识（6.04:1）；**浮层**：登录/确认对话框毛玻璃 `--glass-bg-3` + `--glass-blur-md` + 1px `--border-glow` 微光边、遮罩 `--z-modal`、图标按钮 6 态、验证码表单聚焦发光、`spin-icon` 克制发光版 |

### 1.2 组件与页面覆盖

- **公共组件**：导航（AppNav）、运行时状态栏（RuntimeStatusBar）、按钮（主/次/危险/图标按钮）、InlineError、EmptyState、LoadingRows/Skeleton、SchemaForm 控件、Switch 开关、状态徽标（status-label / delivery-state / diagnostic-level / login-state-panel / existence-value / section-count）、对话框浮层（登录 + 确认）。
- **业务页面（6/6 全覆盖）**：总览、Agent 管理、渠道、历史（含 @tanstack/react-virtual 长列表）、诊断（含迁移诊断）、设置（含更新/回执/静默时段表单）。
- **交互 6 态**：`Default` / `:hover`（抬升 1px + 描边提亮 + 柔和光晕）/ `:active`（`translateY(0) scale(0.98)` 触底回弹 + 发光收敛）/ `:focus-visible`（2px 亮色发光圈 + offset 2px）/ `:disabled`（0.45 透明度 + `not-allowed`；表单控件改用 muted 文字+底以保对比度）/ `Loading·Skeleton`（扫描微光，占位与真实内容 1:1 对齐防 CLS）。
- **重要特征**：全部 `.tsx` **零改动**——改造完全落在样式层，DOM 结构、文本、role、可访问名、事件、props 原样保留，因此对既有 65 条 Vitest 用例与 Playwright 断言的破坏风险为零。

### 1.3 明确未改动（红线区，git 已取证）

`src/bridge/**`（hostBridge.ts / types.ts / tauriHostBridge.ts / mockHostBridge.ts / index.ts）、`src/data/**`、`src/app/**`、`src/components/**`、`src/features/**`、`src/test/**`、`tests/**/*.spec.ts`、`package.json`、路由、缓存 Key、表单提交逻辑、业务状态机；其他 agent 的 Rust 改动（`crates/`、`hosts/desktop-tauri/`）未碰、未提交、未还原。

---

## 二、夜间自主决策清单（精选 28 条，全量见 `apps/desktop-ui/UI_DECISIONS.log`）

### 色彩与无障碍
1. 暗色阶梯：`--color-canvas #0a0c12` → `--color-surface #12141d` → `--color-surface-muted #1a2030`，文字 `#e9edf6` / `#949eb4`，主色青 `#2ee0f0`（与发光同源）。
2. **对比度逐对实测并留档**（WCAG，正文门槛 4.5:1）：最紧一对 danger 文字对 soft 底 **5.19:1**，text-muted 对玻璃最坏底 **5.82:1**，text 对画布 **16.67:1**。
3. `--color-surface` 保持深色不动：既有组件把它当主色底上的**文字色**（`.button`、品牌标、开关滑块），故主色提亮、正反双向均 ≥11:1，无需改组件。
4. 聚焦环选 `--color-primary` 而非 `--border-glow`：后者 35% 透明青合成后仅 **2.54:1**，低于非文本 3:1 门槛。
5. 语义柔和底 `--color-*-soft` 统一 16% 叠加，供"柔和底 + 语义亮色文字"徽标复用，组件不写裸 rgba。

### 毛玻璃与发光
6. `--glass-bg-1/2/3` + `--glass-blur-sm: blur(8px)` / `--glass-blur-md: blur(16px)` 按任务书精确值落地，另配 `--glass-saturate: saturate(1.25)` 免散落裸参数。
7. **blur 预算**：壳层 `blur-md` 一档 + 内容区 `blur-sm` 一档（同界面 ≤2 档），第二档小卡**不叠 backdrop-filter**（杜绝嵌套模糊脏边）。
8. `--glow-sm: 0 0 10px rgba(0,240,255,0.15)` 常驻柔光、`--glow-md: 0 0 20px rgba(0,240,255,0.3)` 交互强调（2 倍关系）；发光只做点缀，禁大面积辉光。
9. `.button:hover` 用 `--glow-md`（`--glow-sm` 已是主按钮常驻光，同档无感知差）。
10. **危险按钮不发光**：`--glow-*` 全为青系，红底叠青光=脏光，仅用亮度/描边/位移反馈。
11. `--border-hover: rgba(255,255,255,0.1)`（合成亮度比 subtle 高约 24%，即"hover 描边提亮约 20%"）。

### 布局与边缘零瑕疵
12. 页壳定高 + `.app-main` **唯一滚动容器**（grid `minmax(0,1fr)` + `min-height:0`），彻底杜绝双滚动条。
13. 导航与内容拼接处只留**单条 1px `--border-subtle`**，grid 轨道整数像素、描边画在 208px 列内 → 0px 偏差；根容器 `overflow:hidden` + `isolation:isolate` 收口层叠外溢。
14. 滚动条 `scrollbar-width: thin` + 透明轨道（无黑带）+ `scrollbar-gutter: stable`（切页不抖）。
15. 图标 18px → **20px**（CSS 覆盖，落 4px 网格），`inline-flex` 垂直居中，消除半像素基线抖动与 1px 上浮/下沉。
16. 卡片内边距统一收敛 `--space-4`（16px）；间距全部落 `--space-1..6`，清掉 `gap: 0/1px/2px`、`76px`、`z-index: 1`、`line-height: 1.45` 等游离值。
17. z-index 全局位阶收口：`--z-content:1 / --z-sticky:10 / --z-popover:50 / --z-modal:100`，Modal > Popover/Tooltip > Sticky > Content。

### 交互与状态
18. 动效 token：`--duration-fast: 120ms`（微反馈/按压）、`--duration-base: 150ms`（hover/切换）、`--ease-out`；组件一律消费 token，禁裸时长。
19. `@media (prefers-reduced-motion: reduce)` 全局降级（关位移/呼吸，保留状态可辨识）。
20. 骨架屏扫描微光 `control-skeleton-sweep`：条高改 `--font-size-2`（13px，与真实字高 1:1）、周期 1500ms，防 CLS 跳动。
21. 呼吸微光只给"需要注意"的语义态（danger/warning/unknown/pending/error…），成功/正常/计数静态——避免光污染。
22. 状态徽标统一体系：`--color-*-soft` 底 + 语义亮色文字 + `--radius-pill` 胶囊 + 同源呼吸发光（`currentColor` 派生）。
23. 表单 disabled 不用 `opacity:.45`（会跌破 4.5:1），改 muted 文字 + muted 底（6.04:1）。
24. 搜索框组合控件聚焦环画在容器 `:focus-within`（避免双环脏边），焦点可见性 100% 保留。

### 缺陷修复与回归策略
25. **修复既有层叠缺陷**：实测打包顺序 `channels → task9 → task7 → tokens → reset → layout`，确认框「删除」的 `.button-danger` 被后加载的 `.button` 覆盖（危险按钮实为青色）→ 提权为 `.button.button-danger*`；同时 `#ffffff` 对珊瑚红仅 **2.27:1** → 换 `var(--color-surface)`（**8.15:1**）。
26. **a11y 自愈（守门人）**：axe 抓到 Agent 选中行次级文字在半透明主色底上对比度掉到 4.5:1 临界 → 纯 CSS `color-mix()` 提亮至约 **6.8:1**；未抑制 axe 规则、未放宽断言、未改 aria/DOM。
27. **视觉基线刷新**（有意改版下的合法变化）：先逐页目检 `*-actual.png` 排除真缺陷 → `--update-snapshots` 刷新 6 张 → 连续 3 跑零差异验证稳定。**未跳过审查直接覆盖**。
28. 编排级裁决：并行子代理按"选择器段所有权 + 仅精确编辑、禁整文件覆写"避免写冲突；并行阶段禁跑 `build`/playwright 防 `dist/` 争抢假红；Playwright 浏览器按仓库 `tools/ui/gate.ps1` 约定装到 D 盘（AGENTS.md：下载物不落 C 盘）。

---

## 三、门禁通过证明（主编排最终独立复跑，非子代理转述）

### 1) `npm --prefix apps/desktop-ui run build` ✅
```
> agentnotify-desktop-ui@2.0.6 build
> tsc -b && vite build

vite v8.3.0 building client environment for production...
✓ 1989 modules transformed.
dist/index.html                   0.41 kB │ gzip:   0.27 kB
dist/assets/index-BykWOTH6.css   56.97 kB │ gzip:   8.18 kB
dist/assets/index-zelq9thB.js   427.56 kB │ gzip: 127.82 kB
✓ built in 1.39s
```
（改造前 CSS 38.09 kB → 56.97 kB，增量即暗色阶梯/毛玻璃/6 态/骨架屏；JS 427.56 kB **零变化**，佐证业务逻辑零改动。）

### 2) `npm --prefix apps/desktop-ui run test:a11y` ✅
```
Running 7 tests using 7 workers
  ok 1 ... @a11y Agent 管理 没有严重可访问性问题 (5.3s)
  ok 2 ... @a11y 总览 没有严重可访问性问题 (5.2s)
  ok 3 ... @a11y 渠道 没有严重可访问性问题 (6.7s)
  ok 4 ... @a11y 登录对话框支持 Escape 关闭并恢复焦点 (5.1s)
  ok 5 ... @a11y 设置 没有严重可访问性问题 (5.7s)
  ok 6 ... @a11y 诊断 没有严重可访问性问题 (5.6s)
  ok 7 ... @a11y 历史 没有严重可访问性问题 (6.4s)
  7 passed (20.0s)
```

### 3) `npm --prefix apps/desktop-ui run test`（Vitest）✅
```
 Test Files  10 passed (10)
      Tests  65 passed (65)
   Duration  9.78s
```
（含业务分支断言：`preserves unsupported values and clears a submitted secret input`、`sends every supported filter to the history command`、`keeps login sessions isolated by channel`、`blocks invalid numeric and quiet-hour values before calling the host` 等全绿。）

### 4) `npm --prefix apps/desktop-ui run typecheck` ✅
```
> tsc --noEmit
（无输出 = 通过）
```

### 5) `npm --prefix apps/desktop-ui run test:visual` ✅（7 条，刷新后连续 3 跑零差异）
### 6) `npm --prefix apps/desktop-ui run test:e2e` ✅（全量）
```
Running 25 tests using 8 workers
  ok ... dynamic-adapters (3) | accessibility (7) | navigation (2) | states (6) | visual (7)
  25 passed (34.3s)
```

> 门禁执行环境：Playwright 浏览器位于 `D:\Tools\playwright-browsers`，执行命令需带 `$env:PLAYWRIGHT_BROWSERS_PATH = 'D:\Tools\playwright-browsers'`（或直接用仓库规范入口 `tools\ui\gate.ps1`）。

---

## 四、视觉基线变更说明与人工验收建议

`tests/visual.spec.ts-snapshots/` 下 6 张基线（agents / channels / diagnostics / history / overview / settings）**已刷新**。这是**有意的整体暗色 Cyberpunk 改版**导致的合法变化，按技能书"基线更新流程"执行：

| 页面 | 逐页目检结论（对照旧浅色基线） | 刷新原因 |
| --- | --- | --- |
| 总览 | 暗色画布正确、三张信息卡毛玻璃分层清晰、无浅色亮块/黑边/裁切 | 整体暗色改版 |
| Agent 管理 | 选中行主色胶囊态正常、超长中文名换行不裁切、长 ID 不溢出 | 同上 + 选中行次级文字提亮 |
| 渠道 | 卡/列表/详情分层一致、选中账号胶囊态正常、长标题不溢出 | 同上 |
| 历史 | 筛选卡/表格卡/详情层次正确、虚拟列表行底透明无白闪色差 | 同上 |
| 诊断 | 迁移/摘要/诊断项卡片统一、语义徽标可读、无浅色残留 | 同上 |
| 设置 | 表单控件毛玻璃底同源、开关行对齐、长中文不裁切 | 同上 |

**建议人工验收（唯一待办）**：在真实 Tauri 桌面窗口下（1280×800、最大化、以及 640×400 矮窗各看一遍）确认视觉效果与窗口缩放行为；重点看毛玻璃在真实窗口合成下的观感、导航激活胶囊、状态徽标呼吸微光、对话框浮层层次。真实视觉效果需在真实桌面窗口验证后才算最终完成（技能书第七节要求）。

---

## 五、环境与工具链变更（需知悉）

1. **Playwright 浏览器**：预检发现 `test:a11y` 因缺浏览器全红（`Executable doesn't exist`，纯环境问题）。已按仓库 `tools/ui/gate.ps1` 的既有约定安装到 **`D:\Tools\playwright-browsers`**（chromium-1243 + chromium_headless_shell-1243），**未落 C 盘**（遵守 AGENTS.md）。
2. **用户级环境变量**：已设置 `PLAYWRIGHT_BROWSERS_PATH=D:\Tools\playwright-browsers`，防止将来裸跑 `npx playwright install` 又把浏览器下回 C 盘。注意进程环境不保证继承用户级变量，故门禁命令建议显式带上该变量（本报告第三节注）。
3. 未改动 `package.json`、未新增依赖、未改动任何构建/发布脚本；**未改版本号、未打 tag、未发版**（发版涉及 `VERSION`/签名/镜像严格流程，本次未获授权）。改动已按 Conventional Commits 本地提交，**未 push**。

---

## 六、发现但未处置事项（留给后续）

1. `src/styles/global.css`：**未被任何模块引用**（grep 零命中），属历史遗留死文件（内含浅色 `--color-*` 硬编码与重复的 `.app-shell` 规则）。按"只改需求相关代码、不顺手重构"原则**保持原样**，建议后续单独确认后删除或归位。
2. `task7.css` / `task9.css` 按"历史任务"切分而非按组件层次切分，长期看不利于复用；本轮为遵守"不顺手重构"未做拆分，仅通过选择器段注释明确职责边界。
3. 视觉基线刷新后建议由人工做一次真实桌面窗口验收（见第四节），确认后本轮即可视为完整交付。
