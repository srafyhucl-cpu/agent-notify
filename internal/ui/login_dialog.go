//go:build windows

package ui

import (
	"context"
	"runtime"
	"strings"
	"sync"
	"syscall"
	"time"
	"unsafe"

	qrcode "github.com/skip2/go-qrcode"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

const (
	loginWidth  = int32(360)
	loginHeight = int32(500)
	loginTimer  = 2

	loginCodeEditID = 4301
	WM_USER_VERIFY  = 0x0401
)

type loginDialogState struct {
	mu         sync.Mutex
	generation uint64
	cancelFlow context.CancelFunc
	hwnd       uintptr
	bitmap     [][]bool
	status     string
	failure    bool
	success    bool
	prompt     chan string
}

func (state *loginDialogState) snapshot() (uint64, [][]bool, string, bool, bool, bool) {
	state.mu.Lock()
	defer state.mu.Unlock()
	return state.generation, state.bitmap, state.status, state.failure, state.success, state.prompt != nil
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
	state.prompt = nil
	return state.generation, ctx
}

func (state *loginDialogState) setWindow(hwnd uintptr) {
	state.mu.Lock()
	state.hwnd = hwnd
	state.mu.Unlock()
}

func (state *loginDialogState) finish(generation uint64) {
	state.mu.Lock()
	var cancel context.CancelFunc
	if state.generation == generation {
		cancel = state.cancelFlow
		state.cancelFlow = nil
		state.prompt = nil
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
	state.prompt = nil
	state.mu.Unlock()
	if cancel != nil {
		cancel()
	}
}

func (state *loginDialogState) requestVerifyCode(ctx context.Context, retry bool) (string, error) {
	channel := make(chan string, 1)

	state.mu.Lock()
	state.prompt = channel
	if retry {
		state.status = "配对码不匹配，请重新输入"
	} else {
		state.status = "请输入手机微信显示的数字配对码"
	}
	state.failure = false
	hwnd := state.hwnd
	state.mu.Unlock()

	if hwnd != 0 {
		pPostMessageW.Call(hwnd, WM_USER_VERIFY, 0, 0)
	}

	select {
	case code := <-channel:
		return code, nil
	case <-ctx.Done():
		state.mu.Lock()
		if state.prompt == channel {
			state.prompt = nil
		}
		state.mu.Unlock()
		return "", ctx.Err()
	}
}

func (state *loginDialogState) submitVerifyCode(code string) {
	state.mu.Lock()
	channel := state.prompt
	state.prompt = nil
	state.mu.Unlock()
	if channel == nil {
		return
	}
	select {
	case channel <- code:
	default:
	}
}

func startLoginFlow(state *loginDialogState) {
	generation, ctx := state.begin(5 * time.Minute)
	go func() {
		defer state.finish(generation)

		credentials, err := clawbot.Login(ctx, clawbot.LoginOptions{
			OnQRCode: func(response clawbot.QRCodeResponse) error {
				code, err := qrcode.New(response.DisplayContent(), qrcode.Medium)
				if err != nil {
					return err
				}
				state.update(generation, code.Bitmap(), "请使用微信扫描二维码", false, false)
				return nil
			},
			OnStatus: func(status string) {
				if text := loginStatusText(status); text != "" {
					state.update(generation, nil, text, false, false)
				}
			},
			VerifyCode: func(retry bool) (string, error) {
				return state.requestVerifyCode(ctx, retry)
			},
		})
		if err != nil {
			state.update(generation, nil, "登录失败："+err.Error(), true, false)
			return
		}
		if err := clawbot.SaveCredentials(credentials); err != nil {
			state.update(generation, nil, "凭据保存失败："+err.Error(), true, false)
			return
		}
		state.update(generation, nil, "已登录。请在微信中给 ClawBot 发送一条消息以建立主动推送会话", false, true)
	}()
}

func loginStatusText(status string) string {
	switch status {
	case clawbot.StatusScanned:
		return "已扫码，请在微信中确认"
	case clawbot.StatusScannedRedirect:
		return "正在切换扫码节点…"
	case clawbot.StatusConfirmed:
		return "已确认，正在保存凭据…"
	case clawbot.StatusExpired:
		return "二维码已过期，正在重新获取…"
	case clawbot.StatusVerifyBlocked:
		return "配对码错误次数过多，正在重新获取二维码…"
	default:
		return ""
	}
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
	var codeVisible bool
	var codeEdit uintptr
	var hoverClose, hoverRetry, hoverDone, hoverSubmit bool

	wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
		switch uint32(msg) {
		case WM_CREATE:
			setUIDPI(windowDPI(hwnd))
			state.setWindow(hwnd)
			editFont := newBaseFont()
			codeEdit, _, _ = pCreateWindowExW.Call(
				0,
				uintptr(unsafe.Pointer(StringToUTF16Ptr("EDIT"))),
				0,
				WS_CHILD|ES_AUTOHSCROLL|ES_NUMBER,
				uintptr(scaleFloat(layout.code.Left+10)),
				uintptr(scaleFloat(layout.code.Top+6)),
				uintptr(scaleFloat(layout.code.Right-layout.code.Left-20)),
				uintptr(scaleFloat(layout.code.Bottom-layout.code.Top-12)),
				hwnd, loginCodeEditID, hInstance, 0,
			)
			pSendMessageW.Call(codeEdit, WM_SETFONT, editFont, 1)
			startLoginFlow(state)
			pSetTimer.Call(hwnd, loginTimer, 100, 0)
			return 0

		case WM_DPICHANGED:
			setUIDPI(uint32(wParam & 0xFFFF))
			resizeForCurrentDPI(hwnd, loginWidth, loginHeight)
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_USER_VERIFY:
			if codeEdit != 0 {
				setWindowText(codeEdit, "")
				pShowWindow.Call(codeEdit, SW_SHOW)
				user32.NewProc("SetFocus").Call(codeEdit)
				codeVisible = true
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			return 0

		case WM_ERASEBKGND:
			return 1

		case WM_TIMER:
			if wParam == loginTimer {
				_, _, _, _, _, promptActive := state.snapshot()
				if promptActive != codeVisible {
					codeVisible = promptActive
					if codeEdit != 0 {
						if codeVisible {
							pShowWindow.Call(codeEdit, SW_SHOW)
						} else {
							pShowWindow.Call(codeEdit, SW_HIDE)
						}
					}
				}
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
			previousClose, previousRetry, previousDone, previousSubmit := hoverClose, hoverRetry, hoverDone, hoverSubmit
			hoverClose = pointInRect(x, y, layout.winClose)
			hoverRetry = pointInRect(x, y, layout.retry)
			hoverDone = pointInRect(x, y, layout.done)
			hoverSubmit = codeVisible && pointInRect(x, y, layout.codeSubmit)
			if hoverClose != previousClose || hoverRetry != previousRetry || hoverDone != previousDone || hoverSubmit != previousSubmit {
				pInvalidateRect.Call(hwnd, 0, 0)
			}
			if hoverClose || hoverRetry || hoverDone || hoverSubmit {
				hand, _, _ := pLoadCursorW.Call(0, uintptr(IDC_HAND))
				pSetCursor.Call(hand)
			}
			return 0

		case WM_MOUSELEAVE:
			tracking = false
			hoverClose, hoverRetry, hoverDone, hoverSubmit = false, false, false, false
			pInvalidateRect.Call(hwnd, 0, 0)
			return 0

		case WM_LBUTTONDOWN:
			x, y := unscalePoint(int32(lParam&0xFFFF), int32((lParam>>16)&0xFFFF))
			switch {
			case codeVisible && pointInRect(x, y, layout.codeSubmit):
				code := strings.TrimSpace(getWindowText(codeEdit))
				if code == "" {
					return 0
				}
				pShowWindow.Call(codeEdit, SW_HIDE)
				codeVisible = false
				state.submitVerifyCode(code)
				pInvalidateRect.Call(hwnd, 0, 0)
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

				titleFont := newTitleFont()
				baseFont := newBaseFont()
				smallFont := newSmallFont()
				iconFont := newUIIconFont()
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
				DrawText(hdc, "扫码登录后，还需发送一条微信消息建立会话", &RECT{20, 42, 300, 64}, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)
				drawWindowButton(hdc, layout.winClose, "\uE8BB", hoverClose, true, iconFont)

				_, bitmap, status, failure, success, promptActive := state.snapshot()
				drawQRCode(hdc, layout.qr, bitmap)

				statusColor := uintptr(RGB(137, 149, 160))
				if failure {
					statusColor = uintptr(RGB(230, 112, 112))
				} else if success {
					statusColor = uintptr(RGB(55, 190, 147))
				}
				pSelectObject.Call(hdc, baseFont)
				pSetTextColor.Call(hdc, statusColor)
				DrawText(hdc, status, &RECT{24, 344, 336, 378}, DT_CENTER|DT_WORDBREAK|DT_NOPREFIX)

				if promptActive {
					drawFieldFrame(hdc, layout.code)
					drawIconTextButton(hdc, layout.codeSubmit, "\uE73E", "提交", hoverSubmit, true, false, baseFont, iconFont)
				}

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
	winClose   RECT
	qr         RECT
	code       RECT
	codeSubmit RECT
	retry      RECT
	done       RECT
}

func loginLayoutRects() loginLayout {
	return loginLayout{
		winClose:   RECT{320, 8, 352, 40},
		qr:         RECT{45, 78, 315, 334},
		code:       RECT{64, 384, 212, 418},
		codeSubmit: RECT{220, 384, 296, 418},
		retry:      RECT{54, 434, 168, 470},
		done:       RECT{192, 434, 306, 470},
	}
}
