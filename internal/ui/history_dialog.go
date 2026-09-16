//go:build windows

package ui

import (
	"strings"
	"syscall"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// SetClipboardText copies text to the Windows system clipboard.
func SetClipboardText(text string) {
	utf16, err := syscall.UTF16FromString(text)
	if err != nil || len(utf16) == 0 {
		return
	}
	pOpenClipboard.Call(0)
	defer pCloseClipboard.Call()
	pEmptyClipboard.Call()

	const gmemMoveable = 0x0002
	const cfUnicodeText = 13
	size := len(utf16) * 2
	hGlobal, _, _ := kernel32.NewProc("GlobalAlloc").Call(gmemMoveable, uintptr(size))
	if hGlobal == 0 {
		return
	}
	pointer, _, _ := kernel32.NewProc("GlobalLock").Call(hGlobal)
	if pointer != 0 {
		kernel32.NewProc("RtlMoveMemory").Call(pointer, uintptr(unsafe.Pointer(&utf16[0])), uintptr(size))
		kernel32.NewProc("GlobalUnlock").Call(hGlobal)
		pSetClipboardData.Call(cfUnicodeText, hGlobal)
	}
}

func historyTime(item notify.HistoryItem) string {
	if timestamp := item.LocalTime(); !timestamp.IsZero() {
		return timestamp.Format("01-02 15:04")
	}
	return truncateUI(item.Timestamp, 16)
}

func historyAgent(item notify.HistoryItem) string {
	if item.Agent == "test" {
		return "测试"
	}
	if descriptor, ok := agentmeta.Lookup(item.Agent); ok {
		return descriptor.DisplayName
	}
	if strings.TrimSpace(item.Agent) == "" {
		return "通用"
	}
	return item.Agent
}
