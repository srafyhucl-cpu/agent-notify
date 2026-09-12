# 安全策略

## 支持的版本

仅最新 Release 接收安全修复。请先升级到最新版本再报告问题。

## 报告漏洞

请不要用公开 Issue 报告安全问题。

首选在 GitHub 仓库的 **Security** 页面选择 **Report a vulnerability**。如果无法使用私密报告，可先提交一个不含复现细节的 Issue，说明需要私下联系渠道。

## 凭据与数据

- ClawBot token、bot id 和 recipient user id 只保存到 `%USERPROFILE%\.config\agent-notify\clawbot.json`。
- 凭据文件创建时使用当前用户可读写权限。
- `status`、悬浮窗和日志只显示脱敏后的用户标识，不输出 token。
- DryRun 只输出渲染后的标题和消息，不读取或打印凭据。
- 推送历史和调试日志不包含 ClawBot token。

## 网络与发送

- 默认使用 HTTPS 连接 `https://ilinkai.weixin.qq.com`。
- 状态探测使用 HEAD 请求，不发送业务数据。
- 发送失败按可重试类别进行有限退避，不无限循环。
- HTTP 非 200 或业务返回 `ret != 0` 会写入历史错误并返回失败状态。

## 本地文件安全

- 配置、凭据和历史写入前会创建目标目录。
- 配置和凭据使用临时文件替换，降低写入中断导致的损坏风险。
- 安装器只处理安装目录和记录文件，不读取或执行安装包内的任意代码。
- 卸载器只删除 `agent-notify-install.json` 中记录的文件、固定插件与 Agent-notify 快捷方式。
- Codex 配置只在 notify 行缺失或直指 `codex-computer-use.exe` 时修改，修改前创建备份；自定义 notify 程序不会被覆盖。

## Agent Hook 隔离

- OpenCode 插件和 Codex hook 调用失败不得阻塞 agent。
- Codex 先透传上游 `codex-computer-use.exe`，再执行通知发送。
- 若上游透传失败，通知层不得吞掉或改写上游结果。
- 开关关闭只影响 Agent-notify 推送，不改变 agent 自身行为。

## 依赖与构建

- Go 模块依赖由 `go.mod` 和 `go.sum` 固定。
- npm 只用于 TypeScript 类型检查，不进入发布运行时。
- GitHub Actions 使用官方 actions，并在 Windows 上运行测试与 smoke。
- Release 包附带 `SHA256SUMS.txt`，发布前由 CI 构建。

## 范围之外

- ClawBot 服务本身及微信账号的安全。
- OpenCode、Codex 与 `codex-computer-use.exe` 自身的漏洞。
- 用户自行修改安装脚本、配置文件或运行环境后造成的问题。
