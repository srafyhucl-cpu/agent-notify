package agent

import (
	"os"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func sendWithAgentPolicy(opts notify.NotifyOptions, markerPath, disabledReason string) notify.NotifyResult {
	if marker.IsOff(markerPath) {
		return notify.RecordSkipped(opts, disabledReason)
	}
	if !opts.DryRun && skippedForQuietHours() {
		return notify.RecordSkipped(opts, "当前处于勿扰时段")
	}
	if isDoNotDisturbTitle(opts.Title) {
		return notify.RecordSkipped(opts, "标题包含勿扰标记")
	}
	return notify.SendNotification(opts)
}

func envFlag(name string) bool {
	value := strings.TrimSpace(os.Getenv(name))
	return value == "1" || strings.EqualFold(value, "true") || strings.EqualFold(value, "on")
}
