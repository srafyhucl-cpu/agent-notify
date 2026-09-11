package notify

import (
	"bufio"
	"os"
	"regexp"
	"strings"
	"time"

	"linkweixin/internal/config"
)

// HistoryItem represents a parsed entry from the push log.
type HistoryItem struct {
	Time     string `json:"time"`
	RawTime  string `json:"rawTime"`
	Title    string `json:"title"`
	Summary  string `json:"summary"`
	Channels string `json:"channels"`
	Status   string `json:"status"`
	Raw      string `json:"raw"`
}

var (
	reLogLine = regexp.MustCompile(`^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?)\s+(.*)$`)
	reTitle   = regexp.MustCompile(`title=([^|]+)`)
	reSummary = regexp.MustCompile(`summary=([^|]+)`)
	reChan    = regexp.MustCompile(`channels=([^|]+)`)
	reStatus  = regexp.MustCompile(`status=([^|]+)`)
)

// GetHistory reads push history from logPath (default: paths.PushLog), returns most recent entries first.
func GetHistory(limit int, logPath string) ([]HistoryItem, error) {
	if logPath == "" {
		paths := config.GetPaths()
		logPath = paths.PushLog
	}

	if limit <= 0 {
		limit = 50
	}

	file, err := os.Open(logPath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()

	var lines []string
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line != "" {
			lines = append(lines, line)
		}
	}

	var results []HistoryItem
	for i := len(lines) - 1; i >= 0; i-- {
		line := lines[i]
		m := reLogLine.FindStringSubmatch(line)
		if len(m) != 3 {
			continue
		}

		timeStr := m[1]
		rest := m[2]
		localTime := timeStr
		if t, err := time.Parse(time.RFC3339Nano, timeStr); err == nil {
			localTime = t.Local().Format("2006-01-02 15:04:05")
		} else if t, err := time.Parse(time.RFC3339, timeStr); err == nil {
			localTime = t.Local().Format("2006-01-02 15:04:05")
		}

		title := ""
		summary := ""
		channels := "PushPlus"
		status := "成功"

		if tm := reTitle.FindStringSubmatch(rest); len(tm) == 2 {
			title = strings.TrimSpace(tm[1])
		}
		if sm := reSummary.FindStringSubmatch(rest); len(sm) == 2 {
			summary = strings.TrimSpace(sm[1])
		}
		if cm := reChan.FindStringSubmatch(rest); len(cm) == 2 {
			channels = strings.TrimSpace(cm[1])
		}
		if stm := reStatus.FindStringSubmatch(rest); len(stm) == 2 {
			status = strings.TrimSpace(stm[1])
		}

		if title == "" {
			title = rest
		}

		results = append(results, HistoryItem{
			Time:     localTime,
			RawTime:  timeStr,
			Title:    title,
			Summary:  summary,
			Channels: channels,
			Status:   status,
			Raw:      line,
		})

		if len(results) >= limit {
			break
		}
	}

	return results, nil
}

// ClearHistory removes the push log file.
func ClearHistory(logPath string) error {
	if logPath == "" {
		paths := config.GetPaths()
		logPath = paths.PushLog
	}
	err := os.Remove(logPath)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}
