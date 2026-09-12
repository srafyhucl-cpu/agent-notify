//go:build windows

package ui

import (
	"syscall"
	"unsafe"
)

var (
	user32   = syscall.NewLazyDLL("user32.dll")
	gdi32    = syscall.NewLazyDLL("gdi32.dll")
	shell32  = syscall.NewLazyDLL("shell32.dll")
	kernel32 = syscall.NewLazyDLL("kernel32.dll")
	dwmapi   = syscall.NewLazyDLL("dwmapi.dll")

	pGetModuleHandleW         = kernel32.NewProc("GetModuleHandleW")
	pCreateMutexW             = kernel32.NewProc("CreateMutexW")
	pCreateEventW             = kernel32.NewProc("CreateEventW")
	pOpenEventW               = kernel32.NewProc("OpenEventW")
	pSetEvent                 = kernel32.NewProc("SetEvent")
	pWaitForSingleObject      = kernel32.NewProc("WaitForSingleObject")
	pCloseHandle              = kernel32.NewProc("CloseHandle")
	pAttachConsole            = kernel32.NewProc("AttachConsole")
	pCreateToolhelp32Snapshot = kernel32.NewProc("CreateToolhelp32Snapshot")
	pProcess32FirstW          = kernel32.NewProc("Process32FirstW")
	pProcess32NextW           = kernel32.NewProc("Process32NextW")

	pOpenDesktopW           = user32.NewProc("OpenDesktopW")
	pSetThreadDesktop       = user32.NewProc("SetThreadDesktop")
	pRegisterClassExW       = user32.NewProc("RegisterClassExW")
	pCreateWindowExW        = user32.NewProc("CreateWindowExW")
	pDefWindowProcW         = user32.NewProc("DefWindowProcW")
	pDestroyWindow          = user32.NewProc("DestroyWindow")
	pShowWindow             = user32.NewProc("ShowWindow")
	pUpdateWindow           = user32.NewProc("UpdateWindow")
	pSetForegroundWindow    = user32.NewProc("SetForegroundWindow")
	pEnableWindow           = user32.NewProc("EnableWindow")
	pIsChild                = user32.NewProc("IsChild")
	pUnregisterClassW       = user32.NewProc("UnregisterClassW")
	pGetMessageW            = user32.NewProc("GetMessageW")
	pTranslateMessage       = user32.NewProc("TranslateMessage")
	pDispatchMessageW       = user32.NewProc("DispatchMessageW")
	pPostQuitMessage        = user32.NewProc("PostQuitMessage")
	pPostMessageW           = user32.NewProc("PostMessageW")
	pSendMessageW           = user32.NewProc("SendMessageW")
	pBeginPaint             = user32.NewProc("BeginPaint")
	pEndPaint               = user32.NewProc("EndPaint")
	pInvalidateRect         = user32.NewProc("InvalidateRect")
	pSetTimer               = user32.NewProc("SetTimer")
	pKillTimer              = user32.NewProc("KillTimer")
	pGetClientRect          = user32.NewProc("GetClientRect")
	pGetWindowRect          = user32.NewProc("GetWindowRect")
	pSetWindowPos           = user32.NewProc("SetWindowPos")
	pGetCursorPos           = user32.NewProc("GetCursorPos")
	pScreenToClient         = user32.NewProc("ScreenToClient")
	pSetCursor              = user32.NewProc("SetCursor")
	pLoadCursorW            = user32.NewProc("LoadCursorW")
	pReleaseCapture         = user32.NewProc("ReleaseCapture")
	pTrackMouseEvent        = user32.NewProc("TrackMouseEvent")
	pCreatePopupMenu        = user32.NewProc("CreatePopupMenu")
	pDestroyMenu            = user32.NewProc("DestroyMenu")
	pAppendMenuW            = user32.NewProc("AppendMenuW")
	pTrackPopupMenu         = user32.NewProc("TrackPopupMenu")
	pMessageBoxW            = user32.NewProc("MessageBoxW")
	pSetClipboardData       = user32.NewProc("SetClipboardData")
	pOpenClipboard          = user32.NewProc("OpenClipboard")
	pCloseClipboard         = user32.NewProc("CloseClipboard")
	pEmptyClipboard         = user32.NewProc("EmptyClipboard")
	pRegisterWindowMessageW = user32.NewProc("RegisterWindowMessageW")
	pSetWindowTextW         = user32.NewProc("SetWindowTextW")
	pGetSystemMetrics       = user32.NewProc("GetSystemMetrics")
	pGetDpiForSystem        = user32.NewProc("GetDpiForSystem")
	pGetDpiForWindow        = user32.NewProc("GetDpiForWindow")
	pCreateIconIndirect     = user32.NewProc("CreateIconIndirect")
	pDestroyIcon            = user32.NewProc("DestroyIcon")

	pCreateCompatibleDC     = gdi32.NewProc("CreateCompatibleDC")
	pCreateCompatibleBitmap = gdi32.NewProc("CreateCompatibleBitmap")
	pSelectObject           = gdi32.NewProc("SelectObject")
	pDeleteObject           = gdi32.NewProc("DeleteObject")
	pDeleteDC               = gdi32.NewProc("DeleteDC")
	pBitBlt                 = gdi32.NewProc("BitBlt")
	pCreateSolidBrush       = gdi32.NewProc("CreateSolidBrush")
	pCreatePen              = gdi32.NewProc("CreatePen")
	pFillRect               = user32.NewProc("FillRect")
	pGetDC                  = user32.NewProc("GetDC")
	pReleaseDC              = user32.NewProc("ReleaseDC")
	pRoundRect              = gdi32.NewProc("RoundRect")
	pEllipse                = gdi32.NewProc("Ellipse")
	pSetBkMode              = gdi32.NewProc("SetBkMode")
	pSetTextColor           = gdi32.NewProc("SetTextColor")
	pSetBkColor             = gdi32.NewProc("SetBkColor")
	pCreateFontW            = gdi32.NewProc("CreateFontW")
	pGetDeviceCaps          = gdi32.NewProc("GetDeviceCaps")
	pDrawTextW              = user32.NewProc("DrawTextW")
	pGetTextExtentPoint32W  = gdi32.NewProc("GetTextExtentPoint32W")

	pShell_NotifyIconW     = shell32.NewProc("Shell_NotifyIconW")
	pDwmSetWindowAttribute = dwmapi.NewProc("DwmSetWindowAttribute")
)

