package agent

import (
	"fmt"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// HandleNotify processes one notification from a generic CLI or plugin caller.
// Agent-specific policy such as OpenCode's marker must be applied by the
// caller before invoking it.
func HandleNotify(agentName, title, summary, sessionID string, maxChars int, dryRun, noStdin bool) notify.NotifyResult {
	opts := notify.NotifyOptions{
		Agent:     strings.TrimSpace(agentName),
		SessionID: sessionID,
		Title:     title,
		Summary:   summary,
		MaxChars:  maxChars,
		DryRun:    dryRun,
	}

	if isDoNotDisturbTitle(title) {
		return notify.RecordSkipped(opts, "标题包含勿扰标记")
	}
	if !dryRun && skippedForQuietHours() {
		return notify.RecordSkipped(opts, "当前处于勿扰时段")
	}

	if strings.TrimSpace(summary) == "" && !noStdin {
		if data := ReadPipedStdinNonBlocking(); len(data) > 0 {
			opts.Summary = DecodeConsoleBytes(data)
		}
	}

	result := notify.SendNotification(opts)
	if dryRun && result.DryRunPayload != "" {
		fmt.Println(result.DryRunPayload)
	}
	return result
}
