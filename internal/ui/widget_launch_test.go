//go:build windows

package ui

import "testing"

func TestResolveWidgetPosition(t *testing.T) {
	tests := []struct {
		name  string
		raw   string
		wantX int32
		wantY int32
	}{
		{"valid", "120,130", 120, 130},
		{"negative", "-32000,-32000", 1030, 285},
		{"off screen", "5000,5000", 1030, 285},
		{"invalid", "abc,def", 1030, 285},
		{"empty", "", 1030, 285},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			x, y := resolveWidgetPosition(tt.raw, 1460, 1140, widgetWidth, widgetHeight)
			if x != tt.wantX || y != tt.wantY {
				t.Fatalf("resolveWidgetPosition(%q) = (%d,%d), want (%d,%d)", tt.raw, x, y, tt.wantX, tt.wantY)
			}
		})
	}
}

func TestResolveWidgetPositionClampsRestoredWindowIntoWorkArea(t *testing.T) {
	workArea := RECT{Left: 0, Top: 0, Right: 1707, Bottom: 1019}
	x, y := resolveWidgetPositionInArea("1407,298", workArea, widgetWidth, widgetHeight)
	if x != 1287 || y != 298 {
		t.Fatalf("resolveWidgetPositionInArea() = (%d,%d), want (1287,298)", x, y)
	}
	if x+widgetWidth > workArea.Right-widgetMinimumMargin {
		t.Fatalf("widget right edge %d exceeds work area margin %d", x+widgetWidth, workArea.Right-widgetMinimumMargin)
	}
}

func TestResolveWidgetPositionUsesTaskbarExcludedWorkArea(t *testing.T) {
	workArea := RECT{Left: 0, Top: 0, Right: 1920, Bottom: 1040}
	_, y := resolveWidgetPositionInArea("", workArea, widgetWidth, widgetHeight)
	wantY := workArea.Top + (workArea.Bottom-workArea.Top-widgetHeight)/2
	if y != wantY {
		t.Fatalf("default y = %d, want %d", y, wantY)
	}
}

func TestWidgetLayoutHitTargetsDoNotOverlap(t *testing.T) {
	layout := widgetLayoutRects()
	targets := []struct {
		name string
		rect RECT
	}{
		{"opencode", layout.openCode},
		{"codex", layout.codex},
		{"antigravity", layout.antigravity},
		{"devin", layout.devin},
		{"recent", layout.recent},
		{"test", layout.test},
		{"settings", layout.settings},
		{"history", layout.history},
		{"hide", layout.hide},
		{"update", layout.update},
		{"repair", layout.repair},
	}
	for i := 0; i < len(targets); i++ {
		for j := i + 1; j < len(targets); j++ {
			if rectsOverlap(targets[i].rect, targets[j].rect) {
				t.Fatalf("%s overlaps %s", targets[i].name, targets[j].name)
			}
		}
	}
}

func TestSingleAgentControlsDoNotOverlap(t *testing.T) {
	layout := widgetLayoutRects()
	if rectsOverlap(layout.switchAgent, layout.singleSwitch) {
		t.Fatalf("switchAgent %+v overlaps singleSwitch %+v", layout.switchAgent, layout.singleSwitch)
	}
	if layout.switchAgent.Right >= layout.singleSwitch.Left {
		t.Fatalf("switchAgent right %d must be strictly before singleSwitch left %d", layout.switchAgent.Right, layout.singleSwitch.Left)
	}
}

func rectsOverlap(a, b RECT) bool {
	return a.Left < b.Right && a.Right > b.Left && a.Top < b.Bottom && a.Bottom > b.Top
}
