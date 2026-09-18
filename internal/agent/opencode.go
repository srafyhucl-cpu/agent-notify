package agent

import (
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

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
