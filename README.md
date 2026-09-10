# linkWeixin

多 agent（opencode / codex）任务跑完后推送微信，带任务摘要。

通道：PushPlus → 微信服务号通知。密钥只放环境变量，不落盘、不进仓库。
Windows 10/11 + PowerShell 5.1，中文优先。

## 特性

- **双链路**：opencode 全局插件 / codex notify 中转，任务完成自动推送标题 + 摘要
- **统一渲染**：去代码块、标题加粗、列表分行、按句截断——格式两端一致
- **悬浮窗**：opencode / codex 独立开关、运行灯、上次推送时间、托盘常驻、开机自启
- **免打扰**：开关 marker、标题含 🔕 / `[勿扰]` 跳过、时段静默（如 `23-8`）
- **无窗口承诺**：后台子进程不闪控制台 / Windows Terminal 页签（含看守任务）
- **可维护**：模块化 + 单测/冒烟/守卫、按安装记录精准卸载、版本化发布

## 架构

```text
opencode 桌面端 ──全局插件──▶ notify-ai.ps1 ──┐
                                             ├─▶ PushPlus API ─▶ 微信
codex 桌面端 ──notify wrapper──▶ notify-ai.ps1 ─┘
```

渲染与发送只在 `notify-ai.ps1`（LinkWeixin 模块）做一次，各 agent 只传原文。
组件职责、关键设计决策与完整外部契约见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 前置要求

