package clawbot

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestClawbotDebugRedactsSensitiveFields(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", dir)
	t.Setenv("AGENT_NOTIFY_CLAWBOT_DEBUG", "1")

	writeClawbotDebug("sendmessage", []byte(`{
		"ret": 0,
		"message_id": "platform-1",
		"context_token": "context-secret",
		"item_list": [{"text_item": {"text": "user-reply-secret"}}],
		"data": {"bot_token": "bot-secret", "client_id": "client-1"}
	}`))

	data, err := os.ReadFile(filepath.Join(dir, "clawbot-debug.log"))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	text := string(data)
	if !strings.Contains(text, `"message_id":"platform-1"`) || !strings.Contains(text, `"client_id":"client-1"`) {
		t.Fatalf("debug log lost correlation IDs: %s", text)
	}
	if strings.Contains(text, "context-secret") || strings.Contains(text, "bot-secret") || strings.Contains(text, "user-reply-secret") {
		t.Fatalf("debug log leaked a secret: %s", text)
	}
	if strings.Count(text, "[REDACTED]") != 3 {
		t.Fatalf("debug log redactions = %d, want 3: %s", strings.Count(text, "[REDACTED]"), text)
	}
}

func TestClawbotDebugPreservesLargeNumericIdentifiers(t *testing.T) {
	sanitized := sanitizeClawbotDebugResponse([]byte(`{
		"message_id": 12345678901234567890,
		"plain_text": "hidden"
	}`))
	text := string(sanitized)
	if !strings.Contains(text, `"message_id":12345678901234567890`) {
		t.Fatalf("large identifier was not preserved: %s", text)
	}
	if strings.Contains(text, "hidden") || !strings.Contains(text, `"plain_text":"[REDACTED]"`) {
		t.Fatalf("plain_text was not redacted: %s", text)
	}
}

func TestAccountScopeIsStableAndIsolated(t *testing.T) {
	first := AccountScope("bot-1", "user-1")
	if first == "" || first != AccountScope(" bot-1 ", " user-1 ") {
		t.Fatalf("scope is not stable: %q", first)
	}
	if first == AccountScope("bot-2", "user-1") || first == AccountScope("bot-1", "user-2") {
		t.Fatal("different accounts produced the same scope")
	}
	if strings.Contains(first, "bot-1") || strings.Contains(first, "user-1") {
		t.Fatalf("scope leaked a raw account identifier: %q", first)
	}
}

func TestClawbotDebugOmitsUnparseableResponse(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", dir)
	t.Setenv("AGENT_NOTIFY_CLAWBOT_DEBUG", "1")

	writeClawbotDebug("getupdates", []byte("context_token=raw-secret"))
	data, err := os.ReadFile(filepath.Join(dir, "clawbot-debug.log"))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if strings.Contains(string(data), "raw-secret") {
		t.Fatalf("debug log leaked an unparseable response: %s", data)
	}
}

func TestClawbotDebugCorrelatesSendIdentifiers(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", dir)
	t.Setenv("AGENT_NOTIFY_CLAWBOT_DEBUG", "1")

	var requestClientID string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request sendMessageRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		requestClientID = request.Msg.ClientID
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret":        0,
			"message_id": "platform-1",
			"client_id":  "server-client-1",
		})
	}))
	defer server.Close()

	client := newClientWithBaseURL(boundCredentials(), server.URL)
	if _, err := client.SendText(context.Background(), "hello"); err != nil {
		t.Fatalf("SendText: %v", err)
	}
	if requestClientID == "" {
		t.Fatal("request client_id is empty")
	}

	data, err := os.ReadFile(filepath.Join(dir, "clawbot-debug.log"))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	text := string(data)
	for _, expected := range []string{string(DebugOperationSendRequest), requestClientID, string(DebugOperationSendResult), "platform-1", "server-client-1", AccountScope(boundCredentials().ILinkBotID, boundCredentials().ILinkUserID)} {
		if !strings.Contains(text, expected) {
			t.Fatalf("debug log missing %q: %s", expected, text)
		}
	}
}

func TestClawbotDebugRecordsParsedReferenceIDs(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", dir)
	t.Setenv("AGENT_NOTIFY_CLAWBOT_DEBUG", "1")

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret": 0,
			"msgs": []map[string]any{
				{
					"msg_id":       "reply-1",
					"from_user_id": "user-1",
					"message_type": 1,
					"item_list": []map[string]any{{
						"type": 3,
						"ref_msg": map[string]any{
							"message_item": map[string]any{"msg_id": "platform-1"},
						},
					}},
				},
				{
					"msg_id":       "reply-2",
					"from_user_id": "user-1",
					"message_type": 1,
					"item_list": []map[string]any{{
						"type":    3,
						"ref_msg": map[string]any{},
					}},
				},
				{
					"msg_id":       "ordinary-1",
					"from_user_id": "user-1",
					"message_type": 1,
					"item_list": []map[string]any{{
						"type":      1,
						"text_item": map[string]any{"text": "hello"},
					}},
				},
			},
		})
	}))
	defer server.Close()

	client := newClientWithBaseURL(boundCredentials(), server.URL)
	if _, err := client.GetUpdates(context.Background(), "cursor"); err != nil {
		t.Fatalf("GetUpdates: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(dir, "clawbot-debug.log"))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	text := string(data)
	for _, expected := range []string{
		string(DebugOperationGetUpdatesData),
		`"msg_id":"reply-1"`,
		`"referenced_msg_ids":["platform-1"]`,
		`"msg_id":"reply-2"`,
		`"has_reference":false,"msg_id":"ordinary-1"`,
		`"private":true`,
		`"bound_sender":true`,
		AccountScope(boundCredentials().ILinkBotID, boundCredentials().ILinkUserID),
	} {
		if !strings.Contains(text, expected) {
			t.Fatalf("debug log missing %q: %s", expected, text)
		}
	}
}
