//go:build windows

package ui

import (
	"syscall"
	"unsafe"
)

// TrayManager manages the system tray icon and its context menu.
type TrayManager struct {
	hwnd      uintptr
	hIconOn   uintptr
	hIconMid  uintptr
	hIconOff  uintptr
	lastState int
}

const (
	WM_TRAYICON = WM_USER + 100

	IDM_TOGGLE_SHOW = 2001
	IDM_TOGGLE_OC   = 2002
	IDM_TOGGLE_CX   = 2003
	IDM_TOGGLE_AG   = 2004
	IDM_HISTORY     = 2005
	IDM_SETTINGS    = 2006
	IDM_TEST_PUSH   = 2007
	IDM_SHARE_CARD  = 2008
	IDM_EXIT        = 2009
)

// CreateDotIcon creates a 16x16 colored circle icon.
func CreateDotIcon(color uint32) uintptr {
	hdcScreen, _, _ := user32.NewProc("GetDC").Call(0)
	defer user32.NewProc("ReleaseDC").Call(0, hdcScreen)

	hdcMem, _, _ := pCreateCompatibleDC.Call(hdcScreen)
	defer pDeleteDC.Call(hdcMem)

	hbmColor, _, _ := pCreateCompatibleBitmap.Call(hdcScreen, 16, 16)
	hbmMask, _, _ := gdi32.NewProc("CreateBitmap").Call(16, 16, 1, 1, 0)

	// Draw color bitmap
	hOldBmp, _, _ := pSelectObject.Call(hdcMem, hbmColor)
	hBrBg, _, _ := pCreateSolidBrush.Call(uintptr(RGB(0, 0, 0)))
	rc := RECT{0, 0, 16, 16}
	pFillRect.Call(hdcMem, uintptr(unsafe.Pointer(&rc)), hBrBg)
	pDeleteObject.Call(hBrBg)

	hBrush, _, _ := pCreateSolidBrush.Call(uintptr(color))
	hPen, _, _ := pCreatePen.Call(0, 1, uintptr(RGB(255, 255, 255)))
	hOldBr, _, _ := pSelectObject.Call(hdcMem, hBrush)
	hOldPen, _, _ := pSelectObject.Call(hdcMem, hPen)

	pEllipse.Call(hdcMem, 1, 1, 15, 15)

	pSelectObject.Call(hdcMem, hOldBr)
	pSelectObject.Call(hdcMem, hOldPen)
	pDeleteObject.Call(hBrush)
	pDeleteObject.Call(hPen)
	pSelectObject.Call(hdcMem, hOldBmp)

	// Draw mask bitmap (0 where icon is visible, 1 where transparent)
	hdcMask, _, _ := pCreateCompatibleDC.Call(hdcScreen)
	hOldMaskBmp, _, _ := pSelectObject.Call(hdcMask, hbmMask)
	hWhiteBr, _, _ := pCreateSolidBrush.Call(uintptr(RGB(255, 255, 255)))
	pFillRect.Call(hdcMask, uintptr(unsafe.Pointer(&rc)), hWhiteBr)
	pDeleteObject.Call(hWhiteBr)

	hBlackBr, _, _ := pCreateSolidBrush.Call(uintptr(RGB(0, 0, 0)))
	hBlackPen, _, _ := pCreatePen.Call(0, 1, uintptr(RGB(0, 0, 0)))
	hOldMBr, _, _ := pSelectObject.Call(hdcMask, hBlackBr)
	hOldMPen, _, _ := pSelectObject.Call(hdcMask, hBlackPen)

	pEllipse.Call(hdcMask, 1, 1, 15, 15)

	pSelectObject.Call(hdcMask, hOldMBr)
	pSelectObject.Call(hdcMask, hOldMPen)
	pDeleteObject.Call(hBlackBr)
	pDeleteObject.Call(hBlackPen)
	pSelectObject.Call(hdcMask, hOldMaskBmp)
	pDeleteDC.Call(hdcMask)

	var ii ICONINFO
	ii.FIcon = 1
	ii.HbmMask = hbmMask
	ii.HbmColor = hbmColor

	hIcon, _, _ := pCreateIconIndirect.Call(uintptr(unsafe.Pointer(&ii)))
	pDeleteObject.Call(hbmColor)
	pDeleteObject.Call(hbmMask)

	return hIcon
}

