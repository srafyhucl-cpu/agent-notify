# 发布前预检与运行时健康（暂停点证据）

- 日期：2026-10-01
- 版本：**v2.1.0**；`main` = `71d7267`（`HEAD == origin/main`）
- 目的：在「正式打 tag」这一唯一剩余人工动作之前，把能自动检查的发布路径与运行时健康全部钉死，降低用户 push tag 时的失败风险。

## 1. 发布链接口预检（防 startup_failure，§8 血泪教训）
- `release.yml` 的 `publish` job 调用 `publish-release.yml@main`：
  - 传 `inputs`：`release_tag` / `source_repository` / `artifact_name`（与 callee 声明逐一对应）。
  - 传 `secrets`：仅 `RELEASE_REPO_TOKEN`（callee 声明的唯一 secret，`required: true`）。
- **调用方未映射 callee 未声明的 secret，callee 也未要求调用方未提供的 secret** → 不会重现 2026-09-26 v2.0.7 的 startup 阶段非法。
- `publish-release.yml` 对 `release_tag` / `source_repository` / `artifact_name` 做格式校验，并对 `RELEASE_REPO_TOKEN` 缺失直接失败（不绕过）。

## 2. tag 可达性预检（validate 阶段会查）
- tag 将指向 `main` 的提交（`71d7267`）。release validate 的判定：`gh api repos/<repo>/compare/main...<sha>` 的 `status` 需为 `ahead|behind|identical`。
- 现 `HEAD == origin/main == 71d7267` → tag 指向该提交时 `identical`，**可达性成立**。
- 版本一致性：`tools\check-version.ps1 -Version 2.1.0` → `[version] 2.1.0 在所有发布位置一致`。

## 3. 产物完整性（dry-run 产物，`run 36743632596`）
- `dist\Agent-notify-Setup-v2.1.0.exe` + `dist\Agent-notify-v2.1.0.zip` + `SHA256SUMS.txt` + `release-notes.md`。
- 源 ZIP 内含 **5 个已签名程序**：`agentnotify-desktop.exe`、`agentnotify-ingress.exe`、`agentnotify-codex-hook.exe`、`agentnotify-antigravity-hook.exe`、`agentnotify-devin-hook.exe`。
  - 澄清计划 §5-4 的「五个 ZIP」：实指**一个源 ZIP 内的五个程序**（AGENTS.md 原文「五个 ZIP 内程序」）；dry-run 的 `dist` 就是源码 ZIP + 安装器，客户端更新仓的多包由发布阶段处理。
- `RELEASE-MANIFEST.json`：`product=AgentNotify / version=2.1.0 / files=24`，**覆盖 ZIP 内全部 24 个普通文件**；`RELEASE-MANIFEST.p7s`（1668 B）随包。`build-release.ps1` 已断言清单与程序签名指纹。

## 4. 运行时健康（本机已安装 2.1.0）
- `runtime.log`（`%LOCALAPPDATA%\AgentNotify\logs`）显示 2.1.0 宿主多次干净启动：`host_version=2.1.0 ... 启动桌面运行时` → `旧数据迁移检查完成 migration_state=Partial notifications=374 deliveries=374 routes=250 claims=52 skipped=52` → `桌面生产运行时及宿主服务初始化完成`。
- 生产日志中可见**看门狗/观察者真实工作**：
  - `看门狗回注漏掉的 Agent 汇报，任务自动推进` / `看门狗回注了漏掉的 Agent 汇报 recovered=1`；
  - `Agent 汇报已回注，任务自动推进`；
  - ClawBot 长轮询暂时失败按预期降级为 WARN（不阻塞任务）。
- 说明：这是**只读日志取证**，不等于 §3.4 真机矩阵（后者需用户驱动 UI + 微信）。

## 5. 唯一剩余人工动作（等用户确认）
```powershell
# 用户确认后（可选本地预检在 CI 已完成，无需重复）：
cd D:\Project\Agent-notify
git tag -a v2.1.0 -m "AgentNotify v2.1.0：集群编排 v1"
git push origin v2.1.0
```
- tag push 触发 Release：validate → 签名 build → publish（二进制镜像仓 + 源码 Release）；已发布 Release 不自动覆盖。
- 坏包预案（§6-3）：补丁版 2.1.1 + 审计输出，不覆盖已发布包。

## 6. 结论
- 发布路径接口、tag 可达性、版本一致性、产物完整性（5 程序 + 清单覆盖 24 文件 + p7s）、运行时启动健康**全部预检通过**。
- 未执行（按指示）：打 tag、发布、发布后验证；T-J/§3.4 真机矩阵。
