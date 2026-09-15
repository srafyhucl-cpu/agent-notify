//go:build windows

package ui

import (
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
)

type TrayManager struct {
	hwnd      uintptr
	hIconOn   uintptr
	hIconMid  uintptr
	hIconOff  uintptr
	lastState int
}

const (
	WM_TRAYICON = WM_USER + 100

	IDM_TOGGLE_SHOW        = 3001
	IDM_TOGGLE_OPENCODE    = 3002
	IDM_TOGGLE_CODEX       = 3003
	IDM_TOGGLE_ANTIGRAVITY = 3004
	IDM_TOGGLE_DEVIN       = 3005
	IDM_HISTORY            = 3006
	IDM_SETTINGS           = 3007
	IDM_TEST_PUSH          = 3008
	IDM_EXIT               = 3009
	IDM_UPDATE             = 3010
)

// CreateDotIcon creates a 16x16 colored tray status icon.
func CreateDotIcon(color uint32) uintptr {
	hdcScreen, _, _ := user32.NewProc("GetDC").Call(0)
	defer user32.NewProc("ReleaseDC").Call(0, hdcScreen)

	hdcMem, _, _ := pCreateCompatibleDC.Call(hdcScreen)
	defer pDeleteDC.Call(hdcMem)

	hBitmapColor, _, _ := pCreateCompatibleBitmap.Call(hdcScreen, 16, 16)
	hBitmapMask, _, _ := gdi32.NewProc("CreateBitmap").Call(16, 16, 1, 1, 0)

	oldBitmap, _, _ := pSelectObject.Call(hdcMem, hBitmapColor)
	backgroundBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(0, 0, 0)))
	rect := RECT{0, 0, 16, 16}
	pFillRect.Call(hdcMem, uintptr(unsafe.Pointer(&rect)), backgroundBrush)
	pDeleteObject.Call(backgroundBrush)

	brush, _, _ := pCreateSolidBrush.Call(uintptr(color))
	pen, _, _ := pCreatePen.Call(0, 1, uintptr(RGB(255, 255, 255)))
	oldBrush, _, _ := pSelectObject.Call(hdcMem, brush)
	oldPen, _, _ := pSelectObject.Call(hdcMem, pen)
	pEllipse.Call(hdcMem, 1, 1, 15, 15)
	pSelectObject.Call(hdcMem, oldBrush)
	pSelectObject.Call(hdcMem, oldPen)
	pDeleteObject.Call(brush)
	pDeleteObject.Call(pen)
	pSelectObject.Call(hdcMem, oldBitmap)

	hdcMask, _, _ := pCreateCompatibleDC.Call(hdcScreen)
	oldMask, _, _ := pSelectObject.Call(hdcMask, hBitmapMask)
	whiteBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(255, 255, 255)))
	pFillRect.Call(hdcMask, uintptr(unsafe.Pointer(&rect)), whiteBrush)
	pDeleteObject.Call(whiteBrush)

	blackBrush, _, _ := pCreateSolidBrush.Call(uintptr(RGB(0, 0, 0)))
	blackPen, _, _ := pCreatePen.Call(0, 1, uintptr(RGB(0, 0, 0)))
	oldMaskBrush, _, _ := pSelectObject.Call(hdcMask, blackBrush)
	oldMaskPen, _, _ := pSelectObject.Call(hdcMask, blackPen)
	pEllipse.Call(hdcMask, 1, 1, 15, 15)
	pSelectObject.Call(hdcMask, oldMaskBrush)
	pSelectObject.Call(hdcMask, oldMaskPen)
	pDeleteObject.Call(blackBrush)
	pDeleteObject.Call(blackPen)
	pSelectObject.Call(hdcMask, oldMask)
	pDeleteDC.Call(hdcMask)

	iconInfo := ICONINFO{FIcon: 1, HbmMask: hBitmapMask, HbmColor: hBitmapColor}
	icon, _, _ := pCreateIconIndirect.Call(uintptr(unsafe.Pointer(&iconInfo)))
	pDeleteObject.Call(hBitmapColor)
	pDeleteObject.Call(hBitmapMask)
	return icon
}

func NewTrayManager(hwnd uintptr) *TrayManager {
	manager := &TrayManager{
		hwnd:      hwnd,
		hIconOn:   CreateDotIcon(RGB(54, 190, 144)),
		hIconMid:  CreateDotIcon(RGB(224, 165, 70)),
		hIconOff:  CreateDotIcon(RGB(220, 92, 92)),
		lastState: -1,
	}

	var data NOTIFYICONDATAW
	data.CbSize = uint32(unsafe.Sizeof(data))
	data.HWnd = hwnd
	data.UID = 1
	data.UFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP
	data.UCallbackMessage = WM_TRAYICON
	data.HIcon = manager.hIconOn
	tip, _ := syscall.UTF16FromString("Agent-notify")
	copy(data.SzTip[:], tip)
	if result, _, _ := pShell_NotifyIconW.Call(NIM_ADD, uintptr(unsafe.Pointer(&data))); result == 0 {
		data.CbSize = 504
		pShell_NotifyIconW.Call(NIM_ADD, uintptr(unsafe.Pointer(&data)))
	}
	return manager
}

