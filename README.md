# linkWeixin

<p align="center">
  <img src="https://img.shields.io/badge/version-v0.3.0-blue.svg?style=flat-square" alt="Version" />
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6.svg?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/PowerShell-5.1%20%7C%207%2B-5391FE.svg?style=flat-square" alt="PowerShell" />
  <img src="https://img.shields.io/badge/CI-Passing-success.svg?style=flat-square" alt="CI" />
  <img src="https://img.shields.io/badge/License-MIT-green.svg?style=flat-square" alt="License" />
</p>

> **让 AI Agent 与长耗时任务在完成后，主动到手机上呼唤你。**  
> 专为 **OpenCode** / **Codex** 及**通用命令行任务**打造的通知中枢。支持**微信（PushPlus）**、**企业微信**、**飞书**、**钉钉**多通道聚合推送，附带 Markdown 智能压缩摘要与现代深色 Fluent 悬浮窗。

---

## 痛点与特性对比

| 体验维度 | 传统脚本 / 官方默认 | linkWeixin v0.3.0 |
|---|---|---|
| **任务完成感知** | 需时刻肉眼盯屏，离开电脑就失联 | 手机微信 / 企微 / 飞书秒级收到任务摘要 |
| **多 Agent 支持** | 工具割裂、每个平台重复折腾 | OpenCode + Codex + 任意 CLI 脚本统一中枢 |
| **消息排版质量** | 代码块刷屏、文字冗长难以阅读 | 自动去代码块、标题加粗、按句智能截断排版 |
| **桌面交互体验** | 命令行黑窗口频繁闪现干扰工作 | 现代深色 Fluent 悬浮窗、无窗口承诺、状态灯 |
| **配置与自愈能力** | 手敲环境变量、配置改写后失效 | 图形化设置中心、通道即时测通、闪屏任务一键自愈 |
| **历史记录追溯** | 翻找临时日志文件困难 | 内置推送历史面板，支持完整摘要预览与一键重推 |

---

## 核心特性

- **多通道聚合推送**：
  - **微信服务号**（PushPlus，扫码即用，个人首选）
  - **企业微信群机器人**（原生 Markdown 渲染，企业/团队高频推荐）
  - **飞书群机器人**（卡片式富文本，极客最爱）
  - **钉钉群机器人**（Markdown 格式推送）
  - **自定义 Webhook**（标准 JSON POST 转发）
- **双 Agent 深度集成 + 通用 CLI**：
  - **OpenCode 全局插件**：监听执行结束事件，自动提取会话标题与末条助手回复
  - **Codex Wrapper**：透明拦截 `notify` 事件，原样透传底层电脑操控，静默完成推送
  - **通用 CLI 独立运行**：支持任意模型训练、数据爬取、持续构建脚本执行完毕后直接通知
- **现代 Fluent 深色悬浮窗 (v0.3.0)**：
  - **抗锯齿微光卡片**：彻底修复 DPI 缩放下单词换行截断，提供精致的深空暗调美学
  - **运行监控灯**：实时探测 Agent 运行状态、上次推送相对时间（刚刚 / N分钟前）
  - **推送历史查看器**：悬浮窗直达历史列表，支持内容一键复制与重新发送
  - **可视化设置中心**：图形化配置 Token 与 Webhook，免打扰时段滑动设置
  - **闪屏任务一键自愈**：智能检测旧版闪黑框任务，点击即可一键升级为静默隐藏看守
- **智能免打扰与守护**：
  - **时段静默**：如 `23-8`（晚上 23:00 至次日 08:00 不打扰）
  - **会话冷却**：同会话默认 10 分钟冷却，防止活跃对话连续刷屏
  - **标题勿扰**：会话标题含 🔕 或 `[勿扰]` 自动跳过
  - **五分钟自动看守**：后台进程静默死亡 ≤5 分钟自愈，开机自动随系统启动

---

## 快速安装

### 方式 A：在线极速安装（推荐）

打开 PowerShell（推荐管理员身份以自动注册隐藏看守任务）：

```powershell
irm https://raw.githubusercontent.com/srafyhucl-cpu/linkWeixin/main/install.ps1 | iex
```

### 方式 B：本地源码安装

```powershell
# 1. 克隆仓库
git clone https://github.com/srafyhucl-cpu/linkWeixin.git
cd linkWeixin

# 2. 运行安装脚本
powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1

# 3. 运行完整测试套件（单测 + 冒烟）
powershell -NoProfile -ExecutionPolicy Bypass -File tools	est.ps1
```

