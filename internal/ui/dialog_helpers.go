//go:build windows

package ui

import "unsafe"

func showMessage(hwnd uintptr, text string, flags uintptr) {
	pMessageBoxW.Call(
		hwnd,
		uintptr(unsafe.Pointer(StringToUTF16Ptr(text))),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))),
		flags,
	)
}

func showConfirm(hwnd uintptr, text string) bool {
	answer, _, _ := pMessageBoxW.Call(
		hwnd,
		uintptr(unsafe.Pointer(StringToUTF16Ptr(text))),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))),
		MB_YESNO|MB_ICONQUESTION,
	)
	return answer == IDYES
}
