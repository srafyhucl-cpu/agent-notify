package agent

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func TestResolveAntigravityTitleUsesAnnotation(t *testing.T) {
	dir := t.TempDir()
	writeAntigravityAnnotation(t, dir, "conversation-1",
		`title:"Antigravity Hook Acceptance Test" last_user_view_time:{seconds:1}`)

	resolution := resolveAntigravityTitle(dir, "conversation-1", "")
	if resolution.Title != "Antigravity Hook Acceptance Test" {
		t.Fatalf("title = %q", resolution.Title)
	}
	if resolution.Source != antigravityTitleSourceAnnotation || resolution.Warning != "" {
		t.Fatalf("resolution = %#v", resolution)
	}
}

func TestResolveAntigravityTitleDecodesAnnotationEscapes(t *testing.T) {
	dir := t.TempDir()
	writeAntigravityAnnotation(t, dir, "conversation-1",
		`title:"修复 \"标题\" 读取" last_user_view_time:{seconds:1}`)

	resolution := resolveAntigravityTitle(dir, "conversation-1", "")
	if resolution.Title != `修复 "标题" 读取` {
		t.Fatalf("title = %q", resolution.Title)
	}
}

func TestResolveAntigravityTitleFallsBackToFirstUserRequest(t *testing.T) {
	dir := t.TempDir()
	transcript := filepath.Join(dir, "transcript.jsonl")
	data := strings.Join([]string{
		`{"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>\nAntigravity Hook Acceptance Test\n</USER_REQUEST>\n<ADDITIONAL_METADATA>ignored</ADDITIONAL_METADATA>"}`,
		`{"step_index":1,"type":"PLANNER_RESPONSE","content":"done"}`,
	}, "\n")
	if err := os.WriteFile(transcript, []byte(data), 0600); err != nil {
		t.Fatal(err)
	}

	resolution := resolveAntigravityTitle("", "conversation-1", transcript)
	if resolution.Title != "Antigravity Hook Acceptance Test" {
		t.Fatalf("title = %q", resolution.Title)
	}
	if resolution.Source != antigravityTitleSourceTranscript || resolution.Warning != antigravityTitlePromptWarning {
		t.Fatalf("resolution = %#v", resolution)
	}
}

func TestResolveAntigravityTitleFallsBackWhenAnnotationIsInvalid(t *testing.T) {
	dir := t.TempDir()
	writeAntigravityAnnotation(t, dir, "conversation-1", `last_user_view_time:{seconds:1}`)

	resolution := resolveAntigravityTitle(dir, "conversation-1", "")
	if resolution.Title != "" || resolution.Source != antigravityTitleSourceFallback {
		t.Fatalf("resolution = %#v", resolution)
	}
	if resolution.Warning != antigravityTitleFallbackWarning {
		t.Fatalf("warning = %q", resolution.Warning)
	}
}

func TestHandleAntigravityStopUsesConversationTitle(t *testing.T) {
	prepareHookTest(t)
	annotationsDir := filepath.Join(t.TempDir(), "annotations")
	t.Setenv("AGENT_NOTIFY_ANTIGRAVITY_ANNOTATIONS_DIR", annotationsDir)
	writeAntigravityAnnotation(t, annotationsDir, "conversation-1",
		`title:"真实 Antigravity 标题" last_user_view_time:{seconds:1}`)

	result := HandleAntigravityStop(strings.NewReader(`{"conversationId":"conversation-1","fullyIdle":true}`))
	if result.Status != notify.StatusDryRun {
		t.Fatalf("dry-run status = %q, error = %q", result.Status, result.Error)
	}
	if !strings.Contains(result.DryRunPayload, "🟢【Antigravity】真实 Antigravity 标题") {
		t.Fatalf("dry-run payload = %q", result.DryRunPayload)
	}
}

func writeAntigravityAnnotation(t *testing.T, dir, conversationID, content string) {
	t.Helper()
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, conversationID+".pbtxt")
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}
