# 安全策略

## 支持的版本

仅**最新 Release** 接收安全修复；更早版本请先升级。

## 报告漏洞

**请不要用公开 Issue 报告安全问题。**

首选：仓库 **Security** 标签页 → **Report a vulnerability**（GitHub 私密漏洞报告）。
备用：开一个不含细节的 Issue，说明需要私下联系渠道。

我们会尽快确认并给出处理计划；修复发布后可在 Release Notes 中致谢（如你愿意）。

## 安全设计说明

- `PUSHPLUS_TOKEN` 只从进程环境变量读取：不写入仓库、不落盘、不打印
  （`install.ps1` 只检查存在性；DryRun 输出对 token 打码）
- 推送链路任何失败都静默 `exit 0`，不会阻塞或拖垮 agent 运行
- `codex-notify.ps1` 只改写 `config.toml` 中指向 **上游 `codex-computer-use.exe`**
  的 notify 行，改写前备份；看守任务只恢复该行，不碰其它配置
- 计划任务以当前用户身份运行（注册任务本身需要管理员权限）
- 全部脚本运行在 Windows PowerShell 5.1，未使用 `Invoke-Expression` 执行外部内容
- 仓库守卫测试会阻止无 BOM / 非 CRLF 的 PowerShell 文件混入（编码错误曾导致真实故障）

## 范围之外

- PushPlus 服务与微信绑定本身的安全（见 pushplus.plus 官方说明）
- 第三方 agent（opencode / codex）自身的漏洞
