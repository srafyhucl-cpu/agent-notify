//go:build windows

package ui

import "unsafe"

func drawCard(hdc uintptr, rect RECT, fillColor, borderColor uintptr) {
	fillRoundRect(hdc, rect, 8, fillColor)
	strokeRoundRect(hdc, rect, 8, fillColor, borderColor, 1)
}

func drawFieldFrame(hdc uintptr, rect RECT) {
	fillRoundRect(hdc, rect, 6, uintptr(RGB(16, 20, 24)))
	strokeRoundRect(hdc, rect, 6, uintptr(RGB(16, 20, 24)), uintptr(RGB(48, 58, 68)), 1)
}

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

func setModalParent(parentHwnd uintptr, enabled bool) {
	if parentHwnd == 0 {
		return
	}
	flag := uintptr(1)
	if !enabled {
		flag = 0
	}
	pEnableWindow.Call(parentHwnd, flag)
	if enabled {
		pSetForegroundWindow.Call(parentHwnd)
	}
}

// runDialogLoop pumps messages until the dialog window is destroyed.
func runDialogLoop(dialog *uintptr) {
	var message MSG
	for *dialog != 0 {
		result, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if result == 0 || int32(result) == -1 {
			return
		}
		if message.Message == WM_KEYDOWN && message.WParam == VK_ESCAPE {
			owner := *dialog
			child, _, _ := pIsChild.Call(owner, message.Hwnd)
			if message.Hwnd == owner || child != 0 {
				pDestroyWindow.Call(owner)
				continue
			}
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&message)))
	}
}
