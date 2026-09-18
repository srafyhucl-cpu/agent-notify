//go:build windows

package ui

import (
	"bytes"
	_ "embed"
	"image"
	"image/draw"
	"image/png"
	"unsafe"
)

// 托盘图标直接复用应用图标的设计稿，只把底板换成三态状态色，
// 由 tools/build-icon.py 生成；这里只负责把位图包成 HICON。
//
//go:embed assets/tray_ready.png
var trayReadyPNG []byte

//go:embed assets/tray_warning.png
var trayWarningPNG []byte

//go:embed assets/tray_stopped.png
var trayStoppedPNG []byte

// CreateStatusIcon 按状态色返回托盘图标；颜色没有对应位图时返回 0。
// 返回的 HICON 由调用方用 DestroyIcon 释放。
func CreateStatusIcon(color uint32) uintptr {
	for _, entry := range []struct {
		color uint32
		data  []byte
	}{
		{statusColorReady, trayReadyPNG},
		{statusColorWarning, trayWarningPNG},
		{statusColorStopped, trayStoppedPNG},
	} {
		if entry.color == color {
			return iconFromPNG(entry.data)
		}
	}
	return 0
}

// iconFromPNG 把一张 32bpp PNG 包成带 alpha 的 HICON。
func iconFromPNG(data []byte) uintptr {
	decoded, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		return 0
	}
	bounds := decoded.Bounds()
	width, height := bounds.Dx(), bounds.Dy()
	straight := image.NewNRGBA(image.Rect(0, 0, width, height))
	draw.Draw(straight, straight.Bounds(), decoded, bounds.Min, draw.Src)

	hdcScreen, _, _ := pGetDC.Call(0)
	defer pReleaseDC.Call(0, hdcScreen)

	bmi := bitmapInfo{
		Header: bitmapInfoHeader{
			Size:      uint32(unsafe.Sizeof(bitmapInfoHeader{})),
			Width:     int32(width),
			Height:    int32(-height), // 负高度表示自上而下，与 PNG 行序一致
			Planes:    1,
			BitCount:  32,
			SizeImage: uint32(width * height * 4),
		},
	}
	var bits unsafe.Pointer
	hBitmapColor, _, _ := pCreateDIBSection.Call(
		hdcScreen, uintptr(unsafe.Pointer(&bmi)), dibRGBColors,
		uintptr(unsafe.Pointer(&bits)), 0, 0,
	)
	if hBitmapColor == 0 || bits == nil {
		return 0
	}
	pixels := unsafe.Slice((*byte)(bits), width*height*4)
	for i := 0; i < width*height; i++ {
		pixels[i*4] = straight.Pix[i*4+2]   // DIB 是 BGRA
		pixels[i*4+1] = straight.Pix[i*4+1] // DIB 是 BGRA
		pixels[i*4+2] = straight.Pix[i*4]
		pixels[i*4+3] = straight.Pix[i*4+3]
	}

	hBitmapMask := opaqueIconMask(hdcScreen, width, height)
	if hBitmapMask == 0 {
		pDeleteObject.Call(hBitmapColor)
		return 0
	}

	iconInfo := ICONINFO{FIcon: 1, HbmMask: hBitmapMask, HbmColor: hBitmapColor}
	icon, _, _ := pCreateIconIndirect.Call(uintptr(unsafe.Pointer(&iconInfo)))
	pDeleteObject.Call(hBitmapMask)
	pDeleteObject.Call(hBitmapColor)
	return icon
}

// opaqueIconMask 生成全黑的 1bpp 遮罩：实际透明度由彩色位图的 alpha 通道决定，
// 遮罩只需保证不额外挖空。
func opaqueIconMask(hdcScreen uintptr, width, height int) uintptr {
	hBitmap, _, _ := pCreateBitmap.Call(uintptr(width), uintptr(height), 1, 1, 0)
	if hBitmap == 0 {
		return 0
	}
	hdc, _, _ := pCreateCompatibleDC.Call(hdcScreen)
	if hdc == 0 {
		pDeleteObject.Call(hBitmap)
		return 0
	}
	hOld, _, _ := pSelectObject.Call(hdc, hBitmap)
	blackBrush, _, _ := pCreateSolidBrush.Call(0)
	rect := RECT{0, 0, int32(width), int32(height)}
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rect)), blackBrush)
	pDeleteObject.Call(blackBrush)
	pSelectObject.Call(hdc, hOld)
	pDeleteDC.Call(hdc)
	return hBitmap
}
