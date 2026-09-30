# M1 门禁证据：§2 必修 + §3.3 整库测试复跑

- 日期：2026-09-30（本机，Windows/PowerShell）
- 分支：`feat/cluster-ui-flow`，M1 合并后 HEAD `8847fa7`
  - `345158d merge(m1): Rust 必修 B1/B2/B3/T1`
  - `8847fa7 merge(m1): 前端必修 B4/B5`
- 子代理工作区：`D:\Temp\wt-m1-rust`（`4d053d5`）、`D:\Temp\wt-m1-ui`（`2d0d05e`），均基于 `24b30ba`。
- 纪律：同一时间只跑一个门禁；下列命令逐条串行执行。

## §2 必修落地（源码 review 结论）
| 编号 | 实现 | 验证 |
| --- | --- | --- |
| B1 | `OrcCommandHandler` 增 per-task 写锁；`advance` 拆「加锁外壳 + 无锁内核 `advance_locked`」；`report_from_agent` 增参 `settled_turn_ms: Option<i64>`，命中 `Ok(true)` 时在**同一临界区**完成 `mark_settled_turn`；看门狗删除二次读改写 | `orchestration_commands::concurrent_task_writes_do_not_lose_updates`；`orc_watchdog` 回归通过 |
| B2 | `update_task_step` 增 model≤200 / variant≤64 上限 + `orc_step_variant_invalid`；可注入 `OrcModelCatalog`，variant 非空按模型 `variants` 白名单校验，目录不可用明确报错；生产 4 处注入 `DesktopModelCatalog` | `update_task_step_rejects_variant_outside_whitelist` / `..._rejects_overlong_model_and_variant` / `..._rejects_variant_when_catalog_unavailable` |
| B3 | `validate_task_name` 用 `ORC_TASK_NAME_FORBIDDEN_CHARS=['【','】']` 拒绝分隔符（码 `orc_task_name_invalid`） | `task_name_rejects_wechat_separators`；`wechat_command::address_separator_does_not_misroute` |
| B4 | `useOpencodeModels` 增 `enabled`；`TaskDetail` 仅在下拉聚焦时置 `modelsRequested=true`，失败在区块内就地提示 | ClusterPage B4 用例（未开下拉不请求 / 打开后请求一次 / 失败就地提示） |
| B5 | `OrcNodeInlineModelFields` 250ms 防抖（常量 `INLINE_MODEL_SAVE_DEBOUNCE_MS`）、保存中禁用、卸载清理 timer | OrcNodeChain B5 用例（连续切换只发一次 / 保存中禁用 / 卸载丢弃未发出保存） |
| T1 | `resume.rs` 锁定派活 wire JSON 的 `variant`（Some 写出 / None 不出现） | `dispatch_options_write_variant_when_present` 等 |

## §3.3 整库测试命令与结果（合并后复跑）
| 步骤 | 命令 | 结果 |
|---|---|---|
| 1 Go 静态 | `gofmt -l cmd internal`；`go vet ./...` | 无输出（PASS）；`vet-exit=0` |
| 2 Rust 静态 | `cargo fmt --all --check`；`cargo clippy --workspace --all-targets -- -D warnings` | `fmt-exit=0`；`clippy-exit=0`（零警告，`Finished` in 3m19s） |
| 3 Rust 全门禁 | `powershell -File tools\rust\gate.ps1` | `gate-exit=0`；尾行 `[gate] cleaned 3 test temp directories`；116 个 test 二进制 `test result: ok`，0 failed |
| 4 绑定契约 | `tools\rust\export-bindings.ps1` → `git status --short` | `export-exit=0`；`clean (no stale bindings)` |
| 5 Go 单测 | `go test ./...` | 全 ok，`go-test-exit=0`（含 internal/reply 16.4s、clawbot 3.7s、update 5.3s） |
| 6 插件 | `node --test plugin\rust\agent-notify.test.cjs` | `# pass 30 / # fail 0`，`plugin-exit=0` |
| 7 脚本/冒烟 | `tools\test.ps1`；`tools\lint.ps1` | `SMOKE ALL GREEN`，`tools-test-exit=0`；`[lint] 语法解析通过（38 个文件）`+PSScriptAnalyzer 通过+`[version] 2.0.10 在所有发布位置一致`，`lint-exit=0` |
| 8 前端 | `tsc --noEmit`；`npx vitest run`；`npm run build`；`npm run test:a11y`；`npm run test:visual` | `tsc-exit=0`；vitest `12 files / 140 passed`；build `✓ built in 9.68s`；a11y `18 passed`；visual `16 passed` |
| 9 覆盖率（可选） | `cargo llvm-cov --summary-only`；`npx vitest run --coverage` | **工具不可用，跳过**：`no such command: llvm-cov`；`node_modules\@vitest\coverage-v8` 未安装 |
| 10 仓库 secret 扫描 | `BEGIN ... PRIVATE KEY` / `gho_` / `password="..."` 赋值 / 跟踪的 `*.pfx/*.pem/*.p12/*.key` | **零真实命中**：仅计划文档自身字面提及关键字；无赋值、无密钥文件 |

## 结论
- §2 必修 B1–B5、T1 全部落地并合并；§3.3 步骤 1–8、10 全绿；步骤 9 因本机工具缺失按计划记录并跳过。
- 本机门禁证据日志：`D:\Temp\agentnotify-temp\m1-rust-gate.log`、`m1-clippy.log`、`m1-go-test.log`、`m1-tools-test.log`、`m1-tools-lint.log`、`m1-vitest.log`、`m1-ui-build.log`、`m1-ui-a11y.log`、`m1-ui-visual.log`（仓库外原始输出备份）。
