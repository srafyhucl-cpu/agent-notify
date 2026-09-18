//go:build windows

package ui

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

// handleLeftButtonDown 处理 WM_LBUTTONDOWN：按当前视图分发，仪表盘逻辑兜底。
func (app *WidgetApp) handleLeftButtonDown(hwnd uintptr, lParam uintptr) uintptr {
	x := int32(lParam & 0xFFFF)
	y := int32((lParam >> 16) & 0xFFFF)
	x, y = unscalePoint(x, y)

	switch app.currentView {
	case WidgetViewRepair:
		return app.handleRepairLeftDown(hwnd, x, y)
	case WidgetViewHistory:
		return app.handleHistoryLeftDown(hwnd, x, y)
	case WidgetViewSettings:
		return app.handleSettingsLeftDown(hwnd, x, y)
	case WidgetViewLogin:
		return app.handleLoginLeftDown(hwnd, x, y)
	}
	return app.handleDashboardLeftDown(hwnd, x, y)
}

func (app *WidgetApp) handleRepairLeftDown(hwnd uintptr, x, y int32) uintptr {
	backRect, _, closeRect := subviewCommonHeader()
	recheckBtn := RECT{14, widgetHeight - 56, 195, widgetHeight - 14}
	doneBtn := RECT{205, widgetHeight - 56, 386, widgetHeight - 14}
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
}

func (app *WidgetApp) handleHistoryLeftDown(hwnd uintptr, x, y int32) uintptr {
	backRect, _, closeRect := subviewCommonHeader()
	historyItems := app.loadHistory(historyListPageSize * 10)
	total := len(historyItems)
	const pageSize = historyListPageSize
	prevBtn, nextBtn, clearBtn := historyHeaderButtons(total, pageSize)
	copyBtn := RECT{14, widgetHeight - 56, 195, widgetHeight - 14}
	doneBtn := RECT{205, widgetHeight - 56, 386, widgetHeight - 14}

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
	listCard := RECT{14, 48, 386, 294}
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
}

func (app *WidgetApp) handleSettingsLeftDown(hwnd uintptr, x, y int32) uintptr {
	backRect, _, closeRect := subviewCommonHeader()
	wechatCard := RECT{14, 48, 386, 104}
	reloginBtn := RECT{wechatCard.Right - 100, wechatCard.Top + 12, wechatCard.Right - 12, wechatCard.Bottom - 12}
	optCard := RECT{14, 112, 386, widgetHeight - 64}
	replyTrack := RECT{optCard.Right - 64, optCard.Top + 134, optCard.Right - 18, optCard.Top + 158}
	themePill := RECT{optCard.Right - 100, optCard.Top + 188, optCard.Right - 18, optCard.Top + 220}
	agentPill := RECT{optCard.Right - 120, optCard.Top + 240, optCard.Right - 18, optCard.Top + 270}
	saveBtn := RECT{14, widgetHeight - 56, 195, widgetHeight - 14}
	doneBtn := RECT{205, widgetHeight - 56, 386, widgetHeight - 14}

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
		commandCodeVal := app.commandCodeWindowSec
		if app.commandCodeEdit != 0 {
			if wd, err := strconv.Atoi(strings.TrimSpace(getWindowText(app.commandCodeEdit))); err == nil && wd >= 0 {
				commandCodeVal = wd
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
		cfg.CommandCodeReplyWindowSec = commandCodeVal
		cfg.Theme = app.theme
		cfg.DefaultAgent = app.currentAgent
		if err := config.SaveConfig(cfg, ""); err != nil {
			app.settingsError = "保存失败：" + err.Error()
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0
		}
		app.quietHours = cfg.QuietHours
		app.cooldownMin = cfg.CooldownMin
		app.commandCodeWindowSec = cfg.CommandCodeReplyWindowSec
		app.switchView(WidgetViewDashboard)
		return 0
	}
	return 0
}

func (app *WidgetApp) handleLoginLeftDown(hwnd uintptr, x, y int32) uintptr {
	backRect, _, closeRect := subviewCommonHeader()
	refreshBtn := RECT{14, widgetHeight - 56, 195, widgetHeight - 14}
	doneBtn := RECT{205, widgetHeight - 56, 386, widgetHeight - 14}
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

func (app *WidgetApp) handleDashboardLeftDown(hwnd uintptr, x, y int32) uintptr {
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
}
