# 集群 v1 发版计划（最终版，2026-09-30）

> 分支 `feat/cluster-ui-flow`（22 提交，PR #72）→ 目标版本 **2.1.0**。
> 规范：**极客精神 + 大厂规范**（门禁前置、常量命名、职责单一、文档讲关键点、发布可回退）。
> 纪律：**同一时间只跑一个门禁**（防并发抢 target 假失败）；每步留证据；**任何必修项或门禁失败 → 停止推进，先修**。
> 完成定义：本计划 §9 归档齐全 + **用户侧验收确认**（含 §3.4 的微信入站指令配合）才算完成。

## 0. 范围声明：测试与审计覆盖**整个仓库**（不限本次 diff）
| 区域 | 范围 |
| --- | --- |
| Rust 工作区 | `crates/*`（domain / application / agent-sdk / 各 agent 适配器 / orchestration / runtime / storage-sqlite / channel-* / testkit） |
| Rust 宿主 | `hosts/desktop-tauri`（bridge / production / update / platform / lifecycle）+ `apps/hooks` + `apps/ingress` |
| Go 旧版 | `cmd/`、`internal/`（非发版入口，仍整库测试与审计） |
| 前端 | `apps/desktop-ui`（features / components / data / bridge / styles / tests / 视觉基线） |
| 脚本 | `tools/*.ps1`（build-release / publish-release / lint / test / sync-version / check-version / ui/* / rust/* / hooks/*） |
| 插件与扩展 | `plugin/*`（rust/opencode、commandcode-v2、commandcode-mod、devin-extension、devin-extension-v2） |
| 契约与发布 | bridge 绑定 `types.ts`、迁移脚本、`RELEASE-MANIFEST` 与签名链路、`.github/workflows/*` |
| 文档 | `docs/**`、`README`、`CHANGELOG`、`AGENTS.md` 一致性 |

存量问题**登记清单不阻塞本次发版**；**新增问题零容忍**。

## 1. 发版目标与前置检查（做不到就停）
1. **上一版基线**：确认 `v2.0.10` 的 tag / Release / **安装包可下载**（升级与回退都要用；tag 列表当前只见 v2.0.9，先补齐或确认命名规则）。
2. **CI 权限与 Secret**：`RELEASE_REPO_TOKEN`、签名 PFX 与密码在 Actions 可用（缺失时 dry-run 明确失败，不绕过）。
3. **磁盘空间**：D 盘 ≥ 20 GB 空闲（必要时清理 `agentnotify-rust-target` 非 deps 缓存）。
4. **时间预算**：M1≈30min；M2≈2–3h（含三档真跑、并发、两轮迭代与你的微信配合窗口）；M3≈1–2h；M4≈30min；M5≈1.5h（含 20min 构建）；M6≈1h；M7≈30min。
5. **环境**：Rust（`CARGO_HOME=D:\Tools\cargo`、`RUSTUP_HOME=D:\Tools\rustup`、`CARGO_TARGET_DIR=D:\Temp\agentnotify-rust-target`、`TEMP=TMP=D:\Temp\agentnotify-temp`、`. tools\rust\xwin-env.ps1`）；前端（`PLAYWRIGHT_BROWSERS_PATH=D:\Tools\playwright-browsers`）。
6. **中止条件**：任一必修项未过、任一门禁失败、升级测试不通过 → 停止并回到对应步骤，不带伤推进。

## 2. 发版前必修（代码评审结论；全部落地并自测通过后才进 §3）
| 编号 | 内容 | 位置 | 验收 |
| --- | --- | --- | --- |
| B1 | 看门狗回注后的二次写入存在覆盖窗口：把 `mark_settled_turn` 并入 `report_from_agent` 同一次写入，或给任务写入加 per-task 互斥/乐观版本号 | `orc_watchdog.rs:515-526`、`orc_handler.rs` | 新增并发用例：并发写不丢更新 |
| B2 | `update_task_step` 的 variant 无白名单/长度上限：按模型 `variants` 校验 + model/variant 长度上限 | `orc_handler.rs` | 非法 variant → `orc_step_variant_invalid`；超长被拒 |
| B3 | 任务名含 `】` 会截断微信寻址：创建/编辑名禁止 `】【` 等分隔符，或解析失败给针对性提示 | `wechat_command.rs`、任务名校验 | 新增解析用例：明确报错不误伤 |
| B4 | 展开详情即拉模型列表（OpenCode 未运行时噪音大）：改懒加载，错误只在下拉处呈现 | `TaskDetail.tsx` | 未开下拉不请求；失败有就地提示 |
| B5 | 内联下拉"选择即保存"无防抖：加 200–300ms 防抖；保存中禁用控件 | `OrcNodeChain.tsx` | 连续切换只发一次有效请求 |
| T1 | 派活 job 的 `variant` 缺断言：`Some("high")` → wire JSON 含 `variant`；`None` → 字段不出现 | `crates/agentnotify-agent-opencode/tests/resume.rs` | 新用例通过 |

## 3. 整库测试矩阵
### 3.1 功能场景（真机/编排，全部要跑到）
1. 三档内置模板**各真跑一次**：快速修复(2 步) / 标准交付(3 步) / 完整评估(4 步)；
2. 节点配置：Agent 下拉**只有 OpenCode**；模型下拉（OpenCode 全量模型）；思考强度（`default` + 该模型 `variants`，原样英文 id）；未选齐禁用提交；保存可回读；
3. 角色信封：orchestrator / planner / executor / reviewer 四种各自生效（executor 含"有前序方案就照方案做，没有就按目标直接做"）；
4. 执行闭环：派活 → 逐步自动流转 → 汇总中 → 项目经理最终汇报；
5. 迭代循环：**自动**（汇总含【结论：继续迭代】→ 第 2 轮回 step1）与**手动**（继续迭代弹窗，含本轮要求/留空）各一次；上限 5 轮；
6. **并发**：两个任务并行运行 + 一个"仅创建"任务静态展示；列表/详情互不串台、看门狗不误伤；
7. 阻塞与恢复：失败阻塞（一句中文+建议，列表不堆报错）→「重新发起」；阻塞中禁止推进；
8. 人工介入：发指令 / 确认完成（human_gate）/ 汇报；
9. 看门狗：漏报时（会话 idle 收束）自动回注；**进行中回合不得回注**；旧任务基线不猜历史回合；
10. 通知节奏：只推最终汇报 / 逐步流转；失败提醒不受节奏限制；
11. 微信指令（**需用户配合发一条**）：按任务名寻址（确认/指令/恢复）+ 旧任务 ID 兼容 + 同名消歧 + 未知任务报错；
12. ClawBot 健康四态：正常 / 推送会话失效（stale +「推送已断」）/ 登录失效（重新扫码）/ 刚登录等首条消息（不报警）；**首次断推系统通知只弹一次、恢复后重新武装**；
13. 列表与详情 UI：列表仅名称+状态+进度段+轮次徽标+创建时间；详情四分块；轮次时间线展开/收起；节点卡两行等高、每步主题色、光束、模型·强度内联下拉；任务增删改查。

### 3.2 异常与边界
- 会话说不上话 / 模型不可用 / 余额不足 → 明确阻塞与建议；
- 汇总阶段禁止人工推进；终态只读；阻塞态可改模型但无操作按钮；
- 旧数据兼容：无 `roundHistory` / `createdAt` / `variant` / 步骤快照；
- **旧版 Go 遗留数据**（`reply-routes.jsonl` / `reply-state.jsonl`、迁移状态 Partial）升级后仍被正确读取或忽略；
- 任务名含 `】`、超 8 字被拒；长文本/长路径/760px 窄窗/**125%、150%、200% DPI 缩放**不换行不溢出、无双滚动条、主操作不被遮挡；
- OpenCode 未运行：模型接口报错 +「重新读取」；看门狗跳过不误报；
- ClawBot 长轮询失败/断网：重试且不阻塞任务。

### 3.3 测试层级与命令（**整库范围**）
| 步骤 | 命令 | 通过标准 |
|---|---|---|
| 1 Go 静态 | `gofmt -l cmd internal`；`go vet ./...` | 无输出 |
| 2 Rust 静态 | `cargo fmt --all --check`；`cargo clippy --workspace --all-targets -- -D warnings` | 零 diff / 零警告 |
| 3 Rust 全门禁 | `powershell -File tools\rust\gate.ps1` | 尾行 `[gate] cleaned N test temp directories` |
| 4 绑定契约 | `tools\rust\export-bindings.ps1` → `git status --short` | 无 stale 绑定 |
| 5 Go 单测 | `go test ./...` | 全通过 |
| 6 插件 | `node --test plugin\rust\agent-notify.test.cjs` | 30 通过 |
| 7 脚本/冒烟 | `tools\test.ps1`；`tools\lint.ps1` | `SMOKE ALL GREEN`；语法+版本一致性通过 |
| 8 前端 | `tsc --noEmit`；`npx vitest run`；`npm run build`；`npm run test:a11y`；`npm run test:visual` | 0 错误 / 全通过 / 18 / 16 |
| 9 覆盖率基线（可选） | `cargo llvm-cov --workspace --summary-only`；`npx vitest run --coverage` | 产出存档；**工具不可用则记录并跳过** |
| 10 仓库 secret 扫描 | 关键字扫描（`BEGIN PRIVATE KEY` / `password=` / `gho_` / PFX） | 仓库零命中 |

### 3.4 编排真机自检（用鹈鹕同款模型配置）
1. 准备：`D:\Temp\release-selfcheck` + 调试端口启动预览（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`）。
2. 三档真跑：
   - `发版自检-2`（快速修复 2 步）：目标「写单文件 HTML，纯 CSS 画居中蓝圆」；
   - `发版自检-3`（标准交付 3 步）：目标「写单文件 HTML，纯 CSS 画居中红气球」；节点 = `opencode-go/deepseek-v4.1-flash` / `opencode-go/space-bunny-free` + **强度 high** / `opencode-go/mimo-v2.6-flash`；
   - `发版自检-4`（完整评估 4 步）：目标「给出两个实现方案并选一，说明理由」（覆盖 orchestrator 判定路径）；
   - 一个任务走**自动循环**（判定继续迭代）；另一个用**手动继续迭代**开第 2 轮；三任务中至少两个并行。
3. 逐项取证：① 下拉只有 OpenCode；② 会话与标题 `【集群】<名> · 第 N 步`；③ 四种角色信封；④ 每步推进必有来源——**判定口径：每一步在日志中必有「汇报已回注」或「看门狗回注」其一，且不存在来源缺失的步**；⑤ 汇总 → 最终汇报；⑥ 自动/手动两轮；⑦ 推送头=任务名且投递 `Sent`；⑧ 详情 UI 全项（两行等高/主题色/光束/强度英文 id/内联下拉落库）；⑨ 轮次时间线 + stepReports + round/roundInput；⑩ 边界项（§3.2 相关）。
4. **微信入站指令（blocked-on-user）**：你回一条 `【集群 发版自检-3】确认` 或 `【集群 发版自检-3】指令 加高光`，验证推进/回执/消歧/兼容。
5. **人工确认门**：临时带 `human_gate` 的模板跑一次；若不做，必须在报告里明确"仅单测覆盖"。
6. 收尾：删除三个自检任务、清 `D:\Temp\release-selfcheck` 与脚本、关调试端口并干净启动。

## 4. 整库质量审计
1. 基线排行：文件行数 Top30、函数行数 Top30、impl 方法数 Top20、`TODO/FIXME`、生产代码 `unwrap()/unsafe`。
2. 魔法数字扫描（Rust/TS/CSS 三份 + 人工复核；**本分支新增零裸值**）。
3. 依赖与安全：`cargo audit`、`govulncheck ./...`、`npm audit --omit=dev`；仓库密钥扫描。
4. 抽查：静默吞异常、分层越界、死代码/未用导出、日志脱敏、旧数据兼容。
5. **《整库质量报告》必须包含**：①范围与方法；②四张排行表；③裸字面量清单与新增核对结论；④依赖/安全审计结果；⑤待治理清单（风险排序 + 建议批次）；⑥覆盖率基线（若跑）；⑦结论「存量不阻塞发版 / 新增零违规」。
6. 门槛：本分支新增超标（文件>800 行 / 函数>80 行 / 单 impl 公有方法>20）或裸魔法数字 → 先修再进 §5。

## 5. 合并、版本与升级测试
1. §2 必修全过 + §3/§4 证据齐 → 合并 PR #72 到 main。
2. **版本与文档**：`VERSION` → `2.1.0`；`tools\sync-version.ps1`；README 徽章 + `CHANGELOG.md` 2.1.0 段；`tools\check-version.ps1`；**设计文档 §12 各条进度标记与实际行为逐条核对**。提交 `chore(release): 2.1.0`。
3. **RC 优先协议（防"合并后才发现升级不过"）**：dry-run 与升级测试在**版本提交之后的 main（或临时 RC tag）**上进行；若升级测试失败 → **revert 版本提交或降级为 patch 版本**，修复后重走。
4. **dry-run**：`gh workflow run release.yml -f mode=validate`（≈1min）→ 通过后 `-f mode=build`（≈20min，签名、不发布）；失败即停并记录；下载产物（Setup + 五个 ZIP + `RELEASE-MANIFEST.json`/`p7s`）。
5. **产物级 secret 扫描**：解包 **ZIP 与 Setup.exe** 扫描 PFX/密码/token，零命中。
6. **本地升级测试**（备份：`state.db` + `~/.config/agent-notify`；基线：任务/轮次/通知/投递计数、插件与钩子哈希、Codex notify 链、`VERSION`）：
   - **数据**：计数与关键字段一致、迁移正常、旧版 Go 遗留文件处理正确；
   - **集成归属**：OpenCode 插件、`agentnotify-ingress.exe`、各钩子**必须指向正式安装路径**（预览残留=false）；
   - **WebView2 兼容**：记录运行时版本；确认主题色/柔光在 `color-mix` 不可用时退化为静态色或明确最低版本要求（README 标注）；
   - **功能冒烟**：集群跑一轮 + 继续迭代、推送头任务名、渠道健康/推送已断、更新器自检；
   - **签名/清单**：更新器接受，且**负例**——篡改清单或任一文件后必须拒绝；
   - **卸载残留**：文件/服务/计划任务/快捷方式无残留。
7. **回退与再前进**：卸载 2.1.0 → 重装 **2.0.10**（数据/功能可用）→ 再装 2.1.0（迁移幂等，无重复/无丢失）。
8. 产出《升级测试报告》：前后对比、逐项通过/失败、负例结果、回退与再前进结论。

## 6. 正式发布与发布后
1. §5 全绿 → 打 tag `v2.1.0` 推送 → Release workflow（验证 → 签名构建 → 发布：二进制镜像仓 + 源码 Release）。
2. 发布后验证：客户端"检查更新 → 下载 → 安装 → 校验"全链路；Beta/Stable 通道行为；已发布 Release 不被自动覆盖。
3. **坏包预案**：已发布包不可覆盖 → 补丁版（2.1.1）+ 审计输出；发布后 24h 观察推送/更新告警。

## 7. 工作区清理（发版后）
1. 代码：删 `feat/cluster-ui-flow`（本地+远端）、关 PR #72、清 worktree；`.gitignore`/未跟踪文件核对；`git status` 干净。
2. 产物/缓存：`D:\Temp\agentnotify-temp`、`agentnotify-release-verify`、`%LOCALAPPDATA%\AgentNotify\temp\updates`（缓存+备份）清空；保留活跃 `agentnotify-rust-target`；`dist\rust-preview` 旧包按需删除。
3. 环境：卸载删除 `D:\Temp\agentnotify-preview-install`；删桌面「Agent-notify 预览版」快捷方式；Codex notify 链与 OpenCode 插件指回正式安装。
4. 数据：删测试任务（发版自检 ×3 / 探针）；`D:\Temp\release-selfcheck`、`D:\Temp\pelican-bicycle-svg` 按用户意愿。
5. 收尾核对：git 干净、无 `_tmp-*`、无调试端口、无多余 agentnotify 进程、临时脚本清零。

## 8. 风险与回退预案
| 风险 | 预案 |
|---|---|
| 门禁并发抖动（假失败） | 同一时刻只跑一个；失败先单独重跑确认 |
| 模型不可用（余额/卡顿） | 记录真实原因；必要时临时换模型并标注；看门狗兜底 |
| CI Secret/权限缺失 | 明确失败不绕过；补齐后重跑 |
| 磁盘不足 | 清缓存/归档旧产物后再构建 |
| 升级测试失败 | 保留 2.0.10 安装包与备份；按 §5-3 协议 revert/降级；定位后重走 |
| 已发布包有问题 | 补丁版 + 审计输出，不覆盖已发布 Release |
| Codex 钩子被其更新覆盖 | 升级后核对 notify 链，按 `tools\hooks\install-codex-v2.ps1` 重装并记录 |
| WebView2 过旧导致主题色失效 | `@supports` 回退或 README 标注最低版本 |
| 存量质量问题 | 只登记清单，不在本次发版内修 |

## 9. 里程碑、归档与完成定义
| 里程碑 | 内容 | 阻塞条件 | 证据产物 |
|---|---|---|---|
| M1 | §2 必修 + §3.3 整库测试复跑 | — | 门禁输出摘要、覆盖率基线（可选） |
| M2 | §3.1/3.2 场景 + §3.4 编排自检 | **blocked-on-user**（微信入站指令） | 截图 + 日志 + DB 查询 |
| M3 | §4 整库质量审计 | — | 《整库质量报告》 |
| M4 | §5-1/2 合并与版本 | 依赖 M1–M3 | 版本提交、check-version 输出 |
| M5 | §5-3..8 dry-run + 升级测试 | 依赖 CI Secret；下载产物 | 《升级测试报告》 |
| M6 | §6 正式发布 | 依赖 M5 全绿 | Release 链接、发布后验证 |
| M7 | §7 工作区清理 | 依赖 M6 | 清理核对清单 |

- **归档**：`docs/superpowers/reports/2026-09-30-release/`（门禁摘要、编排自检证据、质量报告、升级测试报告、发布后验证）+ 仓库外原始输出备份；每个结论可追溯到"命令 + 输出 + 时间"。
- **完成定义**：M1–M7 全部完成 + 归档齐全 + **用户验收确认**（含微信侧配合与最终点头），方视为发版完成。
