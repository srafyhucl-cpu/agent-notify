package reply

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func writeDevinHeartbeat(t *testing.T, dir, instanceID string, heartbeat devinHeartbeat) {
	t.Helper()
	writeTestJSON(t, filepath.Join(dir, "heartbeats", instanceID+".json"), heartbeat)
}

// devinTestRunner 固定 Cascade 标识解析结果，避免单测依赖本机 Devin 桌面端状态库。
func devinTestRunner(dir string) DevinQueueRunner {
	return DevinQueueRunner{
		Dir: dir,
		ResolveCascadeID: func(sessionID string) (string, error) {
			return "acp/devin-cli/" + sessionID, nil
		},
	}
}

func TestDevinQueueRequiresFreshExtensionHeartbeat(t *testing.T) {
	dir := t.TempDir()
	runner := devinTestRunner(dir)
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil ||
		!strings.Contains(err.Error(), "扩展未运行") {
		t.Fatalf("Queue error = %v, want missing-extension error", err)
	}

	writeDevinHeartbeat(t, dir, "stale", devinHeartbeat{
		Ready:     true,
		Timestamp: time.Now().Add(-time.Minute),
	})
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil ||
		!strings.Contains(err.Error(), "离线") {
		t.Fatalf("Queue error = %v, want offline-extension error", err)
	}
}

func TestDevinQueueReportsUnsupportedDesktopCommand(t *testing.T) {
	dir := t.TempDir()
	writeDevinHeartbeat(t, dir, "extension-1", devinHeartbeat{
		Ready:     false,
		Timestamp: time.Now(),
	})

	err := devinTestRunner(dir).Queue(context.Background(), "session-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "精确回复能力") {
		t.Fatalf("Queue error = %v, want unsupported-desktop error", err)
	}
}

func TestDevinQueuePersistsJobAndReadsResult(t *testing.T) {
	dir := t.TempDir()
	writeDevinHeartbeat(t, dir, "extension-1", devinHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	runner := devinTestRunner(dir)

	done := make(chan error, 1)
	go func() {
		deadline := time.Now().Add(time.Second)
		for time.Now().Before(deadline) {
			names, _ := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
			if len(names) == 0 {
				time.Sleep(10 * time.Millisecond)
				continue
			}
			var job spoolReplyJob
			data, err := os.ReadFile(names[0])
			if err != nil || json.Unmarshal(data, &job) != nil {
				time.Sleep(10 * time.Millisecond)
				continue
			}
			if job.SessionID != "session-1" || job.Text != "继续检查" {
				done <- errors.New("job fields changed")
				return
			}
			if job.TargetID != "acp/devin-cli/session-1" {
				done <- errors.New("job target id changed")
				return
			}
			writeTestJSON(t, filepath.Join(dir, "results", job.ID+".json"), spoolReplyResult{OK: true})
			done <- nil
			return
		}
		done <- errors.New("timed out waiting for pending job")
	}()

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}

func TestDevinQueueReportsLateFailureWithoutRetry(t *testing.T) {
	dir := t.TempDir()
	writeDevinHeartbeat(t, dir, "extension-1", devinHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	reported := make(chan error, 1)
	runner := DevinQueueRunner{
		Dir:          dir,
		ResultWait:   10 * time.Millisecond,
		AsyncWait:    time.Second,
		PollInterval: 5 * time.Millisecond,
		ResolveCascadeID: func(sessionID string) (string, error) {
			return "acp/devin-cli/" + sessionID, nil
		},
		OnAsyncFailure: func(_ string, _ string, err error) {
			reported <- err
		},
	}
	if err := runner.Queue(context.Background(), "session-1", "继续"); err != nil {
		t.Fatalf("Queue: %v", err)
	}

	pending, err := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
	if err != nil || len(pending) != 1 {
		t.Fatalf("pending jobs = %#v, err=%v", pending, err)
	}
	jobID := strings.TrimSuffix(filepath.Base(pending[0]), ".json")
	writeTestJSON(t, filepath.Join(dir, "results", jobID+".json"), spoolReplyResult{
		OK:   false,
		Code: devinCodeDesktopUnavailable,
	})

	select {
	case err := <-reported:
		if err == nil || !strings.Contains(err.Error(), "精确回复能力") {
			t.Fatalf("async error = %v, want desktop capability guidance", err)
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for async failure")
	}
}

func TestDevinResultErrorsAreActionable(t *testing.T) {
	tests := []struct {
		code   string
		detail string
		want   string
	}{
		{code: devinCodeDesktopUnavailable, want: "精确回复能力"},
		{code: devinCodeInvalidJob, want: "重新引用原通知"},
		{code: devinCodeSessionNotFound, want: "会话不存在"},
		{code: devinCodeTurnFailed, detail: "network unavailable", want: "回复未执行成功：network unavailable"},
		{code: devinCodeAgentMissing, want: "未找到 Devin 桌面端自带的 Agent"},
		{code: devinCodeNotAuthenticated, want: "重新登录"},
		{code: devinCodeSessionLocked, want: "请等本轮结束后再回复"},
		{code: devinCodeWorkspaceUntrusted, want: "信任该工作区"},
		{detail: "cascade not found", want: "会话不存在"},
		{detail: "invalid_message", want: "重新引用原通知"},
		{detail: "command not found", want: "精确回复能力"},
	}
	for _, test := range tests {
		err := devinResultError(test.code, test.detail)
		if err == nil || !strings.Contains(err.Error(), test.want) {
			t.Fatalf("devinResultError(%q, %q) = %v, want %q", test.code, test.detail, err, test.want)
		}
	}
}