// NewTrayManager initializes tray icons and registers the tray icon.
func NewTrayManager(hwnd uintptr) *TrayManager {
	tm := &TrayManager{
		hwnd:      hwnd,
		hIconOn:   CreateDotIcon(RGB(52, 211, 153)), // Green
		hIconMid:  CreateDotIcon(RGB(255, 170, 60)), // Orange
		hIconOff:  CreateDotIcon(RGB(220, 53, 69)),  // Red
		lastState: -1,
	}

	var nid NOTIFYICONDATAW
	nid.CbSize = uint32(unsafe.Sizeof(nid))
	nid.HWnd = hwnd
	nid.UID = 1
	nid.UFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP
	nid.UCallbackMessage = WM_TRAYICON
	nid.HIcon = tm.hIconOn

	tip, _ := syscall.UTF16FromString("linkWeixin 推送")
	copy(nid.SzTip[:], tip)

	ret, _, _ := pShell_NotifyIconW.Call(NIM_ADD, uintptr(unsafe.Pointer(&nid)))
	if ret == 0 {
		nid.CbSize = 504
		pShell_NotifyIconW.Call(NIM_ADD, uintptr(unsafe.Pointer(&nid)))
	}
	return tm
}

// UpdateState updates the icon based on how many agents are ON (0, 1..2, 3).
func (tm *TrayManager) UpdateState(state int) {
	if tm.lastState == state {
		return
	}
	tm.lastState = state

	hIcon := tm.hIconMid
	if state == 2 { // All on
		hIcon = tm.hIconOn
	} else if state == 0 { // All off
		hIcon = tm.hIconOff
	}

	var nid NOTIFYICONDATAW
	nid.CbSize = uint32(unsafe.Sizeof(nid))
	nid.HWnd = tm.hwnd
	nid.UID = 1
	nid.UFlags = NIF_ICON
	nid.HIcon = hIcon

	pShell_NotifyIconW.Call(NIM_MODIFY, uintptr(unsafe.Pointer(&nid)))
}

// Destroy cleans up the tray icon and GDI handles.
func (tm *TrayManager) Destroy() {
	var nid NOTIFYICONDATAW
	nid.CbSize = uint32(unsafe.Sizeof(nid))
	nid.HWnd = tm.hwnd
	nid.UID = 1
	pShell_NotifyIconW.Call(NIM_DELETE, uintptr(unsafe.Pointer(&nid)))

	if tm.hIconOn != 0 {
		pDestroyIcon.Call(tm.hIconOn)
	}
	if tm.hIconMid != 0 {
		pDestroyIcon.Call(tm.hIconMid)
	}
	if tm.hIconOff != 0 {
		pDestroyIcon.Call(tm.hIconOff)
	}
}

// ShowContextMenu opens the tray context menu at current cursor position.
func (tm *TrayManager) ShowContextMenu(isWindowVisible bool, onOc, onCx, onAg bool) {
	hMenu, _, _ := pCreatePopupMenu.Call()
	if hMenu == 0 {
		return
	}
	defer pDestroyMenu.Call(hMenu)

	showText := "显示悬浮窗"
	if isWindowVisible {
		showText = "隐藏悬浮窗"
	}
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_TOGGLE_SHOW, uintptr(unsafe.Pointer(StringToUTF16Ptr(showText))))

	ocText := "开启 opencode 推送"
	if onOc {
		ocText = "关闭 opencode 推送"
	}
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_TOGGLE_OC, uintptr(unsafe.Pointer(StringToUTF16Ptr(ocText))))

	cxText := "开启 codex 推送"
	if onCx {
		cxText = "关闭 codex 推送"
	}
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_TOGGLE_CX, uintptr(unsafe.Pointer(StringToUTF16Ptr(cxText))))

	agText := "开启 antigravity 推送"
	if onAg {
		agText = "关闭 antigravity 推送"
	}
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_TOGGLE_AG, uintptr(unsafe.Pointer(StringToUTF16Ptr(agText))))

	pAppendMenuW.Call(hMenu, MF_SEPARATOR, 0, 0)
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_HISTORY, uintptr(unsafe.Pointer(StringToUTF16Ptr("推送历史记录"))))
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_SETTINGS, uintptr(unsafe.Pointer(StringToUTF16Ptr("通道与偏好设置"))))
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_TEST_PUSH, uintptr(unsafe.Pointer(StringToUTF16Ptr("发送测试推送"))))
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_SHARE_CARD, uintptr(unsafe.Pointer(StringToUTF16Ptr("复制推荐名片 / 分享"))))
	pAppendMenuW.Call(hMenu, MF_SEPARATOR, 0, 0)
	pAppendMenuW.Call(hMenu, MF_STRING, IDM_EXIT, uintptr(unsafe.Pointer(StringToUTF16Ptr("退出"))))

	var pt POINT
	pGetCursorPos.Call(uintptr(unsafe.Pointer(&pt)))

	pSetForegroundWindow.Call(tm.hwnd)
	pTrackPopupMenu.Call(hMenu, TPM_RIGHTBUTTON, uintptr(pt.X), uintptr(pt.Y), 0, tm.hwnd, 0)
}
