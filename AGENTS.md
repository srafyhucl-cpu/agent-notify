# Agent-notify 开发约定

## 沟通
- 收到任务先理解、想清楚再动手；没把握时每次只问一个问题，确认后再开始，不擅自扩大范围。
- 需求不清楚就停下来问，不靠猜。

## 编码
- 代码干净、职责单一：不堆上帝类，不把逻辑全塞进一个大文件。
- 不用魔法数字，写成命名常量或配置。
- 只改和需求相关的代码，不顺手重构、不格式化无关文件。
- 失败要明确暴露，不做猜测式兜底（宁可报错也不猜目标）；错误提示要用户在微信里看得懂。
- 注释和文档用中文，只讲关键点，不写废话。
- 向后兼容：旧配置、旧数据、旧调用保持可用；新功能默认保守，先默认关闭再经真实验收。

## 目录与临时文件
- 不要把下载物、依赖缓存、构建产物放到 C 盘；优先 `D:\Temp` 或项目内目录（`install.ps1` 等脚本已按此约定）。
- 临时文件随用随清，结束前检查自己创建的东西；删除只针对明确路径，不批量删、不递归删目录。

## 验证
- 改完先过门禁：`go test ./...`、`go vet ./...`、`gofmt -l cmd internal`、`node_modules\.bin\tsc.cmd --noEmit`；涉及脚本或插件时加跑 `tools\test.ps1`、`tools\lint.ps1`。
- 真实功能必须在真实链路上验收（微信收到消息、引用回复进入正确线程），不能只看单测。

## 提交与发版
- 提交信息用 Conventional Commits + 中文描述，例如 `feat(reply): ...`、`docs: ...`、`fix(ui): ...`。
- 版本号只认 `internal/app/version.go`；发版时同步 `VERSION`、`agent-notify.manifest`、README 徽章与包名、`CHANGELOG.md`，按 SemVer 递增，打 tag 后推送，Release workflow 会自动发布产物。
- Release workflow 只把产物发到源码仓库；客户端更新源是二进制仓库 `srafyhucl-cpu/agent-notify-releases`。workflow 末尾会用 secret `RELEASE_REPO_TOKEN` 自动把安装器、ZIP 与 `SHA256SUMS.txt` 镜像过去并置为 Latest；该 secret 缺失时镜像步骤直接失败提醒（否则用户点"升级"会误报"当前已是最新版本"）。手动补发用 `tools\publish-release.ps1 -Version x.y.z -DistDir <产物目录>`。
- 发布必须签名：Release workflow 强制要求 secret `AGENT_NOTIFY_SIGN_PFX_BASE64` 存在，构建脚本再用 `tools\signature-common.ps1` 校验产物签名者指纹等于 `internal/update/signature.go` 的 `defaultSignatureThumbprint`；未签名或指纹不符直接失败（避免发出客户端拒绝的包）。轮换证书前必须先更新内置指纹并随新版本发布。
- `.github/workflows/*.yml` 必须保持纯 ASCII：Actions 会把 `run` 脚本写成无 BOM 临时文件，PS 5.1 按 ANSI 读取，中文会破坏引号导致步骤语法错误（`tools\lint.ps1` 已加校验）。
- 版本一致性由 `tools\check-version.ps1` 校验（`VERSION`、manifest、README 徽章、`BotAgent`、Devin 扩展、CHANGELOG 段落）；`tools\lint.ps1` 与 Release workflow 都会调用它。Release workflow 的发布步骤已幂等，重跑或用新提交重指 tag 都不会因 Release 已存在而失败。
- CI 静态检查包含 `govulncheck`；项目最低 Go 版本以 `go.mod` 的 `go` 行为准（当前 1.26.8）。
- 提交前确认工作区里没有别人未完成的改动（同一仓库可能有并行 agent 在工作），只提交本次相关文件。
