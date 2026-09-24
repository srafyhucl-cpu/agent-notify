# Agent-notify 开源成熟度平衡加固设计

- 日期：2026-09-24
- 状态：待用户书面审阅
- 目标版本：v2.0.7
- 适用范围：公开源码前的平衡加固，不包含 UI 视觉改版

## 1. 背景

Agent-notify 已具备 MIT 许可证、README、安全策略、贡献指南、更新日志、CI、签名发布和双仓 Release 镜像，但开源审查发现以下公开前问题：

1. 远端 `codex/rust-desktop-rewrite` 分支仍可达真实账号哈希和平台消息 ID；
2. README 的“彻底清理”没有覆盖 Windows 凭据管理器中的敏感凭据；
3. ZIP 自动回退只验证主程序签名，未认证插件、Hook、DLL 和脚本；
4. Release workflow 把 tag 名直接拼入 PowerShell，并在注入签名材料前执行未校验的 Inno Setup；
5. 两个 Release 仓库的发布不是先验证后公开；
6. Issue 模板、安全签名文档、卸载说明和贡献门禁与 2.0 现状不一致；
7. 私有仓状态下 Dependabot Alerts、Code Scanning、分支保护和私密漏洞报告等治理能力尚未启用。

本设计采用“平衡加固”路线：先解决公开阻塞和高优先级安全问题，再公开仓库并完成 v2.0.7 发布；不要求在公开前一次性完成所有长期增强项。

## 2. 目标

1. 在转公开前保证所有远端可达历史不再包含已知真实标识；
2. 让用户对安装、升级、日志、卸载和凭据清理的文档承诺与真实行为一致；
3. 保留 ZIP 自动回退，但用同一发布证书签名的文件清单认证 ZIP 内全部内容；
4. 降低 tag、构建工具、Release 资产和双仓发布链路的供应链风险；
5. 转公开后立即启用可用的免费安全治理能力；
6. 完成 v2.0.7 源码 Release 与客户端更新仓 Release，并验证两边资产一致；
7. 全程在独立 worktree 中实施，不覆盖并行 UI 会话的工作区改动。

## 3. 非目标

本轮不做以下事项：

1. 不修改 UI 视觉与交互；
2. 不修改或新增 Go 业务代码、Go 测试；Go 1.x 仅保留为冻结的回滚实现；
3. 不把插件拆成多文件分发包；
4. 不引入 HSM、云 KMS 或完整 RFC 3161 时间戳服务；文档明确记录其后续路线；
5. 不在公开前追求完整英文文档、双语 Issue 模板或社区讨论区；
6. 不在没有真实门禁结果时宣称 v2.0.7 已发布。

## 4. 总体实施顺序

实施拆成五个可独立验收的阶段：

1. **仓库与文档止血**：修正隐私、清理承诺、安全文档和社区入口；
2. **签名清单与更新器**：认证 ZIP 内全部文件，并补齐回退测试；
3. **Release 供应链加固**：修复 tag 注入、工具校验、权限和双仓发布流程；
4. **历史重写与公开**：重写所有公开 refs，删除旧分支，转 public，启用治理设置；
5. **v2.0.7 发布验收**：发布、验签、核对双仓资产并完成更新链路验收。

每个阶段只提交本阶段文件。历史重写必须在前三阶段全部提交后重新创建离线 mirror，不能复用旧演练结果。

## 5. 签名清单设计

### 5.1 目标

ZIP 保持现有自动回退能力，但发布包中的每个文件都必须由受信任发布证书认证。`SHA256SUMS.txt` 继续用于发现下载损坏，不再作为 ZIP 内部内容的信任根。

### 5.2 控制文件

ZIP 发布根目录新增：

```text
Agent-notify/
├─ RELEASE-MANIFEST.json
├─ RELEASE-MANIFEST.p7s
├─ VERSION
├─ bin/...
├─ plugin/...
└─ tools/...
```

文件名、格式版本和大小上限由 Rust 客户端常量定义；PowerShell 门禁通过合同测试读取并比对这些值，禁止在构建脚本中维护一套可能漂移的独立常量。

### 5.3 清单格式

`RELEASE-MANIFEST.json` 使用 UTF-8 无 BOM：

```json
{
  "schemaVersion": 1,
  "product": "AgentNotify",
  "version": "2.0.7",
  "hashAlgorithm": "sha256",
  "files": [
    {
      "path": "VERSION",
      "size": 6,
      "sha256": "<64 位小写十六进制>"
    }
  ]
}
```

约束如下：

