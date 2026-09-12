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

func boundCredentials() Credentials {
	return Credentials{
		BotToken:      "token-1",
		ILinkBotID:    "bot-1",
		ILinkUserID:   "user-1",
		ContextToken:  "ctx-1",
		ContextUserID: "user-1",
	}
}

func TestClientSendText(t *testing.T) {
	var captured sendMessageRequest
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ilink/bot/sendmessage" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		if r.Header.Get("Authorization") != "Bearer token-1" {
			t.Fatalf("unexpected authorization: %s", r.Header.Get("Authorization"))
		}
		if r.Header.Get("AuthorizationType") != "ilink_bot_token" {
			t.Fatalf("unexpected authorization type: %s", r.Header.Get("AuthorizationType"))
		}
		if r.Header.Get("iLink-App-Id") != AppID {
			t.Fatalf("unexpected app id: %s", r.Header.Get("iLink-App-Id"))
		}
		if r.Header.Get("iLink-App-ClientVersion") != AppClientVersion {
			t.Fatalf("unexpected client version: %s", r.Header.Get("iLink-App-ClientVersion"))
		}
		if r.Header.Get("X-WECHAT-UIN") == "" {
			t.Fatal("missing X-WECHAT-UIN")
		}
		_ = json.NewDecoder(r.Body).Decode(&captured)
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	client := newClientWithBaseURL(boundCredentials(), server.URL)
	if err := client.SendText(context.Background(), "hello"); err != nil {
		t.Fatalf("SendText returned error: %v", err)
	}
	if captured.Msg.FromUserID != "" {
		t.Fatalf("from_user_id must stay empty, got %q", captured.Msg.FromUserID)
	}
	if captured.Msg.ToUserID != "user-1" {
		t.Fatalf("unexpected recipient: %q", captured.Msg.ToUserID)
	}
	if captured.Msg.ContextToken != "ctx-1" {
		t.Fatalf("unexpected context token: %q", captured.Msg.ContextToken)
	}
	if captured.BaseInfo.ChannelVersion != ChannelVersion || captured.BaseInfo.BotAgent == "" {
		t.Fatalf("unexpected base info: %#v", captured.BaseInfo)
	}
	if len(captured.Msg.ItemList) != 1 || captured.Msg.ItemList[0].TextItem == nil {
		t.Fatalf("unexpected item list: %#v", captured.Msg.ItemList)
	}
	if captured.Msg.ItemList[0].TextItem.Text != "hello" {
		t.Fatalf("unexpected text: %q", captured.Msg.ItemList[0].TextItem.Text)
	}
}

func TestClientSendTextRequiresSession(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.ContextToken = ""
	credentials.ContextUserID = ""
	client := newClientWithBaseURL(credentials, server.URL)
	err := client.SendText(context.Background(), "hello")
	if !errors.Is(err, ErrNoSession) {
		t.Fatalf("error = %v, want ErrNoSession", err)
	}
	if calls.Load() != 0 {
		t.Fatalf("server calls = %d, want 0", calls.Load())
	}
}

func TestClientRetriesTransientFailure(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) == 1 {
			http.Error(w, "temporary", http.StatusBadGateway)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	client := newClientWithBaseURL(boundCredentials(), server.URL)
	client.attempts = 2
	client.httpClient = server.Client()
	if err := client.SendText(context.Background(), "hello"); err != nil {
		t.Fatalf("SendText should retry transient failure: %v", err)
	}
	if calls.Load() != 2 {
		t.Fatalf("server calls = %d, want 2", calls.Load())
	}
}

func TestClientStaleTokenIsNotRetried(t *testing.T) {
	cases := []struct {
		name string
		body map[string]any
	}{
		{name: "ret", body: map[string]any{"ret": -14, "errmsg": "token expired"}},
		{name: "errcode", body: map[string]any{"ret": 0, "errcode": -14, "errmsg": "token expired"}},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				_ = json.NewEncoder(w).Encode(testCase.body)
			}))
			defer server.Close()

			client := newClientWithBaseURL(boundCredentials(), server.URL)
			client.attempts = 3
			client.httpClient = server.Client()
			err := client.SendText(context.Background(), "hello")
			if !errors.Is(err, ErrStaleToken) {
				t.Fatalf("error = %v, want ErrStaleToken", err)
			}
			if calls.Load() != 1 {
				t.Fatalf("server calls = %d, want 1", calls.Load())
			}
		})
	}
}

func TestClientGetUpdates(t *testing.T) {
	var captured getUpdatesRequest
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ilink/bot/getupdates" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		_ = json.NewDecoder(r.Body).Decode(&captured)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret":             0,
			"get_updates_buf": "cursor-2",
			"msgs": []map[string]any{{
				"from_user_id":  "user-1",
				"to_user_id":    "bot-1",
				"message_type":  MessageTypeUser,
				"message_state": MessageStateFinish,
				"context_token": "ctx-2",
				"item_list":     []map[string]any{{"type": ItemTypeText, "text_item": map[string]string{"text": "hi"}}},
			}},
		})
	}))
	defer server.Close()

	client := newClientWithBaseURL(boundCredentials(), server.URL)
	updates, err := client.GetUpdates(context.Background(), "cursor-1")
	if err != nil {
		t.Fatalf("GetUpdates: %v", err)
	}
	if captured.GetUpdatesBuf != "cursor-1" {
		t.Fatalf("request cursor = %q, want cursor-1", captured.GetUpdatesBuf)
	}
	if captured.BaseInfo.ChannelVersion != ChannelVersion {
		t.Fatalf("missing base info: %#v", captured.BaseInfo)
	}
	if updates.Cursor != "cursor-2" {
		t.Fatalf("cursor = %q, want cursor-2", updates.Cursor)
	}
	if len(updates.Messages) != 1 {
		t.Fatalf("messages = %d, want 1", len(updates.Messages))
	}
	if text := updates.Messages[0].Text(); text != "hi" {
		t.Fatalf("message text = %q, want hi", text)
	}
}

func TestClientRejectsIncompleteCredentials(t *testing.T) {
	if _, err := NewClient(Credentials{BotToken: "token"}); err == nil {
		t.Fatal("expected incomplete credentials to fail")
	}
}

func TestNewClientRejectsStaleLogin(t *testing.T) {
	credentials := boundCredentials()
	credentials.StaleAt = "2026-01-01T00:00:00Z"
	if _, err := NewClient(credentials); !errors.Is(err, ErrStaleToken) {
		t.Fatalf("error = %v, want ErrStaleToken", err)
	}
}

func TestProbe(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	defer server.Close()
	if err := Probe(context.Background(), server.URL); err != nil {
		t.Fatalf("Probe: %v", err)
	}
}
