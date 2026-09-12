//go:build windows

package ui

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agent"
	"github.com/srafyhucl-cpu/agent-notify/internal/app"
	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

const (
	widgetWidth  = int32(380)
	widgetHeight = int32(300)
)

var (
	classNameWidget   = StringToUTF16Ptr("AgentNotifyWidgetMain")
	windowTitleWidget = StringToUTF16Ptr("Agent-notify")
	msgWakeupID       uint32
	wndProcCallback   uintptr
)

type WidgetApp struct {
	hwnd            uintptr
	tray            *TrayManager
	paths           config.Paths
	onOpenCode      bool
	onCodex         bool
	clawbotLoggedIn bool
	quietHours      string
	lastPushText    string
	procStatus      ProcessStatus
	hoverOpenCode   bool
	hoverCodex      bool
	hoverMin        bool
	hoverClose      bool
	hoverHistory    bool
	hoverSettings   bool
	hoverTest       bool
	hoverExit       bool
	isTracking      bool
}

func debugLog(format string, args ...interface{}) {
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0700)
	entry := fmt.Sprintf("[%s] [PID:%d] %s\r\n", time.Now().Format("15:04:05.000"), os.Getpid(), fmt.Sprintf(format, args...))
	file, err := os.OpenFile(paths.WidgetTraceLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err == nil {
		_, _ = file.WriteString(entry)
		_ = file.Close()
	}
}

func resolveWidgetPosition(raw string, screenWidth, screenHeight, winWidth, winHeight int32) (int32, int32) {
	defaultX := screenWidth - winWidth - 30
	defaultY := (screenHeight - winHeight) / 2
	if defaultX < 20 {
		defaultX = 20
	}
	if defaultY < 20 {
		defaultY = 20
	}
	parts := strings.Split(strings.TrimSpace(raw), ",")
	if len(parts) != 2 {
		return defaultX, defaultY
	}
	x, errX := strconv.Atoi(strings.TrimSpace(parts[0]))
	y, errY := strconv.Atoi(strings.TrimSpace(parts[1]))
	if errX != nil || errY != nil {
		return defaultX, defaultY
	}
	if int32(x) < 0 || int32(y) < 0 || int32(x) > screenWidth-winWidth/2 || int32(y) > screenHeight-winHeight/2 {
		return defaultX, defaultY
	}
	return int32(x), int32(y)
}

func savePosition(hwnd uintptr, posFile string) {
	if hwnd == 0 {
		return
	}
	var rect RECT
	pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
	if rect.Left < -10000 || rect.Top < -10000 {
		return
	}
	_ = os.WriteFile(posFile, []byte(fmt.Sprintf("%d,%d", rect.Left, rect.Top)), 0600)
}

func restoreAndBringToFront(hwnd uintptr) {
	if hwnd == 0 {
		return
	}
	pShowWindow.Call(hwnd, SW_SHOW)
	pShowWindow.Call(hwnd, SW_RESTORE)
	pSetWindowPos.Call(hwnd, uintptr(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW)
	ForceForegroundWindow(hwnd)
	pInvalidateRect.Call(hwnd, 0, 1)
}

func getLastPushRelative(paths config.Paths) string {
	history, err := notify.GetHistory(1, paths.PushLog)
	if err != nil || len(history) == 0 {
		return "暂无推送"
	}
	timestamp := history[0].LocalTime()
	if timestamp.IsZero() {
		return history[0].Timestamp
	}
	delta := time.Since(timestamp)
	switch {
	case delta < time.Minute:
		return "刚刚"
	case delta < time.Hour:
		return fmt.Sprintf("%d 分钟前", int(delta.Minutes()))
	case timestamp.Year() == time.Now().Year() && timestamp.YearDay() == time.Now().YearDay():
		return timestamp.Format("15:04")
	default:
		return timestamp.Format("01-02 15:04")
	}
}

func truncateUI(value string, maxRunes int) string {
	runes := []rune(strings.TrimSpace(value))
	if len(runes) <= maxRunes {
		return string(runes)
	}
	return string(runes[:maxRunes]) + "…"
}

func newFont(size, weight int32) uintptr {
	font, _, _ := pCreateFontW.Call(
		uintptr(size), 0, 0, 0, uintptr(weight), 0, 0, 0, 1, 0, 0, 0, 0,
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei UI"))),
	)
	return font
}

func fillRoundRect(hdc uintptr, rect RECT, radius, color uintptr) {
	brush, _, _ := pCreateSolidBrush.Call(color)
	nullPen, _, _ := pCreatePen.Call(5, 0, 0)
	oldBrush, _, _ := pSelectObject.Call(hdc, brush)
	oldPen, _, _ := pSelectObject.Call(hdc, nullPen)
	pRoundRect.Call(hdc, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right), uintptr(rect.Bottom), radius, radius)
	pSelectObject.Call(hdc, oldBrush)
	pSelectObject.Call(hdc, oldPen)
	pDeleteObject.Call(brush)
	pDeleteObject.Call(nullPen)
}

