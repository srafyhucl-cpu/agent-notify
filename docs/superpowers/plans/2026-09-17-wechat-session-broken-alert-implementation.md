# 「微信会话已断开」可见提醒 Implementation Plan

**状态：** 对应实现已随 v1.x 发布（`internal/ui/wechat_link.go` 的会话世代与托盘气泡提醒）；本计划复选框未回填，保留原文作为历史记录。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让「已登录但主动推送会话失效」在微信连接区域四处可见，并在首次出现时弹一次 Windows 托盘气泡提醒，同时区分「刚登录的正常等待」与「曾经正常、后来失效」。

**Architecture:** `internal/clawbot` 在凭据文件中记录会话世代（`session_established_at` / `session_alert_at`），并投影为 `Status.EverReady` / `Status.Alerted`；`internal/ui` 用单一枚举 `wechatLinkState` 把这两个布尔量翻译成显示状态，四处绘制与托盘气泡共用同一判定；气泡复用现有托盘图标的 `NIF_INFO` 通道，不引入新依赖。

**Tech Stack:** Go 1.26、Win32 GDI / `Shell_NotifyIconW`、Go 标准库 `testing`

## Global Constraints

- 只把「会话未建立」纳入气泡提醒；未登录与登录失效不弹气泡。
- 「未登录」维持现有红色显示与文案；「登录失效」只在设置页 ClawBot 卡片与微信配置页修正为真实状态（现状是绿色「会话正常」，属错误陈述）。
- 旧 `clawbot.json` 缺少新字段时必须按空值工作，不要求用户重新登录。
- 新增字段全部可选（`omitempty`），旧版本程序读到新文件不得报错。
- 气泡每次断开只弹一次，跨进程重启仍然成立；会话恢复后重置，再次断开可再弹。
- 重新登录必须重置会话世代，不得把上一次登录的「曾就绪」记忆带进新登录。
- dock 按钮宽度仅 87 逻辑像素且绘制无省略号，文案固定 4 个汉字。
- 注释与用户可见文案使用中文。
- **并行改动提示：** 本仓库同时有其他 agent 在重构 `internal/ui`（把 `widget_wndproc.go` 拆成 `widget_mouse_down.go`、`widget_mouse_move.go` 等）。本计划只按**符号名**定位，实施前先用 `grep` 重新确认符号所在文件；若目标文件正在被改动，先等对方提交再动手。

---

### Task 1: clawbot 记录会话世代与提醒状态

**Files:**
- Modify: `internal/clawbot/types.go`（`Credentials`、`Status`）
- Modify: `internal/clawbot/auth.go`（`SaveCredentials`、`GetStatus`）
- Modify: `internal/clawbot/session.go`（`savePolledSession`、新增 `MarkSessionAlerted`）
- Test: `internal/clawbot/state_test.go`、`internal/clawbot/session_test.go`

**Interfaces:**
- Consumes: 已有 `updateCredentials(func(*Credentials) error) error`、`saveCredentials(Credentials) error`、`GetStatus() Status`
- Produces: `Credentials.SessionEstablishedAt string`、`Credentials.SessionAlertAt string`
- Produces: `Status.EverReady bool`、`Status.Alerted bool`
- Produces: `func MarkSessionAlerted() error`

- [ ] **Step 1: 在 state_test.go 增加失败测试（登录边界与会话记忆）**

追加到 `internal/clawbot/state_test.go` 末尾：

```go
func TestSaveCredentialsResetsSessionGeneration(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	if err := SaveCredentials(boundCredentials()); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	// 用 updateCredentials 模拟运行期写入，绕过登录边界重置。
	if err := updateCredentials(func(credentials *Credentials) error {
		credentials.SessionEstablishedAt = "2026-09-17T15:00:00+08:00"
		credentials.SessionAlertAt = "2026-09-17T16:40:00+08:00"
		return nil
	}); err != nil {
		t.Fatalf("updateCredentials: %v", err)
	}

	if err := SaveCredentials(boundCredentials()); err != nil {
		t.Fatalf("SaveCredentials relogin: %v", err)
	}
	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got.SessionEstablishedAt != "" || got.SessionAlertAt != "" {
		t.Fatalf("重新登录后会话世代残留: %#v", got)
	}
}

func TestClearSessionContextKeepsEstablishedMarker(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	if err := SaveCredentials(boundCredentials()); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	if err := updateCredentials(func(credentials *Credentials) error {
		credentials.SessionEstablishedAt = "2026-09-17T15:00:00+08:00"
		return nil
	}); err != nil {
		t.Fatalf("updateCredentials: %v", err)
	}

	if err := ClearSessionContext("ctx-1"); err != nil {
		t.Fatalf("ClearSessionContext: %v", err)
	}
	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got.ContextToken != "" || got.ContextUserID != "" {
		t.Fatalf("会话上下文未清理: %#v", got)
	}
	if got.SessionEstablishedAt == "" {
		t.Fatalf("prepare failed 清理丢失了「曾就绪」记忆: %#v", got)
	}

	status := GetStatus()
	if status.SessionReady {
		t.Fatalf("SessionReady = true, want false: %#v", status)
	}
	if !status.EverReady {
		t.Fatalf("EverReady = false, want true: %#v", status)
	}
}

func TestMarkSessionAlertedPersists(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	if err := SaveCredentials(boundCredentials()); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	if status := GetStatus(); status.Alerted {
		t.Fatalf("Alerted 初始应为 false: %#v", status)
	}
	if err := MarkSessionAlerted(); err != nil {
		t.Fatalf("MarkSessionAlerted: %v", err)
	}
	if status := GetStatus(); !status.Alerted {
		t.Fatalf("Alerted = false, want true: %#v", status)
	}
}
```

- [ ] **Step 2: 在 session_test.go 扩展轮询测试**

在 `internal/clawbot/session_test.go` 的 `TestPollSessionOncePersistsContextAndCursor` 中，`SaveCredentials(credentials)` 之后、`PollSessionOnce` 之前插入预置提醒标记：

