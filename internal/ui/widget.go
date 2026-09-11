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

	"linkweixin/internal/agent"
	"linkweixin/internal/config"
	"linkweixin/internal/marker"
	"linkweixin/internal/notify"
)

const AppVersion = "0.4.0"

var (
	classNameWidget   = StringToUTF16Ptr("LinkWeixinWidgetMain")
	windowTitleWidget = StringToUTF16Ptr("linkWeixin")
	msgWakeupID       uint32
	wndProcCallback   uintptr
)

// WidgetApp is the main window and event coordinator.
type WidgetApp struct {
	hwnd         uintptr
	tray         *TrayManager
	paths        config.Paths
	onOc         bool
	onCx         bool
	onAg         bool
	hoverOc      bool
	hoverCx      bool
	hoverAg      bool
	hoverMin     bool
	hoverX       bool
	hoverHist    bool
	hoverSet     bool
	hoverTest    bool
	hoverQuit    bool
	hoverLnkHist bool
	procStatus   ProcessStatus
	lastPushText string
	todayCount   int
	isTracking   bool
}

func getLighterColor(c uint32) uint32 {
	r := byte(c & 0xFF)
	g := byte((c >> 8) & 0xFF)
	b := byte((c >> 16) & 0xFF)
	min := func(a, b byte) byte {
		if a > b {
			return a
		}
		return b
	}
	r = min(255, r+25)
	g = min(255, g+25)
	b = min(255, b+25)
	return RGB(r, g, b)
}

func savePosition(hwnd uintptr, posFile string) {
	if hwnd == 0 {
		return
	}
	var rc RECT
	pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&rc)))
	// 防御最小化异常坐标（-32000）与外太空非法坐标
	if rc.Left < -10000 || rc.Top < -10000 {
		return
	}
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	if screenWidth > 0 && screenHeight > 0 {
		if rc.Left > int32(screenWidth) || rc.Top > int32(screenHeight) {
			return
		}
	}
	content := fmt.Sprintf("%d,%d", rc.Left, rc.Top)
	_ = os.WriteFile(posFile, []byte(content), 0644)
}

func debugLog(format string, a ...interface{}) {
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0755)
	logPath := filepath.Join(paths.TempDir, "widget-trace.log")
	msg := fmt.Sprintf("[%s] [PID:%d] %s\r\n", time.Now().Format("15:04:05.000"), os.Getpid(), fmt.Sprintf(format, a...))
	f, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err == nil {
		_, _ = f.WriteString(msg)
		_ = f.Close()
	}
}

func getLastPushRelative(paths config.Paths) string {
	hist, err := notify.GetHistory(1, paths.PushLog)
	if err != nil || len(hist) == 0 {
		return "暂无推送"
	}
	item := hist[0]
	if t, err := time.Parse(time.RFC3339Nano, item.RawTime); err == nil {
		d := time.Since(t)
		if d < time.Minute {
			return "刚刚"
		}
		if d < time.Hour {
			return fmt.Sprintf("%d 分钟前", int(d.Minutes()))
		}
		local := t.Local()
		now := time.Now()
		if local.YearDay() == now.YearDay() && local.Year() == now.Year() {
			return local.Format("15:04")
		}
		return local.Format("01-02 15:04")
	}
	return item.Time
}

func restoreAndBringToFront(hwnd uintptr) {
	debugLog("restoreAndBringToFront enter hwnd=%d", hwnd)
	if hwnd == 0 {
		debugLog("restoreAndBringToFront: hwnd is 0, returning")
		return
	}
	pShowWindow.Call(hwnd, SW_SHOW)
	pShowWindow.Call(hwnd, SW_RESTORE)

	var rc RECT
	pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&rc)))
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	debugLog("restoreAndBringToFront: screen=[%d,%d], currentRect=[%d,%d,%d,%d]", screenWidth, screenHeight, rc.Left, rc.Top, rc.Right, rc.Bottom)
	if rc.Left < 0 || rc.Left > int32(screenWidth)-100 || rc.Top < 0 || rc.Top > int32(screenHeight)-100 {
		newX := int32(screenWidth) - 380 - 40
		newY := (int32(screenHeight) - 450) / 2
		debugLog("restoreAndBringToFront: offscreen! Repositioning to [%d,%d]", newX, newY)
		pSetWindowPos.Call(hwnd, uintptr(HWND_TOPMOST), uintptr(newX), uintptr(newY), 380, 450, SWP_SHOWWINDOW)
	} else {
		pSetWindowPos.Call(hwnd, uintptr(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW)
	}

	ForceForegroundWindow(hwnd)
	pInvalidateRect.Call(hwnd, 0, 1)
	debugLog("restoreAndBringToFront complete")
}

