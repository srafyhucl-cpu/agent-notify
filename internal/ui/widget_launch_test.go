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
		{"negative", "-32000,-32000", 1050, 420},
		{"off screen", "5000,5000", 1050, 420},
		{"invalid", "abc,def", 1050, 420},
		{"empty", "", 1050, 420},
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
