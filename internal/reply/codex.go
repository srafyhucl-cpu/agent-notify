package reply

import (
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/sysproc"
)

const (
	defaultCodexQueueTimeout = 30 * time.Second
	codexQueueHelpTimeout    = 5 * time.Second
	maxCommandErrorRunes     = 500
)

// ErrCodexQueueUnconfirmed means the command ended after it may already have
// reached the Codex app server. Callers must not retry it automatically.
var ErrCodexQueueUnconfirmed = errors.New("codex queue: delivery unconfirmed")

// ProcessRunner executes one command without involving a shell.
type ProcessRunner interface {
	Run(ctx context.Context, binary string, args ...string) ([]byte, error)
}

type hiddenProcessRunner struct{}

func (hiddenProcessRunner) Run(ctx context.Context, binary string, args ...string) ([]byte, error) {
	command := exec.CommandContext(ctx, binary, args...)
	sysproc.ConfigureHidden(command)
	applyProcessEnv(command, ctx)
	return command.CombinedOutput()
}

// CodexQueueRunner submits one text message to an existing Codex thread.
type CodexQueueRunner struct {
	Binary  string
	Timeout time.Duration
	Runner  ProcessRunner
}

func (r CodexQueueRunner) Queue(ctx context.Context, threadID, text string) error {
	threadID = strings.TrimSpace(threadID)
	text = strings.TrimSpace(text)
	if threadID == "" {
		return fmt.Errorf("codex queue: thread id is empty")
	}
	if text == "" {
		return fmt.Errorf("codex queue: message text is empty")
	}
	if err := ctx.Err(); err != nil {
		return err
	}

	binary, err := resolveCodexBinary(r.Binary)
	if err != nil {
		return err
	}

	timeout := r.Timeout
	if timeout <= 0 {
		timeout = defaultCodexQueueTimeout
	}
	queueCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	runner := r.Runner
	if runner == nil {
		runner = hiddenProcessRunner{}
	}
	output, err := runner.Run(queueCtx, binary, codexQueueArgs(threadID, text)...)
	if err == nil {
		return nil
	}
	if ctxErr := ctx.Err(); errors.Is(ctxErr, context.Canceled) || errors.Is(ctxErr, context.DeadlineExceeded) {
		return fmt.Errorf("%w: %v", ErrCodexQueueUnconfirmed, ctxErr)
	}
	if queueCtx.Err() != nil {
		return fmt.Errorf("%w: 超时（%s）", ErrCodexQueueUnconfirmed, timeout)
	}
	return codexQueueFailure(threadID, output, err)
}

func codexQueueFailure(threadID string, output []byte, commandErr error) error {
	detail := compactCommandDetail(output)
	lower := strings.ToLower(detail)
	switch {
	case strings.Contains(lower, "no active session found"):
		return errors.New("Codex 未找到可续聊的目标线程：请确认线程 ID 正确，并更新 Codex 后重试")
	case strings.Contains(lower, "no rollout found for thread id"), strings.Contains(lower, "thread not found"):
		return errors.New("目标 Codex 线程不存在或已删除：请在 Codex 中确认该线程后重试")
	case strings.Contains(lower, "ephemeral thread does not support queued submissions"):
		return errors.New("目标 Codex 会话是临时会话，不支持引用续聊")
	case strings.Contains(lower, "user message queue is unavailable"):
		return errors.New("当前 Codex 未启用可持久化的消息队列，无法引用续聊：请更新 Codex 并重启会话后重试")
	case strings.Contains(lower, "cannot queue through an embedded app server"):
		return errors.New("Codex 本地服务状态冲突：请重启 Codex 后重新引用通知回复")
	case strings.Contains(lower, "invalid thread id"):
		return errors.New("目标 Codex 线程 ID 无效，无法续聊")
	case strings.Contains(lower, "is archived"):
		return fmt.Errorf("目标 Codex 线程已归档：请先运行 codex unarchive %s，再重新引用通知回复", threadID)
	case strings.Contains(lower, "does not support thread/queue/add"):
		return errors.New("当前 Codex 会话不支持 queue：请更新 Codex 并重启会话后重试")
	case detail == "":
		return fmt.Errorf("codex queue 失败: %w", commandErr)
	default:
		return fmt.Errorf("codex queue 失败: %w: %s", commandErr, detail)
	}
}

func compactCommandDetail(output []byte) string {
	return truncateRunes(strings.TrimSpace(string(output)), maxCommandErrorRunes)
}

func codexQueueArgs(threadID, text string) []string {
	return []string{"queue", "--thread=" + strings.TrimSpace(threadID), "--message=" + strings.TrimSpace(text)}
}

// CheckCodexQueue verifies that the installed Codex CLI exposes queue.
func CheckCodexQueue(ctx context.Context) (string, error) {
	return checkCodexQueue(ctx, "", nil)
}

func checkCodexQueue(ctx context.Context, binary string, runner ProcessRunner) (string, error) {
	resolved, err := resolveCodexBinary(binary)
	if err != nil {
		return "", err
	}
	binary = resolved
	if runner == nil {
		runner = hiddenProcessRunner{}
	}
	checkCtx, cancel := context.WithTimeout(ctx, codexQueueHelpTimeout)
	defer cancel()
	output, err := runner.Run(checkCtx, binary, "queue", "--help")
	if err != nil {
		if checkCtx.Err() != nil {
			return binary, fmt.Errorf("codex queue --help 超时")
		}
		detail := strings.TrimSpace(string(output))
		if detail == "" {
			return binary, fmt.Errorf("codex queue 不可用: %w", err)
		}
		return binary, fmt.Errorf("codex queue 不可用: %s", detail)
	}
	return binary, nil
}
