//go:build windows

package ui

import (
	"context"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agent"
	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/app"
	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/diag"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

const (
	widgetWidth  = int32(400)
	widgetHeight = int32(450)

	WM_USER_REFRESH = WM_USER + 1

	// 自定义 WM_USER 消息统一编号，避免跨文件撞号：
	// +2..+5 归更新检查（update.go）、+6 接入修复完成、+7 扫码配对码；
	// +100 托盘、+200 唤醒。历史遗留的 WM_USER_VERIFY=0x0401 与刷新消息撞号，故改为 +7。
	WM_USER_REPAIR_DONE = WM_USER + 6
	WM_USER_VERIFY      = WM_USER + 7
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

// 整体状态色：窗口顶部状态条与托盘图标共用同一套语义。
var (
	statusColorReady   = RGB(54, 190, 144) // 正常
	statusColorWarning = RGB(224, 165, 70) // 等待用户处理
	statusColorStopped = RGB(220, 92, 92)  // 未登录 / 登录失效 / 全部暂停
)

var (
	classNameWidget   = StringToUTF16Ptr("AgentNotifyWidgetMain")
	windowTitleWidget = StringToUTF16Ptr("Agent-notify")
	msgWakeupID       uint32
	//lint:ignore U1000 Retained so the Windows callback remains reachable.
	wndProcCallback uintptr
)

type WidgetApp struct {
	hwnd                 atomic.Uintptr
	tray                 *TrayManager
	paths                config.Paths
	theme                string
	currentView          WidgetView
	agentDropdownOpen    bool
	dropdownHoverIndex   int
	agentMode            string
	currentAgent         string
	onOpenCode           bool
	onCodex              bool
	onAntigravity        bool
	onDevin              bool
	clawbotLoggedIn      bool
	clawbotSessionReady  bool
	clawbotStale         bool
	clawbotHint          string
	quietHours           string
	replyEnabled         bool
	cooldownMin          int
	lastPushText         string
	lastPushTitle        string
	lastPushSummary      string
	lastPushStatus       string
	lastPushAgent        string
	integrations         map[string]integration.Status
	updateState          widgetUpdateState
	procStatus           ProcessStatus
	hover                widgetHoverState
	repairHover          repairViewHover
	repairError          string
	repairing            bool
	settingsError        string
	historyHover         historyViewHover
	historyConfirmClear  bool
	historyPageOffset    int
	historySelectedIndex int
	historyStamp         string
	historyLimit         int
	historyCache         []notify.HistoryItem
	fonts                widgetFonts
	settingsHover        settingsViewHover
	loginHover           loginViewHover
	loginState           loginDialogState
	quietEdit            uintptr
	cooldownEdit         uintptr
	verifyEdit           uintptr
	verifyPrompt         bool
	editBrush            uintptr
	isTracking           bool
	setupError           string
	repairSetup          func(context.Context) error
	repairDone           chan repairResult
	sessionCancel        context.CancelFunc
}

type WidgetOptions struct {
	InitialSetupError error
	RepairSetup       func(context.Context) error
	ShowLoginOnStart  bool
}

type widgetLayout struct {
	drag         RECT
	themeToggle  RECT
	minimize     RECT
	close        RECT
	modeToggle   RECT
	switchAgent  RECT
	singleSwitch RECT
	singleAgent  RECT
	openCode     RECT
	codex        RECT
	antigravity  RECT
	devin        RECT
	recent       RECT
	test         RECT
	settings     RECT
	history      RECT
	hide         RECT
	update       RECT
	repair       RECT
}

type widgetHoverState struct {
	openCode     bool
	codex        bool
	antigravity  bool
	devin        bool
	singleAgent  bool
	modeToggle   bool
	switchAgent  bool
	singleSwitch bool
	themeToggle  bool
	minimize     bool
	close        bool
	recent       bool
	history      bool
	settings     bool
	test         bool
	hide         bool
	update       bool
	repair       bool
}

func (s widgetHoverState) any() bool {
	return s.openCode || s.codex || s.antigravity || s.devin || s.singleAgent || s.modeToggle || s.switchAgent || s.singleSwitch ||
		s.themeToggle || s.minimize || s.close || s.recent || s.history || s.settings || s.test || s.hide || s.update || s.repair
}

func widgetHoverAt(x, y int32, layout widgetLayout) widgetHoverState {
	return widgetHoverState{
		openCode:     pointInRect(x, y, layout.openCode),
		codex:        pointInRect(x, y, layout.codex),
		antigravity:  pointInRect(x, y, layout.antigravity),
		devin:        pointInRect(x, y, layout.devin),
		singleAgent:  pointInRect(x, y, layout.singleAgent),
		modeToggle:   pointInRect(x, y, layout.modeToggle),
		switchAgent:  pointInRect(x, y, layout.switchAgent),
		singleSwitch: pointInRect(x, y, layout.singleSwitch),
		themeToggle:  pointInRect(x, y, layout.themeToggle),
		minimize:     pointInRect(x, y, layout.minimize),
		close:        pointInRect(x, y, layout.close),
		recent:       pointInRect(x, y, layout.recent),
		history:      pointInRect(x, y, layout.history),
		settings:     pointInRect(x, y, layout.settings),
		test:         pointInRect(x, y, layout.test),
		hide:         pointInRect(x, y, layout.hide),
		update:       pointInRect(x, y, layout.update),
		repair:       pointInRect(x, y, layout.repair),
	}
}

func widgetLayoutRects() widgetLayout {
	return widgetLayout{
		drag:         RECT{0, 0, 308, 48},
		themeToggle:  RECT{312, 12, 338, 38},
		minimize:     RECT{340, 12, 366, 38},
		close:        RECT{368, 12, 394, 38},
		modeToggle:   RECT{270, 54, 386, 72},
		switchAgent:  RECT{76, 92, 256, 128},
		singleSwitch: RECT{326, 98, 372, 122},
		singleAgent:  RECT{14, 78, 386, 222},
		openCode:     RECT{14, 78, 195, 146},
		codex:        RECT{205, 78, 386, 146},
		antigravity:  RECT{14, 154, 195, 222},
		devin:        RECT{205, 154, 386, 222},
		recent:       RECT{14, 232, 386, 318},
		test:         RECT{14, 328, 101, 384},
		history:      RECT{109, 328, 196, 384},
		settings:     RECT{204, 328, 291, 384},
		hide:         RECT{299, 328, 386, 384},
		update:       RECT{106, 394, 242, 436},
		repair:       RECT{250, 394, 386, 436},
	}
}

// widgetTextLayout 集中定义悬浮窗内的文本区域，绘制与布局测试共用，避免文案改宽后溢出。
type widgetTextLayout struct {
	title         RECT
	subtitle      RECT
	footerVersion RECT
	footerUpdate  RECT
	footerHint    RECT
}

func widgetTextRects() widgetTextLayout {
	return widgetTextLayout{
		title:         RECT{14, 12, 230, 32},
		subtitle:      RECT{14, 32, 290, 48},
		footerVersion: RECT{14, 394, 98, 436},
		footerUpdate:  RECT{106, 394, 242, 436},
		footerHint:    RECT{250, 394, 386, 436},
	}
}

func pointInRect(x, y int32, rect RECT) bool {

	return x >= rect.Left && x < rect.Right && y >= rect.Top && y < rect.Bottom
}

func debugLog(format string, args ...interface{}) {
	paths := config.GetPaths()
	entry := fmt.Sprintf("[%s] [PID:%d] %s\r\n", time.Now().Format("15:04:05.000"), os.Getpid(), fmt.Sprintf(format, args...))
	diag.Append(paths.WidgetTraceLog, entry)
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
	instance.sessionCancel = sessionCancel
	defer sessionCancel()
	replyDispatcher := reply.NewDispatcher(reply.DispatcherOptions{
		SendText: reply.NewClawBotTextSender(),
	})
	go clawbot.RunSessionLoop(sessionCtx, replyDispatcher.Handle, func(err error) {
		debugLog("clawbot session loop: %v", err)
		if instance.window() != 0 {
			pPostMessageW.Call(instance.window(), WM_USER_REFRESH, 0, 0)
		}
	})

	if hWakeupEvent != 0 {
		go func() {
			for {
				result, _, _ := pWaitForSingleObject.Call(hWakeupEvent, 500)
				if result == WAIT_OBJECT_0 && instance.window() != 0 {
					pPostMessageW.Call(instance.window(), WM_USER_WAKEUP, 0, 0)
				}
			}
		}()
	}

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	wndProc := syscall.NewCallback(instance.handleMessage)
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
	if instance.theme == "light" {
		darkMode = 0
	}
	pDwmSetWindowAttribute.Call(hwnd, 20, uintptr(unsafe.Pointer(&darkMode)), 4)
	pShowWindow.Call(hwnd, SW_SHOW)
	pUpdateWindow.Call(hwnd)
	ForceForegroundWindow(hwnd)
	if options.ShowLoginOnStart && !clawbot.HasCredentials() {
		instance.switchView(WidgetViewLogin)
	}

	var message MSG
	for {
		result, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if result == 0 || int32(result) == -1 {
			break
		}
		// 配对码输入框聚焦时回车只会送到 EDIT 控件，这里统一转成"提交配对码"。
		if instance.verifyPrompt && message.Message == WM_KEYDOWN && message.WParam == VK_RETURN {
			pPostMessageW.Call(hwnd, WM_COMMAND, uintptr(IDOK), 0)
			continue
		}
		pTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		pDispatchMessageW.Call(uintptr(unsafe.Pointer(&message)))
	}
}

// window 原子读取窗口句柄；唤醒/会话后台 goroutine 与 UI 线程通过它共享 hwnd。
func (app *WidgetApp) window() uintptr {
	return app.hwnd.Load()
}

func (app *WidgetApp) getTheme() ThemePalette {
	return GetTheme(app.theme)
}

func (app *WidgetApp) toggleTheme() {
	if app.theme == "light" {
		app.theme = "dark"
	} else {
		app.theme = "light"
	}
	app.mutateConfig(func(cfg *config.AppConfig) {
		cfg.Theme = app.theme
	})
	app.applyThemeToWindow()
	if app.quietEdit != 0 {
		pInvalidateRect.Call(app.quietEdit, 0, 1)
	}
	if app.cooldownEdit != 0 {
		pInvalidateRect.Call(app.cooldownEdit, 0, 1)
	}
	pInvalidateRect.Call(app.window(), 0, 0)
}

func (app *WidgetApp) applyThemeToWindow() {
	if app.window() == 0 {
		return
	}
	darkMode := uint32(1)
	if app.theme == "light" {
		darkMode = 0
	}
	pDwmSetWindowAttribute.Call(app.window(), 20, uintptr(unsafe.Pointer(&darkMode)), 4)
}

func (app *WidgetApp) switchView(view WidgetView) {
	if app.currentView == view {
		return
	}
	if app.currentView == WidgetViewSettings {
		if app.quietEdit != 0 {
			pShowWindow.Call(app.quietEdit, SW_HIDE)
		}
		if app.cooldownEdit != 0 {
			pShowWindow.Call(app.cooldownEdit, SW_HIDE)
		}
	}
	if app.currentView == WidgetViewLogin {
		// 离开登录视图要真正结束扫码流程，否则后台会继续轮询并可能在没有界面的情况下写凭据。
		app.hideVerifyPrompt()
		app.loginState.cancel()
	}

	app.currentView = view
	app.agentDropdownOpen = false

	switch view {
	case WidgetViewRepair:
		app.refreshState()
	case WidgetViewSettings:
		if cfg, err := config.LoadConfig(""); err == nil {
			app.quietHours = cfg.QuietHours
			app.cooldownMin = cfg.CooldownMin
			app.replyEnabled = cfg.ReplyEnabled
		}
		app.settingsError = ""
		app.ensureSettingsEdits()
		if app.quietEdit != 0 {
			setWindowText(app.quietEdit, app.quietHours)
			pShowWindow.Call(app.quietEdit, SW_SHOW)
		}
		if app.cooldownEdit != 0 {
			setWindowText(app.cooldownEdit, strconv.Itoa(app.cooldownMin))
			pShowWindow.Call(app.cooldownEdit, SW_SHOW)
		}
	case WidgetViewLogin:
		if !app.clawbotLoggedIn {
			app.startInAppLoginFlow()
		}
	case WidgetViewHistory:
		app.historyConfirmClear = false
		app.historyPageOffset = 0
		app.historySelectedIndex = 0
	}

	if app.window() != 0 {
		pInvalidateRect.Call(app.window(), 0, 0)
	}
}

func (app *WidgetApp) ensureSettingsEdits() {
	if app.window() == 0 {
		return
	}
	hInstance, _, _ := pGetModuleHandleW.Call(0)
	font := newBaseFont()
	if app.quietEdit == 0 {
		rect := scaleRect(RECT{246, 126, 372, 150})
		app.quietEdit, _, _ = pCreateWindowExW.Call(
			0,
			uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
			uintptr(unsafe.Pointer(StringToUTF16Ptr(app.quietHours))),
			WS_CHILD|ES_AUTOHSCROLL,
			uintptr(rect.Left), uintptr(rect.Top),
			uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top),
			app.window(), uintptr(IDC_SETTINGS_QUIET), hInstance, 0,
		)
		pSendMessageW.Call(app.quietEdit, WM_SETFONT, font, 1)
	}
	if app.cooldownEdit == 0 {
		rect := scaleRect(RECT{246, 180, 372, 204})
		app.cooldownEdit, _, _ = pCreateWindowExW.Call(
			0,
			uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
			uintptr(unsafe.Pointer(StringToUTF16Ptr(strconv.Itoa(app.cooldownMin)))),
			WS_CHILD|ES_AUTOHSCROLL|ES_NUMBER,
			uintptr(rect.Left), uintptr(rect.Top),
			uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top),
			app.window(), uintptr(IDC_SETTINGS_COOLDOWN), hInstance, 0,
		)
		pSendMessageW.Call(app.cooldownEdit, WM_SETFONT, font, 1)
	}
}

