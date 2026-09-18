package notify

import (
	"strings"
	"testing"
	"time"
)

func TestNotificationFooterShowsReplyWindow(t *testing.T) {
	footer := notificationFooter("commandcode", time.Date(2026, time.September, 18, 14, 2, 0, 0, time.FixedZone("CST", 8*60*60)), 60)
	const want = "—\n*引用此消息可继续对话（60 秒内）* · 09/18 14:02"
	if footer != want {
		t.Fatalf("footer = %q, want %q", footer, want)
	}
}

func TestNotificationFooterUsesShortDividerAndUprightTime(t *testing.T) {
	footer := notificationFooter("opencode", time.Date(2026, time.September, 18, 14, 2, 0, 0, time.FixedZone("CST", 8*60*60)), 0)
	const want = "—\n*引用此消息可继续对话* · 09/18 14:02"
	if footer != want {
		t.Fatalf("footer = %q, want %q", footer, want)
	}
}

func TestRenderNotificationFormat(t *testing.T) {
	now := time.Date(2026, time.September, 13, 15, 18, 0, 0, time.FixedZone("CST", 8*60*60))
	tests := []struct {
		name    string
		opts    NotifyOptions
		message string
	}{
		{
			name: "codex",
			opts: NotifyOptions{
				Agent:   "codex",
				Title:   "评估微信消息转发到Agent",
				Summary: "摘要正文……",
			},
			message: "**🟢 Codex｜评估微信消息转发到Agent**\n\n摘要正文……\n\n—\n*引用此消息可继续对话* · 09/13 15:18",
		},
		{
			name: "opencode",
			opts: NotifyOptions{
				Agent:   "opencode",
				Title:   "会话标题",
				Summary: "摘要正文……",
			},
			message: "**🟢 OpenCode｜会话标题**\n\n摘要正文……\n\n—\n*引用此消息可继续对话* · 09/13 15:18",
		},
	}

	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			rendered := renderNotification(testCase.opts, now)
			if rendered.Message != testCase.message {
				t.Fatalf("Message = %q, want %q", rendered.Message, testCase.message)
			}
		})
	}
}

func TestRenderNotificationKeepsLongProbe(t *testing.T) {
	const probeRunes = 3086
	marker := "末尾-3086"
	body := strings.Repeat("字", probeRunes-runeCount(marker)) + marker
	rendered := renderNotification(NotifyOptions{
		Agent:   "codex",
		Title:   "长消息探针",
		Summary: body,
	}, time.Unix(0, 0))
	if !strings.Contains(rendered.Message, marker) || !strings.Contains(rendered.Message, notificationDivider) {
		t.Fatalf("last marker was truncated: %q", rendered.Message[len(rendered.Message)-40:])
	}
	if runeCount(rendered.Summary) != probeRunes {
		t.Fatalf("summary runes = %d, want %d", runeCount(rendered.Summary), probeRunes)
	}
}

func TestRenderNotificationExplicitLimitIncludesTitleAndFooter(t *testing.T) {
	rendered := renderNotification(NotifyOptions{
		Agent:    "codex",
		Title:    "标题",
		Summary:  strings.Repeat("正文", 200),
		MaxChars: 60,
	}, time.Unix(0, 0))
	if got := runeCount(rendered.Message); got > 60 {
		t.Fatalf("message runes = %d, want <= 60 (%q)", got, rendered.Message)
	}
	if !strings.Contains(rendered.Message, replyHintText) {
		t.Fatalf("footer missing from fitted message: %q", rendered.Message)
	}
}

func TestRenderNotificationIncludesNotice(t *testing.T) {
	now := time.Date(2026, time.September, 13, 15, 18, 0, 0, time.Local)
	rendered := renderNotification(NotifyOptions{
		Agent:   "codex",
		Title:   "标题",
		Summary: "正文",
		Notice:  "标题读取失败：数据库不可读。",
	}, now)
	want := "**⚠️ Codex｜标题**\n\n正文\n\n> ⚠️ 标题读取失败：数据库不可读。\n\n—\n*引用此消息可继续对话* · 09/13 15:18"
	if rendered.Message != want {
		t.Fatalf("Message = %q, want %q", rendered.Message, want)
	}
}

func TestSendNotificationProtocolPolicy(t *testing.T) {
	t.Run("normal heartbeat is skipped", func(t *testing.T) {
		result := SendNotification(NotifyOptions{
			Agent:   "codex",
			Title:   "心跳",
			Summary: "<heartbeat><decision>DONT_NOTIFY</decision><message>正常</message></heartbeat>",
			DryRun:  true,
		})
		if result.Status != StatusSkipped {
			t.Fatalf("result = %#v", result)
		}
	})

	t.Run("abnormal heartbeat is rendered without tags", func(t *testing.T) {
		result := SendNotification(NotifyOptions{
			Agent:   "codex",
			Title:   "心跳",
			Summary: "<heartbeat><decision>NOTIFY</decision><message>推送失败</message></heartbeat>",
			DryRun:  true,
		})
		if result.Status != StatusDryRun {
			t.Fatalf("result = %#v", result)
		}
		if strings.Contains(result.DryRunPayload, "<heartbeat>") || !strings.Contains(result.DryRunPayload, "推送失败") {
			t.Fatalf("dry-run payload = %q", result.DryRunPayload)
		}
	})

	t.Run("dont notify wins", func(t *testing.T) {
		result := SendNotification(NotifyOptions{
			Agent:   "codex",
			Title:   "静默",
			Summary: "正文<decision>DONT_NOTIFY</decision>",
			DryRun:  true,
		})
		if result.Status != StatusSkipped {
			t.Fatalf("result = %#v", result)
		}
	})
}

// SessionName 供引用送达确认复用，应剥离旧前缀与徽标。
func TestRenderNotificationReturnsSessionName(t *testing.T) {
	rendered := renderNotification(NotifyOptions{Agent: "codex", Title: "【codex】重构登录页", Summary: "x"}, time.Unix(0, 0))
	if rendered.SessionName != "重构登录页" {
		t.Fatalf("SessionName = %q, want %q", rendered.SessionName, "重构登录页")
	}
}

func TestRenderNotificationDefaultTitles(t *testing.T) {
	now := time.Date(2026, time.September, 13, 15, 18, 0, 0, time.FixedZone("CST", 8*60*60))
	tests := []struct {
		agent   string
		message string
	}{
		{agent: "codex", message: "**🟢 Codex｜任务已完成**\n\n任务已完成。\n\n—\n*引用此消息可继续对话* · 09/13 15:18"},
		{agent: "opencode", message: "**🟢 OpenCode｜任务已完成**\n\n任务已完成。\n\n—\n*引用此消息可继续对话* · 09/13 15:18"},
		{agent: "", message: "**🟢 通知｜任务已完成**\n\n任务已完成。"},
	}
	for _, testCase := range tests {
		rendered := renderNotification(NotifyOptions{Agent: testCase.agent}, now)
		if rendered.Message != testCase.message {
			t.Fatalf("agent %q Message = %q, want %q", testCase.agent, rendered.Message, testCase.message)
		}
	}
}
