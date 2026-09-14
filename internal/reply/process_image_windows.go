//go:build windows

package reply

import (
	"syscall"
	"unsafe"
)

const processImageBufferUTF16 = 32768

// processImageName 返回 PID 对应进程的完整映像路径。
// 第二个返回值表示进程是否存在；存在但读不到映像名时返回空字符串和 true。
func processImageName(pid int) (string, bool) {
	if pid <= 0 {
		return "", false
	}
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	openProcess := kernel32.NewProc("OpenProcess")
	queryImage := kernel32.NewProc("QueryFullProcessImageNameW")
	closeHandle := kernel32.NewProc("CloseHandle")

	handle, _, openErr := openProcess.Call(processQueryLimitedInfo, 0, uintptr(uint32(pid)))
	if handle == 0 {
		if errno, ok := openErr.(syscall.Errno); ok && errno == syscall.ERROR_ACCESS_DENIED {
			return "", true
		}
		return "", false
	}
	defer closeHandle.Call(handle)

	buffer := make([]uint16, processImageBufferUTF16)
	size := uint32(len(buffer))
	result, _, _ := queryImage.Call(
		handle,
		0,
		uintptr(unsafe.Pointer(&buffer[0])),
		uintptr(unsafe.Pointer(&size)),
	)
	if result == 0 || size == 0 {
		return "", true
	}
	return syscall.UTF16ToString(buffer[:size]), true
}
