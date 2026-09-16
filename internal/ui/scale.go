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

func unscaleFloat(value int32) int32 {
	scale := float64(uiDPI) / baseDPI
	if scale <= 0 {
		scale = 1
	}
	return int32(math.Round(float64(value) / scale))
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

// initializeWidgetWorkArea returns the work area for the window. New windows
// use the primary work area during creation, then WM_CREATE and WM_DPICHANGED
// re-clamp them against their own monitor.
func widgetWorkArea(hwnd uintptr) RECT {
	if hwnd != 0 && pMonitorFromWindow.Find() == nil && pGetMonitorInfoW.Find() == nil {
		monitor, _, _ := pMonitorFromWindow.Call(hwnd, MONITOR_DEFAULTTONEAREST)
		if monitor != 0 {
			info := MONITORINFO{CbSize: uint32(unsafe.Sizeof(MONITORINFO{}))}
			if result, _, _ := pGetMonitorInfoW.Call(monitor, uintptr(unsafe.Pointer(&info))); result != 0 && validWorkArea(info.RcWork) {
				return info.RcWork
			}
		}
	}

	if pSystemParametersInfoW.Find() == nil {
		var area RECT
		if result, _, _ := pSystemParametersInfoW.Call(SPI_GETWORKAREA, 0, uintptr(unsafe.Pointer(&area)), 0); result != 0 && validWorkArea(area) {
			return area
		}
	}

	width, _, _ := pGetSystemMetrics.Call(0)
	height, _, _ := pGetSystemMetrics.Call(1)
	return RECT{Left: 0, Top: 0, Right: int32(width), Bottom: int32(height)}
}

func validWorkArea(area RECT) bool {
	return area.Right > area.Left && area.Bottom > area.Top
}

func clampWidgetPosition(x, y, winWidth, winHeight int32, area RECT) (int32, int32) {
	if !validWorkArea(area) {
		return x, y
	}
	minX := area.Left + widgetMinimumMargin
	minY := area.Top + widgetMinimumMargin
	maxX := area.Right - winWidth - widgetMinimumMargin
	maxY := area.Bottom - winHeight - widgetMinimumMargin
	if maxX < minX {
		x = area.Left
	} else {
		x = minInt32(maxInt32(x, minX), maxX)
	}
	if maxY < minY {
		y = area.Top
	} else {
		y = minInt32(maxInt32(y, minY), maxY)
	}
	return x, y
}

func minInt32(a, b int32) int32 {
	if a < b {
		return a
	}
	return b
}

func maxInt32(a, b int32) int32 {
	if a > b {
		return a
	}
	return b
}

// resizeForCurrentDPI resizes a window to its logical size at the active DPI
// and re-clamps it into the active monitor work area.
func resizeForCurrentDPI(hwnd uintptr, width, height int32) {
	if hwnd == 0 {
		return
	}
	var rect RECT
	pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
	scaledWidth, scaledHeight := logicalSize(width, height)
	x, y := clampWidgetPosition(rect.Left, rect.Top, scaledWidth, scaledHeight, widgetWorkArea(hwnd))
	pSetWindowPos.Call(
		hwnd,
		0,
		uintptr(x),
		uintptr(y),
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
