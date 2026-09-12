package clawbot

import (
	"context"
	"errors"
	"io/fs"
	"strings"
	"time"
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
	for _, message := range updates.Messages {
		if !messageCarriesContext(message) {
			continue
		}
		from := strings.TrimSpace(message.FromUserID)
		if from != strings.TrimSpace(credentials.ILinkUserID) {
			continue
		}
		next.ContextToken = strings.TrimSpace(message.ContextToken)
		next.ContextUserID = from
		if onMessage != nil {
			onMessage(message)
		}
	}

	if next != credentials {
		if err := SaveCredentials(next); err != nil {
			return GetStatus(), err
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
		if err := sleepContext(ctx, time.Second); err != nil {
			return Credentials{}, err
		}
	}
}

// RunSessionLoop keeps the message cursor and context token fresh for as long
// as the process is alive, and announces the ClawBot client lifecycle once the
// bound account is known. It stops on a stale token; callers should ask the
// user to log in again.
func RunSessionLoop(ctx context.Context, onMessage func(InboundMessage), onError func(error)) {
	backoff := time.Second
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
				backoff = time.Second
				continue
			}
			if onError != nil {
				onError(err)
			}
			if err := sleepContext(ctx, backoff); err != nil {
				return
			}
			if backoff < time.Minute {
				backoff *= 2
				if backoff > time.Minute {
					backoff = time.Minute
				}
			}
			continue
		}
		backoff = time.Second

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
	notifyCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
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
	stopCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
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

func messageCarriesContext(message InboundMessage) bool {
	return strings.TrimSpace(message.ContextToken) != "" &&
		strings.TrimSpace(message.GroupID) == "" &&
		message.MessageType == MessageTypeUser
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
