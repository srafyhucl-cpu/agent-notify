//go:build windows

package ui

import (
	"fmt"
	"runtime"
	"syscall"
	"unsafe"

	"linkweixin/internal/notify"
)

// SetClipboardText copies text to the Windows system clipboard.
func SetClipboardText(text string) {
	utf16, err := syscall.UTF16FromString(text)
	if err != nil || len(utf16) == 0 {
		return
	}

	pOpenClipboard.Call(0)
	defer pCloseClipboard.Call()
	pEmptyClipboard.Call()

	const GMEM_MOVEABLE = 0x0002
	const CF_UNICODETEXT = 13
	size := len(utf16) * 2

	hGlobal, _, _ := kernel32.NewProc("GlobalAlloc").Call(GMEM_MOVEABLE, uintptr(size))
	if hGlobal == 0 {
		return
	}

	ptr, _, _ := kernel32.NewProc("GlobalLock").Call(hGlobal)
	if ptr != 0 {
		src := unsafe.Pointer(&utf16[0])
		kernel32.NewProc("RtlMoveMemory").Call(ptr, uintptr(src), uintptr(size))
		kernel32.NewProc("GlobalUnlock").Call(hGlobal)
		pSetClipboardData.Call(CF_UNICODETEXT, hGlobal)
	}
}

