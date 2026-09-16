package notify

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
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

// 倒序扫描要跨读取块也能取到正确的最近 N 条，并跳过损坏行。
func TestGetHistoryTailScanAcrossChunks(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "push.log")
	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	summary := strings.Repeat("详情", 200) // 让每条记录远小于读取块、但总量跨多块
	total := 300
	for i := 0; i < total; i++ {
		item := HistoryItem{Timestamp: "2026-09-11T10:00:00Z", Title: fmt.Sprintf("record-%03d", i), Summary: summary, Status: StatusSuccess}
		data, err := json.Marshal(item)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := file.Write(append(data, '\n')); err != nil {
			t.Fatal(err)
		}
		if i == 120 {
			if _, err := file.WriteString("not-json\n\n"); err != nil { // 损坏行与空行
				t.Fatal(err)
			}
		}
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}

	items, err := GetHistory(5, logPath)
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(items) != 5 {
		t.Fatalf("len = %d, want 5", len(items))
	}
	for index, item := range items {
		want := fmt.Sprintf("record-%03d", total-1-index)
		if item.Title != want {
			t.Fatalf("items[%d].Title = %q, want %q", index, item.Title, want)
		}
	}
}

// 文件末尾没有换行时，最后一条记录仍要能被解析出来。
func TestGetHistoryWithoutTrailingNewline(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "push.log")
	body := `{"timestamp":"2026-09-11T10:00:00Z","title":"no-newline","status":"成功"}`
	if err := os.WriteFile(logPath, []byte(body), 0600); err != nil {
		t.Fatal(err)
	}
	items, err := GetHistory(1, logPath)
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(items) != 1 || items[0].Title != "no-newline" {
		t.Fatalf("items = %#v", items)
	}
}

// 日志超过上限时裁剪旧记录，且保留末尾整行、新记录不丢。
func TestTrimHistoryKeepsNewestRecords(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "push.log")
	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	padding := strings.Repeat("x", 200)
	for i := 0; i < 60; i++ {
		line := fmt.Sprintf(`{"timestamp":"2026-09-11T10:00:00Z","title":"record-%03d","summary":"%s","status":"成功"}`, i, padding)
		if _, err := file.WriteString(line + "\n"); err != nil {
			t.Fatal(err)
		}
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	before, err := os.Stat(logPath)
	if err != nil {
		t.Fatal(err)
	}

	if err := trimHistoryTo(logPath, before.Size()-100, 1024); err != nil {
		t.Fatalf("trimHistoryTo: %v", err)
	}
	after, err := os.Stat(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if after.Size() >= before.Size() {
		t.Fatalf("日志未裁剪：before=%d after=%d", before.Size(), after.Size())
	}

	items, err := GetHistory(60, logPath)
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(items) == 0 || items[0].Title != "record-059" {
		t.Fatalf("裁剪后最新记录丢失：%#v", items)
	}
	for _, item := range items {
		if item.Title == "record-000" {
			t.Fatal("裁剪后仍保留最旧记录")
		}
	}
	// 保留内容必须都是完整 JSON（残行已丢弃）
	if len(items) < 2 {
		t.Fatalf("裁剪后保留记录过少：%d", len(items))
	}
}
