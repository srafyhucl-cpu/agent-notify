//go:build windows

package ui

import (
	"strings"
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func newIconFont(size int32) uintptr {
	font, _, _ := pCreateFontW.Call(
		uintptr(scaleFloat(size)), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0,
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Segoe Fluent Icons"))),
	)
	return font
}

func drawIconTextButton(hdc uintptr, rect RECT, glyph, label string, hover, primary, danger bool, font, iconFont uintptr, themes ...ThemePalette) {
	theme := ThemeDark
	if len(themes) > 0 {
		theme = themes[0]
	}
	fillColor := uintptr(theme.ButtonBg)
	textColor := uintptr(theme.TextPrimary)
	borderColor := uintptr(theme.ButtonBorder)
	if primary {
		fillColor = uintptr(theme.AccentSuccess)
		textColor = uintptr(RGB(245, 252, 250))
		borderColor = uintptr(theme.AccentSuccess)
	}
	if danger {
		textColor = uintptr(theme.AccentDanger)
		borderColor = uintptr(theme.AccentDanger)
	}
	if hover {
		fillColor = uintptr(theme.ButtonBgHover)
		borderColor = uintptr(theme.ButtonBorderHover)
		if primary {
			fillColor = uintptr(theme.AccentSuccess)
		}
		if danger {
			fillColor = uintptr(theme.ButtonBgHover)
		}
	}
	fillRoundRect(hdc, rect, 7, fillColor)
	strokeRoundRect(hdc, rect, 7, fillColor, borderColor, 1)

	textWidth := unscaleFloat(measureHdcTextWidth(hdc, font, label))
	iconWidth := int32(0)
	gap := int32(0)
	if glyph != "" {
		iconWidth = 16
		if label != "" {
			gap = 6
		}
	}
	totalWidth := iconWidth + gap + textWidth
	rectWidth := rect.Right - rect.Left
	startX := rect.Left + (rectWidth-totalWidth)/2
	if startX < rect.Left+4 {
		startX = rect.Left + 4
	}

	if glyph != "" {
		iconRect := RECT{
			Left:   startX,
			Top:    rect.Top,
			Right:  startX + iconWidth,
			Bottom: rect.Bottom,
		}
		pSelectObject.Call(hdc, iconFont)
		pSetTextColor.Call(hdc, textColor)
		DrawText(hdc, glyph, &iconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	}

	if label != "" {
		textRect := RECT{
			Left:   startX + iconWidth + gap,
			Top:    rect.Top,
			Right:  rect.Right - 4,
			Bottom: rect.Bottom,
		}
		pSelectObject.Call(hdc, font)
		pSetTextColor.Call(hdc, textColor)
		DrawText(hdc, label, &textRect, DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	}
}

func measureHdcTextWidth(hdc, font uintptr, text string) int32 {
	if hdc == 0 || font == 0 || text == "" {
		return 0
	}
	wide, err := syscall.UTF16FromString(text)
	if err != nil || len(wide) <= 1 {
		return 0
	}
	oldFont, _, _ := pSelectObject.Call(hdc, font)
	var size SIZE
	pGetTextExtentPoint32W.Call(
		hdc,
		uintptr(unsafe.Pointer(&wide[0])),
		uintptr(len(wide)-1),
		uintptr(unsafe.Pointer(&size)),
	)
	pSelectObject.Call(hdc, oldFont)
	return size.CX
}

func drawWindowButton(hdc uintptr, rect RECT, glyph string, hover, danger bool, iconFont uintptr, themes ...ThemePalette) {
	theme := ThemeDark
	if len(themes) > 0 {
		theme = themes[0]
	}
	fillColor := uintptr(theme.Background)
	if hover {
		fillColor = uintptr(theme.WindowBtnHover)
		if danger {
			fillColor = uintptr(theme.WindowBtnDangerHover)
		}
	}
	fillRoundRect(hdc, rect, 6, fillColor)
	pSelectObject.Call(hdc, iconFont)
	if danger && hover {
		pSetTextColor.Call(hdc, uintptr(RGB(255, 245, 245)))
	} else {
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	}
	DrawText(hdc, glyph, &rect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
}

func drawFluentDockButton(hdc uintptr, rect RECT, glyph, label string, hover, active bool, accentColor uint32, font, iconFont uintptr, themes ...ThemePalette) {
	theme := ThemeDark
	if len(themes) > 0 {
		theme = themes[0]
	}
	fillColor := uintptr(theme.ButtonBg)
	borderColor := uintptr(theme.ButtonBorder)
	iconColor := uintptr(theme.TextSecondary)
	textColor := uintptr(theme.TextSecondary)

	if active && accentColor != 0 {
		fillColor = uintptr(theme.ButtonBgHover)
		borderColor = uintptr(accentColor)
		iconColor = uintptr(accentColor)
		textColor = uintptr(accentColor)
	}

	if hover {
		fillColor = uintptr(theme.ButtonBgHover)
		borderColor = uintptr(theme.ButtonBorderHover)
		iconColor = uintptr(theme.TextPrimary)
		textColor = uintptr(theme.TextPrimary)
		if active && accentColor != 0 {
			borderColor = uintptr(accentColor)
		}
	}

	fillRoundRect(hdc, rect, 8, fillColor)
	strokeRoundRect(hdc, rect, 8, fillColor, borderColor, 1)

	iconRect := RECT{
		Left:   rect.Left,
		Top:    rect.Top + 6,
		Right:  rect.Right,
		Bottom: rect.Top + 28,
	}
	pSelectObject.Call(hdc, iconFont)
	pSetTextColor.Call(hdc, iconColor)
	DrawText(hdc, glyph, &iconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	textRect := RECT{
		Left:   rect.Left + 2,
		Top:    rect.Top + 28,
		Right:  rect.Right - 2,
		Bottom: rect.Bottom - 4,
	}
	pSelectObject.Call(hdc, font)
	pSetTextColor.Call(hdc, textColor)
	DrawText(hdc, label, &textRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
}

func drawSingleAgentCard(hdc uintptr, card widgetAgentCard, app *WidgetApp, layout widgetLayout, strongFont, baseFont, smallFont, iconFont uintptr, theme ThemePalette) {
	rect := layout.singleAgent
	fillColor := uintptr(theme.CardBg)
	borderColor := uintptr(theme.CardBorder)
	if card.Hover {
		fillColor = uintptr(theme.CardBgHover)
		borderColor = uintptr(theme.CardBorderHover)
	}
	fillRoundRect(hdc, rect, 10, fillColor)
	strokeRoundRect(hdc, rect, 10, fillColor, borderColor, 1)

	stateColor := uintptr(theme.TextMuted)
	switch {
	case !card.Enabled:
		stateColor = uintptr(theme.TextMuted)
	case card.State == integration.StateConnected:
		stateColor = uintptr(theme.AccentSuccess)
	case card.State == integration.StatePendingRestart:
		stateColor = uintptr(theme.AccentWarning)
	case card.State == integration.StateError:
		stateColor = uintptr(theme.AccentDanger)
	}

	// 1. 左上角图标徽章 (36x36 呼吸圆角块)
	badge := RECT{rect.Left + 14, rect.Top + 14, rect.Left + 50, rect.Top + 50}
	fillRoundRect(hdc, badge, 8, uintptr(theme.BadgeBg))
	drawEllipseLogical(hdc, badge.Left+13, badge.Top+13, badge.Left+23, badge.Top+23, stateColor, stateColor)

	// 2. 下拉选择框 Dropdown Menu (switchAgent)
	dropdownRect := layout.switchAgent
	dropFill := uintptr(theme.DropdownBg)
	dropBorder := uintptr(theme.DropdownBorder)
	if app.hover.switchAgent || app.agentDropdownOpen {
		dropFill = uintptr(theme.DropdownHover)
		dropBorder = uintptr(theme.AccentSuccess)
	}
	fillRoundRect(hdc, dropdownRect, 6, dropFill)
	strokeRoundRect(hdc, dropdownRect, 6, dropFill, dropBorder, 1)

	nameRect := RECT{dropdownRect.Left + 12, dropdownRect.Top, dropdownRect.Right - 32, dropdownRect.Bottom}
	pSelectObject.Call(hdc, strongFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, card.Name, &nameRect, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	arrowRect := RECT{dropdownRect.Right - 28, dropdownRect.Top, dropdownRect.Right - 8, dropdownRect.Bottom}
	pSelectObject.Call(hdc, iconFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	arrowGlyph := "\uE70D"
	if app.agentDropdownOpen {
		arrowGlyph = "\uE70E"
	}
	DrawText(hdc, arrowGlyph, &arrowRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	// 3. 右侧独立 Switch 滑块开关
	track := layout.singleSwitch
	trackColor := uintptr(theme.SwitchTrackOff)
	if card.Enabled {
		trackColor = uintptr(theme.SwitchTrackOn)
	}
	fillRoundRect(hdc, track, 10, trackColor)
	knobX := track.Left + 3
	if card.Enabled {
		knobX = track.Right - 19
	}
	drawEllipseLogical(hdc, knobX, track.Top+3, knobX+16, track.Bottom-3, uintptr(theme.KnobColor), uintptr(theme.KnobColor))

	// 4. 中间微诊断区域 (高 54px)
	diagBox := RECT{rect.Left + 12, rect.Top + 58, rect.Right - 12, rect.Top + 112}
	fillRoundRect(hdc, diagBox, 6, uintptr(theme.DiagBoxBg))
	strokeRoundRect(hdc, diagBox, 6, uintptr(theme.DiagBoxBg), uintptr(theme.DiagBoxBorder), 1)

	divLine := RECT{diagBox.Left + 10, diagBox.Top + 26, diagBox.Right - 10, diagBox.Top + 27}
	fillRectLogical(hdc, divLine, uintptr(theme.Divider))

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	DrawText(hdc, "接入方式", &RECT{diagBox.Left + 12, diagBox.Top + 3, diagBox.Left + 72, diagBox.Top + 23}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
	DrawText(hdc, "运行状态", &RECT{diagBox.Left + 12, diagBox.Top + 29, diagBox.Left + 72, diagBox.Top + 49}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, agentHookDescription(card.ID), &RECT{diagBox.Left + 76, diagBox.Top + 3, diagBox.Right - 12, diagBox.Top + 23}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)

	detailText := card.Detail
	if strings.TrimSpace(detailText) == "" {
		detailText = "状态正常"
	}
	pSetTextColor.Call(hdc, stateColor)
	DrawText(hdc, detailText, &RECT{diagBox.Left + 76, diagBox.Top + 29, diagBox.Right - 12, diagBox.Top + 49}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)

	// 5. 底部文字说明
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	DrawText(hdc, "任务结束或空闲时，将通过微信主动推送任务摘要", &RECT{rect.Left + 14, rect.Top + 116, rect.Right - 14, rect.Bottom - 4}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)
}

func drawAgentCard(hdc uintptr, card widgetAgentCard, baseFont, smallFont uintptr, theme ThemePalette) {
	rect := card.Rect
	fillColor := uintptr(theme.CardBg)
	borderColor := uintptr(theme.CardBorder)
	if card.Hover {
		fillColor = uintptr(theme.CardBgHover)
		borderColor = uintptr(theme.CardBorderHover)
	}
	fillRoundRect(hdc, rect, 8, fillColor)
	strokeRoundRect(hdc, rect, 8, fillColor, borderColor, 1)

	stateColor := uintptr(theme.TextMuted)
	stateText := card.Label
	switch {
	case !card.Enabled:
		stateText = "已暂停"
	case card.State == integration.StateConnected:
		stateColor = uintptr(theme.AccentSuccess)
	case card.State == integration.StatePendingRestart:
		stateColor = uintptr(theme.AccentWarning)
	case card.State == integration.StateError:
		stateColor = uintptr(theme.AccentDanger)
	}
	badge := RECT{rect.Left + 12, rect.Top + 12, rect.Left + 40, rect.Top + 40}
	fillRoundRect(hdc, badge, 6, uintptr(theme.BadgeBg))
	drawEllipseLogical(hdc, badge.Left+10, badge.Top+10, badge.Left+18, badge.Top+18, stateColor, stateColor)

	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, card.Name, &RECT{rect.Left + 48, rect.Top + 8, rect.Right - 10, rect.Top + 28}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, stateColor)
	DrawText(hdc, stateText, &RECT{rect.Left + 48, rect.Top + 28, rect.Right - 50, rect.Top + 46}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	track := RECT{rect.Right - 46, rect.Top + 26, rect.Right - 12, rect.Top + 44}
	trackColor := uintptr(theme.SwitchTrackOff)
	if card.Enabled {
		trackColor = uintptr(theme.SwitchTrackOn)
	}
	fillRoundRect(hdc, track, 8, trackColor)
	knobX := track.Left + 3
	if card.Enabled {
		knobX = track.Right - 17
	}
	drawEllipseLogical(hdc, knobX, track.Top+2, knobX+14, track.Bottom-2, uintptr(theme.KnobColor), uintptr(theme.KnobColor))
}

func cleanRecentPushTitle(title, agentID string) string {
	title = strings.TrimSpace(title)
	for _, prefix := range []string{"🟢", "⚠️", "🔴", "⚪", "⚡", "✨"} {
		title = strings.TrimPrefix(title, prefix)
	}
	title = strings.TrimSpace(title)
	if desc, ok := agentmeta.Lookup(agentID); ok {
		title = strings.TrimPrefix(title, desc.TitlePrefix)
	}
	if strings.HasPrefix(title, "【") {
		if idx := strings.Index(title, "】"); idx > 0 {
			tag := strings.ToLower(title[len("【"):idx])
			if tag == strings.ToLower(agentID) || tag == "通知" || tag == "测试" {
				title = strings.TrimSpace(title[idx+len("】"):])
			}
		}
	}
	return strings.TrimSpace(title)
}

func drawRecentCard(hdc uintptr, rect RECT, app *WidgetApp, strongFont, smallFont, iconFont uintptr, theme ThemePalette) {
	fillColor := uintptr(theme.CardBg)
	borderColor := uintptr(theme.CardBorder)
	if app.hover.recent {
		fillColor = uintptr(theme.CardBgHover)
		borderColor = uintptr(theme.CardBorderHover)
	}
	fillRoundRect(hdc, rect, 8, fillColor)
	strokeRoundRect(hdc, rect, 8, fillColor, borderColor, 1)

	// 1. Header 行
	pSelectObject.Call(hdc, iconFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	iconRect := RECT{rect.Left + 12, rect.Top + 7, rect.Left + 28, rect.Top + 25}
	DrawText(hdc, "\uE715", &iconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	labelRect := RECT{rect.Left + 30, rect.Top + 7, rect.Left + 86, rect.Top + 25}
	DrawText(hdc, "最近推送", &labelRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	// 来源微胶囊
	if app.lastPushAgent != "" {
		agentName := historyAgent(notify.HistoryItem{Agent: app.lastPushAgent})
		agentBadgeWidth := unscaleFloat(measureHdcTextWidth(hdc, smallFont, agentName)) + 12
		agentBadgeRect := RECT{rect.Left + 90, rect.Top + 8, rect.Left + 90 + agentBadgeWidth, rect.Top + 24}
		fillRoundRect(hdc, agentBadgeRect, 4, uintptr(theme.BadgeBg))
		strokeRoundRect(hdc, agentBadgeRect, 4, uintptr(theme.BadgeBg), uintptr(theme.CardBorderHover), 1)
		pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
		DrawText(hdc, agentName, &agentBadgeRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	}

	// 右侧状态与相对时间
	recentMeta := app.lastPushText
	statusColor := uintptr(app.recentStatusColor(theme))
	statusText := app.lastPushStatus
	switch statusText {
	case notify.StatusSuccess, "success":
		statusText = "成功"
	case notify.StatusFailed, "failed":
		statusText = "失败"
	}
	if statusText != "" {
		recentMeta = statusText + " · " + recentMeta
	}
	pSetTextColor.Call(hdc, statusColor)
	metaRect := RECT{rect.Left + 160, rect.Top + 7, rect.Right - 14, rect.Top + 25}
	DrawText(hdc, recentMeta, &metaRect, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	// 2. 细分割线
	divLine := RECT{rect.Left + 10, rect.Top + 29, rect.Right - 10, rect.Top + 30}
	fillRectLogical(hdc, divLine, uintptr(theme.Divider))

	// 3. 内容区：主标题 (Top+33~53) + 摘要预览 (Top+54~Bottom-6)
	rawTitle := app.lastPushTitle
	displayTitle := cleanRecentPushTitle(rawTitle, app.lastPushAgent)
	if displayTitle == "" {
		displayTitle = rawTitle
	}
	titleColor := uintptr(theme.TextPrimary)
	if strings.TrimSpace(displayTitle) == "" || displayTitle == "暂无推送记录" {
		displayTitle = "暂无推送记录"
		titleColor = uintptr(theme.TextMuted)
	}
	pSelectObject.Call(hdc, strongFont)
	pSetTextColor.Call(hdc, titleColor)
	titleRect := RECT{rect.Left + 12, rect.Top + 33, rect.Right - 32, rect.Top + 53}
	DrawText(hdc, displayTitle, &titleRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_END_ELLIPSIS|DT_NOPREFIX)

	summaryText := app.lastPushSummary
	if summaryText == "" {
		summaryText = "点击卡片可查看完整推送历史与详情"
	}
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
	summaryRect := RECT{rect.Left + 12, rect.Top + 54, rect.Right - 32, rect.Bottom - 6}
	DrawText(hdc, summaryText, &summaryRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_END_ELLIPSIS|DT_NOPREFIX)

	// 右侧微箭头 ›
	pSelectObject.Call(hdc, iconFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	arrowRect := RECT{rect.Right - 28, rect.Top + 38, rect.Right - 8, rect.Top + 66}
	DrawText(hdc, "\uE76C", &arrowRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
}

// connectionText returns the headline and supporting line for the ClawBot card.
func (app *WidgetApp) connectionText() (string, string, uint32) {
	switch {
	case app.setupError != "":
		return "首次接入失败", app.setupError, RGB(224, 165, 70)
	case !app.clawbotLoggedIn:
		return "ClawBot 未连接", "点击设置微信推送", RGB(224, 104, 104)
	case app.clawbotStale:
		return "ClawBot 登录已失效", "点击重新扫码登录", RGB(224, 104, 104)
	case !app.clawbotSessionReady:
		return "等待建立微信会话", "请先给 ClawBot 发送一条微信消息", RGB(224, 165, 70)
	default:
		detail := "主动推送会话已就绪"
		if app.clawbotHint != "" {
			detail = "已绑定 " + app.clawbotHint
		}
		return "ClawBot 已连接", detail, RGB(55, 190, 147)
	}
}

func (app *WidgetApp) focusedAgentHint() string {
	agentID := app.focusedAgentID()
	agentName := agentID
	if desc, ok := agentmeta.Lookup(agentID); ok {
		agentName = desc.DisplayName
	}
	if !app.agentEnabled(agentID) {
		return "聚焦 " + agentName + " · 通知已暂停"
	}
	status := app.integrationStatus(agentID)
	switch status.State {
	case integration.StateConnected:
		if app.agentRunning(agentID) {
			return "聚焦 " + agentName + " · 正常运行中"
		}
		return "聚焦 " + agentName + " · 已接入，待启动"
	case integration.StatePendingRestart:
		return "聚焦 " + agentName + " · 待重启生效"
	case integration.StateError:
		return "聚焦 " + agentName + " · 接入配置异常"
	default:
		if app.agentRunning(agentID) {
			return "聚焦 " + agentName + " · 进程运行但未接入"
		}
		return "聚焦 " + agentName + " · 未配置接入"
	}
}

func drawUI(hdc uintptr, width, height int32, app *WidgetApp) {
	theme := app.getTheme()
	background := RECT{0, 0, width, height}
	backgroundBrush, _, _ := pCreateSolidBrush.Call(uintptr(theme.Background))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&background)), backgroundBrush)
	pDeleteObject.Call(backgroundBrush)
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

	// If currently in an in-app subview, render that view!
	switch app.currentView {
	case WidgetViewRepair:
		drawRepairView(hdc, width, height, app, theme, titleFont, strongFont, baseFont, smallFont, iconFont)
		return
	case WidgetViewHistory:
		drawHistoryView(hdc, width, height, app, theme, titleFont, strongFont, baseFont, smallFont, iconFont)
		return
	case WidgetViewSettings:
		drawSettingsView(hdc, width, height, app, theme, titleFont, strongFont, baseFont, smallFont, iconFont)
		return
	case WidgetViewLogin:
		drawLoginView(hdc, width, height, app, theme, titleFont, strongFont, baseFont, smallFont, iconFont)
		return
	}

	// [WidgetViewDashboard: 主面板]
	healthColor, _ := app.currentHealth()
	fillRectLogical(hdc, RECT{0, 0, widgetWidth, 3}, uintptr(healthColor))

	layout := widgetLayoutRects()
	text := widgetTextRects()

	// [模块 1：顶栏 Header]
	pSelectObject.Call(hdc, titleFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "Agent-notify", &text.title, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	subtitle := "4 个 Agent · ClawBot 微信通知"
	if app.isSingleAgentMode() {
		subtitle = app.focusedAgentHint()
	}
	DrawText(hdc, subtitle, &text.subtitle, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	// 右侧按钮：主题切换 (☀️/🌙) + 最小化 + 关闭 (彻底去掉大绿色状态胶囊)
	themeGlyph := "\uE706" // 太阳 (当前为深色，点击切换到白天)
	if !theme.IsDark {
		themeGlyph = "\uE708" // 月亮 (当前为浅色，点击切换到夜晚)
	}
	drawWindowButton(hdc, layout.themeToggle, themeGlyph, app.hover.themeToggle, false, iconFont, theme)
	drawWindowButton(hdc, layout.minimize, "\uE921", app.hover.minimize, false, iconFont, theme)
	drawWindowButton(hdc, layout.close, "\uE8BB", app.hover.close, true, iconFont, theme)

	// [模块 2：核心卡片]
	pSelectObject.Call(hdc, smallFont)
	if app.isSingleAgentMode() {
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		DrawText(hdc, "聚焦代理", &RECT{14, 54, 200, 72}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		toggleColor := uintptr(theme.AccentSuccess)
		if app.hover.modeToggle {
			toggleColor = uintptr(theme.ButtonBorderHover)
		}
		pSetTextColor.Call(hdc, toggleColor)
		DrawText(hdc, "展开全部 4 个 ▾", &layout.modeToggle, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		cards := app.agentCards(layout)
		if len(cards) > 0 {
			drawSingleAgentCard(hdc, cards[0], app, layout, strongFont, baseFont, smallFont, iconFont, theme)
		}
	} else {
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		DrawText(hdc, "全部代理 (4 个)", &RECT{14, 54, 200, 72}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		toggleColor := uintptr(theme.TextSecondary)
		if app.hover.modeToggle {
			toggleColor = uintptr(theme.TextPrimary)
		}
		pSetTextColor.Call(hdc, toggleColor)
		DrawText(hdc, "收起为单个 ▴", &layout.modeToggle, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		for _, card := range app.allAgentCards(layout) {
			drawAgentCard(hdc, card, strongFont, smallFont, theme)
		}
	}

	// [模块 3：信息板块（最近推送卡片）]
	drawRecentCard(hdc, layout.recent, app, strongFont, smallFont, iconFont, theme)

	// [模块 4：快捷操作（四个功能按钮：静音时段、推送历史、系统设置、微信配置）]
	quietActive := strings.TrimSpace(app.quietHours) != ""
	quietLabel := "静音时段"
	if quietActive {
		quietLabel = "勿扰中"
	}
	drawFluentDockButton(hdc, layout.test, "\uE708", quietLabel, app.hover.test, quietActive, theme.AccentWarning, smallFont, iconFont, theme)
	drawFluentDockButton(hdc, layout.history, "\uE81C", "推送历史", app.hover.history, false, 0, smallFont, iconFont, theme)
	drawFluentDockButton(hdc, layout.settings, "\uE713", "系统设置", app.hover.settings, false, 0, smallFont, iconFont, theme)

	wechatActive := !app.clawbotLoggedIn
	wechatLabel := "微信配置"
	if wechatActive {
		wechatLabel = "微信未连"
	}
	drawFluentDockButton(hdc, layout.hide, "\uE8BD", wechatLabel, app.hover.hide, wechatActive, theme.AccentDanger, smallFont, iconFont, theme)

	// [模块 5：底部状态条（版本号 + 升级 + 检查修复）]
	verRect := text.footerVersion
	fillRoundRect(hdc, verRect, 6, uintptr(theme.CardBg))
	strokeRoundRect(hdc, verRect, 6, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)
	pSelectObject.Call(hdc, iconFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	iconRect := RECT{verRect.Left + 4, verRect.Top, verRect.Left + 20, verRect.Bottom}
	DrawText(hdc, "\uE8EC", &iconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
	tagRect := RECT{verRect.Left + 22, verRect.Top, verRect.Right - 2, verRect.Bottom}
	DrawText(hdc, "v"+app.Version(), &tagRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	pSelectObject.Call(hdc, smallFont)
	updateLabel, updatePrimary := app.updateButtonState()
	drawIconTextButton(hdc, layout.update, "\uE895", updateLabel, app.hover.update, updatePrimary, false, smallFont, iconFont, theme)

	repairIssues := app.agentIntegrationIssues() > 0
	repairText := "检查接入"
	if repairIssues {
		repairText = "检查修复"
	}
	drawIconTextButton(hdc, layout.repair, "\uE90F", repairText, app.hover.repair, false, repairIssues, smallFont, iconFont, theme)

	// 自定义 Fluent Agent 下拉卡片浮层
	if app.isSingleAgentMode() && app.agentDropdownOpen {
		drawAgentDropdownOverlay(hdc, app, theme, strongFont, smallFont, iconFont)
	}
}

func (app *WidgetApp) recentStatusColor(theme ThemePalette) uint32 {
	switch app.lastPushStatus {
	case notify.StatusSuccess:
		return theme.AccentSuccess
	case notify.StatusFailed, notify.StatusNotLoggedIn:
		return theme.AccentDanger
	default:
		return theme.TextMuted
	}
}
