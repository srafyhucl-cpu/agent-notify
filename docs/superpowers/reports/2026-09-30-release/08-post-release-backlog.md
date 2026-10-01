# 发布后 backlog / bug 迭代清单（不阻塞 v2.1.0）

- 日期：2026-10-01
- 定位：发版后按需处理，**不阻塞 2.1.0 发布**。ClawBot 推送会话机制另见 `07-clawbot-push-session-research.md`。

## B1 Codex 通知中断（高优先级，发布后迭代）
- **现象**：最后一条 Codex 通知为 `2026-10-01T00:54:51Z`；此后 Codex 会话持续活跃（rollout 写到 10:17），但 `notifications` 表无新增 codex 行。应用侧管道正常（OpenCode 通知持续 `Sent`、spool 无积压、无推送失败日志）。
- **根因（高置信）**：08:33:24 `.codex/config.toml` 的 `notify` 链被 `codex-computer-use.exe` 重包裹为「CUA 外层 → `--previous-notify` → `agentnotify-codex-hook`」；而经过验证的形态是「**Hook 直连外层**，CUA 作为 `--previous-notify` 载荷由 Hook 透传」（见 `tools\hooks\install-codex-v2.ps1` 与 `03-upgrade-test-report.md`）。外层 CUA 的转发不可靠：08:45/08:52/08:54 只转了 3 条即停。
- **修复（发布后）**：重跑 `tools\hooks\install-codex-v2.ps1` 恢复 Hook 直连；增强「检测到 CUA 外层包裹即纠正/提示」；`docs\TROUBLESHOOTING.md` 补一条。
- **验收**：重启 Codex 跑一轮，`notifications` 表新增 codex 行。

## B2 失败推送文案去重（低风险文案）
- 前缀「第 N 步失败：」与 `blockReason` 开头「第 N 步执行失败：」语义重复；`blockReason` 句尾「。」与模板后缀相连出现「。。」。统一为一处原因即可（详见 `06-m2-realmachine.md` §推送与 `07` §5-4）。

## B3 渠道账号硬删除（能力缺口）
- 渠道页「退出账号」= 登出并置 `enabled=false`（明示「历史保留」），**无硬删除**；停用账号行只能保留。删除停用账号 `clawbot-0a1900c1ff2d12f9` 未能通过桌面端完成（UI 无该能力）。

## B4 无人值守长回合易被中断（观测，非 2.1.0 回归）
- `发版自检-4步` 第 5 轮第 1/3 步均出现「约 60 分钟长回合被模型中断 → 无产出」→ 宿主按「防静默跳过」判失败并给一行中文+建议（符合 §3.2）。发布后可视情况研究更长回合/中断重试策略。

## 相关 backlog（ClawBot 推送）
- 失败推送排队补投、推送配额节流/合并、保活提示产品化 —— 见 `07-clawbot-push-session-research.md` §5。
