//go:build windows

package ui

import (
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
)

func TestAgentCardUsesIntegrationStateInsteadOfSwitch(t *testing.T) {
	app := WidgetApp{
		onCodex: true,
		integrations: map[string]integration.Status{
			agentmeta.Codex: {
				Agent:   agentmeta.Codex,
				Enabled: true,
				State:   integration.StateError,
				Detail:  "Codex notify 仍指向 codex-computer-use.exe",
			},
		},
	}
	cards := app.agentCards(widgetLayoutRects())
	codex := cards[1]
	if codex.Enabled != true || codex.Label != "接入异常" || codex.Detail != app.integrations[agentmeta.Codex].Detail {
		t.Fatalf("codex card = %#v", codex)
	}
}

func TestHealthReportsPendingRestartInsteadOfNormal(t *testing.T) {
	app := WidgetApp{
		clawbotLoggedIn:     true,
		clawbotSessionReady: true,
		onCodex:             true,
		integrations: map[string]integration.Status{
			agentmeta.Codex: {
				Agent:   agentmeta.Codex,
				Enabled: true,
				State:   integration.StatePendingRestart,
			},
		},
	}
	_, label := app.health()
	if label != "待重启" {
		t.Fatalf("health label = %q, want 待重启", label)
	}
	if app.enabledIntegrationsReady() {
		t.Fatal("pending integration must not be reported as ready")
	}
}

func TestHealthReadyOnlyWhenRelevantIntegrationsAreConnected(t *testing.T) {
	app := WidgetApp{
		clawbotLoggedIn:     true,
		clawbotSessionReady: true,
		onCodex:             true,
		integrations: map[string]integration.Status{
			agentmeta.Codex: {
				Agent:   agentmeta.Codex,
				Enabled: true,
				State:   integration.StateConnected,
			},
		},
	}
	_, label := app.health()
	if label != "正常" || !app.enabledIntegrationsReady() {
		t.Fatalf("health = %q, ready = %v", label, app.enabledIntegrationsReady())
	}
}
