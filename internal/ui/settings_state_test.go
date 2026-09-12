//go:build windows

package ui

import (
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

func TestSettingsConnectionState(t *testing.T) {
	tests := []struct {
		name       string
		status     clawbot.Status
		wantTitle  string
		wantDetail string
		wantLogin  string
	}{
		{
			name:       "not logged in",
			status:     clawbot.Status{},
			wantTitle:  "ClawBot 未连接",
			wantDetail: "扫码登录后，还需发送一条微信消息",
			wantLogin:  "扫码登录",
		},
		{
			name:       "login expired",
			status:     clawbot.Status{LoggedIn: true, Stale: true},
			wantTitle:  "ClawBot 登录已失效",
			wantDetail: "请重新扫码登录",
			wantLogin:  "重新登录",
		},
		{
			name:       "waiting for message",
			status:     clawbot.Status{LoggedIn: true, UserHint: "user...1234"},
			wantTitle:  "已登录，等待微信消息",
			wantDetail: "请给 ClawBot 发送一条消息建立会话",
			wantLogin:  "重新登录",
		},
		{
			name:       "session ready",
			status:     clawbot.Status{LoggedIn: true, SessionReady: true, UserHint: "user...1234"},
			wantTitle:  "ClawBot 已连接",
			wantDetail: "主动推送会话已就绪 · user...1234",
			wantLogin:  "重新登录",
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			login, title, detail, _, _ := settingsConnectionState(test.status)
			if login != test.wantLogin || title != test.wantTitle || detail != test.wantDetail {
				t.Fatalf("settingsConnectionState() = (%q, %q, %q), want (%q, %q, %q)", login, title, detail, test.wantLogin, test.wantTitle, test.wantDetail)
			}
		})
	}
}
