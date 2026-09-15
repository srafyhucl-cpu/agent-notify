//go:build windows

package ui

import (
	"context"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agent"
	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/app"
	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

const (
	widgetWidth  = int32(400)
	widgetHeight = int32(450)

	WM_USER_REFRESH = WM_USER + 1
)

const (
	widgetScreenInset   = int32(30)
	widgetMinimumMargin = int32(20)
)

const (
	widgetTrayStateStopped = iota
	widgetTrayStatePartial
	widgetTrayStateReady
)

var (
	classNameWidget   = StringToUTF16Ptr("AgentNotifyWidgetMain")
	windowTitleWidget = StringToUTF16Ptr("Agent-notify")
	msgWakeupID       uint32
	//lint:ignore U1000 Retained so the Windows callback remains reachable.
	wndProcCallback uintptr
)

type WidgetApp struct {
	hwnd                uintptr
	tray                *TrayManager
	paths               config.Paths
	onOpenCode          bool
	onCodex             bool
	onAntigravity       bool
	onDevin             bool
	clawbotLoggedIn     bool
	clawbotSessionReady bool
	clawbotStale        bool
	clawbotHint         string
	quietHours          string
	lastPushText        string
	lastPushTitle       string
	lastPushStatus      string
	lastPushAgent       string
	integrations        map[string]integration.Status
	updateState         widgetUpdateState
	procStatus          ProcessStatus
	hover               widgetHoverState
	isTracking          bool
	setupError          string
	repairSetup         func(context.Context) error
}

type WidgetOptions struct {
	InitialSetupError error
	RepairSetup       func(context.Context) error
}

type widgetLayout struct {
	drag        RECT
	minimize    RECT
	close       RECT
	connection  RECT
	openCode    RECT
	codex       RECT
	antigravity RECT
	devin       RECT
	recent      RECT
	test        RECT
	settings    RECT
	history     RECT
	hide        RECT
	update      RECT
	repair      RECT
}

type widgetHoverState struct {
	openCode    bool
	codex       bool
	antigravity bool
	devin       bool
	minimize    bool
	close       bool
	connection  bool
	recent      bool
	history     bool
	settings    bool
	test        bool
	hide        bool
	update      bool
	repair      bool
}

func (s widgetHoverState) any() bool {
	return s.openCode || s.codex || s.antigravity || s.devin || s.minimize || s.close || s.connection ||
		s.recent || s.history || s.settings || s.test || s.hide || s.update || s.repair
}

func widgetHoverAt(x, y int32, layout widgetLayout) widgetHoverState {
	return widgetHoverState{
		openCode:    pointInRect(x, y, layout.openCode),
		codex:       pointInRect(x, y, layout.codex),
		antigravity: pointInRect(x, y, layout.antigravity),
		devin:       pointInRect(x, y, layout.devin),
		minimize:    pointInRect(x, y, layout.minimize),
		close:       pointInRect(x, y, layout.close),
		connection:  pointInRect(x, y, layout.connection),
		recent:      pointInRect(x, y, layout.recent),
		history:     pointInRect(x, y, layout.history),
		settings:    pointInRect(x, y, layout.settings),
		test:        pointInRect(x, y, layout.test),
		hide:        pointInRect(x, y, layout.hide),
		update:      pointInRect(x, y, layout.update),
		repair:      pointInRect(x, y, layout.repair),
	}
}

func widgetLayoutRects() widgetLayout {
	return widgetLayout{
		drag:        RECT{0, 0, 240, 48},
		minimize:    RECT{344, 4, 372, 36},
		close:       RECT{372, 4, 400, 36},
		connection:  RECT{14, 56, 386, 112},
		openCode:    RECT{14, 136, 193, 212},
		codex:       RECT{207, 136, 386, 212},
		antigravity: RECT{14, 218, 193, 294},
		devin:       RECT{207, 218, 386, 294},
		recent:      RECT{14, 304, 386, 366},
		test:        RECT{14, 380, 132, 420},
		settings:    RECT{140, 380, 238, 420},
		history:     RECT{246, 380, 314, 420},
		hide:        RECT{322, 380, 386, 420},
		update:      RECT{80, 426, 250, 448},
		repair:      RECT{258, 426, 386, 448},
	}
}

// widgetTextLayout 集中定义悬浮窗内的文本区域，绘制与布局测试共用，避免文案改宽后溢出。
type widgetTextLayout struct {
	title            RECT
	subtitle         RECT
	connectionTitle  RECT
	connectionDetail RECT
	quiet            RECT
	recentLabel      RECT
	recentMeta       RECT
	recentTitle      RECT
	footerVersion    RECT
	footerUpdate     RECT
	footerHint       RECT
}

func widgetTextRects() widgetTextLayout {
	return widgetTextLayout{
		title:            RECT{14, 7, 230, 36},
		subtitle:         RECT{15, 31, 250, 50},
		connectionTitle:  RECT{46, 62, 286, 84},
		connectionDetail: RECT{46, 84, 300, 103},
		quiet:            RECT{286, 74, 372, 94},
		recentLabel:      RECT{28, 312, 110, 330},
		recentMeta:       RECT{120, 311, 370, 330},
		recentTitle:      RECT{28, 332, 370, 356},
		footerVersion:    RECT{14, 432, 70, 448},
		footerUpdate:     RECT{104, 432, 238, 448},
		footerHint:       RECT{254, 432, 386, 448},
	}
}

func pointInRect(x, y int32, rect RECT) bool {

	return x >= rect.Left && x < rect.Right && y >= rect.Top && y < rect.Bottom
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
	return resolveWidgetPositionInArea(raw, RECT{Right: screenWidth, Bottom: screenHeight}, winWidth, winHeight)
}

func resolveWidgetPositionInArea(raw string, workArea RECT, winWidth, winHeight int32) (int32, int32) {
	defaultX := workArea.Right - winWidth - widgetScreenInset
	defaultY := workArea.Top + (workArea.Bottom-workArea.Top-winHeight)/2
	defaultX, defaultY = clampWidgetPosition(defaultX, defaultY, winWidth, winHeight, workArea)
	parts := strings.Split(strings.TrimSpace(raw), ",")
	if len(parts) != 2 {
		return defaultX, defaultY
	}
	x, errX := strconv.Atoi(strings.TrimSpace(parts[0]))
	y, errY := strconv.Atoi(strings.TrimSpace(parts[1]))
	if errX != nil || errY != nil {
		return defaultX, defaultY
	}
	if int32(x) < workArea.Left-winWidth/2 || int32(y) < workArea.Top-winHeight/2 ||
		int32(x) > workArea.Right-winWidth/2 || int32(y) > workArea.Bottom-winHeight/2 {
		return defaultX, defaultY
	}
	return clampWidgetPosition(int32(x), int32(y), winWidth, winHeight, workArea)
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

func relativeHistoryTime(item notify.HistoryItem) string {
	return relativeHistoryTimeAt(item, time.Now())
}

func relativeHistoryTimeAt(item notify.HistoryItem, now time.Time) string {
	timestamp := item.LocalTime()
	if timestamp.IsZero() {
		return truncateUI(item.Timestamp, 16)
	}
	delta := now.Sub(timestamp)
	switch {
	case delta < time.Minute:
		return "刚刚"
	case delta < time.Hour:
		return fmt.Sprintf("%d 分钟前", int(delta.Minutes()))
	case timestamp.Year() == now.Year() && timestamp.YearDay() == now.YearDay():
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
	size = scaleFloat(size)
	font, _, _ := pCreateFontW.Call(
		uintptr(size), 0, 0, 0, uintptr(weight), 0, 0, 0, 1, 0, 0, 0, 0,
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Microsoft YaHei UI"))),
	)
	return font
}

func fillRoundRect(hdc uintptr, rect RECT, radius, color uintptr) {
	scaled := scaleRect(rect)
	scaledRadius := uintptr(scaleFloat(int32(radius)))
	brush, _, _ := pCreateSolidBrush.Call(color)
	nullPen, _, _ := pCreatePen.Call(5, 0, 0)
	oldBrush, _, _ := pSelectObject.Call(hdc, brush)
	oldPen, _, _ := pSelectObject.Call(hdc, nullPen)
	pRoundRect.Call(hdc, uintptr(scaled.Left), uintptr(scaled.Top), uintptr(scaled.Right), uintptr(scaled.Bottom), scaledRadius, scaledRadius)
	pSelectObject.Call(hdc, oldBrush)
	pSelectObject.Call(hdc, oldPen)
	pDeleteObject.Call(brush)
	pDeleteObject.Call(nullPen)
}

// RunWidget starts the native Windows GUI widget.
func RunWidget(options WidgetOptions) {
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

	instance := &WidgetApp{
		paths:       paths,
		repairSetup: options.RepairSetup,
	}
	if options.InitialSetupError != nil {
		instance.setupError = options.InitialSetupError.Error()
	}

	sessionCtx, sessionCancel := context.WithCancel(context.Background())
	defer sessionCancel()
	replyDispatcher := reply.NewDispatcher(reply.DispatcherOptions{
		SendText: reply.NewClawBotTextSender(),
	})
	go clawbot.RunSessionLoop(sessionCtx, replyDispatcher.Handle, func(err error) {
		debugLog("clawbot session loop: %v", err)
		if instance.hwnd != 0 {
			pPostMessageW.Call(instance.hwnd, WM_USER_REFRESH, 0, 0)
		}
	})

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
		if message == WM_USER_REFRESH {
			instance.refreshState()
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0
		}
		if instance.handleUpdateMessage(hwnd, message) {
			return 0
		}

		switch message {
		case WM_CREATE:
			setUIDPI(windowDPI(hwnd))
			resizeForCurrentDPI(hwnd, widgetWidth, widgetHeight)
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

		case WM_DPICHANGED:
			setUIDPI(uint32(wParam & 0xFFFF))
			resizeForCurrentDPI(hwnd, widgetWidth, widgetHeight)
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_ERASEBKGND:
			return 1

		case WM_PAINT:
			paintDoubleBuffered(hwnd, func(hdc uintptr, width, height int32) {
				drawUI(hdc, width, height, instance)
			})
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

			x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
			layout := widgetLayoutRects()
			previous := instance.hover
			instance.hover = widgetHoverAt(x, y, layout)
			if instance.hover != previous {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if instance.hover.any() {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WM_MOUSELEAVE:
			instance.isTracking = false
			instance.hover = widgetHoverState{}
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)
			layout := widgetLayoutRects()
			x, y = unscalePoint(x, y)
			if pointInRect(x, y, layout.drag) && !pointInRect(x, y, layout.minimize) && !pointInRect(x, y, layout.close) {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, paths.WidgetPosFile)
				return 0
			}
			if pointInRect(x, y, layout.minimize) || pointInRect(x, y, layout.close) {
				savePosition(hwnd, paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
				return 0
			}
			if pointInRect(x, y, layout.connection) {
				ShowSettingsDialog(hwnd)
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if instance.toggleAgentAt(x, y, layout) {
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if pointInRect(x, y, layout.recent) || pointInRect(x, y, layout.history) {
				ShowHistoryDialog(hwnd)
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if pointInRect(x, y, layout.settings) {
				ShowSettingsDialog(hwnd)
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if pointInRect(x, y, layout.test) {
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
			if pointInRect(x, y, layout.repair) {
				instance.repairIntegrations(hwnd)
				return 0
			}
			if pointInRect(x, y, layout.update) {
				instance.handleUpdateClick(hwnd)
				return 0
			}
			if pointInRect(x, y, layout.hide) {
				savePosition(hwnd, paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
				return 0
			}
			return 0

		case WM_TRAYICON:
			switch lParam {
			case WM_LBUTTONDBLCLK:
				restoreAndBringToFront(hwnd)
			case WM_RBUTTONUP:
				visible, _, _ := user32.NewProc("IsWindowVisible").Call(hwnd)
				instance.tray.ShowContextMenu(visible != 0, instance.enabledAgentStates())
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
			case IDM_TOGGLE_OPENCODE, IDM_TOGGLE_CODEX, IDM_TOGGLE_ANTIGRAVITY, IDM_TOGGLE_DEVIN:
				if agentID, ok := trayAgentIDForCommand(int(wParam & 0xFFFF)); ok && instance.toggleAgent(agentID) {
					instance.refreshState()
					pInvalidateRect.Call(hwnd, 0, 0)
				}
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
			case IDM_UPDATE:
				instance.startUpdateCheck(hwnd)
			case IDM_EXIT:
				savePosition(hwnd, paths.WidgetPosFile)
				_ = os.WriteFile(paths.WidgetExitMarker, []byte(time.Now().Format(time.RFC3339)), 0600)
				sessionCancel()
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
			sessionCancel()
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

	workArea := widgetWorkArea(0)
	setUIDPI(systemDPI())
	rawPosition := ""
	if data, err := os.ReadFile(paths.WidgetPosFile); err == nil {
		rawPosition = string(data)
	}
	winWidth, winHeight := logicalSize(widgetWidth, widgetHeight)
	winX, winY := resolveWidgetPositionInArea(rawPosition, workArea, winWidth, winHeight)
	debugLog("widget placement raw=%q work=(%d,%d,%d,%d) dpi=%d size=%dx%d pos=(%d,%d)", rawPosition, workArea.Left, workArea.Top, workArea.Right, workArea.Bottom, uiDPI, winWidth, winHeight, winX, winY)

	hwnd, _, createErr := pCreateWindowExW.Call(
		widgetExtendedStyle(),
		uintptr(unsafe.Pointer(classNameWidget)),
		uintptr(unsafe.Pointer(windowTitleWidget)),
		WS_POPUP|WS_MINIMIZEBOX|WS_SYSMENU|WS_VISIBLE,
		uintptr(winX), uintptr(winY), uintptr(winWidth), uintptr(winHeight),
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
	app.refreshAgentSwitches()
	clawbotStatus := clawbot.GetStatus()
	app.clawbotLoggedIn = clawbotStatus.LoggedIn
	app.clawbotSessionReady = clawbotStatus.SessionReady
	app.clawbotStale = clawbotStatus.Stale
	app.clawbotHint = clawbotStatus.UserHint
	if cfg, err := config.LoadConfig(""); err == nil {
		app.quietHours = cfg.QuietHours
	}
	app.procStatus = DetectProcesses()
	history, _ := notify.GetHistory(1, app.paths.PushLog)
	if len(history) > 0 {
		item := history[0]
		app.lastPushTitle = truncateUI(item.Title, 28)
		app.lastPushStatus = item.Status
		app.lastPushAgent = item.Agent
		app.lastPushText = relativeHistoryTime(item)
	} else {
		app.lastPushTitle = "暂无推送记录"
		app.lastPushStatus = ""
		app.lastPushAgent = ""
		app.lastPushText = "暂无记录"
	}

	state := widgetTrayStateStopped
	ready := app.clawbotLoggedIn && app.clawbotSessionReady
	if ready && app.enabledIntegrationsReady() {
		state = widgetTrayStateReady
	} else if ready && app.anyAgentEnabled() {
		state = widgetTrayStatePartial
	}
	if app.tray != nil {
		app.tray.UpdateState(state)
	}
}

func (app *WidgetApp) health() (uint32, string) {
	if app.setupError != "" {
		return RGB(224, 165, 70), "接入异常"
	}
	if !app.clawbotLoggedIn {
		return RGB(220, 92, 92), "未登录"
	}
	if app.clawbotStale {
		return RGB(220, 92, 92), "登录已失效"
	}
	if !app.clawbotSessionReady {
		return RGB(224, 165, 70), "等待微信消息"
	}
	errors, restarts, missing := app.agentIntegrationCounts()
	if errors > 0 || missing > 0 {
		return RGB(224, 165, 70), "接入异常"
	}
	if restarts > 0 {
		return RGB(224, 165, 70), "待重启"
	}
	if app.enabledIntegrationsReady() {
		return RGB(54, 190, 144), "正常"
	}
	if app.anyAgentEnabled() {
		return RGB(224, 165, 70), "等待 Agent"
	}
	return RGB(220, 92, 92), "全部暂停"
}

func (app *WidgetApp) repairIntegrations(hwnd uintptr) {
	executable, _ := os.Executable()
	failures := make([]string, 0)
	if app.repairSetup != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
		err := app.repairSetup(ctx)
		cancel()
		if err != nil {
			failures = append(failures, "首次接入："+err.Error())
		} else {
			app.setupError = ""
		}
	}
	for _, status := range app.integrations {
		if status.Fixable && status.Repair == integration.RepairCodexWatch {
			if err := agent.HandleWatch(app.paths.CodexConfig, executable); err != nil {
				failures = append(failures, status.Name+"："+err.Error())
			}
		}
	}
	app.refreshState()

	lines := make([]string, 0)
	if len(failures) > 0 {
		lines = append(lines, "修复失败：", strings.Join(failures, "\n"), "")
	}
	lines = append(lines, "接入检查完成：")
	for _, descriptor := range agentmeta.All() {
		status := app.integrationStatus(descriptor.ID)
		line := fmt.Sprintf("%s：%s", descriptor.DisplayName, status.Label())
		if status.Detail != "" {
			line += " - " + status.Detail
		}
		lines = append(lines, line)
		if status.Action != "" && status.State != integration.StateConnected {
			lines = append(lines, "  "+status.Action)
		}
	}
	message := strings.Join(lines, "\n")
	pMessageBoxW.Call(hwnd, uintptr(unsafe.Pointer(StringToUTF16Ptr(message))), uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify 接入检查"))), MB_OK|MB_ICONINFO)
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

// widgetExtendedStyle keeps the floating widget out of the taskbar and out of
// Alt+Tab while it stays topmost. The tray icon remains the way back in, so a
// second taskbar button would only duplicate the entry the user already has.
func widgetExtendedStyle() uintptr {
	return WS_EX_TOOLWINDOW | WS_EX_TOPMOST
}
