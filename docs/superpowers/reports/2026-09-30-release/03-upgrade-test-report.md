# M5 升级测试报告（v2.0.10 → v2.1.0）

- 日期：2026-10-01（本机 Windows / PowerShell 5.1）
- 候选版本：**v2.1.0**（`main` = `fbb6d24 chore(release): 2.1.0`，VERSION=2.1.0）
- 上游 dry-run 产物：Release workflow `mode=build` run `36743632596`（`AGENT_NOTIFY_SIGN_PFX` 签名，**未发布**）
- 本机现状：安装版 2.0.10 → 升级 2.1.0 → 卸载 → 回退 2.0.10 → 再前进 2.1.0（最终停在 **2.1.0**）
- 数据：`C:\Users\srafy\AppData\Local\AgentNotify\data\state.db`；配置：`C:\Users\srafy\.config\agent-notify`

## 0. 前置（§1）
- CI Secret 就绪：`RELEASE_REPO_TOKEN`、`AGENT_NOTIFY_SIGN_PFX_BASE64`、`AGENT_NOTIFY_SIGN_PFX_PASSWORD` 均在 Actions 可见。
- D 盘可用空间 > 50 GB；dry-run 与本地安装均在本机完成。

## 1. dry-run（§5-4）
| 步骤 | 命令 | 结果 |
|---|---|---|
| validate | `gh workflow run release.yml -f mode=validate`（run `36743388274`） | `success` |
| build | `gh workflow run release.yml -f mode=build`（run `36743632596`） | `success`；`Publish release assets` = **skipped**（dry-run 不发布） |
| 发布侧断言 | `gh release list` / `git ls-remote --tags` | 无 `v2.1.0` tag、无 `v2.1.0` Release（未越界发布） |

**产物**（`gh run download 36743632596` → `D:\Temp\agentnotify-release-verify\release-assets`）：
- `dist\Agent-notify-Setup-v2.1.0.exe`（8.93 MB）、`dist\Agent-notify-v2.1.0.zip`（10.46 MB）、`dist\SHA256SUMS.txt`、`release-notes.md`。
- 源 ZIP 含 `RELEASE-MANIFEST.json`（schemaVersion/product/version/hashAlgorithm/files）与 `RELEASE-MANIFEST.p7s`；构建步骤已断言 ZIP 内程序签名指纹（`build-release.ps1`）。
- 说明：dry-run 只产出「源码 ZIP + 安装器」；客户端更新仓的多个 ZIP 由发布阶段（`publish-release.yml`）处理，本次 dry-run 不触及。

## 2. 产物级 secret 扫描（§5-5）
- 解包 ZIP 全量文本扫描 + Setup.exe 二进制扫描关键字：`BEGIN PRIVATE KEY` / `BEGIN RSA PRIVATE KEY` / `gho_` / `password=` → **零命中**。

## 3. 签名/清单正负例（§5-6 签名）
- `cargo test -p agentnotify-desktop --test update_verify --test update_manifest`（在 **Windows PowerShell 5.1** 父进程下，与 gate/CI 一致）：
  - `update_manifest`：`8 passed; 0 failed`（含 `a_tampered_manifest_or_payload_is_rejected_before_install`、`extra_or_missing_payload_files_are_rejected`、`a_single_missing_control_file_is_always_rejected`、`wrong_signer_..._rejected`）。
  - `update_verify`：`11 passed; 0 failed`（含 `updater_rejects_checksum_mismatch_and_unsigned_package_when_required`、`updater_rejects_non_pe_content_even_when_checksum_matches`、`pinned_formal_channel_reads_the_embedded_signature_and_reject_unknown_signers`）。
- 结论：签名/清单**正例接受、负例（篡改清单/任一文件、校验和不符、未签名、非 PE、未知签名者）全部拒绝**。
- **环境注记（非代码缺陷）**：该套件从 PowerShell 7（pwsh）父进程直接跑时，会让子 `powershell.exe`(5.1) 继承 PS7 的 `PSModulePath`，导致 `New-SelfSignedCertificate` 报 `Cannot find drive 'Cert'`；从 Windows PowerShell 5.1 父进程（`tools\rust\gate.ps1`、CI、或 `powershell -File` 包装）运行即通过。已复现并确认：同样的脚本在 5.1 父进程下签名成功、指纹读出正确。