```go
	if err := updateCredentials(func(credentials *Credentials) error {
		credentials.SessionAlertAt = "2026-09-17T16:40:00+08:00"
		return nil
	}); err != nil {
		t.Fatalf("updateCredentials: %v", err)
	}
```

并把该测试末尾的断言块扩展为：

```go
	if updated.ContextToken != "ctx-1" || updated.ContextUserID != "user-1" {
		t.Fatalf("context not persisted: %#v", updated)
	}
	if updated.GetUpdatesBuf != "cursor-1" {
		t.Fatalf("cursor = %q, want cursor-1", updated.GetUpdatesBuf)
	}
	if updated.SessionEstablishedAt == "" {
		t.Fatalf("收到微信消息后未记录会话世代: %#v", updated)
	}
	if updated.SessionAlertAt != "" {
		t.Fatalf("会话恢复后未清空提醒标记: %#v", updated)
	}
```

- [ ] **Step 3: 运行测试并确认失败**

Run:

```powershell
go test ./internal/clawbot/...
```

Expected: FAIL，编译错误 `unknown field 'SessionEstablishedAt'` / `undefined: MarkSessionAlerted`。

- [ ] **Step 4: 给 Credentials 与 Status 增加字段**

在 `internal/clawbot/types.go` 的 `Credentials` 中，`StaleAt` 之后追加：

```go
	// SessionEstablishedAt 记录最近一次成功建立主动推送会话的时间。
	// 登录边界会重置它；会话被服务端回收时保留，用于区分「从未就绪」和「曾经就绪后失效」。
	SessionEstablishedAt string `json:"session_established_at,omitempty"`
	// SessionAlertAt 记录最近一次因「会话失效」提醒过用户的时间，会话恢复时清空。
	SessionAlertAt string `json:"session_alert_at,omitempty"`
```

在同文件 `Status` 中追加：

```go
	// EverReady 表示当前会话未就绪，但历史上建立过会话（即「曾经正常、现在失效」）。
	EverReady bool `json:"everReady,omitempty"`
	// Alerted 表示已针对当前这次断开提醒过用户。
	Alerted bool `json:"alerted,omitempty"`
```

- [ ] **Step 5: 在 SaveCredentials 落实登录边界**

把 `internal/clawbot/auth.go` 的 `SaveCredentials` 改为：

```go
// SaveCredentials writes credentials atomically with restrictive permissions.
// 它同时是登录边界：新登录必须丢掉上一次登录的会话世代与提醒标记，
// 否则会把「曾经就绪」的记忆带进新登录，误报成会话失效。
// 运行期的会话状态写入走 updateCredentials，不受此重置影响。
func SaveCredentials(creds Credentials) error {
	credentialsMu.Lock()
	defer credentialsMu.Unlock()
	creds.SessionEstablishedAt = ""
	creds.SessionAlertAt = ""
	return saveCredentials(creds)
}
```

生产代码中只有 `cmd/agent-notify/main.go` 的 `runLogin` 与 `internal/ui/login_dialog.go` 的 `startLoginFlow` 调用 `SaveCredentials`，因此该重置等价于「登录时重置」。

- [ ] **Step 6: 在 GetStatus 投影新字段**

在 `internal/clawbot/auth.go` 的 `GetStatus` 中，`status.SessionReady = ...` 赋值之后追加：

```go
	status.EverReady = !status.SessionReady && strings.TrimSpace(credentials.SessionEstablishedAt) != ""
	status.Alerted = strings.TrimSpace(credentials.SessionAlertAt) != ""
```

- [ ] **Step 7: 会话恢复时刷新世代并清空提醒标记**

在 `internal/clawbot/session.go` 的 `savePolledSession` 内层闭包中，`credentials.ContextUserID = next.ContextUserID` 之后追加：

```go
		// 收到新的入站上下文即视为会话可用：刷新「曾就绪」时间并解除已提醒标记。
		if strings.TrimSpace(next.ContextToken) != "" {
			credentials.SessionEstablishedAt = time.Now().Format(time.RFC3339)
			credentials.SessionAlertAt = ""
		}
```

- [ ] **Step 8: 新增 MarkSessionAlerted**

在 `internal/clawbot/session.go` 的 `ClearSessionContext` 之后追加：

```go
// MarkSessionAlerted 记录界面已针对当前这次「会话失效」提醒过用户。
// 会话恢复（savePolledSession）会清空该标记，因此同一次断开会话只提醒一次，
// 恢复后再次失效可以重新提醒。
func MarkSessionAlerted() error {
	return updateCredentials(func(credentials *Credentials) error {
		credentials.SessionAlertAt = time.Now().Format(time.RFC3339)
		return nil
	})
}
```

- [ ] **Step 9: 运行测试并确认通过**

Run:

```powershell
go test ./internal/clawbot/...
go vet ./internal/clawbot/...
```

Expected: PASS，无 vet 告警。

- [ ] **Step 10: 提交**

```powershell
git add internal/clawbot/types.go internal/clawbot/auth.go internal/clawbot/session.go internal/clawbot/state_test.go internal/clawbot/session_test.go
git commit -m "feat(clawbot): 记录会话世代并暴露「曾就绪/已提醒」状态"
```

---

### Task 2: ui 微信链路状态与文案映射（纯逻辑）

**Files:**
- Create: `internal/ui/wechat_link.go`
- Test: `internal/ui/wechat_link_test.go`

**Interfaces:**
- Consumes: `clawbot.Status` 的 `LoggedIn` / `Stale` / `SessionReady` / `EverReady`
- Produces: `type wechatLinkState int` 与五个常量
- Produces: `func wechatLinkStateFor(loggedIn, stale, sessionReady, everReady bool) wechatLinkState`
- Produces: `func wechatDockLabel(state wechatLinkState) (label string, emphasize bool)`
- Produces: `func wechatDockAccent(theme ThemePalette, state wechatLinkState) uint32`
- Produces: `func wechatCardText(theme ThemePalette, state wechatLinkState) (label string, dot uint32)`
- Produces: `func wechatLinkCardText(state wechatLinkState) (icon, title, hint string)`

