//go:build windows

package ui

import (
	"math"
	"unsafe"
)

const (
	baseDPI      = 96.0
	defaultDPI   = uint32(96)
	widgetDPImin = uint32(72)
	widgetDPImax = uint32(384)
)

var uiDPI = defaultDPI

func normalizeDPI(dpi uint32) uint32 {
	if dpi < widgetDPImin {
		return widgetDPImin
	}
	if dpi > widgetDPImax {
		return widgetDPImax
	}
	return dpi
}

func setUIDPI(dpi uint32) {
	uiDPI = normalizeDPI(dpi)
}

func systemDPI() uint32 {
	if pGetDpiForSystem.Find() == nil {
		if dpi, _, _ := pGetDpiForSystem.Call(); dpi != 0 {
			return normalizeDPI(uint32(dpi))
		}
	}
	hdc, _, _ := pGetDC.Call(0)
	if hdc == 0 {
		return defaultDPI
	}
	defer pReleaseDC.Call(0, hdc)
	const logPixelsX = 88
	dpi, _, _ := pGetDeviceCaps.Call(hdc, logPixelsX)
	if dpi == 0 {
		return defaultDPI
	}
	return normalizeDPI(uint32(dpi))
}

func windowDPI(hwnd uintptr) uint32 {
	if hwnd != 0 && pGetDpiForWindow.Find() == nil {
		if dpi, _, _ := pGetDpiForWindow.Call(hwnd); dpi != 0 {
			return normalizeDPI(uint32(dpi))
		}
	}
	return systemDPI()
}

func scaleFloat(value int32) int32 {
	scaled := math.Round(float64(value) * float64(uiDPI) / baseDPI)
	if scaled > math.MaxInt32 {
		return math.MaxInt32
	}
	if scaled < math.MinInt32 {
		return math.MinInt32
	}
	return int32(scaled)
}

func scaleRect(rect RECT) RECT {
	return RECT{
		Left:   scaleFloat(rect.Left),
		Top:    scaleFloat(rect.Top),
		Right:  scaleFloat(rect.Right),
		Bottom: scaleFloat(rect.Bottom),
	}
}

func unscalePoint(x, y int32) (int32, int32) {
	scale := float64(uiDPI) / baseDPI
	if scale <= 0 {
		scale = 1
	}
	return int32(math.Round(float64(x) / scale)), int32(math.Round(float64(y) / scale))
}

func logicalSize(width, height int32) (int32, int32) {
	return scaleFloat(width), scaleFloat(height)
}

// resizeForCurrentDPI resizes a window to its logical size at the active DPI,
// keeping the current top-left corner so per-monitor moves stay predictable.
func resizeForCurrentDPI(hwnd uintptr, width, height int32) {
	if hwnd == 0 {
		return
	}
	var rect RECT
	pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
	scaledWidth, scaledHeight := logicalSize(width, height)
	pSetWindowPos.Call(
		hwnd,
		0,
		uintptr(rect.Left),
		uintptr(rect.Top),
		uintptr(scaledWidth),
		uintptr(scaledHeight),
		SWP_NOZORDER|SWP_NOACTIVATE,
	)
}

func fillRectLogical(hdc uintptr, rect RECT, color uintptr) {
	scaled := scaleRect(rect)
	brush, _, _ := pCreateSolidBrush.Call(uintptr(color))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&scaled)), brush)
	pDeleteObject.Call(brush)
}

func strokeRoundRect(hdc uintptr, rect RECT, radius int32, fillColor, lineColor uintptr, thickness int32) {
	scaled := scaleRect(rect)
	scaledRadius := scaleFloat(radius)
	scaledThickness := int32(1)
	if thickness > 1 {
		scaledThickness = scaleFloat(thickness)
	}
	brush, _, _ := pCreateSolidBrush.Call(fillColor)
	pen, _, _ := pCreatePen.Call(0, uintptr(scaledThickness), lineColor)
	oldBrush, _, _ := pSelectObject.Call(hdc, brush)
	oldPen, _, _ := pSelectObject.Call(hdc, pen)
	pRoundRect.Call(hdc, uintptr(scaled.Left), uintptr(scaled.Top), uintptr(scaled.Right), uintptr(scaled.Bottom), uintptr(scaledRadius), uintptr(scaledRadius))
	pSelectObject.Call(hdc, oldBrush)
	pSelectObject.Call(hdc, oldPen)
	pDeleteObject.Call(brush)
	pDeleteObject.Call(pen)
}

func drawEllipseLogical(hdc uintptr, left, top, right, bottom int32, fillColor, lineColor uintptr) {
	scaled := scaleRect(RECT{Left: left, Top: top, Right: right, Bottom: bottom})
	brush, _, _ := pCreateSolidBrush.Call(fillColor)
	pen, _, _ := pCreatePen.Call(0, 1, lineColor)
	oldBrush, _, _ := pSelectObject.Call(hdc, brush)
	oldPen, _, _ := pSelectObject.Call(hdc, pen)
	pEllipse.Call(hdc, uintptr(scaled.Left), uintptr(scaled.Top), uintptr(scaled.Right), uintptr(scaled.Bottom))
	pSelectObject.Call(hdc, oldBrush)
	pSelectObject.Call(hdc, oldPen)
	pDeleteObject.Call(brush)
	pDeleteObject.Call(pen)
}

func paintDoubleBuffered(hwnd uintptr, draw func(hdc uintptr, width, height int32)) {
	var paint PAINTSTRUCT
	hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
	var rect RECT
	pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
	width := rect.Right - rect.Left
	height := rect.Bottom - rect.Top

	hdcMem, _, _ := pCreateCompatibleDC.Call(hdc)
	hBitmap, _, _ := pCreateCompatibleBitmap.Call(hdc, uintptr(width), uintptr(height))
	if hdcMem == 0 || hBitmap == 0 {
		if hBitmap != 0 {
			pDeleteObject.Call(hBitmap)
		}
		if hdcMem != 0 {
			pDeleteDC.Call(hdcMem)
		}
		draw(hdc, width, height)
		pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
		return
	}
	oldBitmap, _, _ := pSelectObject.Call(hdcMem, hBitmap)
	draw(hdcMem, width, height)
	pBitBlt.Call(hdc, 0, 0, uintptr(width), uintptr(height), hdcMem, 0, 0, SRCCOPY)
	pSelectObject.Call(hdcMem, oldBitmap)
	pDeleteObject.Call(hBitmap)
	pDeleteDC.Call(hdcMem)
	pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
}
