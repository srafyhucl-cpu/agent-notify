//go:build !windows

package reply

import "syscall"

// processImageName 在非 Windows 平台上只能判断进程是否存在，
// 因此返回空映像名；调用方会把“存在但未知”按占用处理。
func processImageName(pid int) (string, bool) {
	if pid <= 0 {
		return "", false
	}
	if err := syscall.Kill(pid, 0); err != nil {
		return "", err == syscall.EPERM
	}
	return "", true
}
