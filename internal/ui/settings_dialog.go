//go:build windows

package ui

import (
	"fmt"
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	BS_AUTOCHECKBOX = 0x00000003
	ES_AUTOHSCROLL  = 0x0080
	ES_NUMBER       = 0x2000
	WS_BORDER       = 0x00800000

	BM_GETCHECK = 0x00F0
	BM_SETCHECK = 0x00F1
	BST_CHECKED = 1

	WM_SETFONT        = 0x0030
	WM_SETTEXT        = 0x000C
	WM_GETTEXT        = 0x000D
	WM_CTLCOLOREDIT   = 0x0133
	WM_CTLCOLORSTATIC = 0x0138

	IDC_SETTINGS_QUIET    = 4001
	IDC_SETTINGS_COOLDOWN = 4002
	IDC_SETTINGS_SAVE     = 4003
	IDC_SETTINGS_CANCEL   = 4004
	IDC_SETTINGS_LOGIN    = 4005
	IDC_SETTINGS_LOGOUT   = 4006
)

func getWindowText(hwnd uintptr) string {
	buffer := make([]uint16, 1024)
	pSendMessageW.Call(hwnd, WM_GETTEXT, 1024, uintptr(unsafe.Pointer(&buffer[0])))
	return syscall.UTF16ToString(buffer)
}

func setWindowText(hwnd uintptr, text string) {
	pSendMessageW.Call(hwnd, WM_SETTEXT, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr(text))))
}

// LaunchClawBotLogin opens the QR login flow in a dedicated console window.
func LaunchClawBotLogin() error {
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	command := exec.Command(executable, "login")
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: 0x00000010}
	return command.Start()
}

func ShowSettingsDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("AgentNotifySettingsDialog")
	cfg, _ := config.LoadConfig("")

	var dialog uintptr
	var quietEdit, cooldownEdit, backgroundBrush uintptr

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_CREATE:
			font := newFont(13, 400)
			backgroundBrush, _, _ = pCreateSolidBrush.Call(uintptr(RGB(22, 26, 31)))

			quietEdit, _, _ = pCreateWindowExW.Call(
				0,
				uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
				0,
				WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL,
				170, 168, 120, 28,
				hwnd, IDC_SETTINGS_QUIET, hInstance, 0,
			)
			cooldownEdit, _, _ = pCreateWindowExW.Call(
				0,
				uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
				0,
				WS_CHILD|WS_VISIBLE|WS_BORDER|ES_AUTOHSCROLL|ES_NUMBER,
				170, 208, 120, 28,
				hwnd, IDC_SETTINGS_COOLDOWN, hInstance, 0,
			)
			pSendMessageW.Call(quietEdit, WM_SETFONT, font, 1)
			pSendMessageW.Call(cooldownEdit, WM_SETFONT, font, 1)
			setWindowText(quietEdit, cfg.QuietHours)
			setWindowText(cooldownEdit, fmt.Sprintf("%d", cfg.CooldownMin))

			createButton := func(text string, id uintptr, x, y, width, height uintptr) uintptr {
				control, _, _ := pCreateWindowExW.Call(
					0,
					uintptr(unsafe.Pointer(StringToUTF16Ptr("BUTTON"))),
					uintptr(unsafe.Pointer(StringToUTF16Ptr(text))),
					WS_CHILD|WS_VISIBLE,
					x, y, width, height,
					hwnd, id, hInstance, 0,
				)
				pSendMessageW.Call(control, WM_SETFONT, font, 1)
				return control
			}
			createButton("扫码登录 / 重新登录", IDC_SETTINGS_LOGIN, 250, 62, 170, 32)
			createButton("退出 ClawBot 登录", IDC_SETTINGS_LOGOUT, 250, 102, 170, 32)
			createButton("保存", IDC_SETTINGS_SAVE, 250, 270, 80, 34)
			createButton("取消", IDC_SETTINGS_CANCEL, 340, 270, 80, 34)
			return 0

		case WM_PAINT:
			var paint PAINTSTRUCT
			hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
			var rect RECT
			pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
			background, _, _ := pCreateSolidBrush.Call(uintptr(RGB(22, 26, 31)))
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rect)), background)
			pDeleteObject.Call(background)
			pSetBkMode.Call(hdc, TRANSPARENT)

			titleFont := newFont(18, 700)
			baseFont := newFont(13, 400)
			smallFont := newFont(11, 400)
			oldFont, _, _ := pSelectObject.Call(hdc, titleFont)

			pSetTextColor.Call(hdc, uintptr(RGB(242, 245, 247)))
			DrawText(hdc, "Agent-notify 设置", &RECT{20, 14, 300, 42}, DT_SINGLELINE|DT_VCENTER)

			pSelectObject.Call(hdc, baseFont)
			pSetTextColor.Call(hdc, uintptr(RGB(232, 237, 240)))
			DrawText(hdc, "ClawBot", &RECT{20, 62, 120, 88}, DT_SINGLELINE|DT_VCENTER)

			status := clawbot.GetStatus()
			statusText := "未登录，点击右侧按钮扫码"
			statusColor := uintptr(RGB(220, 92, 92))
			if status.LoggedIn {
				statusText = "已登录 · " + status.UserHint
				statusColor = uintptr(RGB(54, 190, 144))
			}
			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, statusColor)
			DrawText(hdc, statusText, &RECT{20, 86, 230, 108}, DT_SINGLELINE|DT_VCENTER)

			pSetTextColor.Call(hdc, uintptr(RGB(151, 160, 170)))
			DrawText(hdc, "凭据仅保存在本机，不会上传到第三方服务。", &RECT{20, 116, 220, 140}, DT_SINGLELINE|DT_VCENTER)

			pSelectObject.Call(hdc, baseFont)
			pSetTextColor.Call(hdc, uintptr(RGB(232, 237, 240)))
			DrawText(hdc, "勿扰时段", &RECT{20, 168, 145, 196}, DT_SINGLELINE|DT_VCENTER)
			DrawText(hdc, "会话冷却（分钟）", &RECT{20, 208, 145, 236}, DT_SINGLELINE|DT_VCENTER)

			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
			DrawText(hdc, "留空表示关闭；格式如 23-8", &RECT{298, 168, 442, 196}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)
			DrawText(hdc, "同一会话去重，默认 10", &RECT{298, 208, 442, 236}, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)

			pSelectObject.Call(hdc, oldFont)
			pDeleteObject.Call(titleFont)
			pDeleteObject.Call(baseFont)
			pDeleteObject.Call(smallFont)
			pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
			return 0

		case WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC:
			pSetTextColor.Call(uintptr(wParam), uintptr(RGB(235, 239, 242)))
			pSetBkColor := user32.NewProc("SetBkColor")
			pSetBkColor.Call(uintptr(wParam), uintptr(RGB(16, 19, 23)))
			return backgroundBrush

		case WM_COMMAND:
			switch int(wParam & 0xFFFF) {
			case IDC_SETTINGS_SAVE:
				quiet := strings.TrimSpace(getWindowText(quietEdit))
				if !config.ValidQuietHours(quiet) {
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("勿扰时段格式无效。留空表示关闭，或使用 23-8 形式。"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
					return 0
				}
				cooldown := config.DefaultCooldownMin
				if parsed, err := strconv.Atoi(strings.TrimSpace(getWindowText(cooldownEdit))); err == nil && parsed > 0 {
					cooldown = parsed
				}
				if err := config.SaveConfig(config.AppConfig{QuietHours: quiet, CooldownMin: cooldown}, ""); err != nil {
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("保存失败："+err.Error()))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
					return 0
				}
				pDestroyWindow.Call(hwnd)
				return 0

			case IDC_SETTINGS_CANCEL:
				pDestroyWindow.Call(hwnd)
				return 0

			case IDC_SETTINGS_LOGIN:
				if err := LaunchClawBotLogin(); err != nil {
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("无法启动登录终端："+err.Error()))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
					return 0
				}
				pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("已打开登录终端。请使用微信扫描二维码；完成后关闭终端即可。"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
				return 0

			case IDC_SETTINGS_LOGOUT:
				answer, _, _ := pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("确定退出当前 ClawBot 登录吗？"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_YESNO|MB_ICONQUESTION)
				if answer == IDYES {
					_ = clawbot.DeleteCredentials()
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("已退出 ClawBot 登录。"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
					pInvalidateRect.Call(hwnd, 0, 1)
				}
				return 0
			}
			return 0

		case WM_CLOSE:
			pDestroyWindow.Call(hwnd)
			return 0

		case WM_DESTROY:
			if backgroundBrush != 0 {
				pDeleteObject.Call(backgroundBrush)
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
	pRegisterClassExW.Call(uintptr(unsafe.Pointer(&windowClass)))

	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	width := int32(460)
	height := int32(330)
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

	var message MSG
	for dialog != 0 {
		result, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if result == 0 || int32(result) == -1 {
			break
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&message)))
	}
}
