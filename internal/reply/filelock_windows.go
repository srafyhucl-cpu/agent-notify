//go:build windows

package reply

import (
	"errors"
	"fmt"
	"os"
	"syscall"
	"unsafe"
)

const (
	lockFileExclusiveLock   = 0x00000002
	lockFileFailImmediately = 0x00000001
)

// errorLockViolation（Win32 ERROR_LOCK_VIOLATION，33）表示锁已被其他持有者占用。
// Go 1.26 的 syscall 包未导出该常量，因此在此显式声明。
const errorLockViolation = syscall.Errno(33)

var (
	kernel32         = syscall.NewLazyDLL("kernel32.dll")
	procLockFileEx   = kernel32.NewProc("LockFileEx")
	procUnlockFileEx = kernel32.NewProc("UnlockFileEx")
	allLockFileBytes = ^uint32(0)
)

// tryLockFile 以非阻塞方式尝试加独占锁；锁被占用时返回 ERROR_LOCK_VIOLATION。
func tryLockFile(file *os.File) error {
	var overlapped syscall.Overlapped
	result, _, callErr := procLockFileEx.Call(
		file.Fd(),
		lockFileExclusiveLock|lockFileFailImmediately,
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

// isFileLockBusy 判断错误是否为「锁被其他持有者占用」，只有这种情况才值得重试。
func isFileLockBusy(err error) bool {
	return errors.Is(err, errorLockViolation)
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
