package reply

import (
	"context"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

type recordingReplySender struct {
	sessionID string
	text      string
	calls     int
}

func (s *recordingReplySender) Send(_ context.Context, sessionID, text string) error {
	s.calls++
	s.sessionID = sessionID
	s.text = text
	return nil
}

func TestReplySenderAdaptersDelegate(t *testing.T) {
	codex := &fakeCodexQueue{}
	if err := (CodexReplySender{Queue: codex}).Send(context.Background(), "thread-1", "继续"); err != nil {
		t.Fatalf("CodexReplySender: %v", err)
	}
	if len(codex.threadIDs) != 1 || codex.threadIDs[0] != "thread-1" || codex.messages[0] != "继续" {
		t.Fatalf("Codex delegate = %#v / %#v", codex.threadIDs, codex.messages)
	}

	openCode := &fakeOpenCodeQueue{}
	if err := (OpenCodeReplySender{Queue: openCode}).Send(context.Background(), "session-1", "继续"); err != nil {
		t.Fatalf("OpenCodeReplySender: %v", err)
	}
	if len(openCode.threadIDs) != 1 || openCode.threadIDs[0] != "session-1" || openCode.messages[0] != "继续" {
		t.Fatalf("OpenCode delegate = %#v / %#v", openCode.threadIDs, openCode.messages)
	}
}

func TestReplySenderAdaptersRejectMissingQueue(t *testing.T) {
	tests := []struct {
		name string
		send func() error
		want string
	}{
		{
			name: "codex",
			send: func() error {
				return (CodexReplySender{}).Send(context.Background(), "thread-1", "继续")
			},
			want: "codex reply: queue is not configured",
		},
		{
			name: "opencode",
			send: func() error {
				return (OpenCodeReplySender{}).Send(context.Background(), "session-1", "继续")
			},
			want: "opencode reply: queue is not configured",
		},
	}

	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			err := testCase.send()
			if err == nil || !strings.Contains(err.Error(), testCase.want) {
				t.Fatalf("Send error = %v, want %q", err, testCase.want)
			}
		})
	}
}

func TestDispatcherNormalizesAndPreservesRegisteredSender(t *testing.T) {
	recorder := &recordingReplySender{}
	dispatcher := NewDispatcher(DispatcherOptions{
		Senders: map[string]ReplySender{
			"  CoDeX  ": recorder,
		},
	})

	if got := dispatcher.senders["codex"]; got != recorder {
		t.Fatalf("codex sender = %#v, want injected sender", got)
	}
	if _, ok := dispatcher.senders["opencode"].(OpenCodeReplySender); !ok {
		t.Fatalf("opencode sender = %#v, want default adapter", dispatcher.senders["opencode"])
	}
}

func TestDispatcherUsesRegisteredReplySender(t *testing.T) {
	dispatcher, _, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	recorder := &recordingReplySender{}
	dispatcher.senders["custom"] = recorder
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-custom",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "Custom",
		SessionID: "session-custom",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-custom", "platform-custom", "继续"))
	if recorder.calls != 1 || recorder.sessionID != "session-custom" || recorder.text != "继续" {
		t.Fatalf("registered sender = %#v", recorder)
	}
	if len(*failures) != 0 {
		t.Fatalf("unexpected failure notices: %#v", *failures)
	}
}
