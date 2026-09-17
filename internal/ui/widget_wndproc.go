//go:build windows

package ui

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
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
		if !app.isTracking {
			var track TRACKMOUSEEVENT
			track.CbSize = uint32(unsafe.Sizeof(track))
			track.DwFlags = 0x00000002
			track.HWndTrack = hwnd
			pTrackMouseEvent.Call(uintptr(unsafe.Pointer(&track)))
			app.isTracking = true
		}

		x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))

		switch app.currentView {
		case WidgetViewRepair:
			backRect, _, closeRect := subviewCommonHeader()
			recheckBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}
			prev := app.repairHover
			app.repairHover = repairViewHover{
				back:    pointInRect(x, y, backRect),
				close:   pointInRect(x, y, closeRect),
				recheck: pointInRect(x, y, recheckBtn),
				done:    pointInRect(x, y, doneBtn),
			}
			if app.repairHover != prev {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if app.repairHover.back || app.repairHover.close || app.repairHover.recheck || app.repairHover.done {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WidgetViewHistory:
			backRect, _, closeRect := subviewCommonHeader()
			historyItems := app.loadHistory(historyListPageSize * 10)
			total := len(historyItems)
			const pageSize = historyListPageSize
			prevBtn, nextBtn, clearBtn := historyHeaderButtons(total, pageSize)
			copyBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}
			listCard := RECT{14, 48, 386, 260}
			rowIdx := historyRowIndexAt(x, y, listCard, pageSize)
			if rowIdx >= 0 && (app.historyPageOffset+rowIdx) >= total {
				rowIdx = -1
			}
			prev := app.historyHover
			app.historyHover = historyViewHover{
				back:     pointInRect(x, y, backRect),
				close:    pointInRect(x, y, closeRect),
				clear:    pointInRect(x, y, clearBtn),
				prev:     pointInRect(x, y, prevBtn),
				next:     pointInRect(x, y, nextBtn),
				copy:     pointInRect(x, y, copyBtn),
				done:     pointInRect(x, y, doneBtn),
				rowIndex: rowIdx,
			}
			if app.historyHover != prev {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if app.historyHover.back || app.historyHover.close || app.historyHover.clear || app.historyHover.prev || app.historyHover.next || app.historyHover.copy || app.historyHover.done || rowIdx >= 0 {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WidgetViewSettings:
			backRect, _, closeRect := subviewCommonHeader()
			wechatCard := RECT{14, 48, 386, 104}
			reloginBtn := RECT{wechatCard.Right - 100, wechatCard.Top + 12, wechatCard.Right - 12, wechatCard.Bottom - 12}
			optCard := RECT{14, 112, 386, 384}
			replyTrack := RECT{optCard.Right - 64, optCard.Top + 134, optCard.Right - 18, optCard.Top + 158}
			themePill := RECT{optCard.Right - 100, optCard.Top + 188, optCard.Right - 18, optCard.Top + 220}
			agentPill := RECT{optCard.Right - 120, optCard.Top + 240, optCard.Right - 18, optCard.Top + 270}
			saveBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}
			prev := app.settingsHover
			app.settingsHover = settingsViewHover{
				back:        pointInRect(x, y, backRect),
				close:       pointInRect(x, y, closeRect),
				relogin:     pointInRect(x, y, reloginBtn),
				replyToggle: pointInRect(x, y, replyTrack),
				themePill:   pointInRect(x, y, themePill),
				agentCycle:  pointInRect(x, y, agentPill),
				save:        pointInRect(x, y, saveBtn),
				done:        pointInRect(x, y, doneBtn),
			}
			if app.settingsHover != prev {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if app.settingsHover.back || app.settingsHover.close || app.settingsHover.relogin || app.settingsHover.replyToggle || app.settingsHover.themePill || app.settingsHover.agentCycle || app.settingsHover.save || app.settingsHover.done {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WidgetViewLogin:
			backRect, _, closeRect := subviewCommonHeader()
			refreshBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}
			_, submitBtn := loginVerifyRects()
			prev := app.loginHover
			app.loginHover = loginViewHover{
				back:    pointInRect(x, y, backRect),
				close:   pointInRect(x, y, closeRect),
				refresh: pointInRect(x, y, refreshBtn),
				done:    pointInRect(x, y, doneBtn),
				submit:  app.verifyPrompt && pointInRect(x, y, submitBtn),
			}
			if app.loginHover != prev {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if app.loginHover.back || app.loginHover.close || app.loginHover.refresh || app.loginHover.done || app.loginHover.submit {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		default:
			if app.isSingleAgentMode() && app.agentDropdownOpen {
				dropRect := RECT{76, 130, 266, 272}
				if pointInRect(x, y, dropRect) {
					app.dropdownHoverIndex = int((y - 134) / 34)
					pInvalidateRect.Call(hwnd, 0, 0)
					hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
					pSetCursor.Call(hand)
					return 0
				} else {
					if app.dropdownHoverIndex != -1 {
						app.dropdownHoverIndex = -1
						pInvalidateRect.Call(hwnd, 0, 0)
					}
				}
			}

			layout := widgetLayoutRects()
			previous := app.hover
			app.hover = widgetHoverAt(x, y, layout)
			if app.hover != previous {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if app.hover.any() {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
		}
		return 0

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
		x := int32(lParam & 0xFFFF)
		y := int32((lParam >> 16) & 0xFFFF)
		x, y = unscalePoint(x, y)

		switch app.currentView {
		case WidgetViewRepair:
			backRect, _, closeRect := subviewCommonHeader()
			recheckBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}
			if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
				app.switchView(WidgetViewDashboard)
				return 0
			}
			if isSubviewHeaderDrag(x, y) {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, app.paths.WidgetPosFile)
				return 0
			}
			if pointInRect(x, y, recheckBtn) {
				if !app.repairing {
					app.repairing = true
					app.repairError = ""
					pInvalidateRect.Call(hwnd, 0, 0)
					app.runRepairCheck(hwnd)
				}
				return 0
			}
			return 0

		case WidgetViewHistory:
			backRect, _, closeRect := subviewCommonHeader()
			historyItems := app.loadHistory(historyListPageSize * 10)
			total := len(historyItems)
			const pageSize = historyListPageSize
			prevBtn, nextBtn, clearBtn := historyHeaderButtons(total, pageSize)
			copyBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}

			if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
				app.historyConfirmClear = false
				app.switchView(WidgetViewDashboard)
				return 0
			}
			if isSubviewHeaderDrag(x, y, clearBtn, prevBtn, nextBtn) {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, app.paths.WidgetPosFile)
				return 0
			}
			if total > pageSize {
				if pointInRect(x, y, prevBtn) {
					if app.historyPageOffset >= pageSize {
						app.historyPageOffset -= pageSize
						app.historySelectedIndex = app.historyPageOffset
						pInvalidateRect.Call(hwnd, 0, 0)
					}
					return 0
				}
				if pointInRect(x, y, nextBtn) {
					if app.historyPageOffset+pageSize < total {
						app.historyPageOffset += pageSize
						app.historySelectedIndex = app.historyPageOffset
						pInvalidateRect.Call(hwnd, 0, 0)
					}
					return 0
				}
			}
			if pointInRect(x, y, clearBtn) {
				if !app.historyConfirmClear {
					app.historyConfirmClear = true
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				app.historyConfirmClear = false
				if err := os.WriteFile(app.paths.PushLog, []byte{}, 0600); err != nil {
					app.reportActionError("清空推送历史失败：%v", err)
					return 0
				}
				app.historyPageOffset = 0
				app.historySelectedIndex = 0
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			app.historyConfirmClear = false
			listCard := RECT{14, 48, 386, 260}
			if pointInRect(x, y, listCard) {
				if rowIdx := historyRowIndexAt(x, y, listCard, pageSize); rowIdx >= 0 {
					targetIdx := app.historyPageOffset + rowIdx
					if targetIdx >= 0 && targetIdx < len(historyItems) {
						app.historySelectedIndex = targetIdx
						pInvalidateRect.Call(hwnd, 0, 0)
					}
				}
				return 0
			}
			if pointInRect(x, y, copyBtn) {
				if len(historyItems) > app.historySelectedIndex {
					item := historyItems[app.historySelectedIndex]
					SetClipboardText(fmt.Sprintf("%s\n\n%s", item.Title, item.Summary))
				}
				return 0
			}
			return 0

		case WidgetViewSettings:
			backRect, _, closeRect := subviewCommonHeader()
			wechatCard := RECT{14, 48, 386, 104}
			reloginBtn := RECT{wechatCard.Right - 100, wechatCard.Top + 12, wechatCard.Right - 12, wechatCard.Bottom - 12}
			optCard := RECT{14, 112, 386, 384}
			replyTrack := RECT{optCard.Right - 64, optCard.Top + 134, optCard.Right - 18, optCard.Top + 158}
			themePill := RECT{optCard.Right - 100, optCard.Top + 188, optCard.Right - 18, optCard.Top + 220}
			agentPill := RECT{optCard.Right - 120, optCard.Top + 240, optCard.Right - 18, optCard.Top + 270}
			saveBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}

			if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
				app.settingsError = ""
				app.switchView(WidgetViewDashboard)
				return 0
			}
			if isSubviewHeaderDrag(x, y) {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, app.paths.WidgetPosFile)
				return 0
			}
			if pointInRect(x, y, reloginBtn) {
				app.switchView(WidgetViewLogin)
				return 0
			}
			if pointInRect(x, y, replyTrack) {
				app.replyEnabled = !app.replyEnabled
				app.mutateConfig(func(cfg *config.AppConfig) {
					cfg.ReplyEnabled = app.replyEnabled
				})
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if pointInRect(x, y, themePill) {
				app.toggleTheme()
				return 0
			}
			if pointInRect(x, y, agentPill) {
				app.nextAgent()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if pointInRect(x, y, saveBtn) {
				quietVal := ""
				if app.quietEdit != 0 {
					quietVal = strings.TrimSpace(getWindowText(app.quietEdit))
				}
				if quietVal != "" && !config.ValidQuietHours(quietVal) {
					app.settingsError = "格式无效：如 23-8，留空关闭"
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				app.settingsError = ""
				cooldownVal := app.cooldownMin
				if app.cooldownEdit != 0 {
					if cd, err := strconv.Atoi(strings.TrimSpace(getWindowText(app.cooldownEdit))); err == nil && cd > 0 {
						cooldownVal = cd
					}
				}
				cfg, err := config.LoadConfig("")
				if err != nil {
					app.settingsError = "读取设置失败，未保存：" + err.Error()
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				cfg.QuietHours = quietVal
				cfg.CooldownMin = cooldownVal
				cfg.ReplyEnabled = app.replyEnabled
				cfg.Theme = app.theme
				cfg.DefaultAgent = app.currentAgent
				if err := config.SaveConfig(cfg, ""); err != nil {
					app.settingsError = "保存失败：" + err.Error()
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				app.quietHours = cfg.QuietHours
				app.cooldownMin = cfg.CooldownMin
				app.switchView(WidgetViewDashboard)
				return 0
			}
			return 0

		case WidgetViewLogin:
			backRect, _, closeRect := subviewCommonHeader()
			refreshBtn := RECT{14, 394, 195, 436}
			doneBtn := RECT{205, 394, 386, 436}
			if app.verifyPrompt {
				if _, submitBtn := loginVerifyRects(); pointInRect(x, y, submitBtn) {
					app.submitVerifyCode(hwnd)
					return 0
				}
			}
			if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
				app.switchView(WidgetViewDashboard)
				return 0
			}
			if isSubviewHeaderDrag(x, y) {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, app.paths.WidgetPosFile)
				return 0
			}
			if pointInRect(x, y, refreshBtn) {
				app.startInAppLoginFlow()
				return 0
			}
			return 0
		}

		// In WidgetViewDashboard:
		layout := widgetLayoutRects()

		if app.isSingleAgentMode() && app.agentDropdownOpen {
			dropRect := RECT{76, 130, 266, 272}
			if pointInRect(x, y, dropRect) {
				descriptors := agentmeta.All()
				if y >= 134 && y < 134+int32(len(descriptors))*34 {
					itemIdx := int((y - 134) / 34)
					if itemIdx >= 0 && itemIdx < len(descriptors) {
						app.currentAgent = descriptors[itemIdx].ID
						app.mutateConfig(func(cfg *config.AppConfig) {
							cfg.DefaultAgent = app.currentAgent
						})
						app.agentDropdownOpen = false
						app.refreshState()
						pInvalidateRect.Call(hwnd, 0, 0)
						return 0
					}
				}
			}
			app.agentDropdownOpen = false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0
		}

		if pointInRect(x, y, layout.drag) && !pointInRect(x, y, layout.minimize) && !pointInRect(x, y, layout.close) && !pointInRect(x, y, layout.themeToggle) {
			pReleaseCapture.Call()
			pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
			savePosition(hwnd, app.paths.WidgetPosFile)
			return 0
		}
		if pointInRect(x, y, layout.themeToggle) {
			app.toggleTheme()
			return 0
		}
		if pointInRect(x, y, layout.minimize) || pointInRect(x, y, layout.close) {
			savePosition(hwnd, app.paths.WidgetPosFile)
			pShowWindow.Call(hwnd, SW_HIDE)
			return 0
		}
		if pointInRect(x, y, layout.modeToggle) {
			app.toggleAgentMode()
			app.refreshState()
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0
		}
		if app.isSingleAgentMode() {
			badge := RECT{layout.singleAgent.Left + 14, layout.singleAgent.Top + 14, layout.singleAgent.Left + 50, layout.singleAgent.Top + 50}
			if pointInRect(x, y, layout.switchAgent) || pointInRect(x, y, badge) {
				app.showAgentDropdown(hwnd, layout.switchAgent)
				return 0
			}
			if pointInRect(x, y, layout.singleSwitch) {
				app.toggleAgent(app.focusedAgentID())
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
		}
		if app.toggleAgentAt(x, y, layout) {
			app.refreshState()
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0
		}
		if pointInRect(x, y, layout.recent) || pointInRect(x, y, layout.history) {
			app.switchView(WidgetViewHistory)
			return 0
		}
		if pointInRect(x, y, layout.test) || pointInRect(x, y, layout.settings) {
			app.switchView(WidgetViewSettings)
			return 0
		}
		if pointInRect(x, y, layout.hide) {
			app.switchView(WidgetViewLogin)
			return 0
		}
		if pointInRect(x, y, layout.repair) {
			app.switchView(WidgetViewRepair)
			return 0
		}
		if pointInRect(x, y, layout.update) {
			app.handleUpdateClick(hwnd)
			return 0
		}
		return 0

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
