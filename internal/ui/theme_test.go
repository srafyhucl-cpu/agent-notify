//go:build windows

package ui

import (
	"testing"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

func TestGetTheme(t *testing.T) {
	dark := GetTheme("dark")
	if !dark.IsDark || dark.Name != "dark" {
		t.Fatalf("GetTheme(dark) = %+v, want dark theme", dark)
	}

	light := GetTheme("light")
	if light.IsDark || light.Name != "light" {
		t.Fatalf("GetTheme(light) = %+v, want light theme", light)
	}

	def := GetTheme("")
	if !def.IsDark || def.Name != "dark" {
		t.Fatalf("GetTheme(\"\") = %+v, want dark theme by default", def)
	}

	invalid := GetTheme("random_unknown")
	if !invalid.IsDark || invalid.Name != "dark" {
		t.Fatalf("GetTheme(random) = %+v, want dark theme fallback", invalid)
	}
}

func TestWidgetAppToggleTheme(t *testing.T) {
	// toggleTheme 会写 config.json，必须隔离到临时目录，避免污染真实用户配置。
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	if err := config.SaveConfig(config.AppConfig{Theme: "dark"}, ""); err != nil {
		t.Fatalf("SaveConfig: %v", err)
	}

	app := WidgetApp{
		theme: "dark",
	}

	app.toggleTheme()
	if app.theme != "light" {
		t.Fatalf("app.theme after first toggle = %q, want light", app.theme)
	}
	currentTheme := app.getTheme()
	if currentTheme.IsDark {
		t.Fatalf("currentTheme.IsDark = true, want false")
	}

	app.toggleTheme()
	if app.theme != "dark" {
		t.Fatalf("app.theme after second toggle = %q, want dark", app.theme)
	}
	currentTheme = app.getTheme()
	if !currentTheme.IsDark {
		t.Fatalf("currentTheme.IsDark = false, want true")
	}
}

func TestWidgetAppSwitchView(t *testing.T) {
	// 标记已登录，避免切换视图时真的发起扫码登录流程（本测试不接触网络）。
	app := WidgetApp{
		currentView:     WidgetViewDashboard,
		clawbotLoggedIn: true,
	}

	views := []WidgetView{
		WidgetViewRepair,
		WidgetViewHistory,
		WidgetViewSettings,
		WidgetViewLogin,
		WidgetViewDashboard,
	}

	for _, v := range views {
		app.switchView(v)
		if app.currentView != v {
			t.Fatalf("switchView(%v) = %v, want %v", v, app.currentView, v)
		}
	}
}

// 离开登录视图必须取消扫码流程，否则后台会继续轮询并可能在没有界面的情况下写凭据。
func TestSwitchViewCancelsLoginFlow(t *testing.T) {
	app := WidgetApp{currentView: WidgetViewLogin, clawbotLoggedIn: true}
	_, ctx := app.loginState.begin(time.Minute)
	app.verifyPrompt = true

	app.switchView(WidgetViewDashboard)

	if ctx.Err() == nil {
		t.Fatal("离开登录视图后登录流程仍在运行")
	}
	if app.verifyPrompt {
		t.Fatal("离开登录视图后配对码输入状态未重置")
	}
}

func TestCleanRecentPushTitleWithEmojis(t *testing.T) {
	cases := []struct {
		title string
		agent string
		want  string
	}{
		{"⚡【测试】Agent-notify 微信链路验证", "test", "Agent-notify 微信链路验证"},
		{"🟢【Antigravity】构建成功", "antigravity", "构建成功"},
		{"⚠️【Codex】需要人工介入", "codex", "需要人工介入"},
		{"🔴【Devin】任务超时", "devin", "任务超时"},
		{"✨【OpenCode】重构完成", "opencode", "重构完成"},
		{"普通消息无前缀", "antigravity", "普通消息无前缀"},
	}

	for _, c := range cases {
		got := cleanRecentPushTitle(c.title, c.agent)
		if got != c.want {
			t.Fatalf("cleanRecentPushTitle(%q, %q) = %q, want %q", c.title, c.agent, got, c.want)
		}
	}
}

func TestHistoryHeaderButtons(t *testing.T) {
	// When total <= pageSize (5)
	prev, next, clear := historyHeaderButtons(3, 5)
	if prev.Right != 0 || next.Right != 0 {
		t.Fatalf("historyHeaderButtons for 3 items should have empty prev/next buttons")
	}
	if clear != (RECT{336, 12, 364, 38}) {
		t.Fatalf("historyHeaderButtons clear = %+v, want standard right clearBtn", clear)
	}

	// When total > pageSize (e.g. 12 > 5)
	prev, next, clear = historyHeaderButtons(12, 5)
	if prev != (RECT{288, 12, 314, 38}) {
		t.Fatalf("historyHeaderButtons prev = %+v, want 288..314", prev)
	}
	if next != (RECT{320, 12, 346, 38}) {
		t.Fatalf("historyHeaderButtons next = %+v, want 320..346", next)
	}
	if clear != (RECT{256, 12, 284, 38}) {
		t.Fatalf("historyHeaderButtons clear = %+v, want shifted clearBtn", clear)
	}
}

func TestIsSubviewHeaderDrag(t *testing.T) {
	back, _, closeRect := subviewCommonHeader()

	// Clicks on back and close buttons must NOT trigger drag
	if isSubviewHeaderDrag(back.Left+5, back.Top+5) {
		t.Fatalf("isSubviewHeaderDrag on back button returned true, want false")
	}
	if isSubviewHeaderDrag(closeRect.Left+5, closeRect.Top+5) {
		t.Fatalf("isSubviewHeaderDrag on close button returned true, want false")
	}

	// Clicks outside top header (y >= 48) must NOT trigger drag
	if isSubviewHeaderDrag(100, 60) {
		t.Fatalf("isSubviewHeaderDrag below header returned true, want false")
	}

	// Clicks in empty title area (e.g. x=100, y=20) MUST trigger drag
	if !isSubviewHeaderDrag(100, 20) {
		t.Fatalf("isSubviewHeaderDrag on title area returned false, want true")
	}

	// Extra button (e.g. clearBtn) must NOT trigger drag
	clearBtn := RECT{256, 12, 284, 38}
	if isSubviewHeaderDrag(clearBtn.Left+5, clearBtn.Top+5, clearBtn) {
		t.Fatalf("isSubviewHeaderDrag on extra button returned true, want false")
	}
}
