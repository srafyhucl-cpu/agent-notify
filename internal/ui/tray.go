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
	IDM_TOGGLE_COMMANDCODE = 3011
	IDM_HISTORY            = 3006
	IDM_SETTINGS           = 3007
	IDM_TEST_PUSH          = 3008
	IDM_EXIT               = 3009
	IDM_UPDATE             = 3010
)

func NewTrayManager(hwnd uintptr) *TrayManager {
	manager := &TrayManager{
		hwnd:      hwnd,
		hIconOn:   CreateStatusIcon(statusColorReady),
		hIconMid:  CreateStatusIcon(statusColorWarning),
		hIconOff:  CreateStatusIcon(statusColorStopped),
		lastState: -1,
	}

	var data NOTIFYICONDATAW
	data.CbSize = uint32(unsafe.Sizeof(data))
	data.HWnd = hwnd
	data.UID = 1
	data.UFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP
	data.UCallbackMessage = WM_TRAYICON
	data.HIcon = manager.hIconOn
	tip, _ := syscall.UTF16FromString("AgentNotify")
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

const (
	// NOTIFYICONDATAW 的 SzInfoTitle 与 SzInfo 容量含结尾 NUL，有效上限是 63 / 255。
	notifInfoTitleLimit = 63
	notifInfoBodyLimit  = 255
)

// truncateNotifText 把通知文案裁剪到 NOTIFYICONDATAW 能容纳的上限。
// copy 到定长数组时超长会被静默截断，先显式裁剪，避免丢掉半个字形后难以排查。
func truncateNotifText(text string, limit int) string {
	runes := []rune(text)
	if len(runes) <= limit {
		return text
	}
	return string(runes[:limit])
}

// ShowAlert 通过托盘图标弹出一条气泡通知，由系统决定展示时长。
// Win10/11 会把气泡并入通知中心，用户错过也能回看。
func (manager *TrayManager) ShowAlert(title, body string) {
	if manager.hwnd == 0 {
		return
	}
	var data NOTIFYICONDATAW
	data.CbSize = uint32(unsafe.Sizeof(data))
	data.HWnd = manager.hwnd
	data.UID = 1
	data.UFlags = NIF_INFO
	data.DwInfoFlags = NIIF_WARNING
	if text, err := syscall.UTF16FromString(truncateNotifText(body, notifInfoBodyLimit)); err == nil {
		copy(data.SzInfo[:], text)
	}
	if text, err := syscall.UTF16FromString(truncateNotifText(title, notifInfoTitleLimit)); err == nil {
		copy(data.SzInfoTitle[:], text)
	}
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

	showText := "显示 AgentNotify"
	if windowVisible {
		showText = "隐藏 AgentNotify"
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
	{ID: IDM_TOGGLE_COMMANDCODE, AgentID: agentmeta.CommandCode},
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
