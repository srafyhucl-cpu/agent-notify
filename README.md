# linkWeixin

多 agent（opencode / codex）任务跑完后推送微信，带任务摘要。

通道：PushPlus → 微信服务号通知。密钥只放环境变量，不落盘、不进仓库。

## 架构

```text
opencode 桌面端 ──全局插件──▶ notify-ai.ps1 ──┐
                                             ├─▶ PushPlus API ─▶ 微信
codex 桌面端 ──notify wrapper──▶ notify-ai.ps1 ──┘
```

渲染（标题加粗 / 列表 / 代码块剔除 / 按句截断 / HTML）只在 `notify-ai.ps1` 做一次，
各 agent 只传原文，保证两端格式一致。

## 仓库结构

```text
linkWeixin/
├── scripts/
│   ├── notify-ai.ps1           # 通用推送脚本：唯一发 PushPlus 的地方
│   ├── codex-notify.ps1        # codex notify 中转：透传原电脑操控集成 + 推送
│   ├── codex-notify-watch.ps1  # 看守：codex 改写配置后恢复 wrapper
│   ├── notify-toggle.ps1       # 随用随开：翻转 marker 总开关（只管 opencode 侧）
│   └── linkweixin-widget.ps1   # 悬浮窗：大开关 + 运行灯 + 上次推送（开机自启）
├── opencode-plugin/
│   └── notify-pushplus.ts      # opencode 全局插件，订阅任务完成事件
├── tests/
│   └── smoke.ps1               # 冒烟测试：语法 + DryRun + watcher 幂等 + toggle 翻转
├── install.ps1                 # 一键安装（计划任务需管理员；悬浮窗开机快捷方式无需）
├── uninstall.ps1               # 卸载还原（含悬浮窗进程 + 开机快捷方式）
├── .env.example                # 环境变量模板
├── LICENSE                     # MIT
└── README.md
```

## 前置要求

