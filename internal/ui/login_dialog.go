//go:build windows

package ui

import (
	"context"
	"runtime"
	"sync"
	"syscall"
	"time"
	"unsafe"

	qrcode "github.com/skip2/go-qrcode"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

const (
	loginWidth  = int32(360)
	loginHeight = int32(470)
	loginTimer  = 2
)

type loginDialogState struct {
	mu         sync.Mutex
	generation uint64
	cancelFlow context.CancelFunc
	bitmap     [][]bool
	status     string
	failure    bool
	success    bool
}

func (state *loginDialogState) snapshot() (uint64, [][]bool, string, bool, bool) {
	state.mu.Lock()
	defer state.mu.Unlock()
	return state.generation, state.bitmap, state.status, state.failure, state.success
}

func (state *loginDialogState) update(generation uint64, bitmap [][]bool, status string, failure, success bool) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if generation != state.generation {
		return
	}
	if bitmap != nil {
		state.bitmap = bitmap
	}
	if status != "" {
		state.status = status
	}
	state.failure = failure
	state.success = success
}

func (state *loginDialogState) begin(timeout time.Duration) (uint64, context.Context) {
	state.mu.Lock()
	defer state.mu.Unlock()

	if state.cancelFlow != nil {
		state.cancelFlow()
		state.cancelFlow = nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	state.cancelFlow = cancel
	state.generation++
	state.bitmap = nil
	state.status = "正在获取二维码…"
	state.failure = false
	state.success = false
	return state.generation, ctx
}

func (state *loginDialogState) finish(generation uint64) {
	state.mu.Lock()
	var cancel context.CancelFunc
	if state.generation == generation {
		cancel = state.cancelFlow
		state.cancelFlow = nil
	}
	state.mu.Unlock()
	if cancel != nil {
		cancel()
	}
}

func (state *loginDialogState) cancel() {
	state.mu.Lock()
	state.generation++
	cancel := state.cancelFlow
	state.cancelFlow = nil
	state.mu.Unlock()
	if cancel != nil {
		cancel()
	}
}

func startLoginFlow(state *loginDialogState) {
	generation, ctx := state.begin(5 * time.Minute)
	go func() {
		defer state.finish(generation)

		client := clawbot.NewAuthClient(clawbot.DefaultBaseURL)
		response, err := client.FetchQRCode(ctx)
		if err != nil {
			state.update(generation, nil, "获取二维码失败："+err.Error(), true, false)
			return
		}
		qr, err := qrcode.New(response.QRCode, qrcode.Medium)
		if err != nil {
			state.update(generation, nil, "二维码生成失败："+err.Error(), true, false)
			return
		}
		state.update(generation, qr.Bitmap(), "请使用微信扫描二维码", false, false)

		credentials, err := client.PollQRStatus(ctx, response.QRCode, func(status string) {
			switch status {
			case clawbot.StatusScanned:
				state.update(generation, nil, "已扫码，请在微信中确认", false, false)
			case clawbot.StatusConfirmed:
				state.update(generation, nil, "已确认，正在保存凭据…", false, false)
			case clawbot.StatusExpired:
				state.update(generation, nil, "二维码已过期，请重新获取", true, false)
			}
		})
		if err != nil {
			state.update(generation, nil, "登录失败："+err.Error(), true, false)
			return
		}
		if err := clawbot.SaveCredentials(credentials); err != nil {
			state.update(generation, nil, "凭据保存失败："+err.Error(), true, false)
			return
		}
		state.update(generation, nil, "登录成功，微信推送已连接", false, true)
	}()
}

func drawQRCode(hdc uintptr, rect RECT, bitmap [][]bool) {
	fillRoundRect(hdc, rect, 8, uintptr(RGB(248, 250, 251)))
	strokeRoundRect(hdc, rect, 8, uintptr(RGB(248, 250, 251)), uintptr(RGB(76, 87, 96)), 1)
	if len(bitmap) == 0 || len(bitmap[0]) == 0 {
		return
	}

	modules := len(bitmap)
	if columns := len(bitmap[0]); columns < modules {
		modules = columns
	}
	available := rect.Right - rect.Left - 24
	moduleSize := available / int32(modules)
	if moduleSize < 1 {
		moduleSize = 1
	}
	codeSize := moduleSize * int32(modules)
	startX := rect.Left + (rect.Right-rect.Left-codeSize)/2
	startY := rect.Top + (rect.Bottom-rect.Top-codeSize)/2
	blackBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(22, 27, 31)))
	defer pDeleteObject.Call(blackBrush)

	for row, line := range bitmap {
		for column, filled := range line {
			if !filled {
				continue
			}
			module := RECT{
				Left:   startX + int32(column)*moduleSize,
				Top:    startY + int32(row)*moduleSize,
				Right:  startX + int32(column+1)*moduleSize + 1,
				Bottom: startY + int32(row+1)*moduleSize + 1,
			}
			scaled := scaleRect(module)
			pFillRect.Call(hdc, uintptr(unsafe.Pointer(&scaled)), blackBrush)
		}
	}
}

