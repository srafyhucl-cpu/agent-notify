package reply

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

var errOpenCodeResultUnconfirmed = errors.New("opencode reply: 未在有效期内确认会话 prompt 执行结果")

// OpenCodeFailureReporter reports a durable OpenCode job that failed after the
// synchronous Queue result window. Implementations must not retry the job.
type OpenCodeFailureReporter func(sessionID, text string, err error)

func consumeOpenCodeResult(path string) (bool, error) {
	result, found, err := readOpenCodeResult(path)
	if err != nil {
		_ = os.Remove(path)
		return false, err
	}
	if !found {
		return false, nil
	}
	_ = os.Remove(path)
	if result.OK {
		return true, nil
	}
	if detail := strings.TrimSpace(result.Error); detail != "" {
		return true, openCodeResultError(detail)
	}
	return true, fmt.Errorf("opencode reply: 会话 prompt 失败")
}

func openCodeResultError(detail string) error {
	normalized := strings.ToLower(strings.TrimSpace(detail))
	if strings.Contains(normalized, "session.notfounderror") ||
		strings.Contains(normalized, "session not found") {
		return fmt.Errorf("opencode reply: 目标 OpenCode 会话不存在或已删除，请确认会话后再试")
	}
	return fmt.Errorf("opencode reply: %s", detail)
}

func (r OpenCodeQueueRunner) startOpenCodeAsyncObserver(
	dir string,
	jobID string,
	sessionID string,
	text string,
	expiresAt time.Time,
) {
	if r.OnAsyncFailure == nil {
		return
	}
	go r.watchOpenCodeResult(dir, jobID, sessionID, text, expiresAt)
}

func (r OpenCodeQueueRunner) watchOpenCodeResult(
	dir string,
	jobID string,
	sessionID string,
	text string,
	expiresAt time.Time,
) {
	resultPath := filepath.Join(dir, "results", jobID+".json")
	timeout := r.asyncWait(expiresAt)
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	ticker := time.NewTicker(r.pollInterval())
	defer ticker.Stop()

	for {
		finished, err := consumeOpenCodeResult(resultPath)
		if finished || err != nil {
			if err != nil {
				r.reportAsyncFailure(sessionID, text, err)
			}
			return
		}

		select {
		case <-timer.C:
			finished, err := consumeOpenCodeResult(resultPath)
			if finished || err != nil {
				if err != nil {
					r.reportAsyncFailure(sessionID, text, err)
				}
				return
			}
			r.reportAsyncFailure(sessionID, text, errOpenCodeResultUnconfirmed)
			return
		case <-ticker.C:
		}
	}
}

func (r OpenCodeQueueRunner) reportAsyncFailure(sessionID, text string, err error) {
	if r.OnAsyncFailure != nil {
		r.OnAsyncFailure(sessionID, text, err)
	}
}

func (r OpenCodeQueueRunner) asyncWait(expiresAt time.Time) time.Duration {
	remaining := time.Until(expiresAt)
	if remaining <= 0 {
		return 0
	}
	if r.AsyncWait > 0 && r.AsyncWait < remaining {
		return r.AsyncWait
	}
	return remaining
}
