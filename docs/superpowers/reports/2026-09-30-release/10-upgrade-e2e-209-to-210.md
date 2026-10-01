# 真机升级端到端：2.0.9 → 2.1.0（补 §6-2「低版本 → 2.1.0」实测）

- 日期：2026-10-01（发布后补充验证）
- 方法：CDP（Playwright `connectOverCDP`）驱动真实 Tauri WebView2，完整走用户路径「设置 → 检查与安装更新」，无人工干预。
- 结论：**2.0.9 客户端真机升级到 2.1.0 全链路成功**（检查更新 → 发现新版本 → 下载 → 校验 → 静默安装 → 自动重启），补齐 `09-post-release-verification.md` §3 中「未能真机端到端」的缺口。

## 步骤与证据
| 步骤 | 结果 | 证据 |
|---|---|---|
| 安装官方 2.0.9（Release 安装器） | 成功；以 `host_version=2.0.9` 启动 | `runtime.log` 2026-10-01T04:35:54Z |
| 「检查更新」 | 返回「发现新版本 v2.1.0，可下载并安装。」 | 截图 `03-check-result.png`、`run.log` |
| 「下载并安装」 | 进入「正在下载并安装」；`SHA256SUMS.txt` 与 `Agent-notify-Setup-v2.1.0.exe`（8.9 MB）落到 `%LOCALAPPDATA%\AgentNotify\temp\updates\2.1.0\` | 截图 `04/05`、目录时间戳 12:36:39–43 |
| 校验与安装 | 安装器静默执行：写入文件、创建图标/快捷方式、重建卸载键 `{E7A4419F-…}_is1`、`Installation process succeeded`，Run 项拉起新版本 | `last-update.log` 12:36:44–57 |
| 重启为 2.1.0 | `host_version=2.1.0` 启动；注册表版本 2.1.0 | `runtime.log` 04:36:52Z、注册表 |
| 集成回归 | Codex notify 链 = Hook 直连（正式安装路径）；OpenCode 插件 `BAKED_INGRESS` 指向正式安装路径 | 文件检查 |
| 数据迁移 | 升级后 `schema_migrations = 1,2,3`（迁移 3 由 2.1.0 应用） | sqlite 查询 |

## 说明
- **前向迁移限制的规避**：本机原库含迁移 3（编排预览），官方 2.0.9 无法打开（见 `09` §4）。本次用「原库副本去掉迁移 3 记录与 `orc_tasks`（`0003` 为纯增表）」作为 2.0.9 的起始库；升级后由 2.1.0 正常重建迁移 3。验证完成后已把**原始库完整恢复**（含编排任务，`orc_tasks=3`），并以干净方式重启（无调试端口）。
- 升级耗时：从点击「下载并安装」到新版本启动约半分钟（下载 8.9 MB + 校验 + 安装 + 重启）。
- 证据文件（仓库外）：`D:\Temp\agentnotify-upgrade-test\`（截图 01–05、`run.log`、升级后的库副本 `state-after-upgrade.db`）。
- 本页仅归档验证证据，不涉及代码改动。