// ShowLoginDialog displays the ClawBot QR login flow without opening a console.
func ShowLoginDialog(parentHwnd uintptr) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	parentDPI := windowDPI(parentHwnd)
	setUIDPI(parentDPI)
	defer setUIDPI(parentDPI)

	hInstance, _, _ := pGetModuleHandleW.Call(0)
	className := StringToUTF16Ptr("AgentNotifyLoginDialog")
	state := &loginDialogState{}
	layout := loginLayoutRects()
	var dialog uintptr
	var tracking bool
	var hoverClose, hoverRetry, hoverDone bool

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_CREATE:
			setUIDPI(windowDPI(hwnd))
			startLoginFlow(state)
			pSetTimer.Call(hwnd, loginTimer, 100, 0)
			return 0

		case WM_DPICHANGED:
			setUIDPI(uint32(wParam & 0xFFFF))
			resizeForCurrentDPI(hwnd, loginWidth, loginHeight)
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_ERASEBKGND:
			return 1

		case WM_TIMER:
			if wParam == loginTimer {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			return 0

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
			previousClose, previousRetry, previousDone := hoverClose, hoverRetry, hoverDone
			hoverClose = pointInRect(x, y, layout.winClose)
			hoverRetry = pointInRect(x, y, layout.retry)
			hoverDone = pointInRect(x, y, layout.done)
			if hoverClose != previousClose || hoverRetry != previousRetry || hoverDone != previousDone {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if hoverClose || hoverRetry || hoverDone {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WM_MOUSELEAVE:
			tracking = false
			hoverClose, hoverRetry, hoverDone = false, false, false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
			switch {
			case pointInRect(x, y, layout.retry):
				startLoginFlow(state)
			case pointInRect(x, y, layout.winClose), pointInRect(x, y, layout.done):
				state.cancel()
				pDestroyWindow.Call(hwnd)
			}
			return 0

		case WM_PAINT:
			paintDoubleBuffered(hwnd, func(hdc uintptr, width, height int32) {
				fillRectLogical(hdc, RECT{0, 0, loginWidth, loginHeight}, uintptr(RGB(15, 19, 23)))
				pSetBkMode.Call(hdc, TRANSPARENT)

				titleFont := newFont(18, 700)
				baseFont := newFont(12, 400)
				smallFont := newFont(10, 400)
				iconFont := newIconFont(13)
				oldFont, _, _ := pSelectObject.Call(hdc, titleFont)
				defer func() {
					pSelectObject.Call(hdc, oldFont)
					pDeleteObject.Call(titleFont)
					pDeleteObject.Call(baseFont)
					pDeleteObject.Call(smallFont)
					pDeleteObject.Call(iconFont)
				}()

				pSetTextColor.Call(hdc, uintptr(RGB(242, 246, 247)))
				DrawText(hdc, "连接 ClawBot", &RECT{20, 14, 260, 42}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				pSelectObject.Call(hdc, smallFont)
				pSetTextColor.Call(hdc, uintptr(RGB(137, 149, 160)))
				DrawText(hdc, "使用微信扫描二维码完成绑定", &RECT{20, 42, 280, 64}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				drawWindowButton(hdc, layout.winClose, "\uE8BB", hoverClose, true, iconFont)

				_, bitmap, status, failure, success := state.snapshot()
				drawQRCode(hdc, layout.qr, bitmap)

				statusColor := uintptr(RGB(137, 149, 160))
				if failure {
					statusColor = uintptr(RGB(230, 112, 112))
				} else if success {
					statusColor = uintptr(RGB(55, 190, 147))
				}
				pSelectObject.Call(hdc, baseFont)
				pSetTextColor.Call(hdc, statusColor)
				DrawText(hdc, status, &RECT{24, 352, 336, 392}, DT_CENTER|DT_WORDBREAK|DT_NOPREFIX)

				drawIconTextButton(hdc, layout.retry, "\uE72C", "重新获取", hoverRetry, false, false, baseFont, iconFont)
				label := "关闭"
				if success {
					label = "完成"
				}
				drawIconTextButton(hdc, layout.done, "\uE8BB", label, hoverDone, success, false, baseFont, iconFont)
			})
			return 0

		case WM_CLOSE:
			state.cancel()
			pDestroyWindow.Call(hwnd)
			return 0

		case WM_DESTROY:
			state.cancel()
			pKillTimer.Call(hwnd, loginTimer)
			dialog = 0
			return 0
		}
		result, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
		return result
	})
	loginWndProcCallback = wndProc

	var windowClass WNDCLASSEXW
	windowClass.CbSize = uint32(unsafe.Sizeof(windowClass))
	windowClass.LpfnWndProc = wndProc
	windowClass.HInstance = hInstance
	windowClass.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
	windowClass.LpszClassName = className
	if atom, _, registerErr := pRegisterClassExW.Call(uintptr(unsafe.Pointer(&windowClass))); atom == 0 {
		debugLog("RegisterClassExW login failed: %v", registerErr)
		return
	}
	defer pUnregisterClassW.Call(uintptr(unsafe.Pointer(className)), hInstance)

	width, height := logicalSize(loginWidth, loginHeight)
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)
	x := (int32(screenWidth) - width) / 2
	y := (int32(screenHeight) - height) / 2
	dialog, _, _ = pCreateWindowExW.Call(
		WS_EX_TOPMOST,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(StringToUTF16Ptr("Agent-notify ClawBot 登录"))),
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

var loginWndProcCallback uintptr

type loginLayout struct {
	winClose RECT
	qr       RECT
	retry    RECT
	done     RECT
}

func loginLayoutRects() loginLayout {
	return loginLayout{
		winClose: RECT{320, 8, 352, 40},
		qr:       RECT{45, 78, 315, 348},
		retry:    RECT{54, 402, 168, 438},
		done:     RECT{192, 402, 306, 438},
	}
}