const (
	WS_OVERLAPPED    = 0x00000000
	WS_POPUP         = 0x80000000
	WS_CHILD         = 0x40000000
	WS_MINIMIZEBOX   = 0x00020000
	WS_SYSMENU       = 0x00080000
	WS_VISIBLE       = 0x10000000
	WS_EX_TOOLWINDOW = 0x00000080
	WS_EX_APPWINDOW  = 0x00040000
	WS_EX_TOPMOST    = 0x00000008

	SW_HIDE       = 0
	SW_SHOWNORMAL = 1
	SW_SHOW       = 5
	SW_MINIMIZE   = 6
	SW_RESTORE    = 9

	WM_CREATE        = 0x0001
	WM_DESTROY       = 0x0002
	WM_PAINT         = 0x000F
	WM_CLOSE         = 0x0010
	WM_ERASEBKGND    = 0x0014
	WM_DPICHANGED    = 0x02E0
	WM_KEYDOWN       = 0x0100
	WM_TIMER         = 0x0113
	WM_MOUSEMOVE     = 0x0200
	WM_LBUTTONDOWN   = 0x0201
	WM_LBUTTONUP     = 0x0202
	WM_LBUTTONDBLCLK = 0x0203
	WM_RBUTTONUP     = 0x0205
	WM_MOUSEWHEEL    = 0x020A
	WM_MOUSELEAVE    = 0x02A3
	WM_NCLBUTTONDOWN = 0x00A1
	WM_USER          = 0x0400
	EM_SETCUEBANNER  = 0x1501
	WM_COMMAND       = 0x0111
	WM_USER_WAKEUP   = WM_USER + 200

	EVENT_MODIFY_STATE = 0x0002
	WAIT_OBJECT_0      = 0x00000000

	HTCAPTION = 2

	CS_GLOBALCLASS = 0x4000
	HWND_BROADCAST = 0xFFFF

	SWP_NOSIZE     = 0x0001
	SWP_NOMOVE     = 0x0002
	SWP_NOZORDER   = 0x0004
	SWP_NOACTIVATE = 0x0010
	SWP_SHOWWINDOW = 0x0040
	HWND_TOPMOST   = ^uintptr(0) // -1

	MF_STRING       = 0x00000000
	MF_SEPARATOR    = 0x00000800
	TPM_RIGHTBUTTON = 0x0002

	DT_CENTER       = 0x00000001
	DT_RIGHT        = 0x00000002
	DT_VCENTER      = 0x00000004
	DT_SINGLELINE   = 0x00000020
	DT_WORDBREAK    = 0x00000010
	DT_NOPREFIX     = 0x00000800
	DT_END_ELLIPSIS = 0x00008000

	TRANSPARENT = 1

	NIM_ADD    = 0x00000000
	NIM_MODIFY = 0x00000001
	NIM_DELETE = 0x00000002

	NIF_MESSAGE = 0x00000001
	NIF_ICON    = 0x00000002
	NIF_TIP     = 0x00000004

	MB_OK           = 0x00000000
	MB_YESNO        = 0x00000004
	MB_ICONQUESTION = 0x00000020
	MB_ICONINFO     = 0x00000040
	IDYES           = 6

	IDC_ARROW = 32512
	IDC_HAND  = 32649

	SRCCOPY = 0x00CC0020
)

