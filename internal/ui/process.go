//go:build windows

package ui

import (
	"os"
	"path/filepath"
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
	CommandCodeRunning bool
}

const (
	processTerminateAccess         = 0x0001 // PROCESS_TERMINATE
	processQueryLimitedInformation = 0x1000 // PROCESS_QUERY_LIMITED_INFORMATION
)

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
		if strings.Contains(nameLower, "commandcode") || strings.Contains(nameLower, "command-code") {
			status.CommandCodeRunning = true
		}
		ret, _, _ = pProcess32NextW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
		if ret == 0 {
			break
		}
	}
	return status
}

// KillOtherAgentNotifyInstances terminates stale GUI instances of this install
// directory except the current PID. Other agent-notify processes (CLI hooks run
// from a different location) must not be touched.
func KillOtherAgentNotifyInstances() {
	currentPID := uint32(syscall.Getpid())
	self, err := os.Executable()
	if err != nil {
		return
	}
	selfDir := filepath.Dir(self)
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

	for {
		name := syscall.UTF16ToString(entry.SzExeFile[:])
		if strings.EqualFold(name, "agent-notify.exe") && entry.Th32ProcessID != currentPID {
			// 只结束同一安装目录下的实例，避免误杀用户正在运行的 CLI/hook 进程。
			if sameProgramDir(selfDir, processImagePath(entry.Th32ProcessID)) {
				hProcess, _, _ := pOpenProcess.Call(processTerminateAccess, 0, uintptr(entry.Th32ProcessID))
				if hProcess != 0 {
					pTerminateProcess.Call(hProcess, 0)
					pCloseHandle.Call(hProcess)
				}
			}
		}
		ret, _, _ = pProcess32NextW.Call(hSnap, uintptr(unsafe.Pointer(&entry)))
		if ret == 0 {
			return
		}
	}
}

// sameProgramDir 判断两个可执行文件是否位于同一目录（大小写不敏感，忽略结尾分隔符）。
func sameProgramDir(selfDir, otherPath string) bool {
	if strings.TrimSpace(otherPath) == "" {
		return false
	}
	return strings.EqualFold(strings.TrimRight(selfDir, `\/`), strings.TrimRight(filepath.Dir(otherPath), `\/`))
}

// processImagePath 返回进程可执行文件的完整路径；权限不足或进程已退出时返回空串。
func processImagePath(pid uint32) string {
	hProcess, _, _ := pOpenProcess.Call(processQueryLimitedInformation, 0, uintptr(pid))
	if hProcess == 0 {
		return ""
	}
	defer pCloseHandle.Call(hProcess)

	buffer := make([]uint16, syscall.MAX_LONG_PATH)
	size := uint32(len(buffer))
	if result, _, _ := pQueryFullProcessImageNameW.Call(hProcess, 0, uintptr(unsafe.Pointer(&buffer[0])), uintptr(unsafe.Pointer(&size))); result == 0 {
		return ""
	}
	return syscall.UTF16ToString(buffer[:size])
}
