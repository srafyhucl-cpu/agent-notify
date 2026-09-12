package agent

import (
	"fmt"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
)

// HandleToggle changes the marker for one agent or both agents.
func HandleToggle(agentName, mode string) error {
	paths := config.GetPaths()
	if mode == "" {
		mode = "Flip"
	}
	agentName = strings.ToLower(strings.TrimSpace(agentName))
	if agentName == "" || agentName == "all" {
		openCode, err := marker.SetMarker(paths.OpenCodeMarker, mode)
		if err != nil {
			return err
		}
		codex, err := marker.SetMarker(paths.CodexMarker, mode)
		if err != nil {
			return err
		}
		fmt.Printf("opencode: %s\n", openCode)
		fmt.Printf("codex: %s\n", codex)
		return nil
	}

	var markerPath string
	switch agentName {
	case "opencode":
		markerPath = paths.OpenCodeMarker
	case "codex":
		markerPath = paths.CodexMarker
	default:
		return fmt.Errorf("unknown agent: %s", agentName)
	}
	result, err := marker.SetMarker(markerPath, mode)
	if err != nil {
		return err
	}
	fmt.Println(result)
	return nil
}
