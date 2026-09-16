//go:build windows

package ui

import (
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
)

// 托盘图标必须与窗口顶部状态条共用 health() 的分级：
// 登录成功但微信会话未就绪属于等待（黄），不能误判成停止（红）。
func TestTrayStateFollowsHealthLevels(t *testing.T) {
	cases := []struct {
		name  string
		build func() WidgetApp
		want  int
	}{
		{
			name: "未登录为停止",
			build: func() WidgetApp {
				return WidgetApp{onCodex: true, integrations: connectedCodex()}
			},
			want: widgetTrayStateStopped,
		},
		{
			name: "登录失效为停止",
			build: func() WidgetApp {
				return WidgetApp{clawbotLoggedIn: true, clawbotStale: true}
			},
			want: widgetTrayStateStopped,
		},
		{
			name: "等待微信消息为部分就绪",
			build: func() WidgetApp {
				return WidgetApp{clawbotLoggedIn: true, onCodex: true, integrations: connectedCodex()}
			},
			want: widgetTrayStatePartial,
		},
		{
			name: "全部暂停为停止",
			build: func() WidgetApp {
				return WidgetApp{clawbotLoggedIn: true, clawbotSessionReady: true}
			},
			want: widgetTrayStateStopped,
		},
		{
			name: "接入异常为部分就绪",
			build: func() WidgetApp {
				return WidgetApp{
					clawbotLoggedIn:     true,
					clawbotSessionReady: true,
					onCodex:             true,
					integrations: map[string]integration.Status{
						agentmeta.Codex: {Agent: agentmeta.Codex, Enabled: true, State: integration.StateError},
					},
				}
			},
			want: widgetTrayStatePartial,
		},
		{
			name: "全部就绪为就绪",
			build: func() WidgetApp {
				return WidgetApp{
					clawbotLoggedIn:     true,
					clawbotSessionReady: true,
					onCodex:             true,
					integrations:        connectedCodex(),
				}
			},
			want: widgetTrayStateReady,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			app := tc.build()
			statusColor, _ := app.health()
			if got := trayStateForStatus(statusColor); got != tc.want {
				t.Fatalf("tray state = %d, want %d（health 颜色 %#x）", got, tc.want, statusColor)
			}
		})
	}
}

func connectedCodex() map[string]integration.Status {
	return map[string]integration.Status{
		agentmeta.Codex: {Agent: agentmeta.Codex, Enabled: true, State: integration.StateConnected},
	}
}