// RunWidget starts the native Windows GUI widget.
func RunWidget() {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	debugLog("RunWidget enter: PID=%d", os.Getpid())

	// Ensure GUI thread is explicitly attached to user's interactive desktop (Default)
	if pOpenDesktopW.Find() == nil && pSetThreadDesktop.Find() == nil {
		hDesk, _, errDesk := pOpenDesktopW.Call(
			uintptr(unsafe.Pointer(StringToUTF16Ptr("Default"))),
			0,
			0,
			0x01FF, // DESKTOP_ALL_ACCESS
		)
		debugLog("OpenDesktopW(Default) => hDesk=%d err=%v", hDesk, errDesk)
		if hDesk != 0 {
			ret, _, errSet := pSetThreadDesktop.Call(hDesk)
			debugLog("SetThreadDesktop => ret=%d err=%v", ret, errSet)
		}
	}

	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0755)

	// Remove exit marker if present
	_ = os.Remove(paths.WidgetExitMarker)

	// Register system-wide unique wakeup broadcast message
	msgWakeup, _, _ := pRegisterWindowMessageW.Call(uintptr(unsafe.Pointer(StringToUTF16Ptr("LinkWeixin_Wakeup_Message_v1"))))
	msgWakeupID = uint32(msgWakeup)

	wakeupEventName := StringToUTF16Ptr("Local\\LinkWeixin_Wakeup_Event")

	// Single instance mutex (Local namespace for standard user compatibility)
	const ERROR_ALREADY_EXISTS = syscall.Errno(183)
	hMutex, _, err := pCreateMutexW.Call(0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Local\\LinkWeixinWidgetSingleInstance"))))
	debugLog("CreateMutexW => hMutex=%d err=%v", hMutex, err)
	if hMutex != 0 {
		if errno, ok := err.(syscall.Errno); ok && errno == ERROR_ALREADY_EXISTS {
			pCloseHandle.Call(hMutex)
			debugLog("Second instance detected existing mutex. Signaling Named Event...")

			// 1. Primary wakeup: trigger Named Event (immune to SW_HIDE, zero latency)
			hEvt, _, errOpen := pOpenEventW.Call(EVENT_MODIFY_STATE, 0, uintptr(unsafe.Pointer(wakeupEventName)))
			debugLog("OpenEventW => hEvt=%d err=%v", hEvt, errOpen)
			if hEvt != 0 {
				setRet, _, errSet := pSetEvent.Call(hEvt)
				debugLog("SetEvent => ret=%d err=%v", setRet, errSet)
				pCloseHandle.Call(hEvt)
			}

			// 2. Broadcast wakeup message as secondary fallback
			if msgWakeupID != 0 {
				pPostMessageW.Call(HWND_BROADCAST, uintptr(msgWakeupID), 0, 0)
			}

			// 3. Direct FindWindow fallback
			hExisting, _, _ := user32.NewProc("FindWindowW").Call(uintptr(unsafe.Pointer(classNameWidget)), 0)
			if hExisting == 0 {
				hExisting, _, _ = user32.NewProc("FindWindowW").Call(0, uintptr(unsafe.Pointer(windowTitleWidget)))
			}
			debugLog("FindWindowW fallback => hExisting=%d", hExisting)
			if hExisting != 0 {
				pShowWindow.Call(hExisting, SW_SHOW)
				pShowWindow.Call(hExisting, SW_RESTORE)
				ForceForegroundWindow(hExisting)
			}

			// 4. Check if existing instance is healthy (heartbeat alive within 15 seconds)
			isHealthy := false
			if hEvt != 0 {
				if fi, errStat := os.Stat(paths.WidgetAliveFile); errStat == nil {
					if time.Since(fi.ModTime()) < 15*time.Second {
						isHealthy = true
					}
				}
			}

			if isHealthy {
				debugLog("Existing instance is alive and healthy. Second instance exiting cleanly.")
				return
			}

			// If existing instance is dead/zombie, kill other instances and take over as primary!
			debugLog("WARNING: Existing instance is dead/zombie (hEvt=%d, alive stale). Terminating zombies and taking over!", hEvt)
			KillOtherLinkWeixinInstances()
			time.Sleep(200 * time.Millisecond)

			// Re-acquire mutex
			hMutexRetry, _, _ := pCreateMutexW.Call(0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Local\\LinkWeixinWidgetSingleInstance"))))
			if hMutexRetry != 0 {
				hMutex = hMutexRetry
				defer pCloseHandle.Call(hMutex)
				debugLog("Successfully re-acquired mutex after zombie cleanup! Proceeding to create UI...")
			}
		} else {
			defer pCloseHandle.Call(hMutex)
		}
	}

	// Create auto-reset Named Event for single-instance wakeup
	hWakeupEvent, _, errEvt := pCreateEventW.Call(0, 0, 0, uintptr(unsafe.Pointer(wakeupEventName)))
	debugLog("Primary instance CreateEventW => hWakeupEvent=%d err=%v", hWakeupEvent, errEvt)
	if hWakeupEvent != 0 {
		defer pCloseHandle.Call(hWakeupEvent)
	}

	// Touch heartbeat alive
	_ = os.WriteFile(paths.WidgetAliveFile, []byte(time.Now().Format(time.RFC3339)), 0644)

	// Start Codex notify config watchdog goroutine (replaces scheduled task)
	go func() {
		time.Sleep(2 * time.Second)
		agent.HandleWatch("", "")
		ticker := time.NewTicker(2 * time.Minute)
		defer ticker.Stop()
		for range ticker.C {
			agent.HandleWatch("", "")
		}
	}()

	app := &WidgetApp{
		paths: paths,
	}

	// Start background goroutine listening for Named Event wakeup
	if hWakeupEvent != 0 {
		go func() {
			debugLog("Named Event background listener started")
			for {
				ret, _, _ := pWaitForSingleObject.Call(hWakeupEvent, 500)
				if ret == WAIT_OBJECT_0 {
					debugLog("Named Event SIGNALED! app.hwnd=%d", app.hwnd)
					if app.hwnd != 0 {
						pPostMessageW.Call(app.hwnd, WM_USER_WAKEUP, 0, 0)
					}
				}
			}
		}()
	}

	hInstance, _, _ := pGetModuleHandleW.Call(0)

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		uMsg := uint32(msg)

		// Handle wakeup from a newly clicked launcher (either Named Event or broadcast)
		if uMsg == WM_USER_WAKEUP || (msgWakeupID != 0 && uMsg == msgWakeupID) {
			debugLog("wndProc received WAKEUP (uMsg=0x%X). Calling restoreAndBringToFront(hwnd=%d)", uMsg, hwnd)
			restoreAndBringToFront(hwnd)
			return 0
		}

		switch uMsg {
		case WM_CREATE:
			app.hwnd = hwnd
			app.tray = NewTrayManager(hwnd)
			app.refreshState()

			// 5-second timer for process and push check
			pSetTimer.Call(hwnd, 1, 5000, 0)
			// Touch boot log
			_ = os.WriteFile(filepath.Join(paths.TempDir, "widget-boot.log"), []byte(fmt.Sprintf("boot ok %d %s", os.Getpid(), time.Now().Format(time.RFC3339))), 0644)
			return 0

		case WM_TIMER:
			app.refreshState()
			pInvalidateRect.Call(hwnd, 0, 0)
			// Heartbeat alive
			_ = os.WriteFile(paths.WidgetAliveFile, []byte(time.Now().Format(time.RFC3339)), 0644)
			return 0

		case WM_PAINT:
			var ps PAINTSTRUCT
			hdc, _, _ := pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps)))

			var rc RECT
			pGetClientRect.Call(hwnd, uintptr(unsafe.Pointer(&rc)))
			width := rc.Right - rc.Left
			height := rc.Bottom - rc.Top

			// Double buffering
			hdcMem, _, _ := pCreateCompatibleDC.Call(hdc)
			hbmMem, _, _ := pCreateCompatibleBitmap.Call(hdc, uintptr(width), uintptr(height))
			hOldBmp, _, _ := pSelectObject.Call(hdcMem, hbmMem)

			app.drawUI(hdcMem, width, height)

			pBitBlt.Call(hdc, 0, 0, uintptr(width), uintptr(height), hdcMem, 0, 0, SRCCOPY)

			pSelectObject.Call(hdcMem, hOldBmp)
			pDeleteObject.Call(hbmMem)
			pDeleteDC.Call(hdcMem)

			pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps)))
			return 0

		case WM_MOUSEMOVE:
			if !app.isTracking {
				var tme TRACKMOUSEEVENT
				tme.CbSize = uint32(unsafe.Sizeof(tme))
				tme.DwFlags = 0x00000002 // TME_LEAVE
				tme.HWndTrack = hwnd
				pTrackMouseEvent.Call(uintptr(unsafe.Pointer(&tme)))
				app.isTracking = true
			}

			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)

			prevOc := app.hoverOc
			prevCx := app.hoverCx
			prevAg := app.hoverAg
			prevMin := app.hoverMin
			prevX := app.hoverX
			prevHist := app.hoverHist
			prevSet := app.hoverSet
			prevTest := app.hoverTest
			prevQuit := app.hoverQuit
			prevLnk := app.hoverLnkHist

			app.hoverMin = x >= 298 && x <= 338 && y >= 4 && y <= 42
			app.hoverX = x >= 338 && x <= 378 && y >= 4 && y <= 42
			app.hoverOc = x >= 20 && x <= 126 && y >= 56 && y <= 128
			app.hoverCx = x >= 137 && x <= 243 && y >= 56 && y <= 128
			app.hoverAg = x >= 254 && x <= 360 && y >= 56 && y <= 128
			app.hoverLnkHist = x >= 244 && x <= 340 && y >= 252 && y <= 282
			app.hoverHist = x >= 96 && x <= 154 && y >= 362 && y <= 394
			app.hoverSet = x >= 162 && x <= 220 && y >= 362 && y <= 394
			app.hoverTest = x >= 228 && x <= 286 && y >= 362 && y <= 394
			app.hoverQuit = x >= 294 && x <= 360 && y >= 362 && y <= 394

			// Set cursor
			if app.hoverMin || app.hoverX || app.hoverOc || app.hoverCx || app.hoverAg ||
				app.hoverLnkHist || app.hoverHist || app.hoverSet || app.hoverTest || app.hoverQuit {
				hHand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hHand)
			}

			if prevOc != app.hoverOc || prevCx != app.hoverCx || prevAg != app.hoverAg ||
				prevMin != app.hoverMin || prevX != app.hoverX || prevHist != app.hoverHist ||
				prevSet != app.hoverSet || prevTest != app.hoverTest || prevQuit != app.hoverQuit || prevLnk != app.hoverLnkHist {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			return 0

		case WM_MOUSELEAVE:
			app.isTracking = false
			app.hoverOc = false
			app.hoverCx = false
			app.hoverAg = false
			app.hoverMin = false
			app.hoverX = false
			app.hoverHist = false
			app.hoverSet = false
			app.hoverTest = false
			app.hoverQuit = false
			app.hoverLnkHist = false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)

			// Title bar dragging (0..290)
			if y >= 4 && y <= 42 && x < 290 {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, paths.WidgetPosFile)
				return 0
			}

			// Minimize button
			if x >= 298 && x <= 338 && y >= 4 && y <= 42 {
				savePosition(hwnd, paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
				return 0
			}

			// Close to tray button
			if x >= 338 && x <= 378 && y >= 4 && y <= 42 {
				savePosition(hwnd, paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
				return 0
			}

			// OpenCode switch
			if x >= 20 && x <= 126 && y >= 56 && y <= 128 {
				_, _ = marker.SetMarker(paths.OpenCodeMarker, "Flip")
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}

			// Codex switch
			if x >= 137 && x <= 243 && y >= 56 && y <= 128 {
				_, _ = marker.SetMarker(paths.CodexMarker, "Flip")
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}

			// Antigravity switch
			if x >= 254 && x <= 360 && y >= 56 && y <= 128 {
				_, _ = marker.SetMarker(paths.AntigravityMarker, "Flip")
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}

			// History button or link
			if (x >= 96 && x <= 154 && y >= 362 && y <= 394) || (x >= 244 && x <= 340 && y >= 252 && y <= 282) {
				ShowHistoryDialog(hwnd)
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}

			// Settings button
			if x >= 162 && x <= 220 && y >= 362 && y <= 394 {
				ShowSettingsDialog(hwnd)
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}

			// Test Push button
			if x >= 228 && x <= 286 && y >= 362 && y <= 394 {
				go func() {
					res := notify.SendNotification(notify.NotifyOptions{
						Title:   "【测试】linkWeixin",
						Summary: "来自原生 Go 独立单文件悬浮窗的测试推送，通道与排版运行正常！",
					})
					msgText := fmt.Sprintf("测试推送已发出！状态: %s", res.Status)
					pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr(msgText))), uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin"))), MB_OK|MB_ICONINFO)
				}()
				return 0
			}

			// Quit button
			if x >= 294 && x <= 360 && y >= 362 && y <= 394 {
				savePosition(hwnd, paths.WidgetPosFile)
				_ = os.WriteFile(paths.WidgetExitMarker, []byte(time.Now().Format(time.RFC3339)), 0644)
				if app.tray != nil {
					app.tray.Destroy()
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
				isVisible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
				app.tray.ShowContextMenu(isVisible != 0, app.onOc, app.onCx, app.onAg)
			}
			return 0

		case WM_COMMAND:
			cmdId := int(wParam & 0xFFFF)
			switch cmdId {
			case IDM_TOGGLE_SHOW:
				isVisible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
				if isVisible != 0 {
					savePosition(hwnd, paths.WidgetPosFile)
					pShowWindow.Call(hwnd, SW_HIDE)
				} else {
					restoreAndBringToFront(hwnd)
				}
			case IDM_TOGGLE_OC:
				_, _ = marker.SetMarker(paths.OpenCodeMarker, "Flip")
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			case IDM_TOGGLE_CX:
				_, _ = marker.SetMarker(paths.CodexMarker, "Flip")
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			case IDM_TOGGLE_AG:
				_, _ = marker.SetMarker(paths.AntigravityMarker, "Flip")
				app.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
			case IDM_HISTORY:
				ShowHistoryDialog(hwnd)
			case IDM_SETTINGS:
				ShowSettingsDialog(hwnd)
			case IDM_TEST_PUSH:
				go func() {
					_ = notify.SendNotification(notify.NotifyOptions{
						Title:   "【测试】linkWeixin",
						Summary: "来自托盘菜单触发的测试推送，多通道工作正常。",
					})
				}()
			case IDM_SHARE_CARD:
				shareText := fmt.Sprintf("🤖 linkWeixin v%s - 多 Agent AI 任务推送助手\r\n支持 OpenCode / Codex / Antigravity 任务完成后自动推送微信、企微、飞书、钉钉！\r\n✨ 原生 Go 独立编译 · 零闪烁无控制台黑框 · 毫秒自愈 · 桌面微光卡片\r\n开源地址：https://github.com/srafyhucl-cpu/linkWeixin", AppVersion)
				SetClipboardText(shareText)
				pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr("推荐名片文案已复制到剪贴板，可直接粘贴分享！"))), uintptr(unsafe.Pointer(StringToUTF16Ptr("linkWeixin 分享"))), MB_OK|MB_ICONINFO)
			case IDM_EXIT:
				savePosition(hwnd, paths.WidgetPosFile)
				_ = os.WriteFile(paths.WidgetExitMarker, []byte(time.Now().Format(time.RFC3339)), 0644)
				if app.tray != nil {
					app.tray.Destroy()
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
			if app.tray != nil {
				app.tray.Destroy()
			}
			pPostQuitMessage.Call(0)
			return 0
		}

		ret, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
		return ret
	})
	wndProcCallback = wndProc

	// Set Per-Monitor V2 DPI awareness
	pSetProcessDpiAwarenessContext := user32.NewProc("SetProcessDpiAwarenessContext")
	if pSetProcessDpiAwarenessContext.Find() == nil {
		const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = ^uintptr(3) // -4
		pSetProcessDpiAwarenessContext.Call(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
	}

	var wc WNDCLASSEXW
	wc.CbSize = uint32(unsafe.Sizeof(wc))
	wc.Style = 0x0003 | CS_GLOBALCLASS // CS_HREDRAW | CS_VREDRAW | CS_GLOBALCLASS
	wc.LpfnWndProc = wndProc
	wc.HInstance = hInstance
	wc.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
	wc.HbrBackground, _, _ = pCreateSolidBrush.Call(uintptr(RGB(24, 24, 27)))
	wc.LpszClassName = classNameWidget
	pRegisterClassExW.Call(uintptr(unsafe.Pointer(&wc)))

	// Window positioning (default to right center for maximum accessibility)
	winWidth := int32(380)
	winHeight := int32(450)
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	defaultX := int32(screenWidth) - winWidth - 30
	defaultY := (int32(screenHeight) - winHeight) / 2
	if defaultX < 50 {
		defaultX = 50
	}
	if defaultY < 50 {
		defaultY = 50
	}

	winX := defaultX
	winY := defaultY

	if data, err := os.ReadFile(paths.WidgetPosFile); err == nil {
		parts := strings.Split(strings.TrimSpace(string(data)), ",")
		if len(parts) == 2 {
			if x, err1 := strconv.Atoi(parts[0]); err1 == nil {
				if y, err2 := strconv.Atoi(parts[1]); err2 == nil {
					// Strict sanity check to ensure window is comfortably visible on primary screen
					if int32(x) >= 0 && int32(x) <= int32(screenWidth)-winWidth/2 &&
						int32(y) >= 0 && int32(y) <= int32(screenHeight)-winHeight/2 {
						winX = int32(x)
						winY = int32(y)
					}
				}
			}
		}
	}

	debugLog("Calling CreateWindowExW: winX=%d winY=%d winW=%d winH=%d", winX, winY, winWidth, winHeight)
	hwnd, _, errCreate := pCreateWindowExW.Call(
		WS_EX_APPWINDOW|WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(classNameWidget)),
		uintptr(unsafe.Pointer(windowTitleWidget)),
		WS_POPUP|WS_MINIMIZEBOX|WS_SYSMENU|WS_VISIBLE,
		uintptr(winX), uintptr(winY), uintptr(winWidth), uintptr(winHeight),
		0, 0, hInstance, 0,
	)
	debugLog("CreateWindowExW => hwnd=%d err=%v", hwnd, errCreate)
	if hwnd == 0 {
		debugLog("FATAL: CreateWindowExW returned 0! Exiting.")
		return
	}
	pSetWindowTextW.Call(hwnd, uintptr(unsafe.Pointer(windowTitleWidget)))
	_ = os.WriteFile(filepath.Join(paths.TempDir, "widget-hwnd.txt"), []byte(fmt.Sprintf("%d", hwnd)), 0644)

	// Round corners & dark mode
	cornerPref := uint32(2)
	pDwmSetWindowAttribute.Call(hwnd, 33, uintptr(unsafe.Pointer(&cornerPref)), 4)
	darkMode := uint32(1)
	pDwmSetWindowAttribute.Call(hwnd, 20, uintptr(unsafe.Pointer(&darkMode)), 4)

	debugLog("Calling ShowWindow(SW_SHOW)...")
	pShowWindow.Call(hwnd, SW_SHOW)
	pUpdateWindow.Call(hwnd)
	pSetWindowPos.Call(hwnd, uintptr(HWND_TOPMOST), uintptr(winX), uintptr(winY), uintptr(winWidth), uintptr(winHeight), SWP_SHOWWINDOW)
	debugLog("Calling ForceForegroundWindow...")
	ForceForegroundWindow(hwnd)

	debugLog("Entering Win32 message loop...")
	// Message loop
	var msg MSG
	for {
		ret, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0)
		if ret == 0 || int32(ret) == -1 {
			debugLog("GetMessageW loop break ret=%d", ret)
			break
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&msg)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&msg)))
	}
	debugLog("RunWidget exit cleanly")
}

