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

func writeOpenCodeHeartbeat(t *testing.T, dir, instanceID string, heartbeat openCodeHeartbeat) {
	t.Helper()
	writeTestJSON(t, filepath.Join(dir, openCodeHeartbeatDirName, instanceID+".json"), heartbeat)
}

func TestOpenCodeQueueRequiresFreshPluginHeartbeat(t *testing.T) {
	dir := t.TempDir()
	runner := OpenCodeQueueRunner{Dir: dir}
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil {
		t.Fatal("Queue should fail without a heartbeat")
	}

	writeOpenCodeHeartbeat(t, dir, "unsupported", openCodeHeartbeat{
		Ready:     false,
		Timestamp: time.Now(),
	})
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil || !strings.Contains(err.Error(), "session.prompt") {
		t.Fatalf("Queue error = %v, want session prompt capability error", err)
	}
	if err := os.Remove(filepath.Join(dir, openCodeHeartbeatDirName, "unsupported.json")); err != nil {
		t.Fatal(err)
	}

	writeOpenCodeHeartbeat(t, dir, "stale", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now().Add(-time.Minute),
	})
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil || !strings.Contains(err.Error(), "离线") {
		t.Fatalf("Queue error = %v, want stale heartbeat error", err)
	}
	if err := os.Remove(filepath.Join(dir, openCodeHeartbeatDirName, "stale.json")); err != nil {
		t.Fatal(err)
	}

	writeOpenCodeHeartbeat(t, dir, "future", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now().Add(time.Minute),
	})
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil || !strings.Contains(err.Error(), "时间无效") {
		t.Fatalf("Queue error = %v, want future heartbeat error", err)
	}
}

func TestOpenCodeHeartbeatAggregatesInstances(t *testing.T) {
	dir := t.TempDir()
	now := time.Now()
	writeOpenCodeHeartbeat(t, dir, "stale-capable", openCodeHeartbeat{
		Ready:     true,
		Timestamp: now.Add(-time.Minute),
	})
	writeOpenCodeHeartbeat(t, dir, "fresh-unsupported", openCodeHeartbeat{
		Ready:     false,
		Timestamp: now,
	})
	if err := requireOpenCodeHeartbeat(dir, now); err == nil || !strings.Contains(err.Error(), "session.prompt") {
		t.Fatalf("heartbeat error = %v, want session prompt capability error", err)
	}

	writeOpenCodeHeartbeat(t, dir, "fresh-capable", openCodeHeartbeat{
		Ready:     true,
		Timestamp: now,
	})
	if err := requireOpenCodeHeartbeat(dir, now); err != nil {
		t.Fatalf("heartbeat error = %v, want a fresh capable instance", err)
	}
}

func TestOpenCodeQueuePersistsJobAndReadsResult(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	runner := OpenCodeQueueRunner{Dir: dir}

	done := make(chan struct{})
	go func() {
		defer close(done)
		deadline := time.Now().Add(time.Second)
		for time.Now().Before(deadline) {
			names, _ := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
			if len(names) > 0 {
				var job openCodeReplyJob
				data, err := os.ReadFile(names[0])
				if err == nil && json.Unmarshal(data, &job) == nil {
					writeTestJSON(t, filepath.Join(dir, "results", job.ID+".json"), openCodeReplyResult{OK: true})
					return
				}
			}
			time.Sleep(10 * time.Millisecond)
		}
	}()

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	<-done
}

func TestOpenCodeQueueTreatsResultTimeoutAsAccepted(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	runner := OpenCodeQueueRunner{
		Dir:          dir,
		ResultWait:   20 * time.Millisecond,
		PollInterval: 5 * time.Millisecond,
	}

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	pending, err := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
	if err != nil {
		t.Fatal(err)
	}
	if len(pending) != 1 {
		t.Fatalf("pending jobs = %#v, want one durable job", pending)
	}
}

func TestOpenCodeQueueTreatsCancellationAfterPersistAsAccepted(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	runner := OpenCodeQueueRunner{
		Dir:          dir,
		ResultWait:   time.Second,
		PollInterval: 5 * time.Millisecond,
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go func() {
		deadline := time.Now().Add(time.Second)
		for time.Now().Before(deadline) {
			pending, _ := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
			if len(pending) == 1 {
				cancel()
				return
			}
			time.Sleep(5 * time.Millisecond)
		}
	}()

	if err := runner.Queue(ctx, "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue after durable write: %v", err)
	}
	pending, err := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
	if err != nil {
		t.Fatal(err)
	}
	if len(pending) != 1 {
		t.Fatalf("pending jobs = %#v, want one durable job", pending)
	}
}

