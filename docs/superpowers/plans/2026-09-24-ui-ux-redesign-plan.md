# Agent-notify 第二轮 UI/UX 重构 · 实施计划

- 日期：2026-09-24
- 依据：`docs/superpowers/specs/2026-09-24-ui-ux-redesign-design.md`（设计稿定稿：气质 B、呼吸档位 1、渠道为主、明暗双主题一等公民）
- 说明：`writing-plans` skill 未安装，本计划按同规范手写
- 执行模型：子代理统一 `opencode-go/deepseek-v4.1-flash`；主编排负责派发、独立验证、门禁收口与提交

## 执行纪律（每阶段强制）

1. **业务红线零改动**：`src/bridge/**`、业务逻辑、数据结构、数据请求、TanStack Query 缓存 Key、表单提交逻辑、**路由路径**、业务状态机。
2. **可访问名与 role 尽量原样保留**（无障碍契约 + 测试契约双重要求），如「添加渠道账号」「关闭登录窗口」「全局暂停」、`role="switch"`、h2「… 详情」。
3. **测试同步规则**：只允许同步"选择器/预期顺序"类断言，**断言意图不变**（可访问名 / 无裁切 / 无溢出 / 无重叠 / 键盘顺序与视觉顺序一致）；禁止放宽断言凑绿。
4. **编辑纪律**：文件级所有权 + 精确字符串替换、禁整文件覆写；不格式化无关文件。
5. **工程底线**：4/8px 网格、无魔法数字（token 化）、对比度 ≥4.5:1（正文）/≥3:1（非文本）、focus-visible 可见、reduced-motion 降级、双主题下都成立。
6. **每阶段门禁**：`typecheck` + `test -- --run` + `build`；**a11y 每阶段必跑**；视觉基线在 Phase 6 统一刷新（中途红为预期）。
7. **决策日志**：`apps/desktop-ui/UI_DECISIONS.d/<phase>.md`，主编排收口时合并进 `UI_DECISIONS.log`。
8. **提交**：每阶段完成后主编排独立复验再提交；只暂存本轮相关文件。

## Phase 1+2｜双主题 token 体系 + 主题机制 + 渠道优先导航（本次派发）

**目标**：语义角色 token 不变、新增"表面语义层"使组件零分支换肤；亮色主题完整设计（非反色）；主题切换（明/暗/跟随系统）+ 记忆；导航顺序改渠道优先。

**文件**：`styles/tokens.css`、`app/theme.ts`(新)、`components/ThemeSwitcher.tsx`(新)、`components/AppNav.tsx`、`app/navigation.ts`(仅顺序)、`main.tsx`(首帧应用)、`styles/layout.css`(仅 .app-nav* 段 + 开关样式)、`tests/playwright.config.ts`(desktop 项目显式 `colorScheme:'dark'`)、`tests/navigation.spec.ts`(Tab 顺序同步)。

**关键点**：
- 新增表面语义层：`--surface-1/2/3`、`--surface-*-blur`（dark=glass-blur、light=none）、`--surface-*-border`、`--surface-*-shadow`（dark=微光、light=elevation 1/2/3）、`--hairline`、`--elevation-1/2/3`。
- 亮色：画布 `#f5f7fa` 级 / 面 `#fff` / 文本 `#1a2230` 级 / 次级 `#5b6675` 级 / 主色深青 `#0e7490` 级 / 语义色暗一档 / 柔和底加深；零玻璃零辉光。
- CSS 只认 `:root[data-theme="dark"|"light"]`（`:root` 默认给 dark 兜底）；`color-scheme` 随主题。
- 对比度：两套逐对核算，数值进决策日志。
- 主题机制：`localStorage['agentnotify.theme'] ∈ {dark,light,system}`，system→`matchMedia`，监听系统变化；`main.tsx` 渲染前应用（防闪变）。
- 开关：导航底部三段控件，键盘可达、focus-visible、有可访问名。

**门禁**：typecheck + vitest + build + a11y；visual 红为预期（Phase 6 刷新）。

## Phase 3｜范式骨架与公共组件

- 新视图层组件：`PageHeader` / `KpiCard` / `SectionCard` / `FieldRow` / `StatusBadge` / `EmptyFunnel`（渠道漏斗空态）/ `SkeletonRows`；新建 `styles/patterns.css`（main.tsx 引入）。
- 五原则落地为**唯一一套**间距/字阶/elevation 规则；旧规则随页面迁移逐个删除，不堆死代码。
- 兼容策略：先并存后迁移；每迁一页，旧段删干净。

## Phase 4｜渠道页（第一优先业务页）

- 结构：页头（因果说明 + 「添加渠道账号」）→ 渠道卡带 → 账号行卡 → 右详情四段（身份/健康/配置/测试）。
- **空态 = 全 App 最重要空态**：锚点「先连接渠道」+ 三步说明 + 主 CTA。
- 登录对话框：流程逻辑零改动，仅视觉重排（模态玻璃/微光边/矮窗内部滚动）。

## Phase 5｜其余五页（5a Agent → 5b 总览 → 5c 历史 → 5d 诊断 → 5e 设置）

- 5b-5e 文件互不重叠时可并行；设置页按 §5.2（左目录 + 一决策一行）。
- 总览：KPI 带渠道健康居首 + 例外面板前置 + 投递流时间分组。
- 历史：筛选带收拢、固定列轨道、虚拟列表防白闪、双栏底对齐。

## Phase 6｜双主题回归 + 全量门禁 + 视觉矩阵

- `playwright.config.ts`：新增 `light` 项目（`colorScheme:'light'`）跑 `@visual`/`@a11y`；dark 项目同样跑（矩阵 = 6 页 × 2 主题）。
- 视觉基线：先逐页目检 actual（两主题）排除真缺陷 → `--update-snapshots` → 3 连跑验证稳定。
- 全量：typecheck / vitest / build / a11y / visual / e2e + 仓库级 `go test ./...`、`go vet ./...`、`gofmt -l cmd internal`。
- 收口：合并决策日志、更新报告（第二轮章节）、git diff 红线取证、提交。

## 风险与回退

| 风险 | 缓解 |
| --- | --- |
| 双主题遗漏硬编码色值 | 每阶段扫描领地内裸 hex/rgba；亮色下人工目检 |
| 测试契约漂移（可访问名被改） | e2e/a11y 每阶段兜底；名字类断言零改动 |
| 视觉矩阵抖动（毛玻璃/呼吸） | `animations:'disabled'` + 3 连跑 |
| 中途视觉门禁红 | 预期内（基线 Phase 6 刷新），以 typecheck/vitest/build/a11y 为阶段门槛 |
| 工作区并行 agent 冲突 | 只暂存本轮文件；每阶段前 `git status` 检查 |
