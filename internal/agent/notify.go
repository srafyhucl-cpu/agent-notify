package agent

import (
	"fmt"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// HandleNotify processes one notification from a generic CLI or plugin caller.
// 显式声明受支持的 agent 时按其本地开关（如 opencode.off、commandcode.off）强制执行策略；
// 未声明或未知 agent 的通用调用不做任何 agent 专属开关检查。
func HandleNotify(agentName, title, summary, sessionID string, maxChars int, dryRun, noStdin bool, replyWindowSec int) notify.NotifyResult {
	opts := notify.NotifyOptions{
		Agent:          strings.TrimSpace(agentName),
		SessionID:      sessionID,
		Title:          title,
		Summary:        summary,
		MaxChars:       maxChars,
		DryRun:         dryRun,
		ReplyWindowSec: replyWindowSec,
	}
	appendPipedSummary(&opts, noStdin)

	markerPath, disabledReason := agentMarkerPolicy(opts.Agent)
	result := sendWithAgentPolicy(opts, markerPath, disabledReason)
	if dryRun && result.DryRunPayload != "" {
		fmt.Println(result.DryRunPayload)
	}
	return result
}

// agentMarkerPolicy 返回显式声明的 agent 需要强制执行的本地开关。
// 未知 agent 返回空路径，表示不做 agent 专属开关检查。
func agentMarkerPolicy(agentName string) (string, string) {
	paths := config.GetPaths()
	switch strings.ToLower(strings.TrimSpace(agentName)) {
	case agentmeta.OpenCode:
		return paths.OpenCodeMarker, "OpenCode 推送已关闭"
	case agentmeta.CommandCode:
		return paths.CommandCodeMarker, "Command Code 推送已关闭"
	default:
		return "", ""
	}
}

func appendPipedSummary(opts *notify.NotifyOptions, noStdin bool) {
	if noStdin || opts == nil || strings.TrimSpace(opts.Summary) != "" {
		return
	}
	if data := ReadPipedStdinNonBlocking(); len(data) > 0 {
		opts.Summary = DecodeConsoleBytes(data)
	}
}
