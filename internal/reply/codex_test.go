package reply

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"
)

type processRunnerFunc func(context.Context, string, ...string) ([]byte, error)

func (f processRunnerFunc) Run(ctx context.Context, binary string, args ...string) ([]byte, error) {
	return f(ctx, binary, args...)
}

func TestCodexQueueArgs(t *testing.T) {
	got := codexQueueArgs(" thread-1 ", " 继续检查 ")
	want := []string{"queue", "--thread=thread-1", "--message=继续检查"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("codexQueueArgs() = %#v, want %#v", got, want)
	}
}

func TestCodexQueueArgsAcceptsOptionLikeReplyText(t *testing.T) {
	got := codexQueueArgs("-thread-name", "--help")
	want := []string{"queue", "--thread=-thread-name", "--message=--help"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("codexQueueArgs() = %#v, want %#v", got, want)
	}
}

func TestCodexQueueRunnerUsesArgumentArrayAndTimeout(t *testing.T) {
	var gotBinary string
	var gotArgs []string
	var remaining time.Duration
	runner := CodexQueueRunner{
		Binary:  "codex-test",
		Timeout: 2 * time.Second,
		Runner: processRunnerFunc(func(ctx context.Context, binary string, args ...string) ([]byte, error) {
			gotBinary = binary
			gotArgs = append([]string(nil), args...)
			deadline, ok := ctx.Deadline()
			if !ok {
				t.Fatal("runner context has no deadline")
			}
			remaining = time.Until(deadline)
			return nil, nil
		}),
	}

	if err := runner.Queue(context.Background(), "thread-1", `继续 "检查" $HOME`); err != nil {
		t.Fatalf("Queue: %v", err)
	}
	if gotBinary != "codex-test" {
		t.Fatalf("binary = %q", gotBinary)
	}
	want := []string{"queue", "--thread=thread-1", "--message=" + `继续 "检查" $HOME`}
	if !reflect.DeepEqual(gotArgs, want) {
		t.Fatalf("args = %#v, want %#v", gotArgs, want)
	}
	if remaining <= 0 || remaining > 2*time.Second {
		t.Fatalf("remaining timeout = %v", remaining)
	}
}

func TestCodexQueueRunnerPreservesCanceledContext(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	runner := CodexQueueRunner{Binary: "codex-test"}
	if err := runner.Queue(ctx, "thread-1", "继续"); !errors.Is(err, context.Canceled) {
		t.Fatalf("Queue error = %v, want context.Canceled", err)
	}
}

func TestCodexQueueRunnerReportsTimeout(t *testing.T) {
	runner := CodexQueueRunner{
		Binary:  "codex-test",
		Timeout: 20 * time.Millisecond,
		Runner: processRunnerFunc(func(ctx context.Context, _ string, _ ...string) ([]byte, error) {
			<-ctx.Done()
			return nil, ctx.Err()
		}),
	}
	err := runner.Queue(context.Background(), "thread-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "超时") {
		t.Fatalf("Queue error = %v, want timeout", err)
	}
	if !errors.Is(err, ErrCodexQueueUnconfirmed) {
		t.Fatalf("Queue error = %v, want ErrCodexQueueUnconfirmed", err)
	}
}

func TestCodexQueueRunnerPropagatesFailureOutput(t *testing.T) {
	runner := CodexQueueRunner{
		Binary: "codex-test",
		Runner: processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
			return []byte("failed to write queue: disk is full"), errors.New("exit status 1")
		}),
	}
	err := runner.Queue(context.Background(), "thread-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "disk is full") {
		t.Fatalf("Queue error = %v, want command output", err)
	}
}

func TestCodexQueueRunnerExplainsMissingActiveTarget(t *testing.T) {
	runner := CodexQueueRunner{
		Binary: "codex-test",
		Runner: processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
			return []byte("Error: No active session found matching 'thread-1'."), errors.New("exit status 1")
		}),
	}
	err := runner.Queue(context.Background(), "thread-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "未找到可续聊") || !strings.Contains(err.Error(), "更新 Codex") {
		t.Fatalf("Queue error = %v, want actionable missing-target guidance", err)
	}
}

func TestCodexQueueRunnerExplainsMissingRollout(t *testing.T) {
	runner := CodexQueueRunner{
		Binary: "codex-test",
		Runner: processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
			return []byte("Error: failed to queue session message: thread/queue/add failed: failed to read thread: invalid thread-store request: no rollout found for thread id 019abc (code -32603)"), errors.New("exit status 1")
		}),
	}
	err := runner.Queue(context.Background(), "019abc", "继续")
	if err == nil || !strings.Contains(err.Error(), "不存在或已删除") || !strings.Contains(err.Error(), "确认该线程") {
		t.Fatalf("Queue error = %v, want actionable missing-thread guidance", err)
	}
}