- `schemaVersion` 必须为 1；
- `product` 必须为 `AgentNotify`；
- `version` 必须与 Release 目标版本、`VERSION` 文件和主程序版本一致；
- `hashAlgorithm` 必须为 `sha256`；
- `path` 是相对 `Agent-notify/` 根目录的发布路径，统一使用 `/`；
- 路径禁止为空、绝对路径、`.`、`..`、反斜杠和控制字符；
- 路径按 ordinal 顺序排列；
- 拒绝重复路径和仅大小写不同的冲突；
- 清单最多 256 个文件、1 MiB；CMS 签名最多 64 KiB；
- `size` 使用非负 64 位整数，`sha256` 必须是 64 位小写十六进制；
- `RELEASE-MANIFEST.json` 与 `RELEASE-MANIFEST.p7s` 不出现在 `files` 中，也不复制到安装目录。

清单文件本身不需要规范化 JSON：CMS 签名覆盖实际字节，客户端对签名后的原始字节验签，再解析 JSON。

### 5.4 构建端签名

构建端使用 Windows PowerShell 5.1/.NET Framework 的 `System.Security.Cryptography.Pkcs.SignedCms`：

1. 从 `AGENT_NOTIFY_SIGN_PFX_BASE64` 和 `AGENT_NOTIFY_SIGN_PFX_PASSWORD` 读取同一张发布证书；
2. 不把 PFX 写入磁盘，确认私钥存在、指纹命中内置信任锚且证书当前有效；
3. 对清单原始字节生成 SHA-256 detached CMS/PKCS#7 签名；
4. DER 编码写入 `RELEASE-MANIFEST.p7s`；
5. 签名者私钥和临时对象在 `finally` 中清理，不输出密码或私钥材料。

生产门禁以 Rust 客户端的 `DEFAULT_SIGNATURE_THUMBPRINT` 为 2.0 信任锚唯一来源。冻结的 Go 常量只作为旧发布工具兼容输入，不再作为新构建的首选来源。

### 5.5 构建数据流

`tools/build-release.ps1` 调整为单一 staging 数据流：

1. 构建五个可执行文件；
2. 用现有 Authenticode 流程签名并验证五个可执行文件；
3. 把所有 ZIP 发布文件复制到独立 staging 根目录；
4. 排除清单和签名文件，对 staging 中全部文件排序；
5. 计算每个文件的相对路径、字节大小和 SHA-256；
6. 写清单并生成 CMS 签名；
7. 从 staging 创建最终 ZIP；
8. 重新打开 ZIP，调用发布门禁验证清单、文件集合、哈希与五个可执行文件签名；
9. 构建安装器；
10. 最后生成 `SHA256SUMS.txt`。

正常发布缺少签名材料时立即失败。显式使用 `-SkipInstaller` 的本地开发构建可以生成无清单兼容 ZIP，但必须输出醒目警告；该 ZIP 无法通过发布门禁，也不能被正式客户端安装。

### 5.6 客户端验证

正式 ZIP 的验证顺序固定：

1. 现有 ZIP 路径穿越、符号链接和 200 MiB 解包预算检查；
2. 定位清单和 CMS 签名，执行大小上限检查；
3. 使用 Windows `CryptVerifyDetachedMessageSignature` 验证 detached 签名；
4. 确认只有一个签名者，摘要算法必须为 SHA-256；
5. 确认签名者指纹命中当前信任列表；
6. 确认签名证书的 `NotBefore`/`NotAfter` 覆盖当前时间；
7. 使用 `serde` 严格解析 JSON，拒绝未知字段和重复路径；
8. 确认清单版本与目标版本一致；
9. 收集实际文件集合，与清单做双向精确比较；
10. 逐文件校验字节大小和 SHA-256；
11. 继续执行现有主程序 PE、64 位、文件版本和 Authenticode 验证；
12. `StagedRelease` 保存已验证的安装文件列表，复制阶段只处理该列表。

任何一步失败都在修改安装目录前终止。目录回滚继续使用现有备份和逆序恢复机制。

### 5.7 信任策略

- Stable 通道要求清单和签名都存在且完全有效；
- Beta 通道仅在清单与签名**同时缺失**时允许旧开发 ZIP；
- Beta 中只出现一个控制文件、清单损坏或签名无效时必须拒绝，不能把损坏包降级成旧格式；
- 自签名根不受系统信任不构成失败，证书指纹是信任锚；
- 证书过期、签名者指纹不符、签名数学无效和清单篡改必须失败；
- 吊销检查不适用于当前私有自签名证书，后续迁移到可撤销的公开信任链时再启用；
- 本轮不引入可信时间戳，证书到期后旧清单会按设计拒绝，因此必须在当前证书到期前完成轮换，不能把“签名数学有效”误写成长期有效。