func (app *WidgetApp) refreshState() {
	app.onOc = !marker.IsOff(app.paths.OpenCodeMarker)
	app.onCx = !marker.IsOff(app.paths.CodexMarker)
	app.onAg = !marker.IsOff(app.paths.AntigravityMarker)
	app.procStatus = DetectProcesses()
	app.lastPushText = getLastPushRelative(app.paths)

	onCount := 0
	if app.onOc {
		onCount++
	}
	if app.onCx {
		onCount++
	}
	if app.onAg {
		onCount++
	}

	state := 1
	if onCount == 3 {
		state = 2
	} else if onCount == 0 {
		state = 0
	}

	if app.tray != nil {
		app.tray.UpdateState(state)
	}
}

func (app *WidgetApp) drawUI(hdc uintptr, width, height int32) {
	// 1. Fill background (#18181B)
	rcAll := RECT{0, 0, width, height}
	hbrBg, _, _ := pCreateSolidBrush.Call(uintptr(RGB(24, 24, 27)))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcAll)), hbrBg)
	pDeleteObject.Call(hbrBg)

	pSetBkMode.Call(hdc, TRANSPARENT)

	// 2. Top glow status strip (4px height)
	stripColor := RGB(200, 130, 30) // Amber
	onCount := 0
	if app.onOc {
		onCount++
	}
	if app.onCx {
		onCount++
	}
	if app.onAg {
		onCount++
	}
	if onCount == 3 {
		stripColor = RGB(16, 185, 129) // Green
	} else if onCount == 0 {
		stripColor = RGB(220, 53, 69) // Red
	}

	rcStrip := RECT{0, 0, width, 4}
	hbrStrip, _, _ := pCreateSolidBrush.Call(uintptr(stripColor))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcStrip)), hbrStrip)
	pDeleteObject.Call(hbrStrip)

	// 3. Title bar (0..42)
	hFontBold, _, _ := pCreateFontW.Call(16, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
	hOldFont, _, _ := pSelectObject.Call(hdc, hFontBold)
	pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))

	rcTitle := RECT{14, 6, 260, 42}
	DrawText(hdc, "linkWeixin 推送", &rcTitle, DT_SINGLELINE|DT_VCENTER)

	// Minimize button
	minColor := RGB(161, 161, 170)
	if app.hoverMin {
		minColor = RGB(255, 255, 255)
	}
	pSetTextColor.Call(hdc, uintptr(minColor))
	rcMin := RECT{298, 6, 338, 42}
	DrawText(hdc, "—", &rcMin, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

	// Close button
	xColor := RGB(161, 161, 170)
	if app.hoverX {
		xColor = RGB(255, 255, 255)
	}
	pSetTextColor.Call(hdc, uintptr(xColor))
	rcX := RECT{338, 6, 378, 42}
	DrawText(hdc, "✕", &rcX, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

	// Separator below title bar
	rcSep := RECT{0, 42, width, 43}
	hbrSep, _, _ := pCreateSolidBrush.Call(uintptr(RGB(40, 40, 46)))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcSep)), hbrSep)
	pDeleteObject.Call(hbrSep)

	// 4. Three large agent toggle buttons
	drawAgentBtn := func(x, y, w, h int32, name string, isOn, isHover bool) {
		btnColor := RGB(220, 53, 69) // Red
		statusText := "○ 已暂停"
		if isOn {
			btnColor = RGB(16, 185, 129) // Green
			statusText = "● 监听中"
		}
		if isHover {
			btnColor = getLighterColor(btnColor)
		}

		hbrBtn, _, _ := pCreateSolidBrush.Call(uintptr(btnColor))
		hNullPen, _, _ := pCreatePen.Call(5, 0, 0) // PS_NULL
		hOldB, _, _ := pSelectObject.Call(hdc, hbrBtn)
		hOldP, _, _ := pSelectObject.Call(hdc, hNullPen)

		pRoundRect.Call(hdc, uintptr(x), uintptr(y), uintptr(x+w), uintptr(y+h), 12, 12)

		pSelectObject.Call(hdc, hOldB)
		pSelectObject.Call(hdc, hOldP)
		pDeleteObject.Call(hbrBtn)
		pDeleteObject.Call(hNullPen)

		// Text
		pSetTextColor.Call(hdc, uintptr(RGB(255, 255, 255)))
		rcName := RECT{x, y + 14, x + w, y + 36}
		DrawText(hdc, name, &rcName, DT_CENTER|DT_SINGLELINE)

		hFontSub, _, _ := pCreateFontW.Call(13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
		pSelectObject.Call(hdc, hFontSub)
		rcStatus := RECT{x, y + 40, x + w, y + 60}
		DrawText(hdc, statusText, &rcStatus, DT_CENTER|DT_SINGLELINE)
		pSelectObject.Call(hdc, hFontBold)
		pDeleteObject.Call(hFontSub)
	}

	drawAgentBtn(20, 56, 106, 72, "OpenCode", app.onOc, app.hoverOc)
	drawAgentBtn(137, 56, 106, 72, "Codex", app.onCx, app.hoverCx)
	drawAgentBtn(254, 56, 106, 72, "Antigravity", app.onAg, app.hoverAg)

	// 5. Status Card (20, 142, 340, 148, #27272C)
	hbrCard, _, _ := pCreateSolidBrush.Call(uintptr(RGB(39, 39, 44)))
	hNullPen, _, _ := pCreatePen.Call(5, 0, 0)
	hOldB, _, _ := pSelectObject.Call(hdc, hbrCard)
	hOldP, _, _ := pSelectObject.Call(hdc, hNullPen)
	pRoundRect.Call(hdc, 20, 142, 360, 290, 12, 12)
	pSelectObject.Call(hdc, hOldB)
	pSelectObject.Call(hdc, hOldP)
	pDeleteObject.Call(hbrCard)
	pDeleteObject.Call(hNullPen)

	hFontBase, _, _ := pCreateFontW.Call(15, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
	pSelectObject.Call(hdc, hFontBase)

	drawRow := func(y int32, label string, isRunning bool) {
		dotColor := RGB(100, 100, 105)
		statusDesc := "未运行"
		if isRunning {
			dotColor = RGB(52, 211, 153)
			statusDesc = "运行中"
		}
		pSetTextColor.Call(hdc, uintptr(dotColor))
		rcDot := RECT{32, y, 52, y + 26}
		DrawText(hdc, "●", &rcDot, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

		pSetTextColor.Call(hdc, uintptr(RGB(244, 244, 245)))
		rcTxt := RECT{58, y, 320, y + 26}
		fullText := fmt.Sprintf("%s  %s", label, statusDesc)
		DrawText(hdc, fullText, &rcTxt, DT_VCENTER|DT_SINGLELINE)
	}

	drawRow(152, "OpenCode", app.procStatus.OpenCodeRunning)
	drawRow(184, "Codex", app.procStatus.CodexRunning)
	drawRow(216, "Antigravity", app.procStatus.AntigravityRunning)

	// Row 4: Last Push & History detail link
	pSetTextColor.Call(hdc, uintptr(RGB(161, 161, 170)))
	rcDotLast := RECT{32, 248, 52, 274}
	DrawText(hdc, "•", &rcDotLast, DT_CENTER|DT_VCENTER|DT_SINGLELINE)

	rcTxtLast := RECT{58, 248, 240, 274}
	lastPushDesc := fmt.Sprintf("上次推送  %s", app.lastPushText)
	DrawText(hdc, lastPushDesc, &rcTxtLast, DT_VCENTER|DT_SINGLELINE)

	// History Link
	lnkColor := RGB(96, 165, 250)
	if app.hoverLnkHist {
		lnkColor = RGB(255, 255, 255)
	}
	pSetTextColor.Call(hdc, uintptr(lnkColor))
	rcLnk := RECT{244, 248, 345, 274}
	DrawText(hdc, "历史详情 ›", &rcLnk, DT_RIGHT|DT_VCENTER|DT_SINGLELINE)

	// 6. Hint Bar (20, 298, 340, 36)
	hFontHint, _, _ := pCreateFontW.Call(13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
	pSelectObject.Call(hdc, hFontHint)
	pSetTextColor.Call(hdc, uintptr(RGB(161, 161, 170)))
	rcHint := RECT{20, 302, 360, 338}
	DrawText(hdc, "三开关独立各管一边 · 原生独立单文件已就绪", &rcHint, DT_VCENTER|DT_SINGLELINE)

	// Separator
	rcSepFoot := RECT{20, 346, 360, 347}
	hbrSep2, _, _ := pCreateSolidBrush.Call(uintptr(RGB(40, 40, 46)))
	pFillRect.Call(hdc, uintptr(unsafe.Pointer(&rcSepFoot)), hbrSep2)
	pDeleteObject.Call(hbrSep2)

	// 7. Footer buttons (20..362)
	hFontFoot, _, _ := pCreateFontW.Call(13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei"))))
	pSelectObject.Call(hdc, hFontFoot)

	// Version text
	pSetTextColor.Call(hdc, uintptr(RGB(120, 120, 130)))
	rcVer := RECT{20, 362, 90, 394}
	DrawText(hdc, "v"+AppVersion, &rcVer, DT_VCENTER|DT_SINGLELINE)

	drawSmallBtn := func(x, y, w, h int32, text string, isHover bool, isDanger bool) {
		btnBg := RGB(39, 39, 44)
		btnFg := RGB(244, 244, 245)
		if isDanger {
			btnBg = RGB(45, 30, 32)
			btnFg = RGB(240, 100, 100)
			if isHover {
				btnBg = RGB(65, 35, 40)
			}
		} else if isHover {
			btnBg = RGB(55, 55, 62)
		}

		hbr, _, _ := pCreateSolidBrush.Call(uintptr(btnBg))
		hNullP, _, _ := pCreatePen.Call(5, 0, 0)
		hOldB2, _, _ := pSelectObject.Call(hdc, hbr)
		hOldP2, _, _ := pSelectObject.Call(hdc, hNullP)
		pRoundRect.Call(hdc, uintptr(x), uintptr(y), uintptr(x+w), uintptr(y+h), 8, 8)
		pSelectObject.Call(hdc, hOldB2)
		pSelectObject.Call(hdc, hOldP2)
		pDeleteObject.Call(hbr)
		pDeleteObject.Call(hNullP)

		pSetTextColor.Call(hdc, uintptr(btnFg))
		rcBtn := RECT{x, y, x + w, y + h}
		DrawText(hdc, text, &rcBtn, DT_CENTER|DT_VCENTER|DT_SINGLELINE)
	}

	drawSmallBtn(96, 362, 58, 32, "历史", app.hoverHist, false)
	drawSmallBtn(162, 362, 58, 32, "设置", app.hoverSet, false)
	drawSmallBtn(228, 362, 58, 32, "测试", app.hoverTest, false)
	drawSmallBtn(294, 362, 66, 32, "退出", app.hoverQuit, true)

	pSelectObject.Call(hdc, hOldFont)
	pDeleteObject.Call(hFontBold)
	pDeleteObject.Call(hFontBase)
	pDeleteObject.Call(hFontHint)
	pDeleteObject.Call(hFontFoot)
}

// ForceForegroundWindow forces a window to front across threads and processes.
func ForceForegroundWindow(hwnd uintptr) {
	debugLog("ForceForegroundWindow enter: hwnd=%d", hwnd)
	if hwnd == 0 {
		return
	}
	pShowWindow.Call(hwnd, SW_SHOW)
	pShowWindow.Call(hwnd, SW_RESTORE)

	hCurFg, _, _ := user32.NewProc("GetForegroundWindow").Call()
	fgThreadId, _, _ := user32.NewProc("GetWindowThreadProcessId").Call(hCurFg, 0)
	targetThreadId, _, _ := user32.NewProc("GetWindowThreadProcessId").Call(hwnd, 0)
	debugLog("ForceForegroundWindow: hCurFg=%d fgThreadId=%d targetThreadId=%d", hCurFg, fgThreadId, targetThreadId)

	if fgThreadId != 0 && targetThreadId != 0 && fgThreadId != targetThreadId {
		pAttachThreadInput := user32.NewProc("AttachThreadInput")
		pAttachThreadInput.Call(fgThreadId, targetThreadId, 1)
		pBringWindowToTop := user32.NewProc("BringWindowToTop")
		pBringWindowToTop.Call(hwnd)
		pSetForegroundWindow.Call(hwnd)
		pAttachThreadInput.Call(fgThreadId, targetThreadId, 0)
	} else {
		pBringWindowToTop := user32.NewProc("BringWindowToTop")
		pBringWindowToTop.Call(hwnd)
		pSetForegroundWindow.Call(hwnd)
	}

	pSetWindowPos.Call(hwnd, uintptr(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW)
	debugLog("ForceForegroundWindow complete")
}
