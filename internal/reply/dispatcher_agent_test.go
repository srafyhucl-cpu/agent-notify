package reply

import (
	"fmt"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

func TestDispatcherRegistersDesktopAgentSenders(t *testing.T) {
	dispatcher := NewDispatcher(DispatcherOptions{})
	if _, ok := dispatcher.senders[agentmeta.Antigravity].(AntigravityAgentAPISender); !ok {
		t.Fatalf("Antigravity sender = %#v", dispatcher.senders[agentmeta.Antigravity])
	}

	sender, ok := dispatcher.senders[agentmeta.Devin].(DevinReplySender)
	if !ok {
		t.Fatalf("Devin sender = %#v", dispatcher.senders[agentmeta.Devin])
	}
	runner, ok := sender.Queue.(DevinQueueRunner)
	if !ok || runner.OnAsyncFailure == nil {
		t.Fatalf("Devin runner = %#v, want async failure reporter", sender.Queue)
	}
}

func TestDispatcherWiresDevinAsyncFailureReporter(t *testing.T) {
	dispatcher, _, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	sender := dispatcher.senders[agentmeta.Devin].(DevinReplySender)
	runner := sender.Queue.(DevinQueueRunner)
	runner.OnAsyncFailure("session-1", "继续", fmt.Errorf("command not found"))

	if len(*failures) != 1 ||
		!strings.Contains((*failures)[0], "Devin") ||
		!strings.Contains((*failures)[0], "command not found") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}
