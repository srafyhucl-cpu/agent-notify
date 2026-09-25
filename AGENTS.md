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
- 版本号唯一来源是仓库根 `VERSION`（2.0.0 起；此前为 `internal/app/version.go`，Go 版 UI 已不再作为发布入口）。发版前先跑 `tools\sync-version.ps1` 把版本同步到 `hosts/desktop-tauri/tauri.conf.json`、`Cargo.toml` 的 `[workspace.package]` 与 Devin 扩展，再更新 README 徽章与 `CHANGELOG.md`，按 SemVer 递增，打 tag 后推送；Release workflow 只在验证和构建成功后调用受保护 `main` 上的可复用发布 workflow。
- 客户端更新源是二进制仓库 `srafyhucl-cpu/agent-notify-releases`，源码 Release 由同一可复用 workflow 创建；发布阶段只接收已构建资产，不读取 PFX，也不执行 tag 中的脚本。`RELEASE_REPO_TOKEN` 缺失时发布直接失败；已发布 Release 不允许 workflow 自动覆盖，补发必须显式运行 `tools\publish-release.ps1 -Version x.y.z -DistDir <产物目录>` 并保留审计输出。
- 2.0 Rust 客户端的签名信任锚只有 `hosts/desktop-tauri/src/update/verify.rs` 的 `DEFAULT_SIGNATURE_THUMBPRINT`；旧 `internal/update/signature.go` 只作为兼容读取路径。正式 ZIP 必须包含由同一发布证书签名的 `RELEASE-MANIFEST.json` 与 `RELEASE-MANIFEST.p7s`，清单覆盖 ZIP 内全部普通文件；缺清单、签名无效、文件集合或哈希不一致时，Stable 更新器在替换文件前拒绝，Beta 仅在两个控制文件同时缺失时兼容旧开发包。
- 发布必须签名：构建 job 强制要求 `AGENT_NOTIFY_SIGN_PFX_BASE64` 和密码 secret，`tools\build-release.ps1`、`tools\signature-common.ps1` 与 `tools\publish-release.ps1` 会校验安装器、五个 ZIP 内程序和签名清单的指纹；未签名或指纹不符直接失败。轮换证书前必须先更新 Rust 内置指纹并随新版本发布。门禁自身由 `tests\signature-gate.tests.ps1` 回归（`tools\test.ps1` 会执行）。
- `.github/workflows/*.yml` 必须保持纯 ASCII：Actions 会把 `run` 脚本写成无 BOM 临时文件，PS 5.1 按 ANSI 读取，中文会破坏引号导致步骤语法错误（`tools\lint.ps1` 已加校验）。官方 Actions 固定完整 commit SHA，checkout 关闭凭据持久化；Release 的 validate/build/publish 权限和 Secret 边界不能混合。
- 版本一致性由 `tools\check-version.ps1` 校验（`VERSION`、Tauri 配置、`Cargo.toml` 的 workspace 版本、README 徽章、Devin 扩展、CHANGELOG 段落、安装包名规则）；`tools\lint.ps1`、CI 与 Release validate 都会调用它。
- 正式包由 Rust 桌面端构建：构建机需要 `D:\Tools\cargo` + `D:\Tools\rustup`（约定见 `tools\rust\gate.ps1`），本地发布门禁还需要 Inno Setup 6 与签名工具（`AGENT_NOTIFY_ISCC`、`AGENT_NOTIFY_SIGNTOOL`）。
- CI 静态检查包含 `govulncheck`；项目最低 Go 版本以 `go.mod` 的 `go` 行为准（当前 1.26.8）。
- 提交前确认工作区里没有别人未完成的改动（同一仓库可能有并行 agent 在工作），只提交本次相关文件。
