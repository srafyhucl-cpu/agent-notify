package reply

import (
	"fmt"
	"os"
	"path/filepath"
	"time"
)

const (
	// lockAcquireTimeoutDefault 是获取本地状态文件锁的默认最长等待时间。
	lockAcquireTimeoutDefault = 5 * time.Second
	// lockRetryInterval 是锁被占用时的重试间隔。
	lockRetryInterval = 20 * time.Millisecond
)

// lockAcquireTimeout 定义为变量，便于测试缩短等待时间；生产默认 5 秒。
var lockAcquireTimeout = lockAcquireTimeoutDefault

func withFileLock(path string, action func() error) error {
	if err := os.MkdirAll(filepath.Dir(path), privateDirPerm); err != nil {
		return err
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, privateFilePerm)
	if err != nil {
		return err
	}
	defer file.Close()
	_ = file.Chmod(privateFilePerm)
	if err := acquireFileLock(file); err != nil {
		return err
	}
	defer unlockFile(file)
	return action()
}

// acquireFileLock 有界等待文件锁：锁被占用时按 lockRetryInterval 重试，
// 超过 lockAcquireTimeout 返回用户可读的超时错误；其他错误立即返回，不重试掩盖。
func acquireFileLock(file *os.File) error {
	deadline := time.Now().Add(lockAcquireTimeout)
	for {
		err := tryLockFile(file)
		if err == nil {
			return nil
		}
		if !isFileLockBusy(err) {
			return err
		}
		if !time.Now().Before(deadline) {
			return fmt.Errorf(
				"reply: 等待本地状态文件锁超时（%s）：%s，可能有另一个 agent-notify 进程正在处理，请稍后重试",
				lockAcquireTimeout,
				file.Name(),
			)
		}
		time.Sleep(lockRetryInterval)
	}
}
