//go:build windows

package sysproc

import (
	"os/exec"
	"syscall"
)

// createNoWindow prevents a console window from flashing when a GUI process
// spawns a console helper.
const createNoWindow = 0x08000000

// ConfigureHidden 让命令在无控制台窗口的情况下运行。
func ConfigureHidden(command *exec.Cmd) {
	command.SysProcAttr = &syscall.SysProcAttr{
		HideWindow:    true,
		CreationFlags: createNoWindow,
	}
}
