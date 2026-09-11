//go:build windows

package ui

import (
	"fmt"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"unsafe"

	"linkweixin/internal/config"
)

const (
	BS_AUTOCHECKBOX = 0x00000003
	ES_AUTOHSCROLL  = 0x0080
	ES_PASSWORD     = 0x0020
	WS_BORDER       = 0x00800000

	BM_GETCHECK = 0x00F0
	BM_SETCHECK = 0x00F1
	BST_CHECKED = 1

	WM_SETFONT = 0x0030
	WM_SETTEXT = 0x000C
	WM_GETTEXT = 0x000D
)

func getWindowText(hwnd uintptr) string {
	buf := make([]uint16, 1024)
	pSendMessageW.Call(hwnd, WM_GETTEXT, 1024, uintptr(unsafe.Pointer(&buf[0])))
	return syscall.UTF16ToString(buf)
}

func setWindowText(hwnd uintptr, text string) {
	pSendMessageW.Call(hwnd, WM_SETTEXT, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr(text))))
}

func isChecked(hwnd uintptr) bool {
	res, _, _ := pSendMessageW.Call(hwnd, BM_GETCHECK, 0, 0)
	return res == BST_CHECKED
}

func setChecked(hwnd uintptr, checked bool) {
	val := uintptr(0)
	if checked {
		val = BST_CHECKED
	}
	pSendMessageW.Call(hwnd, BM_SETCHECK, val, 0)
}

