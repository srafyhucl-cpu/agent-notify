# 「微信会话已断开」可见提醒设计

## 背景

ClawBot 主动推送依赖服务端下发的 `context_token`。该上下文会被服务端回收，典型表现是发送接口返回 `ret=-2 / errmsg="prepare failed"`。此时登录仍然有效，但所有通知都发不出去，`push.log` 记录为 `会话未建立`。

实际发生的事故：2026-09-17 16:35 上下文失效，到 19:05 之间连续 9 条通知全部失败，用户直到手动排查才发现。

现有可见性不足，原因有三处：

1. `internal/ui/widget_draw.go:608` 的 `recentStatusColor` 把 `会话未建立` 归入 `default` 分支，用灰色文字显示在「最近推送」卡片右侧，视觉上等同正常。
2. `internal/ui/widget_draw.go:571` 的底部「微信配置」按钮只在 `!clawbotLoggedIn` 时变成红色「微信未连」；已登录但会话失效时仍显示「微信配置」，看起来一切正常。
3. `internal/ui/widget_views.go:565` 的微信配置页只要已登录就显示「ClawBot 微信已成功连接 / 主动推送链路正常」，在会话失效时是错误陈述。

同时项目没有任何 Windows 通知能力：`NOTIFYICONDATAW` 结构体已有 `SzInfo` / `SzInfoTitle` 字段，但从未使用，`NIF_INFO` 常量未定义。

## 目标

1. 「已登录但会话失效」在微信连接相关的四处位置有显著且一致的状态显示。
2. 该状态首次出现时弹一次 Windows 托盘气泡通知；问题未解决时不重复弹，恢复后再次失效可再弹。
3. 「刚登录、还没收到首条微信消息」属于正常等待，不能被当成故障提醒。
4. 旧凭据文件和新凭据文件都能正常工作，不要求用户重新登录。

## 非目标

1. 不改变「未登录」的现有红色显示与文案。「登录已失效」只在设置页 ClawBot 卡片与微信配置页修正为真实状态（现状是绿色「会话正常」，属于错误陈述），不纳入气泡提醒范围。
2. 不把网络错误、服务端限流等其他推送失败纳入本次提醒范围。
3. 不新增用户开关或配置项。
4. 不做自绘飞入提示窗口，不引入 WinRT Toast 依赖。
5. 不改变 ClawBot 的登录、轮询、发送逻辑本身。

## 状态模型

微信链路对外收敛为五种状态。判定输入为 `clawbot.GetStatus()` 的 `loggedIn` / `stale` / `sessionReady`，加上本设计新增的「曾经就绪」记忆：

| 状态 | 条件 | 性质 |
|---|---|---|
| `wechatLinkOK` | `loggedIn && !stale && sessionReady` | 正常 |
| `wechatLinkAwaitingFirst` | `loggedIn && !stale && !sessionReady && 从未就绪过` | 正常等待，非故障 |
| `wechatLinkBroken` | `loggedIn && !stale && !sessionReady && 曾经就绪过` | 故障，需强提醒 |
| `wechatLinkNotLoggedIn` | `!loggedIn` | 故障（维持现状） |
| `wechatLinkStale` | `loggedIn && stale` | 故障（维持现状） |

关键区分点是「曾经就绪过」：刚扫码登录后 `context_token` 必然为空，属于登录流程的既定步骤，只做界面引导；只有曾经成功建立过会话、之后失效才算链路断开。

## 技术方案

### 1. 数据层：记录会话世代

`clawbot.json` 新增两个可选字段。该文件已经在保存 `stale_at`、`get_updates_buf` 等会话状态，沿用同一位置不引入新文件。

| 字段 | 含义 | 写入时机 | 清空时机 |
|---|---|---|---|
| `session_established_at` | 最近一次成功建立主动推送会话的时间（RFC3339） | `internal/clawbot/session.go` 的 `savePolledSession` 写入 `context_token` 时 | 重新登录时 |
| `session_alert_at` | 最近一次弹出「已断开」提醒的时间（RFC3339） | 气泡弹出后 | 会话恢复时 |

约束：

- `ClearSessionContext`（处理 `prepare failed` 的清理路径）清空 `context_token` / `context_user_id` / `get_updates_buf`，但**不清除** `session_established_at`，否则会丢失「曾经就绪过」的记忆。
- 两个字段都不含密钥，可以安全出现在诊断输出中。
- 旧文件缺失这两个字段时按空字符串处理，与现有 `stale_at` 的读取方式一致。

`clawbot.Status` 新增两个只读布尔字段，界面不需要接触原始时间戳：

```go
EverReady bool `json:"everReady"` // 当前未就绪，但历史上建立过会话
Alerted   bool `json:"alerted"`   // 针对当前这次断开已经提醒过
```

并新增写入接口，供界面在弹出气泡后记录：

```go
// MarkSessionAlerted 记录"已针对当前断开提醒过"，失败时返回错误，由调用方决定如何处理。
func MarkSessionAlerted() error
```

`clawbot` 包负责凭据文件读写，界面不直接改 `clawbot.json`。

### 2. 判定层：单一枚举

新增 `internal/ui/wechat_link.go`，把四处分散的组合判断收拢到一处：

```go
type wechatLinkState int

const (
    wechatLinkOK wechatLinkState = iota
    wechatLinkAwaitingFirst
    wechatLinkBroken
    wechatLinkNotLoggedIn
    wechatLinkStale
)

func wechatLinkStateFor(loggedIn, stale, sessionReady, everReady bool) wechatLinkState
```

