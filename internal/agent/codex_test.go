package agent

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

func withCodexNotificationTestServer(t *testing.T) {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret":        0,
			"message_id": "platform-1",
		})
	}))
	t.Cleanup(server.Close)

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("CODEX_HOME", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "")
	t.Setenv("AGENT_NOTIFY_QUIET", "")
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot-1",
		BaseURL:       server.URL,
		ILinkUserID:   "user-1",
		ContextToken:  "context",
		ContextUserID: "user-1",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
}

func TestConvertCodexArgs(t *testing.T) {
	jsonArg := `{"input-messages":["重构 Agent-notify 模块并测试"],"last-assistant-message":"已经完成重构，所有单元测试通过。"}`
	title, summary, threadID := ConvertCodexArgs([]string{"turn-ended", jsonArg})

	if title != "【codex】重构 Agent-notify 模块并测试" {
		t.Fatalf("title = %q", title)
	}
	if summary != "已经完成重构，所有单元测试通过。" {
		t.Fatalf("summary = %q", summary)
	}
	if threadID != "" {
		t.Fatalf("threadID = %q, want empty", threadID)
	}

	longInput := "这是一段非常非常非常非常非常非常非常非常非常非常非常非常长的主题任务描述"
	titleLong, _, _ := ConvertCodexArgs([]string{`{"input-messages":["` + longInput + `"],"last-assistant-message":"完成"}`})
	expected := "【codex】" + longInput
	if titleLong != expected {
		t.Fatalf("titleLong = %q, want %q", titleLong, expected)
	}

	titleEmpty, summaryEmpty, _ := ConvertCodexArgs([]string{"turn-ended", "something-else"})
	if titleEmpty != "【codex】跑完了" || summaryEmpty != "" {
		t.Fatalf("empty event = %q / %q", titleEmpty, summaryEmpty)
	}

	_, _, eventThreadID := ConvertCodexArgs([]string{`{"thread-id":"019abc","last-assistant-message":"完成"}`})
	if eventThreadID != "019abc" {
		t.Fatalf("eventThreadID = %q, want 019abc", eventThreadID)
	}

	_, _, snakeThreadID := ConvertCodexArgs([]string{`{"thread_id":"snake-019","last-assistant-message":"完成"}`})
	if snakeThreadID != "snake-019" {
		t.Fatalf("snakeThreadID = %q, want snake-019", snakeThreadID)
	}

	_, _, threadOnlyID := ConvertCodexArgs([]string{`{"thread-id":"thread-only"}`})
	if threadOnlyID != "thread-only" {
		t.Fatalf("threadOnlyID = %q, want thread-only", threadOnlyID)
	}

	_, _, crossEventThreadID := ConvertCodexArgs([]string{
		`{"thread-id":"thread-from-first-arg"}`,
		`{"input-messages":["后续事件"],"last-assistant-message":"完成"}`,
	})
	if crossEventThreadID != "" {
		t.Fatalf("crossEventThreadID = %q, want empty", crossEventThreadID)
	}

	_, _, laterThreadID := ConvertCodexArgs([]string{
		`{"last-assistant-message":"完成"}`,
		`{"thread-id":"thread-from-later-arg"}`,
	})
	if laterThreadID != "" {
		t.Fatalf("laterThreadID = %q, want empty", laterThreadID)
	}

	firstTitle, firstSummary, laterEventThreadID := ConvertCodexArgs([]string{
		`{"input-messages":["第一段任务"],"last-assistant-message":"第一段摘要"}`,
		`{"thread-id":"thread-from-later-arg","input-messages":["第二段任务"],"last-assistant-message":"第二段摘要"}`,
	})
	if firstTitle != "【codex】第一段任务" || firstSummary != "第一段摘要" || laterEventThreadID != "" {
		t.Fatalf("first-event compatibility = %q / %q / %q", firstTitle, firstSummary, laterEventThreadID)
	}

	_, _, sameEventThreadID := ConvertCodexArgs([]string{
		`{"type":"agent-turn-complete","thread-id":"same-thread","last-assistant-message":"第一段"}`,
		`{"type":"agent-turn-complete","thread-id":"same-thread","last-assistant-message":"第二段"}`,
	})
	if sameEventThreadID != "same-thread" {
		t.Fatalf("sameEventThreadID = %q, want same-thread", sameEventThreadID)
	}

	_, _, changedEventThreadID := ConvertCodexArgs([]string{
		`{"type":"agent-turn-complete","thread-id":"thread-a","last-assistant-message":"第一段"}`,
		`{"type":"agent-turn-complete","thread-id":"thread-b","last-assistant-message":"第二段"}`,
	})
	if changedEventThreadID != "" {
		t.Fatalf("changedEventThreadID = %q, want empty", changedEventThreadID)
	}

	_, _, preferredThreadID := ConvertCodexArgs([]string{
		`{"type":"agent-turn-complete","thread-id":"authoritative-thread","thread_id":"compat-thread"}`,
	})
	if preferredThreadID != "" {
		t.Fatalf("preferredThreadID = %q, want empty without content", preferredThreadID)
	}

	_, _, preferredContentThreadID := ConvertCodexArgs([]string{
		`{"type":"agent-turn-complete","thread-id":"authoritative-thread","thread_id":"compat-thread","last-assistant-message":"完成"}`,
	})
	if preferredContentThreadID != "authoritative-thread" {
		t.Fatalf("preferredContentThreadID = %q, want authoritative-thread", preferredContentThreadID)
	}

	_, _, conflictingThreadID := ConvertCodexArgs([]string{
		`{"type":"agent-turn-complete","thread-id":"thread-a","last-assistant-message":"第一段"}`,
		`{"type":"agent-turn-complete","thread-id":"thread-b","last-assistant-message":"第二段"}`,
	})
	if conflictingThreadID != "" {
		t.Fatalf("conflictingThreadID = %q, want empty", conflictingThreadID)
	}

	_, _, preferredAfterCompatibilityConflict := ConvertCodexArgs([]string{
		`{"thread_id":"compat-a"}`,
		`{"thread_id":"compat-b"}`,
		`{"thread-id":"authoritative-thread"}`,
	})
	if preferredAfterCompatibilityConflict != "" {
		t.Fatalf("preferredAfterCompatibilityConflict = %q, want empty", preferredAfterCompatibilityConflict)
	}

	_, _, conflictingCompatibilityThreadID := ConvertCodexArgs([]string{
		`{"thread_id":"compat-a"}`,
		`{"thread_id":"compat-b"}`,
	})
	if conflictingCompatibilityThreadID != "" {
		t.Fatalf("conflictingCompatibilityThreadID = %q, want empty", conflictingCompatibilityThreadID)
	}

	_, _, numericThreadID := ConvertCodexArgs([]string{
		`{"thread-id":12345,"last-assistant-message":"完成"}`,
	})
	if numericThreadID != "12345" {
		t.Fatalf("numericThreadID = %q, want 12345", numericThreadID)
	}

	_, _, looseThreadID := ConvertCodexArgs([]string{`{thread-id: '019-loose'}`})
	if looseThreadID != "" {
		t.Fatalf("looseThreadID = %q, want no route target from malformed JSON", looseThreadID)
	}
}

