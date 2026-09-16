# 代码签名接入清单

产物的 Authenticode 签名是自动更新链路唯一的"防篡改 + 防冒充"锚点：
`SHA256SUMS.txt` 与安装包同源下载，能同时被篡改；只有签名能把伪造者挡在外面。
当前仓库**未配置**代码签名（`gh secret list` 只有 `RELEASE_REPO_TOKEN`），因此发布产物是
`NotSigned`，客户端只对"有签名但校验失败"的包做拒绝。按下面步骤接入即可补齐。

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

配置后构建会强制校验签名：主程序或安装器签名状态不是 `Valid` 就直接失败，
不会出现"配了签名却发出未签名包"的情况。

## 3. 验证

```powershell
# 本地：对已下载的产物检查
Get-AuthenticodeSignature -LiteralPath .\Agent-notify-Setup-vX.Y.Z.exe | Format-List Status, SignerCertificate
Get-AuthenticodeSignature -LiteralPath .\Agent-notify-vX.Y.Z.zip  # ZIP 本身不签名，校验解压后的 exe

# 客户端视角：强制要求签名（先在 CI 配好，再用此环境变量自测）
$env:AGENT_NOTIFY_REQUIRE_SIGNATURE = '1'
# 悬浮窗"升级"会下载并校验；未签名/无效会直接报错并给出原因
```

## 4. 让所有客户端只信任你的证书（可选但推荐）

配置签名后，攻击者若拿到**另一张**有效证书仍可署名。把签发证书的 SHA1 指纹写进
`internal/update/signature.go` 的 `defaultSignatureThumbprint`，所有客户端就只接受该签名者：

```powershell
(Get-AuthenticodeSignature .\Agent-notify-Setup-vX.Y.Z.exe).SignerCertificate.Thumbprint
```

把输出（去掉空格）填进常量即可；留空表示不限定签名者。
客户端也可用环境变量 `AGENT_NOTIFY_SIGNATURE_THUMBPRINT` 临时限定（多个用逗号/分号分隔），
优先级高于常量。

## 5. 轮换与过期

- 证书换签后，务必先更新 `defaultSignatureThumbprint`（若已启用）再发版，否则老客户端会拒绝新版本；
- 建议同时配置时间戳（`/tr`），证书过期后既有产物的签名仍然有效；
- 证书私钥泄露时立即吊销，并在下一版移除旧指纹。