// ShowHistoryDialog displays the push history modal window.
func ShowHistoryDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("LinkWeixinHistoryDialog")

	records, _ := notify.GetHistory(100, "")
	selectedIndex := 0

	var dlgHwnd uintptr

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_PAINT:
			var ps PAINTSTRUCT
			hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps)))

			var rc RECT
			pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rc)))

			hbrBg, _, _ := pCreateSolidBrush.Call(uintptr(RGB(24, 24, 27)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rc)), hbrBg)
			pDeleteObject.Call(hbrBg)

			pSetBkMode.Call(hdc, TRANSPARENT)

			// Header title
			hFontBold, _, _ := pCreateFontW.Call(18, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
			hOldFont, _, _ := pSelectObject.Call(hdc, hFontBold)
			pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))
			rcHead := RECT{16, 12, 400, 36}
			DrawText(hdc, "最近推送记录（点击行查看详情）：", &rcHead, DT_SINGLELINE|DT_VCENTER)

			// List area background
			rcList := RECT{16, 46, 528, 240}
			hbrCard, _, _ := pCreateSolidBrush.Call(uintptr(RGB(39, 39, 44)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcList)), hbrCard)
			pDeleteObject.Call(hbrCard)

			// Items in list
			hFontBase, _, _ := pCreateFontW.Call(15, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
			pSelectObject.Call(hdc, hFontBase)

			maxVisible := 7
			yPos := int32(50)
			for i := 0; i < len(records) && i < maxVisible; i++ {
				r := records[i]
				rcItem := RECT{20, yPos, 524, yPos + 26}
				if i == selectedIndex {
					hbrSel, _, _ := pCreateSolidBrush.Call(uintptr(RGB(40, 80, 140)))
					pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcItem)), hbrSel)
					pDeleteObject.Call(hbrSel)
				}

				pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))
				lineStr := fmt.Sprintf("%-19s | %-16s | %s [%s]", r.Time, r.Title, r.Channels, r.Status)
				DrawText(hdc, lineStr, &rcItem, DT_SINGLELINE|DT_VCENTER)
				yPos += 26
			}

			if len(records) == 0 {
				rcEmpty := RECT{20, 120, 524, 150}
				pSetTextColor.Call(hdc, uintptr(RGB(161, 161, 170)))
				DrawText(hdc, "暂无推送历史记录", &rcEmpty, DT_CENTER|DT_VCENTER|DT_SINGLELINE)
			}

			// Detail title
			rcDetailTitle := RECT{16, 250, 400, 270}
			pSetTextColor.Call(hdc, uintptr(RGB(161, 161, 170)))
			DrawText(hdc, "摘要详情：", &rcDetailTitle, DT_SINGLELINE|DT_VCENTER)

			// Detail text box area
			rcDetailBox := RECT{16, 274, 528, 370}
			hbrDetail, _, _ := pCreateSolidBrush.Call(uintptr(RGB(30, 30, 34)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcDetailBox)), hbrDetail)
			pDeleteObject.Call(hbrDetail)

			detailText := "暂无详情"
			if selectedIndex >= 0 && selectedIndex < len(records) {
				item := records[selectedIndex]
				detailText = fmt.Sprintf("【%s】\n时间：%s (%s - %s)\n%s", item.Title, item.Time, item.Channels, item.Status, item.Summary)
			}
			rcDetailPad := RECT{24, 280, 520, 364}
			pSetTextColor.Call(hdc, uintptr(RGB(220, 220, 225)))
			DrawText(hdc, detailText, &rcDetailPad, DT_WORDBREAK)

			// Buttons at bottom
			// Copy button (16, 385, 116, 415)
			rcBtnCopy := RECT{16, 385, 116, 417}
			hbrBtn, _, _ := pCreateSolidBrush.Call(uintptr(RGB(39, 39, 44)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcBtnCopy)), hbrBtn)
			pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))
			DrawText(hdc, "复制摘要", &rcBtnCopy, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

			// Clear button (126, 385, 226, 417)
			rcBtnClear := RECT{126, 385, 226, 417}
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcBtnClear)), hbrBtn)
			pSetTextColor.Call(hdc, uintptr(RGB(220, 100, 100)))
			DrawText(hdc, "清空日志", &rcBtnClear, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

			// Close button (428, 385, 528, 417)
			rcBtnClose := RECT{428, 385, 528, 417}
			hbrGreen, _, _ := pCreateSolidBrush.Call(uintptr(RGB(16, 185, 129)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcBtnClose)), hbrGreen)
			pDeleteObject.Call(hbrGreen)
			pSetTextColor.Call(hdc, uintptr(RGB(255, 255, 255)))
			DrawText(hdc, "关闭", &rcBtnClose, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

			pDeleteObject.Call(hbrBtn)
			pSelectObject.Call(hdc, hOldFont)
			pDeleteObject.Call(hFontBold)
			pDeleteObject.Call(hFontBase)

			pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps)))
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)

			// Check list item clicks
			if x >= 16 && x <= 528 && y >= 46 && y < 240 {
				idx := int((y - 46) / 26)
				if idx >= 0 && idx < len(records) {
					selectedIndex = idx
					pInvalidateRect.Call(hwnd, 0, 1)
				}
			}

			// Copy button
			if x >= 16 && x <= 116 && y >= 385 && y <= 417 {
				if selectedIndex >= 0 && selectedIndex < len(records) {
					item := records[selectedIndex]
					t := fmt.Sprintf("【%s】\n时间：%s (%s - %s)\n%s", item.Title, item.Time, item.Channels, item.Status, item.Summary)
					SetClipboardText(t)
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("已复制到剪贴板。"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin"))), MB_OK|MB_ICONINFO)
				}
			}

			// Clear button
			if x >= 126 && x <= 226 && y >= 385 && y <= 417 {
				res, _, _ := pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("确定清空所有推送历史日志吗？"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin"))), MB_YESNO|MB_ICONQUESTION)
				if res == IDYES {
					_ = notify.ClearHistory("")
					records, _ = notify.GetHistory(100, "")
					selectedIndex = 0
					pInvalidateRect.Call(hwnd, 0, 1)
				}
			}

			// Close button
			if x >= 428 && x <= 528 && y >= 385 && y <= 417 {
				pDestroyWindow.Call(hwnd)
			}
			return 0

		case WM_CLOSE:
			pDestroyWindow.Call(hwnd)
			return 0

		case WM_DESTROY:
			dlgHwnd = 0
			return 0
		}
		ret, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
		return ret
	})

	var wc WNDCLASSEXW
	wc.CbSize = uint32(unsafe.Sizeof(wc))
	wc.LpfnWndProc = wndProc
	wc.HInstance = hInstance
	wc.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
	wc.LpszClassName = className
	pRegisterClassExW.Call(uintptr(unsafe.Pointer(&wc)))

	// Window centered
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	dlgWidth := int32(560)
	dlgHeight := int32(460)
	dlgX := (int32(screenWidth) - dlgWidth) / 2
	dlgY := (int32(screenHeight) - dlgHeight) / 2

	dlgHwnd, _, _ = pCreateWindowExW.Call(
		WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin - 推送历史"))),
		WS_POPUP|WS_SYSMENU|WS_VISIBLE,
		uintptr(dlgX), uintptr(dlgY), uintptr(dlgWidth), uintptr(dlgHeight),
		parentHwnd, 0, hInstance, 0,
	)

	// Round corners & dark mode
	cornerPref := uint32(2)
	pDwmSetWindowAttribute.Call(dlgHwnd, 33, uintptr(unsafe.Pointer(&cornerPref)), 4)
	darkMode := uint32(1)
	pDwmSetWindowAttribute.Call(dlgHwnd, 20, uintptr(unsafe.Pointer(&darkMode)), 4)

	pShowWindow.Call(dlgHwnd, SW_SHOW)
	pUpdateWindow.Call(dlgHwnd)

	// Modal message loop
	var msg MSG
	for dlgHwnd != 0 {
		ret, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0)
		if ret == 0 || int32(ret) == -1 {
			break
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&msg)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&msg)))
	}
}
