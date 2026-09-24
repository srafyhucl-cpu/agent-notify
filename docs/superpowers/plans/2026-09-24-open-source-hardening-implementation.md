# Agent-notify 开源成熟度平衡加固实施计划

- 日期：2026-09-24
- 对应设计：`docs/superpowers/specs/2026-09-24-open-source-hardening-design.md`
- 实施分支：`chore/open-source-hardening`
- 独立 worktree：由当前 OpenCode 会话维护，不在公开文档中固化本机绝对路径
- 目标版本：v2.0.7

## 1. 执行约束

1. 只在当前独立 worktree 修改文件，不直接操作主工作区的未提交 UI 改动；
2. 不修改 Go 业务代码和 Go 测试，只按仓库门禁执行现有 Go 检查；
3. 不修改 UI 视觉和交互；CI 只接入既有 UI 门禁；
4. 每个任务先写失败测试或静态门禁，再写实现；
5. 每个任务只暂存本任务文件，提交前运行 `git diff --cached --check`；
6. 签名材料、证书私钥、过滤掩码和历史重写中间产物只允许位于明确的 `D:\Temp` 路径，用后清理，不能进入 Git；
7. 在主工作区仍有 UI 未提交改动时，不合并本分支、不重置主工作区；
8. 任何签名、版本、Release 资产或真实链路门禁失败都停止发布，不做猜测式降级。

## 2. 阶段与提交

实施按以下提交推进：

1. `docs(open-source): 明确平衡加固实施与私密报告渠道`
2. `chore(repo): 忽略本地签名与密钥材料`
3. `docs(open-source): 对齐安全卸载与社区指南`
4. `feat(manifest): 定义并验证签名发布清单`
5. `feat(release): 构建并门禁签名 ZIP 清单`
6. `feat(update): 使用签名清单校验 ZIP 回退`
7. `ci: 加固工作流与依赖更新覆盖`
8. `docs(release): 更新安全构建与贡献门禁说明`

功能实现完成并与最新 `main` 同步后，再执行不属于代码提交的历史重写、公开和 v2.0.7 发版。

## 3. 任务一：收口设计与实施计划

### 涉及文件

- 修改：`docs/superpowers/specs/2026-09-24-open-source-hardening-design.md`
- 新增：`docs/superpowers/plans/2026-09-24-open-source-hardening-implementation.md`

### 步骤

1. 把安全漏洞私密入口固定为 GitHub Private Vulnerability Reporting；
2. 明确公开前不虚构安全邮箱，公开后启用私密报告；
3. 明确发布 job 使用受保护 `main` 上的可复用 workflow；
4. 扫描计划中的未完成标记、真实账号哈希、消息 ID 和本机敏感路径；
5. 运行 `git diff --check`；
6. 只提交设计补充和本计划。

### 验收

- 设计、实施计划和代码边界一致；
- 文档不包含待脱敏真实值；
- 当前 worktree 除两份文档外无其它改动。

## 4. 任务二：仓库密钥忽略规则

### 涉及文件

- 修改：`.gitignore`

### 步骤

1. 增加 `.env.*`，并用 `!.env.example` 保留示例文件；
2. 增加 `*.pfx`、`*.pem`、`*.p12`、`*.cer`、`*.snk`、`*.key`；
3. 不增加宽泛的 `*.json`、`*.txt` 或 `secrets*` 规则；
4. 用 `git check-ignore --no-index` 验证测试文件均被忽略、`.env.example` 未被忽略；
5. 确认仓库没有因为本次规则变化而出现意外删除或新增文件。

### 测试

```powershell
git check-ignore -q --no-index sample.pfx
git check-ignore -q --no-index sample.pem
git check-ignore -q --no-index sample.p12
git check-ignore -q --no-index sample.cer
git check-ignore -q --no-index sample.snk
git check-ignore -q --no-index sample.key
git check-ignore -q --no-index .env.local
git check-ignore -q --no-index .env.example
```

`.env.example` 的命令应返回未忽略。

### 提交

```text
chore(repo): 忽略本地签名与密钥材料
```

