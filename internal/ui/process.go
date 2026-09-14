//go:build windows

package ui

import (
	"strings"
	"syscall"
	"unsafe"
)

// ProcessStatus tracks the supported local agents.
type ProcessStatus struct {
	OpenCodeRunning    bool
	CodexRunning       bool
	AntigravityRunning bool
	DevinRunning       bool
}

// DetectProcesses scans running processes using CreateToolhelp32Snapshot.
func DetectProcesses() ProcessStatus {
	const TH32CS_SNAPPROCESS = 0x00000002
	hSnap, _, _ := pCreateToolhelp32Snapshot.Call(TH32CS_SNAPPROCESS, 0)
	if hSnap == 0 || hSnap == ^uintptr(0) {
		return ProcessStatus{}
	}
	defer pCloseHandle.Call(hSnap)

	var entry PROCESSENTRY32W
	entry.DwSize = uint32(unsafe.Sizeof(entry))
	ret, _, _ := pProcess32FirstW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
	if ret == 0 {
		return ProcessStatus{}
	}

	var status ProcessStatus
	for {
		nameLower := strings.ToLower(syscall.UTF16ToString(entry.SzExeFile[:]))
		if strings.Contains(nameLower, "opencode") {
			status.OpenCodeRunning = true
		}
		if strings.Contains(nameLower, "codex") && !strings.Contains(nameLower, "codex-plus-plus") {
			status.CodexRunning = true
		}
		if strings.Contains(nameLower, "antigravity") {
			status.AntigravityRunning = true
		}
		if strings.Contains(nameLower, "devin") {
			status.DevinRunning = true
		}
		ret, _, _ = pProcess32NextW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
		if ret == 0 {
			break
		}
	}
	return status
}

// KillOtherAgentNotifyInstances terminates stale GUI instances except the current PID.
func KillOtherAgentNotifyInstances() {
	currentPID := uint32(syscall.Getpid())
	const TH32CS_SNAPPROCESS = 0x00000002
	hSnap, _, _ := pCreateToolhelp32Snapshot.Call(TH32CS_SNAPPROCESS, 0)
	if hSnap == 0 || hSnap == ^uintptr(0) {
		return
	}
	defer pCloseHandle.Call(hSnap)

	var entry PROCESSENTRY32W
	entry.DwSize = uint32(unsafe.Sizeof(entry))
	ret, _, _ := pProcess32FirstW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
	if ret == 0 {
		return
	}

	pOpenProcess := kernel32.NewProc("OpenProcess")
	pTerminateProcess := kernel32.NewProc("TerminateProcess")
	for {
		name := syscall.UTF16ToString(entry.SzExeFile[:])
		if strings.EqualFold(name, "agent-notify.exe") && entry.Th32ProcessID != currentPID {
			hProcess, _, _ := pOpenProcess.Call(0x0001, 0, uintptr(entry.Th32ProcessID))
			if hProcess != 0 {
				pTerminateProcess.Call(hProcess, 0)
				pCloseHandle.Call(hProcess)
			}
		}
		ret, _, _ = pProcess32NextW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
		if ret == 0 {
			break
		}
	}
}
