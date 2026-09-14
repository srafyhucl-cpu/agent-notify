package agent

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func prepareHookTest(t *testing.T) {
	t.Helper()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("AGENT_NOTIFY_QUIET", "")
	t.Setenv("AGENT_NOTIFY_DEVIN_SESSIONS_DB", filepath.Join(t.TempDir(), "missing-sessions.db"))
	t.Setenv("AGENT_NOTIFY_ANTIGRAVITY_DRYRUN", "1")
	t.Setenv("AGENT_NOTIFY_ANTIGRAVITY_ANNOTATIONS_DIR", filepath.Join(t.TempDir(), "annotations"))
	t.Setenv("AGENT_NOTIFY_DEVIN_DRYRUN", "1")
}

func TestReadHookJSONAcceptsBOM(t *testing.T) {
	var event struct {
		SessionID string `json:"session_id"`
	}
	err := readHookJSON(strings.NewReader("\xef\xbb\xbf{\"session_id\":\"session-1\"}"), &event)
	if err != nil {
		t.Fatalf("readHookJSON: %v", err)
	}
	if event.SessionID != "session-1" {
		t.Fatalf("session_id = %q", event.SessionID)
	}
}

func TestHandleAntigravityStopRequiresIdleSession(t *testing.T) {
	prepareHookTest(t)

	result := HandleAntigravityStop(strings.NewReader(`{"conversationId":"conversation-1","fullyIdle":false}`))
	if result.Status != notify.StatusSkipped || !strings.Contains(result.Error, "完全空闲") {
		t.Fatalf("not-idle result = %#v", result)
	}

	result = HandleAntigravityStop(strings.NewReader(`{"conversationId":"","fullyIdle":true}`))
	if result.Status != notify.StatusSkipped || !strings.Contains(result.Error, "conversationId") {
		t.Fatalf("missing-session result = %#v", result)
	}
}

func TestHandleAntigravityStopDryRun(t *testing.T) {
	prepareHookTest(t)

	result := HandleAntigravityStop(strings.NewReader(`{"conversationId":"conversation-1","fullyIdle":true,"error":""}`))
	if result.Status != notify.StatusDryRun {
		t.Fatalf("dry-run status = %q, error = %q", result.Status, result.Error)
	}
	if !strings.Contains(result.DryRunPayload, "【antigravity】跑完了") {
		t.Fatalf("dry-run payload = %q", result.DryRunPayload)
	}
}

func TestExtractAntigravityTranscriptSummary(t *testing.T) {
	data := []byte(strings.Join([]string{
		`{"role":"user","text":"ignore this"}`,
		`{"role":"assistant","content":[{"type":"text","text":"assistant result"}]}`,
	}, "\n"))
	if got := extractAntigravityTranscriptSummary(data); got != "assistant result" {
		t.Fatalf("summary = %q", got)
	}
}

func TestHandleDevinStopRequiresExactStopSession(t *testing.T) {
	prepareHookTest(t)

	result := HandleDevinStop(strings.NewReader(`{"session_id":"session-1","hook_event_name":"Stop","stop_hook_active":true}`))
	if result.Status != notify.StatusSkipped || !strings.Contains(result.Error, "Stop hook") {
		t.Fatalf("active hook result = %#v", result)
	}

	result = HandleDevinStop(strings.NewReader(`{"session_id":"session-1","hook_event_name":"SessionStart"}`))
	if result.Status != notify.StatusSkipped || !strings.Contains(result.Error, "不是 Stop") {
		t.Fatalf("wrong event result = %#v", result)
	}

	result = HandleDevinStop(strings.NewReader(`{"session_id":"","hook_event_name":"Stop"}`))
	if result.Status != notify.StatusSkipped || !strings.Contains(result.Error, "session_id") {
		t.Fatalf("missing session result = %#v", result)
	}
}

func TestHandleDevinStopUsesSessionTitle(t *testing.T) {
	prepareHookTest(t)
	path := filepath.Join(t.TempDir(), "sessions.db")
	db := openTestSQLite(t, path)
	execTestSQLite(t, db, "CREATE TABLE sessions (id TEXT NOT NULL, title TEXT, working_directory TEXT);")
	execTestSQLite(t, db, `INSERT INTO sessions (id, title, working_directory) VALUES ('session-1', '真实 Devin 标题', 'D:\Project\yueyou');`)
	closeTestSQLite(t, db)
	t.Setenv("AGENT_NOTIFY_DEVIN_SESSIONS_DB", path)

	result := HandleDevinStop(strings.NewReader(`{
		"session_id":"session-1",
		"hook_event_name":"Stop",
		"stop_hook_active":false,
		"last_assistant_message":"devin result"
	}`))
	if result.Status != notify.StatusDryRun {
		t.Fatalf("dry-run status = %q, error = %q", result.Status, result.Error)
	}
	if !strings.Contains(result.DryRunPayload, "【devin】真实 Devin 标题") {
		t.Fatalf("dry-run payload = %q", result.DryRunPayload)
	}
}

func TestHandleDevinStopDryRun(t *testing.T) {
	prepareHookTest(t)

	result := HandleDevinStop(strings.NewReader(`{
		"session_id":"session-1",
		"hook_event_name":"Stop",
		"stop_hook_active":false,
		"last_assistant_message":"devin result"
	}`))
	if result.Status != notify.StatusDryRun {
		t.Fatalf("dry-run status = %q, error = %q", result.Status, result.Error)
	}
	if !strings.Contains(result.DryRunPayload, "【devin】跑完了") {
		t.Fatalf("dry-run payload = %q", result.DryRunPayload)
	}
}

func TestHasJSONValue(t *testing.T) {
	for _, raw := range []string{"", "null", `""`, "{}", "[]"} {
		if hasJSONValue([]byte(raw)) {
			t.Fatalf("%q should be empty", raw)
		}
	}
	for _, raw := range []string{`"error"`, "1", "true", `{"reason":"failed"}`, `["failed"]`} {
		if !hasJSONValue([]byte(raw)) {
			t.Fatalf("%q should be present", raw)
		}
	}
}

func TestConfigPathsIncludeAgentHookLocations(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	paths := config.GetPaths()
	if paths.AntigravityMarker == "" || paths.DevinMarker == "" || paths.AntigravityHooks == "" || paths.AntigravityAnnotations == "" || paths.DevinConfig == "" {
		t.Fatalf("incomplete paths: %#v", paths)
	}
}
