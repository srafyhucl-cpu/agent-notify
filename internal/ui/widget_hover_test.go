//go:build windows

package ui

import "testing"

func TestWidgetHoverAtMapsEveryInteractiveRegion(t *testing.T) {
	layout := widgetLayoutRects()
	tests := []struct {
		name  string
		rect  RECT
		hover func(widgetHoverState) bool
	}{
		{"opencode", layout.openCode, func(h widgetHoverState) bool { return h.openCode }},
		{"codex", layout.codex, func(h widgetHoverState) bool { return h.codex }},
		{"antigravity", layout.antigravity, func(h widgetHoverState) bool { return h.antigravity }},
		{"devin", layout.devin, func(h widgetHoverState) bool { return h.devin }},
		{"minimize", layout.minimize, func(h widgetHoverState) bool { return h.minimize }},
		{"close", layout.close, func(h widgetHoverState) bool { return h.close }},
		{"connection", layout.connection, func(h widgetHoverState) bool { return h.connection }},
		{"recent", layout.recent, func(h widgetHoverState) bool { return h.recent }},
		{"history", layout.history, func(h widgetHoverState) bool { return h.history }},
		{"settings", layout.settings, func(h widgetHoverState) bool { return h.settings }},
		{"test", layout.test, func(h widgetHoverState) bool { return h.test }},
		{"hide", layout.hide, func(h widgetHoverState) bool { return h.hide }},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			x := tt.rect.Left + (tt.rect.Right-tt.rect.Left)/2
			y := tt.rect.Top + (tt.rect.Bottom-tt.rect.Top)/2
			got := widgetHoverAt(x, y, layout)
			if !tt.hover(got) {
				t.Fatalf("widgetHoverAt(%d,%d) did not mark %s hovered: %+v", x, y, tt.name, got)
			}
			if countWidgetHoverFields(got) != 1 {
				t.Fatalf("widgetHoverAt(%d,%d) marked overlapping hover regions: %+v", x, y, got)
			}
		})
	}
}

func TestWidgetHoverAtOutsideLayoutClearsState(t *testing.T) {
	got := widgetHoverAt(-1, -1, widgetLayoutRects())
	if got.any() {
		t.Fatalf("widgetHoverAt outside layout = %+v, want no hover", got)
	}
}

func countWidgetHoverFields(hover widgetHoverState) int {
	count := 0
	for _, active := range []bool{
		hover.openCode,
		hover.codex,
		hover.antigravity,
		hover.devin,
		hover.minimize,
		hover.close,
		hover.connection,
		hover.recent,
		hover.history,
		hover.settings,
		hover.test,
		hover.hide,
	} {
		if active {
			count++
		}
	}
	return count
}
