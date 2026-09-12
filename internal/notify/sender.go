package notify

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	StatusSuccess     = "成功"
	StatusFailed      = "失败"
	StatusNotLoggedIn = "未登录"
	StatusDryRun      = "DryRun"
	StatusSkipped     = "已跳过"
)

// NotifyOptions holds arguments for sending one notification.
type NotifyOptions struct {
	Agent     string
	SessionID string
	Title     string
	Summary   string
	MaxChars  int
	DryRun    bool
}

// NotifyResult contains execution details of one delivery attempt.
type NotifyResult struct {
	Status        string
	Error         string
	DryRunPayload string
}

// SendNotification renders one plain-text ClawBot message and records the
// outcome in the structured history log.
func SendNotification(opts NotifyOptions) NotifyResult {
	if opts.MaxChars <= 0 {
		opts.MaxChars = 800
	}

	title := strings.TrimSpace(strings.ReplaceAll(opts.Title, "\n", " "))
	if title == "" {
		title = "任务完成"
	}
	if !strings.HasPrefix(title, "【") {
		title = "【通知】" + title
	}

	summary := FormatNotifySummary(opts.Summary, opts.MaxChars)
	if summary == "" {
		summary = fmt.Sprintf("任务已完成。%s", time.Now().Format("01-02 15:04:05"))
	}
	message := title + "\n\n" + summary

	if opts.DryRun {
		payload, _ := json.Marshal(map[string]string{
			"title":   title,
			"message": message,
		})
		return NotifyResult{Status: StatusDryRun, DryRunPayload: string(payload)}
	}

	creds, err := clawbot.LoadCredentials()
	if err != nil {
		return recordFailure(opts, title, summary, StatusNotLoggedIn, "未登录 ClawBot，请先运行 agent-notify login")
	}
	client, err := clawbot.NewClient(creds)
	if err != nil {
		return recordFailure(opts, title, summary, StatusFailed, err.Error())
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	if err := client.SendText(ctx, message); err != nil {
		return recordFailure(opts, title, summary, StatusFailed, err.Error())
	}

	_ = appendHistory(HistoryItem{
		Timestamp: time.Now().Format(time.RFC3339Nano),
		Agent:     strings.TrimSpace(opts.Agent),
		Session:   strings.TrimSpace(opts.SessionID),
		Title:     title,
		Summary:   summary,
		Status:    StatusSuccess,
	}, config.GetPaths().PushLog)
	return NotifyResult{Status: StatusSuccess}
}

func recordFailure(opts NotifyOptions, title, summary, status, message string) NotifyResult {
	_ = appendHistory(HistoryItem{
		Timestamp: time.Now().Format(time.RFC3339Nano),
		Agent:     strings.TrimSpace(opts.Agent),
		Session:   strings.TrimSpace(opts.SessionID),
		Title:     title,
		Summary:   summary,
		Status:    status,
		Error:     message,
	}, config.GetPaths().PushLog)
	return NotifyResult{Status: status, Error: message}
}
