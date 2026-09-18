//go:build windows

package ui

import "testing"

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
		t.Fatalf("正常 dock = (%q,%v), want (微信配置,false)", label, emphasize)
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
