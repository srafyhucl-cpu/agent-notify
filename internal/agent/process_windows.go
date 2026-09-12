//go:build windows

package agent

import (
	"os/exec"
	"syscall"
)

// createNoWindow prevents a console window from flashing when the notifier
// spawns the upstream Codex helper from a GUI process.
const createNoWindow = 0x08000000

func configureHiddenProcess(command *exec.Cmd) {
	command.SysProcAttr = &syscall.SysProcAttr{
		HideWindow:    true,
		CreationFlags: createNoWindow,
	}
}
