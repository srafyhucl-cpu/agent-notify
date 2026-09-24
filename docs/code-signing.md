# 代码签名接入清单

安装器与桌面主程序的 Authenticode 签名是自动更新链路的“防篡改 + 防冒充”锚点：
`SHA256SUMS.txt` 与安装包同源下载，能同时被篡改；只有签名能把伪造者挡在外面。ZIP 内的插件、Hook 和
脚本还需要独立的签名文件清单，清单实现前不能把外层 ZIP 哈希当作全部内容的签名证明。
本仓库**已启用**代码签名（自签名证书 + 指纹锁定）：`gh secret list` 包含
`AGENT_NOTIFY_SIGN_PFX_BASE64` / `AGENT_NOTIFY_SIGN_PFX_PASSWORD`，产物以 `UnknownError`
状态签名，指纹为 `EDF9E283DF2407B318E65D59BB430FD546509ACD`。构建脚本与发布门禁会在发布前强制校验
"五个可执行文件与安装器都已签名且指纹等于内置常量"（见 `tools/signature-common.ps1` 与
`tools/release-gate.ps1`），未签名或指纹不符直接失败。

## 1. 准备证书

任选一种，能拿到 **Windows Authenticode 代码签名**能力即可：

| 方式 | 说明 |
|---|---|
| OV/EV 代码签名证书 | 向 CA（DigiCert、Sectigo、GlobalSign 等）购买，导出 PFX 或存储到证书存储 |
| 云签名服务 | Azure Trusted Signing、SignPath 等，提供本地 CLI 包装器 |
| 硬件令牌/HSM | EV 证书常见；需要厂商提供的 `signtool` 兼容包装命令 |

## 2. 在 CI 配置 `AGENT_NOTIFY_SIGNTOOL`

该 secret 的值是**一条可执行命令的路径或命令名**，构建脚本会以 `<tool> sign <file>` 的形式调用它
（见 `tools/build-release.ps1` 与 `tools/build-installer.ps1`）：

1. 若用原始 `signtool.exe`，建议封装成一个脚本（例如 `sign.cmd`），内部执行
   `signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 %1`；
2. 把该包装器与证书/凭据一起放到 CI 可访问的位置（上传为 artifact、或用云签名 CLI 的安装步骤）；
3. 在源码仓库 Settings → Secrets and variables → Actions 新建 secret，名称必须是
   `AGENT_NOTIFY_SIGNTOOL`，值为包装器路径或命令名。

配置后构建会强制校验签名：五个可执行文件（`agentnotify-desktop.exe`、`agentnotify-ingress.exe`、
`agentnotify-codex-hook.exe`、`agentnotify-antigravity-hook.exe`、`agentnotify-devin-hook.exe`）
与安装器必须已签名，且签名者指纹必须等于客户端内置的 `defaultSignatureThumbprint`，否则直接失败。
补发路径 `tools\publish-release.ps1`（编排在 `tools\release-gate.ps1`）会在上传前再校验一次：
安装器签名、`SHA256SUMS.txt` 覆盖安装器与 ZIP 且哈希一致，以及 ZIP 内这五个程序的签名指纹；
任一项不符都拒绝补发。这样既能挡住"secret 丢失导致静默发出未签名包"，
也能挡住"换证书只改了 secret、忘了同步内置指纹"（后者会让客户端报"签名者不匹配"而无法
自动更新自救）。Release workflow 另有一道前置步骤强制要求 `AGENT_NOTIFY_SIGN_PFX_BASE64` 存在，
secret 被删除或改名时会在构建前直接失败，而不是发出未签名包。

## 2.5 零成本方案：自签名证书 + 指纹锁定

没有付费证书时，用自签名证书同样能拿到"防篡改"这一核心能力——客户端把**证书指纹**当作信任锚，
不依赖 Windows 受信链（自签名产物的状态是 `UnknownError`/`NotTrusted`，指纹匹配即放行；
被篡改的包会变成 `HashMismatch`，仍然拒绝）。

1. 本地生成证书并导出 PFX（有效期自定）：

   ```powershell
   $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=AgentNotify" `
     -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(5)
   Export-PfxCertificate -Cert $cert -FilePath agent-notify.pfx `
     -Password (Read-Host -AsSecureString "PFX 密码")
   Get-ChildItem Cert:\CurrentUser\My\$($cert.Thumbprint) | Select-Object Thumbprint   # 记下指纹
   ```

2. 两个 secret：`AGENT_NOTIFY_SIGN_PFX_BASE64`（PFX 的 base64 文本）与 `AGENT_NOTIFY_SIGN_PFX_PASSWORD`；
3. `AGENT_NOTIFY_SIGNTOOL` 指向仓库自带的垫片脚本 `tools\sign-selfsigned.cmd`
   （它转发到 `sign-selfsigned.ps1`，按 `<tool> sign <file>` 约定签名，并在签名后立刻从证书存储清理；
   Inno Setup 无法直接执行 .ps1，所以必须用 .cmd）；
4. 把指纹（去掉空格）同时填进两处常量，保证发布门禁与客户端同一信任锚：
   - Rust 客户端：`hosts/desktop-tauri/src/update/verify.rs` 的 `DEFAULT_SIGNATURE_THUMBPRINT`；
   - 发布门禁来源：`internal/update/signature.go` 的 `defaultSignatureThumbprint`
     （`tools\signature-common.ps1` 固定从这个文件读取期望指纹）。
   两处一致后发一版，之后所有客户端都会只信任这张证书；
   - 本仓库当前已内置指纹：`EDF9E283DF2407B318E65D59BB430FD546509ACD`（2026-09-16 启用，2031-09-16 到期）；
   - CI 侧凭据在 GitHub secrets（`AGENT_NOTIFY_SIGN_PFX_BASE64` / `AGENT_NOTIFY_SIGN_PFX_PASSWORD`），
     Release workflow 检测到 PFX secret 后会自动把 `AGENT_NOTIFY_SIGNTOOL` 指向垫片脚本；
