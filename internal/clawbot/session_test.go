package clawbot

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestPollSessionOncePersistsContextAndCursor(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ilink/bot/getupdates" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Header.Get("Authorization") != "Bearer token-1" {
			t.Fatalf("unexpected authorization: %s", r.Header.Get("Authorization"))
		}
		var payload getUpdatesRequest
		_ = json.NewDecoder(r.Body).Decode(&payload)
		if payload.GetUpdatesBuf != "cursor-0" {
			t.Fatalf("cursor = %q, want cursor-0", payload.GetUpdatesBuf)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret":             0,
			"get_updates_buf": "cursor-1",
			"msgs": []map[string]any{{
				"from_user_id":  "user-1",
				"to_user_id":    "bot-1",
				"message_type":  MessageTypeUser,
				"context_token": "ctx-1",
				"item_list":     []map[string]any{{"type": ItemTypeText, "text_item": map[string]string{"text": "你好"}}},
			}},
		})
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.ContextToken = ""
	credentials.ContextUserID = ""
	credentials.GetUpdatesBuf = "cursor-0"
	credentials.BaseURL = server.URL
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	var received []string
	status, err := PollSessionOnce(context.Background(), func(message InboundMessage) {
		received = append(received, message.Text())
	})
	if err != nil {
		t.Fatalf("PollSessionOnce: %v", err)
	}
	if !status.SessionReady {
		t.Fatalf("session not ready: %#v", status)
	}
	if len(received) != 1 || received[0] != "你好" {
		t.Fatalf("unexpected messages: %#v", received)
	}
	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.ContextToken != "ctx-1" || updated.ContextUserID != "user-1" {
		t.Fatalf("context not persisted: %#v", updated)
	}
	if updated.GetUpdatesBuf != "cursor-1" {
		t.Fatalf("cursor = %q, want cursor-1", updated.GetUpdatesBuf)
	}
}

func TestPollSessionOnceIgnoresOtherSenders(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret":             0,
			"get_updates_buf": "cursor-1",
			"msgs": []map[string]any{
				{"from_user_id": "stranger", "message_type": MessageTypeUser, "context_token": "ctx-x"},
				{"from_user_id": "user-1", "message_type": MessageTypeUser, "context_token": "ctx-good"},
			},
		})
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.ContextToken = ""
	credentials.ContextUserID = ""
	credentials.BaseURL = server.URL
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	status, err := PollSessionOnce(context.Background(), nil)
	if err != nil {
		t.Fatalf("PollSessionOnce: %v", err)
	}
	if !status.SessionReady {
		t.Fatalf("session not ready: %#v", status)
	}
	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.ContextToken != "ctx-good" || updated.ContextUserID != "user-1" {
		t.Fatalf("unexpected context: %#v", updated)
	}
}

func TestPollSessionOnceMarksStaleToken(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": -14, "errmsg": "token expired"})
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.BaseURL = server.URL
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	status, err := PollSessionOnce(context.Background(), nil)
	if !errors.Is(err, ErrStaleToken) {
		t.Fatalf("error = %v, want ErrStaleToken", err)
	}
	if !status.LoggedIn || !status.Stale || status.SessionReady {
		t.Fatalf("unexpected status: %#v", status)
	}
	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.ContextToken != "" || updated.ContextUserID != "" {
		t.Fatalf("stale credentials kept context: %#v", updated)
	}
}