func TestOpenCodeQueueReportsLateSuccessWithoutFailure(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{Ready: true, Timestamp: time.Now()})
	reported := make(chan error, 1)
	runner := OpenCodeQueueRunner{
		Dir:          dir,
		ResultWait:   10 * time.Millisecond,
		AsyncWait:    time.Second,
		PollInterval: 5 * time.Millisecond,
		OnAsyncFailure: func(_ string, _ string, err error) {
			reported <- err
		},
	}

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	jobID := firstOpenCodeJobID(t, dir)
	resultPath := filepath.Join(dir, "results", jobID+".json")
	writeTestJSON(t, resultPath, openCodeReplyResult{OK: true})
	waitForFileRemoval(t, resultPath)

	select {
	case err := <-reported:
		t.Fatalf("unexpected async failure: %v", err)
	default:
	}
}

func TestOpenCodeQueueReportsLatePromptFailure(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{Ready: true, Timestamp: time.Now()})
	reported := make(chan error, 1)
	runner := OpenCodeQueueRunner{
		Dir:          dir,
		ResultWait:   10 * time.Millisecond,
		AsyncWait:    time.Second,
		PollInterval: 5 * time.Millisecond,
		OnAsyncFailure: func(_ string, _ string, err error) {
			reported <- err
		},
	}

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	jobID := firstOpenCodeJobID(t, dir)
	writeTestJSON(t, filepath.Join(dir, "results", jobID+".json"), openCodeReplyResult{
		OK:    false,
		Error: "session missing",
	})

	select {
	case err := <-reported:
		if err == nil || !strings.Contains(err.Error(), "session missing") {
			t.Fatalf("async error = %v, want session missing", err)
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for async failure")
	}
}

func TestOpenCodeQueueReportsUnconfirmedAsyncResult(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{Ready: true, Timestamp: time.Now()})
	reported := make(chan error, 1)
	runner := OpenCodeQueueRunner{
		Dir:          dir,
		ResultWait:   10 * time.Millisecond,
		AsyncWait:    30 * time.Millisecond,
		PollInterval: 5 * time.Millisecond,
		OnAsyncFailure: func(_ string, _ string, err error) {
			reported <- err
		},
	}

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	select {
	case err := <-reported:
		if err == nil || !strings.Contains(err.Error(), "未在有效期内确认") {
			t.Fatalf("async error = %v, want unconfirmed result", err)
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for unconfirmed result report")
	}
}

func TestOpenCodeQueueDoesNotObserveWithoutFailureReporter(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{Ready: true, Timestamp: time.Now()})
	runner := OpenCodeQueueRunner{
		Dir:          dir,
		ResultWait:   10 * time.Millisecond,
		AsyncWait:    time.Second,
		PollInterval: 5 * time.Millisecond,
	}

	if err := runner.Queue(context.Background(), "session-1", "继续检查"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	jobID := firstOpenCodeJobID(t, dir)
	resultPath := filepath.Join(dir, "results", jobID+".json")
	writeTestJSON(t, resultPath, openCodeReplyResult{OK: false, Error: "later failure"})
	time.Sleep(50 * time.Millisecond)
	if _, err := os.Stat(resultPath); err != nil {
		t.Fatalf("result file was consumed without a reporter: %v", err)
	}
}

func TestConsumeOpenCodeResultMapsSessionNotFound(t *testing.T) {
	path := filepath.Join(t.TempDir(), "result.json")
	writeTestJSON(t, path, openCodeReplyResult{OK: false, Error: "Session.NotFoundError"})

	finished, err := consumeOpenCodeResult(path)
	if !finished {
		t.Fatal("result was not marked finished")
	}
	if err == nil || !strings.Contains(err.Error(), "会话不存在或已删除") {
		t.Fatalf("consumeOpenCodeResult error = %v, want session-not-found message", err)
	}
	if _, statErr := os.Stat(path); !os.IsNotExist(statErr) {
		t.Fatalf("result file still exists: %v", statErr)
	}
}

