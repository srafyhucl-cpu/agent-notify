# 故障排查

先查日志，再对症状。所有日志都在 `%TEMP%\opencode\` 下（不记录 token，可放心粘贴）。

## 日志位置速查

| 文件 | 写入方 | 看什么 |
|---|---|---|
| `notify-push.log` | opencode 插件 | 推送成功记录（时间 / 会话 / 标题）；悬浮窗"上次推送"也读它 |
| `notify-debug.log` | opencode 插件 | `OPENCODE_NOTIFY_DEBUG=1` 时的事件/跳过原因（marker/冷却/时段） |
| `codex-notify-debug.log` | codex wrapper | `CODEX_NOTIFY_DEBUG=1` 时的参数、marker 跳过、推送耗时 |
| `codex-watch.log` | 看守 | 每次恢复 codex 配置的时间 |
| `widget-error.log` | 悬浮窗 | UI 异常、模块加载失败、托盘自愈等 |
| `widget-boot.log` | 悬浮窗 | 每次启动的 PID 与时间（对比 `widget-alive.txt` 判断是否还活着） |
| `widget-alive.txt` | 悬浮窗 | ~30 秒一次心跳 |
| `notify-push-sent.json` | 插件 | 跨实例冷却状态（去重） |

## 症状排查

### 微信完全收不到

1. 先确认服务号已关注，且 PushPlus 后台已绑定微信（不绑定一定收不到）
2. 直推验证：`powershell -File "$env:USERPROFILE\bin\notify-ai.ps1" -Title t -Summary "直推验证"`
   - 无输出且微信收到 → 通道 OK，问题在触发端
   - stderr 有 `code=999` → PushPlus 拒收重复内容（固定文案第二次起发不出去；默认文案已带时间戳规避）
   - stderr 有超时 → 网络/代理问题，脚本超时 20 秒；检查代理设置
3. 没输出也没收到？`-DryRun` 看 payload 是否正常生成

### opencode 任务跑完没推

1. 开 `OPENCODE_NOTIFY_DEBUG=1`，**重启 opencode 桌面端（含后台 service）**，跑一个任务
2. 看 `notify-debug.log`：
   - 没有 `session.execution.succeeded` 事件 → 桌面端大版本可能改了事件名，带日志提 Issue
   - `skip: marker-off` → 开关关着（悬浮窗或 `notify-toggle.ps1` 打开）
   - `skip: quiet-hours` → 时段静默中
   - `skip: cooldown` / `file-cooldown` → 同会话冷却（默认 10 分钟），可调 `OPENCODE_NOTIFY_COOLDOWN_MIN`
   - `skip: dnd-title` → 会话标题含 🔕 或 `[勿扰]`
   - `skip: no PUSHPLUS_TOKEN` → 环境变量没生效，见下文
3. 悬浮窗底栏橙字提示"插件旧版/未安装"→ 重跑 `install.ps1` 并重启桌面端

### codex turn 跑完没推

1. 开 `CODEX_NOTIFY_DEBUG=1`，重启 codex 桌面端，跑一个 turn
2. 看 `codex-notify-debug.log`：
   - 没有新行 → config.toml 的 notify 行没指向 wrapper（见下方"配置被改回"）
   - `marker-off skip push` → 开关关着
   - `push exit=...` 看子进程退出码与耗时
3. 透传是否正常：原电脑操控功能是否还在（wrapper 先透传后推送，推送失败不影响透传）

### 开关关了还在推

- opencode：看悬浮窗底栏是否报"插件旧版"（旧插件不认 marker）；重跑 `install.ps1` + 重启桌面端
- codex：marker 只跳推送，透传不受影响是预期行为；仍推就开 debug 看 `marker-off` 是否出现
- 标题/时段免打扰只管 opencode，codex 不看它们

### 悬浮窗打不开 / 不见了 / 灯不灭

- 打不开：跑窗体脚本看报错（`%TEMP%\opencode\widget-error.log`）；模块加载失败会弹窗提示
- 不见了：托盘 `^` 区域找绿/橙/红点，拖出来；或桌面双击「linkWeixin 悬浮窗」（单实例接管）
- 灯灭不了：`codex-plus-plus*`（Codex++，另一个软件）已排除；仍绿先确认 Codex
  进程真退了（看守/后台 service 常驻也会亮）
- 图标丢了：每 5 分钟自愈重建，等一会；仍有问题看 `widget-error.log`

### 每 5 分钟闪一下窗口

看守任务注册动作是旧版（没经 `run-hidden.vbs`）。管理员重跑一遍 `install.ps1` 重注册即可；
悬浮窗底栏会橙字提示。

### codex 配置被改回直调 exe

codex 桌面启动/更新时会重写 `config.toml`。看守任务每 5 分钟恢复；
没装看守（`-SkipScheduledTask` 装的）就手动重跑 `install.ps1` 或看守脚本。

### 环境变量改了不生效

环境变量只在进程启动时读一次：`setx` 后**必须重启对应桌面端（含后台 service）**。
opencode 后台 service 可用 `opencode-cli.exe service restart` 单独重启。

## 历史踩坑（复现时注意）

1. **PushPlus 拒收重复内容（code=999）**：固定文案第二次起发不出去。
   解法：兜底文案带时间戳保证唯一；脚本对非 200 打 stderr，不静默吞。
2. **stdin 悬挂**：`execFile` 默认 stdin 是常开管道，脚本 `ReadToEnd()` 会等到超时。
   解法：插件侧显式 `child.stdin.end()` + 脚本 `-NoStdin` 开关双保险。
3. **PowerShell 5.1 读 UTF-8 无 BOM 按 GBK 解析**：所有 `.ps1/.psm1/.psd1` 必须带
   BOM（仓库守卫测试硬检；新增文件忘了会被 `Repo.Tests.ps1` 拦下）。
4. **`Invoke-RestMethod` 5.1 默认 latin-1 发 body**：中文变问号，必须转 UTF-8 字节数组。
5. **空 `-Summary` 嵌套调用绑定失败**：`powershell -File` 链下空字符串参数会丢，
   导致子进程报 MissingArgument。解法：为空时整个省略该参数。
6. **codex 桌面会改写 `config.toml`**：启动/更新后 notify 被改回直调 exe
   （exe 路径里的哈希目录也会变）。解法：wrapper 动态找最新 exe + watcher 定时恢复。
7. **opencode 事件名以实测为准**：SDK 文档的 `session.idle` 本版不发，
   以 debug 抓到的 `session.execution.succeeded` 为准；`session.context()` 返回
   扁平 `{type, text}` + assistant `content[]` 结构，不是文档里的 `{info, parts}`。
8. **环境变量只在进程启动时读一次**：见上。
9. **全局插件对本机所有会话生效**（含 agent/API 会话），靠冷却压频率。
10. **开关关了还推**：先看悬浮窗底栏，报插件旧版就是装上去的插件没更新——
    重跑 `install.ps1` 再重启桌面端（插件只在启动时加载）。
11. **悬浮窗 codex 灯灭不了**：`codex-plus-plus*` 已排除；仍绿先确认
    Codex 桌面进程真的退了。
12. **Win11 默认终端是 Windows Terminal 时黑窗口/页签闪**：WT 会在 powershell
    应用 `-WindowStyle Hidden` 之前先把窗口建出来，所以 `.lnk` 快捷方式和
    计划任务**必须**经 `src\run-hidden.vbs` 中转（wscript 本身无控制台）。
    悬浮窗另走 `pythonw`（GUI 宿主，连控制台都不分配，防 WT 空页签被误关）。

## 提 Issue 前建议收集

- `linkweixin-install.json`（版本 / launcher 字段）
- 相关日志的**最后 30 行**（上面速查表按症状选）
- 环境：Windows 版本、`$PSVersionTable.PSVersion`、agent 版本、是否装了 Python
- 已尝试的排查步骤

模板见仓库 Issue 表单。