type WNDCLASSEXW struct {
	CbSize        uint32
	Style         uint32
	LpfnWndProc   uintptr
	CbClsExtra    int32
	CbWndExtra    int32
	HInstance     uintptr
	HIcon         uintptr
	HCursor       uintptr
	HbrBackground uintptr
	LpszMenuName  *uint16
	LpszClassName *uint16
	HIconSm       uintptr
}

type POINT struct {
	X int32
	Y int32
}

type RECT struct {
	Left   int32
	Top    int32
	Right  int32
	Bottom int32
}

type MSG struct {
	Hwnd    uintptr
	Message uint32
	WParam  uintptr
	LParam  uintptr
	Time    uint32
	Pt      POINT
}

type PAINTSTRUCT struct {
	Hdc         uintptr
	FErase      int32
	RcPaint     RECT
	FRestore    int32
	FIncUpdate  int32
	RgbReserved [32]byte
}

type NOTIFYICONDATAW struct {
	CbSize           uint32
	HWnd             uintptr
	UID              uint32
	UFlags           uint32
	UCallbackMessage uint32
	HIcon            uintptr
	SzTip            [128]uint16
	DwState          uint32
	DwStateMask      uint32
	SzInfo           [256]uint16
	UVersion         uint32
	SzInfoTitle      [64]uint16
	DwInfoFlags      uint32
	GuidItem         [16]byte
	HBalloonIcon     uintptr
}

type TRACKMOUSEEVENT struct {
	CbSize      uint32
	DwFlags     uint32
	HWndTrack   uintptr
	DwHoverTime uint32
}

type SIZE struct {
	CX int32
	CY int32
}

type ICONINFO struct {
	FIcon    int32
	XHotspot uint32
	YHotspot uint32
	HbmMask  uintptr
	HbmColor uintptr
}

type PROCESSENTRY32W struct {
	DwSize              uint32
	CntUsage            uint32
	Th32ProcessID       uint32
	Th32DefaultHeapID   uintptr
	Th32ModuleID        uint32
	CntThreads          uint32
	Th32ParentProcessID uint32
	PcPriClassBase      int32
	DwFlags             uint32
	SzExeFile           [260]uint16
}

// RGB helper returns a Win32 COLORREF (0x00BBGGRR).
func RGB(r, g, b byte) uint32 {
	return uint32(r) | (uint32(g) << 8) | (uint32(b) << 16)
}

func StringToUTF16Ptr(s string) *uint16 {
	p, _ := syscall.UTF16PtrFromString(s)
	return p
}

// DrawText wraps DrawTextW with automatic length and conversion.
func DrawText(hdc uintptr, text string, rc *RECT, flags uint32) {
	scaled := scaleRect(*rc)
	pDrawTextW.Call(
		hdc,
		uintptr(unsafe.Pointer(StringToUTF16Ptr(text))),
		^uintptr(0), // -1 in uintptr
		uintptr(unsafe.Pointer(&scaled)),
		uintptr(flags),
	)
}
