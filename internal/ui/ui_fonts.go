//go:build windows

package ui

import (
	"syscall"
	"unsafe"
)

// 界面字号按 96 DPI 的逻辑像素定义，绘制前由 newFont/newIconFont 统一缩放。
// 改这里就能同时放大悬浮窗、设置、历史和登录窗口，避免各窗口字号走散。
const (
	fontTitleSize  = int32(22)
	fontBaseSize   = int32(15)
	fontStrongSize = int32(15)
	fontSmallSize  = int32(13)
	fontIconSize   = int32(17)
)

func newTitleFont() uintptr  { return newFont(fontTitleSize, 700) }
func newBaseFont() uintptr   { return newFont(fontBaseSize, 400) }
func newStrongFont() uintptr { return newFont(fontStrongSize, 700) }
func newSmallFont() uintptr  { return newFont(fontSmallSize, 400) }
func newUIIconFont() uintptr { return newIconFont(fontIconSize) }

// measureTextWidth 用与窗口绘制相同的字体测量文本像素宽度，供布局测试断言不溢出。
func measureTextWidth(font uintptr, text string) int32 {
	if font == 0 {
		return 0
	}
	hdc, _, _ := pGetDC.Call(0)
	if hdc == 0 {
		return 0
	}
	defer pReleaseDC.Call(0, hdc)
	oldFont, _, _ := pSelectObject.Call(hdc, font)
	defer pSelectObject.Call(hdc, oldFont)

	wide, err := syscall.UTF16FromString(text)
	if err != nil || len(wide) <= 1 {
		return 0
	}
	var size SIZE
	if ok, _, _ := pGetTextExtentPoint32W.Call(
		hdc,
		uintptr(unsafe.Pointer(&wide[0])),
		uintptr(len(wide)-1),
		uintptr(unsafe.Pointer(&size)),
	); ok == 0 {
		return 0
	}
	return size.CX
}