- [ ] **Step 1: 写失败测试**

Create `internal/ui/wechat_link_test.go`:

```go
//go:build windows

package ui

import "testing"

func TestWechatLinkStateFor(t *testing.T) {
	tests := []struct {
		name         string
		loggedIn     bool
		stale        bool
		sessionReady bool
		everReady    bool
		want         wechatLinkState
	}{
		{"未登录优先于其他标记", false, false, false, true, wechatLinkNotLoggedIn},
		{"登录失效优先于会话状态", true, true, false, true, wechatLinkStale},
		{"会话就绪", true, false, true, true, wechatLinkOK},
		{"首次等待：从未就绪不得判为断开", true, false, false, false, wechatLinkAwaitingFirst},
		{"曾就绪后失效", true, false, false, true, wechatLinkBroken},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := wechatLinkStateFor(tt.loggedIn, tt.stale, tt.sessionReady, tt.everReady); got != tt.want {
				t.Fatalf("wechatLinkStateFor() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestWechatDockLabelKeepsLegacyStates(t *testing.T) {
	if label, emphasize := wechatDockLabel(wechatLinkOK); label != "微信配置" || emphasize {
		t.Fatalf("OK dock = (%q,%v), want (微信配置,false)", label, emphasize)
	}
	if label, emphasize := wechatDockLabel(wechatLinkNotLoggedIn); label != "微信未连" || !emphasize {
		t.Fatalf("未登录 dock = (%q,%v), want (微信未连,true)", label, emphasize)
	}
	// 登录失效维持现状：不强调、沿用常规文案。
	if label, emphasize := wechatDockLabel(wechatLinkStale); label != "微信配置" || emphasize {
		t.Fatalf("登录失效 dock = (%q,%v), want (微信配置,false)", label, emphasize)
	}
	if label, emphasize := wechatDockLabel(wechatLinkBroken); label != "推送已断" || !emphasize {
		t.Fatalf("已断开 dock = (%q,%v), want (推送已断,true)", label, emphasize)
	}
	if label, emphasize := wechatDockLabel(wechatLinkAwaitingFirst); label != "待发消息" || !emphasize {
		t.Fatalf("等待首条 dock = (%q,%v), want (待发消息,true)", label, emphasize)
	}
}

func TestWechatDockAccentAndCardText(t *testing.T) {
	if got := wechatDockAccent(ThemeLight, wechatLinkNotLoggedIn); got != ThemeLight.AccentDanger {
		t.Fatalf("未登录 dock 强调色 = %#v, want AccentDanger", got)
	}
	if got := wechatDockAccent(ThemeLight, wechatLinkBroken); got != ThemeLight.AccentWarning {
		t.Fatalf("已断开 dock 强调色 = %#v, want AccentWarning", got)
	}
	if got := wechatDockAccent(ThemeLight, wechatLinkStale); got != 0 {
		t.Fatalf("登录失效 dock 强调色 = %#v, want 0（维持现状）", got)
	}

	if label, dot := wechatCardText(ThemeLight, wechatLinkBroken); label != "主动推送会话已失效" || dot != ThemeLight.AccentWarning {
		t.Fatalf("已断开卡片 = (%q,%#v)", label, dot)
	}
	if label, dot := wechatCardText(ThemeLight, wechatLinkAwaitingFirst); label != "等待微信消息" || dot != ThemeLight.AccentWarning {
		t.Fatalf("等待首条卡片 = (%q,%#v)", label, dot)
	}
	// 登录失效此前误报为绿色「会话正常」，必须修正为红色真实状态。
	if label, dot := wechatCardText(ThemeLight, wechatLinkStale); label != "ClawBot 微信登录已失效" || dot != ThemeLight.AccentDanger {
		t.Fatalf("登录失效卡片 = (%q,%#v)", label, dot)
	}
	if label, dot := wechatCardText(ThemeLight, wechatLinkOK); label != "ClawBot 微信会话正常" || dot != ThemeLight.AccentSuccess {
		t.Fatalf("正常卡片 = (%q,%#v)", label, dot)
	}
}

func TestWechatLinkCardText(t *testing.T) {
	if _, title, _ := wechatLinkCardText(wechatLinkOK); title != "ClawBot 微信已成功连接" {
		t.Fatalf("正常标题 = %q", title)
	}
	if _, title, _ := wechatLinkCardText(wechatLinkAwaitingFirst); title != "已登录，等待第一条消息" {
		t.Fatalf("等待标题 = %q", title)
	}
	_, brokenTitle, brokenHint := wechatLinkCardText(wechatLinkBroken)
	if brokenTitle != "主动推送会话已断开" {
		t.Fatalf("断开标题 = %q", brokenTitle)
	}
	if brokenHint == "" {
		t.Fatal("断开提示为空")
	}
	if _, title, _ := wechatLinkCardText(wechatLinkStale); title != "ClawBot 微信登录已失效" {
		t.Fatalf("登录失效标题 = %q", title)
	}
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
go test ./internal/ui/ -run 'TestWechat' -v
```

Expected: FAIL，编译错误 `undefined: wechatLinkStateFor`。

- [ ] **Step 3: 实现 wechat_link.go**

Create `internal/ui/wechat_link.go`:

