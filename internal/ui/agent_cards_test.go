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

func TestHealthReportsSetupFailure(t *testing.T) {
	app := WidgetApp{
		clawbotLoggedIn:     true,
		clawbotSessionReady: true,
		setupError:          "首次接入失败：hook 被占用",
	}
	_, label := app.health()
	if label != "接入异常" {
		t.Fatalf("health label = %q", label)
	}
}

func TestConnectionTextReportsSetupFailure(t *testing.T) {
	app := WidgetApp{setupError: "首次接入失败：hook 被占用"}
	title, detail, _ := app.connectionText()
	if title != "首次接入失败" || detail != app.setupError {
		t.Fatalf("connection text = (%q, %q)", title, detail)
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

func TestFocusedAgentHealthLinkage(t *testing.T) {
	app := WidgetApp{
		agentMode:     "single",
		currentAgent:  agentmeta.Antigravity,
		onAntigravity: false,
	}
	_, label := app.focusedAgentHealth()
	if label != "已暂停" {
		t.Fatalf("disabled agent health = %q, want 已暂停", label)
	}

	app.onAntigravity = true
	app.procStatus.AntigravityRunning = true
	app.integrations = map[string]integration.Status{
		agentmeta.Antigravity: {
			Agent:   agentmeta.Antigravity,
			Enabled: true,
			State:   integration.StateConnected,
		},
	}
	_, label = app.focusedAgentHealth()
	if label != "正常" {
		t.Fatalf("connected and running agent health = %q, want 正常", label)
	}

	app.procStatus.AntigravityRunning = false
	_, label = app.focusedAgentHealth()
	if label != "已接入" {
		t.Fatalf("connected not running agent health = %q, want 已接入", label)
	}

	app.integrations[agentmeta.Antigravity] = integration.Status{
		Agent:   agentmeta.Antigravity,
		Enabled: true,
		State:   integration.StatePendingRestart,
	}
	_, label = app.focusedAgentHealth()
	if label != "待重启" {
		t.Fatalf("pending restart agent health = %q, want 待重启", label)
	}

	app.integrations[agentmeta.Antigravity] = integration.Status{
		Agent:   agentmeta.Antigravity,
		Enabled: true,
		State:   integration.StateError,
	}
	_, label = app.focusedAgentHealth()
	if label != "异常" {
		t.Fatalf("error agent health = %q, want 异常", label)
	}
}

func TestCurrentHealthSwitchesBetweenSingleAndGrid(t *testing.T) {
	app := WidgetApp{
		agentMode:           "single",
		currentAgent:        agentmeta.Antigravity,
		onAntigravity:       true,
		clawbotLoggedIn:     true,
		clawbotSessionReady: true,
		procStatus:          ProcessStatus{AntigravityRunning: true},
		integrations: map[string]integration.Status{
			agentmeta.Antigravity: {
				Agent:   agentmeta.Antigravity,
				Enabled: true,
				State:   integration.StateConnected,
			},
			agentmeta.Codex: {
				Agent:   agentmeta.Codex,
				Enabled: true,
				State:   integration.StateError,
			},
		},
		onCodex: true,
	}

	// 单 Agent 模式下聚焦 Antigravity，状态为当前 Agent 的“正常”
	_, singleLabel := app.currentHealth()
	if singleLabel != "正常" {
		t.Fatalf("single mode currentHealth = %q, want 正常", singleLabel)
	}

	// 切换到网格展开模式，反映全局状态（由于 Codex Error，返回“接入异常”）
	app.agentMode = "grid"
	_, gridLabel := app.currentHealth()
	if gridLabel != "接入异常" {
		t.Fatalf("grid mode currentHealth = %q, want 接入异常", gridLabel)
	}
}

func TestFiveModulesLayoutVerticalRhythm(t *testing.T) {
	layout := widgetLayoutRects()

	// 模块 1：Header 按钮
	if layout.themeToggle.Top < 10 || layout.themeToggle.Bottom > 48 {
		t.Fatalf("header theme toggle button out of bounds: %+v", layout.themeToggle)
	}
	// 模块 2：核心卡片
	if layout.singleAgent.Top < 52 || layout.singleAgent.Bottom > 224 {
		t.Fatalf("core card out of bounds: %+v", layout.singleAgent)
	}
	if layout.switchAgent.Right >= layout.singleSwitch.Left {
		t.Fatalf("dropdown overlaps switch: switchAgent=%+v singleSwitch=%+v", layout.switchAgent, layout.singleSwitch)
	}
	// 模块 3：信息板块（最近推送）
	if layout.recent.Top < layout.singleAgent.Bottom || layout.recent.Bottom > 320 {
		t.Fatalf("recent card out of bounds: %+v", layout.recent)
	}
	// 模块 4：快捷操作四按钮（等宽均布）
	buttons := []RECT{layout.test, layout.history, layout.settings, layout.hide}
	for i, b := range buttons {
		if b.Top != 328 || b.Bottom != 384 {
			t.Fatalf("button %d top/bottom not aligned to 328/384: %+v", i, b)
		}
		if i > 0 && buttons[i-1].Right >= b.Left {
			t.Fatalf("button %d overlaps previous: %+v vs %+v", i, buttons[i-1], b)
		}
	}
	// 模块 5：底部状态条三元素（水平对齐）
	text := widgetTextRects()
	if text.footerVersion.Top != 394 || text.footerVersion.Bottom != 436 {
		t.Fatalf("version badge not aligned to 394/436: %+v", text.footerVersion)
	}
	if layout.update.Top != 394 || layout.update.Bottom != 436 {
		t.Fatalf("update button not aligned to 394/436: %+v", layout.update)
	}
	if layout.repair.Top != 394 || layout.repair.Bottom != 436 {
		t.Fatalf("repair button not aligned to 394/436: %+v", layout.repair)
	}
	if text.footerVersion.Right >= layout.update.Left || layout.update.Right >= layout.repair.Left {
		t.Fatalf("footer elements overlap: version=%+v update=%+v repair=%+v", text.footerVersion, layout.update, layout.repair)
	}
	// 检查统一边距规范：底部边距严格为 14px，与四周 padding 保持一致，彻底消除旧版的 28px 底部不规则空白
	if bottomGap := widgetHeight - layout.repair.Bottom; bottomGap != 14 {
		t.Fatalf("bottom margin = %d, want 14", bottomGap)
	}
}

func TestFocusedAgentHintLinkage(t *testing.T) {
	app := WidgetApp{
		currentAgent:  agentmeta.Antigravity,
		onAntigravity: true,
		procStatus:    ProcessStatus{AntigravityRunning: true},
		integrations: map[string]integration.Status{
			agentmeta.Antigravity: {
				Agent:   agentmeta.Antigravity,
				Enabled: true,
				State:   integration.StateConnected,
			},
		},
	}
	if hint := app.focusedAgentHint(); hint != "聚焦 Antigravity · 正常运行中" {
		t.Fatalf("running hint = %q, want 聚焦 Antigravity · 正常运行中", hint)
	}

	app.procStatus.AntigravityRunning = false
	if hint := app.focusedAgentHint(); hint != "聚焦 Antigravity · 已接入，待启动" {
		t.Fatalf("not running hint = %q, want 聚焦 Antigravity · 已接入，待启动", hint)
	}

	app.integrations[agentmeta.Antigravity] = integration.Status{
		Agent:   agentmeta.Antigravity,
		Enabled: true,
		State:   integration.StatePendingRestart,
	}
	if hint := app.focusedAgentHint(); hint != "聚焦 Antigravity · 待重启生效" {
		t.Fatalf("pending restart hint = %q, want 聚焦 Antigravity · 待重启生效", hint)
	}

	app.onAntigravity = false
	if hint := app.focusedAgentHint(); hint != "聚焦 Antigravity · 通知已暂停" {
		t.Fatalf("paused hint = %q, want 聚焦 Antigravity · 通知已暂停", hint)
	}
}

func TestCleanRecentPushTitle(t *testing.T) {
	tests := []struct {
		title string
		agent string
		want  string
	}{
		{"🟢【Antigravity】测试会话完成", "antigravity", "测试会话完成"},
		{"⚠️【Codex】标题读取失败", "codex", "标题读取失败"},
		{"【通知】普通任务完成", "", "普通任务完成"},
		{"普通任务无前缀", "antigravity", "普通任务无前缀"},
		{"【测试】连通性测试", "test", "连通性测试"},
	}
	for _, tt := range tests {
		if got := cleanRecentPushTitle(tt.title, tt.agent); got != tt.want {
			t.Errorf("cleanRecentPushTitle(%q, %q) = %q, want %q", tt.title, tt.agent, got, tt.want)
		}
	}
}

func TestAgentMenuLabels(t *testing.T) {
	app := WidgetApp{
		onAntigravity: true,
		procStatus:    ProcessStatus{AntigravityRunning: true},
		integrations: map[string]integration.Status{
			agentmeta.Antigravity: {
				Agent:   agentmeta.Antigravity,
				Enabled: true,
				State:   integration.StateConnected,
			},
			agentmeta.Codex: {
				Agent:   agentmeta.Codex,
				Enabled: true,
				State:   integration.StatePendingRestart,
			},
		},
		onCodex: true,
		onDevin: true,
	}
	if label := app.agentMenuLabel(agentmeta.Antigravity); label != "正常" {
		t.Fatalf("antigravity menu label = %q, want 正常", label)
	}
	if label := app.agentMenuLabel(agentmeta.Codex); label != "待重启" {
		t.Fatalf("codex menu label = %q, want 待重启", label)
	}
	if label := app.agentMenuLabel(agentmeta.Devin); label != "未配置" {
		t.Fatalf("devin menu label = %q, want 未配置", label)
	}
	if label := app.agentMenuLabel(agentmeta.OpenCode); label != "已暂停" {
		t.Fatalf("opencode menu label = %q, want 已暂停", label)
	}
}

func TestSingleAgentSwitchDoesNotToggleOnBadgeClick(t *testing.T) {
	app := WidgetApp{
		agentMode:     "single",
		currentAgent:  agentmeta.Antigravity,
		onAntigravity: true,
	}
	layout := widgetLayoutRects()
	badge := RECT{layout.singleAgent.Left + 14, layout.singleAgent.Top + 14, layout.singleAgent.Left + 50, layout.singleAgent.Top + 50}

	badgeCenterX := badge.Left + (badge.Right-badge.Left)/2
	badgeCenterY := badge.Top + (badge.Bottom-badge.Top)/2

	// Clicking badge in single mode should NOT toggle
	toggled := app.toggleAgentAt(badgeCenterX, badgeCenterY, layout)
	if toggled {
		t.Fatalf("toggleAgentAt on badge should return false, got true")
	}
	if !app.onAntigravity {
		t.Fatalf("onAntigravity was unexpectedly disabled by clicking badge")
	}
}