// RunWidget starts the native Windows GUI widget.
func RunWidget() {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	debugLog("RunWidget enter")

	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0700)
	_ = os.Remove(paths.WidgetExitMarker)

	msgWakeup, _, _ := pRegisterWindowMessageW.Call(uintptr(unsafe.Pointer(StringToUTF16Ptr("AgentNotify_Wakeup_Message_v1"))))
	msgWakeupID = uint32(msgWakeup)
	wakeupEventName := StringToUTF16Ptr("Local\\AgentNotify_Wakeup_Event")

	const ERROR_ALREADY_EXISTS = syscall.Errno(183)
	hMutex, _, err := pCreateMutexW.Call(0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Local\\AgentNotifyWidgetSingleInstance"))))
	if hMutex != 0 {
		if errno, ok := err.(syscall.Errno); ok && errno == ERROR_ALREADY_EXISTS {
			pCloseHandle.Call(hMutex)
			hEvent, _, _ := pOpenEventW.Call(EVENT_MODIFY_STATE, 0, uintptr(unsafe.Pointer(wakeupEventName)))
			if hEvent != 0 {
				pSetEvent.Call(hEvent)
				pCloseHandle.Call(hEvent)
			}
			if msgWakeupID != 0 {
				pPostMessageW.Call(HWND_BROADCAST, uintptr(msgWakeupID), 0, 0)
			}
			existing, _, _ := user32.NewProc("FindWindowW").Call(uintptr(unsafe.Pointer(classNameWidget)), 0)
			if existing != 0 {
				restoreAndBringToFront(existing)
				return
			}
			KillOtherAgentNotifyInstances()
			time.Sleep(150 * time.Millisecond)
		} else {
			defer pCloseHandle.Call(hMutex)
		}
	}

	hWakeupEvent, _, _ := pCreateEventW.Call(0, 0, 0, uintptr(unsafe.Pointer(wakeupEventName)))
	if hWakeupEvent != 0 {
		defer pCloseHandle.Call(hWakeupEvent)
	}
	_ = os.WriteFile(paths.WidgetAliveFile, []byte(time.Now().Format(time.RFC3339)), 0600)

	go func() {
		time.Sleep(2 * time.Second)
		_ = agent.HandleWatch("", "")
		ticker := time.NewTicker(2 * time.Minute)
		defer ticker.Stop()
		for range ticker.C {
			_ = agent.HandleWatch("", "")
		}
	}()

	instance := &WidgetApp{paths: paths}

	if hWakeupEvent != 0 {
		go func() {
			for {
				result, _, _ := pWaitForSingleObject.Call(hWakeupEvent, 500)
				if result == WAIT_OBJECT_0 && instance.hwnd != 0 {
					pPostMessageW.Call(instance.hwnd, WM_USER_WAKEUP, 0, 0)
				}
			}
		}()
	}

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		message := uint32(msg)
		if message == WM_USER_WAKEUP || (msgWakeupID != 0 && message == msgWakeupID) {
			restoreAndBringToFront(hwnd)
			return 0
		}

		switch message {
		case WM_CREATE:
			instance.hwnd = hwnd
			instance.tray = NewTrayManager(hwnd)
			instance.refreshState()
			pSetTimer.Call(hwnd, 1, 5000, 0)
			return 0

		case WM_TIMER:
			instance.refreshState()
			_ = os.WriteFile(paths.WidgetAliveFile, []byte(time.Now().Format(time.RFC3339)), 0600)
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_PAINT:
			var paint PAINTSTRUCT
			hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
			var rect RECT
			pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
			width := rect.Right - rect.Left
			height := rect.Bottom - rect.Top

			hdcMem, _, _ := pCreateCompatibleDC.Call(hdc)
			hBitmap, _, _ := pCreateCompatibleBitmap.Call(hdc, uintptr(width), uintptr(height))
			oldBitmap, _, _ := pSelectObject.Call(hdcMem, hBitmap)
			instance.drawUI(hdcMem, width, height)
			pBitBlt.Call(hdc, 0, 0, uintptr(width), uintptr(height), hdcMem, 0, 0, SRCCOPY)
			pSelectObject.Call(hdcMem, oldBitmap)
			pDeleteObject.Call(hBitmap)
			pDeleteDC.Call(hdcMem)
			pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&paint)))
			return 0

		case WM_MOUSEMOVE:
			if !instance.isTracking {
				var track TRACKMOUSEEVENT
				track.CbSize = uint32(unsafe.Sizeof(track))
				track.DwFlags = 0x00000002
				track.HWndTrack = hwnd
				pTrackMouseEvent.Call(uintptr(unsafe.Pointer(&track)))
				instance.isTracking = true
			}
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)
			previous := [...]bool{instance.hoverOpenCode, instance.hoverCodex, instance.hoverMin, instance.hoverClose, instance.hoverHistory, instance.hoverSettings, instance.hoverTest, instance.hoverExit}
			instance.hoverOpenCode = x >= 16 && x <= 364 && y >= 54 && y <= 104
			instance.hoverCodex = x >= 16 && x <= 364 && y >= 110 && y <= 160
			instance.hoverMin = x >= 302 && x <= 336 && y >= 6 && y <= 40
			instance.hoverClose = x >= 340 && x <= 368 && y >= 6 && y <= 40
			instance.hoverHistory = x >= 68 && x <= 136 && y >= 252 && y <= 286
			instance.hoverSettings = x >= 144 && x <= 212 && y >= 252 && y <= 286
			instance.hoverTest = x >= 220 && x <= 288 && y >= 252 && y <= 286
			instance.hoverExit = x >= 296 && x <= 364 && y >= 252 && y <= 286
			current := [...]bool{instance.hoverOpenCode, instance.hoverCodex, instance.hoverMin, instance.hoverClose, instance.hoverHistory, instance.hoverSettings, instance.hoverTest, instance.hoverExit}
			changed := false
			for i := range current {
				if current[i] != previous[i] {
					changed = true
				}
			}
			if changed {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if instance.hoverMin || instance.hoverClose || instance.hoverOpenCode || instance.hoverCodex || instance.hoverHistory || instance.hoverSettings || instance.hoverTest || instance.hoverExit {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WM_MOUSELEAVE:
			instance.isTracking = false
			instance.hoverOpenCode = false
			instance.hoverCodex = false
			instance.hoverMin = false
			instance.hoverClose = false
			instance.hoverHistory = false
			instance.hoverSettings = false
			instance.hoverTest = false
			instance.hoverExit = false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)
			if y >= 4 && y <= 42 && x < 302 {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, paths.WidgetPosFile)
				return 0
			}
			if instance.hoverMin || instance.hoverClose {
				savePosition(hwnd, paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
				return 0
			}
			if instance.hoverOpenCode {
				_, _ = marker.SetMarker(paths.OpenCodeMarker, "Flip")
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if instance.hoverCodex {
				_, _ = marker.SetMarker(paths.CodexMarker, "Flip")
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if instance.hoverHistory {
				ShowHistoryDialog(hwnd)
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if instance.hoverSettings {
				ShowSettingsDialog(hwnd)
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if instance.hoverTest {
				go func() {
					result := notify.SendNotification(notify.NotifyOptions{
						Agent:   "test",
						Title:   "【测试】Agent-notify",
						Summary: "ClawBot 推送链路正常。",
					})
					message := "测试推送状态：" + result.Status
					if result.Error != "" {
						message += "\n" + result.Error
					}
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr(message))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify"))), MB_OK|MB_ICONINFO)
				}()
				return 0
			}
			if instance.hoverExit {
				savePosition(hwnd, paths.WidgetPosFile)
				_ = os.WriteFile(paths.WidgetExitMarker, []byte(time.Now().Format(time.RFC3339)), 0600)
				if instance.tray != nil {
					instance.tray.Destroy()
				}
				pDestroyWindow.Call(hwnd)
				pPostQuitMessage.Call(0)
				return 0
			}
			return 0

		case WM_TRAYICON:
			switch lParam {
			case WM_LBUTTONDBLCLK:
				restoreAndBringToFront(hwnd)
			case WM_RBUTTONUP:
				visible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
				instance.tray.ShowContextMenu(visible != 0, instance.onOpenCode, instance.onCodex)
			}
			return 0

		case WM_COMMAND:
			switch int(wParam & 0xFFFF) {
			case IDM_TOGGLE_SHOW:
				visible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
				if visible != 0 {
					savePosition(hwnd, paths.WidgetPosFile)
					pShowWindow.Call(hwnd, SW_HIDE)
				} else {
					restoreAndBringToFront(hwnd)
				}
			case IDM_TOGGLE_OPENCODE:
				_, _ = marker.SetMarker(paths.OpenCodeMarker, "Flip")
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			case IDM_TOGGLE_CODEX:
				_, _ = marker.SetMarker(paths.CodexMarker, "Flip")
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			case IDM_HISTORY:
				ShowHistoryDialog(hwnd)
			case IDM_SETTINGS:
				ShowSettingsDialog(hwnd)
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			case IDM_TEST_PUSH:
				go func() {
					_ = notify.SendNotification(notify.NotifyOptions{Agent: "test", Title: "【测试】Agent-notify", Summary: "ClawBot 推送链路正常。"})
				}()
			case IDM_EXIT:
				savePosition(hwnd, paths.WidgetPosFile)
				_ = os.WriteFile(paths.WidgetExitMarker, []byte(time.Now().Format(time.RFC3339)), 0600)
				if instance.tray != nil {
					instance.tray.Destroy()
				}
				pDestroyWindow.Call(hwnd)
				pPostQuitMessage.Call(0)
			}
			return 0

		case WM_CLOSE:
			savePosition(hwnd, paths.WidgetPosFile)
			pShowWindow.Call(hwnd, SW_HIDE)
			return 0

		case WM_DESTROY:
			if instance.tray != nil {
				instance.tray.Destroy()
			}
			pPostQuitMessage.Call(0)
			return 0
		}
		result, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
		return result
	})
	wndProcCallback = wndProc

	if setDPI := user32.NewProc("SetProcessDpiAwarenessContext"); setDPI.Find() == nil {
		const perMonitorV2 = ^uintptr(3)
		setDPI.Call(perMonitorV2)
	}

	var windowClass WNDCLASSEXW
	windowClass.CbSize = uint32(unsafe.Sizeof(windowClass))
	windowClass.Style = 0x0003 | CS_GLOBALCLASS
	windowClass.LpfnWndProc = wndProc
	windowClass.HInstance = hInstance
	windowClass.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
	windowClass.HbrBackground, _, _ = pCreateSolidBrush.Call(uintptr(RGB(17, 20, 24)))
	windowClass.LpszClassName = classNameWidget
	pRegisterClassExW.Call(uintptr(unsafe.Pointer(&windowClass)))

	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	rawPosition := ""
	if data, err := os.ReadFile(paths.WidgetPosFile); err == nil {
		rawPosition = string(data)
	}
	winX, winY := resolveWidgetPosition(rawPosition, int32(screenWidth), int32(screenHeight), widgetWidth, widgetHeight)

	hwnd, _, createErr := pCreateWindowExW.Call(
		WS_EX_APPWINDOW|WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(classNameWidget)),
		uintptr(unsafe.Pointer(windowTitleWidget)),
		WS_POPUP|WS_MINIMIZEBOX|WS_SYSMENU|WS_VISIBLE,
		uintptr(winX), uintptr(winY), uintptr(widgetWidth), uintptr(widgetHeight),
		0, 0, hInstance, 0,
	)
	if hwnd == 0 {
		debugLog("CreateWindowExW failed: %v", createErr)
		return
	}
	pSetWindowTextW.Call(hwnd, uintptr(unsafe.Pointer(windowTitleWidget)))
	cornerPreference := uint32(2)
	pDwmSetWindowAttribute.Call(hwnd, 33, uintptr(unsafe.Pointer(&cornerPreference)), 4)
	darkMode := uint32(1)
	pDwmSetWindowAttribute.Call(hwnd, 20, uintptr(unsafe.Pointer(&darkMode)), 4)
	pShowWindow.Call(hwnd, SW_SHOW)
	pUpdateWindow.Call(hwnd)
	ForceForegroundWindow(hwnd)

	var message MSG
	for {
		result, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if result == 0 || int32(result) == -1 {
			break
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&message)))
	}
}