func TestHandleCodexRecordsReplyRouteFromRealPayloadShape(t *testing.T) {
	withCodexNotificationTestServer(t)

	payload := `{"type":"agent-turn-complete","thread-id":"01a09406-f545-7303-b065-aa13554f3dd8","turn-id":"turn-1","cwd":"D:\\Project\\Agent-notify","client":"codex_exec","input-messages":["验证完整通知链"],"last-assistant-message":"完整链路验证消息。"}`
	result := HandleCodex([]string{"turn-ended", payload})
	if result.Status != notify.StatusSuccess || result.MessageID != "platform-1" {
		t.Fatalf("result = %#v", result)
	}

	route, err := reply.NewRouteStore("").Find("bot-1", "user-1", "platform-1", "")
	if err != nil {
		t.Fatalf("Find route: %v", err)
	}
	if route.Agent != "codex" || route.SessionID != "01a09406-f545-7303-b065-aa13554f3dd8" {
		t.Fatalf("route = %#v", route)
	}
}

func TestHandleCodexWithoutThreadIDDoesNotRecordReplyRoute(t *testing.T) {
	withCodexNotificationTestServer(t)

	payload := `{"type":"agent-turn-complete","last-assistant-message":"完成"}`
	result := HandleCodex([]string{"turn-ended", payload})
	if result.Status != notify.StatusSuccess || result.MessageID != "platform-1" {
		t.Fatalf("result = %#v", result)
	}

	_, err := reply.NewRouteStore("").Find("bot-1", "user-1", "platform-1", "")
	if !errors.Is(err, reply.ErrRouteNotFound) {
		t.Fatalf("Find route error = %v, want ErrRouteNotFound", err)
	}
}

func TestHandleCodexDryRun(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "1")
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	result := HandleCodex([]string{"turn-ended", `{"input-messages":["DryRun测试"],"last-assistant-message":"完成"}`})
	if result.Status != notify.StatusDryRun {
		t.Fatalf("Status = %q, want %q", result.Status, notify.StatusDryRun)
	}
}

func TestHandleCodexDoNotDisturb(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "1")
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	result := HandleCodex([]string{"turn-ended", `{"input-messages":["[勿扰] 夜间任务"],"last-assistant-message":"完成"}`})
	if result.Status != notify.StatusSkipped || result.Error != "标题包含勿扰标记" {
		t.Fatalf("do-not-disturb result = %#v", result)
	}
}

func TestHandleCodexMarkerRecordsSkipped(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "")
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	paths := config.GetPaths()
	if _, err := marker.SetMarker(paths.CodexMarker, "Off"); err != nil {
		t.Fatal(err)
	}

	result := HandleCodex([]string{"turn-ended", `{"input-messages":["Marker测试"],"last-assistant-message":"完成"}`})
	if result.Status != notify.StatusSkipped {
		t.Fatalf("marker result = %#v", result)
	}
	history, err := notify.GetHistory(10, "")
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(history) != 1 || history[0].Status != notify.StatusSkipped || history[0].Agent != "codex" {
		t.Fatalf("marker history = %#v", history)
	}
}
