package agent

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func TestHandleNotifyIgnoresOpenCodeMarker(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	paths := config.GetPaths()
	if _, err := marker.SetMarker(paths.OpenCodeMarker, "Off"); err != nil {
		t.Fatal(err)
	}

	result := HandleNotify("", "测试", "hello", "", 500, true, true)
	if result.Status != notify.StatusDryRun {
		t.Fatalf("generic notify status = %q, want %q", result.Status, notify.StatusDryRun)
	}
}

func TestHandleNotifyUsesGenericHistorySource(t *testing.T) {
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
		if len(request.Msg.ItemList) != 1 || request.Msg.ItemList[0].TextItem.Text == "" {
			t.Error("missing generic notification payload")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	t.Setenv("AGENT_NOTIFY_QUIET", "")
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot",
		BaseURL:       server.URL,
		ILinkUserID:   "user",
		ContextToken:  "context",
		ContextUserID: "user",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := HandleNotify("", "脚本完成", "summary", "", 500, false, true)
	if result.Status != notify.StatusSuccess {
		t.Fatalf("generic notify status = %q, error = %q", result.Status, result.Error)
	}
	history, err := notify.GetHistory(10, "")
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(history) != 1 || history[0].Agent != "" || history[0].Title != "【通知】脚本完成" {
		t.Fatalf("generic history = %#v", history)
	}
}
