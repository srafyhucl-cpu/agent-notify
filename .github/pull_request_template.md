## 变更说明

<!-- 一句话说明这个 PR 做什么、为什么；行为变化请明确用户影响 -->

## 类型

- [ ] feat 新功能
- [ ] fix 修复
- [ ] docs 文档
- [ ] refactor 重构
- [ ] test 测试
- [ ] chore / ci 工程

## 检查清单

- [ ] `go test ./...`、`go vet ./...` 与 `gofmt -l cmd internal` 全绿
- [ ] `node_modules\.bin\tsc.cmd --noEmit` 全绿
- [ ] `tools\rust\gate.ps1` 全绿
- [ ] `tools\ui\gate.ps1` 全绿
- [ ] `tools\lint.ps1` 全绿
- [ ] `tools\test.ps1` 全绿
- [ ] 修改 PowerShell 脚本时保留 UTF-8 BOM + CRLF
- [ ] `.github/workflows/*.yml` 保持纯 ASCII，Actions 固定到完整 commit SHA
- [ ] 未提交 token、证书私钥、真实账号标识、平台消息 ID 或未经脱敏的个人日志
- [ ] 行为有变化时已更新 `CHANGELOG.md` 和相关用户文档
- [ ] 未破坏 `AGENT_NOTIFY_*`、marker、签名信任锚与 ClawBot 会话契约

## 验证范围

<!-- 说明人工或真实链路验收；无法执行的项目请明确原因和风险 -->

## 关联 Issue

Closes #
