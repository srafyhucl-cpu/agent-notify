//go:build !windows

package reply

import (
	"errors"
	"fmt"
	"os"
	"syscall"
)

// tryLockFile 以非阻塞方式尝试加独占锁；锁被占用时返回 EWOULDBLOCK/EAGAIN。
func tryLockFile(file *os.File) error {
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		return fmt.Errorf("reply: lock %s: %w", file.Name(), err)
	}
	return nil
}

// isFileLockBusy 判断错误是否为「锁被其他持有者占用」，只有这种情况才值得重试。
func isFileLockBusy(err error) bool {
	return errors.Is(err, syscall.EWOULDBLOCK) || errors.Is(err, syscall.EAGAIN)
}

func unlockFile(file *os.File) {
	_ = syscall.Flock(int(file.Fd()), syscall.LOCK_UN)
}