## 4. 本地升级（真实数据，§5-6）
- **备份**：`sqlite3 state.db ".backup 'D:\Temp\agentnotify-upgrade-backup\state.db'"`（一致快照）+ `~/.config/agent-notify` 52 文件。
- **基线**：`orc_tasks=3 / notifications=1214 / deliveries=1209 / reply_routes=659 / inbound_claims=171 / outbox=837 / schema_migrations=3 / 安装版 2.0.10`。
- **安装 2.1.0**（`/VERYSILENT ... /DIR=<官方目录>`，exit 0）后：
  - 安装版 = **2.1.0**；`schema_migrations` 仍为 **3**（集群 v1 未新增迁移，复用既有表）；迁移报告 `legacy-import-report.json` 在。
  - **计数与关键字段完全一致**（3/1214/1209/659/171/837）——数据无损、无重复。
  - **旧版 Go 遗留文件**（`reply-routes.jsonl` 95853B、`reply-state.jsonl` 18356B）时间戳不变（2026-09-19）——未被改写或迁移破坏。
- **集成归属（预览残留=false）**：
  - OpenCode 插件 `~/.config/opencode/plugins/agent-notify.ts`（安装时间 2026-10-01 01:14），`BAKED_INGRESS = "C:\Users\srafy\AppData\Local\Programs\Agent-notify\agentnotify-ingress.exe"`（正式路径）。
  - Codex notify 链：`notify = [ "C:/Users/srafy/AppData/Local/Programs/Agent-notify/agentnotify-codex-hook.exe", "codex", "turn-ended" ]`（正式路径）。
- **WebView2 兼容**：本机 WebView2 Runtime **154.0.4258.37**（支持 `color-mix`，主题色/柔光无退化）；README 已把 WebView2 Runtime 列为安装要求。

## 5. 卸载残留（§5-6 卸载）
- 卸载 2.1.0（`unins001.exe /VERYSILENT`，exit 0）后：
  - 快捷方式（开始菜单 / 桌面 / 启动）**全部移除**；`HKCU\...\Uninstall\{E7A...}_is1` 键**移除**；无计划任务；无服务。
  - 安装目录保留：`agentnotify-desktop.exe`（**因当时进程在运行被占用**，属 Windows 正常行为；关闭进程后可删除，已验证可删）+ 旧 Go 版遗留文件（`agent-notify-install.json`/`docs`/`plugin`/`tools`/`unins000.*`，由旧版卸载器负责，按 §3.2 保留/忽略）。
- 数据与配置（`state.db`、`~/.config/agent-notify`）**保留**。

## 6. 回退与再前进（§5-7）
| 步骤 | 结果 |
|---|---|
| 卸载 2.1.0 | exit 0；数据保留 |
| 安装 **2.0.10**（官方 Release 安装器） | exit 0；安装版 2.0.10；数据计数一致（3/1214/1209/659/171/837）。**更正（2026-10-01 真机复核）**：该库已含迁移 3（orchestration，由更早的 orchestration 预览应用），而**官方 2.0.10 只认得迁移 1–2**，会以「数据库包含当前程序无法识别的迁移版本」拒绝打开 → 对「跑过 orchestration 预览」的库，回退官方 2.0.10 **数据不可用**（前向 schema 限制，非 2.1.0 缺陷）；对从官方 2.0.10 直接升级的库不受影响（迁移 3 由 2.1.0 应用）。详见 `09-post-release-verification.md` |
| 再安装 **2.1.0** | exit 0；安装版 2.1.0；计数仍一致（无重复/无丢失）→ **迁移幂等** |

## 7. 留待用户（不可自动化的真机项）
- **功能冒烟**（§5-6）：集群跑一轮 + 继续迭代、推送头任务名、渠道健康/推送已断、更新器应用内自检——需要 OpenCode 实跑 + 微信渠道 + 用户在场，归入 M2/§3.4 真机矩阵与 **T-J** 一并验收。
- **正式发布**（§6）：打 tag / 发布 / 发布后验证——**按要求停在暂停点，等用户点头**。

## 8. 结论
- dry-run validate/build 全绿且未发布；产物 secret 扫描零命中；签名正负例通过。
- 真实数据 2.0.10→2.1.0 升级数据无损、迁移幂等、集成归属正确；卸载无（受控）残留；回退/再前进通过。
- **M5 可自动化部分全部通过**；功能冒烟与正式发布留待用户。
- 证据目录：本文件 + `D:\Temp\agentnotify-release-verify`、`D:\Temp\agentnotify-upgrade-backup`（仓库外原始产物/备份）。