func (app *WidgetApp) startInAppLoginFlow() {
	app.hideVerifyPrompt()
	startLoginFlow(&app.loginState)
}

// ensureVerifyEdit 懒创建扫码配对码输入框，样式与设置页输入框保持一致。
func (app *WidgetApp) ensureVerifyEdit() {
	if app.window() == 0 || app.verifyEdit != 0 {
		return
	}
	rect := verifyEditRect()
	hInstance, _, _ := pGetModuleHandleW.Call(0)
	app.verifyEdit, _, _ = pCreateWindowExW.Call(
		0,
		uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
		0,
		WS_CHILD|ES_AUTOHSCROLL|ES_NUMBER,
		uintptr(rect.Left), uintptr(rect.Top),
		uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top),
		app.window(), uintptr(loginCodeEditID), hInstance, 0,
	)
	if app.verifyEdit == 0 {
		return
	}
	pSendMessageW.Call(app.verifyEdit, WM_SETFONT, newBaseFont(), 1)
	pSendMessageW.Call(app.verifyEdit, EM_SETCUEBANNER, 0, uintptr(unsafe.Pointer(StringToUTF16Ptr("输入微信配对码"))))
}

// showVerifyPrompt 展示配对码输入框并聚焦，等待用户输入。
func (app *WidgetApp) showVerifyPrompt(hwnd uintptr) {
	app.ensureVerifyEdit()
	app.verifyPrompt = true
	if app.verifyEdit != 0 {
		setWindowText(app.verifyEdit, "")
		pShowWindow.Call(app.verifyEdit, SW_SHOW)
		pSetFocus.Call(app.verifyEdit)
	}
	pInvalidateRect.Call(hwnd, 0, 0)
}

