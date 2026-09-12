package agent

import (
	"fmt"
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
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "OpenCode 推送已关闭"}
	}
	if isDoNotDisturbTitle(title) {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "标题包含勿扰标记"}
	}
	if !dryRun && skippedForQuietHours() {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "当前处于勿扰时段"}
	}

	if strings.TrimSpace(summary) == "" && !noStdin {
		if data := ReadPipedStdinNonBlocking(); len(data) > 0 {
			summary = DecodeConsoleBytes(data)
		}
	}

	result := notify.SendNotification(notify.NotifyOptions{
		Agent:     "opencode",
		SessionID: sessionID,
		Title:     title,
		Summary:   summary,
		MaxChars:  maxChars,
		DryRun:    dryRun,
	})
	if dryRun && result.DryRunPayload != "" {
		fmt.Println(result.DryRunPayload)
	}
	return result
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
