package clawbot

import (
	"context"
	"errors"
	"io/fs"
	"strings"
	"time"
)

const (
	sessionLifecycleTimeout = 10 * time.Second
	sessionIdlePollInterval = time.Second
	sessionBackoffInitial   = time.Second
	sessionBackoffMax       = time.Minute
	// debugOperationNotifyStop 记录离线通知（NotifyStop）的旁路失败，便于排查未送达。
	debugOperationNotifyStop DebugOperation = "notify-stop"
)

// SessionEvent reports one batch of inbound messages.
type SessionEvent struct {
	Messages []InboundMessage
	Cursor   string
}

// PollSessionOnce performs one long poll and persists the returned cursor plus
// any newer context token for the bound account and user.
func PollSessionOnce(ctx context.Context, onMessage func(InboundMessage)) (Status, error) {
	credentials, err := LoadCredentials()
	if err != nil {
		return GetStatus(), err
	}
	if strings.TrimSpace(credentials.StaleAt) != "" {
		return GetStatus(), ErrStaleToken
	}

	client, err := NewClient(credentials)
	if err != nil {
		return GetStatus(), err
	}
	updates, err := client.GetUpdates(ctx, credentials.GetUpdatesBuf)
	if err != nil {
		if errors.Is(err, ErrStaleToken) {
			// 只把发起这次轮询所用的 token 标记失效：轮询期间用户可能已经重新登录。
			_ = markStaleIfToken(credentials.BotToken)
		}
		return GetStatus(), err
	}

	next := credentials
	if cursor := strings.TrimSpace(updates.Cursor); cursor != "" {
		next.GetUpdatesBuf = cursor
	}
	dispatchMessages := make([]InboundMessage, 0, len(updates.Messages))
	for _, message := range updates.Messages {
		from := strings.TrimSpace(message.FromUserID)
		if from != strings.TrimSpace(credentials.ILinkUserID) {
			continue
		}
		if message.MessageType != MessageTypeUser || strings.TrimSpace(message.GroupID) != "" {
			continue
		}
		if token := strings.TrimSpace(message.ContextToken); token != "" {
			next.ContextToken = token
			next.ContextUserID = from
		}
		dispatchMessages = append(dispatchMessages, message)
	}

	if next != credentials {
		if err := savePolledSession(credentials, next); err != nil {
			return GetStatus(), err
		}
	}
	if onMessage != nil {
		for _, message := range dispatchMessages {
			onMessage(message)
		}
	}
	return GetStatus(), nil
}

// AwaitSessionContext long-polls until the bound WeChat user sends one message,
// because proactive ClawBot sends require that inbound context token.
func AwaitSessionContext(ctx context.Context, onMessage func(InboundMessage)) (Credentials, error) {
	for {
		status, err := PollSessionOnce(ctx, onMessage)
		if err != nil {
			return Credentials{}, err
		}
		if status.SessionReady {
			return LoadCredentials()
		}
		if err := sleepContext(ctx, sessionIdlePollInterval); err != nil {
			return Credentials{}, err
		}
	}
}

// RunSessionLoop keeps the message cursor and context token fresh for as long
// as the process is alive, and announces the ClawBot client lifecycle once the
// bound account is known. It stops on a stale token; callers should ask the
// user to log in again.
func RunSessionLoop(ctx context.Context, onMessage func(InboundMessage), onError func(error)) {
	backoff := sessionBackoffInitial
	announcedToken := ""

	defer func() {
		stopSessionLifecycle(announcedToken)
	}()

	for {
		if err := ctx.Err(); err != nil {
			return
		}

		status, err := PollSessionOnce(ctx, onMessage)
		if err != nil {
			if errors.Is(err, ErrStaleToken) {
				if onError != nil {
					onError(err)
				}
				return
			}
			if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
				if ctx.Err() != nil {
					return
				}
				continue
			}
			if delay, report := sessionErrorDelay(err); !report {
				if err := sleepContext(ctx, delay); err != nil {
					return
				}
				backoff = sessionBackoffInitial
				continue
			}
			if onError != nil {
				onError(err)
			}
			if err := sleepContext(ctx, backoff); err != nil {
				return
			}
			if backoff < sessionBackoffMax {
				backoff *= 2
				if backoff > sessionBackoffMax {
					backoff = sessionBackoffMax
				}
			}
			continue
		}
		backoff = sessionBackoffInitial

		if status.SessionReady {
			if token := announceSessionStart(ctx, announcedToken); token != "" {
				announcedToken = token
			}
		}
	}
}