> **提示**：安装完成后，桌面将生成「linkWeixin 悬浮窗」快捷方式，右下角悬浮窗会自动启动。

---

## 30 秒配置与上手

### 1. 配置推送凭据（推荐图形化）

右键点击桌面右下角悬浮窗或托盘图标 → 选择 **「通道与偏好设置」**：
- 输入你的 **PushPlus Token**，或者填入 **企业微信 / 飞书 / 钉钉 Webhook 地址**
- 点击 **「保存配置」** 即可立即生效！

*(亦可使用系统环境变量：`setx PUSHPLUS_TOKEN "你的token"`)*

### 2. 发送一条测试通知

点击悬浮窗底部的 **「测试」** 按钮，或在终端运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:USERPROFILEin
otify-ai.ps1" `
  -Title "安装验证" -Summary "linkWeixin v0.3.0 安装成功，通道与排版正常！"
```

手机端 5 秒内收到通知即配置成功！

---

## 通用 CLI：让任何长耗时任务完成后通知你

linkWeixin 不仅服务于 AI Agent，更是所有 Windows 开发者的长任务通知利器：

```powershell
# 场景 1：深度学习 / 模型训练完成后推送到手机
python train_model.py ; notify-ai -Title "模型训练完毕" -Summary "Epoch 100 结束，验证集准确率 98.4%"

# 场景 2：长时间编译 / 测试运行
cargo build --release && notify-ai -Title "编译成功" -Summary "Release 构建通过"

# 场景 3：数据清洗与爬虫完成
python scrape.py | notify-ai -Title "数据抓取完毕"
```

---

## 悬浮窗交互指南

- **双独立大开关**：点击直接切换 OpenCode / Codex 推送监听（翡翠绿 `● 监听中` / 冷灰红 `○ 已暂停`）。
- **上次推送时间**：点击卡片中的 **「上次推送」** 行，即可直接弹出 **「推送历史查看器」**。
- **历史记录面板**：查看最近 100 条推送详情、完整摘要排版，支持一键复制内容。
- **一键自愈闪屏**：若检测到旧版计划任务，状态栏会亮起 `⚡ 发现旧版任务，点击一键修复闪屏`，点击即可瞬间修复。
- **窗口停靠与托盘**：
  - `—`：最小化到任务栏（任务栏按钮带状态呼吸灯）
  - `✕`：藏入系统托盘（双击托盘图标或桌面快捷方式即可恢复）
  - 右键托盘：支持查看历史、设置、测试推送、复制推荐名片与退出。

---

## 外部契约与配置参考

| 配置项 | 说明 | 默认位置 / 默认值 |
|---|---|---|
| 私有配置文件 | 用户独立配置（凭据与通道开关） | `%USERPROFILE%\.config\linkweixin\config.json` |
| `PUSHPLUS_TOKEN` | PushPlus 微信服务号 Token | 环境变量或私有配置 |
| `WECOM_WEBHOOK_URL` | 企业微信机器人 Webhook 地址 | 环境变量或私有配置 |
| `FEISHU_WEBHOOK_URL` | 飞书群机器人 Webhook 地址 | 环境变量或私有配置 |
| `DINGTALK_WEBHOOK_URL` | 钉钉群机器人 Webhook 地址 | 环境变量或私有配置 |
| `OPENCODE_NOTIFY_QUIET` | 勿扰时段（格式如 `23-8`） | 私有配置优先，解析失败 fail-open |
| `OPENCODE_NOTIFY_COOLDOWN_MIN` | 同一会话推送冷却时间 | 默认 `10` 分钟 |
| 推送日志与状态 | 推送历史及去重状态落盘 | `%TEMP%\opencode
otify-push.log` |

---

## 卸载

在管理员 PowerShell 中运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File uninstall.ps1
```

脚本将根据安装记录精准清理全部运行文件、计划任务及快捷方式，并自动恢复 Codex 原有配置文件。

---

## 贡献与治理

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - 架构设计、职责划分与契约红线
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) - 常见疑难排查指南
- [CHANGELOG.md](CHANGELOG.md) - 详细版本变更历史
- [CONTRIBUTING.md](CONTRIBUTING.md) - 代码提交规范与测试门禁

---

## License

[MIT License](LICENSE) © 2026 linkWeixin Contributors.
