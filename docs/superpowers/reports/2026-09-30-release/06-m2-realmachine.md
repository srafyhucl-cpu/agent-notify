# M2 真机矩阵证据（§3.4，CDP 驱动真实桌面端）

- 日期：2026-10-01（承接 04-m2-status.md：T-J 之外的部分已改为**真机自动执行**）
- 方法：以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动已安装的 **2.1.0** 桌面端，用 Playwright `connectOverCDP('http://127.0.0.1:9222')` 驱动**真实 Tauri 界面**（非 mock），任务真实派活到本机 OpenCode，工作目录 `D:\Temp\release-selfcheck`。
- 模型：全部节点 `opencode-go/deepseek-v4.1-flash`（鹈鹕同款；节点 Agent 一律 OpenCode）；节奏 `final_only`（除注明）。
- 数据/日志取证：`%LOCALAPPDATA%\AgentNotify\data\state.db` + `logs\runtime.log`；截图见 `D:\Temp\agentnotify-temp\m2\*.png`。

## 结果矩阵
| 编号 | 任务 / 操作 | 验收要点 | 结果与证据 |
|---|---|---|---|
| T-A | `发版自检-闭环`（标准交付 3 步，红气球；id `f0c8cade…`） | 角色信封 · 逐步流转 · 汇总 · 推送 | ✅ **闭环通过**：step1→2→3→汇总→完成（多轮迭代至**第 5 轮上限**后 `TASK_STATE_COMPLETED`；日志「项目经理判定继续迭代但已达轮次上限，按完成处理 round=5」）；roundHistory 5 轮含各轮结论；交付物 `D:\Temp\release-selfcheck\index.html`。日志见 observer「Agent 汇报已回注，任务自动推进」。推送见下 §推送 |
| T-B | 同 T-A（自动循环） | 汇总含【继续迭代】→ 第 2 轮回 step1 | ✅ T-A 自动迭代第 1→2→…→5 轮（`round_continue_input` 命中）；T-F 亦自动进入多轮。信封带【第 N 轮迭代】 |
| T-C | `发版自检-闭环` 点「继续迭代（第 4 轮）」 | 弹窗填本轮要求 → 轮次+1、回 step1、透传 | ✅ 弹窗「本轮要求（可留空）」输入「加一条高光」→ 校验：`round=4`、`roundInput=加一条高光`、`currentStep=1`、state WORKING，roundHistory 第 4 轮 input 落库 |
| T-D | T-E 与 T-F 并发运行 | 列表/详情互不串台 | ✅ 两任务并发推进、各自 step/round 独立；看门狗/观察者按各自 task_id 回注，无串台（DB 快照逐任务隔离） |
| T-E | `发版自检-2步`（快速修复 2 步，蓝圆） | 2 步模板 | ✅ 2 步（实施→复核）跑完 `TASK_STATE_COMPLETED`，snapshot 仅 2 个 order，无规划步 |
| T-F | `发版自检-4步`（完整评估 4 步，方案对比；id `9dfd7f80…`） | 4 步模板 | ✅ 4 个节点顺序执行（snapshot order 1–4），跑到汇总后进入多轮迭代（第 1→4 轮，其中第 1 步曾有看门狗回注兜底 `recovered=1`）。**观测注记（in-design 失败路径，非回归）**：无人值守续跑第 5 轮时，第 1 步回合于本机 05:12:46 被判「已结束但无产出」（OpenCode `idle outcome=interrupted`，约 60 分钟无产出/无最终汇报）→ 宿主按「防静默跳过」判失败：`TASK_STATE_FAILED`、blocked=1、原因「第 1 步执行失败：本回合已结束，但没有产出汇报（模型可能提前中断）：请点「重新发起」让它继续，或打开会话检查产出。」——与 §3.2「模型/会话说不上话 → 明确阻塞+建议」一致 |
| T-G | `发版自检-失败`（第 2 步 `bogus-provider/does-not-exist`） | 阻塞一句中文+建议 → 改模型 →「重新发起」 | ✅ 阻塞：`TASK_STATE_FAILED`、blocked=2、原因「第 2 步执行失败：所选模型不可用：请给该节点换一个模型后点「重新发起」」；恢复：节点卡改第 2 步模型为 deepseek-v4.1-flash → 点「重新发起」→ `TASK_STATE_WORKING`、blocked=None（列表回到「执行中」） |
| T-H | 复用既有/本批任务 | 看门狗兜底 | ✅ 日志存在「看门狗回注漏掉的 Agent 汇报，任务自动推进 recovered=1」（用户任务 `0711c95d…`）与观察者回注两条路径；本批任务由观察者回注逐步推进，无来源缺失 |
| T-I | T-F 运行中改第 2/4 步模型+强度 | 快照落库；下一步 job 带 variant | ✅ 快照落库：第 2 步 `mimo-v2.6-flash`、第 4 步 `space-bunny-free` + `variant=low`（强度候选 low/medium/high/xhigh/max 由模型 variants 提供）；payload 的 `variant` 字段由 `resume.rs` 单测（T1）锁定 wire 契约 |
| T-K | 终态 / 阻塞态 | 终态只读；阻塞可改模型 | ✅ 终态（Completed）：详情无 `select`、仅「继续迭代」按钮；阻塞（Failed）：6 个模型/强度内联下拉可编辑 + 「重新发起」 |
| T-L | 用户既有任务（`鹈鹕骑行测试` 等） | 旧数据兼容 | ✅ 5 轮旧任务的轮次时间线（含无 input 的第 4 轮「由项目经理按上一轮结论继续」）正常渲染，无 `roundHistory`/字段缺失不炸 |
| T-J | 用户发 `【集群 发版自检-4步】恢复` | 入站 → 按任务名寻址 → 回执 | ✅ **入站链路通过**：09:11:35 用户消息到达（`inbound_claims` rowid=173）；09:12:35 宿主日志 `识别到微信集群指令 address=发版自检-4步` → 执行 `recover_blocked` 返回 `orc.task_not_blocked`（该任务未阻塞，属预期）→ 回执 `集群指令未生效：任务 <task_id> 未处于阻塞状态，无法恢复` 已发出（无「集群指令回执发送失败」日志）。**同名消歧 / 旧 ID 兼容**由 `orc_wechat_route` 单测覆盖；本机天然存在两个同名 `写一个贪吃蛇小游`（可作消歧用例，待用户发一条或按单测口径） |

