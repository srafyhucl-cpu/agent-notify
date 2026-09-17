package reply

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
)

const (
	lockHelperEnv        = "AGENT_NOTIFY_REPLY_LOCK_HELPER"
	lockHelperPathEnv    = "AGENT_NOTIFY_REPLY_LOCK_PATH"
	lockHelperLogEnv     = "AGENT_NOTIFY_REPLY_LOCK_LOG"
	lockHelperTagEnv     = "AGENT_NOTIFY_REPLY_LOCK_TAG"
	lockHelperReleaseEnv = "AGENT_NOTIFY_REPLY_LOCK_RELEASE"

	lockHelperEnabled    = "1"
	lockHelperRunPattern = "^TestReplyLockHelperProcess$"

	lockHelperWaitTimeout  = 10 * time.Second
	lockHelperPollInterval = 10 * time.Millisecond

	concurrentCompactionRounds       = 5
	concurrentCompactionWriters      = 32
	concurrentCompactionBacklogCount = 200

	expiredFixtureGrace = time.Minute
	activeRouteLifetime = time.Hour
)

type lockHelperProcess struct {
	command *exec.Cmd
	tag     string
	stdout  bytes.Buffer
	stderr  bytes.Buffer
}

// TestReplyLockHelperProcess is the child half of
// TestFileLockSerializesAcrossProcesses. It only does work when the parent set
// the helper environment, so a normal `go test` run skips it.
func TestReplyLockHelperProcess(t *testing.T) {
	if os.Getenv(lockHelperEnv) != lockHelperEnabled {
		t.Skip("helper process for TestFileLockSerializesAcrossProcesses")
	}
	lockPath := os.Getenv(lockHelperPathEnv)
	logPath := os.Getenv(lockHelperLogEnv)
	tag := os.Getenv(lockHelperTagEnv)
	releasePath := os.Getenv(lockHelperReleaseEnv)
	if lockPath == "" || logPath == "" || tag == "" {
		t.Fatal("helper environment is incomplete")
	}

	if err := appendLockMarker(logPath, tag+"-attempt"); err != nil {
		t.Fatalf("record attempt: %v", err)
	}
	err := withFileLock(lockPath, func() error {
		if err := appendLockMarker(logPath, tag+"-start"); err != nil {
			return err
		}
		if releasePath != "" {
			if err := waitForFile(releasePath, lockHelperWaitTimeout); err != nil {
				return err
			}
		}
		return appendLockMarker(logPath, tag+"-end")
	})
	if err != nil {
		t.Fatalf("helper lock: %v", err)
	}
}

// TestFileLockSerializesAcrossProcesses proves the Windows LockFileEx path (and
// the Unix flock fallback) excludes another process, which is what makes the
// inbound reply claim at-most-once across the widget and CLI processes.
func TestFileLockSerializesAcrossProcesses(t *testing.T) {
	dir := t.TempDir()
	lockPath := filepath.Join(dir, "state.jsonl.lock")
	logPath := filepath.Join(dir, "lock-order.log")
	releasePath := filepath.Join(dir, "release-first")

	first := startLockHelper(t, lockPath, logPath, "first", releasePath)
	t.Cleanup(func() { _ = os.WriteFile(releasePath, []byte("release"), privateFilePerm) })
	waitForLockMarker(t, logPath, "first-start", lockHelperWaitTimeout)
	second := startLockHelper(t, lockPath, logPath, "second", "")
	waitForLockMarker(t, logPath, "second-attempt", lockHelperWaitTimeout)
	if err := os.WriteFile(releasePath, []byte("release"), privateFilePerm); err != nil {
		t.Fatalf("release first helper: %v", err)
	}

	first.wait(t)
	second.wait(t)

	markers := readLockMarkers(t, logPath)
	want := []string{"first-attempt", "first-start", "second-attempt", "first-end", "second-start", "second-end"}
	if !slices.Equal(markers, want) {
		t.Fatalf("lock order = %v, want %v", markers, want)
	}
}

// TestStateStoreClaimTimesOutWhileLockHeld proves Claim gives up after
// lockAcquireTimeout instead of blocking forever when another process hangs
// while holding the local state lock. The holder is a real child process so the
// assertion holds on both the Windows LockFileEx and the Unix flock paths.
func TestStateStoreClaimTimesOutWhileLockHeld(t *testing.T) {
	dir := t.TempDir()
	store := NewStateStore(filepath.Join(dir, "state.jsonl"))
	logPath := filepath.Join(dir, "lock-order.log")
	releasePath := filepath.Join(dir, "release-holder")

	holder := startLockHelper(t, store.Path+".lock", logPath, "holder", releasePath)
	t.Cleanup(func() { _ = os.WriteFile(releasePath, []byte("release"), privateFilePerm) })
	waitForLockMarker(t, logPath, "holder-start", lockHelperWaitTimeout)

	previousTimeout := lockAcquireTimeout
	lockAcquireTimeout = 200 * time.Millisecond
	t.Cleanup(func() { lockAcquireTimeout = previousTimeout })

	start := time.Now()
	claimed, err := store.Claim("message:timeout")
	elapsed := time.Since(start)

	if claimed {
		t.Fatal("Claim succeeded while another process held the state lock")
	}
	if err == nil {
		t.Fatal("Claim returned no error while another process held the state lock")
	}
	if !strings.Contains(err.Error(), "等待本地状态文件锁超时") {
		t.Fatalf("Claim error = %v, want lock acquire timeout", err)
	}
	if elapsed < lockAcquireTimeout {
		t.Fatalf("Claim returned after %s, want at least %s", elapsed, lockAcquireTimeout)
	}
	if elapsed > lockAcquireTimeout+2*time.Second {
		t.Fatalf("Claim waited %s, want it bounded near %s", elapsed, lockAcquireTimeout)
	}

	if err := os.WriteFile(releasePath, []byte("release"), privateFilePerm); err != nil {
		t.Fatalf("release holder: %v", err)
	}
	holder.wait(t)
}

