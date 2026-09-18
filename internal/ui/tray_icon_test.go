//go:build windows

package ui

import "testing"

// 托盘三态各自持有一个独立图标句柄：创建失败返回 0，三态之间不能复用同一个句柄。
func TestCreateStatusIconReturnsDistinctHandles(t *testing.T) {
	colors := []uint32{statusColorReady, statusColorWarning, statusColorStopped}
	seen := make(map[uintptr]uint32, len(colors))
	for _, color := range colors {
		icon := CreateStatusIcon(color)
		if icon == 0 {
			t.Fatalf("CreateStatusIcon(%#06x) returned 0", color)
		}
		if previous, ok := seen[icon]; ok {
			t.Fatalf("CreateStatusIcon(%#06x) reused handle %#x from %#06x", color, icon, previous)
		}
		seen[icon] = color
		pDestroyIcon.Call(icon)
	}
}
