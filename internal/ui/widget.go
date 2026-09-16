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
	hwnd                 uintptr
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
		title:            RECT{14, 12, 230, 32},
		subtitle:         RECT{14, 32, 290, 48},
		connectionTitle:  RECT{46, 62, 320, 84},
		connectionDetail: RECT{46, 84, 320, 103},
		quiet:            RECT{270, 54, 386, 72},
		recentLabel:      RECT{26, 238, 100, 260},
		recentMeta:       RECT{160, 238, 374, 260},
		recentTitle:      RECT{26, 268, 354, 308},
		footerVersion:    RECT{14, 394, 98, 436},
		footerUpdate:     RECT{106, 394, 242, 436},
		footerHint:       RECT{250, 394, 386, 436},
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
		if message == WM_USER_VERIFY {
			// 用户可能已经离开登录视图，此时忽略配对码请求。
			if instance.currentView == WidgetViewLogin {
				instance.showVerifyPrompt(hwnd)
			}
			return 0
		}
		if message == WM_USER_REPAIR_DONE {
			instance.applyRepairDone()
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
			instance.loginState.setWindow(hwnd)
			instance.tray = NewTrayManager(hwnd)
			instance.refreshState()
			instance.applyThemeToWindow()
			pSetTimer.Call(hwnd, 1, 5000, 0)
			return 0

		case WM_ACTIVATE:
			if (wParam & 0xFFFF) == 0 { // WA_INACTIVE
				if instance.agentDropdownOpen {
					instance.agentDropdownOpen = false
					pInvalidateRect.Call(hwnd, 0, 0)
				}
			}
			return 0

		case WM_KILLFOCUS:
			if instance.agentDropdownOpen {
				instance.agentDropdownOpen = false
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			return 0

		case WM_MOUSEWHEEL:
			if instance.currentView == WidgetViewHistory {
				delta := int16((wParam >> 16) & 0xFFFF)
				historyItems := instance.loadHistory(historyListPageSize * 10)
				total := len(historyItems)
				const pageSize = historyListPageSize
				if delta > 0 {
					if instance.historyPageOffset >= pageSize {
						instance.historyPageOffset -= pageSize
						instance.historySelectedIndex = instance.historyPageOffset
						pInvalidateRect.Call(hwnd, 0, 0)
					}
				} else if delta < 0 {
					if instance.historyPageOffset+pageSize < total {
						instance.historyPageOffset += pageSize
						instance.historySelectedIndex = instance.historyPageOffset
						pInvalidateRect.Call(hwnd, 0, 0)
					}
				}
				return 0
			}

		case WM_KEYDOWN:
			if wParam == VK_ESCAPE {
				if instance.agentDropdownOpen {
					instance.agentDropdownOpen = false
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				if instance.currentView != WidgetViewDashboard {
					instance.switchView(WidgetViewDashboard)
					return 0
				}
			}
			return 0

		case WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC:
			theme := instance.getTheme()
			editHdc := wParam
			pSetTextColor.Call(editHdc, uintptr(theme.TextPrimary))
			pSetBkColor.Call(editHdc, uintptr(theme.InputBg))
			if instance.editBrush != 0 {
				pDeleteObject.Call(instance.editBrush)
			}
			instance.editBrush, _, _ = pCreateSolidBrush.Call(uintptr(theme.InputBg))
			return instance.editBrush

		case WM_TIMER:
			instance.refreshState()
			_ = os.WriteFile(paths.WidgetAliveFile, []byte(time.Now().Format(time.RFC3339)), 0600)
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_DPICHANGED:
			setUIDPI(uint32(wParam & 0xFFFF))
			resizeForCurrentDPI(hwnd, widgetWidth, widgetHeight)
			if instance.quietEdit != 0 {
				rect := scaleRect(RECT{246, 126, 372, 150})
				pSetWindowPos.Call(instance.quietEdit, 0, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top), SWP_NOZORDER|SWP_NOACTIVATE)
			}
			if instance.cooldownEdit != 0 {
				rect := scaleRect(RECT{246, 180, 372, 204})
				pSetWindowPos.Call(instance.cooldownEdit, 0, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top), SWP_NOZORDER|SWP_NOACTIVATE)
			}
			if instance.verifyEdit != 0 {
				rect := verifyEditRect()
				pSetWindowPos.Call(instance.verifyEdit, 0, uintptr(rect.Left), uintptr(rect.Top), uintptr(rect.Right-rect.Left), uintptr(rect.Bottom-rect.Top), SWP_NOZORDER|SWP_NOACTIVATE)
			}
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

			switch instance.currentView {
			case WidgetViewRepair:
				backRect, _, closeRect := subviewCommonHeader()
				recheckBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}
				prev := instance.repairHover
				instance.repairHover = repairViewHover{
					back:    pointInRect(x, y, backRect),
					close:   pointInRect(x, y, closeRect),
					recheck: pointInRect(x, y, recheckBtn),
					done:    pointInRect(x, y, doneBtn),
				}
				if instance.repairHover != prev {
					pInvalidateRect.Call(hwnd, 0, 0)
				}
				if instance.repairHover.back || instance.repairHover.close || instance.repairHover.recheck || instance.repairHover.done {
					hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
					pSetCursor.Call(hand)
				}
				return 0

			case WidgetViewHistory:
				backRect, _, closeRect := subviewCommonHeader()
				historyItems := instance.loadHistory(historyListPageSize * 10)
				total := len(historyItems)
				const pageSize = historyListPageSize
				prevBtn, nextBtn, clearBtn := historyHeaderButtons(total, pageSize)
				copyBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}
				listCard := RECT{14, 48, 386, 260}
				rowIdx := historyRowIndexAt(x, y, listCard, pageSize)
				if rowIdx >= 0 && (instance.historyPageOffset+rowIdx) >= total {
					rowIdx = -1
				}
				prev := instance.historyHover
				instance.historyHover = historyViewHover{
					back:     pointInRect(x, y, backRect),
					close:    pointInRect(x, y, closeRect),
					clear:    pointInRect(x, y, clearBtn),
					prev:     pointInRect(x, y, prevBtn),
					next:     pointInRect(x, y, nextBtn),
					copy:     pointInRect(x, y, copyBtn),
					done:     pointInRect(x, y, doneBtn),
					rowIndex: rowIdx,
				}
				if instance.historyHover != prev {
					pInvalidateRect.Call(hwnd, 0, 0)
				}
				if instance.historyHover.back || instance.historyHover.close || instance.historyHover.clear || instance.historyHover.prev || instance.historyHover.next || instance.historyHover.copy || instance.historyHover.done || rowIdx >= 0 {
					hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
					pSetCursor.Call(hand)
				}
				return 0

			case WidgetViewSettings:
				backRect, _, closeRect := subviewCommonHeader()
				wechatCard := RECT{14, 48, 386, 104}
				reloginBtn := RECT{wechatCard.Right - 100, wechatCard.Top + 12, wechatCard.Right - 12, wechatCard.Bottom - 12}
				optCard := RECT{14, 112, 386, 384}
				replyTrack := RECT{optCard.Right - 64, optCard.Top + 134, optCard.Right - 18, optCard.Top + 158}
				themePill := RECT{optCard.Right - 100, optCard.Top + 188, optCard.Right - 18, optCard.Top + 220}
				agentPill := RECT{optCard.Right - 120, optCard.Top + 240, optCard.Right - 18, optCard.Top + 270}
				saveBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}
				prev := instance.settingsHover
				instance.settingsHover = settingsViewHover{
					back:        pointInRect(x, y, backRect),
					close:       pointInRect(x, y, closeRect),
					relogin:     pointInRect(x, y, reloginBtn),
					replyToggle: pointInRect(x, y, replyTrack),
					themePill:   pointInRect(x, y, themePill),
					agentCycle:  pointInRect(x, y, agentPill),
					save:        pointInRect(x, y, saveBtn),
					done:        pointInRect(x, y, doneBtn),
				}
				if instance.settingsHover != prev {
					pInvalidateRect.Call(hwnd, 0, 0)
				}
				if instance.settingsHover.back || instance.settingsHover.close || instance.settingsHover.relogin || instance.settingsHover.replyToggle || instance.settingsHover.themePill || instance.settingsHover.agentCycle || instance.settingsHover.save || instance.settingsHover.done {
					hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
					pSetCursor.Call(hand)
				}
				return 0

			case WidgetViewLogin:
				backRect, _, closeRect := subviewCommonHeader()
				refreshBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}
				_, submitBtn := loginVerifyRects()
				prev := instance.loginHover
				instance.loginHover = loginViewHover{
					back:    pointInRect(x, y, backRect),
					close:   pointInRect(x, y, closeRect),
					refresh: pointInRect(x, y, refreshBtn),
					done:    pointInRect(x, y, doneBtn),
					submit:  instance.verifyPrompt && pointInRect(x, y, submitBtn),
				}
				if instance.loginHover != prev {
					pInvalidateRect.Call(hwnd, 0, 0)
				}
				if instance.loginHover.back || instance.loginHover.close || instance.loginHover.refresh || instance.loginHover.done || instance.loginHover.submit {
					hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
					pSetCursor.Call(hand)
				}
				return 0

			default:
				if instance.isSingleAgentMode() && instance.agentDropdownOpen {
					dropRect := RECT{76, 130, 266, 272}
					if pointInRect(x, y, dropRect) {
						instance.dropdownHoverIndex = int((y - 134) / 34)
						pInvalidateRect.Call(hwnd, 0, 0)
						hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
						pSetCursor.Call(hand)
						return 0
					} else {
						if instance.dropdownHoverIndex != -1 {
							instance.dropdownHoverIndex = -1
							pInvalidateRect.Call(hwnd, 0, 0)
						}
					}
				}

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
			}
			return 0

		case WM_MOUSELEAVE:
			instance.isTracking = false
			instance.hover = widgetHoverState{}
			instance.repairHover = repairViewHover{}
			instance.historyHover = historyViewHover{}
			instance.settingsHover = settingsViewHover{}
			instance.loginHover = loginViewHover{}
			instance.dropdownHoverIndex = -1
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x := int32(lParam & 0xFFFF)
			y := int32((lParam >> 16) & 0xFFFF)
			x, y = unscalePoint(x, y)

			switch instance.currentView {
			case WidgetViewRepair:
				backRect, _, closeRect := subviewCommonHeader()
				recheckBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}
				if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
					instance.switchView(WidgetViewDashboard)
					return 0
				}
				if isSubviewHeaderDrag(x, y) {
					pReleaseCapture.Call()
					pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
					savePosition(hwnd, paths.WidgetPosFile)
					return 0
				}
				if pointInRect(x, y, recheckBtn) {
					if !instance.repairing {
						instance.repairing = true
						instance.repairError = ""
						pInvalidateRect.Call(hwnd, 0, 0)
						instance.runRepairCheck(hwnd)
					}
					return 0
				}
				return 0

			case WidgetViewHistory:
				backRect, _, closeRect := subviewCommonHeader()
				historyItems := instance.loadHistory(historyListPageSize * 10)
				total := len(historyItems)
				const pageSize = historyListPageSize
				prevBtn, nextBtn, clearBtn := historyHeaderButtons(total, pageSize)
				copyBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}

				if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
					instance.historyConfirmClear = false
					instance.switchView(WidgetViewDashboard)
					return 0
				}
				if isSubviewHeaderDrag(x, y, clearBtn, prevBtn, nextBtn) {
					pReleaseCapture.Call()
					pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
					savePosition(hwnd, paths.WidgetPosFile)
					return 0
				}
				if total > pageSize {
					if pointInRect(x, y, prevBtn) {
						if instance.historyPageOffset >= pageSize {
							instance.historyPageOffset -= pageSize
							instance.historySelectedIndex = instance.historyPageOffset
							pInvalidateRect.Call(hwnd, 0, 0)
						}
						return 0
					}
					if pointInRect(x, y, nextBtn) {
						if instance.historyPageOffset+pageSize < total {
							instance.historyPageOffset += pageSize
							instance.historySelectedIndex = instance.historyPageOffset
							pInvalidateRect.Call(hwnd, 0, 0)
						}
						return 0
					}
				}
				if pointInRect(x, y, clearBtn) {
					if !instance.historyConfirmClear {
						instance.historyConfirmClear = true
						pInvalidateRect.Call(hwnd, 0, 0)
						return 0
					}
					instance.historyConfirmClear = false
					_ = os.WriteFile(paths.PushLog, []byte{}, 0600)
					instance.historyPageOffset = 0
					instance.historySelectedIndex = 0
					instance.refreshState()
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				instance.historyConfirmClear = false
				listCard := RECT{14, 48, 386, 260}
				if pointInRect(x, y, listCard) {
					if rowIdx := historyRowIndexAt(x, y, listCard, pageSize); rowIdx >= 0 {
						targetIdx := instance.historyPageOffset + rowIdx
						if targetIdx >= 0 && targetIdx < len(historyItems) {
							instance.historySelectedIndex = targetIdx
							pInvalidateRect.Call(hwnd, 0, 0)
						}
					}
					return 0
				}
				if pointInRect(x, y, copyBtn) {
					if len(historyItems) > instance.historySelectedIndex {
						item := historyItems[instance.historySelectedIndex]
						SetClipboardText(fmt.Sprintf("%s\n\n%s", item.Title, item.Summary))
					}
					return 0
				}
				return 0

			case WidgetViewSettings:
				backRect, _, closeRect := subviewCommonHeader()
				wechatCard := RECT{14, 48, 386, 104}
				reloginBtn := RECT{wechatCard.Right - 100, wechatCard.Top + 12, wechatCard.Right - 12, wechatCard.Bottom - 12}
				optCard := RECT{14, 112, 386, 384}
				replyTrack := RECT{optCard.Right - 64, optCard.Top + 134, optCard.Right - 18, optCard.Top + 158}
				themePill := RECT{optCard.Right - 100, optCard.Top + 188, optCard.Right - 18, optCard.Top + 220}
				agentPill := RECT{optCard.Right - 120, optCard.Top + 240, optCard.Right - 18, optCard.Top + 270}
				saveBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}

				if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
					instance.settingsError = ""
					instance.switchView(WidgetViewDashboard)
					return 0
				}
				if isSubviewHeaderDrag(x, y) {
					pReleaseCapture.Call()
					pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
					savePosition(hwnd, paths.WidgetPosFile)
					return 0
				}
				if pointInRect(x, y, reloginBtn) {
					instance.switchView(WidgetViewLogin)
					return 0
				}
				if pointInRect(x, y, replyTrack) {
					instance.replyEnabled = !instance.replyEnabled
					instance.mutateConfig(func(cfg *config.AppConfig) {
						cfg.ReplyEnabled = instance.replyEnabled
					})
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				if pointInRect(x, y, themePill) {
					instance.toggleTheme()
					return 0
				}
				if pointInRect(x, y, agentPill) {
					instance.nextAgent()
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
				if pointInRect(x, y, saveBtn) {
					quietVal := ""
					if instance.quietEdit != 0 {
						quietVal = strings.TrimSpace(getWindowText(instance.quietEdit))
					}
					if quietVal != "" && !config.ValidQuietHours(quietVal) {
						instance.settingsError = "格式无效：如 23-8，留空关闭"
						pInvalidateRect.Call(hwnd, 0, 0)
						return 0
					}
					instance.settingsError = ""
					cooldownVal := instance.cooldownMin
					if instance.cooldownEdit != 0 {
						if cd, err := strconv.Atoi(strings.TrimSpace(getWindowText(instance.cooldownEdit))); err == nil && cd > 0 {
							cooldownVal = cd
						}
					}
					cfg, err := config.LoadConfig("")
					if err != nil {
						instance.settingsError = "读取设置失败，未保存：" + err.Error()
						pInvalidateRect.Call(hwnd, 0, 0)
						return 0
					}
					cfg.QuietHours = quietVal
					cfg.CooldownMin = cooldownVal
					cfg.ReplyEnabled = instance.replyEnabled
					cfg.Theme = instance.theme
					cfg.DefaultAgent = instance.currentAgent
					if err := config.SaveConfig(cfg, ""); err != nil {
						instance.settingsError = "保存失败：" + err.Error()
						pInvalidateRect.Call(hwnd, 0, 0)
						return 0
					}
					instance.quietHours = cfg.QuietHours
					instance.cooldownMin = cfg.CooldownMin
					instance.switchView(WidgetViewDashboard)
					return 0
				}
				return 0

			case WidgetViewLogin:
				backRect, _, closeRect := subviewCommonHeader()
				refreshBtn := RECT{14, 394, 195, 436}
				doneBtn := RECT{205, 394, 386, 436}
				if instance.verifyPrompt {
					if _, submitBtn := loginVerifyRects(); pointInRect(x, y, submitBtn) {
						instance.submitVerifyCode(hwnd)
						return 0
					}
				}
				if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) || pointInRect(x, y, doneBtn) {
					instance.switchView(WidgetViewDashboard)
					return 0
				}
				if isSubviewHeaderDrag(x, y) {
					pReleaseCapture.Call()
					pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
					savePosition(hwnd, paths.WidgetPosFile)
					return 0
				}
				if pointInRect(x, y, refreshBtn) {
					instance.startInAppLoginFlow()
					return 0
				}
				return 0
			}

			// In WidgetViewDashboard:
			layout := widgetLayoutRects()

			if instance.isSingleAgentMode() && instance.agentDropdownOpen {
				dropRect := RECT{76, 130, 266, 272}
				if pointInRect(x, y, dropRect) {
					descriptors := agentmeta.All()
					if y >= 134 && y < 134+int32(len(descriptors))*34 {
						itemIdx := int((y - 134) / 34)
						if itemIdx >= 0 && itemIdx < len(descriptors) {
							instance.currentAgent = descriptors[itemIdx].ID
							instance.mutateConfig(func(cfg *config.AppConfig) {
								cfg.DefaultAgent = instance.currentAgent
							})
							instance.agentDropdownOpen = false
							instance.refreshState()
							pInvalidateRect.Call(hwnd, 0, 0)
							return 0
						}
					}
				}
				instance.agentDropdownOpen = false
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}

			if pointInRect(x, y, layout.drag) && !pointInRect(x, y, layout.minimize) && !pointInRect(x, y, layout.close) && !pointInRect(x, y, layout.themeToggle) {
				pReleaseCapture.Call()
				pSendMessageW.Call(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0)
				savePosition(hwnd, paths.WidgetPosFile)
				return 0
			}
			if pointInRect(x, y, layout.themeToggle) {
				instance.toggleTheme()
				return 0
			}
			if pointInRect(x, y, layout.minimize) || pointInRect(x, y, layout.close) {
				savePosition(hwnd, paths.WidgetPosFile)
				pShowWindow.Call(hwnd, SW_HIDE)
				return 0
			}
			if pointInRect(x, y, layout.modeToggle) {
				instance.toggleAgentMode()
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if instance.isSingleAgentMode() {
				badge := RECT{layout.singleAgent.Left + 14, layout.singleAgent.Top + 14, layout.singleAgent.Left + 50, layout.singleAgent.Top + 50}
				if pointInRect(x, y, layout.switchAgent) || pointInRect(x, y, badge) {
					instance.showAgentDropdown(hwnd, layout.switchAgent)
					return 0
				}
				if pointInRect(x, y, layout.singleSwitch) {
					instance.toggleAgent(instance.focusedAgentID())
					instance.refreshState()
					pInvalidateRect.Call(hwnd, 0, 0)
					return 0
				}
			}
			if instance.toggleAgentAt(x, y, layout) {
				instance.refreshState()
				pInvalidateRect.Call(hwnd, 0, 0)
				return 0
			}
			if pointInRect(x, y, layout.recent) || pointInRect(x, y, layout.history) {
				instance.switchView(WidgetViewHistory)
				return 0
			}
			if pointInRect(x, y, layout.test) || pointInRect(x, y, layout.settings) {
				instance.switchView(WidgetViewSettings)
				return 0
			}
			if pointInRect(x, y, layout.hide) {
				instance.switchView(WidgetViewLogin)
				return 0
			}
			if pointInRect(x, y, layout.repair) {
				instance.switchView(WidgetViewRepair)
				return 0
			}
			if pointInRect(x, y, layout.update) {
				instance.handleUpdateClick(hwnd)
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
			case IDOK:
				// 配对码输入框聚焦时回车由消息循环转成 IDOK，这里按"提交配对码"处理。
				if instance.verifyPrompt {
					instance.submitVerifyCode(hwnd)
				}
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
				instance.switchView(WidgetViewHistory)
				restoreAndBringToFront(hwnd)
			case IDM_SETTINGS:
				instance.switchView(WidgetViewSettings)
				restoreAndBringToFront(hwnd)
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
			instance.releaseFonts()
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
	pInvalidateRect.Call(app.hwnd, 0, 0)
}

func (app *WidgetApp) applyThemeToWindow() {
	if app.hwnd == 0 {
		return
	}
	darkMode := uint32(1)
	if app.theme == "light" {
		darkMode = 0
	}
	pDwmSetWindowAttribute.Call(app.hwnd, 20, uintptr(unsafe.Pointer(&darkMode)), 4)
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

	if app.hwnd != 0 {
		pInvalidateRect.Call(app.hwnd, 0, 0)
	}
}

func (app *WidgetApp) ensureSettingsEdits() {
	if app.hwnd == 0 {
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
			app.hwnd, uintptr(IDC_SETTINGS_QUIET), hInstance, 0,
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
			app.hwnd, uintptr(IDC_SETTINGS_COOLDOWN), hInstance, 0,
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
	if app.hwnd == 0 || app.verifyEdit != 0 {
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
		app.hwnd, uintptr(loginCodeEditID), hInstance, 0,
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
	items, _ := notify.GetHistory(limit, app.paths.PushLog)
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
	if app.hwnd != 0 {
		showMessage(app.hwnd, message, MB_ICONINFO)
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

func (app *WidgetApp) agentMenuLabel(agentID string) string {
	if !app.agentEnabled(agentID) {
		return "已暂停"
	}
	status := app.integrationStatus(agentID)
	switch status.State {
	case integration.StateConnected:
		if app.agentRunning(agentID) {
			return "正常"
		}
		return "已接入"
	case integration.StatePendingRestart:
		return "待重启"
	case integration.StateError:
		return "异常"
	default:
		if app.agentRunning(agentID) {
			return "未接入"
		}
		return "未配置"
	}
}

func (app *WidgetApp) showAgentDropdown(hwnd uintptr, rect RECT) {
	app.agentDropdownOpen = !app.agentDropdownOpen
	pInvalidateRect.Call(hwnd, 0, 0)
}
