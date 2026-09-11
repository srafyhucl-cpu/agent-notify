package agent

import (
	"fmt"
	"strings"

	"linkweixin/internal/config"
	"linkweixin/internal/marker"
	"linkweixin/internal/notify"
)

// HandleOpenCode processes an OpenCode notification event.
func HandleOpenCode(title, summary string, maxChars int, dryRun, noStdin bool) {
	paths := config.GetPaths()
	if marker.IsOff(paths.OpenCodeMarker) {
		return
	}

	// Read from stdin if summary is empty and stdin is piped
	if strings.TrimSpace(summary) == "" && !noStdin {
		if data := ReadPipedStdinNonBlocking(); len(data) > 0 {
			summary = DecodeConsoleBytes(data)
		}
	}

	res := notify.SendNotification(notify.NotifyOptions{
		Title:    title,
		Summary:  summary,
		MaxChars: maxChars,
		DryRun:   dryRun,
	})

	if dryRun && res.DryRunPayload != "" {
		fmt.Println(res.DryRunPayload)
	}
}