### 5.8 兼容边界

- 旧客户端忽略 ZIP 内新增文件，仍可使用带清单的 v2.0.7；
- 新客户端可安装旧客户端使用的 v2.0.6 之前的版本，因为这些版本已低于当前版本，不会触发更新；
- 新客户端通过自定义更新源获取旧 ZIP 时，Stable 通道会拒绝无清单包；文档明确说明自定义正式更新源必须遵循新格式；
- HTML 重定向回退只下载 ZIP，但 ZIP 内清单提供完整认证，不依赖 Release API；
- 安装器优先路径保持现有 Authenticode 校验，不因清单实现改变行为。

### 5.9 错误语义

新增稳定错误码：

| 错误码 | 用户可读含义 |
|---|---|
| `update_manifest_missing` | 正式更新包缺少发布清单 |
| `update_manifest_signature_missing` | 发布清单缺少签名 |
| `update_manifest_signature_invalid` | 发布清单签名无效 |
| `update_manifest_signature_untrusted` | 发布清单签名者不在信任列表 |
| `update_manifest_expired` | 发布清单签名证书已过期或尚不可用 |
| `update_manifest_invalid` | 发布清单格式或版本无效 |
| `update_manifest_file_missing` | 更新包缺少清单声明的文件 |
| `update_manifest_file_extra` | 更新包包含清单未声明的文件 |
| `update_manifest_hash_mismatch` | 更新包文件与发布清单不一致 |

错误信息不得包含本地绝对路径、凭据或完整签名内容。

## 6. Release workflow 加固

### 6.1 tag 与脚本注入

- tag 通过 `env: RELEASE_TAG` 传入 PowerShell，禁止把 `${{ github.ref_name }}` 直接拼进 `run`；
- 在任何下载、构建或 Secret 使用前，以正则 `^v\d+\.\d+\.\d+$` 校验 tag；
- 版本、文件名和 Release 查询全部使用已验证的纯版本值；
- `.github/workflows/*.yml` 继续保持纯 ASCII。

### 6.2 工具供应链

- Actions 固定到完整 40 位 commit SHA，并在注释中保留版本号；
- checkout 显式设置 `persist-credentials: false`；
- Inno Setup 固定官方版本、官方下载地址、SHA-256 和 Authenticode 发布者指纹；
- 下载后先校验摘要与发布者，再执行安装器；
- PSScriptAnalyzer、Rust、Node 和 npm 使用明确版本，不在 CI 中静默追踪浮动最新版；
- 正式 Cargo 门禁统一使用 `--locked`。

### 6.3 权限与 Secret 边界

- 普通 CI job 保持 `contents: read`；
- Release 构建和发布使用受保护的 `release` Environment；
- `contents: write` 只分配给最终发布 job，不分配给工具安装和构建 job；
- 签名 PFX 只进入构建 job；镜像 Token 只进入发布 job；
- 在仓库存在第二位维护者前，Environment 至少限制为 tag；具备第二位审核人后再启用 Required Reviewer，避免配置一个只有维护者本人能通过的形式化审批；
- 文档明确说明：拥有 tag 创建权限的单一维护者仍是最高风险来源，HSM/OIDC 是后续高价值增强。

### 6.4 构建与发布拆分

1. `validate`：无 Secret、只读权限，校验 tag、版本、tag 提交属于受保护 `main` 的可达历史以及工具摘要；
2. `build`：读取签名材料，运行完整门禁、构建、签名和 ZIP 清单验证，暂存 Release 资产；
3. `publish`：不读取 PFX，预检两个仓库写权限，下载已验证资产；
4. 两个仓库都先创建 Draft，上传并验证资产与签名；
5. 镜像更新仓先发布为 Latest，源码仓随后发布；跨 GitHub 仓库无法提供真正事务，因此称为“近原子发布”；
6. 任一验证失败时两个 Release 都保持 Draft，不把半成品暴露给用户；
7. 已发布 Release 不允许 workflow 自动 `--clobber`；补发必须走显式维护脚本并保留审计输出。

工作流为每个 tag 设置 `concurrency: release-${{ github.ref }}`，禁止同版本并发上传。

### 6.5 tag 门禁

Release 构建前在精确 tag 上运行：

