//go:build windows

package ui

import "unsafe"

// handleMouseMove 处理 WM_MOUSEMOVE：先按需启动鼠标离开跟踪，再按当前视图分发。
func (app *WidgetApp) handleMouseMove(hwnd uintptr, lParam uintptr) uintptr {
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
		return app.handleRepairMouseMove(hwnd, x, y)
	case WidgetViewHistory:
		return app.handleHistoryMouseMove(hwnd, x, y)
	case WidgetViewSettings:
		return app.handleSettingsMouseMove(hwnd, x, y)
	case WidgetViewLogin:
		return app.handleLoginMouseMove(hwnd, x, y)
	default:
		return app.handleDashboardMouseMove(hwnd, x, y)
	}
}

func (app *WidgetApp) handleRepairMouseMove(hwnd uintptr, x, y int32) uintptr {
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
}

func (app *WidgetApp) handleHistoryMouseMove(hwnd uintptr, x, y int32) uintptr {
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
}

func (app *WidgetApp) handleSettingsMouseMove(hwnd uintptr, x, y int32) uintptr {
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
}

func (app *WidgetApp) handleLoginMouseMove(hwnd uintptr, x, y int32) uintptr {
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
}

func (app *WidgetApp) handleDashboardMouseMove(hwnd uintptr, x, y int32) uintptr {
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
	return 0
}
