package notify

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	StatusSuccess        = "成功"
	StatusFailed         = "失败"
	StatusNotLoggedIn    = "未登录"
	StatusSessionMissing = "会话未建立"
	StatusDryRun         = "DryRun"
	StatusSkipped        = "已跳过"
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

	title, summary, message := renderNotification(opts)

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
		return recordFailure(opts, title, summary, StatusNotLoggedIn, clawbotHint(err))
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	if err := client.SendText(ctx, message); err != nil {
		status := StatusFailed
		switch {
		case errors.Is(err, clawbot.ErrStaleToken):
			status = StatusNotLoggedIn
		case errors.Is(err, clawbot.ErrNoSession):
			status = StatusSessionMissing
		case errors.Is(err, clawbot.ErrSessionExpired):
			status = StatusSessionMissing
			if clearErr := clawbot.ClearSessionContext(creds.ContextToken); clearErr != nil {
				return recordFailure(opts, title, summary, status, clawbotHint(err)+"；本地会话状态清理失败: "+clearErr.Error())
			}
		}
		return recordFailure(opts, title, summary, status, clawbotHint(err))
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

// clawbotHint turns a ClawBot error into an actionable Chinese message.
func clawbotHint(err error) string {
	switch {
	case errors.Is(err, clawbot.ErrStaleToken):
		return "ClawBot 登录已失效，请重新运行 agent-notify login"
	case errors.Is(err, clawbot.ErrNoSession):
		return "尚未建立微信会话：请先在微信中给 ClawBot 发送一条消息，再运行 agent-notify sync"
	case errors.Is(err, clawbot.ErrSessionExpired):
		return "ClawBot 主动推送会话已失效：请先在微信中给 ClawBot 发送一条消息，再运行 agent-notify sync"
	default:
		return err.Error()
	}
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

// RecordSkipped records a notification suppressed by policy without sending it.
func RecordSkipped(opts NotifyOptions, reason string) NotifyResult {
	if opts.MaxChars <= 0 {
		opts.MaxChars = 800
	}
	reason = strings.TrimSpace(reason)
	if reason == "" {
		reason = "通知已跳过"
	}
	if opts.DryRun {
		return NotifyResult{Status: StatusSkipped, Error: reason}
	}
	title, summary, _ := renderNotification(opts)
	_ = appendHistory(HistoryItem{
		Timestamp: time.Now().Format(time.RFC3339Nano),
		Agent:     strings.TrimSpace(opts.Agent),
		Session:   strings.TrimSpace(opts.SessionID),
		Title:     title,
		Summary:   summary,
		Status:    StatusSkipped,
		Error:     reason,
	}, config.GetPaths().PushLog)
	return NotifyResult{Status: StatusSkipped, Error: reason}
}

func renderNotification(opts NotifyOptions) (title, summary, message string) {
	title = strings.TrimSpace(strings.ReplaceAll(opts.Title, "\n", " "))
	if title == "" {
		title = "任务完成"
	}
	if !strings.HasPrefix(title, "【") {
		title = "【通知】" + title
	}

	summary = FormatNotifySummary(opts.Summary, opts.MaxChars)
	if summary == "" {
		summary = fmt.Sprintf("任务已完成。%s", time.Now().Format("01-02 15:04:05"))
	}
	message = title + "\n\n" + summary
	return title, summary, message
}
