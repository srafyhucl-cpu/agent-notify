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
	if width, height := logicalSize(widgetWidth, widgetHeight); width != 600 || height != 540 {
		t.Fatalf("logicalSize at 144 DPI = (%d,%d), want (600,540)", width, height)
	}
}

func TestDialogControlLayoutsStayInsideAndDoNotOverlap(t *testing.T) {
	settings := settingsLayoutRects()
	login := loginLayoutRects()
	history := historyLayoutRects()

	tests := []struct {
		name     string
		width    int32
		height   int32
		controls []namedRect
	}{
		{
			name:   "settings",
			width:  settingsWidth,
			height: settingsHeight,
			controls: []namedRect{
				{"close", settings.close},
				{"login", settings.login},
				{"logout", settings.logout},
				{"quiet", settings.quiet},
				{"cooldown", settings.cooldown},
				{"cancel", settings.cancel},
				{"save", settings.save},
			},
		},
		{
			name:   "login",
			width:  loginWidth,
			height: loginHeight,
			controls: []namedRect{
				{"window close", login.winClose},
				{"qr", login.qr},
				{"retry", login.retry},
				{"done", login.done},
			},
		},
		{
			name:   "history",
			width:  historyWidth,
			height: historyHeight,
			controls: []namedRect{
				{"close", history.close},
				{"list", history.list},
				{"detail", history.detail},
				{"copy", history.copy},
				{"clear", history.clear},
				{"done", history.done},
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			for _, control := range tt.controls {
				assertRectInside(t, control, tt.width, tt.height)
			}
			for i := 0; i < len(tt.controls); i++ {
				for j := i + 1; j < len(tt.controls); j++ {
					if rectsOverlap(tt.controls[i].rect, tt.controls[j].rect) {
						t.Fatalf("%s overlaps %s", tt.controls[i].name, tt.controls[j].name)
					}
				}
			}
		})
	}

	if len(history.rows) != historyMaxRows {
		t.Fatalf("history rows = %d, want %d", len(history.rows), historyMaxRows)
	}
	for i, row := range history.rows {
		assertRectInside(t, namedRect{name: "row", rect: row}, historyWidth, historyHeight)
		if row.Left < history.list.Left || row.Right > history.list.Right || row.Top < history.list.Top || row.Bottom > history.list.Bottom {
			t.Fatalf("history row %d is outside the list card: %+v", i, row)
		}
	}
}

type namedRect struct {
	name string
	rect RECT
}

func assertRectInside(t *testing.T, control namedRect, width, height int32) {
	t.Helper()
	rect := control.rect
	if rect.Left < 0 || rect.Top < 0 || rect.Right > width || rect.Bottom > height || rect.Right <= rect.Left || rect.Bottom <= rect.Top {
		t.Fatalf("%s rect is invalid or outside %dx%d: %+v", control.name, width, height, rect)
	}
}

func TestHistoryRowAt(t *testing.T) {
	layout := historyLayoutRects()

	if got := historyRowAt(layout, 30, historyRowTop, 0, 20); got != 0 {
		t.Fatalf("historyRowAt first row = %d, want 0", got)
	}
	if got := historyRowAt(layout, 30, historyRowTop+historyRowHeight, 4, 20); got != 5 {
		t.Fatalf("historyRowAt second visible row with offset 4 = %d, want 5", got)
	}
	if got := historyRowAt(layout, 30, layout.rows[0].Bottom-1, 0, 20); got != 0 {
		t.Fatalf("historyRowAt last pixel of first row = %d, want 0", got)
	}
	if got := historyRowAt(layout, 30, historyRowTop+historyRowHeight-2, 0, 20); got != -1 {
		t.Fatalf("historyRowAt row gap = %d, want -1", got)
	}
	if got := historyRowAt(layout, 20, historyRowTop, 0, 20); got != -1 {
		t.Fatalf("historyRowAt outside row padding = %d, want -1", got)
	}
	if got := historyRowAt(layout, 30, layout.list.Bottom, 0, 20); got != -1 {
		t.Fatalf("historyRowAt outside list = %d, want -1", got)
	}
	if got := historyRowAt(layout, 30, historyRowTop+3*historyRowHeight, 0, 3); got != -1 {
		t.Fatalf("historyRowAt beyond total = %d, want -1", got)
	}
}

func TestClampScrollOffset(t *testing.T) {
	tests := []struct {
		name    string
		offset  int
		total   int
		maxRows int
		want    int
	}{
		{"negative", -1, 100, 9, 0},
		{"inside", 4, 100, 9, 4},
		{"at maximum", 91, 100, 9, 91},
		{"past maximum", 95, 100, 9, 91},
		{"fewer rows than page", 10, 5, 9, 0},
		{"empty", 0, 0, 9, 0},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := clampScrollOffset(tt.offset, tt.total, tt.maxRows); got != tt.want {
				t.Fatalf("clampScrollOffset(%d,%d,%d) = %d, want %d", tt.offset, tt.total, tt.maxRows, got, tt.want)
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
	_, bitmap, status, failure, success := state.snapshot()
	if generation != 1 || bitmap != nil || status != "正在获取二维码…" || failure || success {
		t.Fatalf("initial login state = generation %d bitmap %v status %q failure %v success %v", generation, bitmap, status, failure, success)
	}

	qrBitmap := [][]bool{{true, false}, {false, true}}
	state.update(generation, qrBitmap, "请使用微信扫描二维码", false, false)
	_, bitmap, status, failure, success = state.snapshot()
	if len(bitmap) != 2 || !bitmap[0][0] || status != "请使用微信扫描二维码" || failure || success {
		t.Fatalf("updated login state = bitmap %v status %q failure %v success %v", bitmap, status, failure, success)
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
	_, bitmap, status, failure, success = state.snapshot()
	if bitmap != nil || status != "正在获取二维码…" || failure || success {
		t.Fatalf("stale update changed login state = bitmap %v status %q failure %v success %v", bitmap, status, failure, success)
	}

	state.cancel()
	select {
	case <-secondContext.Done():
	default:
		t.Fatal("cancel did not stop the active login flow")
	}
	if got, _, _, _, _ := state.snapshot(); got != nextGeneration+1 {
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
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify DPI test"))),
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
		{96, 400, 360},
		{144, 600, 540},
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
