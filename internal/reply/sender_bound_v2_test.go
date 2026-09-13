package reply

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

func TestNewClawBotTextSenderUsesBoundSession(t *testing.T) {
	type requestItem struct {
		TextItem *struct {
			Text string `json:"text"`
		} `json:"text_item"`
	}
	type requestMessage struct {
		ToUserID     string        `json:"to_user_id"`
		ContextToken string        `json:"context_token"`
		ItemList     []requestItem `json:"item_list"`
	}
	type requestPayload struct {
		Msg requestMessage `json:"msg"`
	}
	type capturedRequest struct {
		authorization string
		toUserID      string
		contextToken  string
		text          string
	}

	captured := make(chan capturedRequest, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request requestPayload
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		text := ""
		if len(request.Msg.ItemList) > 0 && request.Msg.ItemList[0].TextItem != nil {
			text = request.Msg.ItemList[0].TextItem.Text
		}
		captured <- capturedRequest{
			authorization: r.Header.Get("Authorization"),
			toUserID:      request.Msg.ToUserID,
			contextToken:  request.Msg.ContextToken,
			text:          text,
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token-1",
		ILinkBotID:    "bot-1",
		BaseURL:       server.URL,
		ILinkUserID:   "user-1",
		ContextToken:  "context-1",
		ContextUserID: "user-1",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := NewClawBotTextSender()(ctx, "无法续聊：测试错误"); err != nil {
		t.Fatalf("NewClawBotTextSender: %v", err)
	}

	select {
	case got := <-captured:
		if got.authorization != "Bearer token-1" || got.toUserID != "user-1" || got.contextToken != "context-1" || got.text != "无法续聊：测试错误" {
			t.Fatalf("captured request = %#v", got)
		}
	case <-ctx.Done():
		t.Fatal("timed out waiting for send request")
	}
}
