//go:build windows

package reply

import (
	"fmt"
	"os"
	"syscall"
	"unsafe"
)

const lockFileExclusiveLock = 0x00000002

var (
	kernel32         = syscall.NewLazyDLL("kernel32.dll")
	procLockFileEx   = kernel32.NewProc("LockFileEx")
	procUnlockFileEx = kernel32.NewProc("UnlockFileEx")
	allLockFileBytes = ^uint32(0)
)

func lockFile(file *os.File) error {
	var overlapped syscall.Overlapped
	result, _, callErr := procLockFileEx.Call(
		file.Fd(),
		lockFileExclusiveLock,
		0,
		uintptr(allLockFileBytes),
		uintptr(allLockFileBytes),
		uintptr(unsafe.Pointer(&overlapped)),
	)
	if result == 0 {
		return fmt.Errorf("reply: lock %s: %w", file.Name(), callErr)
	}
	return nil
}

func unlockFile(file *os.File) {
	var overlapped syscall.Overlapped
	_, _, _ = procUnlockFileEx.Call(
		file.Fd(),
		0,
		uintptr(allLockFileBytes),
		uintptr(allLockFileBytes),
		uintptr(unsafe.Pointer(&overlapped)),
	)
}
