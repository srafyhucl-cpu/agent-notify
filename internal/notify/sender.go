package notify

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

const (
	StatusSuccess        = "成功"
	StatusFailed         = "失败"
	StatusNotLoggedIn    = "未登录"
	StatusSessionMissing = "会话未建立"
	StatusDryRun         = "DryRun"
	StatusSkipped        = "已跳过"
	// DefaultMaxChars is unlimited. Explicit positive values still cap the
	// complete rendered notification in runes for compatibility.
	DefaultMaxChars         = 0
	notificationSendTimeout = 45 * time.Second
)

// NotifyOptions holds arguments for sending one notification.
type NotifyOptions struct {
	Agent string
	// SessionID is the OpenCode session ID or authoritative Codex thread ID.
	SessionID string
	Title     string
	Summary   string
	Notice    string
	MaxChars  int
	DryRun    bool
}

// NotifyResult contains execution details of one delivery attempt.
type NotifyResult struct {
	Status        string
	Error         string
	DryRunPayload string
	MessageID     string
	ClientID      string
}

// SendNotification renders one plain-text ClawBot message and records the
// outcome in the structured history log.
func SendNotification(opts NotifyOptions) NotifyResult {
	opts, protocol := prepareNotification(opts)
	if protocol.Silent {
		return RecordSkipped(opts, protocol.Reason)
	}

	rendered := renderNotification(opts, time.Now())
	title, summary, message := rendered.Title, rendered.Summary, rendered.Message

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

	ctx, cancel := context.WithTimeout(context.Background(), notificationSendTimeout)
	defer cancel()
	sendResult, err := client.SendText(ctx, message)
	if err != nil {
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
		MessageID: sendResult.MessageID,
		ClientID:  sendResult.ClientID,
	}, config.GetPaths().PushLog)
	if isRouteable(opts, sendResult) {
		_ = reply.RecordRoute(reply.Route{
			MessageID: sendResult.MessageID,
			ClientID:  sendResult.ClientID,
			BotID:     creds.ILinkBotID,
			UserID:    creds.ILinkUserID,
			Agent:     strings.TrimSpace(opts.Agent),
			SessionID: strings.TrimSpace(opts.SessionID),
		})
	}
	return NotifyResult{Status: StatusSuccess, MessageID: sendResult.MessageID, ClientID: sendResult.ClientID}
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
	opts, protocol := prepareNotification(opts)
	if strings.TrimSpace(reason) == "" && protocol.Silent {
		reason = protocol.Reason
	}
	reason = strings.TrimSpace(reason)
	if reason == "" {
		reason = "通知已跳过"
	}
	if opts.DryRun {
		return NotifyResult{Status: StatusSkipped, Error: reason}
	}
	rendered := renderNotification(opts, time.Now())
	title, summary := rendered.Title, rendered.Summary
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

func prepareNotification(opts NotifyOptions) (NotifyOptions, ProtocolResult) {
	if strings.TrimSpace(opts.Summary) == "" {
		return opts, ProtocolResult{}
	}
	protocol := ParseProtocolBlocks(opts.Summary)
	opts.Summary = protocol.Body
	return opts, protocol
}

func isRouteable(opts NotifyOptions, result clawbot.SendResult) bool {
	agent := strings.ToLower(strings.TrimSpace(opts.Agent))
	if agent != "codex" && agent != "opencode" {
		return false
	}
	if strings.TrimSpace(opts.SessionID) == "" {
		return false
	}
	return strings.TrimSpace(result.MessageID) != "" || strings.TrimSpace(result.ClientID) != ""
}
