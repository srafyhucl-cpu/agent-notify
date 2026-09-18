package reply

import (
	"context"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const commandCodeHeartbeatDirName = "heartbeats"

// CommandCodeQueueRunner 通过本地单向 spool 把回复交给 Command Code mod。
// mod 只能把消息注入自己所在的会话，因此就绪检查必须按目标会话匹配心跳。
type CommandCodeQueueRunner struct {
	// Dir 是 mod 的回复收件箱；为空时用配置路径。
	Dir string
	// ResultWait 界定同步错误可见窗口；超时表示任务已落盘，后续失败转异步观察。
	ResultWait time.Duration
	// AsyncWait 界定 ResultWait 之后的失败观察窗口；为零用剩余任务 TTL。
	AsyncWait time.Duration
	// PollInterval 控制同步与异步的结果轮询间隔。
	PollInterval time.Duration
	// OnAsyncFailure 接收 Queue 返回后才发现失败的任务；为空则不做后台观察。
	OnAsyncFailure OpenCodeFailureReporter
}

func (r CommandCodeQueueRunner) Queue(ctx context.Context, sessionID, text string) error {
	dir := r.Dir
	if dir == "" {
		dir = config.GetPaths().CommandCodeReplyDir
	}
	// spool 的就绪检查签名不带会话，这里闭包捕获目标会话，确保只认自己的那个。
	requireReady := func(targetDir string, now time.Time) error {
		return requireCommandCodeHeartbeat(targetDir, sessionID, now)
	}
	queue := newSpoolQueueConfig(
		dir,
		"commandcode reply",
		r.ResultWait,
		r.AsyncWait,
		r.PollInterval,
		requireReady,
		commandCodeResultError,
		errCommandCodeResultUnconfirmed,
		r.OnAsyncFailure,
	)
	return queue.Queue(ctx, sessionID, text)
}