func (manager *TrayManager) UpdateState(state int) {
	if manager.lastState == state {
		return
	}
	manager.lastState = state
	icon := manager.hIconMid
	if state == 2 {
		icon = manager.hIconOn
	} else if state == 0 {
		icon = manager.hIconOff
	}
	var data NOTIFYICONDATAW
	data.CbSize = uint32(unsafe.Sizeof(data))
	data.HWnd = manager.hwnd
	data.UID = 1
	data.UFlags = NIF_ICON
	data.HIcon = icon
	pShell_NotifyIconW.Call(NIM_MODIFY, uintptr(unsafe.Pointer(&data)))
}

func (manager *TrayManager) Destroy() {
	var data NOTIFYICONDATAW
	data.CbSize = uint32(unsafe.Sizeof(data))
	data.HWnd = manager.hwnd
	data.UID = 1
	pShell_NotifyIconW.Call(NIM_DELETE, uintptr(unsafe.Pointer(&data)))
	if manager.hIconOn != 0 {
		pDestroyIcon.Call(manager.hIconOn)
	}
	if manager.hIconMid != 0 {
		pDestroyIcon.Call(manager.hIconMid)
	}
	if manager.hIconOff != 0 {
		pDestroyIcon.Call(manager.hIconOff)
	}
}

func (manager *TrayManager) ShowContextMenu(windowVisible bool, agentEnabled map[string]bool) {
	menu, _, _ := pCreatePopupMenu.Call()
	if menu == 0 {
		return
	}
	defer pDestroyMenu.Call(menu)

	showText := "显示 Agent-notify"
	if windowVisible {
		showText = "隐藏 Agent-notify"
	}
	pAppendMenuW.Call(menu, MF_STRING, IDM_TOGGLE_SHOW, uintptr(unsafe.Pointer(StringToUTF16Ptr(showText))))

	for _, descriptor := range agentmeta.All() {
		commandID, ok := trayCommandForAgent(descriptor.ID)
		if !ok {
			continue
		}
		label := "开启 " + descriptor.DisplayName + " 推送"
		if agentEnabled[descriptor.ID] {
			label = "暂停 " + descriptor.DisplayName + " 推送"
		}
		pAppendMenuW.Call(menu, MF_STRING, commandID, uintptr(unsafe.Pointer(StringToUTF16Ptr(label))))
	}

	pAppendMenuW.Call(menu, MF_SEPARATOR, 0, 0)
	pAppendMenuW.Call(menu, MF_STRING, IDM_HISTORY, uintptr(unsafe.Pointer(StringToUTF16Ptr("推送历史"))))
	pAppendMenuW.Call(menu, MF_STRING, IDM_SETTINGS, uintptr(unsafe.Pointer(StringToUTF16Ptr("设置与 ClawBot 登录"))))
	pAppendMenuW.Call(menu, MF_STRING, IDM_TEST_PUSH, uintptr(unsafe.Pointer(StringToUTF16Ptr("发送测试推送"))))
	pAppendMenuW.Call(menu, MF_STRING, IDM_UPDATE, uintptr(unsafe.Pointer(StringToUTF16Ptr("检查更新"))))
	pAppendMenuW.Call(menu, MF_SEPARATOR, 0, 0)
	pAppendMenuW.Call(menu, MF_STRING, IDM_EXIT, uintptr(unsafe.Pointer(StringToUTF16Ptr("退出"))))

	var point POINT
	pGetCursorPos.Call(uintptr(unsafe.Pointer(&point)))
	pSetForegroundWindow.Call(manager.hwnd)
	pTrackPopupMenu.Call(menu, TPM_RIGHTBUTTON, uintptr(point.X), uintptr(point.Y), 0, manager.hwnd, 0)
}

type trayAgentCommand struct {
	ID      uintptr
	AgentID string
}

var trayAgentCommands = []trayAgentCommand{
	{ID: IDM_TOGGLE_OPENCODE, AgentID: agentmeta.OpenCode},
	{ID: IDM_TOGGLE_CODEX, AgentID: agentmeta.Codex},
	{ID: IDM_TOGGLE_ANTIGRAVITY, AgentID: agentmeta.Antigravity},
	{ID: IDM_TOGGLE_DEVIN, AgentID: agentmeta.Devin},
}

func trayAgentIDForCommand(commandID int) (string, bool) {
	for _, command := range trayAgentCommands {
		if command.ID == uintptr(commandID) {
			return command.AgentID, true
		}
	}
	return "", false
}

func trayCommandForAgent(agentID string) (uintptr, bool) {
	for _, command := range trayAgentCommands {
		if command.AgentID == agentID {
			return command.ID, true
		}
	}
	return 0, false
}
