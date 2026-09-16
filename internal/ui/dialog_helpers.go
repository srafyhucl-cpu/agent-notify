//go:build windows

package ui

import "unsafe"

func drawCard(hdc uintptr, rect RECT, fillColor, borderColor uintptr) {
	fillRoundRect(hdc, rect, 10, fillColor)
	strokeRoundRect(hdc, rect, 10, fillColor, borderColor, 1)
}

func drawFieldFrame(hdc uintptr, rect RECT) {
	fillRoundRect(hdc, rect, 6, uintptr(RGB(20, 26, 33)))
	strokeRoundRect(hdc, rect, 6, uintptr(RGB(20, 26, 33)), uintptr(RGB(48, 60, 74)), 1)
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
		if message.Message == WM_KEYDOWN && message.WParam == VK_RETURN {
			owner := *dialog
			child, _, _ := pIsChild.Call(owner, message.Hwnd)
			if message.Hwnd == owner || child != 0 {
				pPostMessageW.Call(owner, WM_COMMAND, uintptr(IDOK), 0)
				continue
			}
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&message)))
	}
}
