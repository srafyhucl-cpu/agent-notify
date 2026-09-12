//go:build windows

package ui

import (
	"fmt"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	ES_AUTOHSCROLL  = 0x0080
	ES_NUMBER       = 0x2000
	WM_SETFONT      = 0x0030
	WM_SETTEXT      = 0x000C
	WM_GETTEXT      = 0x000D
	WM_CTLCOLOREDIT = 0x0133

	VK_ESCAPE = 0x001B

	IDC_SETTINGS_QUIET    = 4001
	IDC_SETTINGS_COOLDOWN = 4002
	settingsTimer         = 1
)

const (
	settingsWidth  = int32(520)
	settingsHeight = int32(390)
)

func getWindowText(hwnd uintptr) string {
	buffer := make([]uint16, 1024)
	pSendMessageW.Call(hwnd, WM_GETTEXT, 1024, uintptr(unsafe.Pointer(&buffer[0])))
	return syscall.UTF16ToString(buffer)
}

func setWindowText(hwnd uintptr, text string) {
	pSendMessageW.Call(hwnd, WM_SETTEXT, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr(text))))
}

type settingsLayout struct {
	close    RECT
	login    RECT
	logout   RECT
	quiet    RECT
	cooldown RECT
	cancel   RECT
	save     RECT
}

func settingsLayoutRects() settingsLayout {
	return settingsLayout{
		close:    RECT{482, 8, 512, 38},
		login:    RECT{326, 94, 496, 130},
		logout:   RECT{326, 138, 496, 174},
		quiet:    RECT{184, 230, 366, 262},
		cooldown: RECT{184, 282, 366, 314},
		cancel:   RECT{310, 340, 398, 374},
		save:     RECT{410, 340, 498, 374},
	}
}

func placeEdit(ctrl uintptr, rect RECT) {
	inner := scaleRect(RECT{Left: rect.Left + 10, Top: rect.Top + 6, Right: rect.Right - 10, Bottom: rect.Bottom - 6})
	pSetWindowPos.Call(
		ctrl,
		0,
		uintptr(inner.Left),
		uintptr(inner.Top),
		uintptr(inner.Right-inner.Left),
		uintptr(inner.Bottom-inner.Top),
		SWP_NOZORDER|SWP_NOACTIVATE,
	)
}

func settingsConnectionState(status clawbot.Status) (string, string, string, uint32, string) {
	loginLabel := "扫码登录"
	if status.LoggedIn {
		loginLabel = "重新登录"
	}

	switch {
	case !status.LoggedIn:
		return loginLabel, "ClawBot 未连接", "扫码登录后，还需发送一条微信消息", RGB(224, 104, 104), "凭据仅保存在本机，不上传第三方。"
	case status.Stale:
		return loginLabel, "ClawBot 登录已失效", "请重新扫码登录", RGB(224, 104, 104), "不会复用已经失效的登录或会话。"
	case !status.SessionReady:
		return loginLabel, "已登录，等待微信消息", "请给 ClawBot 发送一条消息建立会话", RGB(224, 165, 70), "也可运行 agent-notify sync。"
	default:
		detail := "主动推送会话已就绪"
		if status.UserHint != "" {
			detail += " · " + status.UserHint
		}
		return loginLabel, "ClawBot 已连接", detail, RGB(55, 190, 147), "凭据仅保存在本机，不上传第三方。"
	}
}

func ShowSettingsDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	parentDPI := windowDPI(parentHwnd)
	setUIDPI(parentDPI)
	defer setUIDPI(parentDPI)

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("AgentNotifySettingsDialog")
	cfg, _ := config.LoadConfig("")

	var dialog uintptr
	var quietEdit, cooldownEdit, editFont, backgroundBrush uintptr
	var tracking bool
	var hoverClose, hoverLogin, hoverLogout, hoverCancel, hoverSave bool

	layout := settingsLayoutRects()

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_CREATE:
			setUIDPI(windowDPI(hwnd))
			backgroundBrush, _, _ = pCreateSolidBrush.Call(uintptr(RGB(15, 19, 23)))
			editFont = newFont(14, 400)

			quietEdit, _, _ = pCreateWindowExW.Call(
				0,
				uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
				0,
				WS_CHILD|WS_VISIBLE|ES_AUTOHSCROLL,
				uintptr(scaleFloat(layout.quiet.Left+10)),
				uintptr(scaleFloat(layout.quiet.Top+6)),
				uintptr(scaleFloat(layout.quiet.Right-layout.quiet.Left-20)),
				uintptr(scaleFloat(layout.quiet.Bottom-layout.quiet.Top-12)),
				hwnd, IDC_SETTINGS_QUIET, hInstance, 0,
			)
			cooldownEdit, _, _ = pCreateWindowExW.Call(
				0,
				uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
				0,
				WS_CHILD|WS_VISIBLE|ES_AUTOHSCROLL|ES_NUMBER,
				uintptr(scaleFloat(layout.cooldown.Left+10)),
				uintptr(scaleFloat(layout.cooldown.Top+6)),
				uintptr(scaleFloat(layout.cooldown.Right-layout.cooldown.Left-20)),
				uintptr(scaleFloat(layout.cooldown.Bottom-layout.cooldown.Top-12)),
				hwnd, IDC_SETTINGS_COOLDOWN, hInstance, 0,
			)
			pSendMessageW.Call(quietEdit, WM_SETFONT, editFont, 1)
			pSendMessageW.Call(cooldownEdit, WM_SETFONT, editFont, 1)
			setWindowText(quietEdit, cfg.QuietHours)
			setWindowText(cooldownEdit, fmt.Sprintf("%d", cfg.CooldownMin))
			pSetTimer.Call(hwnd, settingsTimer, 1000, 0)
			return 0

		case WM_DPICHANGED:
			setUIDPI(uint32(wParam & 0xFFFF))
			resizeForCurrentDPI(hwnd, settingsWidth, settingsHeight)
			if editFont != 0 {
				pDeleteObject.Call(editFont)
			}
			editFont = newFont(14, 400)
			pSendMessageW.Call(quietEdit, WM_SETFONT, editFont, 1)
			pSendMessageW.Call(cooldownEdit, WM_SETFONT, editFont, 1)
			placeEdit(quietEdit, layout.quiet)
			placeEdit(cooldownEdit, layout.cooldown)
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_TIMER:
			if wParam == settingsTimer {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			return 0

		case WM_ERASEBKGND:
			return 1

		case WM_PAINT:
			paintDoubleBuffered(hwnd, func(hdc uintptr, width, height int32) {
				fillRectLogical(hdc, RECT{0, 0, settingsWidth, settingsHeight}, uintptr(RGB(15, 19, 23)))
				pSetBkMode.Call(hdc, TRANSPARENT)

				titleFont := newFont(20, 700)
				baseFont := newFont(14, 400)
				strongFont := newFont(14, 700)
				smallFont := newFont(12, 400)
				iconFont := newIconFont(16)
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
				DrawText(hdc, "Agent-notify 设置", &RECT{20, 12, 320, 42}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(133, 145, 156)))
				DrawText(hdc, "微信通知与去重策略", &RECT{20, 40, 320, 62}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

				drawCard(hdc, RECT{16, 74, 504, 190}, uintptr(RGB(22, 28, 34)), uintptr(RGB(41, 50, 59)))
				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(126, 138, 149)))
				DrawText(hdc, "微信推送通道", &RECT{32, 82, 280, 100}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

				status := clawbot.GetStatus()
				loginLabel, statusTitle, statusDetail, statusColor, statusFootnote := settingsConnectionState(status)
				drawEllipseLogical(hdc, 34, 113, 43, 122, uintptr(statusColor), uintptr(statusColor))
				pSelectObject.Call(hdc, strongFont)
				pSetTextColor.Call(hdc, uintptr(RGB(232, 237, 240)))
				DrawText(hdc, statusTitle, &RECT{52, 106, 312, 128}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(137, 149, 160)))
				DrawText(hdc, statusDetail, &RECT{52, 128, 312, 148}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)
				DrawText(hdc, statusFootnote, &RECT{52, 152, 312, 172}, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)

				logoutLabel := "尚未登录"
				logoutDanger := false
				if status.LoggedIn {
					logoutLabel = "退出登录"
					logoutDanger = true
				}
				drawIconTextButton(hdc, layout.login, "\uE72C", loginLabel, hoverLogin, !status.LoggedIn, false, baseFont, iconFont)
				drawIconTextButton(hdc, layout.logout, "\uE7E8", logoutLabel, hoverLogout && status.LoggedIn, false, logoutDanger, baseFont, iconFont)

				drawCard(hdc, RECT{16, 202, 504, 332}, uintptr(RGB(22, 28, 34)), uintptr(RGB(41, 50, 59)))
				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(126, 138, 149)))
				DrawText(hdc, "通知策略", &RECT{32, 210, 280, 228}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

				pSelectObject.Call(hdc, baseFont)
				pSetTextColor.Call(hdc, uintptr(RGB(232, 237, 240)))
				DrawText(hdc, "勿扰时段", &RECT{32, 234, 176, 262}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				DrawText(hdc, "会话冷却（分钟）", &RECT{32, 286, 176, 314}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

				drawFieldFrame(hdc, layout.quiet)
				drawFieldFrame(hdc, layout.cooldown)

				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(133, 145, 156)))
				DrawText(hdc, "留空表示关闭，格式 23-8", &RECT{374, 232, 496, 260}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				DrawText(hdc, "同一会话去重，默认 10", &RECT{374, 284, 496, 312}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

				drawIconTextButton(hdc, layout.cancel, "\uE711", "取消", hoverCancel, false, false, baseFont, iconFont)
				drawIconTextButton(hdc, layout.save, "\uE74E", "保存", hoverSave, true, false, baseFont, iconFont)
				drawWindowButton(hdc, layout.close, "\uE8BB", hoverClose, true, iconFont)
			})
			return 0

		case WM_CTLCOLOREDIT:
			pSetTextColor.Call(wParam, uintptr(RGB(235, 239, 242)))
			pSetBkColor.Call(wParam, uintptr(RGB(16, 20, 24)))
			return backgroundBrush

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
			previous := [...]bool{hoverClose, hoverLogin, hoverLogout, hoverCancel, hoverSave}
			hoverClose = pointInRect(x, y, layout.close)
			hoverLogin = pointInRect(x, y, layout.login)
			hoverLogout = pointInRect(x, y, layout.logout)
			hoverCancel = pointInRect(x, y, layout.cancel)
			hoverSave = pointInRect(x, y, layout.save)
			current := [...]bool{hoverClose, hoverLogin, hoverLogout, hoverCancel, hoverSave}
			changed := false
			for i := range current {
				if current[i] != previous[i] {
					changed = true
				}
			}
			if changed {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if hoverClose || hoverLogin || hoverLogout || hoverCancel || hoverSave {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WM_MOUSELEAVE:
			tracking = false
			hoverClose, hoverLogin, hoverLogout, hoverCancel, hoverSave = false, false, false, false, false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
			switch {
			case pointInRect(x, y, layout.close), pointInRect(x, y, layout.cancel):
				pDestroyWindow.Call(hwnd)
			case pointInRect(x, y, layout.save):
				quiet := strings.TrimSpace(getWindowText(quietEdit))
				if !config.ValidQuietHours(quiet) {
					showMessage(hwnd, "勿扰时段格式无效。留空表示关闭，或使用 23-8 形式。", MB_ICONINFO)
					return 0
				}
				cooldown := config.DefaultCooldownMin
				if parsed, err := strconv.Atoi(strings.TrimSpace(getWindowText(cooldownEdit))); err == nil && parsed > 0 {
					cooldown = parsed
				}
				if err := config.SaveConfig(config.AppConfig{QuietHours: quiet, CooldownMin: cooldown}, ""); err != nil {
					showMessage(hwnd, "保存失败："+err.Error(), MB_ICONINFO)
					return 0
				}
				pDestroyWindow.Call(hwnd)
			case pointInRect(x, y, layout.login):
				ShowLoginDialog(hwnd)
				pInvalidateRect.Call(hwnd, 0, 0)
			case pointInRect(x, y, layout.logout):
				if !clawbot.GetStatus().LoggedIn {
					return 0
				}
				if showConfirm(hwnd, "确定退出当前 ClawBot 登录吗？") {
					_ = clawbot.DeleteCredentials()
					showMessage(hwnd, "已退出 ClawBot 登录。", MB_ICONINFO)
					pInvalidateRect.Call(hwnd, 0, 1)
				}
			}
			return 0

		case WM_KEYDOWN:
			if wParam == VK_ESCAPE {
				pDestroyWindow.Call(hwnd)
			}
			return 0

		case WM_CLOSE:
			pDestroyWindow.Call(hwnd)
			return 0

		case WM_DESTROY:
			pKillTimer.Call(hwnd, settingsTimer)
			if editFont != 0 {
				pDeleteObject.Call(editFont)
				editFont = 0
			}
			if backgroundBrush != 0 {
				pDeleteObject.Call(backgroundBrush)
				backgroundBrush = 0
			}
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
		debugLog("RegisterClassExW settings failed: %v", registerErr)
		return
	}
	defer pUnregisterClassW.Call(uintptr(unsafe.Pointer(className)), hInstance)

	width, height := logicalSize(settingsWidth, settingsHeight)
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	x := (int32(screenWidth) - width) / 2
	y := (int32(screenHeight) - height) / 2
	dialog, _, _ = pCreateWindowExW.Call(
		WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify 设置"))),
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
