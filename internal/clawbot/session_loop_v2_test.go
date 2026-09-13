package clawbot

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestRunSessionLoopRefreshesSessionAndAnnouncesStart(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	messages := make(chan struct{}, 1)
	started := make(chan struct{}, 1)
	loopErrors := make(chan error, 1)

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/ilink/bot/getupdates":
			_ = json.NewEncoder(w).Encode(map[string]any{
				"ret":             0,
				"get_updates_buf": "cursor-1",
				"msgs": []map[string]any{{
					"from_user_id":  "user-1",
					"message_type":  MessageTypeUser,
					"context_token": "ctx-1",
				}},
			})
		case "/ilink/bot/msg/notifystart":
			_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
			select {
			case started <- struct{}{}:
			default:
			}
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.ContextToken = ""
	credentials.ContextUserID = ""
	credentials.BaseURL = server.URL
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		RunSessionLoop(ctx, func(InboundMessage) {
			select {
			case messages <- struct{}{}:
			default:
			}
		}, func(err error) {
			select {
			case loopErrors <- err:
			default:
			}
		})
	}()

	waitClawbotSignal(t, "private message", messages)
	waitClawbotSignal(t, "notifystart", started)
	cancel()
	waitClawbotSignal(t, "session loop exit", done)

	select {
	case err := <-loopErrors:
		t.Fatalf("RunSessionLoop reported error: %v", err)
	default:
	}
}

func TestStopSessionLifecycleSendsStopForMatchingToken(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	stopped := make(chan struct{}, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ilink/bot/msg/notifystop" {
			http.NotFound(w, r)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
		select {
		case stopped <- struct{}{}:
		default:
		}
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.BaseURL = server.URL
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	stopSessionLifecycle("token-1")
	waitClawbotSignal(t, "notifystop", stopped)
}

func waitClawbotSignal(t *testing.T, name string, signal <-chan struct{}) {
	t.Helper()
	select {
	case <-signal:
	case <-time.After(2 * time.Second):
		t.Fatalf("timed out waiting for %s", name)
	}
}