5. 局限与注意：
   - 手动运行安装器时仍会提示"未知发布者"（要消除该提示必须用 CA 签发的证书）；
   - 证书轮换/过期前，**先**更新 `defaultSignatureThumbprint` 并发版，否则老客户端会拒绝新版本；
   - 私钥泄露时立即停止信任旧指纹并轮换证书；自签名证书无法依赖公共 CRL，客户端安全边界是内置指纹而不是系统信任链。

本地联调：

```powershell
# 用包装器签名（需要上面两个环境变量）
powershell -NoProfile -ExecutionPolicy Bypass -File tools\sign-selfsigned.ps1 sign .\signed.exe
# 客户端视角：限定指纹后验证升级
$env:AGENT_NOTIFY_SIGNATURE_THUMBPRINT = '<证书指纹>'
```

> 注意：在 PowerShell 里写 secret 请用文件重定向（`cmd /c "gh secret set NAME < file"`）。
> 用管道写入会带上尾随换行，PFX 会因"密码不正确"导入失败；包装器已对换行做兜底，但仍建议用重定向。

## 3. 验证

```powershell
# 本地：对已下载的产物检查
Get-AuthenticodeSignature -LiteralPath .\Agent-notify-Setup-vX.Y.Z.exe | Format-List Status, SignerCertificate
Get-AuthenticodeSignature -LiteralPath .\Agent-notify-vX.Y.Z.zip  # ZIP 本身不签名，校验解压后的五个 exe

# 发布门禁：校验安装器 + ZIP 内五个程序 + SHA256SUMS.txt 覆盖与哈希（只读，不上传）
. .\tools\signature-common.ps1
. .\tools\release-gate.ps1
$expected = Get-ExpectedSignatureThumbprint -RepoRoot .
Assert-SumsCoversArtifact -SumsPath .\dist\SHA256SUMS.txt -ArtifactPath .\dist\Agent-notify-Setup-vX.Y.Z.exe
Assert-ArchiveExecutables -ZipPath .\dist\Agent-notify-vX.Y.Z.zip -ExpectedThumbprint $expected

# 2.0 桌面端固定要求签名，没有关闭开关；
# 如需在开发机临时限定另一张证书，使用 AGENT_NOTIFY_SIGNATURE_THUMBPRINT 覆盖内置信任列表。
# 桌面端 Settings → 更新 会下载并校验；未签名、无效或指纹不符会直接报错。
```

## 4. 让所有客户端只信任你的证书（可选但推荐）

配置签名后，攻击者若拿到**另一张**有效证书仍可署名。把签发证书的 SHA1 指纹写进两处常量
（Rust 客户端的 `hosts/desktop-tauri/src/update/verify.rs` 与发布门禁读取的
`internal/update/signature.go`，两处必须一致），所有客户端就只接受该签名者：

```powershell
(Get-AuthenticodeSignature .\Agent-notify-Setup-vX.Y.Z.exe).SignerCertificate.Thumbprint
```

把输出（去掉空格）填进常量即可；留空表示不限定签名者。
客户端也可用环境变量 `AGENT_NOTIFY_SIGNATURE_THUMBPRINT` 临时限定（多个用逗号/分号分隔），
优先级高于常量——注意它是**覆盖**而不是追加：如果按早期文档在本机设过别的指纹，新版客户端
会把官方包判为"签名者不匹配"，需要删掉这个环境变量或改成与内置常量一致。

## 5. 轮换与过期

- 证书换签后，务必先更新两处 `defaultSignatureThumbprint` / `DEFAULT_SIGNATURE_THUMBPRINT`（若已启用）
  再发版，否则老客户端会拒绝新版本；构建脚本与补发门禁的指纹校验会在两者不一致时直接失败，
  所以顺序必须是：**先改常量并与新版本一起发布，再轮换 secret 里的证书**；
- 建议同时配置时间戳（`/tr`），证书过期后既有产物的签名仍然有效；
- 证书私钥泄露时立即从发布流程移除旧证书，并在下一版停止信任旧指纹；自签名证书本身不依赖公共吊销服务。

## 6. 公开发布后的 Secret 信任边界

源码公开不代表 Secret 会公开，但**任何能修改发布 workflow 或创建发布 tag 的协作者，都可能让代码在
CI 中读取仓库级 Secret**。因此公开发布不能只依赖“只有一位维护者”这一假设。

发布链必须遵守以下边界：

1. `main` 与 `v*` tag 使用 branch/tag ruleset 保护，禁止协作者绕过评审修改发布入口；
2. 使用 `release` Environment 承载签名与镜像权限；具备第二位维护者后再配置 Required Reviewer，
   不能把只有本人能批准的形式化审批当作独立制衡；
3. 签名 PFX 只进入构建 job，`RELEASE_REPO_TOKEN` 只进入受保护 `main` 上调用的发布 workflow；
4. 发布 workflow 不执行 tag 中可修改的仓库脚本，只接收已构建并通过门禁的资产；
5. 工具下载、Actions 和构建依赖固定版本或不可变摘要，避免 Secret 注入前先执行未校验代码；
6. 维护者账号启用 2FA，定期审查 tag、workflow 运行和 Release 资产变更。

**安全边界一句话**：能控制受保护发布入口或读取 Environment Secret 的人，实际上拥有对应签名/发布能力；
单一维护者阶段还必须把账号安全作为根信任。
