//go:build windows

package ui

import (
	"strings"
	"syscall"
	"unsafe"
)

// ProcessStatus checks running status of the 3 agents.
type ProcessStatus struct {
	OpenCodeRunning    bool
	CodexRunning       bool
	AntigravityRunning bool
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
		name := syscall.UTF16ToString(entry.SzExeFile[:])
		nameLower := strings.ToLower(name)

		if strings.Contains(nameLower, "opencode") {
			status.OpenCodeRunning = true
		}
		if strings.Contains(nameLower, "codex") && !strings.Contains(nameLower, "codex-plus-plus") {
			status.CodexRunning = true
		}
		if strings.Contains(nameLower, "antigravity") || strings.Contains(nameLower, "language_server") {
			status.AntigravityRunning = true
		}

		ret, _, _ = pProcess32NextW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
		if ret == 0 {
			break
		}
	}

	return status
}

// KillOtherLinkWeixinInstances terminates any zombie linkweixin.exe processes except the current PID.
func KillOtherLinkWeixinInstances() {
	curPid := uint32(syscall.Getpid())
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
		if strings.EqualFold(name, "linkweixin.exe") && entry.Th32ProcessID != curPid {
			hProc, _, _ := pOpenProcess.Call(0x0001, 0, uintptr(entry.Th32ProcessID)) // PROCESS_TERMINATE
			if hProc != 0 {
				pTerminateProcess.Call(hProc, 0)
				pCloseHandle.Call(hProc)
			}
		}

		ret, _, _ = pProcess32NextW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
		if ret == 0 {
			break
		}
	}
}
