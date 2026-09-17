package reply

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func testSpoolConfig(dir string, ready func(dir string, now time.Time) error, asyncFailure func(string, string, error)) spoolQueueConfig {
	return spoolQueueConfig{
		Dir:            dir,
		Label:          "test queue",
		ResultWait:     80 * time.Millisecond,
		AsyncWait:      150 * time.Millisecond,
		PollInterval:   10 * time.Millisecond,
		RequireReady:   ready,
		OnAsyncFailure: asyncFailure,
	}
}

func readyOK(string, time.Time) error { return nil }

// waitForJob 等待 pending 目录里出现一条任务并返回它。
func waitForJob(t *testing.T, dir string) spoolReplyJob {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		entries, _ := os.ReadDir(filepath.Join(dir, "pending"))
		for _, entry := range entries {
			if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
				continue
			}
			data, err := os.ReadFile(filepath.Join(dir, "pending", entry.Name()))
			if err != nil {
				continue
			}
			var job spoolReplyJob
			if err := json.Unmarshal(data, &job); err != nil {
				t.Fatalf("decode job: %v", err)
			}
			return job
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatal("等待任务落盘超时")
	return spoolReplyJob{}
}

func writeSpoolResult(t *testing.T, dir, jobID string, result spoolReplyResult) {
	t.Helper()
	data, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "results", jobID+".json")
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
}

// 成功路径：任务落盘 → 扩展确认 → Queue 返回成功，且结果文件被消费。
func TestSpoolQueueSuccess(t *testing.T) {
	dir := t.TempDir()
	config := testSpoolConfig(dir, readyOK, nil)

	done := make(chan error, 1)
	go func() { done <- config.Queue(context.Background(), "session-1", "你好") }()

	job := waitForJob(t, dir)
	if job.SessionID != "session-1" || job.Text != "你好" || job.ID == "" {
		t.Fatalf("任务字段不符：%+v", job)
	}
	if job.CreatedAt.IsZero() || !job.ExpiresAt.After(job.CreatedAt) {
		t.Fatalf("任务时间戳异常：%+v", job)
	}
	writeSpoolResult(t, dir, job.ID, spoolReplyResult{OK: true})

	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("Queue 返回错误：%v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("Queue 未在结果写入后返回")
	}
	if _, err := os.Stat(filepath.Join(dir, "results", job.ID+".json")); !os.IsNotExist(err) {
		t.Fatal("结果文件未被消费")
	}
}

