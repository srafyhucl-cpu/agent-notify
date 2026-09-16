//go:build windows

package ui

import (
	"syscall"
	"unsafe"
)

// 悬浮窗内嵌 EDIT 控件的读写辅助（设置页与配对码输入共用）。

func getWindowText(hwnd uintptr) string {
	buffer := make([]uint16, 1024)
	pSendMessageW.Call(hwnd, WM_GETTEXT, 1024, uintptr(unsafe.Pointer(&buffer[0])))
	return syscall.UTF16ToString(buffer)
}

func setWindowText(hwnd uintptr, text string) {
	pSendMessageW.Call(hwnd, WM_SETTEXT, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr(text))))
}
