# 安全策略

## 支持的版本

仅最新 Release 接收安全修复。请先升级到最新版本再报告问题。

## 报告漏洞

请不要用公开 Issue、公开 PR 或讨论区报告安全问题。

仓库转为公开后，唯一私密入口是 GitHub 仓库的 **Security → Report a vulnerability**。请在私密报告中描述影响版本、复现步骤、可能影响和建议修复；不要先在公开渠道透露漏洞细节。仓库公开前尚未承诺外部漏洞受理入口。

## 凭据与数据

- ClawBot token、bot id、recipient user id、`context_token` 等凭据保存在 Windows 凭据管理器（目标名以 `AgentNotify/` 开头，后接不可逆账号摘要），不落明文文件；在 Channels 页点「退出账号」会删除该账号由应用管理的凭据与会话上下文。
- 旧版 `%USERPROFILE%\.config\agent-notify\clawbot.json` 只在首次启动时只读导入；旧文件不改写、不删除。
- 状态数据（开关、推送历史、引用路由、入站 Claim）保存在 `%LOCALAPPDATA%\AgentNotify\data\state.db`（SQLite WAL）。
- 界面、状态快照与日志只显示脱敏后的用户标识和元数据，不输出 token 或 context token。
- 运行日志逐行脱敏后才落盘（`%LOCALAPPDATA%\AgentNotify\logs\runtime.log`）；从旧版导入的历史错误文本同样会把凭据明文替换为 `[REDACTED]`。
- 切换 bot 账号时会清空旧账号的会话上下文和游标，防止状态串用。

## 网络与发送

- 默认使用 HTTPS 连接 `https://ilinkai.weixin.qq.com`。
- 状态探测使用 HEAD 请求，不发送业务数据。
- 发送失败按可重试类别进行有限退避，不无限循环。
- HTTP 非 2xx 或业务返回 `ret/errcode` 非零会写入历史错误并返回失败状态。
- `ret=-14` 或 `errcode=-14` 会停止轮询并清除上下文，等待用户重新登录。

## 本地文件安全

- 旧数据迁移是只读的：`%USERPROFILE%\.config\agent-notify` 下的旧配置与凭据不会被改写或删除。
- 离线事件写入 `%LOCALAPPDATA%\AgentNotify\spool`，核心恢复后补投；超限或损坏的事件进隔离目录并记录原因，不静默丢弃。
- 安装器只复制声明的文件并调用固定接入脚本；普通卸载会删除程序、快捷方式和 AgentNotify 自己写入的 Codex / Antigravity / Devin / Command Code 接入，但保留 OpenCode 插件、SQLite、旧配置与 Windows 凭据。手动彻底清理的顺序见 README。
- Codex 配置只改写 notify 链内指向本产品的路径，修改前创建备份；自定义 notify 程序不会被覆盖。

## Agent 接入隔离

- OpenCode 插件、Codex / Antigravity / Devin Hook 与 Command Code mod 调用失败不得阻塞各自 agent，失败原因只写各自的调试日志。
- 事件入口 `agentnotify-ingress.exe` 只接受版本化事件协议（有体积上限），命名管道 ACL 只允许当前用户（`D:P(A;;GA;;;<SID>)`）。
- Codex 先透传上游 `codex-computer-use.exe`，再执行通知发送；上游透传失败时通知层不得吞掉或改写上游结果。
- 开关关闭只影响 AgentNotify 推送，不改变 agent 自身行为。

## 更新与安装包

- 应用内安装器路径只接受带 Authenticode 签名、且签名指纹命中内置信任列表的安装包；`AGENT_NOTIFY_SIGNATURE_THUMBPRINT` 会**覆盖**内置列表而不是追加，未签名或指纹不符一律拒绝。
- 下载产物先校验 SHA256、签名与 PE 版本再替换文件；ZIP 回退还要通过下述签名清单；校验失败不触碰已安装文件，当前版本继续可用。
- ZIP 回退会清洗重复/越界条目、拒绝符号链接与特殊文件，并先验证 `RELEASE-MANIFEST.json` 与
  `RELEASE-MANIFEST.p7s` 的 CMS 签名、证书有效期、信任指纹、完整文件集合和逐文件 SHA-256；随后只复制
  清单声明的安装文件。`SHA256SUMS.txt` 只验证外层下载完整性，不能替代 ZIP 内清单。Stable 缺少任一控制
  文件或清单内容不一致都会在替换前拒绝；Beta 仅在两个控制文件同时缺失时兼容旧开发 ZIP。
- `AGENT_NOTIFY_REQUIRE_SIGNATURE` 只被 Go 1.x 更新器读取；2.0 更新器固定要求签名。

## 依赖与构建

- Rust 依赖由 `Cargo.lock` 固定；Go 模块依赖由 `go.mod` 与 `go.sum` 固定（Go 侧只保留回滚窗口）。
- 桌面 UI 的 npm 依赖会构建进正式安装包；插件与扩展的 npm 依赖只用于类型检查与测试，不进入发布运行时。
- GitHub Actions 固定到完整 commit SHA，checkout 不持久化凭据；CI 静态检查包含 `govulncheck` 与 CodeQL。
- Release workflow 分为无 Secret 的 validate、带 PFX 的 build 和受保护 `main` 上的 publish；源码与客户端
  更新仓均先创建 Draft、上传后复核名称/摘要/签名，镜像先发布为 Latest。已发布 Release 不会被 workflow
  自动覆盖，补发必须显式审计。

## 范围之外

- ClawBot 服务本身及微信账号的安全。
- OpenCode、Codex 与 `codex-computer-use.exe` 自身的漏洞。
- 用户自行修改安装脚本、配置文件或运行环境后造成的问题。
