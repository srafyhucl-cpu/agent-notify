//go:build windows

package ui

import (
	"fmt"
	"runtime"
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
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

	const gmemMoveable = 0x0002
	const cfUnicodeText = 13
	size := len(utf16) * 2
	hGlobal, _, _ := kernel32.NewProc("GlobalAlloc").Call(gmemMoveable, uintptr(size))
	if hGlobal == 0 {
		return
	}
	pointer, _, _ := kernel32.NewProc("GlobalLock").Call(hGlobal)
	if pointer != 0 {
		kernel32.NewProc("RtlMoveMemory").Call(pointer, uintptr(unsafe.Pointer(&utf16[0])), uintptr(size))
		kernel32.NewProc("GlobalUnlock").Call(hGlobal)
		pSetClipboardData.Call(cfUnicodeText, hGlobal)
	}
}

func historyTime(item notify.HistoryItem) string {
	if timestamp := item.LocalTime(); !timestamp.IsZero() {
		return timestamp.Format("01-02 15:04")
	}
	return truncateUI(item.Timestamp, 16)
}

func historyAgent(item notify.HistoryItem) string {
	switch item.Agent {
	case "opencode":
		return "OpenCode"
	case "codex":
		return "Codex"
	case "test":
		return "测试"
	default:
		return "通用"
	}
}

func ShowHistoryDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("AgentNotifyHistoryDialog")
	records, _ := notify.GetHistory(100, "")
	selectedIndex := 0
	var dialog uintptr

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_PAINT:
			var paint PAINTSTRUCT
			hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
			var rect RECT
			pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
			background, _, _ := pCreateSolidBrush.Call(uintptr(RGB(22, 26, 31)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rect)), background)
			pDeleteObject.Call(background)
			pSetBkMode.Call(hdc, TRANSPARENT)

			titleFont := newFont(18, 700)
			baseFont := newFont(13, 400)
			smallFont := newFont(11, 400)
			oldFont, _, _ := pSelectObject.Call(hdc, titleFont)

			pSetTextColor.Call(hdc, uintptr(RGB(242, 245, 247)))
			DrawText(hdc, "推送历史", &RECT{18, 12, 220, 40}, DT_SINGLELINE|DT_VCENTER)
			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
			DrawText(hdc, fmt.Sprintf("最近 %d 条", len(records)), &RECT{470, 16, 582, 38}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)

			listRect := RECT{16, 48, 584, 250}
			fillRoundRect(hdc, listRect, 10, uintptr(RGB(17, 21, 25)))

			if len(records) == 0 {
				pSelectObject.Call(hdc, baseFont)
				pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
				DrawText(hdc, "暂无推送记录", &RECT{32, 120, 568, 150}, DT_CENTER|DT_SINGLELINE|DT_VCENTER)
			}

			pSelectObject.Call(hdc, baseFont)
			maxVisible := 7
			for i := 0; i < len(records) && i < maxVisible; i++ {
				item := records[i]
				row := RECT{22, 54 + int32(i*27), 578, 81 + int32(i*27)}
				if i == selectedIndex {
					fillRoundRect(hdc, row, 6, uintptr(RGB(34, 48, 58)))
				}
				pSetTextColor.Call(hdc, uintptr(RGB(226, 231, 234)))
				DrawText(hdc, truncateUI(item.Title, 28), &RECT{32, row.Top, 270, row.Bottom}, DT_SINGLELINE|DT_VCENTER)

				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
				DrawText(hdc, historyTime(item), &RECT{282, row.Top, 372, row.Bottom}, DT_SINGLELINE|DT_VCENTER)
				DrawText(hdc, historyAgent(item), &RECT{384, row.Top, 452, row.Bottom}, DT_SINGLELINE|DT_VCENTER)

				statusColor := uintptr(RGB(139, 148, 158))
				if item.Status == notify.StatusSuccess {
					statusColor = uintptr(RGB(54, 190, 144))
				} else if item.Status == notify.StatusFailed || item.Status == notify.StatusNotLoggedIn {
					statusColor = uintptr(RGB(226, 104, 104))
				}
				pSetTextColor.Call(hdc, statusColor)
				DrawText(hdc, item.Status, &RECT{462, row.Top, 568, row.Bottom}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)
				pSelectObject.Call(hdc, baseFont)
			}

			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
			DrawText(hdc, "摘要详情", &RECT{18, 260, 120, 282}, DT_SINGLELINE|DT_VCENTER)

			detailRect := RECT{16, 284, 584, 374}
			fillRoundRect(hdc, detailRect, 10, uintptr(RGB(17, 21, 25)))
			detail := "选择一条记录查看详情"
			if selectedIndex >= 0 && selectedIndex < len(records) {
				item := records[selectedIndex]
				detail = fmt.Sprintf("%s · %s · %s\n%s", historyTime(item), historyAgent(item), item.Status, item.Summary)
				if item.Error != "" {
					detail += "\n" + item.Error
				}
			}
			pSetTextColor.Call(hdc, uintptr(RGB(214, 220, 224)))
			DrawText(hdc, detail, &RECT{28, 294, 572, 364}, DT_WORDBREAK|DT_NOPREFIX)

			drawButton := func(rect RECT, text string, foreground uint32) {
				fillRoundRect(hdc, rect, 8, uintptr(RGB(31, 37, 43)))
				pSetTextColor.Call(hdc, uintptr(foreground))
				DrawText(hdc, text, &rect, DT_CENTER|DT_VCENTER|DT_SINGLELINE)
			}
			drawButton(RECT{16, 390, 116, 424}, "复制摘要", RGB(224, 229, 232))
			drawButton(RECT{126, 390, 226, 424}, "清空日志", RGB(226, 104, 104))
			drawButton(RECT{484, 390, 584, 424}, "关闭", RGB(54, 190, 144))

			pSelectObject.Call(hdc, oldFont)
			pDeleteObject.Call(titleFont)
			pDeleteObject.Call(baseFont)
			pDeleteObject.Call(smallFont)
			pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)
			if x >= 16 && x <= 584 && y >= 48 && y < 250 {
				index := int((y - 54) / 27)
				if index >= 0 && index < len(records) {
					selectedIndex = index
					pInvalidateRect.Call(hwnd, 0, 1)
				}
			}
			if x >= 16 && x <= 116 && y >= 390 && y <= 424 && selectedIndex >= 0 && selectedIndex < len(records) {
				item := records[selectedIndex]
				SetClipboardText(fmt.Sprintf("%s\n%s · %s\n\n%s", item.Title, historyTime(item), historyAgent(item), item.Summary))
				pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("已复制到剪贴板。"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
			}
			if x >= 126 && x <= 226 && y >= 390 && y <= 424 {
				answer, _, _ := pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("确定清空全部推送历史吗？"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_YESNO|MB_ICONQUESTION)
				if answer == IDYES {
					_ = notify.ClearHistory("")
					records, _ = notify.GetHistory(100, "")
					selectedIndex = 0
					pInvalidateRect.Call(hwnd, 0, 1)
				}
			}
			if x >= 484 && x <= 584 && y >= 390 && y <= 424 {
				pDestroyWindow.Call(hwnd)
			}
			return 0

		case WM_CLOSE:
			pDestroyWindow.Call(hwnd)
			return 0

		case WM_DESTROY:
			dialog = 0
			return 0
		}
		result, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
		return result
	})

	var windowClass WNDCLASSEXW
	windowClass.CbSize = uint32(unsafe.Sizeof(windowClass))
	windowClass.LpfnWndProc = wndProc
	windowClass.HInstance = hInstance
	windowClass.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
	windowClass.LpszClassName = className
	pRegisterClassExW.Call(uintptr(unsafe.Pointer(&windowClass)))

	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	width := int32(600)
	height := int32(460)
	x := (int32(screenWidth) - width) / 2
	y := (int32(screenHeight) - height) / 2
	dialog, _, _ = pCreateWindowExW.Call(
		WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify 推送历史"))),
		WS_POPUP|WS_SYSMENU|WS_VISIBLE,
		uintptr(x), uintptr(y), uintptr(width), uintptr(height),
		parentHwnd, 0, hInstance, 0,
	)
	cornerPreference := uint32(2)
	pDwmSetWindowAttribute.Call(dialog, 33, uintptr(unsafe.Pointer(&cornerPreference)), 4)
	darkMode := uint32(1)
	pDwmSetWindowAttribute.Call(dialog, 20, uintptr(unsafe.Pointer(&darkMode)), 4)
	pShowWindow.Call(dialog, SW_SHOW)
	pUpdateWindow.Call(dialog)

	var message MSG
	for dialog != 0 {
		result, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if result == 0 || int32(result) == -1 {
			break
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&message)))
	}
}
