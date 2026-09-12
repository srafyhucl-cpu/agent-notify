//go:build windows

package ui

import (
	"fmt"
	"strings"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func newIconFont(size int32) uintptr {
	font, _, _ := pCreateFontW.Call(
		uintptr(scaleFloat(size)), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0,
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Segoe Fluent Icons"))),
	)
	return font
}

func drawIconTextButton(hdc uintptr, rect RECT, glyph, label string, hover, primary, danger bool, baseFont, iconFont uintptr) {
	fillColor := uintptr(RGB(25, 31, 37))
	textColor := uintptr(RGB(225, 231, 235))
	borderColor := uintptr(RGB(43, 52, 61))
	if primary {
		fillColor = uintptr(RGB(39, 139, 112))
		textColor = uintptr(RGB(245, 252, 250))
		borderColor = uintptr(RGB(56, 177, 143))
	}
	if danger {
		textColor = uintptr(RGB(239, 126, 126))
	}
	if hover {
		fillColor = uintptr(RGB(35, 43, 51))
		if primary {
			fillColor = uintptr(RGB(46, 158, 127))
		}
		if danger {
			fillColor = uintptr(RGB(58, 34, 38))
		}
	}
	fillRoundRect(hdc, rect, 7, fillColor)
	strokeRoundRect(hdc, rect, 7, fillColor, borderColor, 1)

	iconRect := rect
	iconRect.Right = iconRect.Left + 30
	pSelectObject.Call(hdc, iconFont)
	pSetTextColor.Call(hdc, textColor)
	DrawText(hdc, glyph, &iconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	textRect := rect
	textRect.Left += 24
	pSelectObject.Call(hdc, baseFont)
	DrawText(hdc, label, &textRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
}

func drawWindowButton(hdc uintptr, rect RECT, glyph string, hover, danger bool, iconFont uintptr) {
	fillColor := uintptr(RGB(16, 20, 24))
	if hover {
		fillColor = uintptr(RGB(34, 41, 48))
		if danger {
			fillColor = uintptr(RGB(122, 42, 49))
		}
	}
	fillRoundRect(hdc, rect, 6, fillColor)
	pSelectObject.Call(hdc, iconFont)
	if danger && hover {
		pSetTextColor.Call(hdc, uintptr(RGB(255, 245, 245)))
	} else {
		pSetTextColor.Call(hdc, uintptr(RGB(150, 160, 170)))
	}
	DrawText(hdc, glyph, &rect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
}

func drawAgentCard(hdc uintptr, rect RECT, name string, enabled, running, hover bool, baseFont, smallFont uintptr) {
	fillColor := uintptr(RGB(23, 29, 35))
	borderColor := uintptr(RGB(41, 50, 59))
	if hover {
		fillColor = uintptr(RGB(29, 37, 44))
		borderColor = uintptr(RGB(56, 68, 80))
	}
	fillRoundRect(hdc, rect, 8, fillColor)
	strokeRoundRect(hdc, rect, 8, fillColor, borderColor, 1)

	stateColor := uintptr(RGB(112, 124, 135))
	stateText := "已暂停"
	if enabled {
		stateColor = uintptr(RGB(56, 194, 151))
		stateText = "监听中"
	}
	badge := RECT{rect.Left + 14, rect.Top + 14, rect.Left + 44, rect.Top + 44}
	fillRoundRect(hdc, badge, 7, uintptr(RGB(31, 40, 48)))
	drawEllipseLogical(hdc, badge.Left+13, badge.Top+13, badge.Left+21, badge.Top+21, stateColor, stateColor)

	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(RGB(238, 242, 245)))
	DrawText(hdc, name, &RECT{rect.Left + 54, rect.Top + 10, rect.Right - 12, rect.Top + 34}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, stateColor)
	DrawText(hdc, stateText, &RECT{rect.Left + 54, rect.Top + 34, rect.Right - 12, rect.Top + 54}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	processText := "进程未运行"
	if running {
		processText = "进程运行中"
	}
	pSetTextColor.Call(hdc, uintptr(RGB(126, 137, 148)))
	DrawText(hdc, processText, &RECT{rect.Left + 14, rect.Top + 53, rect.Right - 52, rect.Bottom - 5}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	track := RECT{rect.Right - 48, rect.Bottom - 25, rect.Right - 12, rect.Bottom - 9}
	trackColor := uintptr(RGB(62, 72, 82))
	if enabled {
		trackColor = uintptr(RGB(39, 143, 113))
	}
	fillRoundRect(hdc, track, 8, trackColor)
	knobX := track.Left + 3
	if enabled {
		knobX = track.Right - 19
	}
	drawEllipseLogical(hdc, knobX, track.Top+3, knobX+16, track.Bottom-3, uintptr(RGB(244, 249, 248)), uintptr(RGB(244, 249, 248)))
}

func drawPill(hdc uintptr, rect RECT, text string, color uint32, font uintptr) {
	fillRoundRect(hdc, rect, 9, uintptr(color))
	pSelectObject.Call(hdc, font)
	pSetTextColor.Call(hdc, uintptr(RGB(245, 248, 248)))
	DrawText(hdc, text, &rect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
}

// connectionText returns the headline and supporting line for the ClawBot card.
func (app *WidgetApp) connectionText() (string, string, uint32) {
	switch {
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

func drawUI(hdc uintptr, width, height int32, app *WidgetApp) {
	background := RECT{0, 0, width, height}
	backgroundBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(15, 19, 23)))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&background)), backgroundBrush)
	pDeleteObject.Call(backgroundBrush)
	pSetBkMode.Call(hdc, TRANSPARENT)

	healthColor, healthText := app.health()
	fillRectLogical(hdc, RECT{0, 0, widgetWidth, 3}, uintptr(healthColor))

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

	layout := widgetLayoutRects()
	text := widgetTextRects()
	pSelectObject.Call(hdc, titleFont)
	pSetTextColor.Call(hdc, uintptr(RGB(242, 246, 247)))
	DrawText(hdc, "Agent-notify", &text.title, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(RGB(131, 143, 154)))
	DrawText(hdc, "OpenCode + Codex  ·  ClawBot 微信通知", &text.subtitle, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	drawPill(hdc, RECT{244, 10, 338, 34}, healthText, healthColor, smallFont)
	drawWindowButton(hdc, layout.minimize, "\uE921", app.hoverMin, false, iconFont)
	drawWindowButton(hdc, layout.close, "\uE8BB", app.hoverClose, true, iconFont)

	connectionFill := uintptr(RGB(20, 26, 31))
	connectionBorder := uintptr(RGB(39, 49, 58))
	if app.hoverConnection {
		connectionFill = uintptr(RGB(27, 35, 42))
	}
	fillRoundRect(hdc, layout.connection, 8, connectionFill)
	strokeRoundRect(hdc, layout.connection, 8, connectionFill, connectionBorder, 1)
	connectionTitle, connectionDetail, connectionColor := app.connectionText()
	drawEllipseLogical(hdc, 28, 75, 37, 84, uintptr(connectionColor), uintptr(connectionColor))
	pSelectObject.Call(hdc, strongFont)
	pSetTextColor.Call(hdc, uintptr(RGB(232, 237, 240)))
	DrawText(hdc, connectionTitle, &text.connectionTitle, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(RGB(135, 147, 158)))
	DrawText(hdc, connectionDetail, &text.connectionDetail, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)

	quietText := "勿扰关闭"
	if strings.TrimSpace(app.quietHours) != "" {
		quietText = "勿扰 " + app.quietHours
	}
	pSetTextColor.Call(hdc, uintptr(RGB(255, 225, 163)))
	DrawText(hdc, quietText, &text.quiet, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(RGB(119, 131, 142)))
	DrawText(hdc, "通知代理", &RECT{15, 119, 150, 134}, DT_SINGLELINE|DT_NOPREFIX)
	drawAgentCard(hdc, layout.openCode, "OpenCode", app.onOpenCode, app.procStatus.OpenCodeRunning, app.hoverOpenCode, strongFont, smallFont)
	drawAgentCard(hdc, layout.codex, "Codex", app.onCodex, app.procStatus.CodexRunning, app.hoverCodex, strongFont, smallFont)

	recentFill := uintptr(RGB(22, 28, 34))
	border := uintptr(RGB(41, 50, 59))
	if app.hoverRecent {
		recentFill = uintptr(RGB(29, 37, 44))
	}
	fillRoundRect(hdc, layout.recent, 8, recentFill)
	strokeRoundRect(hdc, layout.recent, 8, recentFill, border, 1)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(RGB(126, 138, 149)))
	DrawText(hdc, "最近推送", &text.recentLabel, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
	pSelectObject.Call(hdc, strongFont)
	pSetTextColor.Call(hdc, uintptr(RGB(230, 235, 238)))
	DrawText(hdc, app.lastPushTitle, &text.recentTitle, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(RGB(134, 146, 157)))
	recentMeta := app.lastPushText
	if status := app.recentStatusText(); status != "" {
		recentMeta = status + " · " + recentMeta
	}
	pSetTextColor.Call(hdc, uintptr(app.recentStatusColor()))
	DrawText(hdc, recentMeta, &text.recentMeta, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)

	drawIconTextButton(hdc, layout.test, "\uE724", "发送测试", app.hoverTest, true, false, baseFont, iconFont)
	drawIconTextButton(hdc, layout.settings, "\uE713", "设置", app.hoverSettings, false, false, baseFont, iconFont)
	drawIconTextButton(hdc, layout.history, "\uE81C", "历史", app.hoverHistory, false, false, baseFont, iconFont)
	drawIconTextButton(hdc, layout.hide, "\uE8A7", "隐藏", app.hoverHide, false, false, baseFont, iconFont)

	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(RGB(105, 117, 128)))
	DrawText(hdc, "v"+app.Version(), &text.footerVersion, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
	DrawText(hdc, "右键托盘图标可退出", &text.footerHint, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
}

func (app *WidgetApp) recentStatusColor() uint32 {
	switch app.lastPushStatus {
	case notify.StatusSuccess:
		return RGB(55, 190, 147)
	case notify.StatusFailed, notify.StatusNotLoggedIn:
		return RGB(224, 104, 104)
	default:
		return RGB(126, 138, 149)
	}
}

func (app *WidgetApp) recentStatusText() string {
	if app.lastPushStatus == "" {
		return ""
	}
	label := app.lastPushStatus
	if app.lastPushAgent != "" {
		label = fmt.Sprintf("%s · %s", historyAgent(notify.HistoryItem{Agent: app.lastPushAgent}), label)
	}
	return label
}
