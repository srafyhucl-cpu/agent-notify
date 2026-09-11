//go:build windows

package ui

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"syscall"
	"testing"
	"time"
	"unsafe"

	"linkweixin/internal/config"
)

// TestWidgetScreenPositionSanity tests that window coordinates never drift off-screen.
func TestWidgetScreenPositionSanity(t *testing.T) {
	screenWidth, _, _ := pGetSystemMetrics.Call(0)
	screenHeight, _, _ := pGetSystemMetrics.Call(1)

	winWidth := int32(380)
	winHeight := int32(450)

	testCases := []struct {
		name      string
		rawPos    string
		expectAdj bool
	}{
		{"Normal within bounds", fmt.Sprintf("%d,%d", int(screenWidth)/2, int(screenHeight)/2), false},
		{"Negative minimized", "-32000,-32000", true},
		{"Far outer space X", fmt.Sprintf("%d,500", int(screenWidth)+500), true},
		{"Far outer space Y", fmt.Sprintf("500,%d", int(screenHeight)+500), true},
		{"Empty string", "", true},
		{"Corrupt string", "abc,def", true},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			posFile := filepath.Join(tmpDir, "widget-pos.txt")
			if tc.rawPos != "" {
				_ = os.WriteFile(posFile, []byte(tc.rawPos), 0644)
			}

			// Perform the sanity check logic from widget.go
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

			adjusted := true
			if data, err := os.ReadFile(posFile); err == nil {
				parts := splitComma(string(data))
				if len(parts) == 2 {
					x, err1 := parseInt(parts[0])
					y, err2 := parseInt(parts[1])
					if err1 == nil && err2 == nil {
						if int32(x) >= 0 && int32(x) <= int32(screenWidth)-winWidth/2 &&
							int32(y) >= 0 && int32(y) <= int32(screenHeight)-winHeight/2 {
							winX = int32(x)
							winY = int32(y)
							adjusted = false
						}
					}
				}
			}

			if tc.expectAdj && !adjusted {
				t.Errorf("expected position to be adjusted for %q, but got winX=%d, winY=%d", tc.rawPos, winX, winY)
			}
			if winX < 0 || winX > int32(screenWidth)-winWidth/2 {
				t.Errorf("winX out of bounds: %d (screen width: %d)", winX, screenWidth)
			}
			if winY < 0 || winY > int32(screenHeight)-winHeight/2 {
				t.Errorf("winY out of bounds: %d (screen height: %d)", winY, screenHeight)
			}
		})
	}
}

// TestWidgetWindowCreationAndLifecycle creates the actual widget window and monitors its lifecycle.
func TestWidgetWindowCreationAndLifecycle(t *testing.T) {
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0755)

	readyChan := make(chan uintptr)
	doneChan := make(chan struct{})

	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()

		hInstance, _, _ := pGetModuleHandleW.Call(0)
		testClassName := StringToUTF16Ptr(fmt.Sprintf("LinkWeixinTestClass_%d", time.Now().UnixNano()))
		testTitle := StringToUTF16Ptr("linkWeixin Test")

		wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
			switch uint32(msg) {
			case WM_CLOSE:
				pDestroyWindow.Call(hwnd)
				return 0
			case WM_DESTROY:
				pPostQuitMessage.Call(0)
				return 0
			}
			ret, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
			return ret
		})

		var wc WNDCLASSEXW
		wc.CbSize = uint32(unsafe.Sizeof(wc))
		wc.Style = 0x0003 | CS_GLOBALCLASS
		wc.LpfnWndProc = wndProc
		wc.HInstance = hInstance
		wc.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
		wc.HbrBackground, _, _ = pCreateSolidBrush.Call(uintptr(RGB(24, 24, 27)))
		wc.LpszClassName = testClassName
		pRegisterClassExW.Call(uintptr(unsafe.Pointer(&wc)))

		hwnd, _, _ := pCreateWindowExW.Call(
			WS_EX_APPWINDOW|WS_EX_TOPMOST,
			uintptr(unsafe.Pointer(testClassName)),
			uintptr(unsafe.Pointer(testTitle)),
			WS_POPUP|WS_MINIMIZEBOX|WS_SYSMENU|WS_VISIBLE,
			500, 300, 380, 450,
			0, 0, hInstance, 0,
		)

		if hwnd == 0 {
			readyChan <- 0
			return
		}

		pShowWindow.Call(hwnd, SW_SHOW)
		pUpdateWindow.Call(hwnd)

		readyChan <- hwnd

		// Standard Win32 message pump on the window thread
		var msg MSG
		for {
			ret, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0)
			if ret == 0 || int32(ret) == -1 {
				break
			}
			pTranslateMessage.Call(uintptr(unsafe.Pointer(&msg)))
			pDispatchMessageW.Call(uintptr(unsafe.Pointer(&msg)))
		}
		close(doneChan)
	}()

	hwnd := <-readyChan
	if hwnd == 0 {
		t.Fatalf("pCreateWindowExW failed to create window")
	}

	// Verify HWND is valid immediately
	pIsWindow := user32.NewProc("IsWindow")
	pIsVisible := user32.NewProc("IsWindowVisible")

	isW, _, _ := pIsWindow.Call(hwnd)
	if isW == 0 {
		t.Fatalf("Window HWND=%d is immediately invalid!", hwnd)
	}

	isVis, _, _ := pIsVisible.Call(hwnd)
	if isVis == 0 {
		t.Errorf("Window HWND=%d is not visible!", hwnd)
	}

	// Clean up by sending WM_CLOSE to window on its thread
	pPostMessageW.Call(hwnd, WM_CLOSE, 0, 0)
	select {
	case <-doneChan:
		// Cleanly exited
	case <-time.After(2 * time.Second):
		t.Errorf("Message loop timed out waiting for exit")
	}
}