## 5. 任务三：用户、安全与社区文档对齐

### 涉及文件

- 修改：`README.md`
- 修改：`SECURITY.md`
- 修改：`CONTRIBUTING.md`
- 修改：`docs/ARCHITECTURE.md`
- 修改：`docs/code-signing.md`
- 修改：`docs/TROUBLESHOOTING.md`
- 修改：`.github/ISSUE_TEMPLATE/bug_report.yml`
- 修改：`.github/ISSUE_TEMPLATE/feature_request.yml`
- 修改：`.github/pull_request_template.md`
- 新增：`.github/ISSUE_TEMPLATE/config.yml`

### 步骤

1. README 补 Windows x64、WebView2、自签名/SmartScreen 预期、哈希与指纹核对；
2. 把回滚步骤从内部 spec 移到 `TROUBLESHOOTING.md` 的稳定用户章节；
3. 明确 2.0.0–2.0.4 的升级限制和 ZIP 回退后需手动重启；
4. 增加 ZIP 便携使用、开发和卸载说明；
5. 增加从克隆到启动 Tauri 桌面的开发 Quick Start，实际命令必须与 `package.json`、Cargo workspace 和现有脚本一致；
6. 区分普通卸载、退出渠道账号和手动彻底清理；手动清理必须包含 Windows 凭据管理器检查；
7. 卸载章节逐项列出 Codex、Antigravity、Devin、Command Code 接入的删除或还原；
8. `SECURITY.md` 把签名指纹环境变量统一写成“覆盖”，不再写“追加”；
9. 明确 `AGENT_NOTIFY_REQUIRE_SIGNATURE` 只属于 Go 1.x；
10. 删除公开 Issue 作为漏洞报告渠道的兜底；唯一私密入口为 GitHub Private Vulnerability Reporting；
11. 签名文档删除仓库私有假设，描述 tag、协作者、Environment 和 Secret 威胁模型；
12. 日志隐私提示改为只附必要行，并先移除用户名、路径、账号提示、客户端标识和消息标识；
13. Bug 模板改为 2.0 桌面端、`runtime.log` 和五个 Agent 日志；Feature 模板同步五个 Agent；
14. 新增 Issue 配置，关闭空白 Issue，链接源码仓文档与安全报告入口；
15. PR 模板列出 Rust、UI、脚本、Go 遗留和全量门禁，不把 `tools/test.ps1` 误称为全部测试；
16. `docs/superpowers/**` 增加维护者归档说明，不删除现有设计记录。

### 测试

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

额外人工检查：

- README 中每个本地命令都存在对应脚本或 npm script；
- 安装、升级、回滚、卸载四处对 ZIP 行为的描述一致；
- SECURITY 与 `docs/code-signing.md` 对环境变量语义一致；
- Issue 模板不出现旧悬浮窗、`push.log`、`widget-error.log` 或旧 doctor 命令。

### 提交

```text
docs(open-source): 对齐安全卸载与社区指南
```

## 6. 任务四：发布清单合同与 Rust 验证器

### 涉及文件

- 新增：`config/release-manifest-contract.json`
- 新增：`hosts/desktop-tauri/src/update/manifest.rs`
- 修改：`hosts/desktop-tauri/src/update/mod.rs`
- 新增：`hosts/desktop-tauri/tests/update_manifest.rs`
- 新增：`hosts/desktop-tauri/tests/fixtures/release-manifest/` 下的非敏感测试载荷、清单和 CMS 签名夹具

### 步骤

1. 先写清单解析测试：合法、未知字段、非法版本、非法算法、重复路径、大小写冲突和路径穿越；
2. 新增合同文件，单一来源定义产品名、schema、文件名、哈希算法、最大文件数和大小上限；
3. Rust 使用 `include_str!` 读取合同，并用 `serde(deny_unknown_fields)` 严格解析；
4. 定义清单条目：相对路径、字节大小和 SHA-256；
5. 实现实际文件集合与清单双向精确比较，拒绝缺失和额外文件；
6. 实现逐文件大小和 SHA-256 校验，继续复用现有分块读取常量；
7. 为 Windows CMS 验证写接口测试，签名错误时返回稳定错误码；
8. 使用 `CryptVerifyDetachedMessageSignature` 验证 detached CMS；
9. 验证签名者数量为 1、摘要算法为 SHA-256、指纹命中信任列表、证书有效期覆盖注入时间；
10. 非 Windows 平台保留明确的“不支持正式清单签名”结果，Stable 不得误放行；
11. 生成测试夹具时只在 `D:\Temp` 创建临时证书，仓库只提交公开清单、公开证书所在的 CMS 签名和非敏感载荷，绝不提交 PFX 或私钥；
12. 测试结束删除临时证书和私钥材料。

