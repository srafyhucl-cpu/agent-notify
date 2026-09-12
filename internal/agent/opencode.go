package agent

import (
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// HandleOpenCode processes one OpenCode completion event.
func HandleOpenCode(title, summary, sessionID string, maxChars int, dryRun, noStdin bool) notify.NotifyResult {
	paths := config.GetPaths()
	if marker.IsOff(paths.OpenCodeMarker) {
		return notify.RecordSkipped(notify.NotifyOptions{
			Agent:     "opencode",
			SessionID: sessionID,
			Title:     title,
			Summary:   summary,
			MaxChars:  maxChars,
			DryRun:    dryRun,
		}, "OpenCode 推送已关闭")
	}
	return HandleNotify("opencode", title, summary, sessionID, maxChars, dryRun, noStdin)
}

func isDoNotDisturbTitle(title string) bool {
	return strings.Contains(title, "🔕") || strings.Contains(title, "[勿扰]")
}

func skippedForQuietHours() bool {
	cfg, err := config.LoadConfig("")
	if err != nil {
		return false
	}
	return config.IsInQuietHours(cfg.QuietHours, time.Now())
}
