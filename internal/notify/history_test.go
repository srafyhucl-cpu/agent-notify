package notify

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestHistoryLifecycle(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "push.log")
	items := []HistoryItem{
		{Timestamp: "2026-09-11T10:00:00Z", Agent: "opencode", Title: "A", Summary: "first", Status: StatusSuccess},
		{Timestamp: "2026-09-11T11:00:00Z", Agent: "codex", Title: "B", Summary: "second", Status: StatusFailed, Error: "network"},
	}
	for _, item := range items {
		if err := appendHistory(item, logPath); err != nil {
			t.Fatalf("appendHistory: %v", err)
		}
	}

	got, err := GetHistory(10, logPath)
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("len(GetHistory) = %d, want 2", len(got))
	}
	if got[0].Title != "B" || got[0].Agent != "codex" {
		t.Fatalf("latest item = %#v", got[0])
	}
	if got[1].Summary != "first" {
		t.Fatalf("oldest item = %#v", got[1])
	}

	limited, err := GetHistory(1, logPath)
	if err != nil {
		t.Fatalf("GetHistory limit: %v", err)
	}
	if len(limited) != 1 || limited[0].Title != "B" {
		t.Fatalf("limited history = %#v", limited)
	}

	if err := ClearHistory(logPath); err != nil {
		t.Fatalf("ClearHistory: %v", err)
	}
	if _, err := os.Stat(logPath); !os.IsNotExist(err) {
		t.Fatal("history file still exists after ClearHistory")
	}
}

func TestGetHistoryMissingFile(t *testing.T) {
	items, err := GetHistory(10, filepath.Join(t.TempDir(), "missing.log"))
	if err != nil {
		t.Fatalf("GetHistory missing file: %v", err)
	}
	if items != nil {
		t.Fatalf("missing file should return nil, got %#v", items)
	}
}

func TestGetHistoryReadsLegacyRecords(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "push.log")
	legacy := `{"timestamp":"2026-09-11T10:00:00Z","agent":"codex","title":"legacy","status":"成功"}` + "\n"
	if err := os.WriteFile(logPath, []byte(legacy), 0600); err != nil {
		t.Fatalf("write legacy history: %v", err)
	}

	items, err := GetHistory(1, logPath)
	if err != nil {
		t.Fatalf("GetHistory legacy: %v", err)
	}
	if len(items) != 1 || items[0].Title != "legacy" || items[0].MessageID != "" || items[0].ClientID != "" {
		t.Fatalf("legacy history = %#v", items)
	}
}

func TestHistoryItemLocalTime(t *testing.T) {
	item := HistoryItem{Timestamp: time.Date(2026, 9, 11, 12, 0, 0, 0, time.UTC).Format(time.RFC3339Nano)}
	if item.LocalTime().IsZero() {
		t.Fatal("LocalTime returned zero for valid timestamp")
	}
}