- Windows + PowerShell 5.1 及以上（暂不支持 macOS / Linux）
- opencode 桌面端 和/或 codex 桌面端（二者装一个即可；codex CLI 理论上同 key，未实测）
- 一个 PushPlus token，按下面四步拿：
  1. 去 [pushplus.plus](https://www.pushplus.plus) 注册登录，后台复制 token；
  2. 微信关注「PushPlus 推送加」服务号；
  3. 在 PushPlus 后台把微信绑定上（不绑定 token 有效但收不到消息）；
  4. `setx PUSHPLUS_TOKEN "你的token"` 写进用户环境变量。
- 安装和日常使用用同一个 Windows 账户（安装路径、`PUSHPLUS_TOKEN` 均按当前用户解析，换账户用会找不到）。

## 快速开始

```powershell
# 1. 配密钥（改完必须重启 opencode / codex 桌面端才生效，见坑 #8）
setx PUSHPLUS_TOKEN "你的token"

# 2. 管理员 PowerShell 里跑安装（注册计划任务要提权）
powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1

# 3. 重启 opencode / codex 桌面端（含后台 service）

# 4. 冒烟测试（不真推，不碰真实配置）
powershell -NoProfile -ExecutionPolicy Bypass -File tests\smoke.ps1

# 5. 真推一条验证
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-ai.ps1" `
  -Title "安装验证" -Summary "linkWeixin 安装成功"
```

安装脚本默认行为（可用参数覆盖，`Get-Help .\install.ps1 -Detailed`）：

| 动作 | 默认目标 |
|---|---|
| 复制 5 个 `.ps1` | `%USERPROFILE%\bin` |
| 安装 opencode 插件 | `%USERPROFILE%\.config\opencode\plugin\notify-pushplus.ts` |
| 接管 codex `notify` | `%USERPROFILE%\.codex\config.toml`（先备份 `.bak-notify-wrapper`；已是 wrapper 或自定义程序则不动） |
| 注册计划任务 `CodexNotifyWatch` | 登录触发 + 每 5 分钟跑 watcher |
| 建悬浮窗快捷方式 | `shell:startup` + 桌面 `linkWeixin 悬浮窗.lnk`（无需管理员，本次同时启动窗体） |

非管理员：加 `-SkipScheduledTask` 跳过任务注册（watcher 不装；日后 codex 配置若被改回，
手动重跑一遍 `install.ps1` 或 watcher 脚本即可恢复）。

## 装完验证（新机器重点看这三处）

1. DryRun 不经过网络：`tests\smoke.ps1` 全绿即渲染链路 OK。
2. 真推一条（第 5 步），微信 10 秒内收到即 token + 通道 OK。
   收不到：先确认服务号已关注且后台已绑定，再看 `%TEMP%\opencode\notify-push.log` 有没有记录。
3. 跑一个真任务：opencode 里跑完一个任务看推送；codex 里跑完一个 turn 看推送。
   opencode 没推：`setx OPENCODE_NOTIFY_DEBUG 1` 后重启桌面端，
   看 `%TEMP%\opencode\notify-debug.log` 里有没有 `session.execution.succeeded` 事件——
   大版本事件名可能变，拿着日志提 issue。

## 环境变量

| 变量 | 说明 |
|---|---|
| `PUSHPLUS_TOKEN` | 必填，PushPlus token（`setx` 写入 HKCU，改完需重启对应桌面端） |
| `NOTIFY_AI_SCRIPT` / `OPENCODE_NOTIFY_SCRIPT` | 推送脚本路径，默认 `%USERPROFILE%\bin\notify-ai.ps1` |
| `OPENCODE_NOTIFY_COOLDOWN_MIN` | 同会话冷却分钟数，默认 10 |
| `OPENCODE_NOTIFY_STATE_FILE` / `OPENCODE_NOTIFY_LOG_FILE` | 去重状态 / 推送记录路径，默认 `%TEMP%\opencode\` 下 |
| `OPENCODE_NOTIFY_OFF=1` | opencode 推送总开关 |
| `OPENCODE_NOTIFY_MARKER_FILE` | 随用随开 marker 路径，文件存在即关，默认 `%USERPROFILE%\.config\opencode\notify-pushplus.off` |
| `OPENCODE_NOTIFY_QUIET` | 勿扰时段，格式 `23-8`（23:00 起到次日 8:00 前静默），解析失败 fail-open（不断推送） |
| `OPENCODE_NOTIFY_DEBUG=1` | opencode 插件调试日志（联调完记得关） |
| `CODEX_NOTIFY_DEBUG=1` | codex wrapper 参数日志（联调完记得关） |
| `CODEX_CONFIG` / `CODEX_NOTIFY_WRAPPER` | 看守脚本的目标配置 / wrapper 路径（默认自动推导，一般不用设） |

`.env.example` 有一份可复制的模板（`.env` 本身已进 `.gitignore`，不会提交）。

## 随用随开（toggle + 悬浮窗）

开会/专注时一键静默 opencode 推送，用完再打开。开关只管 opencode 侧，
codex 侧不受影响是预期行为。

```powershell
# 翻转（有关变开，有开变关，回显 ON/OFF，永远 exit 0）
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-toggle.ps1"

# 显式指定
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-toggle.ps1" -On
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-toggle.ps1" -Off
```

原理：marker 文件 `%USERPROFILE%\.config\opencode\notify-pushplus.off`
存在即关。桌面右下角悬浮窗的大按钮翻的也是它。

另外两道免打扰（与 marker 是或关系，任一命中即跳过，原因进 debug 日志）：

- **标题免打扰**：会话标题含 🔕 或 `[勿扰]`，该会话不推。
  会话改名即生效，无需重启。
- **时段免打扰**：`setx OPENCODE_NOTIFY_QUIET "23-8"`（格式 `起-止` 小时，
  左闭右开，跨天如 `23-8` 表示到次日 8:00 前静默；格式写错 fail-open，
  不断推送）。改完需重启 opencode 桌面端（含后台 service）。

**悬浮窗**（`linkweixin-widget.ps1`，无边框深色小窗，右下角常驻置顶）：

- 大开关：翻 marker，绿底 ON / 红底 OFF，一眼看清状态。
- 运行灯：`opencode` / `codex` 进程在即绿灯（每 5 秒轮询，
  本机实测进程名 `OpenCode*` / `opencode*` / `codex*`，`codex-plus-plus*` 是无关软件已排除），
  仅状态显示。
- 上次推送：读 `%TEMP%\opencode\notify-push.log` 尾行时间，无记录显示暂无推送。
- 底栏异常提示：装上去的插件是旧版（开关不生效）会直接橙字报警。
- 拖标题区移动；右上角 × / — 都是最小化到**任务栏**（按钮一直在，点它恢复，
  找不到了就看任务栏；托盘图标是辅助，Win11 默认收在 `^` 里）；
  **重开**：桌面双击 `linkWeixin 悬浮窗`（单实例，自动接管旧窗体）/
  右键托盘菜单显示 / 手动跑窗体脚本头注释里的命令；
  右键托盘菜单可开关推送或彻底退出（彻底退出后只能桌面双击重开）。
- 开机自启靠 `shell:startup` 快捷方式（`install.ps1` 已建，重启后自启）。
- 不做 token/时段输入框，密钥和时段只走环境变量。

## 工作原理

- **opencode**：插件订阅 `session.execution.succeeded`（实测本版桌面端不发 `session.idle`，
  保留做兼容）。标题取会话标题，摘要取末条 assistant 回复（跳过 reasoning / tool，只取 text，
  原文透传，渲染由 `notify-ai.ps1` 统一做）。
- **codex**：`notify` 收到的 `agent-turn-complete` JSON 里，
  标题取 `input-messages[0]` 前 30 字，摘要取 `last-assistant-message`。
  wrapper 先把原参数/stdin 透传给 `codex-computer-use.exe turn-ended`（路径动态找最新），
  再调 `notify-ai.ps1`，任何一步失败都静默且 `exit 0`。
- **去重**：
  - opencode：内存 Map（同实例并发）+ 状态文件跨实例冷却（默认 10 分钟/会话）；
    推送记录看 `%TEMP%\opencode\notify-push.log`，多推时先查它。
  - codex：turn 结束即推，无冷却（codex 自己的事件粒度已够粗）。
- **看守**：codex 桌面启动/更新会把 `config.toml` 的 `notify` 改回直调 exe，
  计划任务每 5 分钟 + 登录时跑 watcher 恢复（改写前备份，只动指向 exe 的行）。

## 踩过的坑（复现时注意）

1. **PushPlus 拒收重复内容（code=999）**：空内容、固定文案第二次起发不出去。
   解法：兜底文案带时间戳保证唯一；脚本内对非 200 打 stderr，不再静默吞。
2. **stdin 悬挂**：`execFile` 默认 stdin 是常开管道，脚本 `ReadToEnd()` 会等到超时。
   解法：插件侧 `input: ""` + 脚本 `-NoStdin` 开关双保险。
3. **PowerShell 5.1 读 UTF-8 无 BOM 按 GBK 解析**：所有 `.ps1` 必须带 BOM（本仓库已全带，
   新加脚本记得保持；`tests\smoke.ps1` 会间接覆盖到）。
4. **`Invoke-RestMethod` 5.1 默认 latin-1 发 body**：中文变问号，必须转 UTF-8 字节数组。
5. **空 `-Summary` 嵌套调用绑定失败**：`powershell -File` 链下空字符串参数会丢，
   导致子进程报 MissingArgument。解法：为空时整个省略该参数。
6. **codex 桌面会改写 `config.toml`**：启动/更新后 `notify` 被改回直调 exe
   （exe 路径里的哈希目录也会变）。解法：wrapper 动态找最新 exe + watcher 定时恢复。
7. **opencode 事件名以实测为准**：SDK 文档的 `session.idle` 本版不发，
   以 debug 抓到的 `session.execution.succeeded` 为准；`session.context()` 返回
   扁平 `{type, text}` + assistant `content[]` 结构，不是文档里的 `{info, parts}`。
8. **环境变量只在进程启动时读一次**：setx 后必须重启对应桌面端/service，
   重启 UI 不一定重启后台 service（用 `opencode-cli.exe service restart`）。
9. **全局插件对本机所有会话生效**（含 agent/API 会话），靠冷却压频率。
10. **开关关了还推**：先看悬浮窗底栏，报 `插件旧版/未安装` 就是装上去的插件没更新——
    重跑 `install.ps1` 再重启桌面端（含后台 service，插件只在启动时加载）；
    底栏正常但 codex 还在推是预期行为（开关只管 opencode 侧）。
11. **悬浮窗 codex 灯灭不了**：`codex-plus-plus*`（Codex++，另一个软件）已被排除；
    仍绿先确认 Codex 桌面进程真的退了（看守/后台 service 常驻也会亮灯）。

## 卸载

```powershell
# 管理员 PowerShell（删计划任务要提权）
powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
```

删掉装上去的 5 个脚本 + 插件 + 计划任务 + 悬浮窗开机快捷方式（同时杀窗体进程），
codex 配置从 `.bak-notify-wrapper` 还原。
`PUSHPLUS_TOKEN` 环境变量请手动清理。删完重启两个桌面端。

## 安全

- token 只从环境变量读：`notify-ai.ps1` 无 token 直接跳过，`install.ps1` 不写任何密钥。
- 任何推送失败都静默（`exit 0`），永远不卡住 agent。
- 提 PR 前跑一遍 `tests\smoke.ps1`，确认全绿。

## License

MIT，见 [LICENSE](LICENSE)。