- Windows 10 / 11，Windows PowerShell 5.1 及以上（暂不支持 macOS / Linux）
- opencode 桌面端 和/或 codex 桌面端（二者装一个即可）
- 一个 PushPlus token，四步拿：
  1. 去 [pushplus.plus](https://www.pushplus.plus) 注册登录，后台复制 token；
  2. 微信关注「PushPlus 推送加」服务号；
  3. 在 PushPlus 后台把微信绑定上（不绑定 token 有效但收不到消息）；
  4. `setx PUSHPLUS_TOKEN "你的token"` 写进用户环境变量。
- 安装与日常使用用同一个 Windows 账户（路径与 `PUSHPLUS_TOKEN` 均按当前用户解析）
- 可选：Python 3（悬浮窗以 `pythonw` 无控制台启动；不装自动回退 vbs，不影响功能）

## 快速开始

```powershell
# 1. 配密钥（改完必须重启 opencode / codex 桌面端才生效）
setx PUSHPLUS_TOKEN "你的token"

# 2. 管理员 PowerShell 里跑安装（注册计划任务要提权）
powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1

# 3. 重启 opencode / codex 桌面端（含后台 service）

# 4. 跑测试（不真推、不联网、不碰真实配置）
powershell -NoProfile -ExecutionPolicy Bypass -File tools\test.ps1

# 5. 真推一条验证
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-ai.ps1" `
  -Title "安装验证" -Summary "linkWeixin 安装成功"
```

安装脚本默认行为（可用参数覆盖，`Get-Help .\install.ps1 -Detailed`）：

| 动作 | 默认目标 |
|---|---|
| 安装运行文件（`src\` 整树） | `%USERPROFILE%\bin` |
| 安装 opencode 插件 | `%USERPROFILE%\.config\opencode\plugins\notify-pushplus.ts`（V2 约定） |
| 接管 codex `notify` | `%USERPROFILE%\.codex\config.toml`（先备份；自定义配置不动） |
| 注册计划任务 `CodexNotifyWatch` | 登录触发 + 每 5 分钟跑 watcher |
| 建悬浮窗快捷方式 | `shell:startup` + 桌面（自动选 pythonw 或 vbs） |
| 写安装记录 | `%USERPROFILE%\bin\linkweixin-install.json`（卸载按它精准清理） |

非管理员：加 `-SkipScheduledTask` 跳过任务注册（日后 codex 配置若被改回，
手动重跑 `install.ps1` 或看守脚本即可恢复）。

## 验证安装

1. `tools\test.ps1` 全绿即渲染链路与沙箱安装/卸载 OK。
2. 真推一条（上面第 5 步），微信 10 秒内收到即 token + 通道 OK。
   收不到：先确认服务号已关注且后台已绑定，再查 `%TEMP%\opencode\notify-push.log`。
3. 跑一个真任务：opencode 里跑完一个任务看推送；codex 里跑完一个 turn 看推送。
   没推？见 [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) 的症状排查。

## 日常使用

### 悬浮窗

右下角无边框小窗（`linkweixin-widget.ps1`，Win11 圆角 + 悬停反馈）：

- **双开关**：opencode / codex 各一个按钮（绿 ON / 红 OFF），顶条与托盘图标同步
- **运行灯**：两个 agent 进程是否在跑；**上次推送**用相对时间（刚刚 / N 分钟前）；
  正常时底栏显示免打扰与今日推送数，异常时橙字报警；底栏显示版本号
- **窗口找回**：`—` 最小化到任务栏（必定找得回）；`×` 藏到托盘并弹气泡。
  还有三条路：任务栏按钮 / 双击托盘图标 / 桌面「linkWeixin 悬浮窗」
- **自动自愈**：看守任务每 5 分钟看一眼，进程不在且非主动退出就自动拉起
  （静默死亡 ≤5 分钟复活；红字"退出"是主动退出，不会被拉起）
- 拖标题区移动，**位置会被记住**；右键托盘菜单可开关推送、测试推送或退出
- 托盘图标在 Win11 默认收进 `^` 溢出区：可拖出来钉住，或去
  设置 → 个性化 → 任务栏 → 其他系统托盘图标 里打开

### 随用随开 / 免打扰

```powershell
# 翻转两个开关（各回显一行，永远 exit 0）
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-toggle.ps1"
# 只动一边 / 显式开关
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILE\bin\notify-toggle.ps1" -Agent Opencode -Off
```

- marker 文件存在 = 该边关（opencode 看 `notify-pushplus.off`，codex 看 `codex-notify.off`）
- **标题免打扰**：会话标题含 🔕 或 `[勿扰]`，该会话不推（改名即生效）
- **时段免打扰**：`setx OPENCODE_NOTIFY_QUIET "23-8"`（起-止小时，左闭右开，格式错 fail-open），
  改完需重启 opencode 桌面端

## 配置（环境变量）

| 变量 | 说明 |
|---|---|
| `PUSHPLUS_TOKEN` | 必填，PushPlus token（`setx` 写入 HKCU，改完需重启对应桌面端） |
| `NOTIFY_AI_SCRIPT` / `OPENCODE_NOTIFY_SCRIPT` | 推送脚本路径，默认 `%USERPROFILE%\bin\notify-ai.ps1` |
| `OPENCODE_NOTIFY_COOLDOWN_MIN` | 同会话冷却分钟数，默认 10 |
| `OPENCODE_NOTIFY_STATE_FILE` / `OPENCODE_NOTIFY_LOG_FILE` | 去重状态 / 推送记录路径，默认 `%TEMP%\opencode\` 下 |
| `OPENCODE_NOTIFY_OFF=1` | opencode 推送总开关 |
| `OPENCODE_NOTIFY_MARKER_FILE` | opencode 开关 marker 路径，文件存在即关，默认 `%USERPROFILE%\.config\opencode\notify-pushplus.off` |
| `CODEX_NOTIFY_MARKER_FILE` | codex 开关 marker 路径，默认 `%USERPROFILE%\.config\opencode\codex-notify.off` |
| `OPENCODE_NOTIFY_QUIET` | 勿扰时段，格式 `23-8`，解析失败 fail-open（不断推送） |
| `OPENCODE_NOTIFY_DEBUG=1` | opencode 插件调试日志（联调完记得关） |
| `CODEX_NOTIFY_DEBUG=1` | codex wrapper 参数日志（联调完记得关） |
| `CODEX_CONFIG` / `CODEX_NOTIFY_WRAPPER` | 看守脚本目标配置 / wrapper 路径（默认自动推导） |

`.env.example` 有可复制模板（`.env` 已进 `.gitignore`）。

## 文档索引

| 文档 | 内容 |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构、组件职责、关键设计决策、外部契约清单 |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | 日志位置速查、症状 → 排查步骤、历史踩坑 |
| [CHANGELOG.md](CHANGELOG.md) | 版本记录（语义化版本） |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 开发环境、三件套、提交/分支规范、发布流程 |
| [SECURITY.md](SECURITY.md) | 漏洞报告渠道与安全设计说明 |
| [docs/specs/](docs/specs/) | 0.1.0 规范化设计文档与实施计划 |

## 路线图

- [x] 0.1.0：模块化、测试/守卫、工程门禁、安装记录、文档与治理
- [ ] 公开仓库：CODE_OF_CONDUCT（中文版）、winget / scoop、在线一键安装（`irm | iex`）

## 卸载

```powershell
# 管理员 PowerShell（删计划任务要提权）
powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
```

按安装记录清理运行文件与插件、删计划任务与快捷方式、杀悬浮窗进程，
codex 配置从备份还原。`PUSHPLUS_TOKEN` 环境变量请手动清理。删完重启两个桌面端。

## License

MIT，见 [LICENSE](LICENSE)。