### 核心 API

```rust
pub fn verify_release_manifest_at(
    root: &Path,
    expected_version: &str,
    requirement: SignatureRequirement,
    now: SystemTime,
    trusted_thumbprints: &[String],
) -> Result<VerifiedReleaseManifest, UpdateError>;
```

生产包装器读取当前时间和环境信任列表；测试包装器注入固定时间与测试指纹，避免修改全局环境造成并行测试竞态。

### 测试

```powershell
cargo test -p agentnotify-desktop --test update_manifest --locked
```

测试用例至少包括：

- 合法签名清单通过；
- 清单改一字节拒绝；
- 文件哈希或大小变化拒绝；
- 缺失文件、额外文件、重复路径和大小写冲突拒绝；
- 路径穿越和绝对路径拒绝；
- 错误签名者拒绝；
- 固定时间早于 `NotBefore` 或晚于 `NotAfter` 拒绝；
- 多签名者或非 SHA-256 摘要拒绝；
- Stable 缺清单/缺签名拒绝；
- Beta 两个控制文件同时缺失时由后续兼容层处理。

### 提交

```text
feat(manifest): 定义并验证签名发布清单
```

## 7. 任务五：PowerShell 构建与发布门禁

### 涉及文件

- 新增：`tools/release-manifest.ps1`
- 修改：`tools/build-release.ps1`
- 修改：`tools/signature-common.ps1`
- 修改：`tools/release-gate.ps1`
- 修改：`tools/publish-release.ps1`
- 修改：`tests/signature-gate.tests.ps1`
- 修改：`tests/smoke.ps1`

### 步骤

1. 在 `tools/release-manifest.ps1` 实现职责单一的命令函数：
   - 读取合同；
   - 收集 staging 文件；
   - 生成 UTF-8 无 BOM 清单；
   - 使用 PFX 生成 SHA-256 detached CMS；
   - 验证清单与签名；
   - 验证 ZIP 文件集合和哈希；
2. 所有函数使用 `Set-StrictMode -Version Latest` 和明确错误；
3. PFX 只从环境变量读取到内存，不写临时 PFX；
4. 签名者指纹以 Rust `DEFAULT_SIGNATURE_THUMBPRINT` 为 2.0 首选来源，旧 Go 常量只保留兼容读取路径；
5. `build-release.ps1` 改成 staging 单一数据流，不再直接从多个源边压 ZIP 边猜最终内容；
6. 五个可执行文件完成 Authenticode 签名后再生成清单；
7. 最终 ZIP 生成后重新打开 ZIP 验证清单、文件集合、哈希和五个程序签名；
8. `-SkipInstaller` 无签名材料时只允许本地开发包，输出警告，发布门禁必须拒绝；
9. `release-gate.ps1` 新增 `Assert-ArchiveReleaseManifest`，并让 `Assert-ArchiveExecutables` 先验证清单；
10. `publish-release.ps1` 只能通过聚合门禁上传，禁止增加绕过参数；
11. `signature-gate.tests.ps1` 在 CurrentUser 证书存储临时创建测试证书，测试后无论成功失败都删除；
12. 临时证书只存在于内存或明确的 `D:\Temp` 文件，不进入发布包和 Git。

