package reply

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/devinsession"
)

var errDevinResultUnconfirmed = errors.New("devin reply: 未在有效期内确认 Cascade 回复执行结果")

// DevinFailureReporter 接收同步等待结束后才发现的失败；回调实现不得重试任务。
type DevinFailureReporter func(sessionID, text string, err error)

// DevinQueueRunner 把回复投递到随 Agent-notify 安装的 Devin 扩展，由扩展调用
// Devin 桌面端内部的精确会话回复命令写回原 Cascade。整条链路不启动第二个
// Agent 进程，因此既不依赖 Devin CLI 登录状态，也不受会话锁或工作区信任影响。
type DevinQueueRunner struct {
	// Dir 是扩展轮询的回复收件箱；为空时使用配置路径。
	Dir string
	// ResultWait 控制同步错误可见窗口；超时表示任务已持久化接收。
	ResultWait time.Duration
	// AsyncWait 控制同步窗口结束后的失败观察时长；零值使用任务剩余 TTL。
	AsyncWait time.Duration
	// PollInterval 控制同步和异步结果检查间隔。
	PollInterval time.Duration
	// OnAsyncFailure 接收 Queue 返回后才发现的失败；为空时不启动异步观察。
	OnAsyncFailure DevinFailureReporter
	// ResolveCascadeID 把本地会话号解析成桌面端 Cascade 标识；为空时读取 Devin 桌面端会话元数据。
	ResolveCascadeID func(sessionID string) (string, error)
}

func (r DevinQueueRunner) Queue(ctx context.Context, sessionID, text string) error {
	dir := r.Dir
	if dir == "" {
		dir = config.GetPaths().DevinReplyDir
	}
	queue := spoolQueueConfig{
		Dir:          dir,
		Label:        "devin reply",
		ResultWait:   r.ResultWait,
		AsyncWait:    r.AsyncWait,
		PollInterval: r.PollInterval,
		RequireReady: requireDevinHeartbeat,
		ResultError:  devinResultError,
		Unconfirmed:  errDevinResultUnconfirmed,
		TargetID:     r.resolveCascadeID(),
	}
	if r.OnAsyncFailure != nil {
		queue.OnAsyncFailure = r.OnAsyncFailure
	}
	return queue.Queue(ctx, sessionID, text)
}

// resolveCascadeID 返回桌面端 Cascade 标识解析函数，未注入时使用桌面端会话元数据。
func (r DevinQueueRunner) resolveCascadeID() func(sessionID string) (string, error) {
	if r.ResolveCascadeID != nil {
		return r.ResolveCascadeID
	}
	return resolveDevinCascadeID
}

// resolveDevinCascadeID 把本地会话号解析为桌面端 Cascade 标识，并给出微信可读的失败原因。
func resolveDevinCascadeID(sessionID string) (string, error) {
	cascadeID, err := devinsession.LookupDesktopCascadeID(sessionID)
	if err != nil {
		if errors.Is(err, devinsession.ErrSessionNotFound) {
			return "", fmt.Errorf("Devin 桌面端尚未登记该会话，请先在 Devin 中打开该会话后再回复")
		}
		return "", fmt.Errorf("读取 Devin 桌面端会话信息失败：%v", err)
	}
	return cascadeID, nil
}
