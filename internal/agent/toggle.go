package agent

import (
	"fmt"
	"strings"

	"linkweixin/internal/config"
	"linkweixin/internal/marker"
)

// HandleToggle handles toggling of markers for agents.
func HandleToggle(agentName, mode string) {
	paths := config.GetPaths()
	if mode == "" {
		mode = "Flip"
	}
	agentNameLower := strings.ToLower(strings.TrimSpace(agentName))
	if agentNameLower == "" || agentNameLower == "all" {
		oc, _ := marker.SetMarker(paths.OpenCodeMarker, mode)
		cx, _ := marker.SetMarker(paths.CodexMarker, mode)
		ag, _ := marker.SetMarker(paths.AntigravityMarker, mode)
		fmt.Printf("opencode: %s\n", oc)
		fmt.Printf("codex: %s\n", cx)
		fmt.Printf("antigravity: %s\n", ag)
		return
	}

	switch agentNameLower {
	case "opencode":
		res, _ := marker.SetMarker(paths.OpenCodeMarker, mode)
		fmt.Println(res)
	case "codex":
		res, _ := marker.SetMarker(paths.CodexMarker, mode)
		fmt.Println(res)
	case "antigravity":
		res, _ := marker.SetMarker(paths.AntigravityMarker, mode)
		fmt.Println(res)
	default:
		fmt.Printf("Unknown agent: %s\n", agentName)
	}
}