## 推送（T-A 验收 ④）
- 集群消息**已尝试外发**（`final_only`，完成时推送）；日志：`集群消息推送失败，不影响任务状态 code="orc_cluster_reply_unconfirmed"`。
- 根因：ClawBot 渠道当前处于**「推送已断」**/未确认状态（`channel_accounts` 的 clawbot 账号存在 `session_alert_at`，用户既有任务同样出现该 code），属**渠道环境**而非 2.1.0 回归；推送**内容**（头 `【集群 <任务名>】`）由 `orc_notify` 单测锁定。
- **更新（2026-10-01 08:27 / 08:52）**：08:27 会话恢复一次（`notifystart 已发送，主动推送会话就绪`），但 08:52:24 又出现 `session_missing`（ClawBot 平台 `PrepareFailed`、清空本地会话），08:52:27 再弹「微信推送已断」提醒（`alerted=1`）——**老现象、非 2.1.0 回归**；恢复方式为「给 ClawBot 发任意消息」。用户 09:11 的消息已刷新会话上下文。
- 「推送头=任务名且 `Sent`」的**投递确认**仍未取得：`发版自检-4步`（其余任务已终态）现于第 5 轮第 3 步运行中，待其完成后 `final_only` 推送的 `Sent`/`交付` 结果复核；若仍 `session_missing`，记「需重连/重启渠道后再验」。

## 未覆盖 / 用户跳过
- **DPI 125%/150%/200%、760px 窄窗**：属 WebView2 窗口级属性，CDP 无法设置 OS DPI；**用户明确不测（2026-10-01，跳过，已从待办移除）**。
- 人工确认门（§3.4-3）：需临时 `human_gate` 模板，未做（按「仅单测覆盖」记录）。
- ClawBot 健康四态（§3.1-12）首断弹窗只一次/恢复再武装：日志可见「已弹出『微信推送已断』提醒 alerted=1」；会话已于 08:27 恢复（`notifystart 已发送，主动推送会话就绪`）。

## 状态与清理
- 真实机器仍保留 `发版自检-闭环/2步/4步/失败` 四个任务（供用户完成 T-J 与最终验收；按用户「M7 只做非破坏性清理」暂不删除）。当前：`闭环` 完成（第 5 轮上限、终态）、`2步` 完成、`失败` 完成（恢复后跑完）、`4步` 用户重新发起后恢复、现于第 5 轮第 3 步运行中（T-J 的 `恢复` 指令即作用于它，因未阻塞而返回 `orc.task_not_blocked`）。
- CDP 驱动脚本已移出仓库（`D:\Temp\agentnotify-temp\m2\scripts\`），仓库工作区干净。
- **调试端口已关闭**：已按计划 §3.4-4 干净重启应用（不带 `--remote-debugging-port`；`http://127.0.0.1:9222` 已拒绝连接），当前 2.1.0 正常驻留。
