//go:build windows

package ui

import (
	"fmt"
	"runtime"
	"strings"
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"

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
	if item.Agent == "test" {
		return "测试"
	}
	if descriptor, ok := agentmeta.Lookup(item.Agent); ok {
		return descriptor.DisplayName
	}
	if strings.TrimSpace(item.Agent) == "" {
		return "通用"
	}
	return item.Agent
}

const (
	historyWidth     = int32(620)
	historyHeight    = int32(480)
	historyMaxRows   = 9
	historyRowTop    = int32(56)
	historyRowHeight = int32(28)
)

type historyLayout struct {
	close  RECT
	list   RECT
	detail RECT
	copy   RECT
	clear  RECT
	done   RECT
	rows   []RECT
}

func historyLayoutRects() historyLayout {
	rows := make([]RECT, 0, historyMaxRows)
	for i := 0; i < historyMaxRows; i++ {
		top := historyRowTop + int32(i)*historyRowHeight
		rows = append(rows, RECT{22, top, 598, top + historyRowHeight - 2})
	}
	return historyLayout{
		close:  RECT{582, 8, 612, 38},
		list:   RECT{16, 48, 604, 308},
		detail: RECT{16, 322, 604, 420},
		copy:   RECT{16, 432, 128, 466},
		clear:  RECT{136, 432, 248, 466},
		done:   RECT{484, 432, 604, 466},
		rows:   rows,
	}
}

func historyStatusColor(status string) uint32 {
	switch status {
	case notify.StatusSuccess:
		return RGB(55, 190, 147)
	case notify.StatusFailed, notify.StatusNotLoggedIn:
		return RGB(224, 104, 104)
	default:
		return RGB(139, 148, 158)
	}
}

func ShowHistoryDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	parentDPI := windowDPI(parentHwnd)
	setUIDPI(parentDPI)
	defer setUIDPI(parentDPI)

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("AgentNotifyHistoryDialog")
	records, _ := notify.GetHistory(100, "")
	selectedIndex := 0
	scrollOffset := 0
	var dialog uintptr
	var tracking bool
	var hoverCopy, hoverClear, hoverClose bool
	hoverRow := -1
	layout := historyLayoutRects()

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_ERASEBKGND:
			return 1

		case WM_PAINT:
			paintDoubleBuffered(hwnd, func(hdc uintptr, width, height int32) {
				fillRectLogical(hdc, RECT{0, 0, historyWidth, historyHeight}, uintptr(RGB(15, 19, 23)))
				pSetBkMode.Call(hdc, TRANSPARENT)

				titleFont := newTitleFont()
				baseFont := newBaseFont()
				strongFont := newStrongFont()
				smallFont := newSmallFont()
				iconFont := newUIIconFont()
				oldFont, _, _ := pSelectObject.Call(hdc, titleFont)
				defer func() {
					pSelectObject.Call(hdc, oldFont)
					pDeleteObject.Call(titleFont)
					pDeleteObject.Call(baseFont)
					pDeleteObject.Call(strongFont)
					pDeleteObject.Call(smallFont)
					pDeleteObject.Call(iconFont)
				}()

				pSetTextColor.Call(hdc, uintptr(RGB(242, 246, 247)))
				DrawText(hdc, "推送历史", &RECT{20, 12, 240, 42}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				visible := len(records) - scrollOffset
				if visible > historyMaxRows {
					visible = historyMaxRows
				}
				if visible < 0 {
					visible = 0
				}
				rangeText := "暂无记录"
				if len(records) > 0 {
					rangeText = fmt.Sprintf("共 %d 条", len(records))
					if visible > 0 {
						rangeText = fmt.Sprintf("第 %d-%d 条 / 共 %d 条", scrollOffset+1, scrollOffset+visible, len(records))
					}
				}
				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(133, 145, 156)))
				DrawText(hdc, rangeText, &RECT{340, 16, 568, 38}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				drawWindowButton(hdc, layout.close, "\uE8BB", hoverClose, true, iconFont)

				drawCard(hdc, layout.list, uintptr(RGB(20, 25, 30)), uintptr(RGB(41, 50, 59)))
				if len(records) == 0 {
					pSelectObject.Call(hdc, baseFont)
					pSetTextColor.Call(hdc, uintptr(RGB(133, 145, 156)))
					DrawText(hdc, "暂无推送记录", &RECT{32, 150, 588, 190}, DT_CENTER|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				}

				for i := 0; i < historyMaxRows; i++ {
					index := scrollOffset + i
					if index >= len(records) {
						break
					}
					item := records[index]
					row := layout.rows[i]
					switch {
					case index == selectedIndex:
						fillRoundRect(hdc, row, 6, uintptr(RGB(31, 52, 60)))
					case hoverRow == index:
						fillRoundRect(hdc, row, 6, uintptr(RGB(28, 35, 42)))
					}
					pSelectObject.Call(hdc, baseFont)
					pSetTextColor.Call(hdc, uintptr(RGB(228, 233, 236)))
					DrawText(hdc, truncateUI(item.Title, 30), &RECT{row.Left + 10, row.Top, row.Left + 290, row.Bottom}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)

					pSelectObject.Call(hdc, smallFont)
					pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
					DrawText(hdc, historyTime(item), &RECT{row.Left + 300, row.Top, row.Left + 400, row.Bottom}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
					DrawText(hdc, historyAgent(item), &RECT{row.Left + 410, row.Top, row.Left + 500, row.Bottom}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
					pSetTextColor.Call(hdc, uintptr(historyStatusColor(item.Status)))
					DrawText(hdc, item.Status, &RECT{row.Left + 510, row.Top, row.Right - 10, row.Bottom}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				}

				drawCard(hdc, layout.detail, uintptr(RGB(22, 28, 34)), uintptr(RGB(41, 50, 59)))
				detailMeta := "选择一条记录查看摘要"
				detailBody := "推送摘要与错误信息会显示在这里。"
				detailColor := uintptr(RGB(133, 145, 156))
				if selectedIndex >= 0 && selectedIndex < len(records) {
					item := records[selectedIndex]
					detailMeta = fmt.Sprintf("%s · %s · %s", historyTime(item), historyAgent(item), item.Status)
					detailColor = uintptr(historyStatusColor(item.Status))
					detailBody = strings.TrimSpace(item.Summary)
					if detailBody == "" {
						detailBody = "本条记录没有摘要。"
					}
					if item.Error != "" {
						detailBody += "\n错误：" + item.Error
					}
				}
				pSelectObject.Call(hdc, strongFont)
				pSetTextColor.Call(hdc, detailColor)
				DrawText(hdc, detailMeta, &RECT{30, 332, 590, 354}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)
				pSelectObject.Call(hdc, baseFont)
				pSetTextColor.Call(hdc, uintptr(RGB(214, 220, 224)))
				DrawText(hdc, detailBody, &RECT{30, 356, 590, 412}, DT_WORDBREAK|DT_END_ELLIPSIS|DT_NOPREFIX)

				drawIconTextButton(hdc, layout.copy, "\uE8C8", "复制摘要", hoverCopy, false, false, baseFont, iconFont)
				drawIconTextButton(hdc, layout.clear, "\uE74D", "清空日志", hoverClear, false, true, baseFont, iconFont)
				drawIconTextButton(hdc, layout.done, "\uE8BB", "关闭", hoverClose, false, false, baseFont, iconFont)
			})
			return 0

		case WM_MOUSEMOVE:
			if !tracking {
				var track TRACKMOUSEEVENT
				track.CbSize = uint32(unsafe.Sizeof(track))
				track.DwFlags = 0x00000002
				track.HWndTrack = hwnd
				pTrackMouseEvent.Call(uintptr(unsafe.Pointer(&track)))
				tracking = true
			}
			x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
			previousHoverRow := hoverRow
			previousCopy, previousClear, previousClose := hoverCopy, hoverClear, hoverClose
			hoverRow = historyRowAt(layout, x, y, scrollOffset, len(records))
			hoverCopy = pointInRect(x, y, layout.copy)
			hoverClear = pointInRect(x, y, layout.clear)
			hoverClose = pointInRect(x, y, layout.close) || pointInRect(x, y, layout.done)
			if hoverRow != previousHoverRow || hoverCopy != previousCopy || hoverClear != previousClear || hoverClose != previousClose {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if hoverRow >= 0 || hoverCopy || hoverClear || hoverClose {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WM_MOUSELEAVE:
			tracking = false
			hoverRow = -1
			hoverCopy, hoverClear, hoverClose = false, false, false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_MOUSEWHEEL:
			steps := int(int16(uint16(wParam>>16))) / 120
			if steps == 0 {
				return 0
			}
			scrollOffset = clampScrollOffset(scrollOffset-steps*3, len(records), historyMaxRows)
			if selectedIndex < scrollOffset {
				selectedIndex = scrollOffset
			}
			if lastIndex := scrollOffset + historyMaxRows - 1; selectedIndex > lastIndex {
				selectedIndex = lastIndex
			}
			if selectedIndex >= len(records) {
				selectedIndex = len(records) - 1
			}
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
			switch {
			case pointInRect(x, y, layout.close), pointInRect(x, y, layout.done):
				pDestroyWindow.Call(hwnd)
			case pointInRect(x, y, layout.copy):
				if selectedIndex < 0 || selectedIndex >= len(records) {
					showMessage(hwnd, "暂无可复制的记录。", MB_ICONINFO)
					return 0
				}
				item := records[selectedIndex]
				SetClipboardText(fmt.Sprintf("%s\n%s · %s\n\n%s", item.Title, historyTime(item), historyAgent(item), item.Summary))
				showMessage(hwnd, "已复制到剪贴板。", MB_ICONINFO)
			case pointInRect(x, y, layout.clear):
				if showConfirm(hwnd, "确定清空全部推送历史吗？") {
					_ = notify.ClearHistory("")
					records, _ = notify.GetHistory(100, "")
					selectedIndex = 0
					scrollOffset = 0
					pInvalidateRect.Call(hwnd, 0, 1)
				}
			default:
				if index := historyRowAt(layout, x, y, scrollOffset, len(records)); index >= 0 {
					selectedIndex = index
					pInvalidateRect.Call(hwnd, 0, 1)
				}
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
	if atom, _, registerErr := pRegisterClassExW.Call(uintptr(unsafe.Pointer(&windowClass))); atom == 0 {
		debugLog("RegisterClassExW history failed: %v", registerErr)
		return
	}
	defer pUnregisterClassW.Call(uintptr(unsafe.Pointer(className)), hInstance)

	width, height := logicalSize(historyWidth, historyHeight)
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
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
	setModalParent(parentHwnd, false)
	defer setModalParent(parentHwnd, true)

	runDialogLoop(&dialog)
}

func historyRowAt(layout historyLayout, x, y int32, scrollOffset, total int) int {
	if !pointInRect(x, y, layout.list) {
		return -1
	}
	if y < historyRowTop {
		return -1
	}
	row := int((y - historyRowTop) / historyRowHeight)
	if row < 0 || row >= historyMaxRows {
		return -1
	}
	index := scrollOffset + row
	if index < 0 || index >= total {
		return -1
	}
	if !pointInRect(x, y, layout.rows[row]) {
		return -1
	}
	return index
}

func clampScrollOffset(offset, total, maxRows int) int {
	maximum := total - maxRows
	if maximum < 0 {
		maximum = 0
	}
	if offset < 0 {
		return 0
	}
	if offset > maximum {
		return maximum
	}
	return offset
}
