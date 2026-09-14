//go:build windows

package ui

import (
	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
)

type widgetAgentCard struct {
	ID      string
	Name    string
	Enabled bool
	Running bool
	Rect    RECT
	Hover   bool
}

func (app *WidgetApp) agentCards(layout widgetLayout) []widgetAgentCard {
	return []widgetAgentCard{
		{
			ID:      agentmeta.OpenCode,
			Name:    "OpenCode",
			Enabled: app.onOpenCode,
			Running: app.procStatus.OpenCodeRunning,
			Rect:    layout.openCode,
			Hover:   app.hover.openCode,
		},
		{
			ID:      agentmeta.Codex,
			Name:    "Codex",
			Enabled: app.onCodex,
			Running: app.procStatus.CodexRunning,
			Rect:    layout.codex,
			Hover:   app.hover.codex,
		},
		{
			ID:      agentmeta.Antigravity,
			Name:    "Antigravity",
			Enabled: app.onAntigravity,
			Running: app.procStatus.AntigravityRunning,
			Rect:    layout.antigravity,
			Hover:   app.hover.antigravity,
		},
		{
			ID:      agentmeta.Devin,
			Name:    "Devin",
			Enabled: app.onDevin,
			Running: app.procStatus.DevinRunning,
			Rect:    layout.devin,
			Hover:   app.hover.devin,
		},
	}
}

func (app *WidgetApp) refreshAgentSwitches() {
	app.onOpenCode = !marker.IsOff(app.paths.OpenCodeMarker)
	app.onCodex = !marker.IsOff(app.paths.CodexMarker)
	app.onAntigravity = !marker.IsOff(app.paths.AntigravityMarker)
	app.onDevin = !marker.IsOff(app.paths.DevinMarker)
}

func (app *WidgetApp) agentMarkerPath(agentID string) (string, bool) {
	switch agentID {
	case agentmeta.OpenCode:
		return app.paths.OpenCodeMarker, true
	case agentmeta.Codex:
		return app.paths.CodexMarker, true
	case agentmeta.Antigravity:
		return app.paths.AntigravityMarker, true
	case agentmeta.Devin:
		return app.paths.DevinMarker, true
	default:
		return "", false
	}
}

func (app *WidgetApp) toggleAgent(agentID string) bool {
	path, ok := app.agentMarkerPath(agentID)
	if !ok {
		return false
	}
	_, err := marker.SetMarker(path, "Flip")
	return err == nil
}

func (app *WidgetApp) toggleAgentAt(x, y int32, layout widgetLayout) bool {
	for _, card := range app.agentCards(layout) {
		if pointInRect(x, y, card.Rect) {
			return app.toggleAgent(card.ID)
		}
	}
	return false
}

func (app *WidgetApp) enabledAgentStates() map[string]bool {
	return map[string]bool{
		agentmeta.OpenCode:    app.onOpenCode,
		agentmeta.Codex:       app.onCodex,
		agentmeta.Antigravity: app.onAntigravity,
		agentmeta.Devin:       app.onDevin,
	}
}

func (app *WidgetApp) enabledAgentCount() int {
	count := 0
	for _, enabled := range app.enabledAgentStates() {
		if enabled {
			count++
		}
	}
	return count
}

func (app *WidgetApp) allAgentsEnabled() bool {
	return app.enabledAgentCount() == len(agentmeta.All())
}

func (app *WidgetApp) anyAgentEnabled() bool {
	return app.enabledAgentCount() > 0
}