// submitVerifyCode 读取输入并交回登录流程；输入为空时不打断提示。
func (app *WidgetApp) submitVerifyCode(hwnd uintptr) {
	if !app.verifyPrompt {
		return
	}
	code := ""
	if app.verifyEdit != 0 {
		code = strings.TrimSpace(getWindowText(app.verifyEdit))
	}
	if code == "" {
		return
	}
	app.hideVerifyPrompt()
	app.loginState.submitVerifyCode(code)
	pInvalidateRect.Call(hwnd, 0, 0)
}

// hideVerifyPrompt 只隐藏输入框，不取消登录流程（取消由 switchView 负责）。
func (app *WidgetApp) hideVerifyPrompt() {
	app.verifyPrompt = false
	if app.verifyEdit != 0 {
		pShowWindow.Call(app.verifyEdit, SW_HIDE)
	}
}

// applyRepairDone 在 UI 线程消费修复结果，避免工作线程直接改界面状态。
func (app *WidgetApp) applyRepairDone() {
	if app.repairDone == nil {
		return
	}
	select {
	case result := <-app.repairDone:
		app.repairing = false
		if !result.setupFailed {
			app.setupError = ""
		}
		if len(result.errors) > 0 {
			app.repairError = strings.Join(result.errors, "；")
		} else {
			app.repairError = ""
		}
	default:
	}
	app.repairDone = nil
}

