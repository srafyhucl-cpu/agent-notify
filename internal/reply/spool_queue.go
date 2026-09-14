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
)

const (
	defaultSpoolResultWait   = 10 * time.Second
	defaultSpoolPollInterval = 100 * time.Millisecond
	spoolJobTTL              = 10 * time.Minute
	replyIDBytes             = 16
)

// spoolReplyJob 是发送到桌面端 Agent 扩展的持久化回复任务。
type spoolReplyJob struct {
	ID        string `json:"id"`
	SessionID string `json:"sessionID"`
	Text      string `json:"text"`
	// TargetID 是桌面端 Agent 内部的会话标识；为空时扩展直接使用 SessionID。
	// 只有本地会话号与桌面端标识不一致的 Agent 才会填充。
	TargetID string `json:"targetID,omitempty"`
	// CWD 是目标会话的工作目录；扩展必须在该目录下恢复会话才能命中同一会话。
	CWD       string    `json:"cwd,omitempty"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
}

// spoolReplyResult 是扩展对一次持久化任务的最终确认。
type spoolReplyResult struct {
	OK bool `json:"ok"`
	// Code 是扩展给出的稳定失败原因，Go 侧据此映射用户可读提示。
	Code  string `json:"code,omitempty"`
	Error string `json:"error,omitempty"`
}

// spoolQueueConfig 描述某一 Agent 的本地文件队列差异；持久化与确认流程共用，
// 防止不同 Agent 各自实现一套容易出现重复投递的队列。
type spoolQueueConfig struct {
	Dir            string
	Label          string
	ResultWait     time.Duration
	AsyncWait      time.Duration
	PollInterval   time.Duration
	RequireReady   func(dir string, now time.Time) error
	ResultError    func(code, detail string) error
	Unconfirmed    error
	OnAsyncFailure func(sessionID, text string, err error)
	// SessionCWD 解析目标会话的工作目录；为空表示该 Agent 不需要工作目录。
	SessionCWD func(sessionID string) (string, error)
	// TargetID 解析桌面端 Agent 内部的会话标识；为空表示与 SessionID 相同。
	TargetID func(sessionID string) (string, error)
}

func (c spoolQueueConfig) Queue(ctx context.Context, sessionID, text string) error {
	label := strings.TrimSpace(c.Label)
	if label == "" {
		label = "reply queue"
	}
	sessionID = strings.TrimSpace(sessionID)
	text = strings.TrimSpace(text)
	if sessionID == "" {
		return fmt.Errorf("%s: session id is empty", label)
	}
	if text == "" {
		return fmt.Errorf("%s: message text is empty", label)
	}
	if err := ctx.Err(); err != nil {
		return err
	}

	dir := strings.TrimSpace(c.Dir)
	if dir == "" {
		return fmt.Errorf("%s: queue directory is not configured", label)
	}
	if c.RequireReady == nil {
		return fmt.Errorf("%s: readiness check is not configured", label)
	}
	now := time.Now()
	if err := c.RequireReady(dir, now); err != nil {
		return err
	}

	jobID, err := randomReplyID()
	if err != nil {
		return err
	}
	workingDirectory := ""
	if c.SessionCWD != nil {
		workingDirectory, err = c.SessionCWD(sessionID)
		if err != nil {
			return err
		}
	}
	targetID := ""
	if c.TargetID != nil {
		targetID, err = c.TargetID(sessionID)
		if err != nil {
			return err
		}
	}
	job := spoolReplyJob{
		ID:        jobID,
		SessionID: sessionID,
		Text:      text,
		TargetID:  targetID,
		CWD:       workingDirectory,
		CreatedAt: now,
		ExpiresAt: now.Add(spoolJobTTL),
	}
	pendingDir := filepath.Join(dir, "pending")
	resultDir := filepath.Join(dir, "results")
	if err := os.MkdirAll(pendingDir, privateDirPerm); err != nil {
		return fmt.Errorf("%s: create queue: %w", label, err)
	}
	if err := os.MkdirAll(resultDir, privateDirPerm); err != nil {
		return fmt.Errorf("%s: create result queue: %w", label, err)
	}
	data, err := json.Marshal(job)
	if err != nil {
		return fmt.Errorf("%s: encode job: %w", label, err)
	}
	if err := writeFileAtomic(filepath.Join(pendingDir, jobID+".json"), append(data, '\n')); err != nil {
		return fmt.Errorf("%s: persist job: %w", label, err)
	}

	resultPath := filepath.Join(resultDir, jobID+".json")
	waitCtx, cancel := context.WithTimeout(ctx, c.resultWait())
	defer cancel()
	ticker := time.NewTicker(c.pollInterval())
	defer ticker.Stop()
	for {
		finished, resultErr := consumeSpoolResult(resultPath, label, c.ResultError)
		if resultErr != nil {
			return resultErr
		}
		if finished {
			return nil
		}
		select {
		case <-waitCtx.Done():
			// 任务一旦落盘就可能被扩展领取。等待超时或调用取消都不能证明
			// 扩展没有执行，因此按已接收处理，并转入异步结果观察。
			c.startObserver(dir, job, label)
			return nil
		case <-ticker.C:
		}
	}
}

func (c spoolQueueConfig) resultWait() time.Duration {
	if c.ResultWait > 0 {
		return c.ResultWait
	}
	return defaultSpoolResultWait
}

func (c spoolQueueConfig) pollInterval() time.Duration {
	if c.PollInterval > 0 {
		return c.PollInterval
	}
	return defaultSpoolPollInterval
}

func (c spoolQueueConfig) asyncWait(expiresAt time.Time) time.Duration {
	remaining := time.Until(expiresAt)
	if remaining <= 0 {
		return 0
	}
	if c.AsyncWait > 0 && c.AsyncWait < remaining {
		return c.AsyncWait
	}
	return remaining
}

func (c spoolQueueConfig) startObserver(dir string, job spoolReplyJob, label string) {
	if c.OnAsyncFailure == nil {
		return
	}
	go c.watchResult(dir, job, label)
}

func (c spoolQueueConfig) watchResult(dir string, job spoolReplyJob, label string) {
	resultPath := filepath.Join(dir, "results", job.ID+".json")
	timer := time.NewTimer(c.asyncWait(job.ExpiresAt))
	defer timer.Stop()
	ticker := time.NewTicker(c.pollInterval())
	defer ticker.Stop()

	for {
		finished, err := consumeSpoolResult(resultPath, label, c.ResultError)
		if finished || err != nil {
			if err != nil {
				c.reportAsyncFailure(job.SessionID, job.Text, err)
			}
			return
		}

		select {
		case <-timer.C:
			finished, err := consumeSpoolResult(resultPath, label, c.ResultError)
			if finished || err != nil {
				if err != nil {
					c.reportAsyncFailure(job.SessionID, job.Text, err)
				}
				return
			}
			if c.Unconfirmed != nil {
				c.reportAsyncFailure(job.SessionID, job.Text, c.Unconfirmed)
			}
			return
		case <-ticker.C:
		}
	}
}

func (c spoolQueueConfig) reportAsyncFailure(sessionID, text string, err error) {
	if c.OnAsyncFailure != nil {
		c.OnAsyncFailure(sessionID, text, err)
	}
}

func consumeSpoolResult(
	path string,
	label string,
	mapError func(code, detail string) error,
) (bool, error) {
	result, found, err := readSpoolResult(path, label)
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
		if mapError != nil {
			return true, mapError(strings.TrimSpace(result.Code), detail)
		}
		return true, fmt.Errorf("%s: %s", label, detail)
	}
	if code := strings.TrimSpace(result.Code); code != "" && mapError != nil {
		return true, mapError(code, "")
	}
	return true, fmt.Errorf("%s: 会话 prompt 失败", label)
}

func readSpoolResult(path, label string) (spoolReplyResult, bool, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return spoolReplyResult{}, false, nil
		}
		return spoolReplyResult{}, false, fmt.Errorf("%s: read result: %w", label, err)
	}
	var result spoolReplyResult
	if err := json.Unmarshal(data, &result); err != nil {
		return spoolReplyResult{}, false, fmt.Errorf("%s: invalid result: %w", label, err)
	}
	return result, true, nil
}

func randomReplyID() (string, error) {
	var value [replyIDBytes]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", fmt.Errorf("reply queue: random id: %w", err)
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
