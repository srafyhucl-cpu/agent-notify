package notify

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	// Summaries are stored one JSON object per line; a long summary can legitimately
	// exceed bufio's default 64KiB buffer, so the scanner starts there and stops at
	// historyScanMaxLineBytes.
	historyScanInitialBuffer = 64 * 1024
	historyScanMaxLineBytes  = 2 * 1024 * 1024
	defaultHistoryLimit      = 50
)

// HistoryItem is one structured push record. The file uses JSON Lines so
// summaries can contain newlines without corrupting the history.
type HistoryItem struct {
	Timestamp string `json:"timestamp"`
	Agent     string `json:"agent,omitempty"`
	Session   string `json:"session,omitempty"`
	Title     string `json:"title"`
	Summary   string `json:"summary,omitempty"`
	Status    string `json:"status"`
	Error     string `json:"error,omitempty"`
	MessageID string `json:"messageID,omitempty"`
	ClientID  string `json:"clientID,omitempty"`
}

func (h HistoryItem) LocalTime() time.Time {
	if value, err := time.Parse(time.RFC3339Nano, h.Timestamp); err == nil {
		return value.Local()
	}
	return time.Time{}
}

func appendHistory(item HistoryItem, logPath string) error {
	if strings.TrimSpace(logPath) == "" {
		logPath = config.GetPaths().PushLog
	}
	if strings.TrimSpace(item.Timestamp) == "" {
		item.Timestamp = time.Now().Format(time.RFC3339Nano)
	}
	if err := os.MkdirAll(filepath.Dir(logPath), 0700); err != nil {
		return fmt.Errorf("create history directory: %w", err)
	}
	data, err := json.Marshal(item)
	if err != nil {
		return fmt.Errorf("encode history: %w", err)
	}
	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return fmt.Errorf("open history: %w", err)
	}
	defer file.Close()
	if _, err := file.Write(append(data, '\n')); err != nil {
		return fmt.Errorf("write history: %w", err)
	}
	return nil
}

// GetHistory reads push history from logPath and returns the most recent entries first.
func GetHistory(limit int, logPath string) ([]HistoryItem, error) {
	if strings.TrimSpace(logPath) == "" {
		logPath = config.GetPaths().PushLog
	}
	if limit <= 0 {
		limit = defaultHistoryLimit
	}

	file, err := os.Open(logPath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()

	items := make([]HistoryItem, 0, limit)
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, historyScanInitialBuffer), historyScanMaxLineBytes)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var item HistoryItem
		if err := json.Unmarshal([]byte(line), &item); err != nil {
			continue
		}
		items = append(items, item)
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}

	for left, right := 0, len(items)-1; left < right; left, right = left+1, right-1 {
		items[left], items[right] = items[right], items[left]
	}
	if len(items) > limit {
		items = items[:limit]
	}
	return items, nil
}

// ClearHistory removes the push log file.
func ClearHistory(logPath string) error {
	if strings.TrimSpace(logPath) == "" {
		logPath = config.GetPaths().PushLog
	}
	err := os.Remove(logPath)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}
