package notify

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestGetHistory_EmptyFile(t *testing.T) {
	dir := t.TempDir()
	logFile := filepath.Join(dir, "push.log")
	_ = os.WriteFile(logFile, []byte(""), 0644)

	records, err := GetHistory(50, logFile)
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(records) != 0 {
		t.Errorf("len(records) = %d, want 0", len(records))
	}
}

func TestGetHistory_NonexistentFile(t *testing.T) {
	records, err := GetHistory(50, filepath.Join(t.TempDir(), "nope.log"))
	if err != nil {
		t.Fatalf("should not error for non-existent: %v", err)
	}
	if records != nil {
		t.Errorf("records should be nil for non-existent file")
	}
}

func TestGetHistory_MultipleEntries(t *testing.T) {
	dir := t.TempDir()
	logFile := filepath.Join(dir, "push.log")
	lines := []string{
		"2025-06-15T10:00:00.000Z push title=【AI任务】测试一 | channels=PushPlus | status=成功 | summary=摘要一",
		"2025-06-15T11:00:00.000Z push title=【codex】测试二 | channels=PushPlus,企业微信 | status=部分成功 | summary=摘要二",
		"2025-06-15T12:00:00.000Z push title=【Antigravity】测试三 | channels=飞书 | status=成功 | summary=摘要三",
	}
	content := strings.Join(lines, "\r\n") + "\r\n"
	_ = os.WriteFile(logFile, []byte(content), 0644)

	records, err := GetHistory(50, logFile)
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(records) != 3 {
		t.Fatalf("len(records) = %d, want 3", len(records))
	}

	// Should be most recent first
	if !strings.Contains(records[0].Title, "测试三") {
		t.Errorf("records[0].Title = %q, want to contain 测试三", records[0].Title)
	}
	if records[0].Channels != "飞书" {
		t.Errorf("records[0].Channels = %q, want 飞书", records[0].Channels)
	}
	if records[0].Status != "成功" {
		t.Errorf("records[0].Status = %q, want 成功", records[0].Status)
	}
}

func TestGetHistory_LimitWorks(t *testing.T) {
	dir := t.TempDir()
	logFile := filepath.Join(dir, "push.log")
	lines := []string{
		"2025-06-15T10:00:00.000Z push title=A | channels=PushPlus | status=成功 | summary=1",
		"2025-06-15T11:00:00.000Z push title=B | channels=PushPlus | status=成功 | summary=2",
		"2025-06-15T12:00:00.000Z push title=C | channels=PushPlus | status=成功 | summary=3",
	}
	_ = os.WriteFile(logFile, []byte(strings.Join(lines, "\n")+"\n"), 0644)

	records, _ := GetHistory(2, logFile)
	if len(records) != 2 {
		t.Errorf("len(records) = %d, want 2 (limited)", len(records))
	}
}

func TestClearHistory(t *testing.T) {
	dir := t.TempDir()
	logFile := filepath.Join(dir, "push.log")
	_ = os.WriteFile(logFile, []byte("some data"), 0644)

	err := ClearHistory(logFile)
	if err != nil {
		t.Fatalf("ClearHistory: %v", err)
	}
	if _, err := os.Stat(logFile); !os.IsNotExist(err) {
		t.Error("log file should be removed after ClearHistory")
	}
}

func TestClearHistory_NonexistentFile(t *testing.T) {
	err := ClearHistory(filepath.Join(t.TempDir(), "nonexistent.log"))
	if err != nil {
		t.Errorf("ClearHistory for non-existent should not error: %v", err)
	}
}

func TestCutSentence(t *testing.T) {
	tests := []struct {
		name   string
		text   string
		max    int
		expect string
	}{
		{"短文本原样", "hello", 500, "hello"},
		{"刚好等于 max", strings.Repeat("a", 100), 100, strings.Repeat("a", 100)},
		{"无句号硬切", strings.Repeat("a", 200), 100, strings.Repeat("a", 100) + "…"},
		{"句号在 100+", strings.Repeat("a", 110) + "。" + strings.Repeat("b", 50), 120, strings.Repeat("a", 110) + "。…"},
		{"句号太靠前", "前。" + strings.Repeat("a", 200), 120, "前。" + strings.Repeat("a", 118) + "…"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CutSentence(tt.text, tt.max)
			if got != tt.expect {
				t.Errorf("CutSentence() = %q, want %q", got, tt.expect)
			}
		})
	}
}
