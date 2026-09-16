//go:build windows

package ui

import (
	"os"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
)

type widgetAgentCard struct {
	ID      string
	Name    string
	Enabled bool
	Running bool
	State   integration.State
	Label   string
	Detail  string
	Rect    RECT
	Hover   bool
}

func (app *WidgetApp) allAgentCards(layout widgetLayout) []widgetAgentCard {
	return []widgetAgentCard{
		app.agentCard(agentmeta.OpenCode, "OpenCode", app.onOpenCode, app.procStatus.OpenCodeRunning, layout.openCode, app.hover.openCode),
		app.agentCard(agentmeta.Codex, "Codex", app.onCodex, app.procStatus.CodexRunning, layout.codex, app.hover.codex),
		app.agentCard(agentmeta.Antigravity, "Antigravity", app.onAntigravity, app.procStatus.AntigravityRunning, layout.antigravity, app.hover.antigravity),
		app.agentCard(agentmeta.Devin, "Devin", app.onDevin, app.procStatus.DevinRunning, layout.devin, app.hover.devin),
	}
}

func (app *WidgetApp) agentCards(layout widgetLayout) []widgetAgentCard {
	cards := app.allAgentCards(layout)
	if app.isSingleAgentMode() {
		targetID := app.focusedAgentID()
		for _, card := range cards {
			if card.ID == targetID {
				card.Rect = layout.singleAgent
				card.Hover = app.hover.singleAgent
				return []widgetAgentCard{card}
			}
		}
		if len(cards) > 0 {
			first := cards[0]
			first.Rect = layout.singleAgent
			first.Hover = app.hover.singleAgent
			return []widgetAgentCard{first}
		}
	}
	return cards
}

func agentHookDescription(agentID string) string {
	switch agentID {
	case agentmeta.Antigravity:
		return "hooks.json (Stop Hook)"
	case agentmeta.OpenCode:
		return "agent-notify.ts (全局插件)"
	case agentmeta.Codex:
		return "config.toml (notify 包装)"
	case agentmeta.Devin:
		return "config.json + 回复扩展"
	default:
		return "系统配置"
	}
}

func (app *WidgetApp) agentCard(agentID, name string, enabled, running bool, rect RECT, hover bool) widgetAgentCard {
	status := app.integrationStatus(agentID)
	status.Enabled = enabled
	return widgetAgentCard{
		ID:      agentID,
		Name:    name,
		Enabled: enabled,
		Running: running,
		State:   status.State,
		Label:   status.Label(),
		Detail:  agentCardDetail(status, running),
		Rect:    rect,
		Hover:   hover,
	}
}

func agentCardDetail(status integration.Status, running bool) string {
	if !status.Enabled {
		return "通知已暂停"
	}
	switch status.State {
	case integration.StateConnected:
		if running {
			return "进程运行中，接入正常"
		}
		return "已接入，等待启动"
	case integration.StatePendingRestart:
		if status.Action != "" {
			return status.Action
		}
		return status.Detail
	case integration.StateError:
		return status.Detail
	default:
		if running {
			return "进程运行中但未接入"
		}
		return "未检测到有效配置"
	}
}

func (app *WidgetApp) refreshAgentSwitches() {
	app.onOpenCode = !marker.IsOff(app.paths.OpenCodeMarker)
	app.onCodex = !marker.IsOff(app.paths.CodexMarker)
	app.onAntigravity = !marker.IsOff(app.paths.AntigravityMarker)
	app.onDevin = !marker.IsOff(app.paths.DevinMarker)

	executable, _ := os.Executable()
	statuses := integration.CheckAll(integration.Options{
		Paths:      app.paths,
		Executable: executable,
		Enabled: map[string]bool{
			agentmeta.OpenCode:    app.onOpenCode,
			agentmeta.Codex:       app.onCodex,
			agentmeta.Antigravity: app.onAntigravity,
			agentmeta.Devin:       app.onDevin,
		},
		Now: time.Now(),
	})
	app.integrations = make(map[string]integration.Status, len(statuses))
	for _, status := range statuses {
		app.integrations[status.Agent] = status
	}
}

func (app *WidgetApp) integrationStatus(agentID string) integration.Status {
	if status, ok := app.integrations[agentID]; ok {
		return status
	}
	return integration.Status{
		Agent:   agentID,
		Enabled: app.agentEnabled(agentID),
		State:   integration.StateNotDetected,
		Detail:  "尚未完成接入检查",
	}
}

func (app *WidgetApp) agentEnabled(agentID string) bool {
	switch agentID {
	case agentmeta.OpenCode:
		return app.onOpenCode
	case agentmeta.Codex:
		return app.onCodex
	case agentmeta.Antigravity:
		return app.onAntigravity
	case agentmeta.Devin:
		return app.onDevin
	default:
		return false
	}
}

func (app *WidgetApp) agentRunning(agentID string) bool {
	switch agentID {
	case agentmeta.OpenCode:
		return app.procStatus.OpenCodeRunning
	case agentmeta.Codex:
		return app.procStatus.CodexRunning
	case agentmeta.Antigravity:
		return app.procStatus.AntigravityRunning
	case agentmeta.Devin:
		return app.procStatus.DevinRunning
	default:
		return false
	}
}

func (app *WidgetApp) agentIntegrationIssues() int {
	errors, restarts, missing := app.agentIntegrationCounts()
	return errors + restarts + missing
}

func (app *WidgetApp) agentIntegrationCounts() (errors, restarts, missing int) {
	for _, descriptor := range agentmeta.All() {
		if !app.agentEnabled(descriptor.ID) {
			continue
		}
		status := app.integrationStatus(descriptor.ID)
		switch status.State {
		case integration.StateError:
			errors++
		case integration.StatePendingRestart:
			restarts++
		case integration.StateNotDetected:
			if app.agentRunning(descriptor.ID) {
				missing++
			}
		}
	}
	return errors, restarts, missing
}

func (app *WidgetApp) enabledIntegrationsReady() bool {
	sawRelevant := false
	for _, descriptor := range agentmeta.All() {
		if !app.agentEnabled(descriptor.ID) {
			continue
		}
		status := app.integrationStatus(descriptor.ID)
		if status.State == integration.StateNotDetected && !app.agentRunning(descriptor.ID) {
			continue
		}
		sawRelevant = true
		if status.State != integration.StateConnected {
			return false
		}
	}
	return sawRelevant
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
	if app.isSingleAgentMode() {
		if pointInRect(x, y, layout.singleSwitch) {
			return app.toggleAgent(app.focusedAgentID())
		}
		return false
	}
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

func (app *WidgetApp) anyAgentEnabled() bool {
	return app.enabledAgentCount() > 0
}
