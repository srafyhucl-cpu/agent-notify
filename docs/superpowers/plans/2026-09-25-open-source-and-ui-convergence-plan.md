# 开源收尾与 UI 打磨汇合路线图

- 日期：2026-09-25
- 状态：已与用户确认方向；等待阶段 1（GitHub 设置）与阶段 2（UI 收尾）完成
- 适用对象：本仓库上的全部并行会话（OpenCode / Codex / 人工）
- 依据：`docs/superpowers/plans/2026-09-24-open-source-hardening-implementation.md`（开源加固，任务 1–8 已完成）、`docs/superpowers/plans/2026-09-24-ui-ux-redesign-plan.md`（第二轮 UI 重构，已完成）

## 1. 结论

两条线本身不冲突，冲突的是三件事：**一份仓库两套历史**、**两条线改到同一批公共文件**、**"汇合 + 发版"无人负责**。
因此不限制并行，但把 **汇合（阶段 3）** 与 **发版（阶段 4）** 钉成单人、单次、串行的动作。

## 2. 现状事实（2026-09-25 核查）

| 项 | 事实 |
| --- | --- |
| 公开仓 | `srafyhucl-cpu/agent-notify`，已为 **public**；373 提交，MIT，社区概况 85% |
| 开源加固 | 任务 1–8 已推到 `origin/main`（今天 10:22 那 8 个提交）；历史脱敏已完成 |
| 脱敏验证 | 真实账号哈希在 `origin/main` 全历史命中 **0**；在本地 `main` 命中 **3** 个提交，工作树文档中仍有 3 处 |
| 本地 UI | 第二轮重构 20 个提交在本地 `main`；第三轮打磨 **51 个文件未提交**（用户有意停在半路，继续中） |
| 分叉 | 本地与 `origin/main` 各自领先 337 / 326 个提交，**不可直接互推**（推会被拒，属安全网） |
| 公开仓 CI | **全红**：2026-09-25 11:41 起所有 workflow 任务 3 秒内失败、runner 未分配（`steps=0`）；09-24 01:56 尚正常。高度疑似账户级 Actions 计费/额度拦截（加固计划亦记录过"账单拒绝"） |
| 遗留分支 | 本地另有 `integration/published-base`（脱敏前孪生分支，含真实值）、`chore/open-source-hardening`、6 个 `codex/*`；工作树 3 个（`D:\Temp\agent-notify-hardening`、`D:\Temp\agent-notify-published-base`、`C:\Users\srafy\.codex\worktrees\cdf5\Agent-notify`） |

## 3. 优先级原则

对外、不可逆的先做；对内、可逆的后做。
开源线推错历史 = 永久泄漏，公开仓 CI 红 = 人人可见；UI 线晚一天发布无人察觉。

## 4. 阶段

### 阶段 0｜冻结与上保险（已完成，2026-09-25）
- 全量历史 bundle、未提交改动补丁、未跟踪文件副本：见 `D:\Temp\agent-notify-backup-20260925`（含 `bundle-verify.txt`）。
- UI 打磨保持"进行中"状态，冻结期不触碰 `apps/desktop-ui/**` 与桌面壳文件。

### 阶段 1｜让公开仓可用（执行者：用户；半天）
1. 解除 Actions 拦截：GitHub → Settings → Billing，或仓库 Actions 页红色横幅。
2. 打开安全开关：Dependabot alerts、Security updates、Code scanning、Secret scanning、Push protection、Private vulnerability reporting。
3. `main` 加分支保护（要求 CI、禁 force push）；`v*` 加 tag ruleset；开启合并后自动删分支。
4. 补 `CODE_OF_CONDUCT.md`（社区概况只差这一项）。
- 验收：`main` 上 CI 变绿，6 个 Dependabot PR 能跑门禁。

### 阶段 2｜UI 打磨收尾（执行者：UI 会话）
- 门禁：`typecheck` / `vitest` / `build` / `a11y` / `visual` / `e2e` + `tools\rust\gate.ps1`。
- 真机目检：六页 × 明暗双主题 + 自绘标题栏（最小化 / 最大化 / 拖拽 / 托盘图标）。
- 整理为清晰提交（只暂存本轮相关文件）。

### 阶段 3｜汇合到干净历史（唯一有技术风险，必须一次做成）
- **只允许 rebase / cherry-pick**，把本地 UI 提交重放到 `origin/main` 之上；**禁止 merge**、禁止把本地 `main` 直接推上去。
- 冲突按"两边都要"处理：UI 的构建缓存复用 + 线上的 `--locked` / 签名清单 / 发布门禁。
- 落地后核对：真实账号哈希零命中、提交历史无脏提交、全部门禁绿。
- 走 PR 合入 `main`（留 CI 记录）。

### 阶段 4｜收尾与发版
- 处理 6 个 Dependabot PR；排查 Dependabot cargo 更新任务失败；社区概况刷到 100%。
- 发 v2.0.7：tag → Release workflow → 双仓资产摘要一致 → 安装器与 ZIP 签名校验 → 真机覆盖升级 + 微信收到消息。

## 5. 红线

1. 本地 `main`（含未脱敏历史）**永不** `push --force` / `push --mirror` 到公开仓。
2. 汇合只走 rebase / cherry-pick；合并会把脏历史带进公开仓。
3. 阶段 1 未完成前不发版：CI 跑不了就没有发布门禁。
4. 阶段 2 未完成前不汇合：避免同一批改动搬两次。
5. 只暂存本次相关文件，不 `git add -A`。

## 6. 收尾清理（需用户确认后执行，逐项明确路径）

- 三个遗留 worktree 与本地孪生分支（`integration/published-base` 含真实值，最优先退役）。
- 备份目录 `D:\Temp\agent-notify-backup-20260925` 在阶段 3 完成并验证后清理。
