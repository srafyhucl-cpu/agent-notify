package clawbot

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
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
	if err := updateCredentials(func(credentials *Credentials) error {
		credentials.SessionAlertAt = "2026-09-17T16:40:00+08:00"
		return nil
	}); err != nil {
		t.Fatalf("updateCredentials: %v", err)
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
	if updated.SessionEstablishedAt == "" {
		t.Fatalf("收到微信消息后未记录会话世代: %#v", updated)
	}
	if updated.SessionAlertAt != "" {
		t.Fatalf("会话恢复后未清空提醒标记: %#v", updated)
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

func TestPollSessionOnceDispatchesPrivateMessageWithoutContextToken(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret":             0,
			"get_updates_buf": "cursor-1",
			"msgs": []map[string]any{{
				"msg_id":       "reply-1",
				"from_user_id": "user-1",
				"message_type": MessageTypeUser,
				"item_list":    []map[string]any{{"type": ItemTypeText, "text_item": map[string]string{"text": "继续"}}},
			}},
		})
	}))
	defer server.Close()

	credentials := boundCredentials()
	credentials.BaseURL = server.URL
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	var received []InboundMessage
	if _, err := PollSessionOnce(context.Background(), func(message InboundMessage) {
		received = append(received, message)
	}); err != nil {
		t.Fatalf("PollSessionOnce: %v", err)
	}
	if len(received) != 1 || received[0].PlatformMessageID() != "reply-1" {
		t.Fatalf("received = %#v", received)
	}
	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.ContextToken != credentials.ContextToken {
		t.Fatalf("context changed without a token: %#v", updated)
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

func TestSessionErrorDelaySilencesMissingCredentials(t *testing.T) {
	delay, report := sessionErrorDelay(fmt.Errorf("open credentials: %w", fs.ErrNotExist))
	if report {
		t.Fatal("missing credentials should not be reported")
	}
	if delay != time.Second {
		t.Fatalf("delay = %v, want 1s", delay)
	}

	delay, report = sessionErrorDelay(errors.New("network down"))
	if !report || delay != 0 {
		t.Fatalf("unexpected network result: delay=%v report=%v", delay, report)
	}
}

func TestClearSessionContextPreservesNewerContext(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	credentials := boundCredentials()
	credentials.ContextToken = "new-context"
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	if err := ClearSessionContext("old-context"); err != nil {
		t.Fatalf("ClearSessionContext: %v", err)
	}
	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.ContextToken != "new-context" || updated.ContextUserID != "user-1" {
		t.Fatalf("newer context was cleared: %#v", updated)
	}

	if err := ClearSessionContext("new-context"); err != nil {
		t.Fatalf("ClearSessionContext matching token: %v", err)
	}
	updated, err = LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials after clear: %v", err)
	}
	if updated.ContextToken != "" || updated.ContextUserID != "" {
		t.Fatalf("matching context was not cleared: %#v", updated)
	}
}

// 轮询期间用户重新登录后，旧 token 的失效响应不能把新凭据标记为失效。
func TestMarkStaleIfTokenKeepsRelogin(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	old := boundCredentials()
	if err := SaveCredentials(old); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	relogin := boundCredentials()
	relogin.BotToken = "token-2"
	relogin.ContextToken = "ctx-new"
	relogin.GetUpdatesBuf = "cursor-new"
	if err := SaveCredentials(relogin); err != nil {
		t.Fatalf("SaveCredentials relogin: %v", err)
	}

	if err := markStaleIfToken(old.BotToken); err != nil {
		t.Fatalf("markStaleIfToken(old): %v", err)
	}
	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.StaleAt != "" {
		t.Fatalf("旧轮询把新登录标记失效：%#v", updated)
	}
	if updated.BotToken != "token-2" || updated.GetUpdatesBuf != "cursor-new" || updated.ContextToken != "ctx-new" {
		t.Fatalf("新登录凭据被改写：%#v", updated)
	}

	if err := markStaleIfToken(relogin.BotToken); err != nil {
		t.Fatalf("markStaleIfToken(current): %v", err)
	}
	updated, err = LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials after stale: %v", err)
	}
	if updated.StaleAt == "" || updated.ContextToken != "" || updated.GetUpdatesBuf != "" {
		t.Fatalf("当前 token 失效后未清理会话：%#v", updated)
	}
}

// 轮询结果只能回写游标与上下文；期间重新登录时不得覆盖新凭据。
func TestPollSessionOnceDoesNotOverwriteRelogin(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())

	relogged := false
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !relogged {
			// 模拟长轮询挂起期间用户扫码重新登录。
			relogged = true
			relogin := boundCredentials()
			relogin.BotToken = "token-2"
			relogin.GetUpdatesBuf = "cursor-new"
			relogin.ContextToken = "ctx-new"
			if err := SaveCredentials(relogin); err != nil {
				t.Errorf("SaveCredentials relogin: %v", err)
			}
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
	credentials.BaseURL = server.URL
	credentials.GetUpdatesBuf = "cursor-0"
	if err := SaveCredentials(credentials); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	var received []string
	if _, err := PollSessionOnce(context.Background(), func(message InboundMessage) {
		received = append(received, message.Text())
	}); err != nil {
		t.Fatalf("PollSessionOnce: %v", err)
	}
	if len(received) != 1 || received[0] != "你好" {
		t.Fatalf("unexpected messages: %#v", received)
	}

	updated, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if updated.BotToken != "token-2" {
		t.Fatalf("旧轮询把新登录覆盖回旧 token：%#v", updated)
	}
	if updated.GetUpdatesBuf != "cursor-new" || updated.ContextToken != "ctx-new" {
		t.Fatalf("旧轮询覆盖了新登录的游标或上下文：%#v", updated)
	}
}
