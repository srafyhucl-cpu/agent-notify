package reply

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	defaultOpenCodeResultWait   = 10 * time.Second
	defaultOpenCodePollInterval = 100 * time.Millisecond
	replyIDBytes                = 16
	openCodeHeartbeatMaxAge     = 30 * time.Second
	openCodeHeartbeatFutureSkew = 5 * time.Second
	openCodeJobTTL              = 10 * time.Minute
	openCodeHeartbeatDirName    = "heartbeats"
	openCodeLegacyHeartbeatFile = "heartbeat.json"
)

type openCodeReplyJob struct {
	ID        string    `json:"id"`
	SessionID string    `json:"sessionID"`
	Text      string    `json:"text"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
}

type openCodeReplyResult struct {
	OK    bool   `json:"ok"`
	Error string `json:"error,omitempty"`
}

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

func (r OpenCodeQueueRunner) Queue(ctx context.Context, sessionID, text string) error {
	sessionID = strings.TrimSpace(sessionID)
	text = strings.TrimSpace(text)
	if sessionID == "" {
		return fmt.Errorf("opencode reply: session id is empty")
	}
	if text == "" {
		return fmt.Errorf("opencode reply: message text is empty")
	}
	if err := ctx.Err(); err != nil {
		return err
	}

	dir := strings.TrimSpace(r.Dir)
	if dir == "" {
		dir = config.GetPaths().OpenCodeReplyDir
	}
	if err := requireOpenCodeHeartbeat(dir, time.Now()); err != nil {
		return err
	}

	jobID, err := randomReplyID()
	if err != nil {
		return err
	}
	now := time.Now()
	job := openCodeReplyJob{
		ID:        jobID,
		SessionID: sessionID,
		Text:      text,
		CreatedAt: now,
		ExpiresAt: now.Add(openCodeJobTTL),
	}
	pendingDir := filepath.Join(dir, "pending")
	resultDir := filepath.Join(dir, "results")
	if err := os.MkdirAll(pendingDir, privateDirPerm); err != nil {
		return fmt.Errorf("opencode reply: create queue: %w", err)
	}
	if err := os.MkdirAll(resultDir, privateDirPerm); err != nil {
		return fmt.Errorf("opencode reply: create result queue: %w", err)
	}
	data, err := json.Marshal(job)
	if err != nil {
		return fmt.Errorf("opencode reply: encode job: %w", err)
	}
	if err := writeFileAtomic(filepath.Join(pendingDir, jobID+".json"), append(data, '\n')); err != nil {
		return fmt.Errorf("opencode reply: persist job: %w", err)
	}

	// The plugin polls on a five-second heartbeat. Waiting at least two
	// cycles catches immediate API/auth failures while still treating a
	// durable enqueue as accepted if OpenCode is busy.
	resultPath := filepath.Join(resultDir, jobID+".json")
	waitCtx, cancel := context.WithTimeout(ctx, r.resultWait())
	defer cancel()
	ticker := time.NewTicker(r.pollInterval())
	defer ticker.Stop()
	for {
		finished, resultErr := consumeOpenCodeResult(resultPath)
		if resultErr != nil {
			return resultErr
		}
		if finished {
			return nil
		}
		select {
		case <-waitCtx.Done():
			// The job is already durably queued. Once persisted, a caller
			// timeout or cancellation cannot prove that the plugin did not
			// claim it. Treat it as accepted and observe asynchronously instead
			// of inviting a retry that could execute twice.
			r.startOpenCodeAsyncObserver(dir, jobID, sessionID, text, job.ExpiresAt)
			return nil
		case <-ticker.C:
		}
	}
}

func (r OpenCodeQueueRunner) resultWait() time.Duration {
	if r.ResultWait > 0 {
		return r.ResultWait
	}
	return defaultOpenCodeResultWait
}

func (r OpenCodeQueueRunner) pollInterval() time.Duration {
	if r.PollInterval > 0 {
		return r.PollInterval
	}
	return defaultOpenCodePollInterval
}

func readOpenCodeResult(path string) (openCodeReplyResult, bool, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return openCodeReplyResult{}, false, nil
		}
		return openCodeReplyResult{}, false, fmt.Errorf("opencode reply: read result: %w", err)
	}
	var result openCodeReplyResult
	if err := json.Unmarshal(data, &result); err != nil {
		return openCodeReplyResult{}, false, fmt.Errorf("opencode reply: invalid result: %w", err)
	}
	return result, true, nil
}

func randomReplyID() (string, error) {
	var value [replyIDBytes]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", fmt.Errorf("opencode reply: random id: %w", err)
	}
	return hex.EncodeToString(value[:]), nil
}

func writeFileAtomic(path string, data []byte) error {
	tmp := fmt.Sprintf("%s.tmp-%d", path, os.Getpid())
	file, err := os.OpenFile(tmp, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, privateFilePerm)
	if err != nil {
		return err
	}
	_ = file.Chmod(privateFilePerm)
	if _, err := file.Write(data); err != nil {
		_ = file.Close()
		_ = os.Remove(tmp)
		return err
	}
	if err := file.Sync(); err != nil {
		_ = file.Close()
		_ = os.Remove(tmp)
		return err
	}
	if err := file.Close(); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	return nil
}