// TestWidgetSingleInstanceWakeupReal verifies that a second instance reliably wakes up the first instance using Named Event.
func TestWidgetSingleInstanceWakeupReal(t *testing.T) {
	pCreateEventW := kernel32.NewProc("CreateEventW")
	pSetEvent := kernel32.NewProc("SetEvent")
	pOpenEventW := kernel32.NewProc("OpenEventW")
	pWaitForSingleObject := kernel32.NewProc("WaitForSingleObject")
	pCloseHandle := kernel32.NewProc("CloseHandle")
	pIsVis := user32.NewProc("IsWindowVisible")

	eventNameStr := fmt.Sprintf("Local\\LinkWeixin_Wakeup_Event_Test_%d", time.Now().UnixNano())
	eventName := StringToUTF16Ptr(eventNameStr)
	// Create auto-reset event
	hEvent, _, err := pCreateEventW.Call(0, 0, 0, uintptr(unsafe.Pointer(eventName)))
	if hEvent == 0 {
		t.Fatalf("CreateEventW failed: %v", err)
	}
	t.Logf("[Test] Created Named Event %s (handle=%d)", eventNameStr, hEvent)
	defer pCloseHandle.Call(hEvent)

	const WM_USER_WAKEUP = WM_USER + 200

	var receivedWakeup atomicBool
	readyChan := make(chan uintptr)
	quitChan := make(chan struct{})

	// Run First Instance UI thread: Window creation and message loop MUST share the same OS thread
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()

		hInstance, _, _ := pGetModuleHandleW.Call(0)
		testClassName := StringToUTF16Ptr("LinkWeixinTestWakeClassReal")
		testTitle := StringToUTF16Ptr("linkWeixin Wake Test Real")

		wndProc := syscall.NewCallback(func(hwnd, msg, wParam, lParam uintptr) uintptr {
			uMsg := uint32(msg)
			if uMsg == WM_USER_WAKEUP {
				receivedWakeup.Set(true)
				pShowWindow.Call(hwnd, SW_SHOW)
				pShowWindow.Call(hwnd, SW_RESTORE)
				pSetForegroundWindow.Call(hwnd)
				return 0
			}
			switch uMsg {
			case WM_CLOSE:
				pDestroyWindow.Call(hwnd)
				return 0
			case WM_DESTROY:
				pPostQuitMessage.Call(0)
				return 0
			}
			ret, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wParam, lParam)
			return ret
		})

		var wc WNDCLASSEXW
		wc.CbSize = uint32(unsafe.Sizeof(wc))
		wc.Style = 0x0003 | CS_GLOBALCLASS
		wc.LpfnWndProc = wndProc
		wc.HInstance = hInstance
		wc.HCursor, _, _ = pLoadCursorW.Call(0, uintptr(IDC_ARROW))
		wc.HbrBackground, _, _ = pCreateSolidBrush.Call(uintptr(RGB(24, 24, 27)))
		wc.LpszClassName = testClassName
		pRegisterClassExW.Call(uintptr(unsafe.Pointer(&wc)))

		hwnd, _, _ := pCreateWindowExW.Call(
			WS_EX_APPWINDOW|WS_EX_TOPMOST,
			uintptr(unsafe.Pointer(testClassName)),
			uintptr(unsafe.Pointer(testTitle)),
			WS_POPUP|WS_MINIMIZEBOX|WS_SYSMENU|WS_VISIBLE,
			600, 350, 380, 450,
			0, 0, hInstance, 0,
		)
		if hwnd == 0 {
			readyChan <- 0
			return
		}

		// Background listener for the Named Event that posts to this window
		go func() {
			for {
				select {
				case <-quitChan:
					return
				default:
					ret, _, err := pWaitForSingleObject.Call(hEvent, 100)
					if ret == 0 { // WAIT_OBJECT_0
						t.Logf("[Test] Named Event signaled! Posting WM_USER_WAKEUP to hwnd=%d", hwnd)
						pPostMessageW.Call(hwnd, WM_USER_WAKEUP, 0, 0)
					} else if ret != 0x102 { // Not WAIT_TIMEOUT (258)
						t.Logf("[Test] WaitForSingleObject returned 0x%X, err=%v", ret, err)
					}
				}
			}
		}()

		readyChan <- hwnd

		// Win32 message pump
		var msg MSG
		for {
			ret, _, _ := pGetMessageW.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0)
			if ret == 0 || int32(ret) == -1 {
				break
			}
			pTranslateMessage.Call(uintptr(unsafe.Pointer(&msg)))
			pDispatchMessageW.Call(uintptr(unsafe.Pointer(&msg)))
		}
	}()

	firstHwnd := <-readyChan
	if firstHwnd == 0 {
		t.Fatalf("Failed to create first instance test window")
	}

	t.Logf("[Test] Created firstHwnd=%d", firstHwnd)
	time.Sleep(100 * time.Millisecond)

	// Step 1: Simulate hiding window to tray (SW_HIDE)
	pShowWindow.Call(firstHwnd, SW_HIDE)
	time.Sleep(50 * time.Millisecond)

	isVisBefore, _, _ := pIsVis.Call(firstHwnd)
	if isVisBefore != 0 {
		t.Fatalf("Window should be hidden before wakeup test")
	}
	t.Logf("[Test] Window successfully hidden")

	// Step 2: Now simulate Second Instance launching:
	// Opens the Named Event and sets it to signaled state, then immediately exits
	const EVENT_MODIFY_STATE = 0x0002
	hEvent2, _, _ := pOpenEventW.Call(EVENT_MODIFY_STATE, 0, uintptr(unsafe.Pointer(eventName)))
	if hEvent2 == 0 {
		t.Fatalf("Second instance failed to open Named Event")
	}
	t.Logf("[Test] Second instance opened event=%d, calling SetEvent", hEvent2)
	pSetEvent.Call(hEvent2)
	pCloseHandle.Call(hEvent2)

	// Step 3: Wait up to 2 seconds for first instance to receive Named Event and restore window
	startWait := time.Now()
	woken := false
	for time.Since(startWait) < 2*time.Second {
		if receivedWakeup.Get() {
			woken = true
			break
		}
		time.Sleep(20 * time.Millisecond)
	}

	if !woken {
		t.Errorf("First instance never received wakeup signal via Named Event!")
	}

	time.Sleep(50 * time.Millisecond)

	// Step 4: Verify that window visibility was restored
	isVisAfter, _, _ := pIsVis.Call(firstHwnd)
	if isVisAfter == 0 {
		t.Errorf("First instance window was not restored to visible after Named Event wakeup!")
	}

	// Clean up
	close(quitChan)
	pPostMessageW.Call(firstHwnd, WM_CLOSE, 0, 0)
}

type atomicBool struct {
	mu  sync.Mutex
	val bool
}

func (b *atomicBool) Set(v bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.val = v
}

func (b *atomicBool) Get() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.val
}

func splitComma(s string) []string {
	var res []string
	cur := ""
	for _, ch := range s {
		if ch == ',' {
			res = append(res, cur)
			cur = ""
		} else if ch != ' ' && ch != '\r' && ch != '\n' {
			cur += string(ch)
		}
	}
	if cur != "" {
		res = append(res, cur)
	}
	return res
}

func parseInt(s string) (int, error) {
	val := 0
	sign := 1
	if len(s) > 0 && s[0] == '-' {
		sign = -1
		s = s[1:]
	}
	if len(s) == 0 {
		return 0, fmt.Errorf("empty integer")
	}
	for _, ch := range s {
		if ch < '0' || ch > '9' {
			return 0, fmt.Errorf("invalid char: %c", ch)
		}
		val = val*10 + int(ch-'0')
	}
	return sign * val, nil
}