界面四处显示与气泡触发全部只读这个函数，避免同一状态在多处各判一次且判法不一致。

### 3. 界面四处显示

| 位置 | 文件 | 正常 | 等待首条消息 | 已断开 | 未登录 / 登录失效 |
|---|---|---|---|---|---|
| 底部 dock 按钮 | `internal/ui/widget_draw.go` | 「微信配置」常规色 | 「待发消息」警告色 | 「推送已断」警告色 | 「微信未连」危险色；登录失效维持现状「微信配置」 |
| 设置页 ClawBot 卡片 | `internal/ui/widget_views.go` | 绿点「ClawBot 微信会话正常」 | 黄点「等待你给 ClawBot 发第一条消息」 | 黄点「主动推送会话已失效」 | 红点「ClawBot 微信未登录」/「ClawBot 微信登录已失效」 |
| 微信配置页 | `internal/ui/widget_views.go` | 「已连接，推送正常」 | 「已登录，等待你的第一条消息」 | 提示块：「在微信里给 ClawBot 发一条消息即可恢复」 | 维持扫码登录界面 |
| 「最近推送」卡片状态色 | `internal/ui/widget_draw.go` | 成功绿 / 失败红 | 不适用 | 「会话未建立」由灰改警告色 | 维持现状 |

dock 按钮宽度只有 87 逻辑像素且绘制不使用省略号（`drawFluentDockButton` 的 `DT_CENTER|DT_SINGLELINE`），因此 dock 文案固定为 4 个汉字，与现有「推送历史」「系统设置」一致。

「登录失效」的修正是把设置页卡片与微信配置页从错误的绿色「会话正常」改为红色真实状态，`stale` 不触发气泡通知。

### 4. Windows 通知

- `internal/ui/win32.go` 新增 `NIF_INFO = 0x00000010`、`NIIF_WARNING = 0x00000002`，并给 `NOTIFYICONDATAW` 补充 `SzInfo` / `SzInfoTitle` 的拷贝辅助（字段已存在）。
- `internal/ui/tray.go` 新增 `ShowAlert(title, body string)`：`Shell_NotifyIconW(NIM_MODIFY, ...)`，`UFlags = NIF_INFO`，`DwInfoFlags = NIIF_WARNING`。`NOTIFYICONDATAW` 没有可用的独立 `uTimeout` 字段（该位置是 `uVersion` 的联合体），因此不设超时，由系统决定展示时长；Win10/11 会把气泡并入通知中心，用户可回看。
- 文案：
  - 标题：`微信推送已断开`
  - 正文：`ClawBot 主动推送会话失效，任务通知暂时发不出去了。请在微信里给 ClawBot 发任意一条消息即可恢复。`
- 触发规则：`refreshState` 每 5 秒执行一次；当状态为 `wechatLinkBroken` 且 `Alerted == false` 时弹一次气泡，弹出后调用 `clawbot.MarkSessionAlerted()`。会话恢复时 `savePolledSession` 清空 `session_alert_at`，下次断开可再次提醒。
- 标记持久化在文件中，因此「只弹一次」跨进程重启、跨开机自启均成立。
- `MarkSessionAlerted()` 失败不能让用户每 5 秒被弹一次：进程内另有一个「本次运行已弹过」的布尔量，写入失败只影响跨重启去重，并把错误写入 `widget-error.log`，不做静默吞掉。

### 5. 兼容与默认

- 旧 `clawbot.json` 无 `session_established_at` → 判为从未就绪 → 不弹气泡，只做界面黄色引导。这类用户下次会话恢复时标记自动写入，之后才具备提醒能力。
- 新增字段全部可选，旧版本程序读到新文件时忽略未知字段，不影响回退使用。
- 不新增开关，不需要用户配置。

## 测试与验收

### 单元测试（不触网）

1. `wechatLinkStateFor` 表驱动覆盖全部输入组合，重点断言 `everReady=false` 时不得判为 `wechatLinkBroken`。
2. 提醒一次性：有 `session_established_at`、无 `session_alert_at` → `Alerted=false`；`MarkSessionAlerted()` 之后 → `Alerted=true`；会话恢复清空后再次断开 → `Alerted=false`。
3. `savePolledSession` 写入 `session_established_at` 并清空 `session_alert_at`。
4. `ClearSessionContext` 保留 `session_established_at`、清空 `context_token`。
5. 旧格式凭据文件（无新字段）读出 `EverReady=false`。
6. `recentStatusColor` 对 `会话未建立` 返回警告色。
7. 现有 `internal/ui/tray_state_test.go`、`internal/ui/agent_cards_test.go` 不回归。

### 真实验收（必须走真实链路）

1. 正常发送一条通知：四处显示正常，无气泡。
2. 保留 `session_established_at`、清空 `context_token`，重启悬浮窗：立即出现托盘气泡，三处界面进入「已断开」显示。
3. 在微信中给 ClawBot 发送一条消息：警示消失、`session_alert_at` 清空；再次构造断开：气泡再次出现。
4. 模拟新登录（清空 `session_established_at`）：只有黄色引导，无气泡。

### 门禁

`go test ./...`、`go vet ./...`、`gofmt -l cmd internal`、`tools\test.ps1`、`tools\lint.ps1`。
