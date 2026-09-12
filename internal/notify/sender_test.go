package notify

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

func TestSendNotificationDryRun(t *testing.T) {
	result := SendNotification(NotifyOptions{
		Title:   "测试",
		Summary: "hello **world**",
		DryRun:  true,
	})
	if result.Status != StatusDryRun {
		t.Fatalf("Status = %q, want %q", result.Status, StatusDryRun)
	}
	var payload map[string]string
	if err := json.Unmarshal([]byte(result.DryRunPayload), &payload); err != nil {
		t.Fatalf("decode dry-run payload: %v", err)
	}
	if payload["message"] != "【通知】测试\n\nhello world" {
		t.Fatalf("unexpected dry-run message: %q", payload["message"])
	}
}

func TestSendNotificationWithoutLogin(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", t.TempDir())
	result := SendNotification(NotifyOptions{Title: "测试", Summary: "hello"})
	if result.Status != StatusNotLoggedIn {
		t.Fatalf("Status = %q, want %q", result.Status, StatusNotLoggedIn)
	}
}

func TestSendNotificationSuccess(t *testing.T) {
	var message string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			Msg struct {
				ItemList []struct {
					TextItem struct {
						Text string `json:"text"`
					} `json:"text_item"`
				} `json:"item_list"`
			} `json:"msg"`
		}
		_ = json.NewDecoder(r.Body).Decode(&request)
		if len(request.Msg.ItemList) > 0 {
			message = request.Msg.ItemList[0].TextItem.Text
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:    "token",
		ILinkBotID:  "bot",
		BaseURL:     server.URL,
		ILinkUserID: "user",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := SendNotification(NotifyOptions{
		Agent:     "opencode",
		SessionID: "session-1",
		Title:     "任务完成",
		Summary:   "all good",
	})
	if result.Status != StatusSuccess {
		t.Fatalf("Status = %q, error = %q", result.Status, result.Error)
	}
	if message != "【通知】任务完成\n\nall good" {
		t.Fatalf("sent message = %q", message)
	}

	history, err := GetHistory(10, "")
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(history) != 1 || history[0].Status != StatusSuccess {
		t.Fatalf("history = %#v", history)
	}
}