func (app *WidgetApp) loadHistory(limit int) []notify.HistoryItem {
	if limit <= 0 {
		limit = 1
	}
	stamp := historyFileStamp(app.paths.PushLog)
	if stamp != "" && stamp == app.historyStamp && app.historyLimit >= limit && app.historyCache != nil {
		return app.historyCache
	}
	items, err := notify.GetHistory(limit, app.paths.PushLog)
	if err != nil {
		debugLog("load history: %v", err)
		// 读取失败时返回空列表：不把旧缓存或截断内容伪装成有效历史。
		items = nil
	}
	if items == nil {
		items = []notify.HistoryItem{}
	}
	app.historyStamp = stamp
	app.historyLimit = limit
	app.historyCache = items
	return items
}

// historyFileStamp 用大小 + 修改时间标记日志；文件不存在时返回空串（此时每次都重读，代价可忽略）。
func historyFileStamp(logPath string) string {
	info, err := os.Stat(logPath)
	if err != nil {
		return ""
	}
	return fmt.Sprintf("%d:%d", info.Size(), info.ModTime().UnixNano())
}

// widgetFonts 缓存按 DPI 创建的字体系列：绘制路径每帧都要用字体，
// 缓存后不再每帧创建/销毁 5 个 GDI 字体对象，DPI 变化时重建。
type widgetFonts struct {
	title  uintptr
	base   uintptr
	strong uintptr
	small  uintptr
	icon   uintptr
	dpi    uint32
}