### 测试

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\signature-gate.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\smoke.ps1
```

门禁回归至少覆盖：

- 合法清单放行；
- 清单篡改拒绝；
- 文件篡改拒绝；
- 额外 DLL/脚本拒绝；
- 缺失文件拒绝；
- 错误指纹拒绝；
- 清单签名缺失拒绝；
- ZIP 五个程序缺失或签名不符仍按原逻辑拒绝；
- `publish-release.ps1` 不能绕过聚合门禁。

### 提交

```text
feat(release): 构建并门禁签名 ZIP 清单
```

## 8. 任务六：更新器接入签名清单

### 涉及文件

- 修改：`hosts/desktop-tauri/src/update/install.rs`
- 修改：`hosts/desktop-tauri/src/update/service.rs`
- 修改：`hosts/desktop-tauri/src/update/mod.rs`
- 修改：`hosts/desktop-tauri/tests/update_install.rs`
- 修改：`hosts/desktop-tauri/tests/update_verify.rs`
- 修改：`hosts/desktop-tauri/tests/update_release.rs`

### 步骤

1. 扩展 `StagedRelease`，保存已经验证的安装文件相对路径列表；
2. 基础布局验证仍只负责根目录、主程序和 `VERSION`；
3. 清单验证成功后，安装文件列表只来自签名清单；
4. Stable 缺少清单或签名时，在主程序验证和文件替换前失败；
5. Beta 仅在两个控制文件同时缺失时回退到旧文件集合；半缺失始终失败；
6. 清单验证完成后继续执行主程序 PE、64 位、版本和 Authenticode 检查；
7. `apply_staged_release` 只遍历已验证文件列表，不再递归复制任意文件；
8. 清单与签名控制文件不复制到安装目录；
9. `prepare_archive_artifact` 的两个调用点都传入签名策略和信任锚；
10. 安装器启动失败后的 ZIP 回退复用同一验证链；
11. 新增稳定错误码并提供可读中文；
12. 错误信息不输出绝对路径、签名内容或凭据；
13. 保留现有备份和逆序回滚测试。

### 测试

```powershell
cargo test -p agentnotify-desktop --test update_install --locked
cargo test -p agentnotify-desktop --test update_verify --locked
cargo test -p agentnotify-desktop --test update_release --locked
```

编排测试至少包括：

- Stable 合法签名 ZIP 成功；
- Stable 无清单 ZIP 拒绝且不修改安装目录；
- Beta 无清单旧 ZIP 保持兼容；
- 只缺一个控制文件拒绝；
- 清单声明外的插件/DLL 被拒绝；
- 安装器启动失败后签名 ZIP 回退成功；
- 复制中途失败恢复所有旧文件；
- 主程序、版本或 Authenticode 失败时清单已验证但仍不替换文件。

### 提交

```text
feat(update): 使用签名清单校验 ZIP 回退
```

## 9. 任务七：CI、Dependabot 与 Release workflow

### 涉及文件

- 修改：`.github/dependabot.yml`
- 修改：`.github/workflows/ci.yml`
- 修改：`.github/workflows/release.yml`
- 新增：`.github/workflows/publish-release.yml`
- 新增：`.github/workflows/codeql.yml`
- 修改：`tools/rust/gate.ps1`
- 修改：`AGENTS.md`

### 步骤

1. Dependabot 增加根 Cargo 和 `apps/desktop-ui` npm，更新频率保持月度；
2. CI 与 Release 的所有官方 Actions 固定到完整 commit SHA，保留版本注释；
3. checkout 显式关闭凭据持久化；
4. PSScriptAnalyzer 固定版本；
5. Rust 门禁的 fmt/clippy/test 统一使用 `--locked`；
6. CI 接入 `tools/ui/gate.ps1`，不复制另一套测试命令；
7. 新增 CodeQL workflow，覆盖 Rust 与 JavaScript/TypeScript；
8. Release 改为 `validate → build → publish`：
   - `validate` 无 Secret，只读权限，严格 SemVer；
   - `build` 使用 PFX，在精确 tag 上运行全部门禁；
   - 构建结果通过 Actions artifact 传给发布阶段；
9. tag 只通过 `env` 传入 PowerShell，禁止表达式直接拼入 `run`；
10. 校验 tag 提交属于受保护 `main` 的可达历史；
11. Inno Setup 下载后先校验固定 SHA-256 和官方 Authenticode 发布者，再执行；
12. `publish` 调用 `srafyhucl-cpu/agent-notify/.github/workflows/publish-release.yml@main`；
13. 可复用发布 workflow 从受保护 `main` checkout，不执行 tag 中的发布脚本，只接收构建产物；
14. 源码与镜像 Release 都先创建 Draft，上传后复核名称、摘要和签名；
15. 镜像先发布为 Latest，源码后发布；失败时保留 Draft；
16. 已发布 Release 禁止 workflow 自动覆盖，补发只走显式脚本；
17. 为 tag 增加 `concurrency`，禁止同版本并发；
18. 更新 `AGENTS.md`：Rust 信任指纹为 2.0 单一来源、发布必须通过签名清单、公开可复用 workflow 的边界。

### 静态测试

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

额外检查：

- workflow 文件保持纯 ASCII；
- 所有 `uses:` 均为 40 位 SHA；
- 非可复用发布 job 不包含 `RELEASE_REPO_TOKEN`；
- `build` job 不包含 `contents: write`；
- 可复用发布 job 不包含 PFX；
- 恶意 tag 字符串只作为环境变量值，不进入 PowerShell 源码；
- `publish-release.yml@main` 在创建 v2.0.7 前已经受 branch protection 保护。

### 提交

```text
ci: 加固工作流与依赖更新覆盖
```

## 10. 任务八：维护文档与全量门禁

### 涉及文件

- 修改：`docs/code-signing.md`
- 修改：`docs/ARCHITECTURE.md`
- 修改：`docs/TROUBLESHOOTING.md`
- 修改：`README.md`
- 修改：`SECURITY.md`
- 修改：`CONTRIBUTING.md`
- 修改：`CHANGELOG.md`

### 步骤

1. 补充清单格式、CMS 信任模型、错误语义和 Beta 兼容边界；
2. 更新架构文档的数据流，明确 `SHA256SUMS.txt` 只检查下载完整性；
3. 更新排障文档的 ZIP 失败原因和人工处理方式；
4. README 简述正式更新会验证完整签名清单；
5. CONTRIBUTING 列出完整开发、门禁、发布和证书轮换流程；
6. CHANGELOG 的 v2.0.7 段记录用户可见修复，不提前写“已发布”；
7. 删除文档中“仓库私有”“指纹追加”“固定清单但实际未验证”等旧说法。

### 全量门禁

```powershell
go test ./...
go vet ./...
gofmt -l cmd internal
node_modules\.bin\tsc.cmd --noEmit
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rust\gate.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\ui\gate.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\check-version.ps1 -Version 2.0.7
```

任何门禁失败都先修复，不带失败结果合并。

### 提交

```text
docs(release): 更新安全构建与贡献门禁说明
```

## 11. 任务九：与并行 UI 工作安全汇合

### 前置条件

- 用户确认 UI 会话已停止写入；
- 主工作区的 UI 改动已经由其会话提交；
- 主工作区 `git status` 干净；
- 当前分支测试全绿。

### 步骤

1. 记录 UI 会话提交 SHA 和主分支最新 SHA；
2. 在独立 worktree 中把最新 `main` 合入 `chore/open-source-hardening`；
3. 如 `tools/build-release.ps1`、`tools/rust/gate.ps1` 或 UI 门禁发生冲突，保留双方语义：
   - UI 的缓存复用改动保留；
   - `--locked`、签名清单和发布门禁保留；
4. 重新执行任务八全部门禁；
5. 确认 diff 中没有 UI 视觉文件和非本计划文件；
6. 把加固分支合回 `main`，不重写或丢弃 UI 提交；
7. 合并后再次确认工作区干净并记录最终 SHA。

此任务不产生把 UI 改动“顺手带入”的独立提交；汇合结果必须有清楚的提交历史。

## 12. 任务十：历史重写演练与执行

### 前置条件

- `main` 干净且包含全部加固与 UI 提交；
- `origin/main`、heads、tags 和 Release 列表已备份；
- 仓库仍为 private；
- `v2.0.7` Release 仍不存在。

### 步骤

1. 创建全量 `git bundle`，位置固定在 `D:\Temp\agent-notify-open-source-backup\`；
2. 导出 `git ls-remote --heads --tags origin`；
3. 从最终本地 `main` 创建新的 bare mirror，不复用旧演练目录；
4. 在 `D:\Temp` 生成本地掩码文件，只包含 3 个真实账号哈希和 5 个消息 ID；该文件不进入 Git；
5. 使用已验证的 `git-filter-repo` 精确替换账号哈希和消息 ID，不使用宽泛正则；
6. 验证提交数、发布树、tag 映射和 `commit-map`；
7. 验证原值在所有重写历史中零命中；
8. 明确强推 `main` 和已发布 tags；禁止 `git push --mirror`；
9. 显式删除远端 `codex/rust-desktop-rewrite`；
10. 公开前删除远端失败的 `v2.0.7` tag，不在 private 状态重新创建；
11. 强推后重新枚举远端 heads/tags 并再次扫描原值；
12. 扫描远端全部历史 Release 说明，确认原值零命中；
13. 记录强制推送和验证输出；
14. 清理掩码文件和临时 mirror，只保留 bundle、验证报告和必要日志。

### 验收

- 远端仅保留 `main` 和已发布 tags；
- 远端没有旧 UI 重写分支；
- `v2.0.7` 暂不存在；
- 远端可达历史和 Release 说明的真实标识命中数为 0；
- 本地 UI 与加固提交均存在于重写后的 `main`。

## 13. 任务十一：转公开与仓库设置

### 步骤

1. 把源码仓改为 public；
2. 验证 Actions 已使用公开仓免费分钟，不再出现账单拒绝；
3. 开启：
   - Dependabot Alerts；
   - Automated Security Fixes；
   - Code Scanning；
   - Secret Scanning；
   - Push Protection；
   - Private Vulnerability Reporting；
4. 为 `main` 启用 branch protection/ruleset，要求 CI，禁止 force push；
5. 为 `v*` 启用 tag ruleset；
6. 开启合并后自动删除分支；
7. 添加 `rust`、`tauri`、`notifications`、`agent` topics；
8. 更新二进制 Release 仓 README，链接公开源码；
9. 关闭二进制 Release 仓 Issues；
10. 重新检查 Community Profile。

### 验收

```text
源码仓 visibility = public
vulnerability alerts = enabled
automated security fixes = enabled
private vulnerability reporting = enabled
main = protected
v* = protected
```

## 14. 任务十二：v2.0.7 发布与真实链路验收

### 步骤

1. 从最终重写历史创建本地 `v2.0.7` tag；
2. 确认 tag 指向 `main` 最终提交；
3. 推送 tag，监控新 Release workflow；
4. 验证 `validate`、全量 gate、build、两个 Draft 和最终 publish 全部成功；
5. 下载源码 Release 与二进制 Release 的安装器、ZIP 和 `SHA256SUMS.txt`；
6. 核对两个仓库三项资产 SHA-256 完全一致；
7. 验证安装器 Authenticode；
8. 验证 ZIP CMS 清单和五个可执行文件签名；
9. 验证 Release 资产不存在草稿状态，镜像为 Latest；
10. 用临时安装目录执行一次 ZIP 回退集成验收，不修改用户现有安装；
11. 使用篡改 ZIP 验证客户端在替换安装目录前失败；
12. 验证安装器下载、签名验证、启动和应用退出路径；
13. 记录 run ID、Release URL、资产摘要、证书指纹和验收结果；
14. B4 由用户在自己的环境执行覆盖升级；B5 保持既有验收结论，不重复宣称新的微信链路测试。

### 完成判定

- Release workflow 全绿；
- 源码与更新仓资产摘要一致；
- 清单、五个程序和安装器签名全部通过；
- Stable 客户端拒绝无清单或篡改 ZIP；
- v2.0.7 成为两个仓库的 Latest；
- 仓库设置和分支/tag 保护已启用；
- 没有覆盖 UI 会话改动；
- 所有临时证书、PFX、掩码和构建目录已清理。