func startLockHelper(t *testing.T, lockPath, logPath, tag, releasePath string) *lockHelperProcess {
	t.Helper()
	helper := &lockHelperProcess{tag: tag}
	helper.command = exec.Command(os.Args[0], "-test.run="+lockHelperRunPattern)
	helper.command.Env = append(os.Environ(),
		lockHelperEnv+"="+lockHelperEnabled,
		lockHelperPathEnv+"="+lockPath,
		lockHelperLogEnv+"="+logPath,
		lockHelperTagEnv+"="+tag,
		lockHelperReleaseEnv+"="+releasePath,
	)
	helper.command.Stdout = &helper.stdout
	helper.command.Stderr = &helper.stderr
	if err := helper.command.Start(); err != nil {
		t.Fatalf("start %s helper: %v", tag, err)
	}
	t.Cleanup(func() {
		if helper.command.ProcessState != nil {
			return
		}
		_ = helper.command.Process.Kill()
		_ = helper.command.Wait()
	})
	return helper
}

func (h *lockHelperProcess) wait(t *testing.T) {
	t.Helper()
	if err := h.command.Wait(); err != nil {
		t.Fatalf(
			"%s helper: %v\nstdout:\n%s\nstderr:\n%s",
			h.tag,
			err,
			h.stdout.String(),
			h.stderr.String(),
		)
	}
}

func appendLockMarker(path, marker string) error {
	file, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, privateFilePerm)
	if err != nil {
		return err
	}
	defer file.Close()
	if _, err := file.WriteString(marker + "\n"); err != nil {
		return err
	}
	return file.Sync()
}

func readLockMarkers(t *testing.T, path string) []string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read lock markers: %v", err)
	}
	return parseLockMarkers(data)
}

func parseLockMarkers(data []byte) []string {
	lines := strings.Split(string(data), "\n")
	markers := make([]string, 0, len(lines))
	for _, line := range lines {
		if marker := strings.TrimSpace(line); marker != "" {
			markers = append(markers, marker)
		}
	}
	return markers
}

func waitForLockMarker(t *testing.T, path, marker string, timeout time.Duration) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if slices.Contains(readLockMarkersIfPresent(path), marker) {
			return
		}
		time.Sleep(lockHelperPollInterval)
	}
	t.Fatalf("timed out waiting for lock marker %q", marker)
}

func waitForFile(path string, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for {
		if _, err := os.Stat(path); err == nil {
			return nil
		} else if !os.IsNotExist(err) {
			return err
		}
		if !time.Now().Before(deadline) {
			return fmt.Errorf("timed out waiting for file %q", path)
		}
		time.Sleep(lockHelperPollInterval)
	}
}

func readLockMarkersIfPresent(path string) []string {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	return parseLockMarkers(data)
}

// withTinyCompaction makes every append consider a rewrite, so the concurrent
// tests below exercise the lock around append + compaction instead of append
// alone.
func withTinyCompaction(t *testing.T) {
	t.Helper()
	previous := defaultJSONLCompactionOptions
	defaultJSONLCompactionOptions = testJSONLCompactionOptions()
	t.Cleanup(func() { defaultJSONLCompactionOptions = previous })
}

func expiredRouteFixtures(now time.Time, count int) []any {
	fixtures := make([]any, 0, count)
	for index := 0; index < count; index++ {
		fixtures = append(fixtures, Route{
			MessageID: fmt.Sprintf("expired-%03d", index),
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: fmt.Sprintf("thread-expired-%03d", index),
			CreatedAt: now.Add(-2 * DefaultRouteTTL),
			ExpiresAt: now.Add(-expiredFixtureGrace),
		})
	}
	return fixtures
}

func expiredStateFixtures(now time.Time, count int) []any {
	fixtures := make([]any, 0, count)
	for index := 0; index < count; index++ {
		fixtures = append(fixtures, stateEvent{
			Key:       fmt.Sprintf("expired-%03d", index),
			Status:    replyStateSent,
			Timestamp: now.Add(-DefaultStateTTL - expiredFixtureGrace),
		})
	}
	return fixtures
}

