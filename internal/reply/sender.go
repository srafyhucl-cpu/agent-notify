package reply

import (
	"context"
	"errors"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

// CodexQueue submits text to an existing Codex thread.
type CodexQueue interface {
	Queue(ctx context.Context, threadID, text string) error
}

// OpenCodeQueue submits text to an existing OpenCode session.
type OpenCodeQueue interface {
	Queue(ctx context.Context, sessionID, text string) error
}

// ReplySender submits one user reply to a specific agent conversation.
type ReplySender interface {
	Send(ctx context.Context, sessionID, text string) error
}

// CodexReplySender adapts a Codex queue implementation to ReplySender.
type CodexReplySender struct {
	Queue CodexQueue
}

func (s CodexReplySender) Send(ctx context.Context, sessionID, text string) error {
	if s.Queue == nil {
		return errors.New("codex reply: queue is not configured")
	}
	return s.Queue.Queue(ctx, sessionID, text)
}

// OpenCodeReplySender adapts an OpenCode queue implementation to ReplySender.
type OpenCodeReplySender struct {
	Queue OpenCodeQueue
}

func (s OpenCodeReplySender) Send(ctx context.Context, sessionID, text string) error {
	if s.Queue == nil {
		return errors.New("opencode reply: queue is not configured")
	}
	return s.Queue.Queue(ctx, sessionID, text)
}

// NewClawBotTextSender returns the default visible-error sender.
func NewClawBotTextSender() TextSender {
	return func(ctx context.Context, text string) error {
		credentials, err := clawbot.LoadCredentials()
		if err != nil {
			return err
		}
		client, err := clawbot.NewClient(credentials)
		if err != nil {
			return err
		}
		_, err = client.SendText(ctx, text)
		return err
	}
}