// sessionErrorDelay keeps the pre-login state quiet and responsive: while the
// credential file does not exist yet, poll again quickly instead of growing the
// backoff or spamming the error log.
func sessionErrorDelay(err error) (time.Duration, bool) {
	if errors.Is(err, fs.ErrNotExist) {
		return time.Second, false
	}
	return 0, true
}

func announceSessionStart(ctx context.Context, announcedToken string) string {
	credentials, err := LoadCredentials()
	if err != nil || strings.TrimSpace(credentials.StaleAt) != "" {
		return ""
	}
	token := strings.TrimSpace(credentials.BotToken)
	if token == "" || token == announcedToken {
		return ""
	}
	client, err := NewClient(credentials)
	if err != nil {
		return ""
	}
	notifyCtx, cancel := context.WithTimeout(ctx, sessionLifecycleTimeout)
	defer cancel()
	if err := client.NotifyStart(notifyCtx); err != nil {
		return ""
	}
	return token
}

func stopSessionLifecycle(announcedToken string) {
	if announcedToken == "" {
		return
	}
	credentials, err := LoadCredentials()
	if err != nil || strings.TrimSpace(credentials.BotToken) != announcedToken {
		return
	}
	client, err := NewClient(credentials)
	if err != nil {
		return
	}
	stopCtx, cancel := context.WithTimeout(context.Background(), sessionLifecycleTimeout)
	defer cancel()
	if err := client.NotifyStop(stopCtx); err != nil {
		writeClawbotDebugEvent(debugOperationNotifyStop, map[string]string{"error": err.Error()})
	}
}

// markStaleIfToken 只在 botToken 仍是当前登录凭据时才标记失效并清空会话状态。
// 长轮询最长 35 秒，期间用户可能扫码重新登录；旧 token 的失效响应不能殃及新凭据。
func markStaleIfToken(botToken string) error {
	botToken = strings.TrimSpace(botToken)
	return updateCredentials(func(credentials *Credentials) error {
		if botToken != "" && strings.TrimSpace(credentials.BotToken) != botToken {
			return nil
		}
		credentials.StaleAt = time.Now().Format(time.RFC3339)
		credentials.ContextToken = ""
		credentials.ContextUserID = ""
		credentials.GetUpdatesBuf = ""
		return nil
	})
}

// savePolledSession 只把轮询得到的游标与会话上下文写回，并要求期间没有重新登录：
// 旧轮询结果既不能改写 bot_token，也不能覆盖新登录的凭据与游标。
func savePolledSession(base, next Credentials) error {
	baseToken := strings.TrimSpace(base.BotToken)
	if nextToken := strings.TrimSpace(next.BotToken); baseToken != "" && nextToken != baseToken {
		return nil
	}
	return updateCredentials(func(credentials *Credentials) error {
		if strings.TrimSpace(credentials.BotToken) != baseToken {
			return nil
		}
		credentials.GetUpdatesBuf = next.GetUpdatesBuf
		credentials.ContextToken = next.ContextToken
		credentials.ContextUserID = next.ContextUserID
		// 收到新的入站上下文即视为会话可用：刷新「曾就绪」时间并解除已提醒标记。
		if strings.TrimSpace(next.ContextToken) != "" {
			credentials.SessionEstablishedAt = time.Now().Format(time.RFC3339)
			credentials.SessionAlertAt = ""
		}
		return nil
	})
}

// ClearSessionContext invalidates the saved proactive-message context while
// keeping the ClawBot login and message cursor available for recovery.
func ClearSessionContext(expectedToken string) error {
	expectedToken = strings.TrimSpace(expectedToken)
	return updateCredentials(func(credentials *Credentials) error {
		if expectedToken != "" && strings.TrimSpace(credentials.ContextToken) != expectedToken {
			return nil
		}
		credentials.ContextToken = ""
		credentials.ContextUserID = ""
		return nil
	})
}

// MarkSessionAlerted 记录界面已针对当前这次「会话失效」提醒过用户。
// 会话恢复（savePolledSession）会清空该标记，因此同一次断开会话只提醒一次，
// 恢复后再次失效可以重新提醒。
func MarkSessionAlerted() error {
	return updateCredentials(func(credentials *Credentials) error {
		credentials.SessionAlertAt = time.Now().Format(time.RFC3339)
		return nil
	})
}

func sleepContext(ctx context.Context, duration time.Duration) error {
	timer := time.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