func firstOpenCodeJobID(t *testing.T, dir string) string {
	t.Helper()
	pending, err := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
	if err != nil {
		t.Fatal(err)
	}
	if len(pending) != 1 {
		t.Fatalf("pending jobs = %#v, want one durable job", pending)
	}
	return strings.TrimSuffix(filepath.Base(pending[0]), ".json")
}

func waitForFileRemoval(t *testing.T, path string) {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(path); os.IsNotExist(err) {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s removal", path)
}

func TestOpenCodeQueueReportsInvalidResult(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	runner := OpenCodeQueueRunner{Dir: dir}

	done := make(chan struct{})
	go func() {
		defer close(done)
		deadline := time.Now().Add(time.Second)
		for time.Now().Before(deadline) {
			names, _ := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
			if len(names) == 0 {
				time.Sleep(10 * time.Millisecond)
				continue
			}
			jobID := strings.TrimSuffix(filepath.Base(names[0]), ".json")
			resultDir := filepath.Join(dir, "results")
			if err := os.MkdirAll(resultDir, privateDirPerm); err != nil {
				return
			}
			_ = os.WriteFile(filepath.Join(resultDir, jobID+".json"), []byte("{"), privateFilePerm)
			return
		}
	}()

	err := runner.Queue(context.Background(), "session-1", "继续检查")
	if err == nil || !strings.Contains(err.Error(), "invalid result") {
		t.Fatalf("Queue error = %v, want invalid result", err)
	}
	<-done
}

func TestOpenCodeQueuePropagatesPromptFailure(t *testing.T) {
	dir := t.TempDir()
	writeOpenCodeHeartbeat(t, dir, "plugin-1", openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	runner := OpenCodeQueueRunner{Dir: dir}

	done := make(chan struct{})
	go func() {
		defer close(done)
		deadline := time.Now().Add(time.Second)
		for time.Now().Before(deadline) {
			names, _ := filepath.Glob(filepath.Join(dir, "pending", "*.json"))
			if len(names) == 0 {
				time.Sleep(10 * time.Millisecond)
				continue
			}
			var job openCodeReplyJob
			data, err := os.ReadFile(names[0])
			if err == nil && json.Unmarshal(data, &job) == nil {
				writeTestJSON(t, filepath.Join(dir, "results", job.ID+".json"), openCodeReplyResult{
					OK:    false,
					Error: "session missing",
				})
				return
			}
		}
	}()

	err := runner.Queue(context.Background(), "session-1", "继续检查")
	if err == nil || !strings.Contains(err.Error(), "session missing") {
		t.Fatalf("Queue error = %v, want prompt failure", err)
	}
	<-done
}

func writeTestJSON(t *testing.T, path string, value any) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), privateDirPerm); err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, privateFilePerm); err != nil {
		t.Fatal(err)
	}
}

func TestOpenCodeQueueRejectsMalformedHeartbeat(t *testing.T) {
	dir := t.TempDir()
	heartbeatPath := filepath.Join(dir, openCodeHeartbeatDirName, "broken.json")
	if err := os.MkdirAll(filepath.Dir(heartbeatPath), privateDirPerm); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(heartbeatPath, []byte("{"), privateFilePerm); err != nil {
		t.Fatal(err)
	}
	runner := OpenCodeQueueRunner{Dir: dir}
	if err := runner.Queue(context.Background(), "session-1", "继续"); err == nil || !strings.Contains(err.Error(), "状态无效") {
		t.Fatalf("Queue error = %v, want malformed heartbeat", err)
	}
}

func TestWriteFileAtomicReplacesExistingFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "heartbeat.json")
	if err := os.WriteFile(path, []byte("old"), privateFilePerm); err != nil {
		t.Fatal(err)
	}
	if err := writeFileAtomic(path, []byte("new")); err != nil {
		t.Fatalf("writeFileAtomic: %v", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "new" {
		t.Fatalf("content = %q, want new", data)
	}
	leftovers, err := filepath.Glob(path + ".*.tmp")
	if err != nil {
		t.Fatal(err)
	}
	if len(leftovers) != 0 {
		t.Fatalf("temporary files remain: %#v", leftovers)
	}
}
