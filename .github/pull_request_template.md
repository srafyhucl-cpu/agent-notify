## 变更说明

<!-- 一句话说明这个 PR 做什么、为什么 -->

## 类型

- [ ] feat 新功能
- [ ] fix 修复
- [ ] docs 文档
- [ ] refactor 重构（行为不变）
- [ ] test 测试
- [ ] chore / ci 工程

## 检查清单

- [ ] `tools\lint.ps1` 全绿
- [ ] `tools\test.ps1` 全绿
- [ ] 改动 `plugin/` 时 `npx tsc --noEmit` 全绿
- [ ] 新增/修改 ps1/psm1/psd1 为 UTF-8 BOM + CRLF
- [ ] 行为有变化时已更新 `CHANGELOG.md`
- [ ] 未破坏外部契约（入口名 / 环境变量 / marker 路径 / 静默 exit 0，见 CONTRIBUTING）

## 关联 Issue

Closes #