```go
//go:build windows

package ui

// wechatLinkState 把 ClawBot 登录与主动推送会话状态收敛为界面可用的单一状态。
// 四处显示与托盘提醒全部只读这个枚举，避免同一状态在多处各判一次、判法还不一致。
type wechatLinkState int

const (
	// wechatLinkOK 已登录且主动推送会话可用。
	wechatLinkOK wechatLinkState = iota
	// wechatLinkAwaitingFirst 已登录但还没收到过微信消息，属于登录流程里的正常等待。
	wechatLinkAwaitingFirst
	// wechatLinkBroken 曾经建立过会话、之后被服务端回收，属于故障。
	wechatLinkBroken
	// wechatLinkNotLoggedIn 没有可用凭据。
	wechatLinkNotLoggedIn
	// wechatLinkStale 凭据已被服务端判定失效。
	wechatLinkStale
)

// wechatLinkStateFor 是四处显示与托盘提醒共用的唯一判定入口。
func wechatLinkStateFor(loggedIn, stale, sessionReady, everReady bool) wechatLinkState {
	switch {
	case !loggedIn:
		return wechatLinkNotLoggedIn
	case stale:
		return wechatLinkStale
	case sessionReady:
		return wechatLinkOK
	case everReady:
		return wechatLinkBroken
	default:
		return wechatLinkAwaitingFirst
	}
}

// wechatDockLabel 返回悬浮窗底部「微信」入口的文案与是否强调。
// 未登录维持原有红色「微信未连」；登录失效维持现状，只由托盘与设置页提示。
func wechatDockLabel(state wechatLinkState) (string, bool) {
	switch state {
	case wechatLinkNotLoggedIn:
		return "微信未连", true
	case wechatLinkAwaitingFirst:
		return "待发消息", true
	case wechatLinkBroken:
		return "推送已断", true
	default:
		return "微信配置", false
	}
}

// wechatDockAccent 返回底部入口的强调色；0 表示不着色（维持现状）。
func wechatDockAccent(theme ThemePalette, state wechatLinkState) uint32 {
	switch state {
	case wechatLinkNotLoggedIn:
		return theme.AccentDanger
	case wechatLinkAwaitingFirst, wechatLinkBroken:
		return theme.AccentWarning
	default:
		return 0
	}
}

// wechatCardText 返回设置页 ClawBot 卡片的状态文案与指示灯颜色。
func wechatCardText(theme ThemePalette, state wechatLinkState) (string, uint32) {
	switch state {
	case wechatLinkNotLoggedIn:
		return "ClawBot 微信未登录", theme.AccentDanger
	case wechatLinkStale:
		return "ClawBot 微信登录已失效", theme.AccentDanger
	case wechatLinkAwaitingFirst:
		return "等待微信消息", theme.AccentWarning
	case wechatLinkBroken:
		return "主动推送会话已失效", theme.AccentWarning
	default:
		return "ClawBot 微信会话正常", theme.AccentSuccess
	}
}

// wechatLinkCardText 返回微信配置页顶部卡片在某种链路状态下的图标、标题与说明。
func wechatLinkCardText(state wechatLinkState) (icon, title, hint string) {
	switch state {
	case wechatLinkAwaitingFirst:
		return "\uE7BA", "已登录，等待第一条消息", "请在微信中给 ClawBot 发送任意一条消息，用于建立主动推送会话。"
	case wechatLinkBroken:
		return "\uE7BA", "主动推送会话已断开", "任务通知暂时发不出去。请在微信中给 ClawBot 发送任意一条消息即可恢复。"
	case wechatLinkStale:
		return "\uE783", "ClawBot 微信登录已失效", "请点击下方「刷新二维码」重新扫码登录。"
	default:
		return "\uE73E", "ClawBot 微信已成功连接", "主动推送链路正常。任务完成后将自动通过微信发送消息。"
	}
}

// wechatLinkAccent 返回微信配置页卡片的强调色。
func wechatLinkAccent(theme ThemePalette, state wechatLinkState) uint32 {
	switch state {
	case wechatLinkOK:
		return theme.AccentSuccess
	case wechatLinkStale, wechatLinkNotLoggedIn:
		return theme.AccentDanger
	default:
		return theme.AccentWarning
	}
}
```

