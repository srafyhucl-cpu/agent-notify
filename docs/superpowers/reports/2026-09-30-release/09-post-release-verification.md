# 发布后验证（§6-2，v2.1.0 已发布）

- 日期：2026-10-01
- tag/Release：`v2.1.0`（tag commit `e0f8832`；Release workflow run `36805809455`，validate → 签名 build → publish 全绿）
- 链接：
  - 源码仓：https://github.com/srafyhucl-cpu/agent-notify/releases/tag/v2.1.0
  - 二进制镜像仓（更新源）：https://github.com/srafyhucl-cpu/agent-notify-releases/releases/tag/v2.1.0

## 1. 发布产物与完整性
| 项 | 结果 |
|---|---|
| 两仓 Release | 均 `draft=false / prerelease=false`，发布于 2026-10-01T02:44 UTC；资产 `Agent-notify-Setup-v2.1.0.exe` + `Agent-notify-v2.1.0.zip` + `SHA256SUMS.txt` |
| 下载并自校验 | 下载资产 SHA256 **与发布 `SHA256SUMS.txt` 完全一致**（ZIP `b016fd4c…`、Setup `1a19f569…`） |
| 清单 | 源 ZIP 内 `RELEASE-MANIFEST.json`：product=AgentNotify / version=2.1.0 / files=**24**（覆盖全部普通文件）；`RELEASE-MANIFEST.p7s` 随包 |
| 安装（发布包） | 用**发布**的 Setup 覆盖安装官方目录 → 成功，版本 **2.1.0**，DB 正常打开（`orc_tasks=7`） |

## 2. 客户端「检查更新」（§6-2）
- 在 2.1.0 桌面端「设置 → 应用 → 检查与安装更新」点「检查更新」，宿主查询**已发布 Release** 后返回：
  - 「**当前已是最新版本，无需下载安装。** 当前版本 2.1.0」；
  - 「**正式通道将拒绝未签名更新包。**」（Stable 通道按内置指纹校验签名）；
  - 「下载并安装」按钮**禁用**（无新版本）。
- 结论：**检查更新链路对已发布 Release 端到端可用**（正确识别 2.1.0 为最新；前次检查失败会要求重检；无新版本时 `install_update` 返回 UpToDate 而非假装修装——与 `production_contract.rs` 一致）。

## 3. 下载/安装/校验 全链路（说明）
- **未能真机端到端**：需「低版本 → 更新到 2.1.0」，但本机库含迁移 3（见 §4），官方 2.0.10 无法打开该库，无法作为更新源起点；且 2.1.0 已是通道最新（`下载并安装` 禁用）。
- **已由测试覆盖**：`hosts/desktop-tauri/tests/update_download.rs`、`update_install.rs`、`update_verify.rs`、`update_manifest.rs`、`update_release.rs`（gate 全绿）覆盖下载、校验和、签名/清单正负例、非 PE、版本不符、安装与回滚安装；`production_contract.rs` 覆盖 `get_update_status`/`install_update` 的 UpToDate 语义。
- **Beta/Stable 通道**：Stable 内置指纹校验（未签名/未知签名者拒绝）；Beta 仅两控制文件同时缺失时兼容旧开发包——由 `update_verify`/销售通道用例覆盖。

## 4. 回退限制更正（重要，非 2.1.0 缺陷）
- v2.1.0 相对 v2.0.10 **新增迁移 3**（`0003_orchestration.sql`）；本机库已含迁移 3（由更早的 orchestration 预览应用）。
- **官方 2.0.10 只定义迁移 1–2** → 打开含迁移 3 的库时报「数据库包含当前程序无法识别的迁移版本」，拒绝启动业务（前向 schema 限制）。
- 影响面：仅**跑过 orchestration 预览**的机器回退到官方 2.0.10 时数据不可用；**从官方 2.0.10 直接升级到 2.1.0 的库不受影响**（迁移 3 由 2.1.0 应用）。已在 `03-upgrade-test-report.md` §6 更正。

## 5. 已发布 Release 不被自动覆盖（§6-2）
- 发布仅在 **tag push** 时触发；`workflow_dispatch` dry-run 的 publish job 被 `if: github.event_name == 'push'` 跳过（本会话 dry-run 实测：publish=skipped，无 tag/Release）。
- 补发脚本 `tools\publish-release.ps1` 默认拒绝覆盖已发布 Release，覆盖需显式 `-AllowPublished`（人工 + 审计输出）。

## 6. 坏包预案（§6-3）
- 如已发布包有问题：**不覆盖**已发布 Release，走补丁版 `2.1.1` + 审计输出。

## 7. 结论
- v2.1.0 已在源码仓与二进制镜像仓发布，产物完整、可下载、可安装、签名/清单自洽；客户端「检查更新」对发布通道端到端可用。
- 更新器的下载/安装/校验/签名正负例由测试覆盖；「低版本→2.1.0」真机更新因前向迁移限制未做（已记录）。
- 发布后 backlog（Codex 通知链、文案、账号硬删除、长回合）见 `08-post-release-backlog.md`。
