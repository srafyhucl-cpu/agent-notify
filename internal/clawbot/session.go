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
			_ = markStale()
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
		if err := SaveCredentials(next); err != nil {
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
	_ = client.NotifyStop(stopCtx)
}

func markStale() error {
	return updateCredentials(func(credentials *Credentials) error {
		credentials.StaleAt = time.Now().Format(time.RFC3339)
		credentials.ContextToken = ""
		credentials.ContextUserID = ""
		credentials.GetUpdatesBuf = ""
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
