//go:build windows

package ui

// widgetDialog 是应用内自绘的模态对话框，用来替代原生 MessageBox，
// 让更新提示/确认与操作报错的风格跟悬浮窗其余部分一致。
type widgetDialog struct {
	visible      bool
	title        string
	message      string
	confirm      bool   // true 显示"取消/确定"；false 只显示"确定"
	onConfirm    func() // 点"确定"时调用
	hoverPrimary bool
	hoverCancel  bool
}

// 对话框几何（逻辑坐标，绘制时由各绘制函数自动缩放到物理像素）。
func dialogBoxRect() RECT     { return RECT{40, 125, 360, 325} }
func dialogTitleRect() RECT   { return RECT{60, 141, 340, 167} }
func dialogMessageRect() RECT { return RECT{60, 173, 340, 271} }
func dialogOKRect() RECT      { return RECT{240, 279, 340, 311} }
func dialogCancelRect() RECT  { return RECT{130, 279, 230, 311} }

// showInfoDialog 弹出只有"确定"的提示。
func (app *WidgetApp) showInfoDialog(hwnd uintptr, title, message string) {
	app.dialog = widgetDialog{visible: true, title: title, message: message}
	app.invalidateDialog(hwnd)
}

// showConfirmDialog 弹出"取消/确定"，点"确定"后执行 onConfirm。
func (app *WidgetApp) showConfirmDialog(hwnd uintptr, title, message string, onConfirm func()) {
	app.dialog = widgetDialog{visible: true, title: title, message: message, confirm: true, onConfirm: onConfirm}
	app.invalidateDialog(hwnd)
}

// dismissDialog 关闭对话框且不触发回调（取消 / Esc）。
func (app *WidgetApp) dismissDialog(hwnd uintptr) {
	app.dialog = widgetDialog{}
	app.invalidateDialog(hwnd)
}

// confirmDialog 关闭对话框并触发 onConfirm（确定 / 回车）。
func (app *WidgetApp) confirmDialog(hwnd uintptr) {
	onConfirm := app.dialog.onConfirm
	app.dialog = widgetDialog{}
	app.invalidateDialog(hwnd)
	if onConfirm != nil {
		onConfirm()
	}
}

func (app *WidgetApp) invalidateDialog(hwnd uintptr) {
	if hwnd == 0 {
		hwnd = app.window()
	}
	if hwnd != 0 {
		pInvalidateRect.Call(hwnd, 0, 0)
	}
}

// handleDialogInput 在对话框可见时拦截鼠标/回车/Esc；其它消息返回 false 交给正常流程。
func (app *WidgetApp) handleDialogInput(hwnd uintptr, message uint32, wParam, lParam uintptr) bool {
	switch message {
	case WM_MOUSEMOVE:
		x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
		primary := pointInRect(x, y, dialogOKRect())
		cancel := app.dialog.confirm && pointInRect(x, y, dialogCancelRect())
		if primary != app.dialog.hoverPrimary || cancel != app.dialog.hoverCancel {
			app.dialog.hoverPrimary = primary
			app.dialog.hoverCancel = cancel
			pInvalidateRect.Call(hwnd, 0, 0)
		}
		if primary || cancel {
			hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
			pSetCursor.Call(hand)
		}
		return true
	case WM_LBUTTONDOWN:
		x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
		switch {
		case pointInRect(x, y, dialogOKRect()):
			app.confirmDialog(hwnd)
		case app.dialog.confirm && pointInRect(x, y, dialogCancelRect()):
			app.dismissDialog(hwnd)
		}
		return true
	case WM_KEYDOWN:
		switch wParam {
		case VK_RETURN:
			app.confirmDialog(hwnd)
		case VK_ESCAPE:
			app.dismissDialog(hwnd)
		}
		return true
	}
	return false
}

// drawAppDialog 在最上层绘制对话框：不透明底 + 卡片 + 标题/正文/按钮。
func drawAppDialog(hdc uintptr, app *WidgetApp) {
	theme := app.getTheme()
	fonts := app.uiFonts()

	// 没有 alpha 混合能力，这里用背景色铺满作为模态底，遮住下层视图。
	fillRectLogical(hdc, RECT{0, 0, widgetWidth, widgetHeight}, uintptr(theme.Background))

	box := dialogBoxRect()
	fillRoundRect(hdc, box, 10, uintptr(theme.CardBg))
	strokeRoundRect(hdc, box, 10, uintptr(theme.CardBg), uintptr(theme.CardBorderHover), 1)

	pSelectObject.Call(hdc, fonts.strong)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	titleRect := dialogTitleRect()
	DrawText(hdc, app.dialog.title, &titleRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	pSelectObject.Call(hdc, fonts.small)
	pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
	messageRect := dialogMessageRect()
	DrawText(hdc, app.dialog.message, &messageRect, DT_LEFT|DT_WORDBREAK|DT_NOPREFIX)

	drawIconTextButton(hdc, dialogOKRect(), "", "确定", app.dialog.hoverPrimary, true, false, fonts.small, fonts.icon, theme)
	if app.dialog.confirm {
		drawIconTextButton(hdc, dialogCancelRect(), "", "取消", app.dialog.hoverCancel, false, false, fonts.small, fonts.icon, theme)
	}
}