// ShowSettingsDialog presents the channel & preference configuration dialog.
func ShowSettingsDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("LinkWeixinSettingsDialog")

	curCfg := config.LoadConfig("")

	var dlgHwnd uintptr
	var chkPp, txtPp, chkWx, txtWx, chkFs, txtFs, chkDd, txtDd, chkCu, txtCu uintptr
	var txtQuiet, txtCd uintptr

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_CREATE:
			hFont, _, _ := pCreateFontW.Call(15, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))

			// PushPlus
			chkPp, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("BUTTON"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("PushPlus (微信服务号)"))), WS_CHILD|WS_VISIBLE|BS_AUTOCHECKBOX, 24, 45, 180, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(chkPp, WM_SETFONT, hFont, 1)
			setChecked(chkPp, curCfg.Channels.PushPlus.Enabled)

			txtPp, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 45, 280, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtPp, WM_SETFONT, hFont, 1)
			setWindowText(txtPp, curCfg.Channels.PushPlus.Token)

			// WeCom
			chkWx, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("BUTTON"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("企业微信群机器人"))), WS_CHILD|WS_VISIBLE|BS_AUTOCHECKBOX, 24, 80, 180, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(chkWx, WM_SETFONT, hFont, 1)
			setChecked(chkWx, curCfg.Channels.WeCom.Enabled)

			txtWx, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 80, 280, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtWx, WM_SETFONT, hFont, 1)
			setWindowText(txtWx, curCfg.Channels.WeCom.Webhook)

			// Feishu
			chkFs, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("BUTTON"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("飞书群机器人"))), WS_CHILD|WS_VISIBLE|BS_AUTOCHECKBOX, 24, 115, 180, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(chkFs, WM_SETFONT, hFont, 1)
			setChecked(chkFs, curCfg.Channels.Feishu.Enabled)

			txtFs, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 115, 280, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtFs, WM_SETFONT, hFont, 1)
			setWindowText(txtFs, curCfg.Channels.Feishu.Webhook)

			// DingTalk
			chkDd, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("BUTTON"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("钉钉群机器人"))), WS_CHILD|WS_VISIBLE|BS_AUTOCHECKBOX, 24, 150, 180, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(chkDd, WM_SETFONT, hFont, 1)
			setChecked(chkDd, curCfg.Channels.DingTalk.Enabled)

			txtDd, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 150, 280, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtDd, WM_SETFONT, hFont, 1)
			setWindowText(txtDd, curCfg.Channels.DingTalk.Webhook)

			// Custom
			chkCu, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("BUTTON"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("自定义 Webhook"))), WS_CHILD|WS_VISIBLE|BS_AUTOCHECKBOX, 24, 185, 180, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(chkCu, WM_SETFONT, hFont, 1)
			setChecked(chkCu, curCfg.Channels.Custom.Enabled)

			txtCu, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 185, 280, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtCu, WM_SETFONT, hFont, 1)
			setWindowText(txtCu, curCfg.Channels.Custom.Webhook)

			// Quiet hours
			txtQuiet, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 260, 100, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtQuiet, WM_SETFONT, hFont, 1)
			setWindowText(txtQuiet, curCfg.QuietHours)

			// Cooldown
			txtCd, _, _ = pCreateWindowExW.Call(0, uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))), 0, WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL, 210, 295, 100, 24, hwnd, 0, hInstance, 0)
			pSendMessageW.Call(txtCd, WM_SETFONT, hFont, 1)
			setWindowText(txtCd, fmt.Sprintf("%d", curCfg.CooldownMin))
			return 0

		case WM_PAINT:
			var ps PAINTSTRUCT
			hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps)))

			var rc RECT
			pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rc)))

			hbrBg, _, _ := pCreateSolidBrush.Call(uintptr(RGB(24, 24, 27)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rc)), hbrBg)
			pDeleteObject.Call(hbrBg)

			pSetBkMode.Call(hdc, TRANSPARENT)

			hFontBold, _, _ := pCreateFontW.Call(16, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
			hOldFont, _, _ := pSelectObject.Call(hdc, hFontBold)
			pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))

			// Group 1 Header
			rcG1 := RECT{20, 16, 400, 38}
			DrawText(hdc, "推送通道配置（支持多通道联动）", &rcG1, DT_SINGLELINE|DT_VCENTER)

			// Group 2 Header
			rcG2 := RECT{20, 230, 400, 252}
			DrawText(hdc, "偏好与免打扰", &rcG2, DT_SINGLELINE|DT_VCENTER)

			hFontBase, _, _ := pCreateFontW.Call(14, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
			pSelectObject.Call(hdc, hFontBase)
			pSetTextColor.Call(hdc, uintptr(RGB(161, 161, 170)))

			rcQuietLbl := RECT{24, 262, 200, 284}
			DrawText(hdc, "勿扰时段 (起-止小时)：", &rcQuietLbl, DT_SINGLELINE|DT_VCENTER)

			rcQuietExp := RECT{320, 262, 500, 284}
			DrawText(hdc, "例: 23-8 表示夜间静默", &rcQuietExp, DT_SINGLELINE|DT_VCENTER)

			rcCdLbl := RECT{24, 297, 200, 319}
			DrawText(hdc, "会话防刷冷却 (分钟)：", &rcCdLbl, DT_SINGLELINE|DT_VCENTER)

			// Diagnosis group
			rcDiag := RECT{20, 340, 500, 400}
			pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))
			pSelectObject.Call(hdc, hFontBold)
			DrawText(hdc, "系统健康与自愈诊断", &rcDiag, DT_SINGLELINE)

			pSelectObject.Call(hdc, hFontBase)
			pSetTextColor.Call(hdc, uintptr(RGB(16, 185, 129)))
			rcDiag1 := RECT{24, 370, 500, 390}
			DrawText(hdc, "• 原生 Go 独立内核：运行正常 (零外部依赖、微秒响应)", &rcDiag1, DT_SINGLELINE)
			rcDiag2 := RECT{24, 395, 500, 415}
			DrawText(hdc, "• 多端统一渲染：支持 Markdown 摘要排版与 5 通道投递", &rcDiag2, DT_SINGLELINE)

			// Buttons Save & Cancel
			rcSave := RECT{300, 440, 396, 472}
			hbrGreen, _, _ := pCreateSolidBrush.Call(uintptr(RGB(16, 185, 129)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcSave)), hbrGreen)
			pDeleteObject.Call(hbrGreen)
			pSetTextColor.Call(hdc, uintptr(RGB(255, 255, 255)))
			DrawText(hdc, "保存配置", &rcSave, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

			rcCancel := RECT{406, 440, 496, 472}
			hbrCard, _, _ := pCreateSolidBrush.Call(uintptr(RGB(39, 39, 44)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcCancel)), hbrCard)
			pDeleteObject.Call(hbrCard)
			pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))
			DrawText(hdc, "取消", &rcCancel, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

			pSelectObject.Call(hdc, hOldFont)
			pDeleteObject.Call(hFontBold)
			pDeleteObject.Call(hFontBase)

			pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps)))
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)

			// Save button (300, 440, 396, 472)
			if x >= 300 && x <= 396 && y >= 440 && y <= 472 {
				cooldown := 10
				if cdVal, err := strconv.Atoi(strings.TrimSpace(getWindowText(txtCd))); err == nil && cdVal > 0 {
					cooldown = cdVal
				}

				newCfg := config.AppConfig{
					Channels: config.ChannelsConfig{
						PushPlus: config.ChannelPushPlus{
							Enabled: isChecked(chkPp),
							Token:   strings.TrimSpace(getWindowText(txtPp)),
						},
						WeCom: config.ChannelWebhook{
							Enabled: isChecked(chkWx),
							Webhook: strings.TrimSpace(getWindowText(txtWx)),
						},
						Feishu: config.ChannelWebhook{
							Enabled: isChecked(chkFs),
							Webhook: strings.TrimSpace(getWindowText(txtFs)),
						},
						DingTalk: config.ChannelWebhook{
							Enabled: isChecked(chkDd),
							Webhook: strings.TrimSpace(getWindowText(txtDd)),
						},
						Custom: config.ChannelWebhook{
							Enabled: isChecked(chkCu),
							Webhook: strings.TrimSpace(getWindowText(txtCu)),
						},
					},
					QuietHours:  strings.TrimSpace(getWindowText(txtQuiet)),
					CooldownMin: cooldown,
				}
				_ = config.SaveConfig(newCfg, "")
				pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("配置已保存！"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin"))), MB_OK|MB_ICONINFO)
				pDestroyWindow.Call(hwnd)
			}

			// Cancel button (406, 440, 496, 472)
			if x >= 406 && x <= 496 && y >= 440 && y <= 472 {
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

	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	dlgWidth := int32(520)
	dlgHeight := int32(510)
	dlgX := (int32(screenWidth) - dlgWidth) / 2
	dlgY := (int32(screenHeight) - dlgHeight) / 2

	dlgHwnd, _, _ = pCreateWindowExW.Call(
		WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin - 通道配置与诊断中心"))),
		WS_POPUP|WS_SYSMENU|WS_VISIBLE,
		uintptr(dlgX), uintptr(dlgY), uintptr(dlgWidth), uintptr(dlgHeight),
		parentHwnd, 0, hInstance, 0,
	)

	cornerPref := uint32(2)
	pDwmSetWindowAttribute.Call(dlgHwnd, 33, uintptr(unsafe.Pointer(&cornerPref)), 4)
	darkMode := uint32(1)
	pDwmSetWindowAttribute.Call(dlgHwnd, 20, uintptr(unsafe.Pointer(&darkMode)), 4)

	pShowWindow.Call(dlgHwnd, SW_SHOW)
	pUpdateWindow.Call(dlgHwnd)

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
