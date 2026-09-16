//go:build windows

package ui

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// 历史读取必须带缓存：日志未变化时直接复用，变化（大小/修改时间）后自动失效。
func TestLoadHistoryCachesUntilLogChanges(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "push.log")
	app := WidgetApp{paths: config.Paths{PushLog: logPath}}

	if items := app.loadHistory(1); len(items) != 0 {
		t.Fatalf("缺少日志文件时应返回空列表：%#v", items)
	}

	writeHistoryLine(t, logPath, "first", true)
	items := app.loadHistory(1)
	if len(items) != 1 || items[0].Title != "first" {
		t.Fatalf("首次读取 = %#v", items)
	}

	// 文件未变化：直接命中缓存
	app.historyCache = []notify.HistoryItem{{Title: "cached"}}
	if got := app.loadHistory(1); len(got) != 1 || got[0].Title != "cached" {
		t.Fatalf("日志未变化时未复用缓存：%#v", got)
	}

	// 追加新记录：缓存必须失效并返回最新数据
	writeHistoryLine(t, logPath, "second", true)
	app.historyCache = []notify.HistoryItem{{Title: "stale"}}
	app.historyStamp = historyFileStamp(logPath)
	app.historyLimit = 1
	if got := app.loadHistory(1); len(got) != 1 || got[0].Title != "stale" {
		t.Fatalf("stamp 一致时不应重读：%#v", got)
	}
	writeHistoryLine(t, logPath, "third", true)
	if got := app.loadHistory(1); len(got) != 1 || got[0].Title != "third" {
		t.Fatalf("日志变化后未刷新缓存：%#v", got)
	}

	// 请求更大的 limit 时要重读，不能返回较小 limit 的缓存
	if got := app.loadHistory(5); len(got) != 3 {
		t.Fatalf("扩大 limit 后未重读：%#v", got)
	}
}

func writeHistoryLine(t *testing.T, logPath, title string, newline bool) {
	t.Helper()
	line := `{"timestamp":"2026-09-16T10:00:00Z","title":"` + title + `","status":"成功"}`
	if newline {
		line += "\n"
	}
	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatalf("open history: %v", err)
	}
	if _, err := file.WriteString(line); err != nil {
		t.Fatalf("write history: %v", err)
	}
	if err := file.Close(); err != nil {
		t.Fatalf("close history: %v", err)
	}
}
