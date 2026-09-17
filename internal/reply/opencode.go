package reply

import (
	"context"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	openCodeHeartbeatMaxAge     = 30 * time.Second
	openCodeHeartbeatFutureSkew = 5 * time.Second
	openCodeHeartbeatDirName    = "heartbeats"
	openCodeLegacyHeartbeatFile = "heartbeat.json"
)

// OpenCodeQueueRunner submits replies to the installed OpenCode plugin through
// a local, one-way spool. The plugin owns authentication and session prompt
// compatibility.
type OpenCodeQueueRunner struct {
	// Dir is the plugin-owned reply inbox. Empty uses the configured path.
	Dir string
	// ResultWait bounds synchronous error visibility. A timeout means the
	// durable job was accepted and later failures remain asynchronous.
	ResultWait time.Duration
	// AsyncWait bounds failure observation after ResultWait. Zero uses the
	// remaining job TTL.
	AsyncWait time.Duration
	// PollInterval controls both synchronous and asynchronous result checks.
	PollInterval time.Duration
	// OnAsyncFailure receives failures discovered after Queue has returned.
	// Nil disables background observation.
	OnAsyncFailure OpenCodeFailureReporter
}

type openCodeReplyJob = spoolReplyJob
type openCodeReplyResult = spoolReplyResult

func (r OpenCodeQueueRunner) Queue(ctx context.Context, sessionID, text string) error {
	dir := r.Dir
	if dir == "" {
		dir = config.GetPaths().OpenCodeReplyDir
	}
	queue := newSpoolQueueConfig(
		dir,
		"opencode reply",
		r.ResultWait,
		r.AsyncWait,
		r.PollInterval,
		requireOpenCodeHeartbeat,
		openCodeResultError,
		errOpenCodeResultUnconfirmed,
		r.OnAsyncFailure,
	)
	return queue.Queue(ctx, sessionID, text)
}
