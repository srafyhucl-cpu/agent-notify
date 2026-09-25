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

## 7. 进展记录

### 2026-09-25｜阶段 1 完成（公开仓侧）

- `main` CI 首次全绿。此前红的三个原因全部确证并修复（PR #12，已合并）：
  1. 账户级 Actions 拦截——表现为任务 3 秒失败、不分配 runner（用户侧计费处理后在重跑中恢复）；
  2. `tools/rust/export-bindings.ps1` 写死本机 `D:\Tools\cargo` 约定 → runner 上找不到 cargo；
  3. 发布清单夹具 `RELEASE-MANIFEST.p7s` 签的是 CRLF 字节、仓库存的是 LF → 4 个用例在 CI 上必然失败。
- Dependabot cargo 更新失败根因（日志原文 `dependency_file_not_resolvable`：`error: target tuple in
  channel name 'stable-x86_64-pc-windows-msvc'`）→ 已改为 `channel = "stable"`。复验还需一次 Dependabot
  运行（自动重试，或 Dependency graph → Dependabot → `Cargo.toml` 行的 `⋯` → Check for updates）。
- 仓库设置核对结果：分支保护（strict、4 项必需检查、禁 force push、enforce admins）、`v*` tag ruleset、
  secret scanning + push protection、Dependabot 告警与安全更新、私密漏洞报告、合并后自动删分支——均已开启。
- 社区概况补齐：新增 `CODE_OF_CONDUCT.md`（举报邮箱 srafyhucl@gmail.com，PR #13）。
- 依赖 PR：三个 CodeQL action PR 合为一个统一升级到 v4.38.2（避免工作流 v3/v4 混用）；npm 补丁升级逐个合并。
- strict 保护的副作用：每次合并都会让其余 PR 过期，必须"更新分支 → 重跑门禁 → 合并"逐个推进。

### 待办｜已知不稳定测试（未修，需单独排期）

- 现象：`hosts/desktop-tauri/tests/production_contract.rs:375` 的 `assert!(!diagnostics.components.is_empty())`
  在 CI 上偶发失败（约 1/7 次）；本机连跑 15 次全过，未复现。失败运行：`actions/runs/36130870555`。
- 已确证存在的可疑代码（是否为该 flake 的根因未确证）：`crates/agentnotify-runtime/src/supervisor.rs` 中
  `set_state()` 用 `if let Ok(...) = self.inner.write()`、`components()` 用 `unwrap_or_default()`，两者都
  **静默吞掉锁中毒**。影响不止测试：界面「诊断 → 后台组件」会静默显示为空且不报错，与「失败要明确暴露、
  不做猜测式兜底」的约定冲突。
- 建议修法（未实施）：改为显式从中毒锁恢复（`into_inner()`）并补"中毒后仍能读到组件"的用例；若根因是
  启动竞态，则需让运行时启动在返回前完成组件注册。

### 2026-09-25 续｜安全告警与依赖清理（阶段 1 收尾）

- **锁中毒已修**（PR #15，已合并）：`set_state` / `report_failure` / `components` 三处改为显式从中毒锁恢复，
  并补用例 `poisoned_lock_still_reports_and_updates_components`；`production_contract` 的断言失败时会打印
  运行时状态与全部诊断项 code，便于下次复现直接定位。**偶发失败本身仍未复现**，本项不算已根治。
- **Dependabot cargo 修复已验证生效**：错误从 `target tuple in channel name` 变为
  `security_update_not_possible`，说明通道名问题已解决，剩下的是真实依赖约束。
- **安全告警 3 → 1**：
  - `time` 0.3.45 → 0.3.47、`serde_with` 3.17.0 → 3.21.0（PR #16，锁文件升级，本机全量 Rust 门禁通过）；
  - `glib` 无法在仓库内修复（被 `tauri → gtk 0.18` 钉死，且 Windows 目标下不编译），已按
    「vulnerable code is not used」带原因关闭，评论中写明依据与「未来支持 Linux/macOS 时重新评估」。
- **依赖 PR 清理完成**：CodeQL action 统一升到 v4.38.2（#14），@types/node / jsdom / @tanstack/react-query
  三个补丁升级逐个合并（#6/#7/#8），三个重复的 CodeQL PR 关闭（#9/#10/#11）。合并后开放 PR 归零。
- 提醒：分支保护是 **strict** 模式，任何合并都会让其余 PR 过期，必须"更新分支 → 重跑门禁 → 合并"逐个推进；
  一次合并 ≈ 一轮完整 CI（Rust 任务约 12–20 分钟）。
