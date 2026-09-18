package reply

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func writeCommandCodeHeartbeat(t *testing.T, dir, instance, sessionID string, ready bool, timestamp time.Time) {
	t.Helper()
	heartbeatDir := filepath.Join(dir, commandCodeHeartbeatDirName)
	if err := os.MkdirAll(heartbeatDir, privateDirPerm); err != nil {
		t.Fatal(err)
	}
	payload, err := json.Marshal(map[string]any{
		"ready":     ready,
		"timestamp": timestamp,
		"sessionId": sessionID,
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(heartbeatDir, instance+".json"), payload, privateFilePerm); err != nil {
		t.Fatal(err)
	}
}

func TestRequireCommandCodeHeartbeatRequiresMatchingSession(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 9, 18, 10, 0, 0, 0, time.UTC)
	writeCommandCodeHeartbeat(t, dir, "a", "session-1", true, now)

	if err := requireCommandCodeHeartbeat(dir, "session-1", now); err != nil {
		t.Fatalf("matching session heartbeat = %v, want nil", err)
	}
	// 心跳属于别的会话时必须明确失败，绝不回退到别的会话。
	err := requireCommandCodeHeartbeat(dir, "session-2", now)
	if err == nil || !strings.Contains(err.Error(), "目标会话未在运行") {
		t.Fatalf("other session error = %v", err)
	}
}

func TestRequireCommandCodeHeartbeatRejectsStaleAndUnsupported(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 9, 18, 10, 0, 0, 0, time.UTC)

	writeCommandCodeHeartbeat(t, dir, "stale", "session-1", true, now.Add(-2*time.Minute))
	if err := requireCommandCodeHeartbeat(dir, "session-1", now); err == nil || !strings.Contains(err.Error(), "目标会话未在运行") {
		t.Fatalf("stale heartbeat error = %v", err)
	}

	unsupportedDir := t.TempDir()
	writeCommandCodeHeartbeat(t, unsupportedDir, "noready", "session-1", false, now)
	if err := requireCommandCodeHeartbeat(unsupportedDir, "session-1", now); err == nil || !strings.Contains(err.Error(), "queueMessage") {
		t.Fatalf("unsupported heartbeat error = %v", err)
	}

	emptyDir := t.TempDir()
	if err := requireCommandCodeHeartbeat(emptyDir, "session-1", now); err == nil || !strings.Contains(err.Error(), "未运行") {
		t.Fatalf("missing heartbeat error = %v", err)
	}
}

func TestRequireCommandCodeHeartbeatIgnoresStaleDuplicateForLiveSession(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 9, 18, 10, 0, 0, 0, time.UTC)
	// 旧进程留下的同会话僵死心跳（文件名排序在前）不能让后面的新鲜心跳被忽略。
	writeCommandCodeHeartbeat(t, dir, "aaa-stale", "session-1", true, now.Add(-2*time.Minute))
	writeCommandCodeHeartbeat(t, dir, "zzz-live", "session-1", true, now)
	if err := requireCommandCodeHeartbeat(dir, "session-1", now); err != nil {
		t.Fatalf("stale duplicate shadowed the live heartbeat: %v", err)
	}
}

func TestCommandCodeQueuePersistsJobAndReadsResult(t *testing.T) {
	dir := t.TempDir()
	now := time.Now()
	writeCommandCodeHeartbeat(t, dir, "instance", "session-1", true, now)

	runner := CommandCodeQueueRunner{Dir: dir, ResultWait: 3 * time.Second, PollInterval: 20 * time.Millisecond}

	done := make(chan struct{})
	go func() {
		defer close(done)
		deadline := time.Now().Add(2 * time.Second)
		for time.Now().Before(deadline) {
			entries, err := os.ReadDir(filepath.Join(dir, "pending"))
			if err == nil {
				for _, entry := range entries {
					if !strings.HasSuffix(entry.Name(), ".json") {
						continue
					}
					data, err := os.ReadFile(filepath.Join(dir, "pending", entry.Name()))
					if err != nil {
						continue
					}
					var job spoolReplyJob
					if err := json.Unmarshal(data, &job); err != nil {
						continue
					}
					if job.SessionID != "session-1" || job.Text != "继续" {
						t.Errorf("job = %#v", job)
					}
					result, _ := json.Marshal(spoolReplyResult{OK: true})
					_ = os.MkdirAll(filepath.Join(dir, "results"), privateDirPerm)
					_ = os.WriteFile(filepath.Join(dir, "results", entry.Name()), result, privateFilePerm)
					return
				}
			}
			time.Sleep(10 * time.Millisecond)
		}
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := runner.Queue(ctx, "session-1", "继续"); err != nil {
		t.Fatalf("Queue = %v, want nil", err)
	}
	<-done
}

func TestCommandCodeQueueRejectsWhenSessionNotRunning(t *testing.T) {
	dir := t.TempDir()
	writeCommandCodeHeartbeat(t, dir, "instance", "session-other", true, time.Now())

	runner := CommandCodeQueueRunner{Dir: dir, ResultWait: time.Second, PollInterval: 20 * time.Millisecond}
	err := runner.Queue(context.Background(), "session-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "目标会话未在运行") {
		t.Fatalf("Queue error = %v", err)
	}
}

func TestCommandCodeResultErrorMapping(t *testing.T) {
	if err := commandCodeResultError("session_not_running", ""); err == nil || !strings.Contains(err.Error(), "未在运行") {
		t.Fatalf("session_not_running = %v", err)
	}
	if err := commandCodeResultError("session_idle", ""); err == nil || !strings.Contains(err.Error(), "空闲") {
		t.Fatalf("session_idle = %v", err)
	}
	if err := commandCodeResultError("window_closed", "回复窗口已过（通知发出后 60 秒内可引用回复）"); err == nil || !strings.Contains(err.Error(), "窗口已过") {
		t.Fatalf("window_closed = %v", err)
	}
	if err := commandCodeResultError("inject_failed", "queueMessage unavailable"); err == nil || !strings.Contains(err.Error(), "queueMessage unavailable") {
		t.Fatalf("inject_failed = %v", err)
	}
	if err := commandCodeResultError("", "some detail"); err == nil || !strings.Contains(err.Error(), "some detail") {
		t.Fatalf("fallback = %v", err)
	}
}

func TestDispatcherRegistersCommandCodeSender(t *testing.T) {
	dispatcher := NewDispatcher(DispatcherOptions{})
	sender, ok := dispatcher.senders["commandcode"]
	if !ok {
		t.Fatal("commandcode sender was not registered")
	}
	if _, ok := sender.(CommandCodeReplySender); !ok {
		t.Fatalf("commandcode sender type = %T", sender)
	}
}