// 失败结果（error 文案）交给 ResultError 映射。
func TestSpoolQueueMapsResultError(t *testing.T) {
	dir := t.TempDir()
	mapped := errors.New("用户可读的失败提示")
	config := testSpoolConfig(dir, readyOK, nil)
	config.ResultError = func(code, detail string) error {
		if detail != "扩展执行失败" {
			t.Fatalf("detail = %q", detail)
		}
		return mapped
	}

	done := make(chan error, 1)
	go func() { done <- config.Queue(context.Background(), "session-1", "hi") }()

	job := waitForJob(t, dir)
	writeSpoolResult(t, dir, job.ID, spoolReplyResult{Code: "boom", Error: "扩展执行失败"})

	select {
	case err := <-done:
		if !errors.Is(err, mapped) {
			t.Fatalf("Queue error = %v, want mapped error", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("Queue 未返回")
	}
}

// 只有 code 时同样走映射。
func TestSpoolQueueMapsResultCodeOnly(t *testing.T) {
	dir := t.TempDir()
	gotCode := ""
	config := testSpoolConfig(dir, readyOK, nil)
	config.ResultError = func(code, detail string) error {
		gotCode = code
		return fmt.Errorf("mapped %s", code)
	}

	done := make(chan error, 1)
	go func() { done <- config.Queue(context.Background(), "session-1", "hi") }()

	job := waitForJob(t, dir)
	writeSpoolResult(t, dir, job.ID, spoolReplyResult{Code: "session_not_found"})

	if err := <-done; err == nil {
		t.Fatal("code-only 失败结果必须返回错误")
	}
	if gotCode != "session_not_found" {
		t.Fatalf("映射收到 code = %q", gotCode)
	}
}

// 结果文件损坏：返回错误并删除坏文件，避免反复重试。
func TestSpoolQueueRejectsBrokenResult(t *testing.T) {
	dir := t.TempDir()
	config := testSpoolConfig(dir, readyOK, nil)

	done := make(chan error, 1)
	go func() { done <- config.Queue(context.Background(), "session-1", "hi") }()

	job := waitForJob(t, dir)
	path := filepath.Join(dir, "results", job.ID+".json")
	if err := os.WriteFile(path, []byte("not-json"), 0o600); err != nil {
		t.Fatal(err)
	}

	select {
	case err := <-done:
		if err == nil || !strings.Contains(err.Error(), "invalid result") {
			t.Fatalf("Queue error = %v, want invalid result", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("Queue 未返回")
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatal("损坏的结果文件未被清理")
	}
}

// 等待超时按已接收处理；随后到达的失败结果由异步观察者回调。
func TestSpoolQueueAsyncFailureAfterTimeout(t *testing.T) {
	dir := t.TempDir()
	failures := make(chan error, 1)
	config := testSpoolConfig(dir, readyOK, func(sessionID, text string, err error) {
		if sessionID != "session-1" || text != "hi" {
			t.Errorf("异步回调参数不符：%q %q", sessionID, text)
		}
		failures <- err
	})

	if err := config.Queue(context.Background(), "session-1", "hi"); err != nil {
		t.Fatalf("超时应按已接收处理，actual error = %v", err)
	}
	job := waitForJob(t, dir)
	writeSpoolResult(t, dir, job.ID, spoolReplyResult{Error: "扩展稍后失败"})

	select {
	case err := <-failures:
		if err == nil || !strings.Contains(err.Error(), "扩展稍后失败") {
			t.Fatalf("异步失败回调 err = %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("异步观察者未回调失败")
	}
}

// 超时后没有任何结果：按过期回调 Unconfirmed。
func TestSpoolQueueAsyncUnconfirmed(t *testing.T) {
	dir := t.TempDir()
	unconfirmed := errors.New("未收到桌面端确认")
	failures := make(chan error, 1)
	config := testSpoolConfig(dir, readyOK, func(_, _ string, err error) { failures <- err })
	config.Unconfirmed = unconfirmed

	if err := config.Queue(context.Background(), "session-1", "hi"); err != nil {
		t.Fatalf("Queue: %v", err)
	}

	select {
	case err := <-failures:
		if !errors.Is(err, unconfirmed) {
			t.Fatalf("回调 err = %v, want Unconfirmed", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("未确认路径没有回调")
	}
}

// 没有观察者时超时不报错，也不产生后台 goroutine 行为差异。
func TestSpoolQueueTimeoutWithoutObserver(t *testing.T) {
	dir := t.TempDir()
	config := testSpoolConfig(dir, readyOK, nil)
	if err := config.Queue(context.Background(), "session-1", "hi"); err != nil {
		t.Fatalf("Queue: %v", err)
	}
}

// 参数与依赖校验：不应落盘任何任务。
func TestSpoolQueueValidation(t *testing.T) {
	dir := t.TempDir()
	ready := func(string, time.Time) error { return nil }
	cases := []struct {
		name    string
		config  spoolQueueConfig
		session string
		text    string
		want    string
	}{
		{"空会话", testSpoolConfig(dir, ready, nil), "", "hi", "session id is empty"},
		{"空文本", testSpoolConfig(dir, ready, nil), "s", "", "message text is empty"},
		{"缺少队列目录", testSpoolConfig("", ready, nil), "s", "hi", "queue directory is not configured"},
		{"缺少就绪检查", testSpoolConfig(dir, nil, nil), "s", "hi", "readiness check is not configured"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.config.Queue(context.Background(), tc.session, tc.text)
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error = %v, want %q", err, tc.want)
			}
		})
	}
	if entries, _ := os.ReadDir(filepath.Join(dir, "pending")); len(entries) != 0 {
		t.Fatalf("校验失败仍写入了任务：%v", entries)
	}
}

// 就绪检查与会话解析失败都要直接返回，不能落盘。
func TestSpoolQueuePropagatesCallbacks(t *testing.T) {
	dir := t.TempDir()
	blocked := errors.New("桌面端未就绪")
	config := testSpoolConfig(dir, func(string, time.Time) error { return blocked }, nil)
	if err := config.Queue(context.Background(), "session-1", "hi"); !errors.Is(err, blocked) {
		t.Fatalf("Queue error = %v, want blocked", err)
	}

	config = testSpoolConfig(dir, readyOK, nil)
	config.SessionCWD = func(string) (string, error) { return "", errors.New("找不到会话目录") }
	if err := config.Queue(context.Background(), "session-1", "hi"); err == nil {
		t.Fatal("会话目录解析失败必须返回错误")
	}
	if entries, _ := os.ReadDir(filepath.Join(dir, "pending")); len(entries) != 0 {
		t.Fatalf("回调失败仍写入了任务：%v", entries)
	}
}

// 任务里应带上解析出的 cwd 与 targetID。
func TestSpoolQueueCarriesSessionMetadata(t *testing.T) {
	dir := t.TempDir()
	config := testSpoolConfig(dir, readyOK, nil)
	config.SessionCWD = func(sessionID string) (string, error) { return `D:\work\` + sessionID, nil }
	config.TargetID = func(sessionID string) (string, error) { return "cascade-" + sessionID, nil }

	done := make(chan error, 1)
	go func() { done <- config.Queue(context.Background(), "session-1", "hi") }()
	job := waitForJob(t, dir)
	if job.CWD != `D:\work\session-1` || job.TargetID != "cascade-session-1" {
		t.Fatalf("任务元数据不符：%+v", job)
	}
	writeSpoolResult(t, dir, job.ID, spoolReplyResult{OK: true})
	if err := <-done; err != nil {
		t.Fatalf("Queue: %v", err)
	}
}

// 并发写同一路径时，临时文件必须唯一，最终内容应是某一次完整写入。
// Windows 上并发替换同一目标文件时，部分 writer 的 rename 会明确失败，
// 这是可接受的显式失败；这里只校验「不损坏」：至少一个成功、内容完整、无残留临时文件。
func TestWriteFileAtomicConcurrentWriters(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "heartbeat.json")

	const writers = 16
	payload := func(i int) []byte {
		return bytes.Repeat([]byte{byte('a' + i)}, 4096)
	}

	var wg sync.WaitGroup
	var mu sync.Mutex
	successCount := 0
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			if err := writeFileAtomic(path, payload(i)); err == nil {
				mu.Lock()
				successCount++
				mu.Unlock()
			}
		}(i)
	}
	wg.Wait()

	if successCount < 1 {
		t.Fatal("no writer succeeded, want at least one")
	}

	// 最终内容必须是某一次写入的完整载荷，证明没有交叉写入或截断。
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	matched := false
	for i := 0; i < writers; i++ {
		if bytes.Equal(data, payload(i)) {
			matched = true
			break
		}
	}
	if !matched {
		t.Fatalf("final content length %d does not match any writer payload", len(data))
	}

	leftovers, err := filepath.Glob(path + ".*.tmp")
	if err != nil {
		t.Fatal(err)
	}
	if len(leftovers) != 0 {
		t.Fatalf("temporary files remain: %#v", leftovers)
	}
}
