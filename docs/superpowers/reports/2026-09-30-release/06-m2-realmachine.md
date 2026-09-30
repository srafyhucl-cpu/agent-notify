# M2 真机矩阵证据（§3.4，CDP 驱动真实桌面端）

- 日期：2026-10-01（承接 04-m2-status.md：T-J 之外的部分已改为**真机自动执行**）
- 方法：以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动已安装的 **2.1.0** 桌面端，用 Playwright `connectOverCDP('http://127.0.0.1:9222')` 驱动**真实 Tauri 界面**（非 mock），任务真实派活到本机 OpenCode，工作目录 `D:\Temp\release-selfcheck`。
- 模型：全部节点 `opencode-go/deepseek-v4.1-flash`（鹈鹕同款；节点 Agent 一律 OpenCode）；节奏 `final_only`（除注明）。
- 数据/日志取证：`%LOCALAPPDATA%\AgentNotify\data\state.db` + `logs\runtime.log`；截图见 `D:\Temp\agentnotify-temp\m2\*.png`。

## 结果矩阵
| 编号 | 任务 / 操作 | 验收要点 | 结果与证据 |
|---|---|---|---|
| T-A | `发版自检-闭环`（标准交付 3 步，红气球；id `f0c8cade…`） | 角色信封 · 逐步流转 · 汇总 · 推送 | ✅ **闭环通过**：step1→2→3→汇总；最终 `TASK_STATE_COMPLETED`，roundHistory 3 轮含各轮结论；交付物 `D:\Temp\release-selfcheck\index.html`（3846 B）。日志见 observer「Agent 汇报已回注，任务自动推进」。推送见下 §推送 |
| T-B | 同 T-A（自动循环） | 汇总含【继续迭代】→ 第 2 轮回 step1 | ✅ T-A 与 T-F 均被项目经理判定「继续迭代」而**自动进入下一轮**（T-A 自动走到第 3 轮、T-F 第 4 轮仍在迭代），信封带轮次 |
| T-C | `发版自检-闭环` 点「继续迭代（第 4 轮）」 | 弹窗填本轮要求 → 轮次+1、回 step1、透传 | ✅ 弹窗「本轮要求（可留空）」输入「加一条高光」→ 校验：`round=4`、`roundInput=加一条高光`、`currentStep=1`、state WORKING，roundHistory 第 4 轮 input 落库 |
| T-D | T-E 与 T-F 并发运行 | 列表/详情互不串台 | ✅ 两任务并发推进、各自 step/round 独立；看门狗/观察者按各自 task_id 回注，无串台（DB 快照逐任务隔离） |
| T-E | `发版自检-2步`（快速修复 2 步，蓝圆） | 2 步模板 | ✅ 2 步（实施→复核）跑完 `TASK_STATE_COMPLETED`，snapshot 仅 2 个 order，无规划步 |
| T-F | `发版自检-4步`（完整评估 4 步，方案对比） | 4 步模板 | ✅ 4 个节点顺序执行（snapshot order 1–4），跑到汇总后进入多轮迭代 |
| T-G | `发版自检-失败`（第 2 步 `bogus-provider/does-not-exist`） | 阻塞一句中文+建议 → 改模型 →「重新发起」 | ✅ 阻塞：`TASK_STATE_FAILED`、blocked=2、原因「第 2 步执行失败：所选模型不可用：请给该节点换一个模型后点「重新发起」」；恢复：节点卡改第 2 步模型为 deepseek-v4.1-flash → 点「重新发起」→ `TASK_STATE_WORKING`、blocked=None（列表回到「执行中」） |
| T-H | 复用既有/本批任务 | 看门狗兜底 | ✅ 日志存在「看门狗回注漏掉的 Agent 汇报，任务自动推进 recovered=1」（用户任务 `0711c95d…`）与观察者回注两条路径；本批任务由观察者回注逐步推进，无来源缺失 |
| T-I | T-F 运行中改第 2/4 步模型+强度 | 快照落库；下一步 job 带 variant | ✅ 快照落库：第 2 步 `mimo-v2.6-flash`、第 4 步 `space-bunny-free` + `variant=low`（强度候选 low/medium/high/xhigh/max 由模型 variants 提供）；payload 的 `variant` 字段由 `resume.rs` 单测（T1）锁定 wire 契约 |
| T-K | 终态 / 阻塞态 | 终态只读；阻塞可改模型 | ✅ 终态（Completed）：详情无 `select`、仅「继续迭代」按钮；阻塞（Failed）：6 个模型/强度内联下拉可编辑 + 「重新发起」 |
| T-L | 用户既有任务（`鹈鹕骑行测试` 等） | 旧数据兼容 | ✅ 5 轮旧任务的轮次时间线（含无 input 的第 4 轮「由项目经理按上一轮结论继续」）正常渲染，无 `roundHistory`/字段缺失不炸 |
| T-J | — | 微信入站指令 | ⛔ **留待用户**（需用户在微信发 `【集群 发版自检-闭环】确认` 等） |

## 推送（T-A 验收 ④）
- 集群消息**已尝试外发**（`final_only`，完成时推送）；日志：`集群消息推送失败，不影响任务状态 code="orc_cluster_reply_unconfirmed"`。
- 根因：ClawBot 渠道当前处于**「推送已断」**/未确认状态（`channel_accounts` 的 clawbot 账号存在 `session_alert_at`，用户既有任务同样出现该 code），属**渠道环境**而非 2.1.0 回归；推送**内容**（头 `【集群 <任务名>】`）由 `orc_notify` 单测锁定。
- 因此「推送头=任务名且 `Sent`」的**投递确认**留待用户恢复微信推送会话后复核（与 T-J 一并）。

## 未覆盖（窗口级/需用户，非本次可自动化）
- DPI 125%/150%/200% 与 760px 窄窗不换行/不溢出/无双滚动条——属 WebView2 窗口级属性，CDP 无法设置 OS DPI。
- 人工确认门（§3.4-3）：需临时 `human_gate` 模板，未做（如后续不做，按「仅单测覆盖」记录）。
- ClawBot 健康四态（§3.1-12）首断弹窗只一次/恢复再武装：日志可见「已弹出『微信推送已断』提醒 alerted=1」，完整四态留待用户。

## 状态与清理
- 真实机器仍保留 `发版自检-闭环/2步/4步/失败` 四个任务（供用户完成 T-J 与最终验收；按用户「M7 只做非破坏性清理」暂不删除）。
- CDP 驱动脚本已移出仓库（`D:\Temp\agentnotify-temp\m2\scripts\`），仓库工作区干净。
- 调试端口 `9222` 由启动参数开启；如需彻底关闭按计划 §3.4-4 干净重启（待 T-J 完成后执行）。
