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
		{"negative", "-32000,-32000", 1030, 390},
		{"off screen", "5000,5000", 1030, 390},
		{"invalid", "abc,def", 1030, 390},
		{"empty", "", 1030, 390},
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

func TestWidgetLayoutHitTargetsDoNotOverlap(t *testing.T) {
	layout := widgetLayoutRects()
	targets := []struct {
		name string
		rect RECT
	}{
		{"opencode", layout.openCode},
		{"codex", layout.codex},
		{"connection", layout.connection},
		{"recent", layout.recent},
		{"test", layout.test},
		{"settings", layout.settings},
		{"history", layout.history},
		{"hide", layout.hide},
	}
	for i := 0; i < len(targets); i++ {
		for j := i + 1; j < len(targets); j++ {
			if rectsOverlap(targets[i].rect, targets[j].rect) {
				t.Fatalf("%s overlaps %s", targets[i].name, targets[j].name)
			}
		}
	}
}

func rectsOverlap(a, b RECT) bool {
	return a.Left < b.Right && a.Right > b.Left && a.Top < b.Bottom && a.Bottom > b.Top
}
