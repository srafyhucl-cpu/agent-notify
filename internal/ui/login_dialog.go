//go:build windows

package ui

import (
	"context"
	"sync"
	"time"
	"unsafe"

	qrcode "github.com/skip2/go-qrcode"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

const loginCodeEditID = 4301

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
	if generation != state.generation {
		state.mu.Unlock()
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
	hwnd := state.hwnd
	state.mu.Unlock()
	if hwnd != 0 {
		pPostMessageW.Call(hwnd, WM_USER_REFRESH, 0, 0)
	}
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

// cancel 无条件结束当前扫码流程（离开登录视图时调用）。
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
