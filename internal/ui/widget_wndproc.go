//go:build windows

package ui

import (
	"os"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// handleMessage 处理悬浮窗的窗口消息；由 syscall.NewCallback 在 UI 线程调用。
func (app *WidgetApp) handleMessage(hwnd, msg, wParam, lParam uintptr) uintptr {
	message := uint32(msg)
	if message == WM_USER_WAKEUP || (msgWakeupID != 0 && message == msgWakeupID) {
		restoreAndBringToFront(hwnd)
		return 0
	}
	if message == WM_USER_REFRESH {
		app.refreshState()
		pInvalidateRect.Call(hwnd, 0, 0)
		return 0
	}
	if message == WM_USER_VERIFY {
		// 用户可能已经离开登录视图，此时忽略配对码请求。
		if app.currentView == WidgetViewLogin {
			app.showVerifyPrompt(hwnd)
		}
		return 0
	}
	if message == WM_USER_REPAIR_DONE {
		app.applyRepairDone()
		app.refreshState()
		pInvalidateRect.Call(hwnd, 0, 0)
		return 0
	}
	if app.dialog.visible && app.handleDialogInput(hwnd, message, wParam, lParam) {
		return 0
	}
	if app.handleUpdateMessage(hwnd, message) {
		return 0
	}

	switch message {
	case WM_CREATE:
		setUIDPI(windowDPI(hwnd))
		resizeForCurrentDPI(hwnd, widgetWidth, widgetHeight)
		app.hwnd.Store(hwnd)
		app.loginState.setWindow(hwnd)
		app.tray = NewTrayManager(hwnd)
		app.refreshState()
		app.applyThemeToWindow()
		pSetTimer.Call(hwnd, 1, 5000, 0)
		return 0

	case WM_ACTIVATE:
		if (wParam & 0xFFFF) == 0 { // WA_INACTIVE
			if app.agentDropdownOpen {
				app.agentDropdownOpen = false
				pInvalidateRect.Call(hwnd, 0, 0)
			}
		}
		return 0

	case WM_KILLFOCUS:
		if app.agentDropdownOpen {
			app.agentDropdownOpen = false
			pInvalidateRect.Call(hwnd, 0, 0)
		}
		return 0

	case WM_MOUSEWHEEL:
		if app.currentView == WidgetViewHistory {
			delta := int16((wParam >> 16) & 0xFFFF)
			historyItems := app.loadHistory(historyListPageSize * 10)
			total := len(historyItems)
			const pageSize = historyListPageSize
			if delta > 0 {
				if app.historyPageOffset >= pageSize {
					app.historyPageOffset -= pageSize
					app.historySelectedIndex = app.historyPageOffset
					pInvalidateRect.Call(hwnd, 0, 0)
				}
			} else if delta < 0 {
				if app.historyPageOffset+pageSize < total {
					app.historyPageOffset += pageSize
					app.historySelectedIndex = app.historyPageOffset
					pInvalidateRect.Call(hwnd, 0, 0)
				}
			}
			return 0
		}

	case WM_KEYDOWN:
		if wParam == VK_ESCAPE {
			if app.agentDropdownOpen {
				app.agentDropdownOpen = false
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if app.currentView != WidgetViewDashboard {
				app.switchView(WidgetViewDashboard)
				return 0
			}
		}
		return 0

	case WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC:
		theme := app.getTheme()
		editHdc := wParam
		pSetTextColor.Call(editHdc, uintptr(theme.TextPrimary))
		pSetBkColor.Call(editHdc, uintptr(theme.InputBg))
		if app.editBrush != 0 {
			pDeleteObject.Call(app.editBrush)
		}
		app.editBrush, _, _ = pCreateSolidBrush.Call(uintptr(theme.InputBg))
		return app.editBrush

	case WM_TIMER:
		app.refreshState()
		_ = os.WriteFile(app.paths.WidgetAliveFile, []byte(time.Now().Format(time.RFC3339)), 0600)
		pInvalidateRect.Call(hwnd, 0, 0)
		return 0

	case WM_DPICHANGED:
		setUIDPI(uint32(wParam & 0xFFFF))
		resizeForCurrentDPI(hwnd, widgetWidth, widgetHeight)
		if app.quietEdit != 0 {
			rect := scaleRect(RECT{246, 126, 372, 150})
			pSetWindowPos.Call(app.quietEdit, 0, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top), SWP_NOZORDER|SWP_NOACTIVATE)
		}
		if app.cooldownEdit != 0 {
			rect := scaleRect(RECT{246, 180, 372, 204})
			pSetWindowPos.Call(app.cooldownEdit, 0, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top), SWP_NOZORDER|SWP_NOACTIVATE)
		}
		if app.verifyEdit != 0 {
			rect := verifyEditRect()
			pSetWindowPos.Call(app.verifyEdit, 0, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top), SWP_NOZORDER|SWP_NOACTIVATE)
		}
		pInvalidateRect.Call(hwnd, 0, 0)
		return 0

	case WM_ERASEBKGND:
		return 1

	case WM_PAINT:
		paintDoubleBuffered(hwnd, func(hdc uintptr, width, height int32) {
			drawUI(hdc, width, height, app)
		})
		return 0

	case WM_MOUSEMOVE:
		return app.handleMouseMove(hwnd, lParam)

	case WM_MOUSELEAVE:
		app.isTracking = false
		app.hover = widgetHoverState{}
		app.repairHover = repairViewHover{}
		app.historyHover = historyViewHover{}
		app.settingsHover = settingsViewHover{}
		app.loginHover = loginViewHover{}
		app.dropdownHoverIndex = -1
		pInvalidateRect.Call(hwnd, 0, 0)
		return 0

	case WM_LBUTTONDOWN:
		return app.handleLeftButtonDown(hwnd, lParam)

	case WM_TRAYICON:
		switch lParam {
		case WM_LBUTTONDBLCLK:
			restoreAndBringToFront(hwnd)
		case WM_RBUTTONUP:
			visible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
			app.tray.ShowContextMenu(visible != 0, app.enabledAgentStates())
		}
		return 0

	case WM_COMMAND:
		switch int(wParam & 0xFFFF) {
		case IDOK:
			// 配对码输入框聚焦时回车由消息循环转成 IDOK，这里按"提交配对码"处理。
			if app.verifyPrompt {
				app.submitVerifyCode(hwnd)
			}
		case IDM_TOGGLE_SHOW:
			visible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
			if visible != 0 {
				savePosition(hwnd, app.paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
			} else {
				restoreAndBringToFront(hwnd)
			}
		case IDM_TOGGLE_OPENCODE, IDM_TOGGLE_CODEX, IDM_TOGGLE_ANTIGRAVITY, IDM_TOGGLE_DEVIN:
			if agentID, ok := trayAgentIDForCommand(int(wParam & 0xFFFF)); ok && app.toggleAgent(agentID) {
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			}
		case IDM_HISTORY:
			app.switchView(WidgetViewHistory)
			restoreAndBringToFront(hwnd)
		case IDM_SETTINGS:
			app.switchView(WidgetViewSettings)
			restoreAndBringToFront(hwnd)
		case IDM_TEST_PUSH:
			go func() {
				_ = notify.SendNotification(notify.NotifyOptions{Agent: "test", Title: "【测试】Agent-notify", Summary: "ClawBot 推送链路正常。"})
			}()
		case IDM_UPDATE:
			app.startUpdateCheck(hwnd)
		case IDM_EXIT:
			savePosition(hwnd, app.paths.WidgetPosFile)
			_ = os.WriteFile(app.paths.WidgetExitMarker, []byte(time.Now().Format(time.RFC3339)), 0600)
			app.sessionCancel()
			if app.tray != nil {
				app.tray.Destroy()
			}
			pDestroyWindow.Call(hwnd)
			pPostQuitMessage.Call(0)
		}
		return 0

	case WM_CLOSE:
		savePosition(hwnd, app.paths.WidgetPosFile)
		pShowWindow.Call(hwnd, SW_HIDE)
		return 0

	case WM_DESTROY:
		app.sessionCancel()
		app.releaseFonts()
		if app.tray != nil {
			app.tray.Destroy()
		}
		pPostQuitMessage.Call(0)
		return 0
	}
	result, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
	return result
}
