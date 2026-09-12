package clawbot

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
)

func TestClientSendText(t *testing.T) {
	var captured sendMessageRequest
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ilink/bot/sendmessage" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Header.Get("Authorization") != "Bearer token-1" {
			t.Fatalf("unexpected authorization: %s", r.Header.Get("Authorization"))
		}
		if r.Header.Get("AuthorizationType") != "ilink_bot_token" {
			t.Fatalf("unexpected authorization type: %s", r.Header.Get("AuthorizationType"))
		}
		if r.Header.Get("X-WECHAT-UIN") == "" {
			t.Fatal("missing X-WECHAT-UIN")
		}
		_ = json.NewDecoder(r.Body).Decode(&captured)
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	client := newClientWithBaseURL(Credentials{
		BotToken:    "token-1",
		ILinkBotID:  "bot-1",
		ILinkUserID: "user-1",
	}, server.URL)
	if err := client.SendText(context.Background(), "hello"); err != nil {
		t.Fatalf("SendText returned error: %v", err)
	}
	if captured.Msg.FromUserID != "bot-1" || captured.Msg.ToUserID != "user-1" {
		t.Fatalf("unexpected sender/recipient: %#v", captured.Msg)
	}
	if len(captured.Msg.ItemList) != 1 || captured.Msg.ItemList[0].TextItem == nil {
		t.Fatalf("unexpected item list: %#v", captured.Msg.ItemList)
	}
	if captured.Msg.ItemList[0].TextItem.Text != "hello" {
		t.Fatalf("unexpected text: %q", captured.Msg.ItemList[0].TextItem.Text)
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

	client := newClientWithBaseURL(Credentials{
		BotToken:    "token-1",
		ILinkBotID:  "bot-1",
		ILinkUserID: "user-1",
	}, server.URL)
	client.attempts = 2
	client.httpClient = server.Client()
	if err := client.SendText(context.Background(), "hello"); err != nil {
		t.Fatalf("SendText should retry transient failure: %v", err)
	}
	if calls.Load() != 2 {
		t.Fatalf("server calls = %d, want 2", calls.Load())
	}
}

func TestClientRejectsIncompleteCredentials(t *testing.T) {
	if _, err := NewClient(Credentials{BotToken: "token"}); err == nil {
		t.Fatal("expected incomplete credentials to fail")
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