func (app *WidgetApp) uiFonts() widgetFonts {
	if app.fonts.title != 0 && app.fonts.dpi == uiDPI {
		return app.fonts
	}
	app.releaseFonts()
	app.fonts = widgetFonts{
		title:  newTitleFont(),
		base:   newBaseFont(),
		strong: newStrongFont(),
		small:  newSmallFont(),
		icon:   newUIIconFont(),
		dpi:    uiDPI,
	}
	return app.fonts
}

func (app *WidgetApp) releaseFonts() {
	for _, font := range []uintptr{app.fonts.title, app.fonts.base, app.fonts.strong, app.fonts.small, app.fonts.icon} {
		if font != 0 {
			pDeleteObject.Call(font)
		}
	}
	app.fonts = widgetFonts{}
}

// mutateConfig 读取-修改-保存配置：读取失败时绝不覆盖现有配置（避免用默认值冲掉用户文件），
// 保存失败会把原因直接告诉用户。
func (app *WidgetApp) mutateConfig(apply func(*config.AppConfig)) bool {
	cfg, err := config.LoadConfig("")
	if err != nil {
		app.reportActionError("读取设置失败，本次修改未保存：%v", err)
		return false
	}
	apply(&cfg)
	if err := config.SaveConfig(cfg, ""); err != nil {
		app.reportActionError("保存设置失败：%v", err)
		return false
	}
	return true
}

// reportActionError 把用户操作中的失败明确暴露出来（消息框 + 调试日志）。
func (app *WidgetApp) reportActionError(format string, args ...interface{}) {
	message := fmt.Sprintf(format, args...)
	debugLog("action error: %s", message)
	if app.window() != 0 {
		showMessage(app.window(), message, MB_ICONINFO)
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
		app.replyEnabled = cfg.ReplyEnabled
		app.cooldownMin = cfg.CooldownMin
		if app.theme == "" {
			app.theme = cfg.Theme
			if app.theme == "" {
				app.theme = "dark"
			}
		}
		if app.agentMode == "" {
			if cfg.WidgetAgentMode != "" {
				app.agentMode = cfg.WidgetAgentMode
			} else {
				app.agentMode = "single"
			}
		}
		if app.currentAgent == "" {
			if cfg.DefaultAgent != "" {
				app.currentAgent = cfg.DefaultAgent
			} else {
				app.currentAgent = agentmeta.Antigravity
			}
		}
	}
	app.procStatus = DetectProcesses()
	history := app.loadHistory(1)
	if len(history) > 0 {
		item := history[0]
		app.lastPushTitle = truncateUI(item.Title, 28)
		app.lastPushSummary = truncateUI(item.Summary, 45)
		app.lastPushStatus = item.Status
		app.lastPushAgent = item.Agent
		app.lastPushText = relativeHistoryTime(item)
	} else {
		app.lastPushTitle = "暂无推送记录"
		app.lastPushSummary = "等待 Agent 产生第一条任务推送"
		app.lastPushStatus = ""
		app.lastPushAgent = ""
		app.lastPushText = "暂无记录"
	}

	if app.tray != nil {
		statusColor, _ := app.health()
		app.tray.UpdateState(trayStateForStatus(statusColor))
	}
}