```text
tools/lint.ps1
tools/test.ps1
tools/rust/gate.ps1
tools/ui/gate.ps1
```

任何门禁失败都不进入构建。CI 与 Release 仍可重复执行，但 Release 不依赖“维护者记得先看 CI”的口头约定。

## 7. 仓库与文档修复

### 7.1 凭据与卸载

README 和 SECURITY 明确区分：

1. **普通卸载**：删除程序、快捷方式和自启动项，保留状态、配置与凭据；
2. **退出渠道账号**：删除应用管理的凭据与上下文；
3. **手动彻底清理**：在退出账号后删除 `%LOCALAPPDATA%\AgentNotify`、旧 `.config` 目录和 OpenCode 插件文件，并检查 Windows 凭据管理器是否仍有 AgentNotify 条目。

卸载章节逐项列出 Codex、Antigravity、Devin、Command Code 接入的删除或还原行为。

### 7.2 安全文档

- `AGENT_NOTIFY_SIGNATURE_THUMBPRINT` 统一描述为“覆盖内置信任列表”，不再写成追加；
- `AGENT_NOTIFY_REQUIRE_SIGNATURE` 明确只属于 Go 1.x 遗留更新器；
- 签名文档删除“仓库私有”前提，改为 tag、协作者和 Secret 的威胁模型；
- 私密漏洞报告作为首选后备，补充独立安全邮箱；公开 Issue 不再作为漏洞报告渠道；
- 日志建议改为“只附必要行，并先移除用户名、路径、账号提示、客户端标识和消息标识”。

### 7.3 用户文档

README 补充：

- Windows 10/11 x64 要求；
- WebView2 前置条件；
- 自签名证书和 SmartScreen“未知发布者”的预期；
- `SHA256SUMS.txt` 校验命令与内置指纹核对方式；
- 2.0.0–2.0.4 不能直接应用内升级到 2.0.5 的历史限制；
- ZIP 回退需要手动重启的边界；
- ZIP 便携使用与卸载 runbook；
- 从克隆、安装依赖、生成桥接绑定到启动 Tauri 桌面端的开发 Quick Start。

### 7.4 社区入口

- Bug 模板改为 2.0 桌面端、`runtime.log` 和五个 Agent 独立日志；
- 功能模板同步支持五个 Agent；
- 模板要求用户先移除隐私数据，不承诺日志可无条件公开；
- PR 模板列出完整门禁，修正 `tools/test.ps1` 等于全部测试的错误表述；
- README 不再把用户回滚直接导向内部设计稿，改为稳定的 `TROUBLESHOOTING.md` 章节；
- `docs/superpowers/**` 明确标注为维护者归档材料。

## 8. 仓库卫生与 GitHub 设置

### 8.1 `.gitignore`

新增：

```gitignore
.env.*
!.env.example
*.pfx
*.pem
*.p12
*.cer
*.snk
*.key
```

不得忽略测试夹具所需的公开证书或非敏感 `.cer`；如未来确需提交，按明确路径反向放行。

### 8.2 转公开后立即执行

- 开启 Dependabot Alerts；
- 开启 Automated Security Fixes；
- 开启 CodeQL/Code Scanning；
- 验证 Secret Scanning 和 Push Protection；
- 开启 GitHub 私密漏洞报告；
- 为 `main` 启用 branch protection/ruleset，要求 CI 通过并禁止 force push；
- 为 `v*` 启用 tag ruleset，禁止删除和覆盖已发布版本标签；
- 开启合并后自动删除分支；
- 添加 `rust`、`tauri`、`notifications`、`agent` 等 topics；
- 确认 Community Profile 识别 LICENSE、README、CONTRIBUTING、SECURITY 和 Issue/PR 模板；
- 在二进制 Release 仓 README 中把“source is private”改为公开源码链接；
- 关闭二进制 Release 仓 Issues，并在 README 明确把问题反馈引导到源码仓，避免形成无人分诊的第二入口。

## 9. 历史重写与公开

### 9.1 重写范围

公开前重新执行完整离线演练，基线必须包含实施完成后的所有提交。处理范围包括：

- `main` 的全部历史；
- 全部已发布 `v*` 标签；
- 已存在但没有 Release 的 `v2.0.7` 标签在公开前先从远端删除，公开后再从最终重写历史重新创建并推送，以触发新 workflow；旧失败 run 不重跑，避免继续绑定重写前的提交，也避免私有仓账单阻塞时重复失败；
- 远端 `codex/rust-desktop-rewrite`，该分支已合并、无独有提交、无关联 PR，备份后删除；
- 已检查的 5 个 Dependabot PR 均早于敏感验收文档，不包含待脱敏值。