func TestCodexQueueRunnerExplainsArchivedThread(t *testing.T) {
	runner := CodexQueueRunner{
		Binary: "codex-test",
		Runner: processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
			return []byte("Error: failed to queue session message: thread/queue/add failed: session 019abc is archived. Run `codex unarchive 019abc` to unarchive it first. (code -32600)"), errors.New("exit status 1")
		}),
	}
	err := runner.Queue(context.Background(), "019abc", "继续")
	if err == nil || !strings.Contains(err.Error(), "已归档") || !strings.Contains(err.Error(), "codex unarchive 019abc") {
		t.Fatalf("Queue error = %v, want actionable archived-thread guidance", err)
	}
}

func TestCodexQueueRunnerExplainsIncompatibleSession(t *testing.T) {
	runner := CodexQueueRunner{
		Binary: "codex-test",
		Runner: processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
			return []byte("the current session does not support thread/queue/add; update or restart"), errors.New("exit status 1")
		}),
	}
	err := runner.Queue(context.Background(), "thread-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "不支持 queue") || !strings.Contains(err.Error(), "更新 Codex") {
		t.Fatalf("Queue error = %v, want actionable compatibility guidance", err)
	}
}

func TestCheckCodexQueueUsesInjectedRunner(t *testing.T) {
	var gotBinary string
	var gotArgs []string
	binary, err := checkCodexQueue(
		context.Background(),
		"codex-test",
		processRunnerFunc(func(_ context.Context, command string, args ...string) ([]byte, error) {
			gotBinary = command
			gotArgs = append([]string(nil), args...)
			return []byte("Usage: codex queue"), nil
		}),
	)
	if err != nil {
		t.Fatalf("checkCodexQueue: %v", err)
	}
	if binary != "codex-test" || gotBinary != "codex-test" {
		t.Fatalf("binary = %q / %q, want codex-test", binary, gotBinary)
	}
	if want := []string{"queue", "--help"}; !reflect.DeepEqual(gotArgs, want) {
		t.Fatalf("args = %#v, want %#v", gotArgs, want)
	}
}

func TestCheckCodexQueueReportsUnsupportedCommand(t *testing.T) {
	_, err := checkCodexQueue(
		context.Background(),
		"codex-test",
		processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
			return []byte("error: unrecognized subcommand 'queue'"), errors.New("exit status 2")
		}),
	)
	if err == nil || !strings.Contains(err.Error(), "queue 不可用") || !strings.Contains(err.Error(), "unrecognized subcommand") {
		t.Fatalf("error = %v, want unsupported queue details", err)
	}
}

func TestCheckCodexQueueReportsTimeout(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	_, err := checkCodexQueue(
		ctx,
		"codex-test",
		processRunnerFunc(func(ctx context.Context, _ string, _ ...string) ([]byte, error) {
			<-ctx.Done()
			return nil, ctx.Err()
		}),
	)
	if err == nil || !strings.Contains(err.Error(), "超时") {
		t.Fatalf("error = %v, want timeout", err)
	}
}

func TestCodexQueueRunnerExplainsNewerCodexFailures(t *testing.T) {
	cases := []struct {
		name     string
		output   string
		expected []string
	}{
		{
			name:     "thread not found",
			output:   "Error: failed to queue session message: thread/queue/add failed: thread not found: 019abc (code -32600)",
			expected: []string{"不存在或已删除", "确认该线程"},
		},
		{
			name:     "ephemeral thread",
			output:   "Error: thread/queue/add failed: ephemeral thread does not support queued submissions: 019abc (code -32600)",
			expected: []string{"临时会话", "不支持引用续聊"},
		},
		{
			name:     "queue store unavailable",
			output:   "Error: thread/queue/add failed: user message queue is unavailable (code -32600)",
			expected: []string{"未启用可持久化的消息队列"},
		},
		{
			name:     "embedded app server conflict",
			output:   "Error: cannot queue through an embedded app server while a local app-server daemon is running; remove configuration overrides or use --remote",
			expected: []string{"本地服务状态冲突", "重启 Codex"},
		},
		{
			name:     "invalid thread id",
			output:   "Error: failed to queue session message: thread/queue/add failed: invalid thread id: bad id (code -32600)",
			expected: []string{"线程 ID 无效"},
		},
	}

	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			runner := CodexQueueRunner{
				Binary: "codex-test",
				Runner: processRunnerFunc(func(context.Context, string, ...string) ([]byte, error) {
					return []byte(testCase.output), errors.New("exit status 1")
				}),
			}
			err := runner.Queue(context.Background(), "019abc", "继续")
			if err == nil {
				t.Fatal("Queue error = nil, want failure")
			}
			for _, expected := range testCase.expected {
				if !strings.Contains(err.Error(), expected) {
					t.Fatalf("Queue error = %v, want %q", err, expected)
				}
			}
		})
	}
}