func (app *WidgetApp) refreshState() {
	app.onOpenCode = !marker.IsOff(app.paths.OpenCodeMarker)
	app.onCodex = !marker.IsOff(app.paths.CodexMarker)
	app.clawbotLoggedIn = clawbot.HasCredentials()
	if cfg, err := config.LoadConfig(""); err == nil {
		app.quietHours = cfg.QuietHours
	}
	app.procStatus = DetectProcesses()
	app.lastPushText = getLastPushRelative(app.paths)

	state := 0
	if app.clawbotLoggedIn && app.onOpenCode && app.onCodex {
		state = 2
	} else if app.clawbotLoggedIn && (app.onOpenCode || app.onCodex) {
		state = 1
	}
	if app.tray != nil {
		app.tray.UpdateState(state)
	}
}

func (app *WidgetApp) health() (uint32, string) {
	if !app.clawbotLoggedIn {
		return RGB(220, 92, 92), "未登录"
	}
	if app.onOpenCode && app.onCodex {
		return RGB(54, 190, 144), "正常"
	}
	if app.onOpenCode || app.onCodex {
		return RGB(224, 165, 70), "部分暂停"
	}
	return RGB(220, 92, 92), "全部暂停"
}

func (app *WidgetApp) drawUI(hdc uintptr, width, height int32) {
	background := RECT{0, 0, width, height}
	backgroundBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(17, 20, 24)))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&background)), backgroundBrush)
	pDeleteObject.Call(backgroundBrush)
	pSetBkMode.Call(hdc, TRANSPARENT)

	healthColor, healthText := app.health()
	strip := RECT{0, 0, width, 3}
	stripBrush, _, _ := pCreateSolidBrush.Call(uintptr(healthColor))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&strip)), stripBrush)
	pDeleteObject.Call(stripBrush)

	bold := newFont(17, 700)
	base := newFont(13, 400)
	small := newFont(11, 400)
	oldFont, _, _ := pSelectObject.Call(hdc, bold)
	defer func() {
		pSelectObject.Call(hdc, oldFont)
		pDeleteObject.Call(bold)
		pDeleteObject.Call(base)
		pDeleteObject.Call(small)
	}()

	pSetTextColor.Call(hdc, uintptr(RGB(242, 245, 247)))
	titleRect := RECT{16, 6, 250, 30}
	DrawText(hdc, "Agent-notify", &titleRect, DT_SINGLELINE|DT_VCENTER)

	pSelectObject.Call(hdc, small)
	pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
	subtitleRect := RECT{16, 27, 260, 46}
	DrawText(hdc, "OpenCode + Codex  ·  ClawBot 微信通知", &subtitleRect, DT_SINGLELINE|DT_VCENTER)

	pSetTextColor.Call(hdc, uintptr(healthColor))
	healthRect := RECT{238, 8, 296, 30}
	DrawText(hdc, healthText, &healthRect, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)

	pSetTextColor.Call(hdc, uintptr(RGB(130, 139, 149)))
	minRect := RECT{302, 6, 336, 40}
	DrawText(hdc, "—", &minRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE)
	pSetTextColor.Call(hdc, uintptr(RGB(130, 139, 149)))
	closeRect := RECT{340, 6, 368, 40}
	DrawText(hdc, "×", &closeRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

	drawAgentRow := func(y int32, name string, enabled, running, hover bool) {
		rect := RECT{16, y, 364, y + 50}
		fillColor := uintptr(RGB(26, 31, 36))
		if hover {
			fillColor = uintptr(RGB(32, 38, 44))
		}
		fillRoundRect(hdc, rect, 10, fillColor)

		pSelectObject.Call(hdc, base)
		pSetTextColor.Call(hdc, uintptr(RGB(238, 242, 244)))
		nameRect := RECT{28, y + 5, 170, y + 28}
		DrawText(hdc, name, &nameRect, DT_SINGLELINE|DT_VCENTER)

		pSelectObject.Call(hdc, small)
		stateText := "已暂停"
		stateColor := uintptr(RGB(126, 135, 145))
		if enabled {
			stateText = "监听中"
			stateColor = uintptr(RGB(64, 203, 157))
		}
		pSetTextColor.Call(hdc, stateColor)
		stateRect := RECT{28, y + 27, 90, y + 46}
		DrawText(hdc, stateText, &stateRect, DT_SINGLELINE|DT_VCENTER)

		processText := "未运行"
		if running {
			processText = "运行中"
		}
		pSetTextColor.Call(hdc, uintptr(RGB(126, 135, 145)))
		processRect := RECT{104, y + 27, 210, y + 46}
		DrawText(hdc, processText, &processRect, DT_SINGLELINE|DT_VCENTER)

		track := RECT{278, y + 12, 348, y + 38}
		trackColor := uintptr(RGB(61, 68, 76))
		if enabled {
			trackColor = uintptr(RGB(45, 150, 121))
		}
		fillRoundRect(hdc, track, 13, trackColor)
		knobX := int32(304)
		if enabled {
			knobX = 326
		}
		knobBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(246, 249, 250)))
		knobPen, _, _ := pCreatePen.Call(5, 0, 0)
		oldBrush, _, _ := pSelectObject.Call(hdc, knobBrush)
		oldPen, _, _ := pSelectObject.Call(hdc, knobPen)
		pEllipse.Call(hdc, uintptr(knobX), uintptr(y+15), uintptr(knobX+20), uintptr(y+35))
		pSelectObject.Call(hdc, oldBrush)
		pSelectObject.Call(hdc, oldPen)
		pDeleteObject.Call(knobBrush)
		pDeleteObject.Call(knobPen)
	}

	drawAgentRow(54, "OpenCode", app.onOpenCode, app.procStatus.OpenCodeRunning, app.hoverOpenCode)
	drawAgentRow(110, "Codex", app.onCodex, app.procStatus.CodexRunning, app.hoverCodex)

	statusRect := RECT{16, 170, 364, 238}
	fillRoundRect(hdc, statusRect, 10, uintptr(RGB(23, 28, 33)))

	connectionText := "ClawBot 未登录"
	connectionColor := uintptr(RGB(220, 92, 92))
	if app.clawbotLoggedIn {
		connectionText = "ClawBot 已连接"
		connectionColor = uintptr(RGB(54, 190, 144))
	}
	connectionBrush, _, _ := pCreateSolidBrush.Call(connectionColor)
	connectionPen, _, _ := pCreatePen.Call(5, 0, 0)
	oldBrush, _, _ := pSelectObject.Call(hdc, connectionBrush)
	oldPen, _, _ := pSelectObject.Call(hdc, connectionPen)
	pEllipse.Call(hdc, 28, 181, 36, 189)
	pSelectObject.Call(hdc, oldBrush)
	pSelectObject.Call(hdc, oldPen)
	pDeleteObject.Call(connectionBrush)
	pDeleteObject.Call(connectionPen)

	pSelectObject.Call(hdc, base)
	pSetTextColor.Call(hdc, uintptr(RGB(231, 236, 239)))
	connectionRect := RECT{44, 175, 228, 197}
	DrawText(hdc, connectionText, &connectionRect, DT_SINGLELINE|DT_VCENTER)

	pSelectObject.Call(hdc, small)
	pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
	quietText := "勿扰关闭"
	if strings.TrimSpace(app.quietHours) != "" {
		quietText = "勿扰 " + app.quietHours
	}
	quietRect := RECT{236, 175, 352, 197}
	DrawText(hdc, quietText, &quietRect, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)

	pSetTextColor.Call(hdc, uintptr(RGB(139, 148, 158)))
	lastLabel := RECT{28, 202, 112, 226}
	DrawText(hdc, "上次推送", &lastLabel, DT_SINGLELINE|DT_VCENTER)
	pSetTextColor.Call(hdc, uintptr(RGB(224, 229, 232)))
	lastValue := RECT{112, 202, 352, 226}
	DrawText(hdc, truncateUI(app.lastPushText, 36), &lastValue, DT_RIGHT|DT_SINGLELINE|DT_VCENTER)

	separator := RECT{16, 244, 364, 245}
	separatorBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(42, 48, 54)))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&separator)), separatorBrush)
	pDeleteObject.Call(separatorBrush)

	pSelectObject.Call(hdc, small)
	pSetTextColor.Call(hdc, uintptr(RGB(105, 114, 124)))
	versionRect := RECT{16, 252, 64, 286}
	DrawText(hdc, "v"+app.Version(), &versionRect, DT_SINGLELINE|DT_VCENTER)

	drawButton := func(rect RECT, text string, hover bool, danger bool) {
		color := uintptr(RGB(27, 32, 37))
		foreground := uintptr(RGB(232, 237, 240))
		if hover {
			color = uintptr(RGB(39, 45, 52))
		}
		if danger {
			foreground = uintptr(RGB(233, 112, 112))
			if hover {
				color = uintptr(RGB(53, 33, 36))
			}
		}
		fillRoundRect(hdc, rect, 8, color)
		pSetTextColor.Call(hdc, foreground)
		DrawText(hdc, text, &rect, DT_CENTER|DT_VCENTER|DT_SINGLELINE)
	}
	drawButton(RECT{68, 252, 136, 286}, "历史", app.hoverHistory, false)
	drawButton(RECT{144, 252, 212, 286}, "设置", app.hoverSettings, false)
	drawButton(RECT{220, 252, 288, 286}, "测试", app.hoverTest, false)
	drawButton(RECT{296, 252, 364, 286}, "退出", app.hoverExit, true)
}

