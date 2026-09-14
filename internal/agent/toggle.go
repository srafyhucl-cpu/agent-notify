package agent

import (
	"fmt"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
)

// HandleToggle changes the marker for one agent or all supported agents.
func HandleToggle(agentName, mode string) error {
	paths := config.GetPaths()
	if mode == "" {
		mode = "Flip"
	}
	agentName = strings.ToLower(strings.TrimSpace(agentName))
	targets := []struct {
		name string
		path string
	}{
		{name: "opencode", path: paths.OpenCodeMarker},
		{name: "codex", path: paths.CodexMarker},
		{name: "antigravity", path: paths.AntigravityMarker},
		{name: "devin", path: paths.DevinMarker},
	}

	selected := targets
	if agentName != "" && agentName != "all" {
		selected = nil
		for _, target := range targets {
			if target.name == agentName {
				selected = []struct {
					name string
					path string
				}{target}
				break
			}
		}
		if len(selected) == 0 {
			return fmt.Errorf("unknown agent: %s", agentName)
		}
	}

	for _, target := range selected {
		result, err := marker.SetMarker(target.path, mode)
		if err != nil {
			return err
		}
		if len(selected) == 1 {
			fmt.Println(result)
			continue
		}
		fmt.Printf("%s: %s\n", target.name, result)
	}
	return nil
}