`\uE7BA` 是 Segoe Fluent Icons 的 Warning 字形，`\uE783` 是 Error 字形，`\uE73E` 是现有代码在用的 CheckMark 字形。

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
go test ./internal/ui/ -run 'TestWechat'
gofmt -l internal/ui
```

Expected: PASS，`gofmt -l` 无输出。

- [ ] **Step 5: 提交**

```powershell
git add internal/ui/wechat_link.go internal/ui/wechat_link_test.go
git commit -m "feat(ui): 增加微信链路状态枚举与文案映射"
```

---

### Task 3: ui 四处显示接入

**Files:**
- Modify: `internal/ui/widget.go`（`WidgetApp` 字段、`refreshState`、`health`）
- Modify: `internal/ui/widget_draw.go`（底部 dock 按钮、`recentStatusColor`）
- Modify: `internal/ui/widget_views.go`（`drawSettingsView`、`drawLoginView`）
- Test: `internal/ui/ui_layout_test.go`、`internal/ui/widget_views_test.go`

**Interfaces:**
- Consumes: Task 2 的 `wechatLinkState`、`wechatLinkStateFor`、`wechatDockLabel`、`wechatDockAccent`、`wechatCardText`、`wechatLinkCardText`、`wechatLinkAccent`
- Consumes: `clawbot.GetStatus()` 的 `EverReady` / `Alerted`
- Produces: `WidgetApp.wechatLink wechatLinkState`、`WidgetApp.clawbotEverReady bool`、`WidgetApp.clawbotAlerted bool`
- Produces: `func wechatCardLabelRect() RECT`（绘制与布局测试共用）

- [ ] **Step 1: 写布局溢出测试**

在 `internal/ui/ui_layout_test.go` 的 `TestWidgetTextFitsItsRects` 之后追加：

```go
func TestWechatLabelsFitTheirRects(t *testing.T) {
	layout := widgetLayoutRects()
	card := wechatCardLabelRect()
	checks := []struct {
		name string
		font func() uintptr
		text string
		rect RECT
	}{
		{"dock 正常", newSmallFont, "微信配置", layout.hide},
		{"dock 未登录", newSmallFont, "微信未连", layout.hide},
		{"dock 等待首条", newSmallFont, "待发消息", layout.hide},
		{"dock 已断开", newSmallFont, "推送已断", layout.hide},
		{"卡片未登录", newStrongFont, "ClawBot 微信未登录", card},
		{"卡片登录失效", newStrongFont, "ClawBot 微信登录已失效", card},
		{"卡片等待消息", newStrongFont, "等待微信消息", card},
		{"卡片已断开", newStrongFont, "主动推送会话已失效", card},
	}

	for _, dpi := range []uint32{96, 144, 192} {
		t.Run(fmt.Sprintf("%ddpi", dpi), func(t *testing.T) {
			withUIDPI(t, dpi)
			for _, check := range checks {
				font := check.font()
				measured := measureTextWidth(font, check.text)
				pDeleteObject.Call(font)
				if measured <= 0 {
					t.Fatalf("%s：无法测量文本宽度", check.name)
				}
				limit := scaleFloat(check.rect.Right - check.rect.Left)
				if measured > limit {
					t.Fatalf("%s 在 %d DPI 溢出：文本 %q 需要 %d 像素，可用 %d", check.name, dpi, check.text, measured, limit)
				}
			}
		})
	}
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
go test ./internal/ui/ -run TestWechatLabelsFitTheirRects
```

Expected: FAIL，编译错误 `undefined: wechatCardLabelRect`。

- [ ] **Step 3: 抽出 ClawBot 卡片文本区域**

在 `internal/ui/widget_views.go` 的 `drawSettingsView` 之前追加：

```go
// wechatCardLabelRect 返回设置页 ClawBot 卡片标题文本区域，绘制与布局测试共用。
func wechatCardLabelRect() RECT {
	card := RECT{14, 48, 386, 104}
	return RECT{card.Left + 32, card.Top + 14, card.Left + 250, card.Top + 40}
}
```

并让 `drawSettingsView` 里的 `wechatLabelRect` 改用该函数：

```go
	wechatLabelRect := wechatCardLabelRect()
```

- [ ] **Step 4: 运行测试并确认通过**

Run:

```powershell
go test ./internal/ui/ -run TestWechatLabelsFitTheirRects
```

Expected: PASS（若某一行失败，按失败信息把文案改短，而不是放宽断言）。

- [ ] **Step 5: 给 WidgetApp 增加状态字段**

在 `internal/ui/widget.go` 的 `WidgetApp` 结构体中，`clawbotStale` 之后追加：

```go
	clawbotEverReady bool
	clawbotAlerted   bool
	wechatAlertShown bool
```

**不要**缓存计算结果。`internal/ui/tray_state_test.go` 的 `TestTrayStateFollowsHealthLevels` 直接构造 `WidgetApp{clawbotLoggedIn: ...}` 字面量，若 `health()` 读一个由 `refreshState` 填充的缓存字段，`wechatLink` 会取零值 `wechatLinkOK`，六个用例全部失败。改为按需计算。

- [ ] **Step 6: 增加按需计算的访问器**

在 `internal/ui/widget.go` 的 `health()` 之前追加：

```go
// currentWechatLinkState 按当前字段计算微信链路状态。
// 不缓存结果：health() 与绘制都可能在任何刷新时机被调用，
// 缓存会让「未跑过 refreshState」的调用方拿到错误的零值状态。
func (app *WidgetApp) currentWechatLinkState() wechatLinkState {
	return wechatLinkStateFor(app.clawbotLoggedIn, app.clawbotStale, app.clawbotSessionReady, app.clawbotEverReady)
}
```

- [ ] **Step 7: 在 refreshState 读入新字段**

在 `internal/ui/widget.go` 的 `refreshState` 中，`app.clawbotStale = clawbotStatus.Stale` 之后追加：

```go
	app.clawbotEverReady = clawbotStatus.EverReady
	app.clawbotAlerted = clawbotStatus.Alerted
```

- [ ] **Step 8: 让 health() 复用同一枚举**

把 `internal/ui/widget.go` 中 `health()` 的前四个分支替换为：

```go
	if app.setupError != "" {
		return statusColorWarning, "接入异常"
	}
	switch state := app.currentWechatLinkState(); state {
	case wechatLinkNotLoggedIn:
		return statusColorStopped, "未登录"
	case wechatLinkStale:
		return statusColorStopped, "登录已失效"
	case wechatLinkAwaitingFirst:
		return statusColorWarning, "等待微信消息"
	case wechatLinkBroken:
		return statusColorWarning, "微信推送已断开"
	}
```

其余分支（接入异常、待重启、正常、等待 Agent、全部暂停）保持不变。`TestTrayStateFollowsHealthLevels` 必须继续通过：它构造的字面量没有 `clawbotEverReady`，因此 `everReady=false`，登录成功但未就绪会落到 `wechatLinkAwaitingFirst`（黄），不会误判成断开。

- [ ] **Step 9: 更新底部 dock 按钮**

把 `internal/ui/widget_draw.go` 中 `drawFluentDockButton(hdc, layout.hide, ...)` 之前的判断替换为：

```go
	wechatLabel, wechatActive := wechatDockLabel(app.currentWechatLinkState())
	wechatAccent := wechatDockAccent(theme, app.currentWechatLinkState())
	drawFluentDockButton(hdc, layout.hide, "\uE8BD", wechatLabel, app.hover.hide, wechatActive, wechatAccent, smallFont, iconFont, theme)
```

原来的 `wechatActive := !app.clawbotLoggedIn` 三行（含 `if wechatActive { wechatLabel = "微信未连" }`）全部删除。

- [ ] **Step 10: 更新「最近推送」卡片状态色**

在 `internal/ui/widget_draw.go` 的 `recentStatusColor` 中，`case notify.StatusSuccess` 之后插入：

```go
	case notify.StatusSessionMissing:
		return theme.AccentWarning
```

使「会话未建立」由灰色改为警示色。`StatusSessionMissing` 是 `internal/notify` 已导出的常量。

- [ ] **Step 11: 更新设置页 ClawBot 卡片**

把 `internal/ui/widget_views.go` 的 `drawSettingsView` 中这段：

```go
	wechatDotCol := theme.AccentSuccess
	wechatText := "ClawBot 微信会话正常"
	if !app.clawbotLoggedIn {
		wechatDotCol = theme.AccentDanger
		wechatText = "ClawBot 微信未登录"
	}
```

替换为：

```go
	wechatText, wechatDotCol := wechatCardText(theme, app.currentWechatLinkState())
```

下方「重新扫码 / 立即登录」按钮的文案判断保持原样（`!app.clawbotLoggedIn` → 「立即登录」）。

- [ ] **Step 12: 更新微信配置页卡片**

把 `internal/ui/widget_views.go` 的 `drawLoginView` 中 `if app.clawbotLoggedIn || success { ... }` 分支的内容替换为按状态渲染。保留原有分支条件与 `else`（二维码）分支，只替换卡片内容：

```go
	if app.clawbotLoggedIn || success {
		// 已登录：按微信链路状态区分正常、等待第一条消息、会话已断开、登录已失效。
		switch state := app.currentWechatLinkState(); state {
		case wechatLinkOK, wechatLinkAwaitingFirst, wechatLinkBroken, wechatLinkStale:
			icon, title, hint := wechatLinkCardText(state)
			accent := uintptr(wechatLinkAccent(theme, state))

			pSelectObject.Call(hdc, iconFont)
			pSetTextColor.Call(hdc, accent)
			iconRect := RECT{card.Left + 14, card.Top + 60, card.Right - 14, card.Top + 140}
			DrawText(hdc, icon, &iconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

			pSelectObject.Call(hdc, strongFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
			titleRect := RECT{card.Left + 14, card.Top + 150, card.Right - 14, card.Top + 180}
			DrawText(hdc, title, &titleRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
			hintRect := RECT{card.Left + 24, card.Top + 190, card.Right - 24, card.Top + 240}
			DrawText(hdc, hint, &hintRect, DT_CENTER|DT_WORDBREAK|DT_NOPREFIX)
		default:
			// 未登录等未覆盖状态：沿用原有成功卡片，不改变现有表现。
			pSelectObject.Call(hdc, iconFont)
			pSetTextColor.Call(hdc, uintptr(theme.AccentSuccess))
			checkIconRect := RECT{card.Left + 14, card.Top + 60, card.Right - 14, card.Top + 140}
			DrawText(hdc, "\uE73E", &checkIconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

			pSelectObject.Call(hdc, strongFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
			sTitleRect := RECT{card.Left + 14, card.Top + 150, card.Right - 14, card.Top + 180}
			DrawText(hdc, "ClawBot 微信已成功连接", &sTitleRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
			sHintRect := RECT{card.Left + 24, card.Top + 190, card.Right - 24, card.Top + 240}
			DrawText(hdc, "主动推送链路正常。任务完成后将自动通过微信发送消息。", &sHintRect, DT_CENTER|DT_WORDBREAK|DT_NOPREFIX)
		}
	} else {
```

- [ ] **Step 13: 为配置页文案补布局断言**

在 `internal/ui/widget_views_test.go`（若不存在则在 `ui_layout_test.go` 中新增 `TestWechatLinkCardTextFits`）断言 `wechatLinkCardText` 的四种标题都能放进 `{card.Left + 14, card.Top + 150, card.Right - 14, card.Top + 180}`（宽 344 逻辑像素）：

```go
func TestWechatLinkCardTextFits(t *testing.T) {
	titleRect := RECT{28, 198, 372, 228}
	states := []wechatLinkState{
		wechatLinkOK, wechatLinkAwaitingFirst, wechatLinkBroken, wechatLinkStale,
	}
	for _, dpi := range []uint32{96, 144, 192} {
		t.Run(fmt.Sprintf("%ddpi", dpi), func(t *testing.T) {
			withUIDPI(t, dpi)
			for _, state := range states {
				_, title, _ := wechatLinkCardText(state)
				font := newStrongFont()
				measured := measureTextWidth(font, title)
				pDeleteObject.Call(font)
				if measured <= 0 {
					t.Fatalf("state %d：无法测量标题宽度", state)
				}
				if limit := scaleFloat(titleRect.Right - titleRect.Left); measured > limit {
					t.Fatalf("state %d 标题在 %d DPI 溢出：%q 需要 %d 像素，可用 %d", state, dpi, title, measured, limit)
				}
			}
		})
	}
}
```

- [ ] **Step 14: 运行门禁**

Run:

```powershell
go test ./internal/ui/...
go vet ./internal/ui/...
gofmt -l cmd internal
```

Expected: PASS，`gofmt -l` 无输出。

- [ ] **Step 15: 提交**

```powershell
git add internal/ui/widget.go internal/ui/widget_draw.go internal/ui/widget_views.go internal/ui/ui_layout_test.go internal/ui/widget_views_test.go
git commit -m "feat(ui): 微信连接区域区分等待首条消息与会话已断开"
```

---

### Task 4: 托盘气泡通知

**Files:**
- Modify: `internal/ui/win32.go`（`NIF_INFO`、`NIIF_WARNING` 常量）
- Modify: `internal/ui/tray.go`（`ShowAlert`）
- Test: `internal/ui/tray_state_test.go`

**Interfaces:**
- Produces: `func (manager *TrayManager) ShowAlert(title, body string)`

- [ ] **Step 1: 增加 NOTIFYICON 通知常量**

在 `internal/ui/win32.go` 现有 `NIF_TIP` 之后追加：

```go
	NIF_INFO    = 0x00000010
	NIIF_WARNING = 0x00000002
```

- [ ] **Step 2: 增加常量与截断函数**

在 `internal/ui/tray.go` 中新增：

```go
const (
	// NOTIFYICONDATAW 的 SzInfoTitle 与 SzInfo 容量含结尾 NUL，有效上限是 63 / 255。
	notifInfoTitleLimit = 63
	notifInfoBodyLimit  = 255
)

// truncateNotifText 把通知文案裁剪到 NOTIFYICONDATAW 能容纳的上限。
// copy 到定长数组时超长会被静默截断，先显式裁剪，避免丢掉半个字形后难以排查。
func truncateNotifText(text string, limit int) string {
	runes := []rune(text)
	if len(runes) <= limit {
		return text
	}
	return string(runes[:limit])
}
```

在 `internal/ui/tray_state_test.go` 追加（该文件需补 `strings` 导入）：

```go
func TestTruncateNotifText(t *testing.T) {
	if got := truncateNotifText("微信推送已断开", notifInfoTitleLimit); got != "微信推送已断开" {
		t.Fatalf("短文案被改动: %q", got)
	}
	long := strings.Repeat("断", 300)
	if got := []rune(truncateNotifText(long, notifInfoBodyLimit)); len(got) != notifInfoBodyLimit {
		t.Fatalf("截断长度 = %d, want %d", len(got), notifInfoBodyLimit)
	}
}
```

- [ ] **Step 3: 实现 ShowAlert**

在 `internal/ui/tray.go` 的 `UpdateState` 之后追加：

```go
// ShowAlert 通过托盘图标弹出一条气泡通知，由系统决定展示时长。
// Win10/11 会把气泡并入通知中心，用户错过也能回看。
func (manager *TrayManager) ShowAlert(title, body string) {
	if manager.hwnd == 0 {
		return
	}
	var data NOTIFYICONDATAW
	data.CbSize = uint32(unsafe.Sizeof(data))
	data.HWnd = manager.hwnd
	data.UID = 1
	data.UFlags = NIF_INFO
	data.DwInfoFlags = NIIF_WARNING
	if text, err := syscall.UTF16FromString(truncateNotifText(body, notifInfoBodyLimit)); err == nil {
		copy(data.SzInfo[:], text)
	}
	if text, err := syscall.UTF16FromString(truncateNotifText(title, notifInfoTitleLimit)); err == nil {
		copy(data.SzInfoTitle[:], text)
	}
	pShell_NotifyIconW.Call(NIM_MODIFY, uintptr(unsafe.Pointer(&data)))
}
```

- [ ] **Step 4: 运行测试**

Run:

```powershell
go test ./internal/ui/ -run TestTruncateNotifText
go vet ./internal/ui/...
```

Expected: PASS。

- [ ] **Step 5: 提交**

```powershell
git add internal/ui/win32.go internal/ui/tray.go internal/ui/tray_state_test.go
git commit -m "feat(ui): 托盘图标支持气泡提醒"
```

---

### Task 5: 一次性断线提醒接线

**Files:**
- Modify: `internal/ui/widget.go`（`refreshState` 末尾）
- Test: `internal/ui/wechat_link_test.go`

**Interfaces:**
- Consumes: `TrayManager.ShowAlert`、`clawbot.MarkSessionAlerted`、`WidgetApp.currentWechatLinkState()` / `clawbotAlerted` / `wechatAlertShown`
- Produces: `func (app *WidgetApp) maybeAlertWechatBroken()`
- Produces: `func shouldAlertWechatBroken(state wechatLinkState, alerted, shownInRun bool) bool`

- [ ] **Step 1: 写失败测试**

在 `internal/ui/wechat_link_test.go` 追加。该测试不触网、不建窗口，只验证触发判定本身，因此把判定抽成纯函数：

```go
func TestShouldAlertWechatBroken(t *testing.T) {
	tests := []struct {
		name       string
		state      wechatLinkState
		alerted    bool
		shownInRun bool
		want       bool
	}{
		{"首次断开应提醒", wechatLinkBroken, false, false, true},
		{"已提醒过不再提醒", wechatLinkBroken, true, false, false},
		{"本次运行已弹过不再提醒", wechatLinkBroken, false, true, false},
		{"等待首条不提醒", wechatLinkAwaitingFirst, false, false, false},
		{"正常不提醒", wechatLinkOK, false, false, false},
		{"未登录不提醒", wechatLinkNotLoggedIn, false, false, false},
		{"登录失效不提醒", wechatLinkStale, false, false, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := shouldAlertWechatBroken(tt.state, tt.alerted, tt.shownInRun); got != tt.want {
				t.Fatalf("shouldAlertWechatBroken() = %v, want %v", got, tt.want)
			}
		})
	}
}
```

在 `internal/ui/wechat_link.go` 追加实现：

```go
// shouldAlertWechatBroken 判定是否应为「会话已断开」弹出提醒。
// alerted 是持久化标记（跨重启去重），shownInRun 是进程内标记
// （持久化写入失败时兜底，避免每 5 秒重复弹窗）。
func shouldAlertWechatBroken(state wechatLinkState, alerted, shownInRun bool) bool {
	return state == wechatLinkBroken && !alerted && !shownInRun
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
go test ./internal/ui/ -run TestShouldAlertWechatBroken
```

Expected: FAIL，编译错误 `undefined: shouldAlertWechatBroken`（实现后转为 PASS）。

- [ ] **Step 3: 实现接线**

在 `internal/ui/widget.go` 末尾追加：

```go
const (
	wechatBrokenAlertTitle = "微信推送已断开"
	wechatBrokenAlertBody  = "ClawBot 主动推送会话失效，任务通知暂时发不出去了。请在微信里给 ClawBot 发任意一条消息即可恢复。"
)

// maybeAlertWechatBroken 在「曾就绪但会话失效」首次出现时弹一次托盘气泡。
// 会话恢复后 refreshState 会把 wechatAlertShown 复位，因此再次断开可以重新提醒。
func (app *WidgetApp) maybeAlertWechatBroken() {
	state := app.currentWechatLinkState()
	if state != wechatLinkBroken {
		app.wechatAlertShown = false
		return
	}
	if !shouldAlertWechatBroken(state, app.clawbotAlerted, app.wechatAlertShown) {
		return
	}
	// 悬浮窗还没建好托盘图标时不弹，也不记已弹标记，留给下一次刷新重试。
	if app.tray == nil {
		return
	}
	app.wechatAlertShown = true
	app.tray.ShowAlert(wechatBrokenAlertTitle, wechatBrokenAlertBody)
	if err := clawbot.MarkSessionAlerted(); err != nil {
		errorLog("微信断开提醒标记写入失败: %v", err)
	}
}
```

- [ ] **Step 4: 在 refreshState 末尾调用**

在 `internal/ui/widget.go` 的 `refreshState` 中，托盘状态更新之后追加：

```go
	app.maybeAlertWechatBroken()
```

- [ ] **Step 5: 增加错误日志辅助**

`internal/ui/widget.go` 已有写入 `widget-trace.log` 的 `debugLog`。在其后追加：

```go
// errorLog 记录悬浮窗的可见错误。写入失败只影响诊断，不影响主流程。
func errorLog(format string, args ...interface{}) {
	paths := config.GetPaths()
	entry := fmt.Sprintf("[%s] [PID:%d] %s\r\n", time.Now().Format("2006-01-02T15:04:05"), os.Getpid(), fmt.Sprintf(format, args...))
	diag.Append(paths.WidgetErrorLog, entry)
}
```

- [ ] **Step 6: 运行门禁**

Run:

```powershell
go test ./...
go vet ./...
gofmt -l cmd internal
```

Expected: PASS，`gofmt -l` 无输出。

- [ ] **Step 7: 提交**

```powershell
git add internal/ui/widget.go internal/ui/wechat_link.go internal/ui/wechat_link_test.go
git commit -m "feat(ui): 会话断开时弹一次托盘提醒"
```

---

### Task 6: 文档同步

**Files:**
- Modify: `docs/TROUBLESHOOTING.md`

- [ ] **Step 1: 更新日志表说明**

把 `docs/TROUBLESHOOTING.md` 日志表中 `widget-error.log` 一行改为：

```
| `widget-error.log` | 悬浮窗 | UI 或消息循环错误，以及微信断开提醒标记写入失败 |
```

- [ ] **Step 2: 在「微信完全收不到」补充新入口**

在第 5 步查看 `push.log` 之后插入一条：

```
6. 若通知因为 ClawBot 主动推送会话失效而发不出，悬浮窗会弹一次托盘气泡「微信推送已断开」，底部「微信」入口显示「推送已断」，设置页与微信配置页同步显示真实状态。按提示在微信中给 ClawBot 发送任意一条消息即可恢复；恢复前不会重复弹出同一提醒。
```

- [ ] **Step 3: 提交**

```powershell
git add docs/TROUBLESHOOTING.md
git commit -m "docs: 补充微信会话断开的提醒入口与恢复方式"
```

---

### Task 7: 真实验收

**前置：** 先跑完整门禁。

```powershell
go test ./...
go vet ./...
gofmt -l cmd internal
node_modules\.bin\tsc.cmd --noEmit
tools\test.ps1
tools\lint.ps1
```

- [ ] **Step 1: 编译并把新版放到实际运行路径**

当前悬浮窗运行的是 `C:\Users\srafy\bin\agent-notify.exe`，编译前必须先从托盘完全退出 Agent-notify，否则可执行文件被占用会编译失败。

```powershell
go build -o C:\Users\srafy\bin\agent-notify.exe .\cmd\agent-notify
```

Expected: 编译成功，无输出。

- [ ] **Step 2: 正常态验收**

重新启动 `C:\Users\srafy\bin\agent-notify.exe`，确认：

- 底部「微信」入口显示「微信配置」，非强调色。
- 设置页 ClawBot 卡片绿点「ClawBot 微信会话正常」。
- 微信配置页显示「ClawBot 微信已成功连接 / 主动推送链路正常」。
- 无气泡弹出。
- 微信里收到一条真实测试推送（运行 `agent-notify test`）。

- [ ] **Step 3: 断开态验收**

关闭悬浮窗，手工把 `%USERPROFILE%\.config\agent-notify\clawbot.json` 的 `context_token` / `context_user_id` 清空，但保留 `session_established_at`；重新启动悬浮窗。

Expected:

- 立即（≤5 秒）弹出托盘气泡「微信推送已断开」。
- 底部「微信」入口显示「推送已断」。
- 设置页卡片黄点「主动推送会话已失效」。
- 微信配置页显示「主动推送会话已断开」+ 恢复提示。
- 等待 1 分钟，气泡不再重复弹出。
- 完全退出并重启悬浮窗，气泡**不**再弹出（持久化去重生效）。

- [ ] **Step 4: 恢复态验收**

在微信里给 ClawBot 发送任意一条消息。Expected:

- 5 秒内底部入口恢复「微信配置」、设置页恢复绿点、配置页恢复「已成功连接」。
- `clawbot.json` 的 `session_alert_at` 被清空、`session_established_at` 刷新。
- 微信里能收到真实的 Agent 任务推送（跑一个 OpenCode 任务验证端到端）。

- [ ] **Step 5: 恢复后再次断开**

重复 Step 3 的断开构造。Expected: 气泡**再次**弹出（验证「恢复后重置」），界面重新进入断开态。

- [ ] **Step 6: 首次等待态验收**

完全退出悬浮窗，删掉 `clawbot.json` 里的 `session_established_at`，清空 `context_token`，重启。

Expected: 界面显示「待发消息」/「等待微信消息」/「已登录，等待第一条消息」，**不弹气泡**。

- [ ] **Step 7: 登录失效态验收**

把 `clawbot.json` 的 `stale_at` 设为一个时间戳，重启悬浮窗。

Expected: 设置页卡片红点「ClawBot 微信登录已失效」，微信配置页显示「ClawBot 微信登录已失效」并提示重新扫码，**不弹气泡**，底部入口维持「微信配置」。

- [ ] **Step 8: 记录验收结果**

把四条链路（正常 / 断开 / 恢复 / 首次等待）的观察结果写进本次提交的 PR 描述或会话记录；若任一条不通过，回到对应 Task 修复，不要带着未验证的状态合并。

---

## 不在本计划范围

- 版本号、`CHANGELOG.md`、tag 与 Release：发版是独立流程，需要单独确认后再做（版本号唯一来源是 `internal/app/version.go`）。
- 「未登录 / 登录失效」的气泡提醒：本次只做界面显示修正。
- 网络错误、服务端限流等其他推送失败的提醒。