func (app *WidgetApp) health() (uint32, string) {
	if app.setupError != "" {
		return statusColorWarning, "接入异常"
	}
	if !app.clawbotLoggedIn {
		return statusColorStopped, "未登录"
	}
	if app.clawbotStale {
		return statusColorStopped, "登录已失效"
	}
	if !app.clawbotSessionReady {
		return statusColorWarning, "等待微信消息"
	}
	errors, restarts, missing := app.agentIntegrationCounts()
	if errors > 0 || missing > 0 {
		return statusColorWarning, "接入异常"
	}
	if restarts > 0 {
		return statusColorWarning, "待重启"
	}
	if app.enabledIntegrationsReady() {
		return statusColorReady, "正常"
	}
	if app.anyAgentEnabled() {
		return statusColorWarning, "等待 Agent"
	}
	return statusColorStopped, "全部暂停"
}

// trayStateForStatus 把整体状态色映射为托盘图标状态，保证托盘与窗口状态条语义一致。
func trayStateForStatus(statusColor uint32) int {
	switch statusColor {
	case statusColorReady:
		return widgetTrayStateReady
	case statusColorWarning:
		return widgetTrayStatePartial
	default:
		return widgetTrayStateStopped
	}
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

func (app *WidgetApp) isSingleAgentMode() bool {
	return app.agentMode == "single"
}

func (app *WidgetApp) focusedAgentID() string {
	switch app.currentAgent {
	case agentmeta.OpenCode, agentmeta.Codex, agentmeta.Antigravity, agentmeta.Devin:
		return app.currentAgent
	default:
		return agentmeta.Antigravity
	}
}

func (app *WidgetApp) nextAgent() {
	agents := []string{agentmeta.Antigravity, agentmeta.OpenCode, agentmeta.Codex, agentmeta.Devin}
	for i, a := range agents {
		if a == app.currentAgent {
			app.currentAgent = agents[(i+1)%len(agents)]
			app.mutateConfig(func(cfg *config.AppConfig) {
				cfg.DefaultAgent = app.currentAgent
			})
			return
		}
	}
	app.currentAgent = agentmeta.Antigravity
	app.mutateConfig(func(cfg *config.AppConfig) {
		cfg.DefaultAgent = app.currentAgent
	})
}

func (app *WidgetApp) toggleAgentMode() {
	if app.agentMode == "single" {
		app.agentMode = "grid"
	} else {
		app.agentMode = "single"
	}
	app.mutateConfig(func(cfg *config.AppConfig) {
		cfg.WidgetAgentMode = app.agentMode
	})
}

func (app *WidgetApp) focusedAgentHealth() (uint32, string) {
	agentID := app.focusedAgentID()
	if !app.agentEnabled(agentID) {
		return RGB(120, 132, 143), "已暂停"
	}
	status := app.integrationStatus(agentID)
	switch status.State {
	case integration.StateConnected:
		if app.agentRunning(agentID) {
			return RGB(54, 190, 144), "正常"
		}
		return RGB(54, 190, 144), "已接入"
	case integration.StatePendingRestart:
		return RGB(224, 165, 70), "待重启"
	case integration.StateError:
		return RGB(224, 104, 104), "异常"
	default:
		if app.agentRunning(agentID) {
			return RGB(224, 165, 70), "未接入"
		}
		return RGB(138, 150, 161), "未配置"
	}
}

func (app *WidgetApp) currentHealth() (uint32, string) {
	if app.isSingleAgentMode() {
		return app.focusedAgentHealth()
	}
	return app.health()
}

func (app *WidgetApp) showAgentDropdown(hwnd uintptr, rect RECT) {
	app.agentDropdownOpen = !app.agentDropdownOpen
	pInvalidateRect.Call(hwnd, 0, 0)
}
