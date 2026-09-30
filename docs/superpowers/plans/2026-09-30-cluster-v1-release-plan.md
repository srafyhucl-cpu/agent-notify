# 集群（编排）v1 发版待执行计划（2026-09-30）

> 分支 `feat/cluster-ui-flow`（21 提交，PR #72）→ 目标版本 **2.1.0**。
> 原则：本地完成「完整测试复跑 + 2.0.10 → 2.1.0 升级测试」后才下最终发版结论。

## 一、已完成（本轮迭代梳理）

| 主题 | 提交 |
| --- | --- |
| 集群页 v1（动线重做/新建弹窗/节点锁定/增删改查/进度/文案/详情分块） | b2c06fe 683cfa1 d2b3acd b7edaa5 4cccdec ab42362 4345a3d ff036e5 |
| 角色化信封提示词 + 任务迭代循环（继续迭代/自动新一轮，2/3/4 步通用） | 124b1dc |
| v4 前端四项（轮次时间线/任务信息一行/操作提示/创建时间） | 13cbfe1 |
| 二轮反馈（时间线交互/信息一行/操作随节点）+ 插件终态健壮性 | dde7d12 |
| 宿主看门狗（兜底插件事件丢失；只回注已收束回合） | 5207243 1ceb1b8 |
| 集群推送头改任务名 + 微信指令按名寻址（兼容 ID） | 45de874 |
| ClawBot 推送会话失效可见 + 首次断推系统通知 | 000dcc0 fe0bb44 |
| 第 4 项：variant 透传链 / 任务结束前改模型与强度 / 每步主题色与光束 / 内联编辑与等高卡片 / h3 修正 | 8d98a1d 8ed4d91 f358d35 219b1b9 |
| 集群节点收紧为仅 OpenCode（其它 Agent 会话能力未实现） | 83bd320 |

差异规模：78 文件、+9947/-1353。PR：#72（base main）。

## 二、发版前必须完成（本地）

1. **完整测试复跑（最新 HEAD）**
   - `tools\rust\gate.ps1`（fmt/clippy/全部测试）——最新一次在 83bd320 后为绿，发版前再跑一次；
   - `tools\rust\export-bindings.ps1`（绑定必须无 stale）；
   - `tools\test.ps1` 与 `tools\lint.ps1`（Go 单测 + 插件类型/状态机 + 冒烟 + 脚本语法/版本一致性）；
   - UI：`tsc --noEmit`、`npx vitest run`、`npm run build`、`npm run test:a11y`、`npm run test:visual`。
2. **本地升级测试（关键）**
   - 用 CI 的 Release dry-run 产出**签名**产物（Actions → Release → `mode=validate` 先跑，再 `mode=build`）；本地无法签名；
   - 在本机执行 **正式 2.0.10 → 2.1.0** 覆盖安装，逐项验证：
     - 数据：`state.db`（任务/轮次历史/通知与投递/回复路由）无损，迁移状态正常；
     - 集成更新：OpenCode 插件、Codex 钩子（notify 链）、Devin/Antigravity/CommandCode 集成被正确更新或保持；
     - 功能冒烟：集群创建→执行→继续迭代→看门狗；微信推送（含推送头任务名）；渠道健康与「推送已断」；更新器自检；
     - 回退：卸载/重装 2.0.10 可恢复。
3. **版本流程**（升级测试通过后）
   - 合并 PR #72 到 main；`VERSION` → `tools\sync-version.ps1` → README 徽章 + `CHANGELOG.md`（2.1.0 段落）；
   - tag `v2.1.0` 推送 → Release workflow 验证/构建/发布（客户端更新源为二进制镜像仓）。
4. **结论**：以上 1–3 全部通过后，方可下「可以发版」的最终结论（当前为「具备条件、待验证」）。

## 三、待执行（非发版阻塞）

1. **非 OpenCode 的集群会话能力调研（进行中，结论待写入本节）**
   - 范围：Codex / Antigravity / CommandCode / Devin；
   - 每家评估：能否「新建会话并按会话 ID 续聊」；不能则用「逻辑会话 ↔ 真实会话映射」折中（与 OpenCode 插件 session-map 同构）；
   - 已确认（Codex）：适配器目前只有 `resume(thread_id)`；集群会话 ID 在 Codex 中不存在 → 现状必然失败；汇报事件带 `thread-id` 可用 → 折中方案可行。
   - 待办：Antigravity / CommandCode / Devin 得出结论后，把四家的**结论与实现顺序**统一补进本节，再开工。
2. **渠道页清理**：删除两个无凭据的 ClawBot 账号（用户确认后执行）。
3. 观感遗留（低优先）：详情区标题层级已修（h3）；如后续再做自定义下拉浮层，替代系统原生列表。