### 9.2 推送约束

- 禁止 `git push --mirror`，防止发布本地废弃 `codex/task*` 分支；
- 只允许显式强推 `main` 和已重写 tags；
- 显式删除远端 `codex/rust-desktop-rewrite`；
- 强推后重新 `git ls-remote` 枚举所有 heads 和 tags；
- 对所有远端可达 refs 重新扫描 3 个真实账号哈希和 5 个平台消息 ID，命中数必须为 0；
- 扫描公开前远端存在的全部历史 Release 说明，命中数必须为 0；
- 核对源码 Release 与二进制 Release 的资产名称和 SHA-256。

### 9.3 可见性变更

所有检查通过后才执行：

```text
gh repo edit srafyhucl-cpu/agent-notify --visibility public --accept-visibility-change-consequences
```

公开后立即执行第 8.2 节，验证 Actions 免费分钟已经解除账单阻塞，再从最终重写历史创建并推送 `v2.0.7` 标签触发发布；不得在公开前重建该标签。

## 10. 测试与验收

### 10.1 签名清单

必须覆盖：

- 合法清单通过；
- 清单改一字节后拒绝；
- 文件内容或大小变化后拒绝；
- 缺失文件、额外 DLL/脚本、重复路径、大小写冲突和路径穿越拒绝；
- 错误签名者、过期证书和无效 CMS 拒绝；
- Stable 中任一控制文件缺失拒绝；
- Beta 中两个控制文件同时缺失时保留旧开发包兼容；
- ZIP 安装只复制清单列出的文件；
- 替换中途失败仍完整回滚；
- 安装器启动失败后，签名 ZIP 回退仍成功。

### 10.2 Release

- tag 注入样本无法执行命令；
- 非 SemVer tag 在工具安装前失败；
- Inno Setup 摘要或发布者不符时不执行；
- 构建 job 没有 `contents: write` 或镜像 Token；
- 发布 job 没有 PFX；
- Draft 验证失败时两个 Release 均不公开；
- 镜像与源码资产 SHA-256 一致；
- 补发脚本不能绕过 ZIP 清单和五个可执行文件签名门禁。

### 10.3 仓库门禁

实施完成后执行：

```text
go test ./...
go vet ./...
gofmt -l cmd internal
node_modules\.bin\tsc.cmd --noEmit
tools\lint.ps1
tools\test.ps1
tools\rust\gate.ps1
tools\ui\gate.ps1
```

涉及 workflow、脚本和 UI 契约时再执行对应专项测试。正式发布前运行 `tools/check-version.ps1`，版本仍以根 `VERSION` 为唯一来源。

### 10.4 真实链路

1. 用临时安装目录执行一次“当前版本 → v2.0.7”ZIP 回退集成验收，不修改用户现有安装；
2. 验证回退成功后文件来自签名清单，篡改包不会触碰安装目录；
3. 验证安装器下载、签名校验、启动和进程退出路径；
4. 微信真实推送与引用回复继续沿用既有发布验收，本轮签名清单不改变消息协议；
5. B4 安装器覆盖升级由用户在自己的安装环境执行并反馈。

## 11. 回滚

- 历史重写前保留全量 `git bundle` 和远端 refs 清单；
- 所有加固先在专用分支提交，公开前不改变远端可见性；
- Release 失败时保留 Draft 和构建日志，不发布半成品；
- 签名清单发布失败时删除 Draft、保留 v2.0.6，不退回无清单的 v2.0.7；
- 公开后若发现新的敏感历史，暂停发布，重新离线重写并复验所有 refs；
- 二进制更新仓已有 v2.0.6 可用，源码仓公开失败不会破坏现有客户端更新。

## 12. 完成标准

只有同时满足以下条件，方案 B 才算完成：

1. 所有 P0 已修复；
2. Stable 客户端无法安装无签名清单或清单不匹配的 ZIP；
3. Release workflow 不再直接拼接 tag 到 PowerShell，不执行未校验工具；
4. 公开仓所有远端可达 refs 的真实标识扫描为 0；
5. 仓库已安全转为 public，安全治理设置已启用；
6. v2.0.7 在源码仓和二进制仓均有安装器、ZIP、`SHA256SUMS.txt`，摘要一致；
7. v2.0.7 Release 资产签名、ZIP 清单和五个可执行文件签名全部通过门禁；
8. 全量门禁全绿，且没有覆盖并行 UI 会话改动。