func (app *WidgetApp) Version() string {
	return appVersion()
}

func appVersion() string {
	return app.Version
}

// ForceForegroundWindow forces a window to front across threads and processes.
func ForceForegroundWindow(hwnd uintptr) {
	if hwnd == 0 {
		return
	}
	pShowWindow.Call(hwnd, SW_SHOW)
	pShowWindow.Call(hwnd, SW_RESTORE)
	currentForeground, _, _ := user32.NewProc("GetForegroundWindow").Call()
	foregroundThread, _, _ := user32.NewProc("GetWindowThreadProcessId").Call(currentForeground, 0)
	targetThread, _, _ := user32.NewProc("GetWindowThreadProcessId").Call(hwnd, 0)
	if foregroundThread != 0 && targetThread != 0 && foregroundThread != targetThread {
		attachThreadInput := user32.NewProc("AttachThreadInput")
		attachThreadInput.Call(foregroundThread, targetThread, 1)
		bringWindowToTop := user32.NewProc("BringWindowToTop")
		bringWindowToTop.Call(hwnd)
		pSetForegroundWindow.Call(hwnd)
		attachThreadInput.Call(foregroundThread, targetThread, 0)
	} else {
		bringWindowToTop := user32.NewProc("BringWindowToTop")
		bringWindowToTop.Call(hwnd)
		pSetForegroundWindow.Call(hwnd)
	}
	pSetWindowPos.Call(hwnd, uintptr(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW)
}

func widgetDebugPath() string {
	return filepath.Join(config.GetPaths().TempDir, "widget-trace.log")
}
