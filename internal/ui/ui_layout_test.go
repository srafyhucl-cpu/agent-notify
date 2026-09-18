//go:build windows

package ui

import (
	"fmt"
	"runtime"
	"syscall"
	"testing"
	"time"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func withUIDPI(t *testing.T, dpi uint32) {
	t.Helper()
	previous := uiDPI
	setUIDPI(dpi)
	t.Cleanup(func() {
		setUIDPI(previous)
	})
}

func TestNormalizeDPI(t *testing.T) {
	tests := []struct {
		input uint32
		want  uint32
	}{
		{0, widgetDPImin},
		{widgetDPImin, widgetDPImin},
		{96, 96},
		{144, 144},
		{widgetDPImax, widgetDPImax},
		{widgetDPImax + 1, widgetDPImax},
	}
	for _, tt := range tests {
		if got := normalizeDPI(tt.input); got != tt.want {
			t.Fatalf("normalizeDPI(%d) = %d, want %d", tt.input, got, tt.want)
		}
	}
}

func TestScaleFloatAndUnscalePoint(t *testing.T) {
	withUIDPI(t, 96)
	if got := scaleFloat(400); got != 400 {
		t.Fatalf("scaleFloat(400) at 96 DPI = %d, want 400", got)
	}
	if x, y := unscalePoint(600, 540); x != 600 || y != 540 {
		t.Fatalf("unscalePoint at 96 DPI = (%d,%d), want (600,540)", x, y)
	}

	withUIDPI(t, 144)
	if got := scaleFloat(400); got != 600 {
		t.Fatalf("scaleFloat(400) at 144 DPI = %d, want 600", got)
	}
	if got := scaleFloat(1); got != 2 {
		t.Fatalf("scaleFloat(1) at 144 DPI = %d, want 2", got)
	}
	if x, y := unscalePoint(600, 540); x != 400 || y != 360 {
		t.Fatalf("unscalePoint at 144 DPI = (%d,%d), want (400,360)", x, y)
	}
	if width, height := logicalSize(widgetWidth, widgetHeight); width != 600 || height != 675 {
		t.Fatalf("logicalSize at 144 DPI = (%d,%d), want (600,675)", width, height)
	}
}

func TestWidgetTextFitsItsRects(t *testing.T) {
	text := widgetTextRects()
	checks := []struct {
		name string
		font func() uintptr
		text string
		rect RECT
	}{
		{"标题", newTitleFont, "AgentNotify", text.title},
		{"副标题", newSmallFont, "4 个 Agent · ClawBot 微信通知", text.subtitle},
		{"单Agent副标题", newSmallFont, "聚焦单 Agent · ClawBot 微信通知", text.subtitle},
		{"页脚版本", newSmallFont, "v10.10.10", text.footerVersion},
		{"页脚升级", newSmallFont, "升级 v10.10.10", text.footerUpdate},
		{"页脚接入检查", newSmallFont, "检查修复", text.footerHint},
	}

	for _, dpi := range []uint32{96, 144, 192} {
		t.Run(fmt.Sprintf("%ddpi", dpi), func(t *testing.T) {
			withUIDPI(t, dpi)
			for _, check := range checks {
				font := check.font()
				measured := measureTextWidth(font, check.text)
				pDeleteObject.Call(font)
				if measured <= 0 {
					t.Fatalf("%s：无法测量文本宽度", check.name)
				}
				limit := scaleFloat(check.rect.Right - check.rect.Left)
				if measured > limit {
					t.Fatalf("%s 在 %d DPI 溢出：文本 %q 需要 %d 像素，可用 %d", check.name, dpi, check.text, measured, limit)
				}
			}
		})
	}
}

func TestWechatLabelsFitTheirRects(t *testing.T) {
	layout := widgetLayoutRects()
	card := wechatCardLabelRect()
	checks := []struct {
		name string
		font func() uintptr
		text string
		rect RECT
	}{
		{"dock 正常", newSmallFont, "微信配置", layout.hide},
		{"dock 未登录", newSmallFont, "微信未连", layout.hide},
		{"dock 等待首条", newSmallFont, "待发消息", layout.hide},
		{"dock 已断开", newSmallFont, "推送已断", layout.hide},
		{"卡片未登录", newStrongFont, "ClawBot 微信未登录", card},
		{"卡片登录失效", newStrongFont, "ClawBot 微信登录已失效", card},
		{"卡片等待消息", newStrongFont, "等待微信消息", card},
		{"卡片已断开", newStrongFont, "主动推送会话已失效", card},
	}

	for _, dpi := range []uint32{96, 144, 192} {
		t.Run(fmt.Sprintf("%ddpi", dpi), func(t *testing.T) {
			withUIDPI(t, dpi)
			for _, check := range checks {
				font := check.font()
				measured := measureTextWidth(font, check.text)
				pDeleteObject.Call(font)
				if measured <= 0 {
					t.Fatalf("%s：无法测量文本宽度", check.name)
				}
				limit := scaleFloat(check.rect.Right - check.rect.Left)
				if measured > limit {
					t.Fatalf("%s 在 %d DPI 溢出：文本 %q 需要 %d 像素，可用 %d", check.name, dpi, check.text, measured, limit)
				}
			}
		})
	}
}

func TestWechatLinkCardTextFits(t *testing.T) {
	titleRect := RECT{28, 198, 372, 228}
	states := []wechatLinkState{
		wechatLinkOK, wechatLinkAwaitingFirst, wechatLinkBroken, wechatLinkStale,
	}
	for _, dpi := range []uint32{96, 144, 192} {
		t.Run(fmt.Sprintf("%ddpi", dpi), func(t *testing.T) {
			withUIDPI(t, dpi)
			for _, state := range states {
				_, title, _ := wechatLinkCardText(state)
				font := newStrongFont()
				measured := measureTextWidth(font, title)
				pDeleteObject.Call(font)
				if measured <= 0 {
					t.Fatalf("state %d：无法测量标题宽度", state)
				}
				if limit := scaleFloat(titleRect.Right - titleRect.Left); measured > limit {
					t.Fatalf("state %d 标题在 %d DPI 溢出：%q 需要 %d 像素，可用 %d", state, dpi, title, measured, limit)
				}
			}
		})
	}
}

func TestRelativeHistoryTime(t *testing.T) {
	now := time.Date(2026, time.September, 12, 12, 0, 0, 0, time.Local)
	tests := []struct {
		name string
		item notify.HistoryItem
		want string
	}{
		{"just now", notify.HistoryItem{Timestamp: now.Add(-30 * time.Second).Format(time.RFC3339Nano)}, "刚刚"},
		{"minutes", notify.HistoryItem{Timestamp: now.Add(-5*time.Minute - 5*time.Second).Format(time.RFC3339Nano)}, "5 分钟前"},
		{"today", notify.HistoryItem{Timestamp: now.Add(-2 * time.Hour).Format(time.RFC3339Nano)}, "10:00"},
		{"past day", notify.HistoryItem{Timestamp: now.Add(-26 * time.Hour).Format(time.RFC3339Nano)}, "09-11 10:00"},
		{"invalid timestamp", notify.HistoryItem{Timestamp: "not-a-time"}, "not-a-time"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := relativeHistoryTimeAt(tt.item, now); got != tt.want {
				t.Fatalf("relativeHistoryTimeAt() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestLoginDialogStateTransitions(t *testing.T) {
	state := &loginDialogState{}
	generation, firstContext := state.begin(time.Minute)
	_, bitmap, status, failure, success, promptActive := state.snapshot()
	if generation != 1 || bitmap != nil || status != "正在获取二维码…" || failure || success || promptActive {
		t.Fatalf("initial login state = generation %d bitmap %v status %q failure %v success %v prompt %v", generation, bitmap, status, failure, success, promptActive)
	}

	qrBitmap := [][]bool{{true, false}, {false, true}}
	state.update(generation, qrBitmap, "请使用微信扫描二维码", false, false)
	_, bitmap, status, failure, success, promptActive = state.snapshot()
	if len(bitmap) != 2 || !bitmap[0][0] || status != "请使用微信扫描二维码" || failure || success || promptActive {
		t.Fatalf("updated login state = bitmap %v status %q failure %v success %v prompt %v", bitmap, status, failure, success, promptActive)
	}

	nextGeneration, secondContext := state.begin(time.Minute)
	if nextGeneration != generation+1 {
		t.Fatalf("generation = %d, want %d", nextGeneration, generation+1)
	}
	select {
	case <-firstContext.Done():
	default:
		t.Fatal("begin did not cancel the previous login flow")
	}
	state.update(generation, nil, "stale status", true, false)
	_, bitmap, status, failure, success, promptActive = state.snapshot()
	if bitmap != nil || status != "正在获取二维码…" || failure || success || promptActive {
		t.Fatalf("stale update changed login state = bitmap %v status %q failure %v success %v prompt %v", bitmap, status, failure, success, promptActive)
	}

	state.cancel()
	select {
	case <-secondContext.Done():
	default:
		t.Fatal("cancel did not stop the active login flow")
	}
	if got, _, _, _, _, _ := state.snapshot(); got != nextGeneration+1 {
		t.Fatalf("generation after cancel = %d, want %d", got, nextGeneration+1)
	}
}

func TestResizeForCurrentDPIAt96And144(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	setThreadDPI := user32.NewProc("SetThreadDpiAwarenessContext")
	previousAwareness := uintptr(0)
	if setThreadDPI.Find() == nil {
		previousAwareness, _, _ = setThreadDPI.Call(^uintptr(3)) // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
		defer setThreadDPI.Call(previousAwareness)
	}

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr(fmt.Sprintf("AgentNotifyScaleTestWindow_%d", time.Now().UnixNano()))
	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		result, _, _ := pDefWindowProcW.Call(hwnd, msg, wParam, lParam)
		return result
	})

	var windowClass WNDCLASSEXW
	windowClass.CbSize = uint32(unsafe.Sizeof(windowClass))
	windowClass.LpfnWndProc = wndProc
	windowClass.HInstance = hInstance
	windowClass.LpszClassName = className
	if atom, _, registerErr := pRegisterClassExW.Call(uintptr(unsafe.Pointer(&windowClass))); atom == 0 {
		t.Fatalf("RegisterClassExW failed: %v", registerErr)
	}

	hwnd, _, createErr := pCreateWindowExW.Call(
		0,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("AgentNotify DPI test"))),
		WS_POPUP,
		0, 0, 400, 360,
		0, 0, hInstance, 0,
	)
	if hwnd == 0 {
		t.Skipf("window creation unavailable: %v", createErr)
	}
	defer pDestroyWindow.Call(hwnd)

	previousDPI := uiDPI
	defer setUIDPI(previousDPI)

	tests := []struct {
		dpi        uint32
		wantWidth  int32
		wantHeight int32
	}{
		{96, 400, 450},
		{144, 600, 675},
	}
	for _, tt := range tests {
		setUIDPI(tt.dpi)
		resizeForCurrentDPI(hwnd, widgetWidth, widgetHeight)
		var rect RECT
		pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&rect)))
		if gotWidth, gotHeight := rect.Right-rect.Left, rect.Bottom-rect.Top; gotWidth != tt.wantWidth || gotHeight != tt.wantHeight {
			t.Fatalf("resize at %d DPI = %dx%d, want %dx%d", tt.dpi, gotWidth, gotHeight, tt.wantWidth, tt.wantHeight)
		}
	}
}
func TestWidgetWindowStaysOutOfTaskbar(t *testing.T) {
	style := widgetExtendedStyle()
	if style&WS_EX_TOOLWINDOW == 0 {
		t.Fatal("widget window must use WS_EX_TOOLWINDOW so it does not add a taskbar button")
	}
	if style&WS_EX_APPWINDOW != 0 {
		t.Fatal("widget window must not use WS_EX_APPWINDOW: the tray icon is the only shell entry")
	}
	if style&WS_EX_TOPMOST == 0 {
		t.Fatal("widget window must stay topmost")
	}
}
