# 一键升级实现计划（Rust 桌面版）

日期：2026-09-22
目标：补齐 Go 版有、Rust 版缺失的最后一块能力——**检查更新 → 下载 → 校验 → 拉起安装**。

## 范围

**做**（对齐 Go 版 `internal/update` 的既有行为）：

1. 查询最新 Release（来源固定为二进制仓库 `srafyhucl-cpu/agent-notify-releases`，见 AGENTS.md）
2. 版本比对（当前版本 vs 最新版本；本地版本更高或相等 → `UpToDate`）
3. 下载安装包到临时目录（有大小上限与重试；不使用 `%TEMP%` 以外的可写位置，且落在应用自己的临时目录）
4. 校验：`SHA256SUMS.txt` 中的 SHA256 + Authenticode 签名指纹 + PE 版本号（复用现有 `update/verify.rs`，不重写）
5. 拉起安装器（优先），失败则退回 ZIP 解包替换（沿用 Go 版语义与路径安全检查）
6. 状态与失败文案走现有 `UpdateStatusDto`（`UpToDate` / `Available` / `ReadyToInstall` / `Unsupported` / `Failed`），失败必须给用户看得懂的中文原因
7. UI：Settings 页在"检查更新"旁增加"下载并安装"入口（仅在 `Available` / `ReadyToInstall` 时可用），显示进度与结果

**不做**（明确排除，避免范围蔓延）：

- 不做增量/差分更新
- 不做后台静默自动升级（必须用户点击）
- 不改签名信任链与内置指纹约定（`internal/update/signature.go` 的 `defaultSignatureThumbprint` 仍是唯一来源，轮换流程不变）
- 不动 Go 版源码（回滚窗口内保留）

## Files

- Create: `hosts/desktop-tauri/src/update/release.rs`（GitHub Release 查询与版本比对）
- Create: `hosts/desktop-tauri/src/update/download.rs`（下载、大小上限、重试、SHA256SUMS 解析）
- Create: `hosts/desktop-tauri/src/update/install.rs`（拉起安装器 / ZIP 回退解包与路径安全）
- Create: `hosts/desktop-tauri/tests/update_release.rs`、`tests/update_download.rs`、`tests/update_install.rs`
- Modify: `hosts/desktop-tauri/src/update/mod.rs`（导出新模块）
- Modify: `hosts/desktop-tauri/src/production/service.rs`（`get_update_status` 接真实查询；新增 `install_update` 命令）
- Modify: `hosts/desktop-tauri/src/bridge/{dto.rs,commands.rs,mod.rs}`（新命令与 DTO，保持 `bridge_contract.rs` 的"命令名只增不改"约定）
- Modify: `apps/desktop-ui/src/features/settings/*`（安装入口与进度）
- Modify: `hosts/desktop-tauri/tests/bridge_contract.rs`（新命令名与 DTO 契约）

## Interfaces

- `UpdateStatusDto` 不变（`state` 覆盖全部状态）
- 新增 `InstallUpdatePayload { }` / `InstallUpdateResultDto { state, message, installed_version: Option<String> }`
- 网络：`reqwest`（工作区已有），超时与大小上限写成命名常量
- 临时目录：走 `AppPaths` 的临时目录（隔离测试可覆盖），**不使用 `%TEMP%` 直连**

## Steps

1. **写失败测试**：版本比对（本地更新/相等/更旧）、Release JSON 解析（缺资产/缺 checksum → 明确错误）、下载大小上限、SHA256SUMS 解析（缺失/格式错/不匹配）
2. **运行并确认失败**（包/模块不存在）
3. **实现 Release 查询与版本比对**
4. **实现下载与校验串联**（复用 `verify_download`；正式渠道强制签名，预览渠道放宽但必须在结果里标明 `signed`/`preview`）
5. **实现安装**：优先拉起安装器（参数与 Go 版一致），失败退回 ZIP 解包替换；两条路径都要有路径越界防护与大小预算
6. **接线**：`get_update_status` 返回真实状态；新增 `install_update` 命令与 UI 入口
7. **门禁并提交**：`gate.ps1`、`lint.ps1`、`node_modules\.bin\tsc.cmd --noEmit`、UI 测试；提交信息 `feat(update): 支持检查更新与一键升级`

## 风险与边界

- **网络与限流**：GitHub API 未认证有速率限制；失败必须明确提示（不静默、不假装已是最新）
- **真机验收**：发布新版本后才能端到端验证；本地可用"把当前版本调低 + 指向测试 Release"的方式验证，但**不得**绕过签名校验
- **回滚**：升级失败必须保持旧版本可用（下载/校验失败绝不触碰已安装文件；替换失败要能恢复备份）
- **Windows 特有**：安装器需要提权时由系统弹 UAC；应用不自行提权