// TestConcurrentRouteRecordsSurviveCompaction keeps a stale backlog in the file
// so the first append triggers a rewrite while other appends are in flight. A
// missing lock loses those appends when the rewritten file replaces the old one.
func TestConcurrentRouteRecordsSurviveCompaction(t *testing.T) {
	withTinyCompaction(t)
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)

	for round := 0; round < concurrentCompactionRounds; round++ {
		store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
		store.Now = func() time.Time { return now }
		writeTestJSONL(t, store.Path, expiredRouteFixtures(now, concurrentCompactionBacklogCount)...)

		start := make(chan struct{})
		errs := make(chan error, concurrentCompactionWriters)
		var wait sync.WaitGroup
		for index := 0; index < concurrentCompactionWriters; index++ {
			wait.Add(1)
			go func(index int) {
				defer wait.Done()
				<-start
				errs <- store.Record(Route{
					MessageID: fmt.Sprintf("message-%02d", index),
					BotID:     "bot-1",
					UserID:    "user-1",
					Agent:     "codex",
					SessionID: fmt.Sprintf("thread-%02d", index),
					CreatedAt: now,
					ExpiresAt: now.Add(activeRouteLifetime),
				})
			}(index)
		}
		close(start)
		wait.Wait()
		close(errs)
		for err := range errs {
			if err != nil {
				t.Fatalf("round %d concurrent Record: %v", round, err)
			}
		}

		routes, err := store.load()
		if err != nil {
			t.Fatalf("round %d load routes: %v", round, err)
		}
		seen := make(map[string]struct{}, len(routes))
		for _, route := range routes {
			seen[route.MessageID] = struct{}{}
		}
		for index := 0; index < concurrentCompactionWriters; index++ {
			id := fmt.Sprintf("message-%02d", index)
			if _, ok := seen[id]; !ok {
				t.Fatalf("round %d lost route %s during compaction (kept %d)", round, id, len(routes))
			}
		}
	}
}

// TestConcurrentStateClaimsSurviveCompaction applies the same pressure to the
// inbound dedup store: every concurrent claim must remain durable, otherwise a
// duplicate WeChat delivery could execute the same reply twice.
func TestConcurrentStateClaimsSurviveCompaction(t *testing.T) {
	withTinyCompaction(t)
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)

	for round := 0; round < concurrentCompactionRounds; round++ {
		store := NewStateStore(filepath.Join(t.TempDir(), "state.jsonl"))
		store.Now = func() time.Time { return now }
		writeTestJSONL(t, store.Path, expiredStateFixtures(now, concurrentCompactionBacklogCount)...)

		keys := make([]string, concurrentCompactionWriters)
		for index := range keys {
			keys[index] = fmt.Sprintf("account:bot-1:%02d", index)
		}

		start := make(chan struct{})
		results := make(chan bool, concurrentCompactionWriters)
		errs := make(chan error, concurrentCompactionWriters)
		var wait sync.WaitGroup
		for _, key := range keys {
			wait.Add(1)
			go func(key string) {
				defer wait.Done()
				<-start
				claimed, err := store.Claim(key)
				results <- claimed
				errs <- err
			}(key)
		}
		close(start)
		wait.Wait()
		close(results)
		close(errs)

		for err := range errs {
			if err != nil {
				t.Fatalf("round %d concurrent Claim: %v", round, err)
			}
		}
		for claimed := range results {
			if !claimed {
				t.Fatalf("round %d rejected a first-time claim", round)
			}
		}

		verificationStore := NewStateStore(store.Path)
		verificationStore.Now = func() time.Time { return now }
		for _, key := range keys {
			claimed, err := verificationStore.Claim(key)
			if err != nil {
				t.Fatalf("round %d re-Claim %s: %v", round, key, err)
			}
			if claimed {
				t.Fatalf("round %d lost claim %s during compaction", round, key)
			}
		}
	}
}

func TestRouteStoreRejectsRouteExpiringExactlyNow(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }

	err := store.Record(Route{
		MessageID: "boundary",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
		CreatedAt: now.Add(-activeRouteLifetime),
		ExpiresAt: now,
	})
	if !errors.Is(err, ErrRouteExpired) {
		t.Fatalf("Record at expiry boundary = %v, want ErrRouteExpired", err)
	}
}

func TestStateStoreClaimAtExactTTLBoundary(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.jsonl")
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewStateStore(path)
	store.Now = func() time.Time { return now }

	claimed, err := store.Claim("message:boundary")
	if err != nil || !claimed {
		t.Fatalf("first Claim = %v, %v", claimed, err)
	}

	store.Now = func() time.Time { return now.Add(DefaultStateTTL) }
	claimed, err = store.Claim("message:boundary")
	if err != nil {
		t.Fatalf("boundary Claim: %v", err)
	}
	if !claimed {
		t.Fatal("claim survived at the exact TTL boundary; replay suppression must end there")
	}
}
