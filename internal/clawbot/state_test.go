package clawbot

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
)

func TestSavedTokenListIgnoresStaleLogin(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	credentials := boundCredentials()
	credentials.StaleAt = "2026-09-12T00:00:00Z"
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	if tokens := savedTokenList(); len(tokens) != 0 {
		t.Fatalf("saved token list = %#v, want empty for stale login", tokens)
	}
}

func TestSaveCredentialsIsolatesAccounts(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	if err := SaveCredentials(boundCredentials()); err != nil {
		t.Fatalf("SaveCredentials first account: %v", err)
	}

	next := Credentials{
		BotToken:      "token-2",
		ILinkBotID:    "bot-2",
		ILinkUserID:   "user-2",
		ContextToken:  "ctx-2",
		ContextUserID: "user-2",
		GetUpdatesBuf: "cursor-2",
	}
	if err := SaveCredentials(next); err != nil {
		t.Fatalf("SaveCredentials second account: %v", err)
	}

	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got.ContextToken != "" || got.ContextUserID != "" || got.GetUpdatesBuf != "" {
		t.Fatalf("account-scoped state leaked across login: %#v", got)
	}
}

func TestSaveCredentialsClearsCursorWhenBoundUserChanges(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	first := boundCredentials()
	first.GetUpdatesBuf = "cursor-1"
	if err := SaveCredentials(first); err != nil {
		t.Fatalf("SaveCredentials first account: %v", err)
	}

	next := first
	next.ILinkUserID = "user-2"
	next.ContextUserID = "user-2"
	next.ContextToken = "ctx-2"
	next.GetUpdatesBuf = "cursor-2"
	if err := SaveCredentials(next); err != nil {
		t.Fatalf("SaveCredentials second user: %v", err)
	}

	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got.ContextToken != "" || got.ContextUserID != "" || got.GetUpdatesBuf != "" {
		t.Fatalf("user-scoped state leaked across login: %#v", got)
	}
}

func TestSaveCredentialsRejectsCrossUserContext(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	credentials := boundCredentials()
	credentials.ContextUserID = "another-user"
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got.ContextToken != "" || got.ContextUserID != "" {
		t.Fatalf("cross-user context was retained: %#v", got)
	}
}

func TestClientRejectsContextFromAnotherUser(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.ContextUserID = "another-user"
	client := newClientWithBaseURL(credentials, server.URL)
	_, err := client.SendText(context.Background(), "hello")
	if !errors.Is(err, ErrNoSession) {
		t.Fatalf("error = %v, want ErrNoSession", err)
	}
	if calls.Load() != 0 {
		t.Fatalf("server calls = %d, want 0", calls.Load())
	}
}

func TestPollQRStatusUsesRedirectBaseAsFallback(t *testing.T) {
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"confirmed","bot_token":"token-2","ilink_bot_id":"bot-2","ilink_user_id":"user-2","ret":0}`))
	}))
	defer target.Close()

	origin := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"scaned_but_redirect","redirect_host":"` + target.URL + `","ret":0}`))
	}))
	defer origin.Close()

	client := NewAuthClient(origin.URL)
	credentials, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{})
	if err != nil {
		t.Fatalf("PollQRStatus: %v", err)
	}
	if credentials.BaseURL != target.URL {
		t.Fatalf("base URL = %q, want redirect host %q", credentials.BaseURL, target.URL)
	}
}
